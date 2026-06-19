# Interactive Agent Terminals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run real interactive agent CLIs (Claude Code / Codex / Gemini) in PTY-backed terminal tabs inside the app, each bound to a git worktree, rendered with xterm.js.

**Architecture:** A Rust PTY layer (`portable-pty`) spawns the chosen CLI in a pseudo-terminal (`cwd = worktree`); a reader thread streams output bytes to the frontend via Tauri events and appends them to a transcript file. The frontend (`@xterm/xterm`) renders each session in a tab, sending keystrokes/resize back. Sessions + transcripts persist; review stays the existing manual M9 flow.

**Tech Stack:** Rust + `portable-pty` + rusqlite, Tauri 2 events/commands, Vue 3 + Pinia + `@xterm/xterm`/`@xterm/addon-fit`, WebdriverIO e2e.

**Library API notes (verified):**
- `portable-pty` 0.9: `native_pty_system()`; `openpty(PtySize{rows,cols,pixel_width,pixel_height})` → `PtyPair{master: Box<dyn MasterPty + Send>, slave}`; `CommandBuilder::new(prog)`, `.args(&[&str])`, `.cwd(path)`, `.env(k,v)`; `slave.spawn_command(cmd) -> Box<dyn Child + Send + Sync>`; `child.clone_killer() -> Box<dyn ChildKiller + Send + Sync>`; `master.try_clone_reader() -> Box<dyn Read + Send>`; `master.take_writer() -> Box<dyn Write + Send>`; `master.resize(PtySize)`; `child.wait() -> ExitStatus` with `.success()` and `.exit_code() -> u32`. **Drop the `slave` after spawning** so the reader sees EOF when the child exits.
- `@xterm/xterm`: `import { Terminal } from '@xterm/xterm'` + `import '@xterm/xterm/css/xterm.css'`; `@xterm/addon-fit`: `import { FitAddon } from '@xterm/addon-fit'`. `term.open(el)`, `term.write(Uint8Array|string)`, `term.onData(cb)`, `fit.fit()` then read `term.cols`/`term.rows`, `term.dispose()`. DOM renderer by default (headless-safe).

---

## File structure

Backend:
- `src-tauri/Cargo.toml` — add `portable-pty` (modify)
- `src-tauri/src/db/migrations/0007_agent_sessions.sql` (create) + `db/mod.rs` register (modify)
- `src-tauri/src/models/agent_session.rs` (create) + `models/mod.rs` (modify)
- `src-tauri/src/services/agent/mod.rs` — adapter registry + capabilities + `resolve_program` (create)
- `src-tauri/src/services/agent/pty.rs` — `spawn` + `pump` + `PtyHandle` (create)
- `src-tauri/src/services/mod.rs` — `pub mod agent;` (modify)
- `src-tauri/src/commands/agent_sessions.rs` — commands + reader-thread runner + `AgentProcesses` state (create)
- `src-tauri/src/commands/mod.rs` (modify) + `src-tauri/src/lib.rs` — register state + commands (modify)

Frontend:
- `package.json` — add `@xterm/xterm`, `@xterm/addon-fit` (modify)
- `src/types/agentSession.ts` (create)
- `src/api/agentSessions.ts` (create)
- `src/stores/agentSessions.ts` (create)
- `src/components/TerminalTab.vue` (create)
- `src/components/AgentsView.vue` (create)
- `src/App.vue` — nav + view wiring (modify)
- `e2e/specs/agent-terminal.e2e.ts` (create) + `scripts/run-e2e.sh` — fake agent script (modify)

---

## Task 1: Dependency + migration + model

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/db/migrations/0007_agent_sessions.sql`
- Modify: `src-tauri/src/db/mod.rs`
- Create: `src-tauri/src/models/agent_session.rs`
- Modify: `src-tauri/src/models/mod.rs`

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml`, under `[dependencies]`, add:

```toml
portable-pty = "0.9"
```

- [ ] **Step 2: Create the migration** `src-tauri/src/db/migrations/0007_agent_sessions.sql`:

```sql
-- An agent session is one run of an interactive agent CLI (Claude Code, Codex,
-- Gemini) in a PTY against a coding workspace's worktree. Raw terminal output is
-- streamed to a transcript file referenced here.
CREATE TABLE agent_sessions (
    id                   TEXT PRIMARY KEY NOT NULL,
    workspace_id         TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    coding_workspace_id  TEXT NOT NULL REFERENCES coding_workspaces(id) ON DELETE CASCADE,
    adapter_id           TEXT NOT NULL,
    command              TEXT NOT NULL,
    status               TEXT NOT NULL DEFAULT 'running',
    exit_code            INTEGER,
    transcript_path      TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
CREATE INDEX idx_agent_sessions_coding_workspace ON agent_sessions(coding_workspace_id);
CREATE INDEX idx_agent_sessions_workspace ON agent_sessions(workspace_id);
```

- [ ] **Step 3: Register migration** — in `src-tauri/src/db/mod.rs`, add after the version-6 `events` entry:

```rust
    (
        7,
        "agent_sessions",
        include_str!("migrations/0007_agent_sessions.sql"),
    ),
```

Also update the stale assertion in `src-tauri/src/models/workspace.rs`: the `migrations_are_idempotent` test asserts the highest version equals `6`; change that literal to `7`.

- [ ] **Step 4: Create the model** `src-tauri/src/models/agent_session.rs`:

