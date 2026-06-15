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
/// An attached (e.g. cloned/third-party) repo is treated as untrusted, so we
/// neutralize config-driven code-execution vectors on every call:
///
/// - `core.fsmonitor=` — `git status` would otherwise run it as a hook command;
/// - `core.hooksPath=/dev/null` — disables repo hooks (e.g. `post-checkout` runs
///   during `worktree add`).
///
/// `git diff` callers additionally pass `--no-ext-diff --no-textconv`.
/// Args are passed as argv (never a shell), so there is no injection from paths
/// or branch names.
fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.fsmonitor=")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
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
    /// Set when the worktree could not be inspected (e.g. its path is gone).
    pub error: Option<String>,
}

/// A deterministic snapshot of a worktree's changes for a review record. Unlike
/// `WorktreeDiff` (which carries the full patch for display), this carries the
/// aggregates the review summary and risk heuristics need: the changed-file set
/// (tracked + untracked), which paths were deleted, total line counts, and
/// whether any change was binary.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ReviewSnapshot {
    /// Raw `git status --short` text.
    pub status_short: String,
    /// `git diff --stat HEAD` summary.
    pub diff_stat: String,
    /// Every changed path (tracked changes + untracked files), de-duplicated.
    pub files: Vec<String>,
    /// Paths reported deleted by status (porcelain `D` code).
    pub deleted: Vec<String>,
    /// Total added lines across tracked changes (numstat).
    pub added_lines: u32,
    /// Total deleted lines across tracked changes (numstat).
    pub deleted_lines: u32,
    /// True if any tracked change was binary (numstat reports `-`).
    pub has_binary: bool,
    /// True when there are no working-tree changes.
    pub is_clean: bool,
    /// Set when the worktree could not be inspected (e.g. its path is gone).
    pub error: Option<String>,
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
///
/// `changed_files` (from `status --porcelain`) lists every change incl. untracked
/// files, which `git diff HEAD` does not show — so the UI renders this list. The
/// diff invocations pass `--no-ext-diff --no-textconv` to prevent the repo's
/// config-driven diff drivers from executing arbitrary commands.
pub fn worktree_diff(worktree: &Path) -> WorktreeDiff {
    // If status itself fails the worktree is gone/broken — report it rather than
    // silently looking clean.
    let status = match run_git(worktree, &["status", "--porcelain"]) {
        Ok(s) => s,
        Err(e) => {
            return WorktreeDiff {
                is_clean: false,
                error: Some(e),
                ..Default::default()
            };
        }
    };
    let changed_files: Vec<String> = status
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let diff_stat = run_git(
        worktree,
        &["diff", "--no-ext-diff", "--no-textconv", "--stat", "HEAD"],
    )
    .unwrap_or_default();
    let diff_text = run_git(
        worktree,
        &["diff", "--no-ext-diff", "--no-textconv", "HEAD"],
    )
    .unwrap_or_default();
    let is_clean = changed_files.is_empty();
    WorktreeDiff {
        changed_files,
        diff_stat,
        diff_text,
        is_clean,
        error: None,
    }
}

/// Parse a `git status --short` path field. Lines are `XY <path>`; a rename is
/// `R  <old> -> <new>` — we take the new path. Returns `None` for blank lines.
///
/// `run_git` trims the entire output, so the leading space of the very first
/// status line (e.g. `" M file"`) can be stripped to `"M file"`. We detect both
/// the normal 3-char prefix (`XY<space>`) and the trimmed 2-char prefix
/// (`X<space>`) so path extraction is correct in either case.
fn porcelain_path(line: &str) -> Option<String> {
    if line.is_empty() {
        return None;
    }
    // Normal format: "XY PATH" — status is exactly 2 chars, separator at pos 2,
    // path at pos 3+. Trimmed first-line format: "X PATH" — separator at pos 1,
    // path at pos 2+ (leading status char was stripped by run_git's trim()).
    let path = if line.len() > 2 && line.as_bytes()[2] == b' ' {
        line[3..].trim_start()
    } else if line.len() > 1 && line.as_bytes()[1] == b' ' {
        line[2..].trim_start()
    } else {
        return None;
    };
    if path.is_empty() {
        return None;
    }
    let path = path.rsplit(" -> ").next().unwrap_or(path);
    Some(path.trim().to_string())
}

