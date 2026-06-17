<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import * as api from "../api/agentSessions";
import { useAgentSessionsStore } from "../stores/agentSessions";
import type { AgentOutput, AgentExit } from "../types/agentSession";

const props = defineProps<{ sessionId: string }>();
const store = useAgentSessionsStore();

const host = ref<HTMLDivElement | null>(null);
let term: Terminal | null = null;
let fit: FitAddon | null = null;
let resizeObserver: ResizeObserver | null = null;
const unlisten: UnlistenFn[] = [];

onMounted(async () => {
  term = new Terminal({ convertEol: false, cursorBlink: true, fontSize: 13 });
  fit = new FitAddon();
  term.loadAddon(fit);
  if (host.value) term.open(host.value);
  fit.fit();

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

  // Live output + exit, routed by session id.
  unlisten.push(
    await listen<AgentOutput>("agent-output", (e) => {
      if (e.payload.session_id === props.sessionId && term) {
        term.write(new Uint8Array(e.payload.bytes));
      }
    }),
  );
  unlisten.push(
    await listen<AgentExit>("agent-exit", (e) => {
      if (e.payload.session_id === props.sessionId) {
        store.setStatus(props.sessionId, e.payload.status, e.payload.exit_code);
      }
    }),
  );

  // Keep the PTY size in sync with the container.
  resizeObserver = new ResizeObserver(() => {
    if (!fit || !term) return;
    fit.fit();
    api.resizeAgentSession(props.sessionId, term.cols, term.rows).catch(() => {});
  });
  if (host.value) resizeObserver.observe(host.value);
  // Push the initial fitted size to the backend.
  if (term) api.resizeAgentSession(props.sessionId, term.cols, term.rows).catch(() => {});
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
  height: 100%;
  min-height: 24rem;
  background: #000;
  padding: 0.25rem;
  border-radius: var(--re-radius-md, 6px);
}
</style>
