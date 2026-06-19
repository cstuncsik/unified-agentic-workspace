use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::now_rfc3339;

/// Metadata for a stored provider credential. The secret itself is NOT here — it
/// lives in the OS keychain under `keychain_ref`. This struct is the only thing
/// serialized to the frontend, so it can never carry a key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAccount {
    pub id: String,
    pub workspace_id: String,
    pub provider: String,
    pub auth_mode: String,
    pub display_name: String,
    pub keychain_ref: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str =
    "id, workspace_id, provider, auth_mode, display_name, keychain_ref, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<ProviderAccount> {
    Ok(ProviderAccount {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        provider: row.get("provider")?,
        auth_mode: row.get("auth_mode")?,
        display_name: row.get("display_name")?,
        keychain_ref: row.get("keychain_ref")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list_by_workspace(
    conn: &Connection,
    workspace_id: &str,
) -> rusqlite::Result<Vec<ProviderAccount>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM provider_accounts WHERE workspace_id = ?1 \
         ORDER BY created_at ASC, display_name ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<ProviderAccount>> {
    let sql = format!("SELECT {COLUMNS} FROM provider_accounts WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Insert a metadata row. The caller generates `id`/`keychain_ref` (they are equal)
/// and is responsible for having stored the secret in the keystore first.
pub fn insert(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    provider: &str,
    auth_mode: &str,
    display_name: &str,
    keychain_ref: &str,
) -> rusqlite::Result<ProviderAccount> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO provider_accounts
         (id, workspace_id, provider, auth_mode, display_name, keychain_ref, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![id, workspace_id, provider, auth_mode, display_name, keychain_ref, now],
    )?;
    Ok(get(conn, id)?.expect("provider account exists immediately after insert"))
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM provider_accounts WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::new_id;

    // Enables PRAGMA foreign_keys = ON so cascade behavior is real (the
    // workspace.rs helper does NOT — mirror project.rs instead).
    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    fn make_workspace(conn: &Connection) -> String {
        crate::models::workspace::create(conn, "WS", "mixed")
            .unwrap()
            .id
    }

    #[test]
    fn insert_then_list_and_get() {
        let conn = migrated_conn();
        let ws = make_workspace(&conn);
        let id = new_id();
        let acct = insert(&conn, &id, &ws, "anthropic", "api-key", "Work key", &id).unwrap();
        assert_eq!(acct.provider, "anthropic");
        assert_eq!(acct.auth_mode, "api-key");
        assert_eq!(acct.keychain_ref, id);

        let listed = list_by_workspace(&conn, &ws).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(get(&conn, &id).unwrap().unwrap().id, id);
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn delete_removes_row() {
        let conn = migrated_conn();
        let ws = make_workspace(&conn);
        let id = new_id();
        insert(&conn, &id, &ws, "openai", "api-key", "Key", &id).unwrap();
        assert!(delete(&conn, &id).unwrap());
        assert!(list_by_workspace(&conn, &ws).unwrap().is_empty());
        assert!(!delete(&conn, &id).unwrap());
    }

    #[test]
    fn deleting_workspace_cascades_accounts() {
        let conn = migrated_conn();
        let ws = make_workspace(&conn);
        let id = new_id();
        insert(&conn, &id, &ws, "anthropic", "api-key", "Key", &id).unwrap();
        crate::models::workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &id).unwrap().is_none());
    }
}
