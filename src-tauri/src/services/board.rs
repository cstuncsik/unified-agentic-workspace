//! Board derivations: the pure stage classifier and the conn-testable card
//! assembler (every field except live git health, which the command layers on).

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;

use crate::models::{agent_session, coding_workspace, project, repository, review};

/// The board column for a coding workspace. The latest review verdict dominates
/// the coding-workspace status; an unknown status defaults to in-progress.
pub fn board_stage(cw_status: &str, latest_review_status: Option<&str>) -> &'static str {
    match latest_review_status {
        Some("pending") => "needs-review",
        // changes-requested means the work bounced back — it resumes coding.
        Some("changes-requested") => "in-progress",
        Some("approved") | Some("done") | Some("rejected") => "reviewed",
        Some(_) => "in-progress",
        None => {
            if cw_status == "needs-review" {
                "needs-review"
            } else {
                "in-progress"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BoardCardBase {
    pub coding_workspace_id: String,
    pub branch_name: String,
    pub base_branch: String,
    pub project_name: String,
    pub repo_name: String,
    pub status: String,
    pub latest_review_status: Option<String>,
    pub agent_status: Option<String>,
    pub last_activity: String,
    pub stage: String,
}

/// Build the non-git board cards for a workspace, pairing each with its
/// `worktree_path` (used by the caller for live git health, then dropped — never
/// sent to the frontend). RFC3339 timestamps sort lexicographically, so the
/// string max gives the most recent activity.
pub fn assemble_cards(
    conn: &Connection,
    workspace_id: &str,
) -> rusqlite::Result<Vec<(BoardCardBase, String)>> {
    let cws = coding_workspace::list(conn, workspace_id)?;
    let mut project_names: HashMap<String, String> = HashMap::new();
    let mut repo_names: HashMap<String, String> = HashMap::new();
    let mut out = Vec::with_capacity(cws.len());

    for cw in cws {
        let project_name = match project_names.get(&cw.project_id) {
            Some(n) => n.clone(),
            None => {
                let n = project::get(conn, &cw.project_id)?
                    .map(|p| p.name)
                    .unwrap_or_else(|| "project".to_string());
                project_names.insert(cw.project_id.clone(), n.clone());
                n
            }
        };
        let repo_name = match repo_names.get(&cw.repository_source_id) {
            Some(n) => n.clone(),
            None => {
                let n = repository::get(conn, &cw.repository_source_id)?
                    .map(|r| r.name)
                    .unwrap_or_else(|| "repo".to_string());
                repo_names.insert(cw.repository_source_id.clone(), n.clone());
                n
            }
        };

        let latest_review = review::latest_for_coding_workspace(conn, &cw.id)?;
        let latest_review_status = latest_review.as_ref().map(|r| r.status.clone());

        let agents = agent_session::list_by_coding_workspace(conn, &cw.id)?;
        let latest_agent = agents.first();
        let agent_status = latest_agent.map(|a| a.status.clone());

        let mut last_activity = cw.updated_at.clone();
        if let Some(r) = latest_review.as_ref() {
            if r.updated_at > last_activity {
                last_activity = r.updated_at.clone();
            }
        }
        if let Some(a) = latest_agent {
            if a.updated_at > last_activity {
                last_activity = a.updated_at.clone();
            }
        }

        let stage = board_stage(&cw.status, latest_review_status.as_deref()).to_string();

        out.push((
            BoardCardBase {
                coding_workspace_id: cw.id,
                branch_name: cw.branch_name,
                base_branch: cw.base_branch,
                project_name,
                repo_name,
                status: cw.status,
                latest_review_status,
                agent_status,
                last_activity,
                stage,
            },
            cw.worktree_path,
        ))
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{project, repository, workspace};
    use crate::util::new_id;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::run_migrations(&mut conn).unwrap();
        conn
    }

    #[test]
    fn stage_matrix() {
        // No review: needs-review only when the workspace says so, else in-progress.
        assert_eq!(board_stage("worktree-created", None), "in-progress");
        assert_eq!(board_stage("needs-review", None), "needs-review");
        // Review verdict dominates.
        assert_eq!(board_stage("worktree-created", Some("pending")), "needs-review");
        assert_eq!(board_stage("needs-review", Some("changes-requested")), "in-progress");
        assert_eq!(board_stage("worktree-created", Some("approved")), "reviewed");
        assert_eq!(board_stage("worktree-created", Some("done")), "reviewed");
        assert_eq!(board_stage("worktree-created", Some("rejected")), "reviewed");
        // Unknown status → safe default.
        assert_eq!(board_stage("worktree-created", Some("weird")), "in-progress");
    }

    fn cw_fixture(conn: &Connection) -> (String, String) {
        let ws = workspace::create(conn, "WS", "mixed").unwrap().id;
        let p = project::create(conn, &ws, "Proj", "code").unwrap().id;
        let r = repository::create(conn, &ws, "Repo", "/tmp/repo", "main", None).unwrap().id;
        let id = new_id();
        coding_workspace::create(conn, &id, &ws, &p, &r, "/tmp/repo",
            &format!("/tmp/wt/{id}"), "feat/x", "main", None).unwrap();
        (ws, id)
    }

    #[test]
    fn assemble_empty_and_basic() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Empty", "mixed").unwrap().id;
        assert!(assemble_cards(&conn, &ws).unwrap().is_empty());

        let (ws2, cw) = cw_fixture(&conn);
        let cards = assemble_cards(&conn, &ws2).unwrap();
        assert_eq!(cards.len(), 1);
        let (base, path) = &cards[0];
        assert_eq!(base.coding_workspace_id, cw);
        assert_eq!(base.project_name, "Proj");
        assert_eq!(base.repo_name, "Repo");
        assert_eq!(base.stage, "in-progress"); // no review yet
        assert_eq!(base.latest_review_status, None);
        assert_eq!(base.agent_status, None);
        assert!(path.contains("/tmp/wt/"));
    }
}
