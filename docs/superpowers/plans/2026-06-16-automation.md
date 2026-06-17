# First Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a one-click "Complete and review" automation that runs a project's configured check command in the worktree, captures the result into a review with risk flags, records an event, and moves the workspace to Needs Review.

**Architecture:** A new `complete_coding_workspace` command orchestrates existing M8 pieces (`git::review_snapshot`, `services::review`, `review::create`) with two new services (`check` runs the shell command with a timeout; `completion` formats output + augments risk notes), a new `events` record, and per-project test-command config in `settings_json`. Pure logic lives in services/model helpers (unit-testable); the command does the lock dance + IO.

**Tech Stack:** Rust + rusqlite + serde_json + `std::process` (sh), Tauri 2 commands, Vue 3 + Pinia, WebdriverIO e2e.

---

## File structure

Backend:
- `src-tauri/src/db/migrations/0006_events.sql` — events table (create)
- `src-tauri/src/db/mod.rs` — register migration #6 (modify)
- `src-tauri/src/models/event.rs` — Event model + CRUD + tests (create)
- `src-tauri/src/models/mod.rs` — `pub mod event;` (modify)
- `src-tauri/src/models/review.rs` — `create` gains `test_output` param (modify)
- `src-tauri/src/models/project.rs` — `update_settings_json` + `test_command_from_settings` + `merge_test_command` + tests (modify)
- `src-tauri/src/services/check.rs` — `CheckOutcome` + `run_check` + tests (create)
- `src-tauri/src/services/completion.rs` — `format_test_output` + `augment_risk_notes` + tests (create)
- `src-tauri/src/services/mod.rs` — `pub mod check; pub mod completion;` (modify)
- `src-tauri/src/commands/reviews.rs` — pass `""` test_output; use `project::test_command_from_settings` (modify)
- `src-tauri/src/commands/projects.rs` — `set_project_test_command` (modify)
- `src-tauri/src/commands/coding_workspaces.rs` — `complete_coding_workspace` (modify)
- `src-tauri/src/lib.rs` — register the two new commands (modify)

Frontend:
- `src/api/codingWorkspaces.ts` — `completeCodingWorkspace` (modify)
- `src/api/projects.ts` — `setProjectTestCommand` (modify)
- `src/stores/codingWorkspaces.ts` — `complete` (modify)
- `src/stores/projects.ts` — `setTestCommand` (modify)
- `src/stores/reviews.ts` — `insert` (modify)
- `src/components/CodingView.vue` — Complete-and-review button + progress (modify)
- `src/components/ProjectsView.vue` — Test command field (modify)
- `e2e/specs/automation.e2e.ts` — completion e2e (create)

---

## Task 1: Events migration

**Files:**
- Create: `src-tauri/src/db/migrations/0006_events.sql`
- Modify: `src-tauri/src/db/mod.rs`

- [ ] **Step 1: Create the migration**

`src-tauri/src/db/migrations/0006_events.sql`:

```sql
-- An append-only audit log of notable automation events (e.g. a coding workspace
-- completion). Payload is an opaque JSON blob describing the event.
CREATE TABLE events (
    id           TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    type         TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}',
    created_at   TEXT NOT NULL
);

CREATE INDEX idx_events_workspace ON events(workspace_id);
```

- [ ] **Step 2: Register it** — in `src-tauri/src/db/mod.rs`, add after the version-5 `reviews` entry, before the closing `];`:

```rust
    (
        6,
        "events",
        include_str!("migrations/0006_events.sql"),
    ),
```

- [ ] **Step 3: Verify** — `cd src-tauri && cargo build` (compiles; `include_str!` resolves).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/db/migrations/0006_events.sql src-tauri/src/db/mod.rs
git commit -m "feat(m9): add events table migration"
```

---

## Task 2: Event model

**Files:**
- Create: `src-tauri/src/models/event.rs`
- Modify: `src-tauri/src/models/mod.rs`

- [ ] **Step 1: Create `src-tauri/src/models/event.rs`**

```rust
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub workspace_id: String,
    pub r#type: String,
    pub payload_json: String,
    pub created_at: String,
}

const COLUMNS: &str = "id, workspace_id, type, payload_json, created_at";

fn from_row(row: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        r#type: row.get("type")?,
        payload_json: row.get("payload_json")?,
        created_at: row.get("created_at")?,
    })
}

pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    event_type: &str,
    payload_json: &str,
) -> rusqlite::Result<Event> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO events (id, workspace_id, type, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, workspace_id, event_type, payload_json, now],
    )?;
    Ok(get(conn, id)?.expect("event exists immediately after insert"))
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Event>> {
    let sql = format!("SELECT {COLUMNS} FROM events WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list_by_workspace(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<Event>> {
    let sql =
        format!("SELECT {COLUMNS} FROM events WHERE workspace_id = ?1 ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::workspace;
    use crate::util::new_id;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn create_then_get_and_list() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let e = create(
            &conn,
            &new_id(),
            &ws,
            "coding_workspace.completed",
            r#"{"checks_passed":false}"#,
        )
        .unwrap();
        assert_eq!(e.workspace_id, ws);
        assert_eq!(e.r#type, "coding_workspace.completed");
        assert!(e.payload_json.contains("checks_passed"));
        assert_eq!(list_by_workspace(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &e.id).unwrap().is_some());
    }

    #[test]
    fn deleting_workspace_cascades_events() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let e = create(&conn, &new_id(), &ws, "x", "{}").unwrap();
        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &e.id).unwrap().is_none());
    }
}
```

- [ ] **Step 2: Register** — in `src-tauri/src/models/mod.rs` add `pub mod event;` (alphabetical, after `coding_workspace`).

- [ ] **Step 3: Test** — `cd src-tauri && cargo test models::event` → 2 pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/models/event.rs src-tauri/src/models/mod.rs
git commit -m "feat(m9): add event model with cascade test"
```

---

## Task 3: Review insert gains `test_output`

**Files:**
- Modify: `src-tauri/src/models/review.rs`
- Modify: `src-tauri/src/commands/reviews.rs`

- [ ] **Step 1: Add the `test_output` parameter to `review::create`**

In `src-tauri/src/models/review.rs`, the `create` function currently inserts `test_output` as a literal `''`. Change its signature and INSERT so the caller supplies it. Replace the whole `create` function with:

```rust
/// Insert a review snapshot. `files`/`risk_notes` are serialized to JSON columns.
/// The caller supplies `id` for consistency with the other models.
#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    coding_workspace_id: &str,
    summary: &str,
    status_short: &str,
    diff_stat: &str,
    files: &[String],
    test_command: Option<&str>,
    test_output: &str,
    risk_notes: &[String],
) -> rusqlite::Result<Review> {
    let now = now_rfc3339();
    let files_json = serde_json::to_string(files).unwrap_or_else(|_| "[]".to_string());
    let risk_json = serde_json::to_string(risk_notes).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO reviews
           (id, workspace_id, coding_workspace_id, status, summary, status_short, diff_stat,
            files_json, test_command, test_output, risk_notes_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            id,
            workspace_id,
            coding_workspace_id,
            summary,
            status_short,
            diff_stat,
            files_json,
            test_command,
            test_output,
            risk_json,
            now
        ],
    )?;
    Ok(get(conn, id)?.expect("review exists immediately after insert"))
}
```

- [ ] **Step 2: Update the model test helper**

In `src-tauri/src/models/review.rs`'s `#[cfg(test)] mod tests`, the `make` helper calls `create`. Add the new `test_output` argument (between `test_command` and `risk_notes`). The call currently ends `... Some("pnpm test"), &["Large change".to_string()])`. Change it to:

```rust
            Some("pnpm test"),
            "",
            &["Large change".to_string()],
```

(i.e. insert `"",` as the `test_output` argument before the `risk_notes` slice.)

- [ ] **Step 3: Update the M8 call site**

In `src-tauri/src/commands/reviews.rs`, `create_review_for_coding_workspace` calls `review::create(...)` ending with `test_command.as_deref(), &risk_notes,`. Insert an empty `test_output` argument so it reads:

```rust
        test_command.as_deref(),
        "",
        &risk_notes,
```

- [ ] **Step 4: Test + build** — `cd src-tauri && cargo test models::review && cargo build` → review tests pass, builds.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models/review.rs src-tauri/src/commands/reviews.rs
git commit -m "feat(m9): review::create accepts captured test_output"
```

---

## Task 4: Check runner service

**Files:**
- Create: `src-tauri/src/services/check.rs`
- Modify: `src-tauri/src/services/mod.rs`

- [ ] **Step 1: Create `src-tauri/src/services/check.rs`**

```rust
//! Runs a project's configured check command inside a worktree. The command is
//! user-authored project configuration — the ONLY string handed to the shell.
//! No repo-derived value (path, branch, diff) is ever interpolated into it.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::util::new_id;

