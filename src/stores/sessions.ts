import { computed, ref } from "vue";
import { defineStore } from "pinia";
import type { Session, SessionStatus } from "../types/session";
import { STATUS_GROUPS } from "../types/session";
import type { CreateSessionInput } from "../api/sessions";
import * as api from "../api/sessions";

export const useSessionsStore = defineStore("sessions", () => {
  const list = ref<Session[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  /** Active sidebar filter; a STATUS_GROUPS key, or null for all sessions. */
  const filterGroup = ref<string | null>(null);

  const grouped = computed(() =>
    STATUS_GROUPS.map((group) => ({
      ...group,
      sessions: list.value.filter((s) => group.statuses.includes(s.status)),
    })),
  );

  const visibleGroups = computed(() =>
    grouped.value.filter((group) =>
      filterGroup.value ? group.key === filterGroup.value : group.sessions.length > 0,
    ),
  );

  // Monotonic token so a slow response for a previous workspace can never
  // overwrite the list after the user has already switched workspaces.
  let loadToken = 0;

  /** Load all sessions for a workspace; clears the list first so stale rows never leak across workspaces. */
  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    list.value = [];
    try {
      const rows = await api.listSessions(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function create(input: CreateSessionInput) {
    const session = await api.createSession(input);
    list.value.unshift(session);
    return session;
  }

  async function setStatus(id: string, status: SessionStatus) {
    const session = await api.updateSessionStatus(id, status);
    if (session) {
      const i = list.value.findIndex((s) => s.id === id);
      if (i >= 0) list.value[i] = session;
    }
  }

  async function remove(id: string) {
    await api.deleteSession(id);
    list.value = list.value.filter((s) => s.id !== id);
  }

  /** Mirror the backend's ON DELETE SET NULL when a project is deleted. */
  function detachProject(projectId: string) {
    list.value = list.value.map((s) =>
      s.project_id === projectId ? { ...s, project_id: null } : s,
    );
  }

  function setFilter(key: string | null) {
    filterGroup.value = key;
  }

  return {
    list,
    loading,
    error,
    filterGroup,
    grouped,
    visibleGroups,
    load,
    create,
    setStatus,
    remove,
    detachProject,
    setFilter,
  };
});
