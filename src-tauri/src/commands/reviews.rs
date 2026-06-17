use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::review::{self, Review};
use crate::models::{coding_workspace, project};
use crate::services::{git, review as review_svc};
use crate::util::new_id;

/// Verdict states a review may hold. `pending` is the initial state; the rest are
/// user-set. Anything else is rejected by `update_review_status`.
const REVIEW_STATUSES: [&str; 5] = ["pending", "approved", "rejected", "changes-requested", "done"];

fn is_valid_status(status: &str) -> bool {
    REVIEW_STATUSES.contains(&status)
}

#[tauri::command]
pub fn list_reviews(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<Review>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    review::list_by_workspace(&conn, &workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_review(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<Review>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    review::get(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_review_for_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
) -> Result<Review, String> {
    // Resolve the coding workspace (for its workspace id + worktree path) and the
    // owning project's configured test command under the lock, then release it
    // before touching git.
    let (workspace_id, worktree_path, test_command) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) =
            coding_workspace::get(&conn, &coding_workspace_id).map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{coding_workspace_id}' does not exist"
            ));
        };
        let test_command = project::get(&conn, &cw.project_id)
            .map_err(|e| e.to_string())?
            .and_then(|p| project::test_command_from_settings(&p.settings_json));
        (cw.workspace_id, cw.worktree_path, test_command)
    };

    let snapshot = git::review_snapshot(Path::new(&worktree_path));
    if let Some(e) = snapshot.error {
        return Err(e);
    }
    let summary = review_svc::summarize(&snapshot);
    let risk_notes = review_svc::compute_risk_notes(&snapshot);

    let id = new_id();
    let conn = state.lock().map_err(|e| e.to_string())?;
    review::create(
        &conn,
        &id,
        &workspace_id,
        &coding_workspace_id,
        &summary,
        &snapshot.status_short,
        &snapshot.diff_stat,
        &snapshot.files,
        test_command.as_deref(),
        "",
        &risk_notes,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_review_status(
    state: State<'_, Mutex<Connection>>,
    id: String,
    status: String,
) -> Result<Option<Review>, String> {
    if !is_valid_status(&status) {
        return Err(format!("Unknown review status '{status}'"));
    }
    let conn = state.lock().map_err(|e| e.to_string())?;
    review::update_status(&conn, &id, &status).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_status_set() {
        assert!(is_valid_status("pending"));
        assert!(is_valid_status("approved"));
        assert!(is_valid_status("changes-requested"));
        assert!(!is_valid_status("bogus"));
        assert!(!is_valid_status(""));
    }
}
