#!/usr/bin/env bash
# Run the WebdriverIO e2e suite against the real Tauri app.
#
# We manage Xvfb directly instead of using `xvfb-run`: xvfb-run waits for every
# process using the display to exit, but WebdriverIO's forked workers keep the
# display open, so `xvfb-run pnpm e2e` deadlocks forever. Starting Xvfb in the
# background and exporting DISPLAY avoids that.
set -euo pipefail

Xvfb :99 -screen 0 1280x1024x24 >/tmp/xvfb.log 2>&1 &
xvfb_pid=$!
trap 'kill "$xvfb_pid" 2>/dev/null || true' EXIT
export DISPLAY=:99

# Wait for the X11 socket before launching the app.
for _ in $(seq 1 40); do
  [ -e /tmp/.X11-unix/X99 ] && break
  sleep 0.25
done

# Fixture git repo the Sources/Coding specs use. Created here (not in the Docker
# image) so it exists in BOTH lanes: the Docker mirror and the native CI runner.
# Always recreated fresh so a leftover branch (the coding spec adds one and keeps
# it on discard) can't collide on a re-run in the same environment. Per-command
# identity, so it never touches global git config.
rm -rf /tmp/fixture-repo
git init -b main /tmp/fixture-repo >/dev/null
echo "# fixture" >/tmp/fixture-repo/README.md
git -C /tmp/fixture-repo add . >/dev/null
git -C /tmp/fixture-repo -c user.email=e2e@uaw.local -c user.name="UAW E2E" \
  commit -m init >/dev/null

# Isolated fixture repo for the dispatch spec.
rm -rf /tmp/fixture-repo-dispatch
git init -b main /tmp/fixture-repo-dispatch >/dev/null
echo "# dispatch fixture" >/tmp/fixture-repo-dispatch/README.md
git -C /tmp/fixture-repo-dispatch add . >/dev/null
git -C /tmp/fixture-repo-dispatch -c user.email=e2e@uaw.local -c user.name="UAW E2E" \
  commit -m init >/dev/null

# Isolated fixture repo for the board spec (its own branches, no cross-spec collisions).
rm -rf /tmp/fixture-repo-board
git init -b main /tmp/fixture-repo-board >/dev/null
echo "# board fixture" >/tmp/fixture-repo-board/README.md
git -C /tmp/fixture-repo-board add . >/dev/null
git -C /tmp/fixture-repo-board -c user.email=e2e@uaw.local -c user.name="UAW E2E" \
  commit -m init >/dev/null

# A fake interactive "agent CLI" for the agent-terminal e2e: prints a banner then
# echoes stdin, so the PTY/xterm round-trip can be asserted without a real claude.
# It also reports (boolean only, never the value) whether a provider API key was
# injected into its env, so the account-injection e2e can prove key injection.
cat >/tmp/uaw-fake-agent <<'AGENT'
#!/usr/bin/env bash
if [ -n "${ANTHROPIC_API_KEY:-}" ] || [ -n "${OPENAI_API_KEY:-}" ]; then
  printf 'KEY:set\n'
else
  printf 'KEY:unset\n'
fi
printf 'AGENT-READY\n'
exec cat
AGENT
chmod +x /tmp/uaw-fake-agent

# A fake Claude Agent SDK sidecar for the SDK e2e, run via `node` (the backend now
# invokes the sidecar as `node <script> <goal> <mode> <model>`). goal=argv[2],
# mode=argv[3] (default "plan"), model=argv[4]. Emits canned NDJSON incl. a deliberate
# $ANTHROPIC_API_KEY echo (to prove the backend redacts it), a non-JSON garbage line
# (to prove no-crash), a KEY:set/unset marker, then a result. In edit mode, writes an
# untracked file into the worktree (cwd) to simulate a real agent edit. No shebang /
# exec bit needed — node runs it as a file argument. (The backend invokes it with its
# default `node`: UAW_AGENT_NODE is intentionally unset in wdio.conf.ts, so node_bin
# falls back to `node` on PATH — the same path production uses.)
cat >/tmp/uaw-fake-sdk <<'SDK'
const goal = process.argv[2] ?? "";
const mode = process.argv[3] === "edit" ? "edit" : "plan";
const model = process.argv[4] ?? "";
const key = process.env.ANTHROPIC_API_KEY ?? "";
const km = key ? "KEY:set" : "KEY:unset";
// cwd is the worktree (the backend sets current_dir); relative path never escapes it.
if (mode === "edit") require("fs").writeFileSync("AGENT_EDIT.md", "edited by fake sdk\n");
const w = (o) => process.stdout.write(JSON.stringify(o) + "\n");
w({ type: "assistant", text: "Planning: " + goal.replace(/"/g, "") });
w({ type: "tool", name: "Read", summary: "README.md" });
w({ type: "tool", name: "echo", summary: key || "none" });
process.stdout.write("this line is not json\n");
w({ type: "tool", name: "probe", summary: km });
w({ type: "tool", name: "model-probe", summary: "MODEL:" + model });
w({ type: "result", status: "success", summary: "Done" });
SDK

# Fake model-list helper, run via `node`: emits canned /v1/models JSON (the shape
# parse_models accepts) and nothing else. No network, no auth, no shebang/exec bit.
cat >/tmp/uaw-fake-list-models <<'MODELS'
process.stdout.write(JSON.stringify({ data: [
  { id: "claude-opus-4-5", display_name: "Claude Opus 4.5" },
  { id: "claude-sonnet-4-5", display_name: "Claude Sonnet 4.5" },
] }) + "\n");
MODELS

# Fail fast on a syntax error in the REAL sidecar scripts. The e2e runs the FAKE
# sidecars above, so this cheap `node --check` is the only place the real ones get
# exercised in the Docker run (the sdk-sidecar.yml CI job adds the option-parse smoke).
node --check sidecar/claude-agent-sdk/index.mjs
node --check sidecar/claude-agent-sdk/list-models.mjs

# Invoke the wdio binary directly (skips pnpm's pre-run deps check). Not exec'd
# so the EXIT trap still runs to stop Xvfb; set -e propagates wdio's exit code.
node_modules/.bin/wdio run wdio.conf.ts
