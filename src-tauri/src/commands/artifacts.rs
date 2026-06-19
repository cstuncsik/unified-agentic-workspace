use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::artifact::{self, Artifact};
use crate::models::{project, workspace};

/// Trim + non-empty title check, mirroring create_session/create_project.
fn validate_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Artifact title cannot be empty".into());
    }
    Ok(title.to_string())
}

/// Validate a new artifact against the DB: title non-empty, the workspace exists,
/// and any provided project belongs to that workspace. Returns the trimmed title.
fn validate_create(
    conn: &Connection,
    workspace_id: &str,
    project_id: Option<&str>,
    title: &str,
) -> Result<String, String> {
    let title = validate_title(title)?;
    if workspace::get(conn, workspace_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("Workspace '{workspace_id}' does not exist"));
    }
    if let Some(project_id) = project_id {
        let Some(project) = project::get(conn, project_id).map_err(|e| e.to_string())? else {
            return Err(format!("Project '{project_id}' does not exist"));
        };
        if project.workspace_id != workspace_id {
            return Err("Project belongs to a different workspace".into());
        }
    }
    Ok(title)
}

#[tauri::command]
pub fn list_artifacts(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<Artifact>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    artifact::list(&conn, &workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_artifact(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    project_id: Option<String>,
    title: String,
) -> Result<Artifact, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    let title = validate_create(&conn, &workspace_id, project_id.as_deref(), &title)?;
    artifact::create(&conn, &workspace_id, project_id.as_deref(), &title).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_artifact(
    state: State<'_, Mutex<Connection>>,
    id: String,
    title: String,
    content: String,
) -> Result<Option<Artifact>, String> {
    let title = validate_title(&title)?;
    let conn = state.lock().map_err(|e| e.to_string())?;
    artifact::update(&conn, &id, &title, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_artifact(state: State<'_, Mutex<Connection>>, id: String) -> Result<bool, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    artifact::delete(&conn, &id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::workspace;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn validate_title_rejects_blank_and_whitespace() {
        assert!(validate_title("").is_err());
        assert!(validate_title("   ").is_err());
        assert_eq!(validate_title("  Hi  ").unwrap(), "Hi");
    }

    #[test]
    fn validate_create_enforces_workspace_and_project_scope() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "WS", "mixed").unwrap().id;
        let p = project::create(&conn, &ws, "P", "research").unwrap().id;

        // Happy path.
        assert!(validate_create(&conn, &ws, Some(&p), "Doc").is_ok());
        assert!(validate_create(&conn, &ws, None, "Doc").is_ok());

        // Empty title / missing workspace / cross-workspace project all rejected.
        assert!(validate_create(&conn, &ws, None, "  ").is_err());
        assert!(validate_create(&conn, "nope", None, "Doc").is_err());

        let other_ws = workspace::create(&conn, "Other", "mixed").unwrap().id;
        let other_p = project::create(&conn, &other_ws, "OP", "research").unwrap().id;
        let err = validate_create(&conn, &ws, Some(&other_p), "Doc").unwrap_err();
        assert!(err.contains("different workspace"), "got: {err}");
    }
}
