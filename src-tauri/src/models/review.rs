use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    pub workspace_id: String,
    pub coding_workspace_id: String,
    pub status: String,
    pub summary: String,
    pub status_short: String,
    pub diff_stat: String,
    pub files: Vec<String>,
    pub test_command: Option<String>,
    pub test_output: String,
    pub risk_notes: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, coding_workspace_id, status, summary, status_short, \
                       diff_stat, files_json, test_command, test_output, risk_notes_json, \
                       created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<Review> {
    let files_json: String = row.get("files_json")?;
    let risk_json: String = row.get("risk_notes_json")?;
    Ok(Review {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        coding_workspace_id: row.get("coding_workspace_id")?,
        status: row.get("status")?,
        summary: row.get("summary")?,
        status_short: row.get("status_short")?,
        diff_stat: row.get("diff_stat")?,
        files: serde_json::from_str(&files_json).unwrap_or_default(),
        test_command: row.get("test_command")?,
        test_output: row.get("test_output")?,
        risk_notes: serde_json::from_str(&risk_json).unwrap_or_default(),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list_by_workspace(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<Review>> {
    let sql =
        format!("SELECT {COLUMNS} FROM reviews WHERE workspace_id = ?1 ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Review>> {
    let sql = format!("SELECT {COLUMNS} FROM reviews WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Insert a review snapshot. `files`/`risk_notes` are serialized to JSON columns.
/// The caller supplies `id` for consistency with the other models.
#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    coding_workspace_id: &str,
    summary: &str,
    status_short: &str,
    diff_stat: &str,
    files: &[String],
    test_command: Option<&str>,
    test_output: &str,
    risk_notes: &[String],
) -> rusqlite::Result<Review> {
    let now = now_rfc3339();
    let files_json = serde_json::to_string(files).unwrap_or_else(|_| "[]".to_string());
    let risk_json = serde_json::to_string(risk_notes).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO reviews
           (id, workspace_id, coding_workspace_id, status, summary, status_short, diff_stat,
            files_json, test_command, test_output, risk_notes_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            id,
            workspace_id,
            coding_workspace_id,
            summary,
            status_short,
            diff_stat,
            files_json,
            test_command,
            test_output,
            risk_json,
            now
        ],
    )?;
    Ok(get(conn, id)?.expect("review exists immediately after insert"))
}

pub fn update_status(
    conn: &Connection,
    id: &str,
    status: &str,
) -> rusqlite::Result<Option<Review>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE reviews SET status = ?2, updated_at = ?3 WHERE id = ?1",
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

    /// Returns (workspace_id, coding_workspace_id).
    fn fixtures(conn: &Connection) -> (String, String) {
        let ws = workspace::create(conn, "Test", "mixed").unwrap().id;
        let p = project::create(conn, &ws, "P", "code").unwrap().id;
        let r = repository::create(conn, &ws, "repo", "/tmp/repo", "main", None)
            .unwrap()
            .id;
        let cw_id = new_id();
        let cw = coding_workspace::create(
            conn,
            &cw_id,
            &ws,
            &p,
            &r,
            "/tmp/repo",
            &format!("/tmp/worktrees/{cw_id}"),
            "feature/x",
            "main",
        )
        .unwrap();
        (ws, cw.id)
    }

    fn make(conn: &Connection, ws: &str, cw: &str) -> Review {
        create(
            conn,
            &new_id(),
            ws,
            cw,
            "1 files changed, 2 insertions(+), 0 deletions(-)",
            " M README.md",
            " README.md | 2 +-",
            &["README.md".to_string()],
            Some("pnpm test"),
            "",
            &["Large change".to_string()],
        )
        .unwrap()
    }

    #[test]
    fn create_then_list_and_get() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        assert!(list_by_workspace(&conn, &ws).unwrap().is_empty());

        let review = make(&conn, &ws, &cw);
        assert_eq!(review.workspace_id, ws);
        assert_eq!(review.coding_workspace_id, cw);
        assert_eq!(review.status, "pending");
        assert_eq!(review.files, vec!["README.md".to_string()]);
        assert_eq!(review.risk_notes, vec!["Large change".to_string()]);
        assert_eq!(review.test_command.as_deref(), Some("pnpm test"));
        assert_eq!(review.test_output, "");

        assert_eq!(list_by_workspace(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &review.id).unwrap().is_some());
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn update_status_changes_verdict() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let review = make(&conn, &ws, &cw);

        let approved = update_status(&conn, &review.id, "approved")
            .unwrap()
            .expect("updated");
        assert_eq!(approved.status, "approved");
        assert!(update_status(&conn, "missing", "approved").unwrap().is_none());
    }

    #[test]
    fn deleting_coding_workspace_cascades_reviews() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let review = make(&conn, &ws, &cw);

        coding_workspace::delete(&conn, &cw).unwrap();
        assert!(get(&conn, &review.id).unwrap().is_none());
    }

    #[test]
    fn deleting_workspace_cascades_reviews() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let review = make(&conn, &ws, &cw);

        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &review.id).unwrap().is_none());
    }
}
