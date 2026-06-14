use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::{new_id, now_rfc3339};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositorySource {
    pub id: String,
    pub workspace_id: String,
    pub project_id: Option<String>,
    pub name: String,
    pub local_path: String,
    pub default_branch: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, project_id, name, local_path, default_branch, \
                       enabled, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<RepositorySource> {
    Ok(RepositorySource {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        project_id: row.get("project_id")?,
        name: row.get("name")?,
        local_path: row.get("local_path")?,
        default_branch: row.get("default_branch")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<RepositorySource>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM repository_sources WHERE workspace_id = ?1
         ORDER BY created_at ASC, name ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<RepositorySource>> {
    let sql = format!("SELECT {COLUMNS} FROM repository_sources WHERE id = ?1");
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
    local_path: &str,
    default_branch: &str,
    project_id: Option<&str>,
) -> rusqlite::Result<RepositorySource> {
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO repository_sources
           (id, workspace_id, project_id, name, local_path, default_branch, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
        params![id, workspace_id, project_id, name, local_path, default_branch, now],
    )?;
    Ok(get(conn, &id)?.expect("repository source exists immediately after insert"))
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM repository_sources WHERE id = ?1", params![id])?;
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

        let repo = create(&conn, &ws, "uaw", "/tmp/uaw", "main", None).unwrap();
        assert_eq!(repo.workspace_id, ws);
        assert_eq!(repo.name, "uaw");
        assert_eq!(repo.local_path, "/tmp/uaw");
        assert_eq!(repo.default_branch, "main");
        assert!(repo.enabled);
        assert_eq!(repo.project_id, None);

        assert_eq!(list(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &repo.id).unwrap().is_some());
        assert!(get(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn list_is_scoped_to_workspace() {
        let conn = migrated_conn();
        let ws_a = workspace_id(&conn);
        let ws_b = workspace_id(&conn);
        create(&conn, &ws_a, "a", "/tmp/a", "main", None).unwrap();

        assert_eq!(list(&conn, &ws_a).unwrap().len(), 1);
        assert!(list(&conn, &ws_b).unwrap().is_empty());
    }

    #[test]
    fn delete_removes_repository() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let repo = create(&conn, &ws, "temp", "/tmp/temp", "main", None).unwrap();

        assert!(delete(&conn, &repo.id).unwrap());
        assert!(list(&conn, &ws).unwrap().is_empty());
        assert!(!delete(&conn, &repo.id).unwrap());
    }

    #[test]
    fn deleting_workspace_cascades_but_project_detaches() {
        let conn = migrated_conn();
        let ws = workspace_id(&conn);
        let p = project::create(&conn, &ws, "P", "code").unwrap();
        let repo = create(&conn, &ws, "scoped", "/tmp/scoped", "main", Some(&p.id)).unwrap();

        // Deleting the project detaches the repo (project_id -> NULL) but keeps it.
        project::delete(&conn, &p.id).unwrap();
        let detached = get(&conn, &repo.id)
            .unwrap()
            .expect("repo survives project delete");
        assert_eq!(detached.project_id, None);

        // Deleting the workspace cascades the repo away.
        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &repo.id).unwrap().is_none());
    }
}
