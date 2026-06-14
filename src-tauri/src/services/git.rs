use std::path::Path;
use std::process::Command;

use serde::Serialize;

/// Result of inspecting a path with git. Used both when validating a path the
/// user wants to attach and when reporting the current status of an attached
/// repository.
#[derive(Debug, Clone, Serialize, Default)]
pub struct GitInspection {
    /// Whether the path is inside a git work tree.
    pub is_git_repo: bool,
    /// The checked-out branch (or `HEAD` when detached); `None` if unknown.
    pub current_branch: Option<String>,
    /// The repository's default branch, derived from `origin/HEAD` when a
    /// remote exists, otherwise a local `main`/`master`, otherwise the current
    /// branch.
    pub default_branch: Option<String>,
    /// Whether the work tree has uncommitted changes.
    pub is_dirty: bool,
    /// The repository root (`git rev-parse --show-toplevel`); lets us canonicalize
    /// a path that points at a subdirectory of a repo to the repo root.
    pub toplevel: Option<String>,
    /// A human-readable reason when `is_git_repo` is false.
    pub error: Option<String>,
}

/// Run a read-only `git -C <repo> <args...>` command and return trimmed stdout.
/// Only ever used for inspection — never for writes or running repo scripts.
fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Best-effort default branch: prefer the remote's `origin/HEAD`, then a local
/// `main`/`master`, then fall back to the current branch.
fn default_branch(repo: &Path) -> Option<String> {
    if let Ok(head) = run_git(repo, &["rev-parse", "--abbrev-ref", "origin/HEAD"]) {
        if let Some(branch) = head.strip_prefix("origin/") {
            // When origin/HEAD is unset git echoes "origin/HEAD" verbatim, which
            // would strip to "HEAD" — fall through to main/master/current instead.
            if !branch.is_empty() && branch != "HEAD" {
                return Some(branch.to_string());
            }
        }
    }
    for candidate in ["main", "master"] {
        if run_git(
            repo,
            &["show-ref", "--verify", &format!("refs/heads/{candidate}")],
        )
        .is_ok()
        {
            return Some(candidate.to_string());
        }
    }
    current_branch(repo)
}

/// The currently checked-out branch (or `HEAD` when detached).
pub fn current_branch(repo: &Path) -> Option<String> {
    run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|b| !b.is_empty())
}

/// Whether the work tree has any staged or unstaged changes.
pub fn is_dirty(repo: &Path) -> bool {
    run_git(repo, &["status", "--porcelain"])
        .map(|out| !out.is_empty())
        .unwrap_or(false)
}

/// Local branch names, ordered as git reports them.
pub fn list_branches(repo: &Path) -> Result<Vec<String>, String> {
    let out = run_git(repo, &["branch", "--format=%(refname:short)"])?;
    Ok(out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Inspect a path: is it a git work tree, and if so its branch/default/dirty state.
pub fn inspect(repo: &Path) -> GitInspection {
    match run_git(repo, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(out) if out == "true" => GitInspection {
            is_git_repo: true,
            current_branch: current_branch(repo),
            default_branch: default_branch(repo),
            is_dirty: is_dirty(repo),
            toplevel: run_git(repo, &["rev-parse", "--show-toplevel"]).ok(),
            error: None,
        },
        Ok(_) => GitInspection {
            is_git_repo: false,
            error: Some("Path is not inside a git work tree".to_string()),
            ..Default::default()
        },
        Err(e) => GitInspection {
            is_git_repo: false,
            error: Some(e),
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Create an isolated git repo in a temp dir with one commit on `main`.
    fn temp_repo() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("uaw-git-test-{}", crate::util::new_id()));
        fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@uaw.local"]);
        run(&["config", "user.name", "UAW Test"]);
        fs::write(dir.join("README.md"), "# temp\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "init"]);
        dir
    }

    #[test]
    fn inspect_reports_clean_git_repo() {
        let repo = temp_repo();
        let info = inspect(&repo);
        assert!(info.is_git_repo);
        assert_eq!(info.current_branch.as_deref(), Some("main"));
        assert_eq!(info.default_branch.as_deref(), Some("main"));
        assert!(!info.is_dirty);
        assert!(info.toplevel.is_some());
        assert!(info.error.is_none());
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn inspect_detects_dirty_work_tree() {
        let repo = temp_repo();
        fs::write(repo.join("README.md"), "# changed\n").unwrap();
        assert!(is_dirty(&repo));
        assert!(inspect(&repo).is_dirty);
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn inspect_rejects_non_git_path() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("uaw-not-git-{}", crate::util::new_id()));
        fs::create_dir_all(&dir).unwrap();
        let info = inspect(&dir);
        assert!(!info.is_git_repo);
        assert!(info.error.is_some());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_branches_includes_main_and_new_branch() {
        let repo = temp_repo();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["branch", "feature/x"])
            .status()
            .unwrap();
        let branches = list_branches(&repo).unwrap();
        assert!(branches.contains(&"main".to_string()));
        assert!(branches.contains(&"feature/x".to_string()));
        fs::remove_dir_all(&repo).ok();
    }
}