#[derive(Debug, Clone, Serialize, Default)]
pub struct CheckOutcome {
    /// False when no command was configured (nothing was run).
    pub ran: bool,
    /// Process exit code; `None` on timeout or spawn failure.
    pub exit_code: Option<i32>,
    /// True when the command exceeded the timeout and was killed.
    pub timed_out: bool,
    /// Combined stdout+stderr (or the spawn-error text).
    pub output: String,
}

impl CheckOutcome {
    /// The "no command configured" outcome.
    pub fn not_run() -> Self {
        CheckOutcome::default()
    }

    /// A configured check that ran to a clean (zero) exit.
    pub fn passed(&self) -> bool {
        self.ran && !self.timed_out && self.exit_code == Some(0)
    }
}

/// Run `command` via `sh -c` in `worktree`, capturing combined stdout+stderr and
/// killing it after `timeout`. stdout+stderr are redirected to one temp file so a
/// full pipe buffer can never deadlock a long check (we don't drain a pipe while
/// polling). A spawn failure is reported as a failed run, not an `Err`.
pub fn run_check(worktree: &Path, command: &str, timeout: Duration) -> CheckOutcome {
    let log_path = std::env::temp_dir().join(format!("uaw-check-{}.log", new_id()));

    let file = match fs::File::create(&log_path) {
        Ok(f) => f,
        Err(e) => return spawn_failure(format!("failed to create check log: {e}")),
    };
    let file_err = match file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            let _ = fs::remove_file(&log_path);
            return spawn_failure(format!("failed to set up check output: {e}"));
        }
    };

    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(file_err)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&log_path);
            return spawn_failure(format!("failed to start check: {e}"));
        }
    };

    let start = Instant::now();
    let (exit_code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break (None, true);
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break (None, false),
        }
    };

    let output = fs::read_to_string(&log_path).unwrap_or_default();
    let _ = fs::remove_file(&log_path);

    CheckOutcome {
        ran: true,
        exit_code,
        timed_out,
        output,
    }
}

