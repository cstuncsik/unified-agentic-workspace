import { ref } from "vue";
import { defineStore } from "pinia";
import type { GitInspection, RepositorySource } from "../types/repository";
import type { CreateRepositoryInput } from "../api/repositories";
import * as api from "../api/repositories";

export const useRepositoriesStore = defineStore("repositories", () => {
  const list = ref<RepositorySource[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  /** Live git status per repository id, populated lazily. */
  const statuses = ref<Record<string, GitInspection>>({});

  // Monotonic token so a slow response for a previous workspace can never
  // overwrite the list after the user has switched workspaces.
  let loadToken = 0;

  /** Load all repositories for a workspace; clears first so rows never leak across workspaces. */
  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    list.value = [];
    statuses.value = {};
    try {
      const rows = await api.listRepositorySources(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
      await Promise.all(rows.map((r) => refreshStatus(r.id)));
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function refreshStatus(id: string) {
    try {
      statuses.value = { ...statuses.value, [id]: await api.getRepositoryStatus(id) };
    } catch (e) {
      // Surface the failure rather than leaving the row stuck on "checking…".
      statuses.value = {
        ...statuses.value,
        [id]: {
          is_git_repo: false,
          current_branch: null,
          default_branch: null,
          is_dirty: false,
          toplevel: null,
          error: String(e),
        },
      };
    }
  }

  function validate(path: string) {
    return api.validateRepositoryPath(path);
  }

  async function create(input: CreateRepositoryInput) {
    const repo = await api.createRepositorySource(input);
    list.value.push(repo);
    await refreshStatus(repo.id);
    return repo;
  }

  async function remove(id: string) {
    await api.deleteRepositorySource(id);
    list.value = list.value.filter((r) => r.id !== id);
    const next = { ...statuses.value };
    delete next[id];
    statuses.value = next;
  }

  return { list, loading, error, statuses, load, refreshStatus, validate, create, remove };
});
