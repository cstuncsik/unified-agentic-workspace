//! Pure helpers for dispatch: extract candidate tasks from an artifact's markdown
//! and derive git-ref-safe branch names. No IO — unit-tested.

/// Candidate task titles from markdown: task-list items (`- [ ] x` / `- [x] x`)
/// if any exist, else `##`/`###` headings. Char-safe (no byte slicing).
pub fn extract_tasks(markdown: &str) -> Vec<String> {
    let mut checks: Vec<String> = Vec::new();
    let mut heads: Vec<String> = Vec::new();
    for raw in markdown.lines() {
        let line = raw.trim();
        if let Some(rest) = line
            .strip_prefix("- [ ] ")
            .or_else(|| line.strip_prefix("- [x] "))
            .or_else(|| line.strip_prefix("- [X] "))
        {
            let t = rest.trim();
            if !t.is_empty() {
                checks.push(t.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("### ").or_else(|| line.strip_prefix("## ")) {
            let t = rest.trim();
            if !t.is_empty() {
                heads.push(t.to_string());
            }
        }
    }
    if !checks.is_empty() {
        checks
    } else {
        heads
    }
}

// Branch-name slugging lives on the frontend (src/utils/slug.ts) to seed the
// editable dispatch rows; the backend only validates incoming branch names
// (validate_dispatch) and lets git reject anything else per-task.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkboxes_take_precedence_over_headings() {
        let md = "## Heading\n\n- [ ] First task\n- [x] Second done\n";
        assert_eq!(extract_tasks(md), vec!["First task", "Second done"]);
    }

    #[test]
    fn falls_back_to_headings() {
        let md = "# Title\n\n## Setup the repo\n\n### Add tests\n\nprose\n";
        assert_eq!(extract_tasks(md), vec!["Setup the repo", "Add tests"]);
    }

    #[test]
    fn empty_or_prose_yields_nothing() {
        assert!(extract_tasks("").is_empty());
        assert!(extract_tasks("just some prose\nwith lines").is_empty());
    }

    #[test]
    fn handles_non_ascii_without_panic() {
        // Multibyte content must not panic (char-safe parsing, no byte slicing).
        let md = "- [ ] Café déjà — vu\n";
        let tasks = extract_tasks(md);
        assert_eq!(tasks, vec!["Café déjà — vu"]);
    }
}