fn spawn_failure(message: String) -> CheckOutcome {
    CheckOutcome {
        ran: true,
        exit_code: None,
        timed_out: false,
        output: message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passing_command_captures_output() {
        let o = run_check(&std::env::temp_dir(), "echo hello-check", Duration::from_secs(10));
        assert!(o.ran);
        assert_eq!(o.exit_code, Some(0));
        assert!(!o.timed_out);
        assert!(o.passed());
        assert!(o.output.contains("hello-check"));
    }

    #[test]
    fn nonzero_exit_is_not_passed() {
        let o = run_check(&std::env::temp_dir(), "echo boom; exit 3", Duration::from_secs(10));
        assert_eq!(o.exit_code, Some(3));
        assert!(!o.passed());
        assert!(o.output.contains("boom"));
    }

    #[test]
    fn timeout_kills_long_command() {
        let o = run_check(&std::env::temp_dir(), "sleep 5", Duration::from_millis(300));
        assert!(o.timed_out);
        assert!(!o.passed());
        assert_eq!(o.exit_code, None);
    }

    #[test]
    fn runs_in_the_given_worktree() {
        let dir = std::env::temp_dir().join(format!("uaw-cwd-{}", new_id()));
        fs::create_dir_all(&dir).unwrap();
        let o = run_check(&dir, "basename \"$(pwd)\"", Duration::from_secs(10));
        let name = dir.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(o.output.trim(), name);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn not_run_helper_is_inert() {
        let o = CheckOutcome::not_run();
        assert!(!o.ran);
        assert!(!o.passed());
        assert!(o.output.is_empty());
    }
}
```

- [ ] **Step 2: Register** — in `src-tauri/src/services/mod.rs` add `pub mod check;` after `pub mod git;`.

- [ ] **Step 3: Test** — `cd src-tauri && cargo test services::check` → 5 pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/check.rs src-tauri/src/services/mod.rs
git commit -m "feat(m9): add check runner (sh -c in worktree, timeout, captured output)"
```

---

## Task 5: Completion helpers

**Files:**
- Create: `src-tauri/src/services/completion.rs`
- Modify: `src-tauri/src/services/mod.rs`

- [ ] **Step 1: Create `src-tauri/src/services/completion.rs`**

```rust
//! Pure derivations for the completion flow: render the captured check result as
//! review test output, and augment the risk notes when checks didn't pass. No IO.

use crate::services::check::CheckOutcome;

/// Render the check result as a review's `test_output`. Empty when no command ran.
pub fn format_test_output(command: &str, outcome: &CheckOutcome) -> String {
    if !outcome.ran {
        return String::new();
    }
    let trailer = if outcome.timed_out {
        "[timed out]".to_string()
    } else if let Some(code) = outcome.exit_code {
        format!("[exit {code}]")
    } else {
        "[no exit code]".to_string()
    };
    format!("$ {command}\n{}\n{trailer}", outcome.output.trim_end())
}

/// Append a risk flag when the check timed out or failed. Unchanged when the
/// check passed or never ran.
pub fn augment_risk_notes(mut notes: Vec<String>, outcome: &CheckOutcome) -> Vec<String> {
    if outcome.timed_out {
        notes.push("Checks timed out".to_string());
    } else if outcome.ran && !outcome.passed() {
        notes.push("Checks failed".to_string());
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(ran: bool, exit: Option<i32>, timed_out: bool, output: &str) -> CheckOutcome {
        CheckOutcome {
            ran,
            exit_code: exit,
            timed_out,
            output: output.to_string(),
        }
    }

    #[test]
    fn not_run_yields_empty_output_and_no_flag() {
        let o = CheckOutcome::not_run();
        assert_eq!(format_test_output("pnpm test", &o), "");
        assert!(augment_risk_notes(vec![], &o).is_empty());
    }

    #[test]
    fn passing_output_has_exit_zero_trailer_and_no_flag() {
        let o = outcome(true, Some(0), false, "all good\n");
        let text = format_test_output("pnpm test", &o);
        assert!(text.starts_with("$ pnpm test\n"));
        assert!(text.contains("all good"));
        assert!(text.ends_with("[exit 0]"));
        assert!(augment_risk_notes(vec![], &o).is_empty());
    }

    #[test]
    fn failing_exit_adds_checks_failed_flag() {
        let o = outcome(true, Some(1), false, "boom");
        assert!(format_test_output("x", &o).ends_with("[exit 1]"));
        let notes = augment_risk_notes(vec!["Large change".to_string()], &o);
        assert_eq!(notes, vec!["Large change".to_string(), "Checks failed".to_string()]);
    }

    #[test]
    fn timeout_adds_timed_out_flag_and_trailer() {
        let o = outcome(true, None, true, "partial");
        assert!(format_test_output("x", &o).ends_with("[timed out]"));
        assert_eq!(augment_risk_notes(vec![], &o), vec!["Checks timed out".to_string()]);
    }

    #[test]
    fn spawn_failure_no_exit_code_trailer() {
        let o = outcome(true, None, false, "failed to start check: ...");
        assert!(format_test_output("x", &o).ends_with("[no exit code]"));
        assert_eq!(augment_risk_notes(vec![], &o), vec!["Checks failed".to_string()]);
    }
}
```

- [ ] **Step 2: Register** — in `src-tauri/src/services/mod.rs` add `pub mod completion;` (after `pub mod check;`).

- [ ] **Step 3: Test** — `cd src-tauri && cargo test services::completion` → 5 pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/services/completion.rs src-tauri/src/services/mod.rs
git commit -m "feat(m9): add pure completion output/risk-note helpers"
```

---

## Task 6: Project test-command config

**Files:**
- Modify: `src-tauri/src/models/project.rs`
- Modify: `src-tauri/src/commands/projects.rs`
- Modify: `src-tauri/src/commands/reviews.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add settings helpers to the project model**

In `src-tauri/src/models/project.rs`, add these functions after the existing `update` function (and before `delete`):

```rust
/// Persist a project's raw `settings_json`.
pub fn update_settings_json(
    conn: &Connection,
    id: &str,
    settings_json: &str,
) -> rusqlite::Result<Option<Project>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE projects SET settings_json = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, settings_json, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

/// Read the optional `test_command` from a project's `settings_json`. Blank or
/// whitespace-only values are treated as absent.
pub fn test_command_from_settings(settings_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(settings_json)
        .ok()
        .and_then(|v| {
            v.get("test_command")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Return a new `settings_json` with `test_command` set (or removed when `None`
/// or blank), preserving any other keys. Malformed input is replaced by a fresh
/// object.
pub fn merge_test_command(settings_json: &str, test_command: Option<&str>) -> String {
    let mut obj = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(settings_json)
        .unwrap_or_default();
    match test_command.map(|c| c.trim()).filter(|c| !c.is_empty()) {
        Some(cmd) => {
            obj.insert("test_command".into(), serde_json::Value::String(cmd.to_string()));
        }
        None => {
            obj.remove("test_command");
        }
    }
    serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or_else(|_| "{}".to_string())
}
```

- [ ] **Step 2: Add unit tests for the pure helpers**

In `src-tauri/src/models/project.rs`'s `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn test_command_round_trips_through_settings() {
        assert_eq!(test_command_from_settings("{}"), None);
        assert_eq!(
            test_command_from_settings(r#"{"test_command":"pnpm test"}"#).as_deref(),
            Some("pnpm test")
        );
        assert_eq!(test_command_from_settings(r#"{"test_command":"  "}"#), None);
        assert_eq!(test_command_from_settings("not json"), None);
    }

    #[test]
    fn merge_sets_removes_and_preserves_other_keys() {
        // Set into an object that has another key — the other key survives.
        let merged = merge_test_command(r#"{"keep":"yes"}"#, Some("cargo test"));
        assert!(merged.contains("\"keep\":\"yes\""));
        assert_eq!(test_command_from_settings(&merged).as_deref(), Some("cargo test"));

        // Remove with None.
        let cleared = merge_test_command(&merged, None);
        assert!(cleared.contains("\"keep\":\"yes\""));
        assert_eq!(test_command_from_settings(&cleared), None);

        // Blank is treated as removal.
        assert_eq!(test_command_from_settings(&merge_test_command("{}", Some("   "))), None);

        // Malformed input becomes a fresh object holding just the command.
        assert_eq!(
            test_command_from_settings(&merge_test_command("garbage", Some("x"))).as_deref(),
            Some("x")
        );
    }
```

- [ ] **Step 3: Add the `set_project_test_command` command**

In `src-tauri/src/commands/projects.rs`, add this command (place it after `update_project`):

```rust
#[tauri::command]
pub fn set_project_test_command(
    state: State<'_, Mutex<Connection>>,
    id: String,
    test_command: Option<String>,
) -> Result<Option<Project>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    let Some(project) = project::get(&conn, &id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let merged = project::merge_test_command(&project.settings_json, test_command.as_deref());
    project::update_settings_json(&conn, &id, &merged).map_err(|e| e.to_string())
}
```

If `Project` is not already imported in `commands/projects.rs`, ensure the existing `use` brings it in (the file already references `project::` and returns `Project`; reuse the existing import — do not add a duplicate).

- [ ] **Step 4: Point the M8 review command at the shared helper**

In `src-tauri/src/commands/reviews.rs`:
1. Delete the private `fn test_command_from_settings(...)` definition and its `#[test] fn test_command_parsing()` (now owned by the project model).
2. In `create_review_for_coding_workspace`, change `.and_then(|p| test_command_from_settings(&p.settings_json))` to `.and_then(|p| project::test_command_from_settings(&p.settings_json))`. (`project` is already imported via `use crate::models::{coding_workspace, project};`.)

- [ ] **Step 5: Register the command** — in `src-tauri/src/lib.rs`, inside `generate_handler!`, add after `commands::projects::delete_project,`:

```rust
            commands::projects::set_project_test_command,
```

- [ ] **Step 6: Test + build** — `cd src-tauri && cargo test models::project && cargo test commands::reviews && cargo build` → all pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/models/project.rs src-tauri/src/commands/projects.rs src-tauri/src/commands/reviews.rs src-tauri/src/lib.rs
git commit -m "feat(m9): per-project test command config + shared settings helpers"
```

---

## Task 7: Completion command

**Files:**
- Modify: `src-tauri/src/commands/coding_workspaces.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add imports**

At the top of `src-tauri/src/commands/coding_workspaces.rs`, the existing imports include `use crate::models::coding_workspace::{self, CodingWorkspace};`, `use crate::models::{project, repository};`, `use crate::services::git::{self, WorktreeDiff};`, `use crate::util::new_id;`. Add:

```rust
use std::time::Duration;

use crate::models::event;
use crate::models::review::{self, Review};
use crate::services::{check, completion, review as review_svc};
```

(Keep the existing imports; just add these. `std::path::Path` and `std::sync::Mutex` / `rusqlite::Connection` / `tauri::State` are already imported.)

- [ ] **Step 2: Add the completion command**

Append this to `src-tauri/src/commands/coding_workspaces.rs`:

```rust
/// Maximum wall-clock time a configured check may run before it is killed.
const CHECK_TIMEOUT: Duration = Duration::from_secs(600);

/// Complete a coding workspace: snapshot the diff, run the project's configured
/// check (if any), persist a review with the captured output and risk flags, move
/// the workspace to Needs Review, and record a completion event. A failing or
/// timed-out check still completes (with a risk flag); only a snapshot or DB
/// error aborts.
#[tauri::command]
pub fn complete_coding_workspace(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
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

    let snapshot = git::review_snapshot(Path::new(&worktree_path));
    if let Some(e) = snapshot.error {
        return Err(e);
    }

    let outcome = match &test_command {
        Some(cmd) => check::run_check(Path::new(&worktree_path), cmd, CHECK_TIMEOUT),
        None => check::CheckOutcome::not_run(),
    };

    let summary = review_svc::summarize(&snapshot);
    let risk_notes =
        completion::augment_risk_notes(review_svc::compute_risk_notes(&snapshot), &outcome);
    let test_output = completion::format_test_output(test_command.as_deref().unwrap_or(""), &outcome);

    let review_id = new_id();
    let payload = serde_json::json!({
        "coding_workspace_id": coding_workspace_id,
        "review_id": review_id,
        "checks_ran": outcome.ran,
        "checks_passed": outcome.passed(),
    })
    .to_string();

    // Persist the review, status move, and event together under one lock.
    let conn = state.lock().map_err(|e| e.to_string())?;
    let review = review::create(
        &conn,
        &review_id,
        &workspace_id,
        &coding_workspace_id,
        &summary,
        &snapshot.status_short,
        &snapshot.diff_stat,
        &snapshot.files,
        test_command.as_deref(),
        &test_output,
        &risk_notes,
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
```

- [ ] **Step 3: Register** — in `src-tauri/src/lib.rs`, inside `generate_handler!`, add after `commands::coding_workspaces::discard_coding_workspace,`:

```rust
            commands::coding_workspaces::complete_coding_workspace,
```

- [ ] **Step 4: Build + full test + clippy**

```bash
cd src-tauri && cargo test 2>&1 | grep "test result:" | tail -3 && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: all tests pass; clippy clean (every new fn is now wired).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/coding_workspaces.rs src-tauri/src/lib.rs
git commit -m "feat(m9): add complete_coding_workspace automation command"
```

---

## Task 8: Frontend api + stores

**Files:**
- Modify: `src/api/codingWorkspaces.ts`
- Modify: `src/api/projects.ts`
- Modify: `src/stores/codingWorkspaces.ts`
- Modify: `src/stores/projects.ts`
- Modify: `src/stores/reviews.ts`

- [ ] **Step 1: API wrappers**

In `src/api/codingWorkspaces.ts`, add an import for the Review type and a wrapper. At the top, the file imports `import type { CodingWorkspace, WorktreeDiff } from "../types/codingWorkspace";` — add below it:

```ts
import type { Review } from "../types/review";
```

And append this function:

```ts
export function completeCodingWorkspace(id: string): Promise<Review> {
  return invoke<Review>("complete_coding_workspace", { codingWorkspaceId: id });
}
```

In `src/api/projects.ts`, append:

```ts
export function setProjectTestCommand(
  id: string,
  testCommand: string | null,
): Promise<Project | null> {
  return invoke<Project | null>("set_project_test_command", { id, testCommand });
}
```

- [ ] **Step 2: Coding store `complete`**

In `src/stores/codingWorkspaces.ts`, add `import type { Review } from "../types/review";` near the top type imports. Then add this action (after `markReady`):

```ts
  async function complete(id: string): Promise<Review> {
    const review = await api.completeCodingWorkspace(id);
    // Completion deterministically moves the workspace to needs-review.
    const i = list.value.findIndex((c) => c.id === id);
    if (i >= 0) list.value[i] = { ...list.value[i], status: "needs-review" };
    return review;
  }
```

And add `complete` to the returned object (the `return { ... }` at the end of the store).

- [ ] **Step 3: Projects store `setTestCommand`**

In `src/stores/projects.ts`, add (after `rename`):

```ts
  async function setTestCommand(id: string, command: string) {
    const trimmed = command.trim();
    const project = await api.setProjectTestCommand(id, trimmed === "" ? null : trimmed);
    if (project) {
      const i = list.value.findIndex((p) => p.id === id);
      if (i >= 0) list.value[i] = project;
    }
    return project;
  }
```

Add `setTestCommand` to the store's returned object.

- [ ] **Step 4: Reviews store `insert`**

In `src/stores/reviews.ts`, add (after `updateStatus`):

```ts
  function insert(review: Review) {
    const i = list.value.findIndex((r) => r.id === review.id);
    if (i >= 0) list.value[i] = review;
    else list.value.unshift(review);
  }
```

Add `insert` to the store's returned object.

- [ ] **Step 5: Build + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build
pnpm format
git add src/api/codingWorkspaces.ts src/api/projects.ts src/stores/codingWorkspaces.ts src/stores/projects.ts src/stores/reviews.ts
git commit -m "feat(m9): frontend api + store actions for completion and test command"
```
Expected: `pnpm build` succeeds.

---

## Task 9: Frontend components

**Files:**
- Modify: `src/components/CodingView.vue`
- Modify: `src/components/ProjectsView.vue`

- [ ] **Step 1: CodingView — completion state + handler**

In `src/components/CodingView.vue` `<script setup>` (which already imports `useReviewsStore` as `reviews` and `useToast` as `toast`), add a ref next to the other refs (e.g. after `const expandedId = ref<string | null>(null);`):

```ts
const completingId = ref<string | null>(null);
```

Add this handler after the existing `createReview` function:

```ts
async function completeAndReview(id: string) {
  completingId.value = id;
  try {
    const review = await coding.complete(id);
    reviews.insert(review);
    toast.success("Completed — review ready in Reviews");
  } catch (e) {
    toast.error(String(e));
  } finally {
    completingId.value = null;
  }
}
```

- [ ] **Step 2: CodingView — the button**

In `src/components/CodingView.vue` `<template>`, inside `<span class="coding__actions">`, add this button immediately after the "Create review" button and before the "Discard" button:

```vue
            <button
              type="button"
              class="re-button"
              data-variant="brand"
              data-size="sm"
              :disabled="completingId === cw.id"
              @click="completeAndReview(cw.id)"
            >
              {{ completingId === cw.id ? "Running checks…" : "Complete and review" }}
            </button>
```

- [ ] **Step 3: ProjectsView — script helpers**

In `src/components/ProjectsView.vue` `<script setup>`, add a `Project` type import to the existing `../types/project` import. It currently reads:

```ts
import { PROJECT_MODES, type ProjectMode } from "../types/project";
```

Change it to:

```ts
import { PROJECT_MODES, type Project, type ProjectMode } from "../types/project";
```

Then add these functions (after `saveRename`):

```ts
function testCommandOf(project: Project): string {
  try {
    return (JSON.parse(project.settings_json)?.test_command as string) ?? "";
  } catch {
    return "";
  }
}

async function saveTestCommand(id: string, event: Event) {
  const value = (event.target as HTMLInputElement).value;
  try {
    await projects.setTestCommand(id, value);
    toast.success("Test command saved");
  } catch (e) {
    toast.error(String(e));
  }
}
```

- [ ] **Step 4: ProjectsView — the field**

In `src/components/ProjectsView.vue` `<template>`, inside the `<template v-else>` block of the project row (the non-editing view, which renders `row__title`, the mode `re-badge`, and `row__actions`), add this input immediately after the `<span class="row__actions">…</span>` closing tag (still inside `<template v-else>`):

```vue
          <input
            class="re-input project__cmd"
            data-size="sm"
            type="text"
            placeholder="Test command (optional)"
            :aria-label="`Test command for ${project.name}`"
            :value="testCommandOf(project)"
            @change="saveTestCommand(project.id, $event)"
            @keyup.enter="saveTestCommand(project.id, $event)"
          />
```

- [ ] **Step 5: ProjectsView — let the row wrap**

In `src/components/ProjectsView.vue` `<style scoped>`, the `.rows .re-card` rule lays the row out as `flex-direction: row`. Add `flex-wrap: wrap;` to it so the command input drops to its own line. Change:

```css
.rows .re-card {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 0.6rem;
  /* Bare .re-card has no padding (it lives in .re-card__body, unused here). */
  padding: 0.6rem 0.85rem;
}
```

to add `flex-wrap: wrap;` after `align-items: center;`, and append a rule for the command input:

```css
.project__cmd {
  flex: 1 1 100%;
  font-family: ui-monospace, monospace;
}
```

- [ ] **Step 6: Build + format + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build && pnpm format
git add src/components/CodingView.vue src/components/ProjectsView.vue
git commit -m "feat(m9): Complete and review action + per-project test command field"
```
Expected: `pnpm build` succeeds.

---

## Task 10: e2e — complete-and-review

**Files:**
- Create: `e2e/specs/automation.e2e.ts`

- [ ] **Step 1: Create `e2e/specs/automation.e2e.ts`**

This is a self-contained spec with its OWN fixture repo (`/tmp/fixture-repo-auto`) so worktree creation never races the `coding.e2e.ts` spec. Each spec already gets an isolated DB + worktrees dir (see `wdio.conf.ts`).

```ts
import { browser, $, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

const REPO = "/tmp/fixture-repo-auto";

/**
 * Milestone 9 end-to-end: configure a project test command, create a worktree,
 * make a change, click "Complete and review", and verify the workspace lands in
 * Needs Review with a persisted review showing the captured check output and a
 * "Checks failed" risk flag. Uses its own fixture repo to stay isolated.
 */
describe("completion automation", () => {
  before(async () => {
    // Fresh, isolated fixture repo so adding a worktree never races coding.e2e.
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (args: string[]) => execFileSync("git", ["-C", REPO, ...args], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "auto@uaw.local"]);
    git(["config", "user.name", "UAW Auto"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# auto fixture\n");
    git(["add", "."]);
    git(["commit", "-m", "init"]);

    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a code project with a test command and an attached repo", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("AutoProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    // Configure the check command (prints a marker, then fails).
    const cmd = await $('[aria-label="Test command for AutoProj"]');
    await cmd.setValue("echo myCheck; exit 1");
    await browser.keys("Enter");

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("AutoFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("creates a worktree from the repo", async () => {
    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("AutoProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("AutoFixture");

    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), {
      timeout: 10_000,
      timeoutMsg: "base branch select never populated",
    });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/auto");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });
  });

  it("completes the worktree: runs checks, lands in Needs Review with output + failure flag", async () => {
    const row = await $('[data-testid="coding-row"]');

    // Make a change so the review has content.
    const worktreePath = (await textOf('[data-testid="coding-row"] .coding__path')).trim();
    fs.writeFileSync(path.join(worktreePath, "change.txt"), "work\n");

    await row.$("button*=Complete and review").click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="coding-row"] .re-badge')).includes("needs-review"),
      { timeout: 30_000, timeoutMsg: "expected the worktree to move to needs-review" },
    );

    // The resulting review shows the captured check output and the failure flag.
    await (await $("button*=Reviews")).click();
    const reviewRow = await $('[data-testid="review-row"]');
    await reviewRow.waitForExist({ timeout: 10_000 });
    await reviewRow.click();

    await browser.waitUntil(
      async () => (await textOf('[data-testid="review-detail"]')).includes("myCheck"),
      { timeout: 10_000, timeoutMsg: "expected the check output (myCheck) in the review" },
    );
    expect(await textOf('[data-testid="review-detail"]')).toContain("Checks failed");
  });
});
```

- [ ] **Step 2: Typecheck**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm e2e:typecheck
```
Expected: no type errors.

- [ ] **Step 3: Run the full e2e (Docker)**

```bash
pnpm e2e:docker
```
Expected: all specs pass, including `automation.e2e.ts` (3 tests). If iterating without Docker, at least confirm `pnpm build` + `pnpm e2e:typecheck` clean; CI runs the suite.

- [ ] **Step 4: Final checks + commit**

```bash
pnpm format && pnpm build && cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cd ..
git add e2e/specs/automation.e2e.ts
git commit -m "test(m9): e2e complete-and-review with configured checks"
```
Expected: format clean, build OK, all Rust tests pass, clippy clean.

---

## Self-review notes

- **Spec coverage:** automation runner (`check` + `completion` + the orchestration command, Tasks 4/5/7) · event record (Tasks 1/2, inserted in Task 7) · trigger command (Task 7) · "Complete and review" button + progress + link (Task 9, review inserted into store + toast) · test-command config (Task 6 + Task 9 field) · run-checks-if-present / complete-anyway-and-flag (Task 7 uses `not_run()` + `augment_risk_notes`) · move to Needs Review (Task 7 `update_status`). All spec items mapped.
- **Type consistency:** `review::create` gains `test_output: &str` between `test_command` and `risk_notes` — every call site (M8 reviews.rs passes `""`, M9 completion passes the captured output, model test helper passes `""`) matches. `CheckOutcome` fields (`ran`/`exit_code`/`timed_out`/`output`) and `passed()`/`not_run()` are used consistently across `check`, `completion`, and the command. `complete_coding_workspace` invoked with `{ codingWorkspaceId }` ↔ Rust `coding_workspace_id` (Tauri camelCase mapping, same as M8's `create_review_for_coding_workspace`). `set_project_test_command` ↔ `{ id, testCommand }`.
- **Out of scope (M10+), intentionally absent:** real agent execution; session↔worktree linkage; streaming live check output; an events UI.
```
