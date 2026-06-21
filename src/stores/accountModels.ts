import { ref } from "vue";
import { defineStore } from "pinia";
import type { ModelInfo } from "../types/agentSession";
import * as api from "../api/agentSessions";

/** Per-account model lists, fetched on demand and cached for the app session.
 *  Deliberately NOT in providerAccounts (whose load() clears state per workspace). */
export const useAccountModelsStore = defineStore("accountModels", () => {
  const modelsByAccount = ref<Record<string, ModelInfo[]>>({});
  const loadingByAccount = ref<Record<string, boolean>>({});
  const errorByAccount = ref<Record<string, string | null>>({});
  // Internal guard, not reactive (Set mutations aren't reactive; read only here).
  const inFlight = new Set<string>();

  async function loadModels(codingWorkspaceId: string, accountId: string) {
    if (!accountId || modelsByAccount.value[accountId] || inFlight.has(accountId)) return;
    inFlight.add(accountId);
    loadingByAccount.value = { ...loadingByAccount.value, [accountId]: true };
    errorByAccount.value = { ...errorByAccount.value, [accountId]: null };
    try {
      const models = await api.listAccountModels(codingWorkspaceId, accountId);
      modelsByAccount.value = { ...modelsByAccount.value, [accountId]: models };
    } catch (e) {
      errorByAccount.value = { ...errorByAccount.value, [accountId]: String(e) };
    } finally {
      inFlight.delete(accountId);
      loadingByAccount.value = { ...loadingByAccount.value, [accountId]: false };
    }
  }

  return { modelsByAccount, loadingByAccount, errorByAccount, loadModels };
});
