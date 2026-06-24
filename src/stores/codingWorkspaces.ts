import { ref } from "vue";
import { defineStore } from "pinia";
import type { CodingWorkspace, WorktreeDiff } from "../types/codingWorkspace";
import type { Review } from "../types/review";
import type { CreateCodingWorkspaceInput } from "../api/codingWorkspaces";
import * as api from "../api/codingWorkspaces";

export const useCodingWorkspacesStore = defineStore("codingWorkspaces", () => {
  const list = ref<CodingWorkspace[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  /** Lazily-loaded diff per coding-workspace id. */
  const diffs = ref<Record<string, WorktreeDiff>>({});

  // Monotonic token so a slow response for a previous workspace can never
  // overwrite the list after the user has switched workspaces.
  let loadToken = 0;

  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    list.value = [];
    diffs.value = {};
    try {
      const rows = await api.listCodingWorkspaces(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function create(input: CreateCodingWorkspaceInput) {
    const token = loadToken;
    const cw = await api.createCodingWorkspace(input);
    if (token !== loadToken) return cw;
    list.value.unshift(cw);
    return cw;
  }

  async function refreshDiff(id: string) {
    try {
      diffs.value = { ...diffs.value, [id]: await api.getCodingWorkspaceDiff(id) };
    } catch (e) {
      // Surface the failure so the panel shows an error instead of "Loading…".
      diffs.value = {
        ...diffs.value,
        [id]: {
          changed_files: [],
          diff_stat: "",
          diff_text: "",
          is_clean: false,
          error: String(e),
        },
      };
    }
  }

  async function markReady(id: string) {
    const cw = await api.markCodingWorkspaceReadyForReview(id);
    if (cw) {
      const i = list.value.findIndex((c) => c.id === id);
      if (i >= 0) list.value[i] = cw;
    }
  }

  async function discard(id: string, force: boolean) {
    await api.discardCodingWorkspace(id, force);
    list.value = list.value.filter((c) => c.id !== id);
    const next = { ...diffs.value };
    delete next[id];
    diffs.value = next;
  }

  async function complete(id: string, runCheck = true): Promise<Review> {
    const review = await api.completeCodingWorkspace(id, runCheck);
    // Completion deterministically moves the workspace to needs-review.
    const i = list.value.findIndex((c) => c.id === id);
    if (i >= 0) list.value[i] = { ...list.value[i], status: "needs-review" };
    return review;
  }

  async function recheck(reviewId: string): Promise<Review> {
    return api.recheckCodingWorkspace(reviewId);
  }

  return { list, loading, error, diffs, load, create, refreshDiff, markReady, discard, complete, recheck };
});