```rust
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub workspace_id: String,
    pub coding_workspace_id: String,
    pub adapter_id: String,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub transcript_path: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, coding_workspace_id, adapter_id, command, status, \
                       exit_code, transcript_path, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<AgentSession> {
    Ok(AgentSession {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        coding_workspace_id: row.get("coding_workspace_id")?,
        adapter_id: row.get("adapter_id")?,
        command: row.get("command")?,
        status: row.get("status")?,
        exit_code: row.get("exit_code")?,
        transcript_path: row.get("transcript_path")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    coding_workspace_id: &str,
    adapter_id: &str,
    command: &str,
    transcript_path: &str,
) -> rusqlite::Result<AgentSession> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO agent_sessions
           (id, workspace_id, coding_workspace_id, adapter_id, command, status,
            exit_code, transcript_path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', NULL, ?6, ?7, ?7)",
        params![id, workspace_id, coding_workspace_id, adapter_id, command, transcript_path, now],
    )?;
    Ok(get(conn, id)?.expect("agent session exists immediately after insert"))
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<AgentSession>> {
    let sql = format!("SELECT {COLUMNS} FROM agent_sessions WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list_by_coding_workspace(
    conn: &Connection,
    coding_workspace_id: &str,
) -> rusqlite::Result<Vec<AgentSession>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM agent_sessions WHERE coding_workspace_id = ?1 ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![coding_workspace_id], from_row)?;
    rows.collect()
}

/// Move a still-running session to a terminal status. No-op if it already left
/// `running` (e.g. a user `stop` raced the natural exit), so a kill recorded as
/// `stopped` is not overwritten by the reader thread's `exited`/`failed`.
pub fn mark_exited(
    conn: &Connection,
    id: &str,
    status: &str,
    exit_code: Option<i64>,
) -> rusqlite::Result<Option<AgentSession>> {
    let now = now_rfc3339();
    conn.execute(
        "UPDATE agent_sessions SET status = ?2, exit_code = ?3, updated_at = ?4
         WHERE id = ?1 AND status = 'running'",
        params![id, status, exit_code, now],
    )?;
    get(conn, id)
}

/// Force a terminal status regardless of current state (used by explicit stop).
pub fn set_status(
    conn: &Connection,
    id: &str,
    status: &str,
) -> rusqlite::Result<Option<AgentSession>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE agent_sessions SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{coding_workspace, project, repository, workspace};
    use crate::util::new_id;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    /// (workspace_id, coding_workspace_id)
    fn fixtures(conn: &Connection) -> (String, String) {
        let ws = workspace::create(conn, "Test", "mixed").unwrap().id;
        let p = project::create(conn, &ws, "P", "code").unwrap().id;
        let r = repository::create(conn, &ws, "repo", "/tmp/repo", "main", None)
            .unwrap()
            .id;
        let cw_id = new_id();
        let cw = coding_workspace::create(
            conn, &cw_id, &ws, &p, &r, "/tmp/repo",
            &format!("/tmp/worktrees/{cw_id}"), "feature/x", "main",
        )
        .unwrap();
        (ws, cw.id)
    }

    fn make(conn: &Connection, ws: &str, cw: &str) -> AgentSession {
        create(conn, &new_id(), ws, cw, "claude-code", "claude", "/tmp/t.log").unwrap()
    }

    #[test]
    fn create_then_get_and_list() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);
        assert_eq!(s.status, "running");
        assert_eq!(s.adapter_id, "claude-code");
        assert_eq!(s.exit_code, None);
        assert_eq!(list_by_coding_workspace(&conn, &cw).unwrap().len(), 1);
        assert!(get(&conn, &s.id).unwrap().is_some());
    }

    #[test]
    fn mark_exited_only_moves_running_sessions() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);

        // A user stop forces 'stopped'...
        set_status(&conn, &s.id, "stopped").unwrap();
        // ...and a racing natural-exit mark_exited must NOT overwrite it.
        let after = mark_exited(&conn, &s.id, "exited", Some(0)).unwrap().unwrap();
        assert_eq!(after.status, "stopped");
        assert_eq!(after.exit_code, None);
    }

    #[test]
    fn mark_exited_records_running_exit() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);
        let after = mark_exited(&conn, &s.id, "exited", Some(0)).unwrap().unwrap();
        assert_eq!(after.status, "exited");
        assert_eq!(after.exit_code, Some(0));
    }

    #[test]
    fn deleting_coding_workspace_cascades_sessions() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = make(&conn, &ws, &cw);
        coding_workspace::delete(&conn, &cw).unwrap();
        assert!(get(&conn, &s.id).unwrap().is_none());
    }
}
```

