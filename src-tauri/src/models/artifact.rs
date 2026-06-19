use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::{new_id, now_rfc3339};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub workspace_id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, project_id, title, content, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        project_id: row.get("project_id")?,
        title: row.get("title")?,
        content: row.get("content")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Insert an artifact (content starts empty). Generates its own id, matching
/// session/project/repository (not the external-id `review::create`).
pub fn create(
    conn: &Connection,
    workspace_id: &str,
    project_id: Option<&str>,
    title: &str,
) -> rusqlite::Result<Artifact> {
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO artifacts (id, workspace_id, project_id, title, content, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '', ?5, ?5)",
        params![id, workspace_id, project_id, title, now],
    )?;
    Ok(get(conn, &id)?.expect("artifact exists immediately after insert"))
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Artifact>> {
    let sql = format!("SELECT {COLUMNS} FROM artifacts WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<Artifact>> {
    let sql =
        format!("SELECT {COLUMNS} FROM artifacts WHERE workspace_id = ?1 ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn update(
    conn: &Connection,
    id: &str,
    title: &str,
    content: &str,
) -> rusqlite::Result<Option<Artifact>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE artifacts SET title = ?2, content = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, title, content, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM artifacts WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{project, workspace};

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn create_update_list_round_trips_realistic_content() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let a = create(&conn, &ws, None, "Notes").unwrap();
        assert_eq!(a.content, ""); // DEFAULT ''
        assert_eq!(a.project_id, None);

        // Realistic multi-line content with quotes/backticks survives the round-trip.
        let body = "# Heading\n\nA \"quoted\" line and `code` and a\nsecond line.\n";
        let updated = update(&conn, &a.id, "Notes v2", body).unwrap().unwrap();
        assert_eq!(updated.title, "Notes v2");
        assert_eq!(updated.content, body);

        assert_eq!(list(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &a.id).unwrap().is_some());
        assert!(delete(&conn, &a.id).unwrap());
        assert!(get(&conn, &a.id).unwrap().is_none());
    }

    #[test]
    fn deleting_workspace_cascades_artifacts() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let a = create(&conn, &ws, None, "Doc").unwrap();
        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &a.id).unwrap().is_none());
    }

    #[test]
    fn deleting_project_detaches_artifacts() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let p = project::create(&conn, &ws, "P", "research").unwrap().id;
        let a = create(&conn, &ws, Some(&p), "Doc").unwrap();
        assert_eq!(a.project_id.as_deref(), Some(p.as_str()));

        project::delete(&conn, &p).unwrap();
        let after = get(&conn, &a.id).unwrap().expect("artifact survives");
        assert_eq!(after.project_id, None); // ON DELETE SET NULL
    }
}
