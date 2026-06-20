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

# A fake Claude Agent SDK sidecar for the SDK e2e: goal via argv ($1), mode via
# argv ($2, default "plan"), emits canned NDJSON incl. a deliberate
# $ANTHROPIC_API_KEY echo (to prove the backend redacts it), a non-JSON garbage
# line (to prove no-crash), a KEY:set/unset presence marker, then a result.
# In edit mode, writes an untracked file into the worktree (cwd) to simulate a
# real agent edit. Exits 0 (NOT exec cat — it is a one-shot, not a REPL).
cat >/tmp/uaw-fake-sdk <<'SDK'
#!/usr/bin/env bash
goal="$1"
mode="${2:-plan}"
km=KEY:unset; [ -n "${ANTHROPIC_API_KEY:-}" ] && km=KEY:set
# In edit mode, simulate an agent edit by writing an untracked file into the
# worktree (cwd is the worktree: the backend sets current_dir). Relative path only,
# never escaping the worktree. Plan mode leaves the tree clean.
if [ "$mode" = "edit" ]; then
  printf 'edited by fake sdk\n' > AGENT_EDIT.md
fi
printf '{"type":"assistant","text":"Planning: %s"}\n' "${goal//\"/}"
printf '{"type":"tool","name":"Read","summary":"README.md"}\n'
printf '{"type":"tool","name":"echo","summary":"%s"}\n' "${ANTHROPIC_API_KEY:-none}"
printf 'this line is not json\n'
printf '{"type":"tool","name":"probe","summary":"%s"}\n' "$km"
printf '{"type":"result","status":"success","summary":"Done"}\n'
SDK
chmod +x /tmp/uaw-fake-sdk

# Invoke the wdio binary directly (skips pnpm's pre-run deps check). Not exec'd
# so the EXIT trap still runs to stop Xvfb; set -e propagates wdio's exit code.
node_modules/.bin/wdio run wdio.conf.ts