- [ ] **Step 5: Register** — `src-tauri/src/models/mod.rs`: add `pub mod agent_session;` (alphabetical, after `agent`? there's no agent model; place after the first entries alphabetically — after nothing named 'a' yet, so it becomes the first: `pub mod agent_session;` before `pub mod coding_workspace;`).

- [ ] **Step 6: Verify + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace/src-tauri && cargo test models::agent_session && cargo build
cd /Users/csaba/projects/unified-agentic-workspace
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/db/migrations/0007_agent_sessions.sql src-tauri/src/db/mod.rs src-tauri/src/models/agent_session.rs src-tauri/src/models/mod.rs src-tauri/src/models/workspace.rs
git commit -m "feat(m10a): agent_sessions table + model + portable-pty dep"
```
Expected: 4 model tests pass; build OK (Cargo.lock updates with portable-pty).

---

## Task 2: Adapter registry

**Files:**
- Create: `src-tauri/src/services/agent/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`

- [ ] **Step 1: Create `src-tauri/src/services/agent/mod.rs`**

```rust
//! Agent CLI adapters: descriptors of the interactive coding CLIs UAW can launch
//! in a PTY. The runtime is identical for each; an adapter just names the program
//! + base args + capabilities. The program is overridable via `UAW_AGENT_BIN`
//! (used by tests to inject a fake interactive program).

pub mod pty;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AgentCapabilities {
    pub streaming: bool,
    pub tool_use: bool,
    pub mcp: bool,
    pub file_edits: bool,
    pub shell_commands: bool,
    pub multi_turn: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentAdapter {
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    pub args: Vec<&'static str>,
    pub capabilities: AgentCapabilities,
}

fn full_capabilities() -> AgentCapabilities {
    AgentCapabilities {
        streaming: true,
        tool_use: true,
        mcp: true,
        file_edits: true,
        shell_commands: true,
        multi_turn: true,
    }
}

/// The built-in interactive CLI adapters.
pub fn adapters() -> Vec<AgentAdapter> {
    vec![
        AgentAdapter { id: "claude-code", name: "Claude Code", program: "claude", args: vec![], capabilities: full_capabilities() },
        AgentAdapter { id: "codex", name: "Codex", program: "codex", args: vec![], capabilities: full_capabilities() },
        AgentAdapter { id: "gemini", name: "Gemini", program: "gemini", args: vec![], capabilities: full_capabilities() },
    ]
}

pub fn find_adapter(id: &str) -> Option<AgentAdapter> {
    adapters().into_iter().find(|a| a.id == id)
}

/// The program to actually spawn for an adapter: `UAW_AGENT_BIN` overrides every
/// adapter (so e2e can substitute a fake interactive program); otherwise the
/// adapter's default program.
pub fn resolve_program(adapter: &AgentAdapter) -> String {
    match std::env::var("UAW_AGENT_BIN") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => adapter.program.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_the_three_clis() {
        let ids: Vec<_> = adapters().iter().map(|a| a.id).collect();
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"gemini"));
        assert!(find_adapter("claude-code").is_some());
        assert!(find_adapter("nope").is_none());
    }

    #[test]
    fn resolve_program_prefers_env_override() {
        let claude = find_adapter("claude-code").unwrap();
        // Default (no override) — guard against a leaked env var from another test.
        std::env::remove_var("UAW_AGENT_BIN");
        assert_eq!(resolve_program(&claude), "claude");
        std::env::set_var("UAW_AGENT_BIN", "/tmp/fake-agent");
        assert_eq!(resolve_program(&claude), "/tmp/fake-agent");
        std::env::remove_var("UAW_AGENT_BIN");
    }
}
```

- [ ] **Step 2: Register** — `src-tauri/src/services/mod.rs`: add `pub mod agent;`.

- [ ] **Step 3: Verify + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace/src-tauri && cargo test services::agent::tests
cd /Users/csaba/projects/unified-agentic-workspace
git add src-tauri/src/services/agent/mod.rs src-tauri/src/services/mod.rs
git commit -m "feat(m10a): agent CLI adapter registry + capabilities"
```
Expected: 2 tests pass. (NOTE: the two adapter tests both touch `UAW_AGENT_BIN`; run with `cargo test services::agent -- --test-threads=1` if they interfere, or keep them in one test — but the plan keeps the env removal guards so default-thread runs are safe.)

---

## Task 3: PTY service

**Files:**
- Create: `src-tauri/src/services/agent/pty.rs`

- [ ] **Step 1: Create `src-tauri/src/services/agent/pty.rs`**

```rust
//! Thin wrapper over `portable-pty`: spawn an interactive command in a PTY and
//! pump its output. The Tauri layer (commands/agent_sessions.rs) wires the pump
//! to persistence + event emission; this file stays free of Tauri/DB so the read
//! loop and spawn are unit-testable.

use std::io::Read;
use std::path::Path;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

/// Live handles for a running PTY session, stored in the process registry.
pub struct PtyHandle {
    pub writer: Box<dyn std::io::Write + Send>,
    pub master: Box<dyn MasterPty + Send>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
}

/// What `spawn` returns: the registry handle plus the pieces the reader thread
/// owns (the output reader and the child to reap).
pub struct Spawned {
    pub handle: PtyHandle,
    pub reader: Box<dyn Read + Send>,
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
}

/// Spawn `program args` in a PTY with `cwd`. The slave is dropped after spawning
/// so the reader observes EOF when the child exits.
pub fn spawn(
    program: &str,
    args: &[&str],
    cwd: &Path,
    cols: u16,
    rows: u16,
) -> Result<Spawned, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("failed to open pty: {e}"))?;

    let mut cmd = CommandBuilder::new(program);
    cmd.args(args);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("failed to start agent '{program}': {e}"))?;
    // Drop the slave so the reader hits EOF when the child exits.
    drop(pair.slave);

    let killer = child.clone_killer();
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("failed to read pty: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("failed to write pty: {e}"))?;

    Ok(Spawned {
        handle: PtyHandle { writer, master: pair.master, killer },
        reader,
        child,
    })
}

/// Read `reader` to EOF, handing each non-empty chunk to `on_chunk`. Returns when
/// the stream closes. Pure of Tauri/DB so it is unit-testable.
pub fn pump<R: Read, F: FnMut(&[u8])>(mut reader: R, mut on_chunk: F) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => on_chunk(&buf[..n]),
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pump_delivers_all_bytes_then_stops() {
        let data = b"hello\nworld\n";
        let mut collected: Vec<u8> = Vec::new();
        pump(&data[..], |chunk| collected.extend_from_slice(chunk));
        assert_eq!(collected, data);
    }

    #[test]
    fn spawn_runs_a_command_in_a_pty_and_exits() {
        let dir = std::env::temp_dir();
        let mut spawned = spawn("sh", &["-c", "printf RUNOK"], &dir, 80, 24)
            .expect("spawn sh in pty");
        let mut out: Vec<u8> = Vec::new();
        pump(spawned.reader, |chunk| out.extend_from_slice(chunk));
        let status = spawned.child.wait().expect("child waits");
        assert!(status.success());
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("RUNOK"), "pty output was {text:?}");
    }
}
```

