use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use tauri::{AppHandle, Manager, State};

use crate::models::coding_workspace::{self, CodingWorkspace};
use crate::models::event;
use crate::models::review::{self, Review};
use crate::models::{project, repository};
use crate::services::git::{self, WorktreeDiff};
use crate::services::{check, completion, review as review_svc};
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

/// Validated worktree creation, the single chokepoint used by both the M7 command
/// and dispatch. Invariants (do not drop): branch/base validated (option-injection
/// guard); the DB lock is released before git and re-acquired for the insert; the
/// on-disk worktree is removed if the row insert fails (no orphan).
pub fn create_worktree_inner(
    app: &AppHandle,
    state: &State<'_, Mutex<Connection>>,
    project_id: &str,
    repository_source_id: &str,
    base_branch: &str,
    branch_name: &str,
    session_id: Option<&str>,
) -> Result<CodingWorkspace, String> {
    let base_branch = base_branch.trim();
    let branch_name = branch_name.trim();
    if base_branch.is_empty() {
        return Err("Base branch cannot be empty".into());
    }
    if branch_name.is_empty() {
        return Err("Branch name cannot be empty".into());
    }
    if base_branch.starts_with('-') || branch_name.starts_with('-') {
        return Err("Branch names cannot start with '-'".into());
    }

    let (workspace_id, repo_path) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(project) = project::get(&conn, project_id).map_err(|e| e.to_string())? else {
            return Err(format!("Project '{project_id}' does not exist"));
        };
        let Some(repo) =
            repository::get(&conn, repository_source_id).map_err(|e| e.to_string())?
        else {
            return Err(format!("Repository '{repository_source_id}' does not exist"));
        };
        if repo.workspace_id != project.workspace_id {
            return Err("Repository and project belong to different workspaces".into());
        }
        (project.workspace_id, repo.local_path)
    };

    let id = new_id();
    let base = worktrees_base(app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("failed to create worktrees dir: {e}"))?;
    let worktree_path = base.join(&id);

    git::create_worktree(Path::new(&repo_path), &worktree_path, branch_name, base_branch)?;

    let worktree_str = worktree_path.to_string_lossy().to_string();
    let conn = state.lock().map_err(|e| e.to_string())?;
    match coding_workspace::create(
        &conn, &id, &workspace_id, project_id, repository_source_id, &repo_path, &worktree_str,
        branch_name, base_branch, session_id,
    ) {
        Ok(cw) => Ok(cw),
        Err(e) => {
            let _ = git::remove_worktree(Path::new(&repo_path), &worktree_path, true);
            Err(e.to_string())
        }
    }
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
    create_worktree_inner(
        &app, &state, &project_id, &repository_source_id, &base_branch, &branch_name, None,
    )
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

/// Maximum wall-clock time a configured check may run before it is killed.
const CHECK_TIMEOUT: Duration = Duration::from_secs(600);

/// The computed fields of a review: the diff snapshot plus (optionally) the
/// project's check outcome. No DB, no lock — the testable seam shared by
/// `complete_coding_workspace` and `recheck_coding_workspace`.
struct ReviewFields {
    summary: String,
    status_short: String,
    diff_stat: String,
    files: Vec<String>,
    test_output: String,
    risk_notes: Vec<String>,
    checks_ran: bool,
    checks_passed: bool,
}

fn compute_review_fields(
    worktree: &Path,
    test_command: Option<&str>,
    run_check: bool,
) -> Result<ReviewFields, String> {
    let snapshot = git::review_snapshot(worktree);
    if let Some(e) = snapshot.error {
        return Err(e);
    }
    let outcome = if run_check {
        match test_command {
            Some(cmd) => check::run_check(worktree, cmd, CHECK_TIMEOUT),
            None => check::CheckOutcome::not_run(),
        }
    } else {
        check::CheckOutcome::not_run()
    };
    let summary = review_svc::summarize(&snapshot);
    let risk_notes =
        completion::augment_risk_notes(review_svc::compute_risk_notes(&snapshot), &outcome);
    let test_output = completion::format_test_output(test_command.unwrap_or(""), &outcome);
    Ok(ReviewFields {
        summary,
        status_short: snapshot.status_short,
        diff_stat: snapshot.diff_stat,
        files: snapshot.files,
        test_output,
        risk_notes,
        checks_ran: outcome.ran,
        checks_passed: outcome.passed(),
    })
}

