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
///
/// `-c core.fsmonitor=` overrides any `core.fsmonitor` set in the target repo's
/// `.git/config`. `git status` treats that setting as a hook command and would
/// otherwise execute it — so an attached (e.g. cloned) repo could run arbitrary
/// code during inspection. Args are passed as argv (never through a shell), so
/// there is no injection from the path or branch names.
fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=")
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

/// The currently checked-out branch, or `None` when detached. `git rev-parse
/// --abbrev-ref HEAD` returns the literal token "HEAD" in a detached state; we
/// treat that as "no branch" rather than storing/showing "HEAD" as a branch.
pub fn current_branch(repo: &Path) -> Option<String> {
    run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|b| !b.is_empty() && b != "HEAD")
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

/// The working-tree changes inside a coding worktree, for review.
#[derive(Debug, Clone, Serialize, Default)]
pub struct WorktreeDiff {
    /// `git status --porcelain` lines (status code + path), incl. untracked.
    pub changed_files: Vec<String>,
    /// `git diff --stat HEAD` summary.
    pub diff_stat: String,
    /// Full unified `git diff HEAD` patch (tracked changes).
    pub diff_text: String,
    /// True when there are no working-tree changes.
    pub is_clean: bool,
}

/// Create an isolated worktree on a new branch: `git worktree add -b <branch>
/// <worktree_path> <base>`. The branch is created off `base`.
pub fn create_worktree(
    repo: &Path,
    worktree_path: &Path,
    branch: &str,
    base: &str,
) -> Result<(), String> {
    let wt = worktree_path.to_string_lossy();
    run_git(repo, &["worktree", "add", "-b", branch, &wt, base]).map(|_| ())
}

/// Remove a worktree: `git worktree remove [--force] <worktree_path>`. The branch
/// is left intact (so the work is recoverable); only the working tree is removed.
pub fn remove_worktree(repo: &Path, worktree_path: &Path, force: bool) -> Result<(), String> {
    let wt = worktree_path.to_string_lossy().to_string();
    let mut args: Vec<&str> = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&wt);
    run_git(repo, &args).map(|_| ())
}

/// Collect the working-tree changes in a worktree (relative to its HEAD).
pub fn worktree_diff(worktree: &Path) -> WorktreeDiff {
    let changed_files: Vec<String> = run_git(worktree, &["status", "--porcelain"])
        .map(|o| {
            o.lines()
                .map(|l| l.trim_end().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let diff_stat = run_git(worktree, &["diff", "--stat", "HEAD"]).unwrap_or_default();
    let diff_text = run_git(worktree, &["diff", "HEAD"]).unwrap_or_default();
    let is_clean = changed_files.is_empty();
    WorktreeDiff {
        changed_files,
        diff_stat,
        diff_text,
        is_clean,
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

    fn git(repo: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn detached_head_is_not_reported_as_a_branch() {
        let repo = temp_repo();
        fs::write(repo.join("README.md"), "# more\n").unwrap();
        git(&repo, &["commit", "-am", "second"]);
        git(&repo, &["checkout", "--detach", "HEAD~1"]);

        let info = inspect(&repo);
        assert!(info.is_git_repo);
        // Detached HEAD must not surface the literal "HEAD" as a branch.
        assert_eq!(info.current_branch, None);
        // default_branch still resolves to the local main, never "HEAD".
        assert_eq!(info.default_branch.as_deref(), Some("main"));
        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn worktree_create_diff_and_remove_lifecycle() {
        let repo = temp_repo();
        let wt = std::env::temp_dir().join(format!("uaw-wt-{}", crate::util::new_id()));

        create_worktree(&repo, &wt, "feature/x", "main").unwrap();
        assert!(wt.exists(), "worktree directory should be created");
        assert_eq!(current_branch(&wt).as_deref(), Some("feature/x"));

        // A fresh worktree off main is clean.
        let clean = worktree_diff(&wt);
        assert!(clean.is_clean);
        assert!(clean.changed_files.is_empty());

        // Editing a file makes it dirty and shows up in the diff.
        fs::write(wt.join("README.md"), "# changed in worktree\n").unwrap();
        let dirty = worktree_diff(&wt);
        assert!(!dirty.is_clean);
        assert!(dirty.changed_files.iter().any(|f| f.contains("README.md")));
        assert!(dirty.diff_text.contains("changed in worktree"));

        // Removing a dirty worktree requires force; the branch survives.
        assert!(remove_worktree(&repo, &wt, false).is_err());
        remove_worktree(&repo, &wt, true).unwrap();
        assert!(!wt.exists());
        assert!(list_branches(&repo)
            .unwrap()
            .contains(&"feature/x".to_string()));

        fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn inspect_canonicalizes_a_subdirectory_to_the_repo_root() {
        let repo = temp_repo();
        let sub = repo.join("nested/dir");
        fs::create_dir_all(&sub).unwrap();

        let info = inspect(&sub);
        assert!(info.is_git_repo);
        let top = info.toplevel.expect("toplevel resolved");
        assert_eq!(
            fs::canonicalize(&top).unwrap(),
            fs::canonicalize(&repo).unwrap(),
            "subdirectory should canonicalize to the repo root"
        );
        fs::remove_dir_all(&repo).ok();
    }
}