- [ ] **Step 2: Verify + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace/src-tauri && cargo test services::agent::pty
cd /Users/csaba/projects/unified-agentic-workspace
git add src-tauri/src/services/agent/pty.rs
git commit -m "feat(m10a): PTY spawn + pump service over portable-pty"
```
Expected: 2 tests pass (one pumps a byte slice; one spawns `sh` in a real PTY). If `child.clone_killer()` / `ExitStatus` method names differ in the installed 0.9 crate, adjust to the crate's actual API (e.g. `Child` may already be a `ChildKiller`); keep the behavior identical.

---

## Task 4: Commands + reader-thread runner + process registry

**Files:**
- Create: `src-tauri/src/commands/agent_sessions.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Create `src-tauri/src/commands/agent_sessions.rs`**

```rust
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::models::agent_session::{self, AgentSession};
use crate::models::{coding_workspace, event};
use crate::services::agent::{self, pty};
use crate::util::new_id;

/// Registry of live PTY sessions, keyed by agent-session id.
#[derive(Default)]
pub struct AgentProcesses(pub Mutex<HashMap<String, pty::PtyHandle>>);

#[derive(Clone, Serialize)]
struct AgentOutput {
    session_id: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Serialize)]
struct AgentExit {
    session_id: String,
    status: String,
    exit_code: Option<i64>,
}

/// Base directory for session transcripts: `UAW_TRANSCRIPTS_DIR` or
/// `<app_data_dir>/transcripts`.
fn transcripts_base(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("UAW_TRANSCRIPTS_DIR") {
        return Ok(PathBuf::from(dir));
    }
    app.path()
        .app_data_dir()
        .map(|d| d.join("transcripts"))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_agent_adapters() -> Vec<agent::AgentAdapter> {
    agent::adapters()
}

#[tauri::command]
pub fn list_agent_sessions(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
) -> Result<Vec<AgentSession>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    agent_session::list_by_coding_workspace(&conn, &coding_workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent_session(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Option<AgentSession>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    agent_session::get(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent_session_transcript(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<String, String> {
    let path = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(s) = agent_session::get(&conn, &id).map_err(|e| e.to_string())? else {
            return Err(format!("Agent session '{id}' does not exist"));
        };
        s.transcript_path
    };
    Ok(std::fs::read_to_string(&path).unwrap_or_default())
}

#[tauri::command]
pub fn start_agent_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    adapter_id: String,
    cols: u16,
    rows: u16,
) -> Result<AgentSession, String> {
    let Some(adapter) = agent::find_adapter(&adapter_id) else {
        return Err(format!("Unknown agent adapter '{adapter_id}'"));
    };

    // Resolve the worktree + its workspace under the lock, then release it.
    let (workspace_id, worktree_path) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) =
            coding_workspace::get(&conn, &coding_workspace_id).map_err(|e| e.to_string())?
        else {
            return Err(format!("Coding workspace '{coding_workspace_id}' does not exist"));
        };
        (cw.workspace_id, cw.worktree_path)
    };

    let program = agent::resolve_program(&adapter);
    let id = new_id();

    // Prepare the transcript file.
    let base = transcripts_base(&app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("failed to create transcripts dir: {e}"))?;
    let transcript_path = base.join(format!("{id}.log"));
    let transcript_str = transcript_path.to_string_lossy().to_string();

    // Spawn the PTY.
    let args: Vec<&str> = adapter.args.clone();
    let spawned = pty::spawn(&program, &args, Path::new(&worktree_path), cols, rows)?;
    let pty::Spawned { handle, reader, mut child } = spawned;

    // Insert the session row.
    let session = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        agent_session::create(
            &conn, &id, &workspace_id, &coding_workspace_id, &adapter.id, &program, &transcript_str,
        )
        .map_err(|e| e.to_string())?
    };

    // Register the handle for input/resize/stop.
    {
        let procs = app.state::<AgentProcesses>();
        procs.0.lock().map_err(|e| e.to_string())?.insert(id.clone(), handle);
    }

    // Record session.started.
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let payload = serde_json::json!({ "agent_session_id": id, "adapter_id": adapter.id }).to_string();
        let _ = event::create(&conn, &new_id(), &workspace_id, "session.started", &payload);
    }

    // Stream PTY output on a background thread: transcript + emit; on EOF reap.
    let thread_app = app.clone();
    let thread_id = id.clone();
    let thread_ws = workspace_id.clone();
    std::thread::spawn(move || {
        let mut transcript = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)
            .ok();
        pty::pump(reader, |chunk| {
            if let Some(f) = transcript.as_mut() {
                let _ = f.write_all(chunk);
            }
            let _ = thread_app.emit(
                "agent-output",
                AgentOutput { session_id: thread_id.clone(), bytes: chunk.to_vec() },
            );
        });

        let (status, code) = match child.wait() {
            Ok(s) if s.success() => ("exited".to_string(), Some(s.exit_code() as i64)),
            Ok(s) => ("failed".to_string(), Some(s.exit_code() as i64)),
            Err(_) => ("failed".to_string(), None),
        };

        if let Some(conn) = thread_app.try_state::<Mutex<Connection>>() {
            if let Ok(conn) = conn.lock() {
                let _ = agent_session::mark_exited(&conn, &thread_id, &status, code);
                let payload = serde_json::json!({ "agent_session_id": thread_id, "status": status }).to_string();
                let _ = event::create(&conn, &new_id(), &thread_ws, "agent.exited", &payload);
            }
        }
        let _ = thread_app.emit(
            "agent-exit",
            AgentExit { session_id: thread_id.clone(), status, exit_code: code },
        );
        if let Some(procs) = thread_app.try_state::<AgentProcesses>() {
            if let Ok(mut map) = procs.0.lock() {
                map.remove(&thread_id);
            }
        }
    });

    Ok(session)
}

#[tauri::command]
pub fn write_agent_session(app: AppHandle, id: String, data: String) -> Result<(), String> {
    let procs = app.state::<AgentProcesses>();
    let mut map = procs.0.lock().map_err(|e| e.to_string())?;
    let Some(handle) = map.get_mut(&id) else {
        return Err("Agent session is not running".into());
    };
    handle.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    handle.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resize_agent_session(
    app: AppHandle,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let procs = app.state::<AgentProcesses>();
    let map = procs.0.lock().map_err(|e| e.to_string())?;
    let Some(handle) = map.get(&id) else {
        return Ok(()); // resizing a finished session is a no-op
    };
    handle
        .master
        .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_agent_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<(), String> {
    // Mark stopped first so the reader thread's mark_exited (guarded on 'running')
    // won't override it, then kill the child (which closes the reader → reap).
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let _ = agent_session::set_status(&conn, &id, "stopped");
    }
    let procs = app.state::<AgentProcesses>();
    let mut map = procs.0.lock().map_err(|e| e.to_string())?;
    if let Some(handle) = map.get_mut(&id) {
        let _ = handle.killer.kill();
    }
    Ok(())
}
```

