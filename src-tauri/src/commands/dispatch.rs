use std::sync::Mutex;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::coding_workspaces::create_worktree_inner;
use crate::models::session::{self, Session};
use crate::models::{artifact, project, repository};
use crate::services::dispatch as svc;

#[derive(Debug, Clone, Deserialize)]
pub struct DispatchTask {
    pub title: String,
    pub branch_name: String,
    pub include: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DispatchTaskResult {
    pub title: String,
    pub session_id: String,
    pub coding_workspace_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DispatchResult {
    pub results: Vec<DispatchTaskResult>,
}

#[tauri::command]
pub fn extract_artifact_tasks(
    state: State<'_, Mutex<Connection>>,
    artifact_id: String,
) -> Result<Vec<String>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    let Some(art) = artifact::get(&conn, &artifact_id).map_err(|e| e.to_string())? else {
        return Err(format!("Artifact '{artifact_id}' does not exist"));
    };
    Ok(svc::extract_tasks(&art.content))
}

#[tauri::command]
pub fn list_artifact_sessions(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    artifact_id: String,
) -> Result<Vec<Session>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    session::list_by_artifact(&conn, &workspace_id, &artifact_id).map_err(|e| e.to_string())
}

/// Structural validation of a dispatch (conn-testable, no Tauri State). Resolves
/// the workspace/project/repo and validates base + every included task's branch +
/// rejects duplicate branch names BEFORE any side effects.
pub fn validate_dispatch(
    conn: &Connection,
    workspace_id: &str,
    project_id: &str,
    repository_source_id: &str,
    base_branch: &str,
    tasks: &[DispatchTask],
) -> Result<(), String> {
    if base_branch.trim().is_empty() {
        return Err("Base branch cannot be empty".into());
    }
    let Some(proj) = project::get(conn, project_id).map_err(|e| e.to_string())? else {
        return Err(format!("Project '{project_id}' does not exist"));
    };
    if proj.workspace_id != workspace_id {
        return Err("Project belongs to a different workspace".into());
    }
    let Some(repo) = repository::get(conn, repository_source_id).map_err(|e| e.to_string())? else {
        return Err(format!("Repository '{repository_source_id}' does not exist"));
    };
    if repo.workspace_id != workspace_id {
        return Err("Repository belongs to a different workspace".into());
    }

    let mut seen: Vec<&str> = Vec::new();
    for task in tasks.iter().filter(|t| t.include) {
        let b = task.branch_name.trim();
        if b.is_empty() {
            return Err(format!("Task '{}' has an empty branch name", task.title));
        }
        if b.starts_with('-') {
            return Err(format!("Branch '{b}' cannot start with '-'"));
        }
        if seen.contains(&b) {
            return Err(format!("Duplicate branch name '{b}' among the selected tasks"));
        }
        seen.push(b);
    }
    Ok(())
}

#[tauri::command]
pub fn dispatch_artifact(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    artifact_id: String,
    project_id: String,
    repository_source_id: String,
    base_branch: String,
    tasks: Vec<DispatchTask>,
) -> Result<DispatchResult, String> {
    // Resolve the artifact + its workspace, then validate structurally.
    let workspace_id = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(art) = artifact::get(&conn, &artifact_id).map_err(|e| e.to_string())? else {
            return Err(format!("Artifact '{artifact_id}' does not exist"));
        };
        validate_dispatch(
            &conn, &art.workspace_id, &project_id, &repository_source_id, &base_branch, &tasks,
        )?;
        art.workspace_id
    };

    let mut results = Vec::new();
    for task in tasks.into_iter().filter(|t| t.include) {
        // Step 1: the session always gets created and linked to the artifact.
        let session = {
            let conn = state.lock().map_err(|e| e.to_string())?;
            session::create(
                &conn, &workspace_id, Some(&project_id), task.title.trim(), "code", "todo",
                Some(&artifact_id),
            )
            .map_err(|e| e.to_string())?
        };

        // Step 2: best-effort worktree, linked to the session.
        match create_worktree_inner(
            &app, &state, &project_id, &repository_source_id, &base_branch, task.branch_name.trim(),
            Some(&session.id),
        ) {
            Ok(cw) => {
                let conn = state.lock().map_err(|e| e.to_string())?;
                let _ = session::update_status(&conn, &session.id, "worktree-created");
                results.push(DispatchTaskResult {
                    title: task.title,
                    session_id: session.id,
                    coding_workspace_id: Some(cw.id),
                    error: None,
                });
            }
            Err(e) => results.push(DispatchTaskResult {
                title: task.title,
                session_id: session.id,
                coding_workspace_id: None,
                error: Some(e),
            }),
        }
    }

    Ok(DispatchResult { results })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::workspace;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::run_migrations(&mut conn).unwrap();
        conn
    }
    fn task(title: &str, branch: &str, include: bool) -> DispatchTask {
        DispatchTask { title: title.into(), branch_name: branch.into(), include }
    }

    #[test]
    fn validate_dispatch_happy_and_rejections() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "WS", "mixed").unwrap().id;
        let p = project::create(&conn, &ws, "P", "code").unwrap().id;
        let r = repository::create(&conn, &ws, "repo", "/tmp/repo", "main", None).unwrap().id;

        assert!(validate_dispatch(&conn, &ws, &p, &r, "main", &[task("A", "feat/a", true)]).is_ok());
        // deselected tasks are ignored.
        assert!(validate_dispatch(&conn, &ws, &p, &r, "main", &[task("A", "", false)]).is_ok());

        assert!(validate_dispatch(&conn, &ws, &p, &r, "", &[task("A", "feat/a", true)]).is_err());
        assert!(validate_dispatch(&conn, &ws, "nope", &r, "main", &[task("A", "x", true)]).is_err());
        assert!(validate_dispatch(&conn, &ws, &p, &r, "main", &[task("A", "", true)]).is_err());
        assert!(validate_dispatch(&conn, &ws, &p, &r, "main", &[task("A", "-x", true)]).is_err());
        let dup = [task("A", "feat/x", true), task("B", "feat/x", true)];
        assert!(validate_dispatch(&conn, &ws, &p, &r, "main", &dup)
            .unwrap_err()
            .contains("Duplicate"));

        // Cross-workspace repo rejected.
        let ws2 = workspace::create(&conn, "WS2", "mixed").unwrap().id;
        let r2 = repository::create(&conn, &ws2, "r2", "/tmp/r2", "main", None).unwrap().id;
        assert!(validate_dispatch(&conn, &ws, &p, &r2, "main", &[task("A", "x", true)]).is_err());
    }
}
