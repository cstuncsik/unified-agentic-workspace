use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::project;
use crate::models::repository::{self, RepositorySource};
use crate::models::workspace;
use crate::services::git::{self, GitInspection};

/// Inspect a path with git without storing anything — drives the "attach repo"
/// form (is it a git repo, current/default branch, dirty state).
#[tauri::command]
pub fn validate_repository_path(path: String) -> Result<GitInspection, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Repository path cannot be empty".into());
    }
    Ok(git::inspect(Path::new(path)))
}

#[tauri::command]
pub fn list_repository_sources(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<RepositorySource>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    repository::list(&conn, &workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_repository_source(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<RepositorySource>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    repository::get(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_repository_source(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    name: String,
    local_path: String,
    project_id: Option<String>,
) -> Result<RepositorySource, String> {
    let name = name.trim();
    let local_path = local_path.trim();
    if name.is_empty() {
        return Err("Repository name cannot be empty".into());
    }
    if local_path.is_empty() {
        return Err("Repository path cannot be empty".into());
    }

    // Only attach actual git repositories.
    let inspection = git::inspect(Path::new(local_path));
    if !inspection.is_git_repo {
        return Err(inspection
            .error
            .unwrap_or_else(|| "Path is not a git repository".into()));
    }
    // Canonicalize to the repository root, so attaching a subdirectory of a repo
    // stores the repo itself.
    let stored_path = inspection
        .toplevel
        .clone()
        .unwrap_or_else(|| local_path.to_string());
    let default_branch = inspection
        .default_branch
        .or(inspection.current_branch)
        .unwrap_or_else(|| "main".to_string());

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

    // Canonicalization means two subdirectories of one repo resolve to the same
    // root, so guard against attaching the same repository twice.
    if repository::list(&conn, &workspace_id)
        .map_err(|e| e.to_string())?
        .iter()
        .any(|r| r.local_path == stored_path)
    {
        return Err("This repository is already attached to the workspace".into());
    }

    repository::create(
        &conn,
        &workspace_id,
        name,
        &stored_path,
        &default_branch,
        project_id.as_deref(),
    )
    .map_err(|e| e.to_string())
}

/// Live git status (current branch + dirty) for an attached repository.
#[tauri::command]
pub fn get_repository_status(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<GitInspection, String> {
    let path = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(repo) = repository::get(&conn, &id).map_err(|e| e.to_string())? else {
            return Err(format!("Repository '{id}' does not exist"));
        };
        repo.local_path
    };
    Ok(git::inspect(Path::new(&path)))
}

#[tauri::command]
pub fn list_repository_branches(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Vec<String>, String> {
    let path = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(repo) = repository::get(&conn, &id).map_err(|e| e.to_string())? else {
            return Err(format!("Repository '{id}' does not exist"));
        };
        repo.local_path
    };
    git::list_branches(Path::new(&path)).map_err(|e| format!("Could not read branches: {e}"))
}

#[tauri::command]
pub fn delete_repository_source(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<bool, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    repository::delete(&conn, &id).map_err(|e| e.to_string())
}
