#!/usr/bin/env node
// Headless Claude Agent SDK runner. Goal via argv[2], mode via argv[3]
// ("plan" | "edit"); key via env (injected by the backend). Emits one compact
// NDJSON object per message on stdout.
import fs from "node:fs";
import path from "node:path";
import { query } from "@anthropic-ai/claude-agent-sdk";

const goal = process.argv[2] ?? "";
const mode = process.argv[3] === "edit" ? "edit" : "plan";
const model = process.argv[4] ?? "";
const cwd = process.cwd();
// Canonical worktree root, resolved once, for the worktree-write boundary check.
let realCwd;
try {
  realCwd = fs.realpathSync(cwd);
} catch {
  realCwd = path.resolve(cwd);
}
const emit = (o) => process.stdout.write(JSON.stringify(o) + "\n");

// Explicit tool surface: an allowlist (allowedTools) plus a denylist of shell/egress/
// subagent tools (disallowedTools), run under bypassPermissions — no interactive
// prompts, nothing dangerous runs. Edit mode adds Write/Edit (bounded to the worktree
// by the PreToolUse hook below); plan mode is read-only.
const allowedTools =
  mode === "edit" ? ["Read", "Glob", "Grep", "Edit", "Write"] : ["Read", "Glob", "Grep"];

// Bound Write/Edit to the worktree. Canonicalize via realpath so a symlinked
// directory inside the worktree pointing outside cannot be used to escape. The
// target file may not exist yet, so resolve the nearest existing ancestor (a
// not-yet-created tail can't be a symlink) and append the remainder. Any
// resolution failure denies (fail closed).
const withinWorktree = (p) => {
  if (!p) return false;
  const abs = path.resolve(cwd, p);
  let existing = abs;
  while (!fs.existsSync(existing)) {
    const parent = path.dirname(existing);
    if (parent === existing) return false; // reached the root without an existing dir
    existing = parent;
  }
  let realExisting;
  try {
    realExisting = fs.realpathSync(existing);
  } catch {
    return false;
  }
  const tail = path.relative(existing, abs);
  const real = tail ? path.join(realExisting, tail) : realExisting;
  return real === realCwd || real.startsWith(realCwd + path.sep);
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
  // bypassPermissions: skip the interactive permission prompt (meaningless headless).
  // The locked-down surface is the allowlist + this denylist (shell/egress/subagents)
  // + the edit-mode worktree hook — NOT the mode name. "dontAsk" is INVALID in SDK
  // 0.1.0 (modes: default | acceptEdits | bypassPermissions | plan) and was rejected
  // at arg-parse, so the real agent never ran.
  // ("Task" is the subagent-spawn tool's CLI name; its SDK type is AgentInput.)
  permissionMode: "bypassPermissions",
  allowedTools,
  disallowedTools: ["Bash", "BashOutput", "KillShell", "NotebookEdit", "WebFetch", "WebSearch", "Task"],
  settingSources: [],
  maxTurns: 30,
  // Spread our env so the grandchild CLI inherits the injected key, and blank
  // ambient tokens that would otherwise outrank it.
  env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
  ...(model ? { model } : {}),
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
