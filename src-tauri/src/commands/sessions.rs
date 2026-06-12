use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::project;
use crate::models::session::{self, Session, SESSION_MODES, SESSION_STATUSES};
use crate::models::workspace;

fn ensure_mode(mode: &str) -> Result<(), String> {
    if SESSION_MODES.contains(&mode) {
        Ok(())
    } else {
        Err(format!(
            "Invalid session mode '{mode}'; expected one of: {}",
            SESSION_MODES.join(", ")
        ))
    }
}

fn ensure_status(status: &str) -> Result<(), String> {
    if SESSION_STATUSES.contains(&status) {
        Ok(())
    } else {
        Err(format!(
            "Invalid session status '{status}'; expected one of: {}",
            SESSION_STATUSES.join(", ")
        ))
    }
}

#[tauri::command]
pub fn list_sessions(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    project_id: Option<String>,
) -> Result<Vec<Session>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    match project_id {
        Some(project_id) => {
            session::list_by_project(&conn, &workspace_id, &project_id).map_err(|e| e.to_string())
        }
        None => session::list(&conn, &workspace_id).map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub fn get_session(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<Session>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    session::get(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_session(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    title: String,
    mode: String,
    project_id: Option<String>,
    status: Option<String>,
) -> Result<Session, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Session title cannot be empty".into());
    }
    ensure_mode(&mode)?;
    let status = status.unwrap_or_else(|| "todo".to_string());
    ensure_status(&status)?;

    let conn = state.lock().map_err(|e| e.to_string())?;
    if workspace::get(&conn, &workspace_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("Workspace '{workspace_id}' does not exist"));
    }
    if let Some(ref project_id) = project_id {
        let Some(project) = project::get(&conn, project_id).map_err(|e| e.to_string())? else {
            return Err(format!("Project '{project_id}' does not exist"));
        };
        if project.workspace_id != workspace_id {
            return Err("Project belongs to a different workspace".into());
        }
    }
    session::create(
        &conn,
        &workspace_id,
        project_id.as_deref(),
        title,
        &mode,
        &status,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_session(
    state: State<'_, Mutex<Connection>>,
    id: String,
    title: String,
    summary: Option<String>,
) -> Result<Option<Session>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Session title cannot be empty".into());
    }
    let conn = state.lock().map_err(|e| e.to_string())?;
    let Some(existing) = session::get(&conn, &id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    // An omitted summary means "keep the existing one", matching update_project/update_workspace.
    let summary = summary.or(existing.summary);
    session::update(&conn, &id, title, summary.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_session_status(
    state: State<'_, Mutex<Connection>>,
    id: String,
    status: String,
) -> Result<Option<Session>, String> {
    ensure_status(&status)?;
    let conn = state.lock().map_err(|e| e.to_string())?;
    session::update_status(&conn, &id, &status).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_session(state: State<'_, Mutex<Connection>>, id: String) -> Result<bool, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    session::delete(&conn, &id).map_err(|e| e.to_string())
}
