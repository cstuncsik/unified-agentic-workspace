use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::{new_id, now_rfc3339};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub settings_json: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, name, kind, settings_json, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get("id")?,
        name: row.get("name")?,
        kind: row.get("kind")?,
        settings_json: row.get("settings_json")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Workspace>> {
    let sql = format!("SELECT {COLUMNS} FROM workspaces ORDER BY created_at ASC, name ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Workspace>> {
    let sql = format!("SELECT {COLUMNS} FROM workspaces WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn create(conn: &Connection, name: &str, kind: &str) -> rusqlite::Result<Workspace> {
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO workspaces (id, name, kind, settings_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, '{}', ?4, ?4)",
        params![id, name, kind, now],
    )?;
    Ok(get(conn, &id)?.expect("workspace exists immediately after insert"))
}

pub fn update(
    conn: &Connection,
    id: &str,
    name: &str,
    kind: &str,
) -> rusqlite::Result<Option<Workspace>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE workspaces SET name = ?2, kind = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, name, kind, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn create_then_list_and_get() {
        let conn = migrated_conn();
        assert!(list(&conn).unwrap().is_empty());

        let ws = create(&conn, "Default", "mixed").unwrap();
        assert_eq!(ws.name, "Default");
        assert_eq!(ws.kind, "mixed");
        assert_eq!(ws.settings_json, "{}");
        assert!(!ws.id.is_empty());

        assert_eq!(list(&conn).unwrap().len(), 1);

        let fetched = get(&conn, &ws.id).unwrap().expect("workspace present");
        assert_eq!(fetched.id, ws.id);
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn update_changes_name_and_kind() {
        let conn = migrated_conn();
        let ws = create(&conn, "Original", "mixed").unwrap();

        let updated = update(&conn, &ws.id, "Renamed", "code")
            .unwrap()
            .expect("workspace updated");
        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.kind, "code");
        assert_eq!(updated.id, ws.id);

        assert!(update(&conn, "missing", "X", "mixed").unwrap().is_none());
    }

    #[test]
    fn delete_removes_workspace() {
        let conn = migrated_conn();
        let ws = create(&conn, "Temp", "mixed").unwrap();

        assert!(delete(&conn, &ws.id).unwrap());
        assert!(list(&conn).unwrap().is_empty());
        assert!(!delete(&conn, &ws.id).unwrap());
    }

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&mut conn).unwrap();
        crate::db::run_migrations(&mut conn).unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(version, 8);
    }
}
