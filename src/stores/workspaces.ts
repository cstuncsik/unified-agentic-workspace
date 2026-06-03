import { computed, ref } from "vue";
import { defineStore } from "pinia";
import type { Workspace } from "../types/workspace";
import * as api from "../api/workspaces";

export const useWorkspacesStore = defineStore("workspaces", () => {
  const list = ref<Workspace[]>([]);
  const currentId = ref<string | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const current = computed(() => list.value.find((w) => w.id === currentId.value) ?? null);

  /** Load all workspaces; create a default one on first launch when none exist. */
  async function load() {
    loading.value = true;
    error.value = null;
    try {
      list.value = await api.listWorkspaces();
      if (list.value.length === 0) {
        list.value = [await api.createWorkspace("Default")];
      }
      if (!currentId.value || !list.value.some((w) => w.id === currentId.value)) {
        currentId.value = list.value[0]?.id ?? null;
      }
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  async function create(name: string, kind?: string) {
    const ws = await api.createWorkspace(name, kind);
    list.value.push(ws);
    currentId.value = ws.id;
    return ws;
  }

  function select(id: string) {
    currentId.value = id;
  }

  async function rename(id: string, name: string) {
    const ws = await api.updateWorkspace(id, name);
    if (ws) {
      const i = list.value.findIndex((w) => w.id === id);
      if (i >= 0) list.value[i] = ws;
    }
  }

  async function remove(id: string) {
    await api.deleteWorkspace(id);
    list.value = list.value.filter((w) => w.id !== id);
    if (currentId.value === id) {
      currentId.value = list.value[0]?.id ?? null;
    }
  }

  return { list, currentId, current, loading, error, load, create, select, rename, remove };
});
