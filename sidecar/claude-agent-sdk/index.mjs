#!/usr/bin/env node
// Headless Claude Agent SDK runner. Goal via argv[2]; key via env (injected by the
// backend). Emits one compact NDJSON object per message on stdout. Plan-only.
import { query } from "@anthropic-ai/claude-agent-sdk";

const goal = process.argv[2] ?? "";
const emit = (o) => process.stdout.write(JSON.stringify(o) + "\n");

try {
  for await (const m of query({
    prompt: goal,
    options: {
      cwd: process.cwd(),
      permissionMode: "plan",
      settingSources: [],
      maxTurns: 30,
      // Spread our env so the grandchild CLI inherits the injected key, and blank
      // ambient tokens that would otherwise outrank it.
      env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
    },
  })) {
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