#[tauri::command]
pub fn complete_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    run_check: bool,
) -> Result<Review, String> {
    // Resolve the workspace + worktree path + configured command under the lock,
    // then release it before the (potentially slow) git + check work.
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

    let fields =
        compute_review_fields(Path::new(&worktree_path), test_command.as_deref(), run_check)?;

    let review_id = new_id();
    let payload = serde_json::json!({
        "coding_workspace_id": coding_workspace_id,
        "review_id": review_id,
        "checks_ran": fields.checks_ran,
        "checks_passed": fields.checks_passed,
    })
    .to_string();

    // Persist the review, status move, and event together under one lock.
    let conn = state.lock().map_err(|e| e.to_string())?;
    let review = review::create(
        &conn,
        &review_id,
        &workspace_id,
        &coding_workspace_id,
        &fields.summary,
        &fields.status_short,
        &fields.diff_stat,
        &fields.files,
        test_command.as_deref(),
        &fields.test_output,
        &fields.risk_notes,
    )
    .map_err(|e| e.to_string())?;
    coding_workspace::update_status(&conn, &coding_workspace_id, "needs-review")
        .map_err(|e| e.to_string())?;
    event::create(
        &conn,
        &new_id(),
        &workspace_id,
        "coding_workspace.completed",
        &payload,
    )
    .map_err(|e| e.to_string())?;

    Ok(review)
}

/// Re-run the project's check command for an existing review and update it in place.
/// The async second half of completion → review automation: the auto path creates the
/// review instantly (`run_check=false`), then calls this to fill in check results
/// without blocking the review's appearance.
#[tauri::command]
pub fn recheck_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    review_id: String,
) -> Result<Review, String> {
    let (worktree_path, test_command) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(review) = review::get(&conn, &review_id).map_err(|e| e.to_string())? else {
            return Err(format!("Review '{review_id}' does not exist"));
        };
        let Some(cw) = coding_workspace::get(&conn, &review.coding_workspace_id)
            .map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{}' does not exist",
                review.coding_workspace_id
            ));
        };
        let test_command = project::get(&conn, &cw.project_id)
            .map_err(|e| e.to_string())?
            .and_then(|p| project::test_command_from_settings(&p.settings_json));
        (cw.worktree_path, test_command)
    };

    let fields = compute_review_fields(Path::new(&worktree_path), test_command.as_deref(), true)?;

    let conn = state.lock().map_err(|e| e.to_string())?;
    review::update_results(
        &conn,
        &review_id,
        &fields.summary,
        &fields.status_short,
        &fields.diff_stat,
        &fields.files,
        test_command.as_deref(),
        &fields.test_output,
        &fields.risk_notes,
    )
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("Review '{review_id}' does not exist"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as PCommand;

    fn temp_git_repo() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("uaw-disp-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            assert!(PCommand::new("git").arg("-C").arg(&dir).args(args).status().unwrap().success());
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "t@uaw.local"]);
        run(&["config", "user.name", "T"]);
        std::fs::write(dir.join("README.md"), "# t\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "init"]);
        dir
    }

    /// Directly exercise git::create_worktree + the cleanup contract: if the row
    /// insert is bypassed/fails, the worktree must not linger. Here we create then
    /// remove to assert the cleanup primitive the inner fn relies on.
    #[test]
    fn worktree_cleanup_removes_dir() {
        let repo = temp_git_repo();
        let wt = std::env::temp_dir().join(format!("uaw-wt-{}", new_id()));
        git::create_worktree(&repo, &wt, "feat/cleanup", "main").unwrap();
        assert!(wt.exists());
        git::remove_worktree(&repo, &wt, true).unwrap();
        assert!(!wt.exists());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn compute_review_fields_runs_check_when_enabled() {
        let repo = temp_git_repo();
        std::fs::write(repo.join("new.txt"), "x\n").unwrap(); // dirty the worktree
        let pass = compute_review_fields(&repo, Some("true"), true).unwrap();
        assert!(pass.checks_ran);
        assert!(pass.checks_passed);
        let fail = compute_review_fields(&repo, Some("false"), true).unwrap();
        assert!(fail.checks_ran);
        assert!(!fail.checks_passed);
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn compute_review_fields_skips_check_when_disabled_or_no_command() {
        let repo = temp_git_repo();
        let disabled = compute_review_fields(&repo, Some("true"), false).unwrap();
        assert!(!disabled.checks_ran);
        let none = compute_review_fields(&repo, None, true).unwrap();
        assert!(!none.checks_ran);
        std::fs::remove_dir_all(&repo).ok();
    }
}
