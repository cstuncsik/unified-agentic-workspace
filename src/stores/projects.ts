import { ref } from "vue";
import { defineStore } from "pinia";
import type { Project, ProjectMode } from "../types/project";
import * as api from "../api/projects";

export const useProjectsStore = defineStore("projects", () => {
  const list = ref<Project[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Monotonic token so a slow response for a previous workspace can never
  // overwrite the list after the user has already switched workspaces.
  let loadToken = 0;

  /** Load all projects for a workspace; clears the list first so stale rows never leak across workspaces. */
  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    list.value = [];
    try {
      const rows = await api.listProjects(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function create(workspaceId: string, name: string, mode: ProjectMode) {
    const project = await api.createProject(workspaceId, name, mode);
    list.value.push(project);
    return project;
  }

  async function rename(id: string, name: string) {
    const project = await api.updateProject(id, name);
    if (project) {
      const i = list.value.findIndex((p) => p.id === id);
      if (i >= 0) list.value[i] = project;
    }
  }

  async function remove(id: string) {
    await api.deleteProject(id);
    list.value = list.value.filter((p) => p.id !== id);
  }

  return { list, loading, error, load, create, rename, remove };
});
