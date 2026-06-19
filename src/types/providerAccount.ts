/** Metadata for a stored provider credential. The API key is NEVER part of this
 *  type — it lives in the OS keychain; the frontend only ever sees metadata. */
export interface ProviderAccount {
  id: string;
  workspace_id: string;
  provider: string;
  auth_mode: string;
  display_name: string;
  keychain_ref: string;
  created_at: string;
  updated_at: string;
}
