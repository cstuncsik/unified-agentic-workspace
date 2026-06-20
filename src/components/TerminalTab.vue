<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import * as api from "../api/agentSessions";
import type { AgentOutput } from "../types/agentSession";

const props = defineProps<{ sessionId: string }>();

const host = ref<HTMLDivElement | null>(null);
let term: Terminal | null = null;
let fit: FitAddon | null = null;
let resizeObserver: ResizeObserver | null = null;
const unlisten: UnlistenFn[] = [];

// Fit the terminal to its container and push the size to the PTY. Guarded so we
// never fit while the element is hidden (a tab not in front) or before layout —
// a 0-height fit would collapse the terminal to a sliver, leaving its output
// stuck in scrollback. Growing the rows later reflows those lines back into view.
function doFit() {
  if (!fit || !term || !host.value) return;
  if (host.value.clientHeight < 1 || host.value.clientWidth < 1) return;
  try {
    fit.fit();
  } catch {
    return;
  }
  api.resizeAgentSession(props.sessionId, term.cols, term.rows).catch(() => {});
}

onMounted(async () => {
  term = new Terminal({ convertEol: false, cursorBlink: true, fontSize: 13 });
  fit = new FitAddon();
  term.loadAddon(fit);
  if (host.value) term.open(host.value);

  // Replay any existing transcript (reopened/finished session), then go live.
  try {
    const transcript = await api.getAgentSessionTranscript(props.sessionId);
    if (transcript) term.write(transcript);
  } catch {
    /* a brand-new session has no transcript yet */
  }

  // User keystrokes → PTY.
  term.onData((data) => {
    api.writeAgentSession(props.sessionId, data).catch(() => {});
  });

  // Live PTY output, routed by session id. (Exit/status is handled globally in the
  // store so it works for SDK sessions too, which never mount this component.)
  unlisten.push(
    await listen<AgentOutput>("agent-output", (e) => {
      if (e.payload.session_id === props.sessionId && term) {
        term.write(new Uint8Array(e.payload.bytes));
      }
    }),
  );

  // Keep the PTY size in sync with the container (fires on show/hide + resize).
  resizeObserver = new ResizeObserver(() => doFit());
  if (host.value) resizeObserver.observe(host.value);

  // Fit once layout has settled (next frame), and again once the monospace font
  // metrics are known, so the terminal fills its container from the start.
  requestAnimationFrame(() => doFit());
  if (typeof document !== "undefined" && document.fonts?.ready) {
    document.fonts.ready.then(() => doFit());
  }
});

onBeforeUnmount(() => {
  unlisten.forEach((u) => u());
  resizeObserver?.disconnect();
  term?.dispose();
});
</script>

<template>
  <div ref="host" class="terminal" data-testid="agent-terminal"></div>
</template>

<style scoped>
.terminal {
  width: 100%;
  /* Fill the flex height the Agents pane gives us; min-height:0 lets the box
     shrink so xterm scrolls internally instead of pushing the page taller. */
  flex: 1;
  min-height: 0;
  background: #000;
  padding: 0.25rem;
  border-radius: var(--re-radius-md, 6px);
  overflow: hidden;
}

/* xterm injects .xterm-viewport with its own scrollbar (shown for inline CLIs
   like Codex that use the normal buffer + scrollback, unlike Claude Code's
   alt-screen). Theme it thin + dark via :deep so it blends into the terminal
   instead of reading like a separate page scrollbar. */
.terminal :deep(.xterm-viewport) {
  scrollbar-width: thin;
  scrollbar-color: rgba(255, 255, 255, 0.25) transparent;
}
.terminal :deep(.xterm-viewport)::-webkit-scrollbar {
  width: 10px;
}
.terminal :deep(.xterm-viewport)::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
  border-radius: 5px;
}
.terminal :deep(.xterm-viewport)::-webkit-scrollbar-track {
  background: transparent;
}
</style>
