use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::provider_account::{self, ProviderAccount};
use crate::models::workspace;
use crate::services::keystore::{self, KeyStore};
use crate::util::new_id;

/// Providers a user may register in this slice.
const PROVIDERS: &[&str] = &["anthropic", "openai"];

/// Testable core: validate, store the secret, insert the row, roll back the
/// keychain entry on insert failure. Takes `&dyn KeyStore` so tests inject a
/// FileKeyStore with no env reliance. Every error is a FIXED, secret-free string
/// because the frontend renders backend errors verbatim.
pub fn create_provider_account_inner(
    conn: &Connection,
    store: &dyn KeyStore,
    workspace_id: &str,
    provider: &str,
    display_name: &str,
    api_key: &str,
) -> Result<ProviderAccount, String> {
    // 1. Validate BEFORE touching the keychain.
    if !PROVIDERS.contains(&provider) {
        return Err("Unknown provider".into());
    }
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err("Account name is required".into());
    }
    if api_key.trim().is_empty() {
        return Err("API key is required".into());
    }
    match workspace::get(conn, workspace_id) {
        Ok(Some(_)) => {}
        Ok(None) => return Err("Workspace not found".into()),
        Err(_) => return Err("Failed to save account".into()),
    }

    // 2. Identifiers. keychain_ref == id (opaque UUID).
    let id = new_id();
    let keychain_ref = id.clone();

    // 3. Store the secret. Drop the backend error (it carries nothing) and return
    //    a fixed opaque message.
    if store.set(&keychain_ref, api_key).is_err() {
        return Err("Failed to store key".into());
    }

    // 4. Insert metadata; roll back the keychain entry on failure.
    match provider_account::insert(
        conn,
        &id,
        workspace_id,
        provider,
        "api-key",
        display_name,
        &keychain_ref,
    ) {
        Ok(account) => Ok(account),
        Err(_) => {
            let _ = store.delete(&keychain_ref);
            Err("Failed to save account".into())
        }
    }
}

#[tauri::command]
pub fn create_provider_account(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    provider: String,
    display_name: String,
    api_key: String,
) -> Result<ProviderAccount, String> {
    let store = keystore::resolve();
    let conn = state.lock().map_err(|e| e.to_string())?;
    create_provider_account_inner(
        &conn,
        store.as_ref(),
        &workspace_id,
        &provider,
        &display_name,
        &api_key,
    )
}

#[tauri::command]
pub fn list_provider_accounts(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<ProviderAccount>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    provider_account::list_by_workspace(&conn, &workspace_id)
        .map_err(|_| "Failed to load accounts".into())
}

/// Testable core for delete: remove the keychain entry (idempotent) then the row.
pub fn delete_provider_account_inner(
    conn: &Connection,
    store: &dyn KeyStore,
    id: &str,
) -> Result<bool, String> {
    if let Some(account) = provider_account::get(conn, id).map_err(|_| "Failed to delete account")? {
        // Idempotent: a missing keychain entry is success.
        if store.delete(&account.keychain_ref).is_err() {
            return Err("Failed to delete account".into());
        }
    }
    provider_account::delete(conn, id).map_err(|_| "Failed to delete account".into())
}

