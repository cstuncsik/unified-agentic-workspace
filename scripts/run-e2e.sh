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

# Invoke the wdio binary directly (skips pnpm's pre-run deps check). Not exec'd
# so the EXIT trap still runs to stop Xvfb; set -e propagates wdio's exit code.
node_modules/.bin/wdio run wdio.conf.ts
