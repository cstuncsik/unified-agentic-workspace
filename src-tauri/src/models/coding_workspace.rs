use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingWorkspace {
    pub id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub repository_source_id: String,
    pub session_id: Option<String>,
    pub repo_path: String,
    pub worktree_path: String,
    pub branch_name: String,
    pub base_branch: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, project_id, repository_source_id, session_id, \
                       repo_path, worktree_path, branch_name, base_branch, status, \
                       created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<CodingWorkspace> {
    Ok(CodingWorkspace {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        project_id: row.get("project_id")?,
        repository_source_id: row.get("repository_source_id")?,
        session_id: row.get("session_id")?,
        repo_path: row.get("repo_path")?,
        worktree_path: row.get("worktree_path")?,
        branch_name: row.get("branch_name")?,
        base_branch: row.get("base_branch")?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<CodingWorkspace>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM coding_workspaces WHERE workspace_id = ?1 ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<CodingWorkspace>> {
    let sql = format!("SELECT {COLUMNS} FROM coding_workspaces WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Insert a coding workspace. The caller supplies the `id` because it also names
/// the on-disk worktree directory, which must exist before the row is written.
#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    project_id: &str,
    repository_source_id: &str,
    repo_path: &str,
    worktree_path: &str,
    branch_name: &str,
    base_branch: &str,
    session_id: Option<&str>,
) -> rusqlite::Result<CodingWorkspace> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO coding_workspaces
           (id, workspace_id, project_id, repository_source_id, session_id, repo_path,
            worktree_path, branch_name, base_branch, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'worktree-created', ?10, ?10)",
        params![
            id, workspace_id, project_id, repository_source_id, session_id, repo_path,
            worktree_path, branch_name, base_branch, now
        ],
    )?;
    Ok(get(conn, id)?.expect("coding workspace exists immediately after insert"))
}

pub fn update_status(
    conn: &Connection,
    id: &str,
    status: &str,
) -> rusqlite::Result<Option<CodingWorkspace>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE coding_workspaces SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM coding_workspaces WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{project, repository, workspace};
    use crate::util::new_id;
    use rusqlite::Connection;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    /// (workspace_id, project_id, repository_source_id)
    fn fixtures(conn: &Connection) -> (String, String, String) {
        let ws = workspace::create(conn, "Test", "mixed").unwrap().id;
        let p = project::create(conn, &ws, "P", "code").unwrap().id;
        let r = repository::create(conn, &ws, "repo", "/tmp/repo", "main", None)
            .unwrap()
            .id;
        (ws, p, r)
    }

    fn make(conn: &Connection, ws: &str, p: &str, r: &str) -> CodingWorkspace {
        let id = new_id();
        create(
            conn,
            &id,
            ws,
            p,
            r,
            "/tmp/repo",
            &format!("/tmp/worktrees/{id}"),
            "feature/x",
            "main",
            None,
        )
        .unwrap()
    }

    #[test]
    fn create_then_list_and_get() {
        let conn = migrated_conn();
        let (ws, p, r) = fixtures(&conn);
        assert!(list(&conn, &ws).unwrap().is_empty());

        let cw = make(&conn, &ws, &p, &r);
        assert_eq!(cw.workspace_id, ws);
        assert_eq!(cw.project_id, p);
        assert_eq!(cw.repository_source_id, r);
        assert_eq!(cw.branch_name, "feature/x");
        assert_eq!(cw.base_branch, "main");
        assert_eq!(cw.status, "worktree-created");
        assert_eq!(cw.session_id, None);

        assert_eq!(list(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &cw.id).unwrap().is_some());
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn create_with_session_id_links_it() {
        let conn = migrated_conn();
        let (ws, p, r) = fixtures(&conn);
        let s = crate::models::session::create(&conn, &ws, Some(&p), "T", "code", "todo", None)
            .unwrap();
        let id = new_id();
        let cw = create(
            &conn, &id, &ws, &p, &r, "/tmp/repo", &format!("/tmp/wt/{id}"),
            "feat/x", "main", Some(&s.id),
        )
        .unwrap();
        assert_eq!(cw.session_id.as_deref(), Some(s.id.as_str()));
    }

    #[test]
    fn mark_ready_and_delete() {
        let conn = migrated_conn();
        let (ws, p, r) = fixtures(&conn);
        let cw = make(&conn, &ws, &p, &r);

        let moved = update_status(&conn, &cw.id, "needs-review")
            .unwrap()
            .expect("updated");
        assert_eq!(moved.status, "needs-review");

        assert!(delete(&conn, &cw.id).unwrap());
        assert!(list(&conn, &ws).unwrap().is_empty());
        assert!(!delete(&conn, &cw.id).unwrap());
    }

    #[test]
    fn deleting_project_or_repository_cascades_the_coding_workspace() {
        let conn = migrated_conn();
        let (ws, p, r) = fixtures(&conn);
        let cw = make(&conn, &ws, &p, &r);

        // project_id is NOT NULL with ON DELETE CASCADE.
        project::delete(&conn, &p).unwrap();
        assert!(get(&conn, &cw.id).unwrap().is_none());

        // Same for the repository source.
        let cw2 = make(
            &conn,
            &ws,
            &project::create(&conn, &ws, "P2", "code").unwrap().id,
            &r,
        );
        repository::delete(&conn, &r).unwrap();
        assert!(get(&conn, &cw2.id).unwrap().is_none());
    }

    #[test]
    fn deleting_workspace_cascades() {
        let conn = migrated_conn();
        let (ws, p, r) = fixtures(&conn);
        let cw = make(&conn, &ws, &p, &r);

        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &cw.id).unwrap().is_none());
    }
}
