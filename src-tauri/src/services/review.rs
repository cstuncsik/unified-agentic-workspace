//! Pure derivations over a `ReviewSnapshot`: the one-line summary and the
//! deterministic risk-flag list. No git or IO — unit-tested directly.

use crate::services::git::ReviewSnapshot;

/// Total changed lines above this flags a "Large change".
const LARGE_CHANGE_LINES: u32 = 300;
/// File count above this flags "Many files changed".
const MANY_FILES: usize = 20;
/// Dependency lock files worth calling out when touched.
const LOCK_FILES: [&str; 4] = [
    "Cargo.lock",
    "pnpm-lock.yaml",
    "package-lock.json",
    "yarn.lock",
];

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn is_migration(path: &str) -> bool {
    path.split('/').any(|seg| seg == "migrations")
}

/// A one-line size summary for the review header.
pub fn summarize(snapshot: &ReviewSnapshot) -> String {
    if snapshot.is_clean {
        return "No changes".to_string();
    }
    format!(
        "{} files changed, {} insertions(+), {} deletions(-)",
        snapshot.files.len(),
        snapshot.added_lines,
        snapshot.deleted_lines
    )
}

/// Deterministic, plain-text risk flags derived from the snapshot.
pub fn compute_risk_notes(snapshot: &ReviewSnapshot) -> Vec<String> {
    let mut notes = Vec::new();

    let total_lines = snapshot.added_lines + snapshot.deleted_lines;
    if total_lines > LARGE_CHANGE_LINES {
        notes.push(format!("Large change: {total_lines} lines changed"));
    }
    if snapshot.files.len() > MANY_FILES {
        notes.push(format!("Many files changed: {}", snapshot.files.len()));
    }
    if snapshot.files.iter().any(|f| is_migration(f)) {
        notes.push("Migration files changed".to_string());
    }
    if snapshot
        .files
        .iter()
        .any(|f| LOCK_FILES.contains(&basename(f)))
    {
        notes.push("Lock file changed".to_string());
    }
    if !snapshot.deleted.is_empty() {
        notes.push(format!("Files deleted: {}", snapshot.deleted.len()));
    }
    if snapshot.has_binary {
        notes.push("Binary or non-text changes".to_string());
    }

    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> ReviewSnapshot {
        ReviewSnapshot::default()
    }

    #[test]
    fn clean_snapshot_summarizes_as_no_changes() {
        let mut s = snapshot();
        s.is_clean = true;
        assert_eq!(summarize(&s), "No changes");
        assert!(compute_risk_notes(&s).is_empty());
    }

    #[test]
    fn summary_counts_files_and_lines() {
        let mut s = snapshot();
        s.files = vec!["a.rs".to_string(), "b.rs".to_string()];
        s.added_lines = 10;
        s.deleted_lines = 3;
        assert_eq!(summarize(&s), "2 files changed, 10 insertions(+), 3 deletions(-)");
    }

    #[test]
    fn large_change_flag_uses_total_lines() {
        let mut s = snapshot();
        s.files = vec!["a.rs".to_string()];
        s.added_lines = 250;
        s.deleted_lines = 100; // 350 total > 300
        assert!(compute_risk_notes(&s)
            .iter()
            .any(|n| n.starts_with("Large change")));
    }

    #[test]
    fn many_files_flag() {
        let mut s = snapshot();
        s.files = (0..21).map(|i| format!("f{i}.rs")).collect();
        assert!(compute_risk_notes(&s)
            .iter()
            .any(|n| n.starts_with("Many files changed")));
    }

    #[test]
    fn migration_flag() {
        let mut s = snapshot();
        s.files = vec!["src-tauri/src/db/migrations/0006_x.sql".to_string()];
        assert!(compute_risk_notes(&s)
            .iter()
            .any(|n| n == "Migration files changed"));
    }

    #[test]
    fn lockfile_flag() {
        let mut s = snapshot();
        s.files = vec!["frontend/pnpm-lock.yaml".to_string()];
        assert!(compute_risk_notes(&s).iter().any(|n| n == "Lock file changed"));
    }

    #[test]
    fn deleted_and_binary_flags() {
        let mut s = snapshot();
        s.files = vec!["old.rs".to_string(), "logo.png".to_string()];
        s.deleted = vec!["old.rs".to_string()];
        s.has_binary = true;
        let notes = compute_risk_notes(&s);
        assert!(notes.iter().any(|n| n.starts_with("Files deleted")));
        assert!(notes.iter().any(|n| n == "Binary or non-text changes"));
    }

    #[test]
    fn quiet_small_change_has_no_flags() {
        let mut s = snapshot();
        s.files = vec!["a.rs".to_string()];
        s.added_lines = 5;
        s.deleted_lines = 1;
        assert!(compute_risk_notes(&s).is_empty());
    }
}
