use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, Manager, State};

use crate::models::coding_workspace::{self, CodingWorkspace};
use crate::models::{project, repository};
use crate::services::git::{self, WorktreeDiff};
use crate::util::new_id;

/// Base directory for generated worktrees. `UAW_WORKTREES_DIR` overrides it (used
/// by e2e); otherwise an app-controlled `<app_data_dir>/worktrees`.
fn worktrees_base(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("UAW_WORKTREES_DIR") {
        return Ok(PathBuf::from(dir));
    }
    app.path()
        .app_data_dir()
        .map(|d| d.join("worktrees"))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_coding_workspaces(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<CodingWorkspace>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    coding_workspace::list(&conn, &workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<CodingWorkspace>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    coding_workspace::get(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_coding_workspace(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    project_id: String,
    repository_source_id: String,
    base_branch: String,
    branch_name: String,
) -> Result<CodingWorkspace, String> {
    let base_branch = base_branch.trim();
    let branch_name = branch_name.trim();
    if base_branch.is_empty() {
        return Err("Base branch cannot be empty".into());
    }
    if branch_name.is_empty() {
        return Err("Branch name cannot be empty".into());
    }
    // A leading '-' would be parsed by `git worktree add` as an option (e.g.
    // `--lock`), so reject it (option injection from the renderer boundary).
    if base_branch.starts_with('-') || branch_name.starts_with('-') {
        return Err("Branch names cannot start with '-'".into());
    }

    // Resolve the project + repository and their shared workspace under the lock,
    // then release it before touching git.
    let (workspace_id, repo_path) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(project) = project::get(&conn, &project_id).map_err(|e| e.to_string())? else {
            return Err(format!("Project '{project_id}' does not exist"));
        };
        let Some(repo) =
            repository::get(&conn, &repository_source_id).map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Repository '{repository_source_id}' does not exist"
            ));
        };
        if repo.workspace_id != project.workspace_id {
            return Err("Repository and project belong to different workspaces".into());
        }
        (project.workspace_id, repo.local_path)
    };

    let id = new_id();
    let base = worktrees_base(&app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("failed to create worktrees dir: {e}"))?;
    let worktree_path = base.join(&id);

    git::create_worktree(
        Path::new(&repo_path),
        &worktree_path,
        branch_name,
        base_branch,
    )?;

    let worktree_str = worktree_path.to_string_lossy().to_string();
    let conn = state.lock().map_err(|e| e.to_string())?;
    match coding_workspace::create(
        &conn,
        &id,
        &workspace_id,
        &project_id,
        &repository_source_id,
        &repo_path,
        &worktree_str,
        branch_name,
        base_branch,
    ) {
        Ok(cw) => Ok(cw),
        Err(e) => {
            // Don't leave an orphaned worktree if the row couldn't be written.
            let _ = git::remove_worktree(Path::new(&repo_path), &worktree_path, true);
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub fn get_coding_workspace_diff(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<WorktreeDiff, String> {
    let worktree_path = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) = coding_workspace::get(&conn, &id).map_err(|e| e.to_string())? else {
            return Err(format!("Coding workspace '{id}' does not exist"));
        };
        cw.worktree_path
    };
    Ok(git::worktree_diff(Path::new(&worktree_path)))
}

#[tauri::command]
pub fn mark_coding_workspace_ready_for_review(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<CodingWorkspace>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    coding_workspace::update_status(&conn, &id, "needs-review").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn discard_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    id: String,
    force: bool,
) -> Result<bool, String> {
    let (repo_path, worktree_path) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) = coding_workspace::get(&conn, &id).map_err(|e| e.to_string())? else {
            return Err(format!("Coding workspace '{id}' does not exist"));
        };
        (cw.repo_path, cw.worktree_path)
    };

    // Never silently destroy uncommitted work: a dirty worktree requires force.
    let dirty = git::is_dirty(Path::new(&worktree_path));
    if dirty && !force {
        return Err("Worktree has uncommitted changes; confirm to discard them".into());
    }
    git::remove_worktree(
        Path::new(&repo_path),
        Path::new(&worktree_path),
        dirty || force,
    )?;

    let conn = state.lock().map_err(|e| e.to_string())?;
    coding_workspace::delete(&conn, &id).map_err(|e| e.to_string())
}