- [ ] **Step 2: Register module + state + commands**

In `src-tauri/src/commands/mod.rs` add `pub mod agent_sessions;`.

In `src-tauri/src/lib.rs`:
1. In the `.setup(...)` closure, after `app.manage(Mutex::new(conn));`, add:
```rust
            app.manage(commands::agent_sessions::AgentProcesses::default());
```
2. In `generate_handler!`, add:
```rust
            commands::agent_sessions::list_agent_adapters,
            commands::agent_sessions::list_agent_sessions,
            commands::agent_sessions::get_agent_session,
            commands::agent_sessions::get_agent_session_transcript,
            commands::agent_sessions::start_agent_session,
            commands::agent_sessions::write_agent_session,
            commands::agent_sessions::resize_agent_session,
            commands::agent_sessions::stop_agent_session,
```

- [ ] **Step 3: Build + full test + clippy**

```bash
cd /Users/csaba/projects/unified-agentic-workspace/src-tauri
cargo build 2>&1 | tail -3
cargo test 2>&1 | grep "test result:" | tail -3
cargo clippy --all-targets -- -D warnings 2>&1 | tail -8
```
Expected: builds; all tests pass; clippy clean. The `tauri::Emitter` trait import is required for `app.emit`. If `try_state` is unavailable in the installed Tauri version, use `app.state::<T>()` (it returns the managed state; the thread holds an owned `AppHandle` clone so this is valid).

- [ ] **Step 4: Commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace
git add src-tauri/src/commands/agent_sessions.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(m10a): agent session commands + PTY streaming runner + registry"
```

---

## Task 5: Frontend types + api + store

**Files:**
- Modify: `package.json`
- Create: `src/types/agentSession.ts`
- Create: `src/api/agentSessions.ts`
- Create: `src/stores/agentSessions.ts`

- [ ] **Step 1: Add xterm deps**

```bash
cd /Users/csaba/projects/unified-agentic-workspace
pnpm add @xterm/xterm @xterm/addon-fit
```

- [ ] **Step 2: Create `src/types/agentSession.ts`**

```ts
export interface AgentCapabilities {
  streaming: boolean;
  tool_use: boolean;
  mcp: boolean;
  file_edits: boolean;
  shell_commands: boolean;
  multi_turn: boolean;
}

export interface AgentAdapter {
  id: string;
  name: string;
  program: string;
  args: string[];
  capabilities: AgentCapabilities;
}

export interface AgentSession {
  id: string;
  workspace_id: string;
  coding_workspace_id: string;
  adapter_id: string;
  command: string;
  status: string; // running | exited | stopped | failed
  exit_code: number | null;
  transcript_path: string;
  created_at: string;
  updated_at: string;
}

/** Streamed PTY output (raw bytes as a number array). */
export interface AgentOutput {
  session_id: string;
  bytes: number[];
}

export interface AgentExit {
  session_id: string;
  status: string;
  exit_code: number | null;
}
```

- [ ] **Step 3: Create `src/api/agentSessions.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";
import type { AgentAdapter, AgentSession } from "../types/agentSession";

export function listAgentAdapters(): Promise<AgentAdapter[]> {
  return invoke<AgentAdapter[]>("list_agent_adapters");
}

export function listAgentSessions(codingWorkspaceId: string): Promise<AgentSession[]> {
  return invoke<AgentSession[]>("list_agent_sessions", { codingWorkspaceId });
}

export function startAgentSession(
  codingWorkspaceId: string,
  adapterId: string,
  cols: number,
  rows: number,
): Promise<AgentSession> {
  return invoke<AgentSession>("start_agent_session", { codingWorkspaceId, adapterId, cols, rows });
}

export function writeAgentSession(id: string, data: string): Promise<void> {
  return invoke<void>("write_agent_session", { id, data });
}

export function resizeAgentSession(id: string, cols: number, rows: number): Promise<void> {
  return invoke<void>("resize_agent_session", { id, cols, rows });
}

export function stopAgentSession(id: string): Promise<void> {
  return invoke<void>("stop_agent_session", { id });
}

export function getAgentSessionTranscript(id: string): Promise<string> {
  return invoke<string>("get_agent_session_transcript", { id });
}
```

- [ ] **Step 4: Create `src/stores/agentSessions.ts`**

```ts
import { ref } from "vue";
import { defineStore } from "pinia";
import type { AgentAdapter, AgentSession } from "../types/agentSession";
import * as api from "../api/agentSessions";

/** An open terminal tab: a started session plus its current status. */
export interface OpenTab {
  session: AgentSession;
}