/// Collect a review snapshot for a worktree (relative to its HEAD). Mirrors
/// `worktree_diff`'s safety: a failing `status` returns an error rather than
/// silently looking clean, and every diff passes `--no-ext-diff --no-textconv`.
pub fn review_snapshot(worktree: &Path) -> ReviewSnapshot {
    let status_short = match run_git(worktree, &["status", "--short"]) {
        Ok(s) => s,
        Err(e) => {
            return ReviewSnapshot {
                is_clean: false,
                error: Some(e),
                ..Default::default()
            };
        }
    };

    let mut files: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    for line in status_short.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(path) = porcelain_path(line) {
            if !files.contains(&path) {
                files.push(path.clone());
            }
            // Porcelain status codes occupy the first two columns; a 'D' in
            // either means the path was deleted (staged or unstaged).
            let code = &line[..2.min(line.len())];
            if code.contains('D') {
                deleted.push(path);
            }
        }
    }

    let diff_stat = run_git(
        worktree,
        &["diff", "--no-ext-diff", "--no-textconv", "--stat", "HEAD"],
    )
    .unwrap_or_default();

    let numstat = run_git(
        worktree,
        &["diff", "--no-ext-diff", "--no-textconv", "--numstat", "HEAD"],
    )
    .unwrap_or_default();

    let mut added_lines: u32 = 0;
    let mut deleted_lines: u32 = 0;
    let mut has_binary = false;
    for line in numstat.lines() {
        let mut cols = line.split('\t');
        let added = cols.next().unwrap_or("");
        let removed = cols.next().unwrap_or("");
        // Binary files report "-\t-\t<path>".
        if added == "-" || removed == "-" {
            has_binary = true;
            continue;
        }
        added_lines += added.parse::<u32>().unwrap_or(0);
        deleted_lines += removed.parse::<u32>().unwrap_or(0);
    }

    let is_clean = files.is_empty();
    ReviewSnapshot {
        status_short,
        diff_stat,
        files,
        deleted,
        added_lines,
        deleted_lines,
        has_binary,
        is_clean,
        error: None,
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
        assert!(dirty.error.is_none());

        // Untracked files don't show in `git diff HEAD` but must still be listed
        // in changed_files so review never silently drops new files.
        fs::write(wt.join("brand_new.txt"), "hello\n").unwrap();
        let with_untracked = worktree_diff(&wt);
        assert!(!with_untracked.is_clean);
        assert!(with_untracked
            .changed_files
            .iter()
            .any(|f| f.contains("brand_new.txt")));

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
    fn review_snapshot_reports_files_deletions_and_line_counts() {
        let repo = temp_repo();
        let wt = std::env::temp_dir().join(format!("uaw-rs-{}", crate::util::new_id()));
        create_worktree(&repo, &wt, "feature/review", "main").unwrap();

        // A fresh worktree off main is clean.
        let clean = review_snapshot(&wt);
        assert!(clean.is_clean);
        assert!(clean.files.is_empty());
        assert!(clean.error.is_none());

        // Edit a tracked file (adds lines), add an untracked file, delete the
        // committed README.
        fs::write(wt.join("README.md"), "# changed\nmore\n").unwrap();
        let snap_edit = review_snapshot(&wt);
        assert!(!snap_edit.is_clean);
        assert!(snap_edit.files.iter().any(|f| f.contains("README.md")));
        assert!(snap_edit.added_lines > 0);

        fs::write(wt.join("new.txt"), "hello\n").unwrap();
        fs::remove_file(wt.join("README.md")).unwrap();
        let snap = review_snapshot(&wt);
        assert!(snap.files.iter().any(|f| f.contains("new.txt")));
        assert!(
            snap.deleted.iter().any(|f| f.contains("README.md")),
            "deleted README should be reported in `deleted`, got {:?}",
            snap.deleted
        );

        remove_worktree(&repo, &wt, true).unwrap();
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
