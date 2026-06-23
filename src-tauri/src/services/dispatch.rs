//! Dispatch helpers: extract candidate tasks from an artifact's markdown, assemble
//! the dispatched-task goal (pure), and the conn-testable goal resolver.

use rusqlite::Connection;

use crate::models::{artifact, coding_workspace, session};

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

/// Assemble the SDK agent's goal from a dispatched task: the task title as the
/// instruction, then the source artifact's content as plan context. Pure. The title
/// legitimately repeats inside the artifact (it was extracted from it); the `Task:`
/// line is what distinguishes N worktrees dispatched from one artifact.
pub fn format_dispatched_goal(task_title: &str, artifact_content: &str) -> String {
    format!(
        "Task: {}\n\nContext — the plan this task was dispatched from:\n\n{}",
        task_title.trim(),
        artifact_content
    )
}

/// Resolve the prefill goal for a (possibly dispatched) coding workspace:
/// `cw.session_id → session.title` + `session.created_from_artifact_id →
/// artifact.content`, assembled by `format_dispatched_goal`. Returns `None` for a
/// plain worktree or any incomplete/empty chain — seed only a real dispatched task
/// with content.
///
/// No workspace-scoping (unlike `list_account_models`, which gates cross-workspace
/// credential access): this reads only objects transitively owned by the cw
/// (cw → its session → that session's artifact), so there is no boundary to enforce.
pub fn resolve_dispatched_goal(
    conn: &Connection,
    coding_workspace_id: &str,
) -> rusqlite::Result<Option<String>> {
    let Some(cw) = coding_workspace::get(conn, coding_workspace_id)? else {
        return Ok(None);
    };
    let Some(session_id) = cw.session_id else {
        return Ok(None);
    };
    let Some(sess) = session::get(conn, &session_id)? else {
        return Ok(None);
    };
    let Some(artifact_id) = sess.created_from_artifact_id else {
        return Ok(None);
    };
    let Some(art) = artifact::get(conn, &artifact_id)? else {
        return Ok(None);
    };
    if sess.title.trim().is_empty() || art.content.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(format_dispatched_goal(&sess.title, &art.content)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{project, repository, workspace};
    use crate::util::new_id;

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

    #[test]
    fn formats_task_then_context_then_content() {
        let g = format_dispatched_goal("Add login", "## Steps\n- do it\n");
        assert_eq!(
            g,
            "Task: Add login\n\nContext — the plan this task was dispatched from:\n\n## Steps\n- do it\n"
        );
    }

    #[test]
    fn format_trims_title_and_keeps_multibyte() {
        let g = format_dispatched_goal("  Café déjà — vu  ", "café\n");
        assert_eq!(
            g,
            "Task: Café déjà — vu\n\nContext — the plan this task was dispatched from:\n\ncafé\n"
        );
    }

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::run_migrations(&mut conn).unwrap();
        conn
    }

    fn base(conn: &Connection) -> (String, String, String) {
        let ws = workspace::create(conn, "WS", "mixed").unwrap().id;
        let p = project::create(conn, &ws, "P", "code").unwrap().id;
        let r = repository::create(conn, &ws, "repo", "/tmp/repo", "main", None).unwrap().id;
        (ws, p, r)
    }

    fn cw_with_session(
        conn: &Connection,
        ws: &str,
        p: &str,
        r: &str,
        session_id: Option<&str>,
    ) -> String {
        let id = new_id();
        coding_workspace::create(
            conn, &id, ws, p, r, "/tmp/repo", &format!("/tmp/wt/{id}"), "feat/x", "main",
            session_id,
        )
        .unwrap();
        id
    }

    #[test]
    fn missing_workspace_has_no_goal() {
        let conn = migrated_conn();
        assert_eq!(resolve_dispatched_goal(&conn, "nope").unwrap(), None);
    }

    #[test]
    fn plain_worktree_has_no_goal() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        let cw = cw_with_session(&conn, &ws, &p, &r, None);
        assert_eq!(resolve_dispatched_goal(&conn, &cw).unwrap(), None);
    }

    #[test]
    fn dispatched_session_without_artifact_has_no_goal() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        // A session not born from an artifact (created_from_artifact_id = None).
        let sess = session::create(&conn, &ws, Some(&p), "Add login", "code", "todo", None).unwrap();
        let cw = cw_with_session(&conn, &ws, &p, &r, Some(&sess.id));
        assert_eq!(resolve_dispatched_goal(&conn, &cw).unwrap(), None);
    }

    #[test]
    fn empty_artifact_content_has_no_goal() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        let art = artifact::create(&conn, &ws, Some(&p), "Plan").unwrap(); // content starts ""
        let sess =
            session::create(&conn, &ws, Some(&p), "Add login", "code", "todo", Some(&art.id))
                .unwrap();
        let cw = cw_with_session(&conn, &ws, &p, &r, Some(&sess.id));
        assert_eq!(resolve_dispatched_goal(&conn, &cw).unwrap(), None);
    }

    #[test]
    fn empty_session_title_has_no_goal() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        let art = artifact::create(&conn, &ws, Some(&p), "Plan").unwrap();
        artifact::update(&conn, &art.id, "Plan", "## Steps\n").unwrap();
        let sess = session::create(&conn, &ws, Some(&p), "", "code", "todo", Some(&art.id)).unwrap();
        let cw = cw_with_session(&conn, &ws, &p, &r, Some(&sess.id));
        assert_eq!(resolve_dispatched_goal(&conn, &cw).unwrap(), None);
    }

    #[test]
    fn full_chain_seeds_task_plus_artifact() {
        let conn = migrated_conn();
        let (ws, p, r) = base(&conn);
        let art = artifact::create(&conn, &ws, Some(&p), "Plan").unwrap();
        artifact::update(&conn, &art.id, "Plan", "## Steps\n- do it\n").unwrap();
        let sess =
            session::create(&conn, &ws, Some(&p), "Add login", "code", "todo", Some(&art.id))
                .unwrap();
        let cw = cw_with_session(&conn, &ws, &p, &r, Some(&sess.id));
        assert_eq!(
            resolve_dispatched_goal(&conn, &cw).unwrap(),
            Some(
                "Task: Add login\n\nContext — the plan this task was dispatched from:\n\n## Steps\n- do it\n"
                    .to_string()
            )
        );
    }
}
