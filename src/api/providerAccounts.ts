import { invoke } from "@tauri-apps/api/core";
import type { ProviderAccount } from "../types/providerAccount";

export function listProviderAccounts(workspaceId: string): Promise<ProviderAccount[]> {
  return invoke<ProviderAccount[]>("list_provider_accounts", { workspaceId });
}

export interface CreateProviderAccountInput {
  workspaceId: string;
  provider: string;
  displayName: string;
  apiKey: string;
}

export function createProviderAccount(input: CreateProviderAccountInput): Promise<ProviderAccount> {
  return invoke<ProviderAccount>("create_provider_account", { ...input });
}

export function deleteProviderAccount(id: string): Promise<boolean> {
  return invoke<boolean>("delete_provider_account", { id });
}
