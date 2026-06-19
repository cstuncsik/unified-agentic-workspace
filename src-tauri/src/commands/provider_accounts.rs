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

    // 3. Store the secret. Log the backend detail to stderr (never the secret,
    //    never the UI); return a fixed opaque message.
    if let Err(e) = store.set(&keychain_ref, api_key) {
        eprintln!("keystore set failed: {}", e.detail());
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
        if let Err(e) = store.delete(&account.keychain_ref) {
            eprintln!("keystore delete failed: {}", e.detail());
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

/// Best-effort: delete every stored key for a workspace's accounts BEFORE the
/// rows are removed (by cascade), so a `keychain_ref` is never stranded. `delete`
/// is idempotent so re-runs are safe.
pub fn cleanup_workspace_keys(conn: &Connection, store: &dyn KeyStore, workspace_id: &str) {
    if let Ok(accounts) = provider_account::list_by_workspace(conn, workspace_id) {
        for account in accounts {
            let _ = store.delete(&account.keychain_ref);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keystore::FileKeyStore;

    const SENTINEL: &str = "SENTINEL_SECRET_123";

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
        // ...and the serialized account never contains it.
        let json = serde_json::to_string(&acct).unwrap();
        assert!(!json.contains(SENTINEL));
        assert!(!json.to_lowercase().contains("api_key"));
        assert!(!json.to_lowercase().contains("\"key\""));
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
    fn workspace_cleanup_removes_all_account_keys() {
        let conn = migrated_conn();
        let store = temp_store();
        let ws = make_ws(&conn);
        let a =
            create_provider_account_inner(&conn, &store, &ws, "anthropic", "A", SENTINEL).unwrap();
        let b =
            create_provider_account_inner(&conn, &store, &ws, "openai", "B", SENTINEL).unwrap();

        super::cleanup_workspace_keys(&conn, &store, &ws);

        assert_eq!(store.get(&a.keychain_ref).unwrap(), None);
        assert_eq!(store.get(&b.keychain_ref).unwrap(), None);
    }
}
