import { ref } from "vue";
import { defineStore } from "pinia";
import { listen } from "@tauri-apps/api/event";
import type { AgentAdapter, AgentSession, SdkEvent } from "../types/agentSession";
import * as api from "../api/agentSessions";

/** An open terminal tab: a started session plus its current status. */
export interface OpenTab {
  session: AgentSession;
}

export const useAgentSessionsStore = defineStore("agentSessions", () => {
  const adapters = ref<AgentAdapter[]>([]);
  const tabs = ref<OpenTab[]>([]);
  const activeId = ref<string | null>(null);
  const error = ref<string | null>(null);
  /** The last-focused tab per workspace id, so switching back restores it. */
  const lastActiveByWorkspace = ref<Record<string, string | null>>({});
  /** Accumulated Claude Agent SDK events per session id (store-owned so events
   *  that arrive before a view mounts are never lost). */
  const sdkEvents = ref<Record<string, SdkEvent[]>>({});
  let sdkListenerStarted = false;

  function parseSdkLine(line: string): SdkEvent | null {
    try {
      return JSON.parse(line) as SdkEvent;
    } catch {
      return null;
    }
  }

  /** Start the global agent-sdk-event listener once. */
  async function ensureSdkListener() {
    if (sdkListenerStarted) return;
    sdkListenerStarted = true;
    await listen<{ session_id: string; line: string }>("agent-sdk-event", (e) => {
      const ev = parseSdkLine(e.payload.line);
      if (!ev) return;
      const id = e.payload.session_id;
      sdkEvents.value = { ...sdkEvents.value, [id]: [...(sdkEvents.value[id] ?? []), ev] };
    });
  }

  /** Replay a finished/reopened SDK session's transcript once into the accumulator. */
  async function loadSdkTranscript(id: string) {
    if (sdkEvents.value[id]) return; // already have live state
    const lines = await api.getAgentSdkTranscript(id);
    const evs = lines.map(parseSdkLine).filter((x): x is SdkEvent => x !== null);
    sdkEvents.value = { ...sdkEvents.value, [id]: evs };
  }

  async function loadAdapters() {
    void ensureSdkListener();
    try {
      adapters.value = await api.listAgentAdapters();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function start(
    codingWorkspaceId: string,
    adapterId: string,
    accountId: string | null,
    prompt: string | null,
    cols: number,
    rows: number,
  ) {
    await ensureSdkListener();
    const session = await api.startAgentSession(codingWorkspaceId, adapterId, accountId, prompt, cols, rows);
    tabs.value.push({ session });
    activeId.value = session.id;
    return session;
  }

  async function stop(id: string) {
    await api.stopAgentSession(id);
  }

  function setStatus(id: string, status: string, exitCode: number | null) {
    const tab = tabs.value.find((t) => t.session.id === id);
    if (tab) {
      tab.session.status = status;
      tab.session.exit_code = exitCode;
    }
  }

  async function closeTab(id: string) {
    const tab = tabs.value.find((t) => t.session.id === id);
    // Closing a running tab must stop its PTY, else the child + registry handle
    // leak for the app's lifetime (there is no re-attach in M10a).
    if (tab && tab.session.status === "running") {
      try {
        await api.stopAgentSession(id);
      } catch {
        /* already gone */
      }
    }
    tabs.value = tabs.value.filter((t) => t.session.id !== id);
    if (activeId.value === id) {
      activeId.value = tabs.value.length ? tabs.value[tabs.value.length - 1].session.id : null;
    }
  }

  return {
    adapters,
    tabs,
    activeId,
    error,
    lastActiveByWorkspace,
    sdkEvents,
    loadSdkTranscript,
    loadAdapters,
    start,
    stop,
    setStatus,
    closeTab,
  };
});
