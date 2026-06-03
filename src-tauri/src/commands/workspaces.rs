use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::workspace::{self, Workspace};

#[tauri::command]
pub fn list_workspaces(state: State<'_, Mutex<Connection>>) -> Result<Vec<Workspace>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    workspace::list(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_workspace(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<Workspace>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    workspace::get(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_workspace(
    state: State<'_, Mutex<Connection>>,
    name: String,
    kind: Option<String>,
) -> Result<Workspace, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Workspace name cannot be empty".into());
    }
    let kind = kind.unwrap_or_else(|| "mixed".to_string());
    let conn = state.lock().map_err(|e| e.to_string())?;
    workspace::create(&conn, name, &kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_workspace(
    state: State<'_, Mutex<Connection>>,
    id: String,
    name: String,
    kind: Option<String>,
) -> Result<Option<Workspace>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Workspace name cannot be empty".into());
    }
    let conn = state.lock().map_err(|e| e.to_string())?;
    let Some(existing) = workspace::get(&conn, &id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let kind = kind.unwrap_or(existing.kind);
    workspace::update(&conn, &id, name, &kind).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_workspace(state: State<'_, Mutex<Connection>>, id: String) -> Result<bool, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    workspace::delete(&conn, &id).map_err(|e| e.to_string())
}