export const useAgentSessionsStore = defineStore("agentSessions", () => {
  const adapters = ref<AgentAdapter[]>([]);
  const tabs = ref<OpenTab[]>([]);
  const activeId = ref<string | null>(null);
  const error = ref<string | null>(null);

  async function loadAdapters() {
    try {
      adapters.value = await api.listAgentAdapters();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function start(codingWorkspaceId: string, adapterId: string, cols: number, rows: number) {
    const session = await api.startAgentSession(codingWorkspaceId, adapterId, cols, rows);
    tabs.value.push({ session });
    activeId.value = session.id;
    return session;
  }

  async function stop(id: string) {
    await api.stopAgentSession(id);
  }

  function setStatus(id: string, status: string, exitCode: number | null) {
    const tab = tabs.value.find((t) => t.session.id === id);
    if (tab) {
      tab.session.status = status;
      tab.session.exit_code = exitCode;
    }
  }

  function closeTab(id: string) {
    tabs.value = tabs.value.filter((t) => t.session.id !== id);
    if (activeId.value === id) {
      activeId.value = tabs.value.length ? tabs.value[tabs.value.length - 1].session.id : null;
    }
  }

  return { adapters, tabs, activeId, error, loadAdapters, start, stop, setStatus, closeTab };
});
```

- [ ] **Step 5: Build + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build && pnpm format
git add package.json pnpm-lock.yaml pnpm-workspace.yaml src/types/agentSession.ts src/api/agentSessions.ts src/stores/agentSessions.ts
git commit -m "feat(m10a): agent session types, api, store + xterm deps"
```
Expected: `pnpm build` succeeds. (pnpm may add the new xterm packages to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` — include it in the commit if so.)

---

## Task 6: TerminalTab component

**Files:**
- Create: `src/components/TerminalTab.vue`

- [ ] **Step 1: Create `src/components/TerminalTab.vue`**

```vue
<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import * as api from "../api/agentSessions";
import { useAgentSessionsStore } from "../stores/agentSessions";
import type { AgentOutput, AgentExit } from "../types/agentSession";

const props = defineProps<{ sessionId: string }>();
const store = useAgentSessionsStore();

const host = ref<HTMLDivElement | null>(null);
let term: Terminal | null = null;
let fit: FitAddon | null = null;
let resizeObserver: ResizeObserver | null = null;
const unlisten: UnlistenFn[] = [];

onMounted(async () => {
  term = new Terminal({ convertEol: false, cursorBlink: true, fontSize: 13 });
  fit = new FitAddon();
  term.loadAddon(fit);
  if (host.value) term.open(host.value);
  fit.fit();

  // Replay any existing transcript (reopened/finished session), then go live.
  try {
    const transcript = await api.getAgentSessionTranscript(props.sessionId);
    if (transcript) term.write(transcript);
  } catch {
    /* a brand-new session has no transcript yet */
  }

  // User keystrokes → PTY.
  term.onData((data) => {
    api.writeAgentSession(props.sessionId, data).catch(() => {});
  });

  // Live output + exit, routed by session id.
  unlisten.push(
    await listen<AgentOutput>("agent-output", (e) => {
      if (e.payload.session_id === props.sessionId && term) {
        term.write(new Uint8Array(e.payload.bytes));
      }
    }),
  );
  unlisten.push(
    await listen<AgentExit>("agent-exit", (e) => {
      if (e.payload.session_id === props.sessionId) {
        store.setStatus(props.sessionId, e.payload.status, e.payload.exit_code);
      }
    }),
  );

  // Keep the PTY size in sync with the container.
  resizeObserver = new ResizeObserver(() => {
    if (!fit || !term) return;
    fit.fit();
    api.resizeAgentSession(props.sessionId, term.cols, term.rows).catch(() => {});
  });
  if (host.value) resizeObserver.observe(host.value);
  // Push the initial fitted size to the backend.
  if (term) api.resizeAgentSession(props.sessionId, term.cols, term.rows).catch(() => {});
});

onBeforeUnmount(() => {
  unlisten.forEach((u) => u());
  resizeObserver?.disconnect();
  term?.dispose();
});
</script>

<template>
  <div ref="host" class="terminal" data-testid="agent-terminal"></div>
</template>

<style scoped>
.terminal {
  width: 100%;
  height: 100%;
  min-height: 24rem;
  background: #000;
  padding: 0.25rem;
  border-radius: var(--re-radius-md, 6px);
}
</style>
```

- [ ] **Step 2: Build + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build && pnpm format
git add src/components/TerminalTab.vue
git commit -m "feat(m10a): TerminalTab — xterm.js bound to a PTY session"
```
Expected: build succeeds.

---

## Task 7: AgentsView + navigation

**Files:**
- Create: `src/components/AgentsView.vue`
- Modify: `src/App.vue`

- [ ] **Step 1: Create `src/components/AgentsView.vue`**

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";
import { useCodingWorkspacesStore } from "../stores/codingWorkspaces";
import { useToast } from "../composables/useToast";
import TerminalTab from "./TerminalTab.vue";

const store = useAgentSessionsStore();
const coding = useCodingWorkspacesStore();
const toast = useToast();

const newWorktreeId = ref("");
const newAdapterId = ref("");
const starting = ref(false);

onMounted(async () => {
  await store.loadAdapters();
  if (store.adapters.length > 0) newAdapterId.value = store.adapters[0].id;
});

const canStart = computed(
  () => newWorktreeId.value !== "" && newAdapterId.value !== "" && !starting.value,
);

const worktreeLabel = (id: string) => {
  const cw = coding.list.find((c) => c.id === id);
  return cw ? cw.branch_name : id;
};
const adapterLabel = (id: string) => store.adapters.find((a) => a.id === id)?.name ?? id;

async function openTerminal() {
  if (!canStart.value) return;
  starting.value = true;
  try {
    // 80x24 is a safe initial size; the TerminalTab fits + resizes on mount.
    await store.start(newWorktreeId.value, newAdapterId.value, 80, 24);
  } catch (e) {
    toast.error(String(e));
  } finally {
    starting.value = false;
  }
}
</script>

<template>
  <section class="agents">
    <header class="agents__bar">
      <ul class="tabs">
        <li
          v-for="t in store.tabs"
          :key="t.session.id"
          class="tab"
          :class="{ 'tab--active': t.session.id === store.activeId }"
          data-testid="agent-tab"
          @click="store.activeId = t.session.id"
        >
          <span class="tab__label">
            {{ adapterLabel(t.session.adapter_id) }} · {{ worktreeLabel(t.session.coding_workspace_id) }}
          </span>
          <span class="re-badge" :data-tone="t.session.status === 'running' ? 'info' : undefined">
            {{ t.session.status }}
          </span>
          <button
            type="button"
            class="tab__close"
            aria-label="Close terminal tab"
            @click.stop="store.closeTab(t.session.id)"
          >
            ×
          </button>
        </li>
      </ul>

      <form class="new" @submit.prevent="openTerminal">
        <select v-model="newWorktreeId" class="re-select" data-size="sm" aria-label="Agent worktree">
          <option value="" disabled>Worktree</option>
          <option v-for="cw in coding.list" :key="cw.id" :value="cw.id">{{ cw.branch_name }}</option>
        </select>
        <select v-model="newAdapterId" class="re-select" data-size="sm" aria-label="Agent CLI">
          <option v-for="a in store.adapters" :key="a.id" :value="a.id">{{ a.name }}</option>
        </select>
        <button class="re-button" data-variant="brand" data-size="sm" type="submit" :disabled="!canStart">
          New terminal
        </button>
      </form>
    </header>

    <p v-if="coding.list.length === 0" class="muted hint">
      Create a worktree in Coding first, then open an agent terminal here.
    </p>

    <div v-if="store.activeId" class="agents__pane">
      <!-- Keep each terminal mounted so its xterm + stream persist across tab switches. -->
      <div
        v-for="t in store.tabs"
        v-show="t.session.id === store.activeId"
        :key="t.session.id"
        class="agents__term"
      >
        <div class="agents__termhead">
          <span class="muted">{{ t.session.command }} · {{ t.session.status }}</span>
          <button
            v-if="t.session.status === 'running'"
            type="button"
            class="re-button"
            data-variant="danger"
            data-size="sm"
            @click="store.stop(t.session.id)"
          >
            Stop
          </button>
        </div>
        <TerminalTab :session-id="t.session.id" />
      </div>
    </div>
    <p v-else class="muted">No terminals open. Pick a worktree and a CLI to start one.</p>
  </section>
</template>

<style scoped>
.agents {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  height: 100%;
}
.agents__bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
}
.tabs {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 0.35rem;
}
.tab {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.3rem 0.55rem;
  border: 1px solid var(--re-color-border);
  border-radius: var(--re-radius-md, 6px);
  cursor: pointer;
  font-size: 0.8rem;
}
.tab--active {
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--re-color-accent-600) 45%, transparent);
}
.tab__close {
  border: none;
  background: none;
  cursor: pointer;
  color: var(--re-color-text-muted);
  font-size: 1rem;
  line-height: 1;
}
.new {
  display: flex;
  gap: 0.35rem;
}
.agents__pane {
  flex: 1;
  min-height: 0;
}
.agents__term {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  height: 100%;
}
.agents__termhead {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 0.8rem;
}
.hint {
  font-size: 0.85rem;
}
.muted {
  color: var(--re-color-text-muted);
}
</style>
```

- [ ] **Step 2: Wire into `src/App.vue` — script**

1. Import after `ReviewsView`:
```ts
import AgentsView from "./components/AgentsView.vue";
```
2. Import the store after `useReviewsStore`:
```ts
import { useAgentSessionsStore } from "./stores/agentSessions";
```
3. Instance after `const reviews = useReviewsStore();`:
```ts
const agentSessions = useAgentSessionsStore();
```
4. Extend `ActiveView`:
```ts
type ActiveView = "inbox" | "projects" | "sources" | "coding" | "reviews" | "agents";
```
5. The `plannedSections` array currently reads `["Skills", "Automations", "Settings"]`. Leave it as-is (Agents is a real nav item below).

- [ ] **Step 3: Wire into `src/App.vue` — template**

After the Reviews nav button (`@click="activeView = 'reviews'"`), add:

```vue
        <button
          class="re-button"
          data-variant="ghost"
          :aria-current="activeView === 'agents' ? 'page' : undefined"
          type="button"
          @click="activeView = 'agents'"
        >
          Agents
        </button>
