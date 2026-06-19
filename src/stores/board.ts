import { ref } from "vue";
import { defineStore } from "pinia";
import type { BoardCard } from "../types/board";
import * as api from "../api/board";

export const useBoardStore = defineStore("board", () => {
  const cards = ref<BoardCard[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Monotonic token so a slow board load can't overwrite a newer one (e.g. after
  // a workspace switch or a Refresh).
  let loadToken = 0;

  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    cards.value = [];
    try {
      const rows = await api.getBoard(workspaceId);
      if (token !== loadToken) return;
      cards.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  return { cards, loading, error, load };
});
