import { ref } from "vue";
import { defineStore } from "pinia";
import type { Artifact } from "../types/artifact";
import * as api from "../api/artifacts";

export const useArtifactsStore = defineStore("artifacts", () => {
  const list = ref<Artifact[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Monotonic token so a slow response for a previous workspace can never
  // overwrite the list after the user has switched workspaces.
  let loadToken = 0;

  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    list.value = [];
    try {
      const rows = await api.listArtifacts(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function create(workspaceId: string, projectId: string | null, title: string) {
    const token = loadToken;
    const artifact = await api.createArtifact(workspaceId, projectId, title);
    if (token !== loadToken) return artifact;
    list.value.unshift(artifact);
    return artifact;
  }

  async function update(id: string, title: string, content: string) {
    const updated = await api.updateArtifact(id, title, content);
    if (updated) {
      const i = list.value.findIndex((a) => a.id === id);
      if (i >= 0) list.value[i] = updated;
    }
    return updated;
  }

  async function remove(id: string) {
    await api.deleteArtifact(id);
    list.value = list.value.filter((a) => a.id !== id);
  }

  /** Mirror ON DELETE SET NULL in the live list when a project is deleted. */
  function detachProject(projectId: string) {
    list.value = list.value.map((a) =>
      a.project_id === projectId ? { ...a, project_id: null } : a,
    );
  }

  return { list, loading, error, load, create, update, remove, detachProject };
});