```

After the `<ReviewsView v-else-if="activeView === 'reviews'" />` line, add:

```vue
        <AgentsView v-else-if="activeView === 'agents'" />
```

- [ ] **Step 4: Build + format + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build && pnpm format
git add src/components/AgentsView.vue src/App.vue
git commit -m "feat(m10a): Agents view with terminal tabs + nav"
```
Expected: build succeeds.

---

## Task 8: e2e — interactive agent terminal

**Files:**
- Modify: `scripts/run-e2e.sh`
- Create: `e2e/specs/agent-terminal.e2e.ts`

- [ ] **Step 1: Add a fake interactive agent to `scripts/run-e2e.sh`**

In `scripts/run-e2e.sh`, after the fixture-repo block (before the `node_modules/.bin/wdio run` line), add a fake interactive "agent" the e2e injects via `UAW_AGENT_BIN`. It prints a banner, then echoes stdin (so typing is reflected), exactly like a real interactive CLI in a PTY:

```bash
# A fake interactive "agent CLI" for the agent-terminal e2e: prints a banner then
# echoes stdin, so the PTY/xterm round-trip can be asserted without a real claude.
cat >/tmp/uaw-fake-agent <<'AGENT'
#!/usr/bin/env bash
printf 'AGENT-READY\n'
exec cat
AGENT
chmod +x /tmp/uaw-fake-agent
```

