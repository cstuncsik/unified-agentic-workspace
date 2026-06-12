use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::{new_id, now_rfc3339};

/// Allowed project modes; see PRD "Project".
pub const PROJECT_MODES: &[&str] = &["research", "code", "mixed"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub mode: String,
    pub settings_json: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, name, mode, settings_json, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        name: row.get("name")?,
        mode: row.get("mode")?,
        settings_json: row.get("settings_json")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<Project>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM projects WHERE workspace_id = ?1 ORDER BY created_at ASC, name ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Project>> {
    let sql = format!("SELECT {COLUMNS} FROM projects WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn create(
    conn: &Connection,
    workspace_id: &str,
    name: &str,
    mode: &str,
) -> rusqlite::Result<Project> {
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO projects (id, workspace_id, name, mode, settings_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '{}', ?5, ?5)",
        params![id, workspace_id, name, mode, now],
    )?;
    Ok(get(conn, &id)?.expect("project exists immediately after insert"))
}

pub fn update(
    conn: &Connection,
    id: &str,
    name: &str,
    mode: &str,
) -> rusqlite::Result<Option<Project>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE projects SET name = ?2, mode = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, name, mode, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::workspace;
    use rusqlite::Connection;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    fn workspace_id(conn: &Connection) -> String {
        workspace::create(conn, "Test", "mixed").unwrap().id
    }

    #[test]
    fn create_then_list_and_get() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        assert!(list(&conn, &ws).unwrap().is_empty());

        let project = create(&conn, &ws, "UAW MVP", "code").unwrap();
        assert_eq!(project.workspace_id, ws);
        assert_eq!(project.name, "UAW MVP");
        assert_eq!(project.mode, "code");
        assert_eq!(project.settings_json, "{}");

        assert_eq!(list(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &project.id).unwrap().is_some());
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn list_is_scoped_to_workspace() {
        let conn = migrated_conn();
        let ws_a = workspace_id(&conn);
        let ws_b = workspace_id(&conn);
        create(&conn, &ws_a, "A", "research").unwrap();

        assert_eq!(list(&conn, &ws_a).unwrap().len(), 1);
        assert!(list(&conn, &ws_b).unwrap().is_empty());
    }

    #[test]
    fn update_changes_name_and_mode() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let project = create(&conn, &ws, "Original", "research").unwrap();

        let updated = update(&conn, &project.id, "Renamed", "mixed")
            .unwrap()
            .expect("project updated");
        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.mode, "mixed");

        assert!(update(&conn, "missing", "X", "research").unwrap().is_none());
    }

    #[test]
    fn delete_removes_project() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let project = create(&conn, &ws, "Temp", "code").unwrap();

        assert!(delete(&conn, &project.id).unwrap());
        assert!(list(&conn, &ws).unwrap().is_empty());
        assert!(!delete(&conn, &project.id).unwrap());
    }

    #[test]
    fn deleting_workspace_cascades_to_projects() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let project = create(&conn, &ws, "Doomed", "code").unwrap();

        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &project.id).unwrap().is_none());
    }
}
