import { ref } from "vue";
import { defineStore } from "pinia";
import type { ProviderAccount } from "../types/providerAccount";
import * as api from "../api/providerAccounts";
import type { CreateProviderAccountInput } from "../api/providerAccounts";

export const useProviderAccountsStore = defineStore("providerAccounts", () => {
  const list = ref<ProviderAccount[]>([]);
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
      const rows = await api.listProviderAccounts(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function create(input: CreateProviderAccountInput) {
    const token = loadToken;
    const account = await api.createProviderAccount(input);
    if (token !== loadToken) return account;
    list.value.push(account);
    return account;
  }

  async function remove(id: string) {
    await api.deleteProviderAccount(id);
    list.value = list.value.filter((a) => a.id !== id);
  }

  return { list, loading, error, load, create, remove };
});