(`UAW_AGENT_BIN` is set per-spec in `wdio.conf.ts` — see Step 2.)

- [ ] **Step 2: Set `UAW_AGENT_BIN` for this spec in `wdio.conf.ts`**

In `wdio.conf.ts`, the `beforeSession(_config, _capabilities, specs)` hook already sets per-spec env (`UAW_DB_PATH`, `UAW_WORKTREES_DIR`). Add a transcripts dir and the fake agent binary so the agent spec is isolated. Inside that hook, after the existing `process.env.UAW_WORKTREES_DIR = ...` assignment, add:

```ts
    process.env.UAW_TRANSCRIPTS_DIR = path.join(sessionDir, "transcripts");
    process.env.UAW_AGENT_BIN = "/tmp/uaw-fake-agent";
```

(These are harmless for the other specs and make the agent spec deterministic.)

- [ ] **Step 3: Create `e2e/specs/agent-terminal.e2e.ts`**

```ts
import { browser, $, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

const REPO = "/tmp/fixture-repo-agent";

/**
 * Milestone 10a end-to-end: open an interactive agent terminal (a fake CLI
 * injected via UAW_AGENT_BIN that prints a banner then echoes stdin) against a
 * worktree, and verify the PTY/xterm round-trip — banner renders, typed input is
 * echoed back, and the session can be stopped. Uses its own fixture repo.
 */
describe("agent terminals", () => {
  before(async () => {
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (args: string[]) => execFileSync("git", ["-C", REPO, ...args], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "agent@uaw.local"]);
    git(["config", "user.name", "UAW Agent"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# agent fixture\n");
    git(["add", "."]);
    git(["commit", "-m", "init"]);

    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a code project + repo + worktree", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("AgentProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("AgentFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });

    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("AgentProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("AgentFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/agent");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });
  });

  it("opens a terminal, renders the agent banner, echoes input, and stops", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/agent");
    // The CLI defaults to the first adapter (Claude Code); UAW_AGENT_BIN makes it
    // run our fake regardless.
    await (await $("button*=New terminal")).click();

    const term = await $('[data-testid="agent-terminal"]');
    await term.waitForExist({ timeout: 10_000 });

    // The fake agent prints AGENT-READY into the PTY → xterm renders it.
    await browser.waitUntil(
      async () => (await textOf('[data-testid="agent-terminal"]')).includes("AGENT-READY"),
      { timeout: 15_000, timeoutMsg: "expected the agent banner to render in the terminal" },
    );

    // Type into the terminal; the fake echoes it back through the PTY.
    await term.click();
    await browser.keys("ping-uaw");
    await browser.keys("Enter");
    await browser.waitUntil(
      async () => (await textOf('[data-testid="agent-terminal"]')).includes("ping-uaw"),
      { timeout: 15_000, timeoutMsg: "expected typed input to be echoed in the terminal" },
    );

    // Stop the session; the tab status reflects a terminal state.
    await (await $("button*=Stop")).click();
    await browser.waitUntil(
      async () => {
        const t = (await textOf('[data-testid="agent-tab"]')).toLowerCase();
        return t.includes("stopped") || t.includes("exited") || t.includes("failed");
      },
      { timeout: 15_000, timeoutMsg: "expected the session to reach a terminal status" },
    );
  });
});
```

- [ ] **Step 4: Typecheck + run e2e**

```bash
cd /Users/csaba/projects/unified-agentic-workspace
pnpm e2e:typecheck
```
Then the orchestrator runs `pnpm e2e:docker` separately (do not run the long Docker build here). Confirm `pnpm e2e:typecheck` and `pnpm format` are clean.

- [ ] **Step 5: Commit**

```bash
git add scripts/run-e2e.sh wdio.conf.ts e2e/specs/agent-terminal.e2e.ts
git commit -m "test(m10a): e2e interactive agent terminal (fake CLI over PTY/xterm)"
```

---

## Self-review notes

- **Spec coverage:** PTY runtime + xterm rendering (Tasks 3/4/6) · tab = terminal bound to a worktree (Task 7) · claude/codex/gemini registry + `UAW_AGENT_BIN` override (Task 2) · `agent_sessions` + transcript persistence + `events` lifecycle rows (Tasks 1/4) · adapter trait/capabilities shape (Task 2) · commands start/write/resize/stop/get/list/transcript (Task 4) · top tab bar + new-terminal picker (Task 7) · review stays manual via M9 (no change needed). All spec items mapped.
- **Type consistency:** `AgentSession` fields match between migration, Rust model, and `types/agentSession.ts`. `start_agent_session` invoked with `{ codingWorkspaceId, adapterId, cols, rows }` ↔ Rust snake_case params. `agent-output` payload `{ session_id, bytes: Vec<u8> }` ↔ `AgentOutput`/`new Uint8Array(bytes)`. `mark_exited` (running-guard) vs `set_status` (force, used by stop) used consistently. `PtyHandle{writer,master,killer}` fields match across pty.rs and the command registry.
- **Library-API caveats flagged in-task:** verify `portable-pty` 0.9 method names (`clone_killer`, `ExitStatus::exit_code`) and Tauri's `Emitter`/`try_state` against the installed versions; behavior is specified so an equivalent call is a drop-in.
- **Out of scope (M10b+), intentionally absent:** provider accounts/keychain/API adapter, model picker, live re-attach after app restart, the Coding→Agents deep-link (the Agents view's worktree picker covers it).
