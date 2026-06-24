import { ref } from "vue";
import { defineStore } from "pinia";
import type { Review } from "../types/review";
import * as api from "../api/reviews";

export const useReviewsStore = defineStore("reviews", () => {
  const list = ref<Review[]>([]);
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
      const rows = await api.listReviews(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function createForCodingWorkspace(codingWorkspaceId: string) {
    const token = loadToken;
    const review = await api.createReviewForCodingWorkspace(codingWorkspaceId);
    // Don't leak the new review into another workspace's list if the workspace
    // changed while the request was in flight.
    if (token !== loadToken) return review;
    list.value.unshift(review);
    return review;
  }

  async function updateStatus(id: string, status: string) {
    const updated = await api.updateReviewStatus(id, status);
    if (updated) {
      const i = list.value.findIndex((r) => r.id === id);
      if (i >= 0) list.value[i] = updated;
    }
    return updated;
  }

  function insert(review: Review) {
    const i = list.value.findIndex((r) => r.id === review.id);
    if (i >= 0) list.value[i] = review;
    else list.value.unshift(review);
  }

  // Per-review "checks are running" flags, for the async recheck after an instant,
  // check-less auto-review. Keyed by review id.
  const rechecking = ref<Record<string, boolean>>({});
  function setRechecking(id: string, value: boolean) {
    rechecking.value = { ...rechecking.value, [id]: value };
  }

  return { list, loading, error, load, createForCodingWorkspace, updateStatus, insert, rechecking, setRechecking };
});