#[tauri::command]
pub fn delete_provider_account(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<bool, String> {
    let store = keystore::resolve();
    let conn = state.lock().map_err(|e| e.to_string())?;
    delete_provider_account_inner(&conn, store.as_ref(), &id)
}

/// Keychain refs for every provider account in a workspace. Read under the
/// connection lock; the caller then deletes the entries (see
/// `delete_keychain_entries`) WITHOUT holding the lock across keychain IO.
pub fn workspace_keychain_refs(
    conn: &Connection,
    workspace_id: &str,
) -> rusqlite::Result<Vec<String>> {
    Ok(provider_account::list_by_workspace(conn, workspace_id)?
        .into_iter()
        .map(|a| a.keychain_ref)
        .collect())
}

/// Delete a set of keychain entries (no DB connection, no lock). `delete` is
/// idempotent (missing entry = success), so re-runs are safe. Best-effort: a
/// single backend failure does not abort the rest.
pub fn delete_keychain_entries(store: &dyn KeyStore, refs: &[String]) {
    for r in refs {
        let _ = store.delete(r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keystore::{FileKeyStore, KeyStoreError};

    const SENTINEL: &str = "SENTINEL_SECRET_123";

    // A keystore whose `set` always fails — drives the "Failed to store key" branch
    // (the one place a secret has been handed to the backend).
    struct FailingSetStore;
    impl KeyStore for FailingSetStore {
        fn set(&self, _r: &str, _s: &str) -> Result<(), KeyStoreError> {
            Err(KeyStoreError)
        }
        fn get(&self, _r: &str) -> Result<Option<String>, KeyStoreError> {
            Ok(None)
        }
        fn delete(&self, _r: &str) -> Result<(), KeyStoreError> {
            Ok(())
        }
    }

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    fn temp_store() -> FileKeyStore {
        let mut d = std::env::temp_dir();
        d.push(format!("uaw-cmd-test-{}", new_id()));
        FileKeyStore::new(d)
    }

    fn make_ws(conn: &Connection) -> String {
        crate::models::workspace::create(conn, "WS", "mixed")
            .unwrap()
            .id
    }

    #[test]
    fn key_stored_in_keystore_not_db_or_payload() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let acct =
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "Work", SENTINEL).unwrap();
        // Returned struct carries only metadata.
        assert_eq!(acct.provider, "anthropic");
        assert_eq!(acct.auth_mode, "api-key");
        // Secret is in the keystore under keychain_ref...
        assert_eq!(
            store.get(&acct.keychain_ref).unwrap(),
            Some(SENTINEL.to_string())
        );
        // ...and the serialized account contains ONLY the known metadata fields
        // (catches any future key-bearing field regardless of its name) and never
        // the sentinel.
        let json = serde_json::to_string(&acct).unwrap();
        assert!(!json.contains(SENTINEL));
        let value: serde_json::Value = serde_json::to_value(&acct).unwrap();
        let keys: std::collections::BTreeSet<String> =
            value.as_object().unwrap().keys().cloned().collect();
        let expected: std::collections::BTreeSet<String> = [
            "id",
            "workspace_id",
            "provider",
            "auth_mode",
            "display_name",
            "keychain_ref",
            "created_at",
            "updated_at",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn store_set_failure_maps_to_fixed_string_and_writes_nothing() {
        let conn = migrated_conn();
        let ws = make_ws(&conn);
        let err =
            create_provider_account_inner(&conn, &FailingSetStore, &ws, "anthropic", "n", SENTINEL)
                .unwrap_err();
        assert_eq!(err, "Failed to store key");
        assert!(!err.contains(SENTINEL));
        // No row was written when the keystore rejected the secret.
        assert!(provider_account::list_by_workspace(&conn, &ws)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn insert_failure_rolls_back_keychain() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        // Force the INSERT to fail AFTER store.set succeeded: drop the table.
        // (workspaces still exists, so the workspace-existence check passes.)
        conn.execute_batch("DROP TABLE provider_accounts;").unwrap();
        let err =
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "n", SENTINEL)
                .unwrap_err();
        assert_eq!(err, "Failed to save account");
        assert!(!err.contains(SENTINEL));
        // The keychain entry set before the failed insert must have been rolled
        // back — the backing dir holds no files. (keychain_ref is unknowable here.)
        let remaining = std::fs::read_dir(store.dir()).map(|d| d.count()).unwrap_or(0);
        assert_eq!(remaining, 0, "keychain entry was not rolled back");
    }

    #[test]
    fn sentinel_key_never_appears_in_any_error() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);

        // Each failure branch, all carrying the sentinel as the key.
        let cases = [
            create_provider_account_inner(&conn, &store, &ws, "evil-provider", "n", SENTINEL),
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "   ", SENTINEL),
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "n", "   "),
            create_provider_account_inner(&conn, &store, "no-such-ws", "anthropic", "n", SENTINEL),
        ];
        for case in cases {
            if let Err(msg) = case {
                assert!(!msg.contains(SENTINEL), "error leaked the key: {msg}");
            }
        }
    }

    #[test]
    fn whitelist_rejects_unknown_and_writes_no_key() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let err = create_provider_account_inner(&conn, &store, &ws, "openrouter", "n", SENTINEL)
            .unwrap_err();
        assert_eq!(err, "Unknown provider");
        assert!(provider_account::list_by_workspace(&conn, &ws)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_empty_name_and_empty_key() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        assert_eq!(
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "  ", SENTINEL)
                .unwrap_err(),
            "Account name is required"
        );
        assert_eq!(
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "n", "   ")
                .unwrap_err(),
            "API key is required"
        );
    }

    #[test]
    fn delete_account_removes_key_and_row() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let acct =
            create_provider_account_inner(&conn, &store, &ws, "openai", "K", SENTINEL).unwrap();
        assert!(delete_provider_account_inner(&conn, &store, &acct.id).unwrap());
        assert_eq!(store.get(&acct.keychain_ref).unwrap(), None);
        assert!(provider_account::get(&conn, &acct.id).unwrap().is_none());
        // Idempotent: deleting again is Ok and returns false (no row).
        assert!(!delete_provider_account_inner(&conn, &store, &acct.id).unwrap());
    }

    #[test]
    fn list_is_metadata_only() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        create_provider_account_inner(&conn, &store, &ws, "anthropic", "A", SENTINEL).unwrap();
        let json =
            serde_json::to_string(&provider_account::list_by_workspace(&conn, &ws).unwrap())
                .unwrap();
        assert!(!json.contains(SENTINEL));
    }

    #[test]
    fn workspace_delete_path_removes_keys_and_rows() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let a =
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "A", SENTINEL).unwrap();
        let b =
            create_provider_account_inner(&conn, &store, &ws, "openai", "B", SENTINEL).unwrap();

        // Mirror delete_workspace's real ordering: collect refs (under the lock),
        // delete keychain entries (outside the lock), THEN cascade-delete the rows.
        let refs = super::workspace_keychain_refs(&conn, &ws).unwrap();
        assert_eq!(refs.len(), 2);
        super::delete_keychain_entries(&store, &refs);
        crate::models::workspace::delete(&conn, &ws).unwrap();

        // Keys gone...
        assert_eq!(store.get(&a.keychain_ref).unwrap(), None);
        assert_eq!(store.get(&b.keychain_ref).unwrap(), None);
        // ...AND rows gone (the cascade ran after the keychain pass).
        assert!(provider_account::list_by_workspace(&conn, &ws)
            .unwrap()
            .is_empty());
    }
}
