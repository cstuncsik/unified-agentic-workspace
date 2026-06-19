use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::{new_id, now_rfc3339};

/// Allowed session modes; see PRD "Session".
pub const SESSION_MODES: &[&str] = &["research", "document", "code", "review", "terminal"];

/// Allowed session statuses; see PRD "Session".
pub const SESSION_STATUSES: &[&str] = &[
    "backlog",
    "todo",
    "running",
    "worktree-created",
    "agent-running",
    "tests-running",
    "review-agent-running",
    "needs-review",
    "done",
    "merged",
    "discarded",
    "cancelled",
    "archived",
    "flagged",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub mode: String,
    pub status: String,
    pub created_from_artifact_id: Option<String>,
    pub summary: Option<String>,
    pub permissions_json: String,
    pub context_refs_json: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, project_id, title, mode, status, summary, \
                       permissions_json, context_refs_json, created_from_artifact_id, \
                       created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        project_id: row.get("project_id")?,
        title: row.get("title")?,
        mode: row.get("mode")?,
        status: row.get("status")?,
        summary: row.get("summary")?,
        permissions_json: row.get("permissions_json")?,
        context_refs_json: row.get("context_refs_json")?,
        created_from_artifact_id: row.get("created_from_artifact_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<Session>> {
    let sql =
        format!("SELECT {COLUMNS} FROM sessions WHERE workspace_id = ?1 ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn list_by_project(
    conn: &Connection,
    workspace_id: &str,
    project_id: &str,
) -> rusqlite::Result<Vec<Session>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM sessions WHERE workspace_id = ?1 AND project_id = ?2
         ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id, project_id], from_row)?;
    rows.collect()
}

pub fn list_by_artifact(
    conn: &Connection,
    workspace_id: &str,
    artifact_id: &str,
) -> rusqlite::Result<Vec<Session>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM sessions
         WHERE workspace_id = ?1 AND created_from_artifact_id = ?2 ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id, artifact_id], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Session>> {
    let sql = format!("SELECT {COLUMNS} FROM sessions WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    workspace_id: &str,
    project_id: Option<&str>,
    title: &str,
    mode: &str,
    status: &str,
    created_from_artifact_id: Option<&str>,
) -> rusqlite::Result<Session> {
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO sessions (id, workspace_id, project_id, title, mode, status,
                               permissions_json, context_refs_json,
                               created_from_artifact_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', '[]', ?7, ?8, ?8)",
        params![id, workspace_id, project_id, title, mode, status, created_from_artifact_id, now],
    )?;
    Ok(get(conn, &id)?.expect("session exists immediately after insert"))
}

pub fn update(
    conn: &Connection,
    id: &str,
    title: &str,
    summary: Option<&str>,
) -> rusqlite::Result<Option<Session>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE sessions SET title = ?2, summary = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, title, summary, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

pub fn update_status(
    conn: &Connection,
    id: &str,
    status: &str,
) -> rusqlite::Result<Option<Session>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE sessions SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{project, workspace};
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

        let session = create(&conn, &ws, None, "Spike research", "research", "todo", None).unwrap();
        assert_eq!(session.workspace_id, ws);
        assert_eq!(session.project_id, None);
        assert_eq!(session.title, "Spike research");
        assert_eq!(session.mode, "research");
        assert_eq!(session.status, "todo");
        assert_eq!(session.summary, None);
        assert_eq!(session.permissions_json, "{}");
        assert_eq!(session.context_refs_json, "[]");

        assert_eq!(list(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &session.id).unwrap().is_some());
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn list_is_scoped_to_workspace_and_project() {
        let conn = migrated_conn();
        let ws_a = workspace_id(&conn);
        let ws_b = workspace_id(&conn);
        let project = project::create(&conn, &ws_a, "P", "code").unwrap();

        create(
            &conn,
            &ws_a,
            Some(&project.id),
            "In project",
            "code",
            "todo",
            None,
        )
        .unwrap();
        create(&conn, &ws_a, None, "Loose", "research", "todo", None).unwrap();

        assert_eq!(list(&conn, &ws_a).unwrap().len(), 2);
        assert!(list(&conn, &ws_b).unwrap().is_empty());
        assert_eq!(list_by_project(&conn, &ws_a, &project.id).unwrap().len(), 1);
        // A project id from another workspace must not leak sessions across the boundary.
        assert!(list_by_project(&conn, &ws_b, &project.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn status_moves_through_core_workflow() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let session = create(&conn, &ws, None, "Implement feature", "code", "todo", None).unwrap();

        for status in ["running", "needs-review", "done"] {
            let moved = update_status(&conn, &session.id, status)
                .unwrap()
                .expect("session updated");
            assert_eq!(moved.status, status);
        }

        assert!(update_status(&conn, "missing", "done").unwrap().is_none());
    }

    #[test]
    fn update_changes_title_and_summary() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let session = create(&conn, &ws, None, "Original", "document", "todo", None).unwrap();

        let updated = update(&conn, &session.id, "Renamed", Some("Wrote the spec"))
            .unwrap()
            .expect("session updated");
        assert_eq!(updated.title, "Renamed");
        assert_eq!(updated.summary.as_deref(), Some("Wrote the spec"));

        assert!(update(&conn, "missing", "X", None).unwrap().is_none());
    }

    #[test]
    fn deleting_project_detaches_sessions() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let project = project::create(&conn, &ws, "P", "code").unwrap();
        let session =
            create(&conn, &ws, Some(&project.id), "Attached", "code", "todo", None).unwrap();

        project::delete(&conn, &project.id).unwrap();
        let detached = get(&conn, &session.id).unwrap().expect("session survives");
        assert_eq!(detached.project_id, None);
    }

    #[test]
    fn deleting_workspace_cascades_to_sessions() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let session = create(&conn, &ws, None, "Doomed", "terminal", "todo", None).unwrap();

        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &session.id).unwrap().is_none());
    }

    #[test]
    fn artifact_link_round_trips_and_lists() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "WS", "mixed").unwrap().id;
        let art = crate::models::artifact::create(&conn, &ws, None, "Spec").unwrap();
        let s = create(&conn, &ws, None, "Task A", "code", "todo", Some(&art.id)).unwrap();
        assert_eq!(s.created_from_artifact_id.as_deref(), Some(art.id.as_str()));
        assert_eq!(list_by_artifact(&conn, &ws, &art.id).unwrap().len(), 1);
    }

    #[test]
    fn deleting_artifact_detaches_sessions() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "WS", "mixed").unwrap().id;
        let art = crate::models::artifact::create(&conn, &ws, None, "Spec").unwrap();
        let s = create(&conn, &ws, None, "Task", "code", "todo", Some(&art.id)).unwrap();
        crate::models::artifact::delete(&conn, &art.id).unwrap();
        let after = get(&conn, &s.id).unwrap().expect("session survives");
        assert_eq!(after.created_from_artifact_id, None); // ON DELETE SET NULL
    }
}
