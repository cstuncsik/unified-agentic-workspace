#!/usr/bin/env node
// Headless Claude Agent SDK runner. Goal via argv[2], mode via argv[3]
// ("plan" | "edit"); key via env (injected by the backend). Emits one compact
// NDJSON object per message on stdout.
import path from "node:path";
import { query } from "@anthropic-ai/claude-agent-sdk";

const goal = process.argv[2] ?? "";
const mode = process.argv[3] === "edit" ? "edit" : "plan";
const cwd = process.cwd();
const emit = (o) => process.stdout.write(JSON.stringify(o) + "\n");

// Explicit tool surface: dontAsk + an allowlist denies everything not listed (no
// shell, no egress, no subagents) — the SDK's documented locked-down pattern. Edit
// mode adds Write/Edit; plan mode is read-only.
const allowedTools =
  mode === "edit"
    ? ["Read", "Glob", "Grep", "Edit", "Write"]
    : ["Read", "Glob", "Grep"];

// Bound Write/Edit to the worktree (cwd). dontAsk skips canUseTool, so a PreToolUse
// hook (runs first, can deny) is the mechanism that scopes writes.
const withinWorktree = (p) => {
  if (!p) return false;
  const resolved = path.resolve(cwd, p);
  return resolved === cwd || resolved.startsWith(cwd + path.sep);
};
const boundToWorktree = async (input) => {
  const ti = input.tool_input ?? {};
  if (!withinWorktree(ti.file_path ?? ti.path)) {
    return {
      hookSpecificOutput: {
        hookEventName: input.hook_event_name,
        permissionDecision: "deny",
        permissionDecisionReason: "Edits are restricted to the worktree",
      },
    };
  }
  return {};
};

const options = {
  cwd,
  permissionMode: "dontAsk",
  allowedTools,
  settingSources: [],
  maxTurns: 30,
  // Spread our env so the grandchild CLI inherits the injected key, and blank
  // ambient tokens that would otherwise outrank it.
  env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
  ...(mode === "edit" && {
    hooks: { PreToolUse: [{ matcher: "Write|Edit", hooks: [boundToWorktree] }] },
  }),
};

try {
  for await (const m of query({ prompt: goal, options })) {
    if (m.type === "assistant") {
      for (const block of m.message?.content ?? []) {
        if (block.type === "text" && block.text) {
          emit({ type: "assistant", text: block.text });
        } else if (block.type === "tool_use") {
          emit({
            type: "tool",
            name: block.name,
            summary: JSON.stringify(block.input ?? {}).slice(0, 200),
          });
        }
      }
    } else if (m.type === "result") {
      emit({
        type: "result",
        status: m.subtype === "success" && !m.is_error ? "success" : "error",
        summary: typeof m.result === "string" ? m.result : "",
      });
    }
  }
} catch {
  emit({ type: "error", message: "Agent run failed" });
  process.exit(1);
}
