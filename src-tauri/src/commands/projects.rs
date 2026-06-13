use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::project::{self, Project, PROJECT_MODES};
use crate::models::workspace;

fn ensure_mode(mode: &str) -> Result<(), String> {
    if PROJECT_MODES.contains(&mode) {
        Ok(())
    } else {
        Err(format!(
            "Invalid project mode '{mode}'; expected one of: {}",
            PROJECT_MODES.join(", ")
        ))
    }
}

#[tauri::command]
pub fn list_projects(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<Project>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    project::list(&conn, &workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_project(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<Project>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    project::get(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_project(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    name: String,
    mode: Option<String>,
) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    let mode = mode.unwrap_or_else(|| "research".to_string());
    ensure_mode(&mode)?;
    let conn = state.lock().map_err(|e| e.to_string())?;
    if workspace::get(&conn, &workspace_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("Workspace '{workspace_id}' does not exist"));
    }
    project::create(&conn, &workspace_id, name, &mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_project(
    state: State<'_, Mutex<Connection>>,
    id: String,
    name: String,
    mode: Option<String>,
) -> Result<Option<Project>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Project name cannot be empty".into());
    }
    let conn = state.lock().map_err(|e| e.to_string())?;
    let Some(existing) = project::get(&conn, &id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let mode = mode.unwrap_or(existing.mode);
    ensure_mode(&mode)?;
    project::update(&conn, &id, name, &mode).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project(state: State<'_, Mutex<Connection>>, id: String) -> Result<bool, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    project::delete(&conn, &id).map_err(|e| e.to_string())
}
