use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub workspace_id: String,
    pub coding_workspace_id: String,
    pub adapter_id: String,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub transcript_path: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, coding_workspace_id, adapter_id, command, status, \
                       exit_code, transcript_path, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<AgentSession> {
    Ok(AgentSession {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        coding_workspace_id: row.get("coding_workspace_id")?,
        adapter_id: row.get("adapter_id")?,
        command: row.get("command")?,
        status: row.get("status")?,
        exit_code: row.get("exit_code")?,
        transcript_path: row.get("transcript_path")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    coding_workspace_id: &str,
    adapter_id: &str,
    command: &str,
    transcript_path: &str,
) -> rusqlite::Result<AgentSession> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO agent_sessions
           (id, workspace_id, coding_workspace_id, adapter_id, command, status,
            exit_code, transcript_path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', NULL, ?6, ?7, ?7)",
        params![id, workspace_id, coding_workspace_id, adapter_id, command, transcript_path, now],
    )?;
    Ok(get(conn, id)?.expect("agent session exists immediately after insert"))
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<AgentSession>> {
    let sql = format!("SELECT {COLUMNS} FROM agent_sessions WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list_by_coding_workspace(
    conn: &Connection,
    coding_workspace_id: &str,
) -> rusqlite::Result<Vec<AgentSession>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM agent_sessions WHERE coding_workspace_id = ?1 ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![coding_workspace_id], from_row)?;
    rows.collect()
}

/// Move a still-running session to a terminal status. No-op if it already left
/// `running` (e.g. a user `stop` raced the natural exit), so a kill recorded as
/// `stopped` is not overwritten by the reader thread's `exited`/`failed`.
pub fn mark_exited(
    conn: &Connection,
    id: &str,
    status: &str,
    exit_code: Option<i64>,
) -> rusqlite::Result<Option<AgentSession>> {
    let now = now_rfc3339();
    conn.execute(
        "UPDATE agent_sessions SET status = ?2, exit_code = ?3, updated_at = ?4
         WHERE id = ?1 AND status = 'running'",
        params![id, status, exit_code, now],
    )?;
    get(conn, id)
}

/// Force a terminal status regardless of current state (used by explicit stop).
pub fn set_status(
    conn: &Connection,
    id: &str,
    status: &str,
) -> rusqlite::Result<Option<AgentSession>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE agent_sessions SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{coding_workspace, project, repository, workspace};
    use crate::util::new_id;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    /// (workspace_id, coding_workspace_id)
    fn fixtures(conn: &Connection) -> (String, String) {
        let ws = workspace::create(conn, "Test", "mixed").unwrap().id;
        let p = project::create(conn, &ws, "P", "code").unwrap().id;
        let r = repository::create(conn, &ws, "repo", "/tmp/repo", "main", None)
            .unwrap()
            .id;
        let cw_id = new_id();
        let cw = coding_workspace::create(
            conn, &cw_id, &ws, &p, &r, "/tmp/repo",
            &format!("/tmp/worktrees/{cw_id}"), "feature/x", "main", None,
        )
        .unwrap();
        (ws, cw.id)
    }

    fn make(conn: &Connection, ws: &str, cw: &str) -> AgentSession {
        create(conn, &new_id(), ws, cw, "claude-code", "claude", "/tmp/t.log").unwrap()
    }

    #[test]
    fn create_then_get_and_list() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);
        assert_eq!(s.status, "running");
        assert_eq!(s.adapter_id, "claude-code");
        assert_eq!(s.exit_code, None);
        assert_eq!(list_by_coding_workspace(&conn, &cw).unwrap().len(), 1);
        assert!(get(&conn, &s.id).unwrap().is_some());
    }

    #[test]
    fn mark_exited_only_moves_running_sessions() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);

        // A user stop forces 'stopped'...
        set_status(&conn, &s.id, "stopped").unwrap();
        // ...and a racing natural-exit mark_exited must NOT overwrite it.
        let after = mark_exited(&conn, &s.id, "exited", Some(0)).unwrap().unwrap();
        assert_eq!(after.status, "stopped");
        assert_eq!(after.exit_code, None);
    }

    #[test]
    fn mark_exited_records_running_exit() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);
        let after = mark_exited(&conn, &s.id, "exited", Some(0)).unwrap().unwrap();
        assert_eq!(after.status, "exited");
        assert_eq!(after.exit_code, Some(0));
    }

    #[test]
    fn deleting_coding_workspace_cascades_sessions() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);
        coding_workspace::delete(&conn, &cw).unwrap();
        assert!(get(&conn, &s.id).unwrap().is_none());
    }
}
