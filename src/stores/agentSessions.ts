import { ref } from "vue";
import { defineStore } from "pinia";
import type { AgentAdapter, AgentSession } from "../types/agentSession";
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

  async function loadAdapters() {
    try {
      adapters.value = await api.listAgentAdapters();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function start(codingWorkspaceId: string, adapterId: string, cols: number, rows: number) {
    const session = await api.startAgentSession(codingWorkspaceId, adapterId, cols, rows);
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
    loadAdapters,
    start,
    stop,
    setStatus,
    closeTab,
  };
});
