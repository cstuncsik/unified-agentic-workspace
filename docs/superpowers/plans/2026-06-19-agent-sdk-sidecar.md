# Claude Agent SDK Sidecar (plan-only) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a headless Claude Agent SDK agent that analyzes a worktree and proposes a plan (no edits), streaming structured events to a read-only feed, reusing M10b-2a key injection.

**Architecture:** A new `claude-agent-sdk` adapter (`kind:"sdk"`) is spawned as a plain piped Node child (not a PTY) in the worktree with the account's key injected; the sidecar runs the SDK `query()` in `permissionMode:"plan"` and emits NDJSON; the backend masks the key in every line, persists the transcript, streams `agent-sdk-event`, and derives status from the `result` event. Pure functions (`redact`/`parse_sdk_line`/`pump_ndjson`/`sdk_status`) are the unit-tested seams.

**Tech Stack:** Rust + rusqlite + std::process + libc (process-group kill); Tauri 2; Node + `@anthropic-ai/claude-agent-sdk`; Vue 3 + Pinia; WebdriverIO.

---

## File Structure

- `src-tauri/src/db/migrations/0012_agent_session_kind.sql` (create), `db/mod.rs` (register 12)
- `src-tauri/src/models/agent_session.rs` (modify — `kind` column), `models/workspace.rs` (idempotency→12)
- `src-tauri/src/services/agent/mod.rs` (modify — adapter `kind`/`requires_account` + SDK adapter + `resolve_sdk_sidecar`)
- `src-tauri/src/services/agent/sdk.rs` (create — `redact`/`parse_sdk_line`/`pump_ndjson`/`sdk_status`/`spawn`/`SdkHandle`)
- `src-tauri/src/commands/agent_sessions.rs` (modify — `AgentProc` enum, `start_sdk_session`, write/resize/stop dispatch, `get_agent_sdk_transcript`, `validate_account_required`)
- `src-tauri/Cargo.toml` (modify — `libc`); `src-tauri/src/lib.rs` (register `get_agent_sdk_transcript`)
- `sidecar/claude-agent-sdk/{package.json,index.mjs,.gitignore}` (create)
- `src/types/agentSession.ts`, `src/api/agentSessions.ts`, `src/stores/agentSessions.ts`, `src/components/AgentsView.vue` (modify); `src/components/SdkRunView.vue` (create)
- `scripts/run-e2e.sh` (modify — fake sidecar), `wdio.conf.ts` (modify — `UAW_AGENT_SDK_SIDECAR`), `e2e/specs/agent-sdk.e2e.ts` (create)

Backend tests: `cargo test --manifest-path src-tauri/Cargo.toml`. CI/clippy is Linux.

---

## Task 1: Migration 0012 + `agent_sessions.kind`

**Files:** Create `src-tauri/src/db/migrations/0012_agent_session_kind.sql`; Modify `db/mod.rs`, `models/agent_session.rs`, `models/workspace.rs:144`.

- [ ] **Step 1: Write the migration**

`src-tauri/src/db/migrations/0012_agent_session_kind.sql`:
```sql
-- Distinguishes interactive PTY sessions ('pty') from headless Claude Agent SDK
-- runs ('sdk') so the frontend picks the right view without re-deriving from the
-- live adapter registry. Existing rows default to 'pty'.
ALTER TABLE agent_sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'pty';
```

- [ ] **Step 2: Register migration 12** — in `db/mod.rs`, after the `11` tuple:
```rust
    (
        12,
        "agent_session_kind",
        include_str!("migrations/0012_agent_session_kind.sql"),
    ),
```

- [ ] **Step 3: Bump idempotency** — `models/workspace.rs` `migrations_are_idempotent`: `assert_eq!(version, 12);`

- [ ] **Step 4: Run** `cargo test --manifest-path src-tauri/Cargo.toml migrations_are_idempotent` → PASS.

- [ ] **Step 5: Add `kind` to the model.** In `models/agent_session.rs`: add `pub kind: String,` to the struct (after `model_id`); add `kind` to `COLUMNS` (after `model_id`); add `kind: row.get("kind")?,` to `from_row`; change `create` to accept `kind: &str` (new last param) and the INSERT:
```rust
#[allow(clippy::too_many_arguments)]
pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    coding_workspace_id: &str,
    adapter_id: &str,
    command: &str,
    transcript_path: &str,
    account_id: Option<&str>,
    model_id: Option<&str>,
    kind: &str,
) -> rusqlite::Result<AgentSession> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO agent_sessions
           (id, workspace_id, coding_workspace_id, adapter_id, command, status,
            exit_code, transcript_path, account_id, model_id, kind, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', NULL, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            id, workspace_id, coding_workspace_id, adapter_id, command,
            transcript_path, account_id, model_id, kind, now
        ],
    )?;
    Ok(get(conn, id)?.expect("agent session exists immediately after insert"))
}
```
Update the test `make` helper in that file to pass `"pty"` as the final arg.

- [ ] **Step 6: Add a round-trip test** to the `tests` module:
```rust
    #[test]
    fn create_records_kind() {
        let conn = migrated_conn();
        let (ws, cw) = fixtures(&conn);
        let s = create(&conn, &new_id(), &ws, &cw, "claude-agent-sdk", "sidecar", "/tmp/t.log", None, None, "sdk").unwrap();
        assert_eq!(s.kind, "sdk");
        assert_eq!(get(&conn, &s.id).unwrap().unwrap().kind, "sdk");
    }
```

- [ ] **Step 7: Run** `cargo test --manifest-path src-tauri/Cargo.toml agent_session` → PASS (the `start_agent_session` call site won't compile yet — Task 6 fixes it; if the crate fails to build, temporarily add `"pty"` to that one call at `agent_sessions.rs:155` to keep green, Task 6 replaces it).

- [ ] **Step 8: Commit**
```bash
git add src-tauri/src/db/migrations/0012_agent_session_kind.sql src-tauri/src/db/mod.rs src-tauri/src/models/agent_session.rs src-tauri/src/models/workspace.rs src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2b): agent_sessions.kind column + model"
```

---

## Task 2: Adapter `kind`/`requires_account` + the SDK adapter + sidecar resolver

**Files:** Modify `src-tauri/src/services/agent/mod.rs`.

- [ ] **Step 1: Add fields** to `AgentAdapter` (after `clear_env`):
```rust
    /// "pty" (interactive terminal) | "sdk" (headless Node sidecar).
    pub kind: &'static str,
    /// SDK adapters require a bound account (no silent ambient identity).
    pub requires_account: bool,
```

- [ ] **Step 2: Set fields on the 3 CLIs + add the SDK adapter.** Each existing adapter gets `kind: "pty", requires_account: false,`. Append a fourth:
```rust
        AgentAdapter {
            id: "claude-agent-sdk",
            name: "Claude Agent SDK",
            program: "", // resolved at runtime via resolve_sdk_sidecar()
            args: vec![],
            provider: Some("anthropic"),
            api_key_env: Some("ANTHROPIC_API_KEY"),
            clear_env: vec!["ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"],
            kind: "sdk",
            requires_account: true,
            capabilities: full_capabilities(),
        },
```

- [ ] **Step 3: Add the resolver** (after `resolve_program`):
```rust
/// The Node sidecar entry to spawn for the SDK adapter: `UAW_AGENT_SDK_SIDECAR`
/// overrides (so e2e injects a fake) else the bundled default path.
pub fn resolve_sdk_sidecar() -> String {
    match std::env::var("UAW_AGENT_SDK_SIDECAR") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => "sidecar/claude-agent-sdk/index.mjs".to_string(),
    }
}
```

- [ ] **Step 4: Extend the registry test** — in `registry_has_the_three_clis` (rename mentally; keep name), add:
```rust
        let sdk = find_adapter("claude-agent-sdk").unwrap();
        assert_eq!(sdk.kind, "sdk");
        assert!(sdk.requires_account);
        assert_eq!(sdk.provider, Some("anthropic"));
        let claude = find_adapter("claude-code").unwrap();
        assert_eq!(claude.kind, "pty");
        assert!(!claude.requires_account);
```
And a resolver test:
```rust
    #[test]
    fn resolve_sdk_sidecar_prefers_env() {
        std::env::remove_var("UAW_AGENT_SDK_SIDECAR");
        assert!(resolve_sdk_sidecar().ends_with("index.mjs"));
        std::env::set_var("UAW_AGENT_SDK_SIDECAR", "/tmp/fake-sdk");
        assert_eq!(resolve_sdk_sidecar(), "/tmp/fake-sdk");
        std::env::remove_var("UAW_AGENT_SDK_SIDECAR");
    }
```

- [ ] **Step 5: Run + commit**
```bash
cargo test --manifest-path src-tauri/Cargo.toml services::agent
git add src-tauri/src/services/agent/mod.rs
git commit -m "feat(m10b-2b): claude-agent-sdk adapter + kind/requires_account + sidecar resolver"
```

---

## Task 3: Pure SDK helpers — `services/agent/sdk.rs`

**Files:** Create `src-tauri/src/services/agent/sdk.rs`; Modify `src-tauri/src/services/agent/mod.rs` (add `pub mod sdk;` at top).

- [ ] **Step 1: Write the failing tests + the pure functions.** Create `src-tauri/src/services/agent/sdk.rs`:
```rust
//! Claude Agent SDK sidecar runtime. The sidecar (a Node process) runs the SDK's
//! `query()` headlessly and emits one NDJSON object per message; this module
//! parses those lines, masks the injected key, derives terminal status, and owns
//! the piped-child spawn + process-group kill. The pure functions below are the
//! unit-tested seams; the transcript-write/emit closure lives in the command.

use std::io::BufRead;

/// Mask the injected API key value anywhere it appears in a line before the line
/// is persisted or emitted. The SDK agent authors content we relay, so a
/// prompt-injected run could otherwise print the key into the transcript/feed.
pub fn redact(line: &str, secret: &str) -> String {
    if secret.is_empty() {
        line.to_string()
    } else {
        line.replace(secret, "***")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SdkLine {
    /// A relayable event line (kind ∈ assistant|tool|result); `raw` is the JSON.
    Event { kind: String, raw: String },
    /// An {"type":"error"} line; carries the message only.
    Error(String),
    /// Blank / non-JSON / unknown-type line — dropped (never panics).
    Skip,
}

/// Classify one NDJSON line. Never panics; bad/unknown input → Skip.
pub fn parse_sdk_line(line: &str) -> SdkLine {
    let t = line.trim();
    if t.is_empty() {
        return SdkLine::Skip;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(t) else {
        return SdkLine::Skip; // non-JSON garbage — drop, don't crash
    };
    match v.get("type").and_then(|x| x.as_str()) {
        Some("assistant") | Some("tool") | Some("result") => SdkLine::Event {
            kind: v["type"].as_str().unwrap().to_string(),
            raw: t.to_string(),
        },
        Some("error") => SdkLine::Error(
            v.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Agent error")
                .to_string(),
        ),
        _ => SdkLine::Skip, // system/init etc. — ignore
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct SdkOutcome {
    pub saw_result: bool,
    pub saw_error: bool,
}

/// Drive a reader of NDJSON, calling `on` per parsed line; returns the terminal
/// signals. Byte-oriented (`read_until`) so long / non-UTF8 lines don't kill the
/// stream the way `lines()` would. Pure of Tauri/DB/child — unit-testable.
pub fn pump_ndjson<R: BufRead, F: FnMut(&SdkLine)>(mut reader: R, mut on: F) -> SdkOutcome {
    let mut out = SdkOutcome::default();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let line = String::from_utf8_lossy(&buf);
        let parsed = parse_sdk_line(&line);
        match &parsed {
            SdkLine::Event { kind, raw } if kind == "result" => {
                out.saw_result = true;
                // Sidecar emits compact JSON, so this substring is reliable.
                if raw.contains("\"status\":\"error\"") {
                    out.saw_error = true;
                }
            }
            SdkLine::Error(_) => out.saw_error = true,
            _ => {}
        }
        on(&parsed);
    }
    out
}

/// Terminal status from the stream signals + the process exit. The `result` event
/// (not the exit code) is authoritative: a sidecar can exit 0 with an error result
/// or crash with none.
pub fn sdk_status(saw_result: bool, saw_error: bool, exit_code: Option<i64>) -> &'static str {
    if saw_result && !saw_error && exit_code == Some(0) {
        "exited"
    } else {
        "failed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_masks_only_when_present() {
        assert_eq!(redact("key=SEKRET here", "SEKRET"), "key=*** here");
        assert_eq!(redact("no secret", "SEKRET"), "no secret");
        assert_eq!(redact("anything", ""), "anything"); // empty secret = no-op
    }

    #[test]
    fn parse_classifies_and_never_panics() {
        assert!(matches!(parse_sdk_line(""), SdkLine::Skip));
        assert!(matches!(parse_sdk_line("not json"), SdkLine::Skip));
        assert!(matches!(parse_sdk_line("{\"type\":\"system\"}"), SdkLine::Skip));
        assert!(matches!(parse_sdk_line("{\"type\":\"assistant\",\"text\":\"hi\"}"), SdkLine::Event { .. }));
        assert!(matches!(parse_sdk_line("{\"type\":\"result\",\"status\":\"success\"}"), SdkLine::Event { .. }));
        assert_eq!(parse_sdk_line("{\"type\":\"error\",\"message\":\"boom\"}"), SdkLine::Error("boom".into()));
    }

    #[test]
    fn pump_skips_garbage_flags_result_and_error() {
        let canned = b"{\"type\":\"assistant\",\"text\":\"hi\"}\n\n\
                       garbage line\n\
                       {\"type\":\"tool\",\"name\":\"Read\"}\n\
                       {\"type\":\"result\",\"status\":\"success\"}\n";
        let mut events = 0;
        let out = pump_ndjson(&canned[..], |_| events += 1);
        assert_eq!(events, 5); // every line is delivered (incl. Skips); 3 are Events
        assert!(out.saw_result);
        assert!(!out.saw_error);

        let err = b"{\"type\":\"result\",\"status\":\"error\"}\n";
        let out2 = pump_ndjson(&err[..], |_| {});
        assert!(out2.saw_result && out2.saw_error);
    }

    #[test]
    fn status_table() {
        assert_eq!(sdk_status(true, false, Some(0)), "exited");
        assert_eq!(sdk_status(false, false, Some(0)), "failed"); // exited 0, no result = crash
        assert_eq!(sdk_status(true, true, Some(0)), "failed"); // error result
        assert_eq!(sdk_status(true, false, Some(1)), "failed"); // non-zero exit
        assert_eq!(sdk_status(true, false, None), "failed");
    }
}
```

- [ ] **Step 2: Register the module** — in `services/agent/mod.rs` top: `pub mod sdk;` (alongside `pub mod pty;`).

- [ ] **Step 3: Run + commit**
```bash
cargo test --manifest-path src-tauri/Cargo.toml services::agent::sdk
git add src-tauri/src/services/agent/sdk.rs src-tauri/src/services/agent/mod.rs
git commit -m "feat(m10b-2b): pure SDK NDJSON helpers (redact/parse/pump/status)"
```

---

## Task 4: Piped-child spawn + `SdkHandle` + env-override test

**Files:** Modify `src-tauri/src/services/agent/sdk.rs`; `src-tauri/Cargo.toml` (add `libc`).

- [ ] **Step 1: Add `libc`** to `src-tauri/Cargo.toml` `[dependencies]`:
```toml
libc = "0.2"
```

- [ ] **Step 2: Add `spawn` + `SdkHandle`** to `sdk.rs` (above the tests):
```rust
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

/// Live handle for a running SDK sidecar — kills the whole process group (the SDK
/// spawns a grandchild CLI, so killing only the Node PID would orphan it).
pub struct SdkHandle {
    pid: u32,
}

impl SdkHandle {
    pub fn kill(&self) {
        #[cfg(unix)]
        unsafe {
            // process_group(0) at spawn made the child a group leader (pgid == pid).
            libc::kill(-(self.pid as i32), libc::SIGTERM);
        }
        #[cfg(not(unix))]
        let _ = self.pid;
    }
}

pub struct SdkSpawned {
    pub stdout: ChildStdout,
    pub child: Child,
    pub handle: SdkHandle,
}

/// Spawn the sidecar as a plain piped child in `cwd`, goal as argv, env injected,
/// stdin null (the goal is argv, not stdin), stderr discarded (never relayed).
pub fn spawn(
    program: &str,
    goal: &str,
    cwd: &Path,
    env: &[(String, String)],
) -> Result<SdkSpawned, String> {
    let mut cmd = Command::new(program);
    cmd.arg(goal)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| "Failed to start the agent sidecar".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to start the agent sidecar".to_string())?;
    let pid = child.id();
    Ok(SdkSpawned {
        stdout,
        child,
        handle: SdkHandle { pid },
    })
}
```

- [ ] **Step 3: Add the env-override test** to `sdk.rs` `tests` (uses `printenv`, which takes the var NAME as its single arg — our "goal" slot — and prints its value; present on Linux + macOS):
```rust
    use std::io::{BufReader, Read};

    #[test]
    fn spawn_injects_env_overriding_inherited() {
        std::env::set_var("UAW_SDK_PROBE", "PARENT");
        let dir = std::env::temp_dir();
        let mut sp = spawn(
            "printenv",
            "UAW_SDK_PROBE", // argv (the goal slot) = the var name printenv echoes
            &dir,
            &[("UAW_SDK_PROBE".into(), "INJECTED".into())],
        )
        .expect("spawn printenv");
        let mut out = String::new();
        BufReader::new(&mut sp.stdout).read_to_string(&mut out).unwrap();
        sp.child.wait().unwrap();
        std::env::remove_var("UAW_SDK_PROBE");
        assert_eq!(out.trim(), "INJECTED"); // injected beats the inherited "PARENT"
    }
```

- [ ] **Step 4: Run + commit**
```bash
cargo test --manifest-path src-tauri/Cargo.toml services::agent::sdk
git add src-tauri/src/services/agent/sdk.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(m10b-2b): SDK sidecar piped spawn + process-group kill"
```

---

## Task 5: Process registry enum + write/resize/stop dispatch

**Files:** Modify `src-tauri/src/commands/agent_sessions.rs`.

- [ ] **Step 1: Replace the registry type.** At the top of `agent_sessions.rs`, add `sdk` to the agent import and replace the `AgentProcesses` definition:
```rust
use crate::services::agent::{self, pty, sdk, AgentAdapter};
```
```rust
/// A live agent process: an interactive PTY or a headless SDK sidecar.
pub enum AgentProc {
    Pty(pty::PtyHandle),
    Sdk(sdk::SdkHandle),
}

/// Registry of live sessions, keyed by agent-session id.
#[derive(Default)]
pub struct AgentProcesses(pub Mutex<HashMap<String, AgentProc>>);
```

- [ ] **Step 2: Update the PTY insert** in `start_agent_session` (the register block ~line 166): `.insert(id.clone(), AgentProc::Pty(handle));`

- [ ] **Step 3: Dispatch write/resize/stop.** Replace the three commands' handle access:
```rust
#[tauri::command]
pub fn write_agent_session(app: AppHandle, id: String, data: String) -> Result<(), String> {
    let procs = app.state::<AgentProcesses>();
    let mut map = procs.0.lock().map_err(|e| e.to_string())?;
    match map.get_mut(&id) {
        Some(AgentProc::Pty(h)) => {
            h.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
            h.writer.flush().map_err(|e| e.to_string())
        }
        Some(AgentProc::Sdk(_)) => Err("This agent does not accept input".into()),
        None => Err("Agent session is not running".into()),
    }
}

#[tauri::command]
pub fn resize_agent_session(app: AppHandle, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let procs = app.state::<AgentProcesses>();
    let map = procs.0.lock().map_err(|e| e.to_string())?;
    match map.get(&id) {
        Some(AgentProc::Pty(h)) => h
            .master
            .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string()),
        _ => Ok(()), // SDK has no terminal; finished session is a no-op
    }
}

#[tauri::command]
pub fn stop_agent_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<(), String> {
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let _ = agent_session::set_status(&conn, &id, "stopped");
    }
    let procs = app.state::<AgentProcesses>();
    let mut map = procs.0.lock().map_err(|e| e.to_string())?;
    match map.get_mut(&id) {
        Some(AgentProc::Pty(h)) => {
            let _ = h.killer.kill();
        }
        Some(AgentProc::Sdk(h)) => h.kill(),
        None => {}
    }
    Ok(())
}
```
Also update the reader thread's registry removal (it uses `map.remove(&thread_id)` — unchanged, works for the enum).

- [ ] **Step 4: Run** `cargo test --manifest-path src-tauri/Cargo.toml` (build check — start_sdk wiring is Task 6; if `start_sdk_session` isn't referenced yet that's fine). Fix compile errors from the enum change.

- [ ] **Step 5: Commit**
```bash
git add src-tauri/src/commands/agent_sessions.rs
git commit -m "feat(m10b-2b): AgentProc enum registry + write/resize/stop dispatch"
```

---

## Task 6: Wire `start_agent_session` (kind branch + `start_sdk_session` + transcript command)

**Files:** Modify `src-tauri/src/commands/agent_sessions.rs`, `src-tauri/src/lib.rs`.

- [ ] **Step 1: Add the require-account validator + an `AgentSdkEvent` payload type** near the top of `agent_sessions.rs`:
```rust
#[derive(Clone, Serialize)]
struct AgentSdkEvent {
    session_id: String,
    line: String, // one redacted NDJSON object
}

/// SDK adapters must have a bound account (no silent ambient identity). Fixed,
/// secret-free error.
pub fn validate_account_required(
    adapter: &AgentAdapter,
    account: Option<&ProviderAccount>,
) -> Result<(), String> {
    if adapter.requires_account && account.is_none() {
        return Err("This agent requires a provider account".into());
    }
    Ok(())
}
```

- [ ] **Step 2: Add the `prompt` param + branch.** Change the `start_agent_session` signature to add `prompt: Option<String>,` (after `account_id`). The block that loads `account`, then `let env = resolve_session_env(...)?` and `let account_row_id = ...` stay shared. Replace the block from `let program = agent::resolve_program(&adapter);` (line ~124) onward with — validate, compute id/transcript up front, branch to the SDK path, else the unchanged PTY path:
```rust
    validate_account_required(&adapter, account.as_ref())?;

    let id = new_id();
    let base = transcripts_base(&app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("failed to create transcripts dir: {e}"))?;
    let transcript_path = base.join(format!("{id}.log"));
    let transcript_str = transcript_path.to_string_lossy().to_string();

    if adapter.kind == "sdk" {
        return start_sdk_session(
            app, state, adapter, env, account_row_id.map(|s| s.to_string()),
            workspace_id, worktree_path, coding_workspace_id, prompt.unwrap_or_default(),
            id, transcript_path, transcript_str,
        );
    }

    // ---- PTY path (unchanged below) ----
    let program = agent::resolve_program(&adapter);
    let args: Vec<&str> = adapter.args.clone();
    let spawned = pty::spawn(&program, &args, Path::new(&worktree_path), &env, cols, rows)?;
    // ...existing PTY spawn/insert/thread... but change agent_session::create to pass "pty" as the final kind arg.
```
In the existing PTY `agent_session::create(...)` call, add `"pty"` as the final argument.

- [ ] **Step 3: Implement `start_sdk_session`** (new fn in `agent_sessions.rs`). It mirrors the PTY tail but spawns the sidecar, redacts, parses, and derives status:
```rust
#[allow(clippy::too_many_arguments)]
fn start_sdk_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    adapter: AgentAdapter,
    env: Vec<(String, String)>,
    account_row_id: Option<String>,
    workspace_id: String,
    worktree_path: String,
    coding_workspace_id: String,
    goal: String,
    id: String,
    transcript_path: PathBuf,
    transcript_str: String,
) -> Result<AgentSession, String> {
    let sidecar = agent::resolve_sdk_sidecar();
    // The injected key value — for masking it out of everything we persist/emit.
    let injected_key = adapter
        .api_key_env
        .and_then(|name| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
        .unwrap_or_default();
    // Isolate the SDK's own on-disk config/session files away from ~/.claude.
    let mut sdk_env = env.clone();
    sdk_env.push((
        "CLAUDE_CONFIG_DIR".to_string(),
        transcript_path.with_extension("cfg").to_string_lossy().to_string(),
    ));

    let sdk::SdkSpawned { stdout, mut child, handle } =
        sdk::spawn(&sidecar, &goal, Path::new(&worktree_path), &sdk_env)?;

    let session = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        agent_session::create(
            &conn, &id, &workspace_id, &coding_workspace_id, adapter.id,
            &sidecar, &transcript_str, account_row_id.as_deref(), None, "sdk",
        )
        .map_err(|e| e.to_string())?
    };

    {
        let procs = app.state::<AgentProcesses>();
        procs.0.lock().map_err(|e| e.to_string())?.insert(id.clone(), AgentProc::Sdk(handle));
    }
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let payload = serde_json::json!({
            "agent_session_id": id, "adapter_id": adapter.id, "account_id": account_row_id,
        })
        .to_string();
        let _ = event::create(&conn, &new_id(), &workspace_id, "session.started", &payload);
    }

    let thread_app = app.clone();
    let thread_id = id.clone();
    let thread_ws = workspace_id.clone();
    std::thread::spawn(move || {
        let mut transcript = std::fs::OpenOptions::new()
            .create(true).append(true).open(&transcript_path).ok();
        let reader = std::io::BufReader::new(stdout);
        let outcome = sdk::pump_ndjson(reader, |parsed| {
            // The raw line for persistence is reconstructed by re-emitting; but pump
            // gives us the parsed SdkLine. Persist + emit only relayable lines.
            let line = match parsed {
                sdk::SdkLine::Event { raw, .. } => sdk::redact(raw, &injected_key),
                sdk::SdkLine::Error(msg) => serde_json::json!({"type":"error","message": sdk::redact(msg, &injected_key)}).to_string(),
                sdk::SdkLine::Skip => return,
            };
            if let Some(f) = transcript.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.write_all(b"\n");
            }
            let _ = thread_app.emit("agent-sdk-event", AgentSdkEvent {
                session_id: thread_id.clone(),
                line,
            });
        });
        let exit = child.wait().ok().and_then(|s| s.code()).map(|c| c as i64);
        let wait_status = sdk::sdk_status(outcome.saw_result, outcome.saw_error, exit).to_string();

        let (status, code) = if let Some(conn) = thread_app.try_state::<Mutex<Connection>>() {
            if let Ok(conn) = conn.lock() {
                let _ = agent_session::mark_exited(&conn, &thread_id, &wait_status, exit);
                let row = agent_session::get(&conn, &thread_id).ok().flatten();
                let status = row.as_ref().map(|s| s.status.clone()).unwrap_or(wait_status);
                let code = row.as_ref().and_then(|s| s.exit_code);
                let payload = serde_json::json!({ "agent_session_id": thread_id, "status": status }).to_string();
                let _ = event::create(&conn, &new_id(), &thread_ws, "agent.exited", &payload);
                (status, code)
            } else { (wait_status, exit) }
        } else { (wait_status, exit) };
        let _ = thread_app.emit("agent-exit", AgentExit { session_id: thread_id.clone(), status, exit_code: code });
        if let Some(procs) = thread_app.try_state::<AgentProcesses>() {
            if let Ok(mut map) = procs.0.lock() { map.remove(&thread_id); }
        }
    });

    Ok(session)
}
```

- [ ] **Step 4: Add the `get_agent_sdk_transcript` command** (parsed, redaction already on disk, skip-bad-line). After `get_agent_session_transcript`:
```rust
#[tauri::command]
pub fn get_agent_sdk_transcript(
    state: State<'_, Mutex<Connection>>,
    id: String,
) -> Result<Vec<String>, String> {
    let path = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(s) = agent_session::get(&conn, &id).map_err(|e| e.to_string())? else {
            return Err(format!("Agent session '{id}' does not exist"));
        };
        s.transcript_path
    };
    // The transcript is already-redacted NDJSON. Return only well-formed relayable
    // lines (the frontend renders them); drop blanks/garbage.
    let bytes = std::fs::read(&path).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    Ok(text
        .lines()
        .filter(|l| !matches!(crate::services::agent::sdk::parse_sdk_line(l), crate::services::agent::sdk::SdkLine::Skip))
        .map(|l| l.to_string())
        .collect())
}
```

- [ ] **Step 5: Register the command** in `lib.rs` `generate_handler!` (after `get_agent_session_transcript`): `commands::agent_sessions::get_agent_sdk_transcript,`.

- [ ] **Step 6: Run the full backend suite + clippy**
```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```
Expected: PASS / clean. Fix the `start_agent_session` call-site signature (the frontend api passes `prompt` in Task 8; the Rust signature now has it).

- [ ] **Step 7: Commit**
```bash
git add src-tauri/src/commands/agent_sessions.rs src-tauri/src/lib.rs
git commit -m "feat(m10b-2b): start_sdk_session + redacted NDJSON stream + transcript command"
```

---

## Task 7: The Node sidecar + packaging

**Files:** Create `sidecar/claude-agent-sdk/package.json`, `index.mjs`, `.gitignore`.

- [ ] **Step 1: `sidecar/claude-agent-sdk/package.json`** (pinned; NOT a workspace member):
```json
{
  "name": "uaw-agent-sdk-sidecar",
  "private": true,
  "type": "module",
  "dependencies": {
    "@anthropic-ai/claude-agent-sdk": "0.1.0"
  }
}
```
(Pin to the exact current version at implementation time; the implementer runs `npm install` in this dir — NOT `pnpm install` at the repo root — so it never enters the root lockfile/workspace.)

- [ ] **Step 2: `sidecar/claude-agent-sdk/.gitignore`**:
```
node_modules/
package-lock.json
```

- [ ] **Step 3: `sidecar/claude-agent-sdk/index.mjs`**:
```js
#!/usr/bin/env node
// Headless Claude Agent SDK runner. Goal via argv[2]; key via env (injected by the
// backend). Emits one compact NDJSON object per message on stdout. Plan-only.
import { query } from "@anthropic-ai/claude-agent-sdk";

const goal = process.argv[2] ?? "";
const emit = (o) => process.stdout.write(JSON.stringify(o) + "\n");

try {
  for await (const m of query({
    prompt: goal,
    options: {
      cwd: process.cwd(),
      permissionMode: "plan",
      settingSources: [],
      maxTurns: 30,
      // Spread our env so the grandchild CLI inherits the injected key, and blank
      // ambient tokens that would otherwise outrank it.
      env: { ...process.env, ANTHROPIC_AUTH_TOKEN: "", CLAUDE_CODE_OAUTH_TOKEN: "" },
    },
  })) {
    if (m.type === "assistant") {
      for (const block of m.message?.content ?? []) {
        if (block.type === "text" && block.text) emit({ type: "assistant", text: block.text });
        else if (block.type === "tool_use") emit({ type: "tool", name: block.name, summary: JSON.stringify(block.input ?? {}).slice(0, 200) });
      }
    } else if (m.type === "result") {
      emit({ type: "result", status: m.subtype === "success" && !m.is_error ? "success" : "error", summary: typeof m.result === "string" ? m.result : "" });
    }
  }
} catch {
  emit({ type: "error", message: "Agent run failed" });
  process.exit(1);
}
```

- [ ] **Step 4: Commit** (script only; node_modules gitignored)
```bash
git add sidecar/claude-agent-sdk/package.json sidecar/claude-agent-sdk/index.mjs sidecar/claude-agent-sdk/.gitignore
git commit -m "feat(m10b-2b): Node Claude Agent SDK sidecar (plan-only, NDJSON)"
```

---

## Task 8: Frontend types / api / store

**Files:** Modify `src/types/agentSession.ts`, `src/api/agentSessions.ts`, `src/stores/agentSessions.ts`.

- [ ] **Step 1: Types.** In `types/agentSession.ts`: `AgentAdapter` += `kind: string;` `requires_account: boolean;`. `AgentSession` += `kind: string;` (after `model_id`). Add:
```ts
export interface SdkEvent {
  type: "assistant" | "tool" | "result" | "error";
  text?: string;
  name?: string;
  summary?: string;
  message?: string;
  status?: string;
}
```

- [ ] **Step 2: API.** In `api/agentSessions.ts`, `startAgentSession` gains `prompt: string | null` (after `accountId`):
```ts
export function startAgentSession(
  codingWorkspaceId: string,
  adapterId: string,
  accountId: string | null,
  prompt: string | null,
  cols: number,
  rows: number,
): Promise<AgentSession> {
  return invoke<AgentSession>("start_agent_session", {
    codingWorkspaceId, adapterId, accountId, prompt, cols, rows,
  });
}

export function getAgentSdkTranscript(id: string): Promise<string[]> {
  return invoke<string[]>("get_agent_sdk_transcript", { id });
}
```

- [ ] **Step 3: Store.** In `stores/agentSessions.ts`: add a store-owned accumulator + listener + replay; thread `prompt` through `start`.
```ts
import { ref } from "vue";
import { defineStore } from "pinia";
import { listen } from "@tauri-apps/api/event";
import type { AgentAdapter, AgentSession, SdkEvent } from "../types/agentSession";
import * as api from "../api/agentSessions";
// ...existing...
  const sdkEvents = ref<Record<string, SdkEvent[]>>({});
  let sdkListenerStarted = false;

  function parseSdkLine(line: string): SdkEvent | null {
    try { return JSON.parse(line) as SdkEvent; } catch { return null; }
  }

  /** Start the global agent-sdk-event listener once (store-owned, so events that
   *  arrive before a view mounts are never lost). */
  async function ensureSdkListener() {
    if (sdkListenerStarted) return;
    sdkListenerStarted = true;
    await listen<{ session_id: string; line: string }>("agent-sdk-event", (e) => {
      const ev = parseSdkLine(e.payload.line);
      if (!ev) return;
      const id = e.payload.session_id;
      sdkEvents.value = { ...sdkEvents.value, [id]: [...(sdkEvents.value[id] ?? []), ev] };
    });
  }

  /** Replay a finished/reopened SDK session's transcript once into the accumulator. */
  async function loadSdkTranscript(id: string) {
    if (sdkEvents.value[id]) return; // already have live state
    const lines = await api.getAgentSdkTranscript(id);
    const evs = lines.map(parseSdkLine).filter((x): x is SdkEvent => x !== null);
    sdkEvents.value = { ...sdkEvents.value, [id]: evs };
  }

  async function start(
    codingWorkspaceId: string,
    adapterId: string,
    accountId: string | null,
    prompt: string | null,
    cols: number,
    rows: number,
  ) {
    await ensureSdkListener();
    const session = await api.startAgentSession(codingWorkspaceId, adapterId, accountId, prompt, cols, rows);
    tabs.value.push({ session });
    activeId.value = session.id;
    return session;
  }
```
Call `ensureSdkListener()` in `loadAdapters` too. Export `sdkEvents`, `loadSdkTranscript`.

- [ ] **Step 4: Typecheck + commit**
```bash
pnpm e2e:typecheck && pnpm build
git add src/types/agentSession.ts src/api/agentSessions.ts src/stores/agentSessions.ts
git commit -m "feat(m10b-2b): frontend SDK event accumulator + types/api"
```

---

## Task 9: Goal box + `SdkRunView` + dispatch

**Files:** Modify `src/components/AgentsView.vue`; Create `src/components/SdkRunView.vue`.

- [ ] **Step 1: `SdkRunView.vue`** (read-only feed):
```vue
<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useAgentSessionsStore } from "../stores/agentSessions";

const props = defineProps<{ sessionId: string }>();
const store = useAgentSessionsStore();
const events = computed(() => store.sdkEvents[props.sessionId] ?? []);
onMounted(() => store.loadSdkTranscript(props.sessionId));

const label = (e: { type: string; name?: string }) =>
  e.type === "tool" ? `🔧 ${e.name ?? "tool"}` : e.type === "result" ? "✓" : e.type === "error" ? "✗" : "";
</script>

<template>
  <div class="sdk-feed" data-testid="agent-sdk-feed">
    <div
      v-for="(e, i) in events"
      :key="i"
      class="sdk-row"
      data-testid="sdk-event"
      :data-kind="e.type"
    >
      <span class="sdk-row__tag">{{ label(e) }}</span>
      <span class="sdk-row__text">{{ e.text ?? e.summary ?? e.message ?? "" }}</span>
    </div>
    <p v-if="events.length === 0" class="muted">Waiting for the agent…</p>
  </div>
</template>

<style scoped>
.sdk-feed { flex: 1; min-height: 0; overflow-y: auto; padding: 0.5rem; display: flex; flex-direction: column; gap: 0.35rem; }
.sdk-row { display: flex; gap: 0.5rem; font-size: 0.85rem; }
.sdk-row[data-kind="error"] { color: var(--re-color-danger-text); }
.sdk-row__tag { flex-shrink: 0; }
.sdk-row__text { white-space: pre-wrap; word-break: break-word; }
.muted { color: var(--re-color-text-muted); }
</style>
```

- [ ] **Step 2: AgentsView script — goal ref, computeds, reset, openTerminal.** Add the import + `SdkRunView`, the goal ref, an `adapterKind` helper, `canStart` gating, resets, and pass `prompt`:
```ts
import SdkRunView from "./SdkRunView.vue";
// ...
const newGoal = ref("");
const adapterKind = (id: string) => store.adapters.find((a) => a.id === id)?.kind ?? "pty";
const selectedIsSdk = computed(() => adapterKind(newAdapterId.value) === "sdk");
```
Change `canStart`:
```ts
const canStart = computed(
  () =>
    newWorktreeId.value !== "" &&
    newAdapterId.value !== "" &&
    !starting.value &&
    (!selectedIsSdk.value || (newGoal.value.trim() !== "" && newAccountId.value !== "")),
);
```
In the `watch(newAdapterId, ...)` reset block AND the `workspaces.currentId` watch, add `newGoal.value = "";`. Change `openTerminal`'s start call:
```ts
    await store.start(
      newWorktreeId.value,
      newAdapterId.value,
      newAccountId.value || null,
      selectedIsSdk.value ? newGoal.value.trim() || null : null,
      80,
      24,
    );
```

- [ ] **Step 3: AgentsView template — goal textarea + render dispatch.** After the account `<select>` block, add (sdk-only):
```html
        <textarea
          v-if="selectedIsSdk"
          v-model="newGoal"
          class="re-input"
          rows="2"
          placeholder="What should the agent plan?"
          aria-label="Agent goal"
        ></textarea>
```
Replace `<TerminalTab :session-id="t.session.id" />` (line ~192) with:
```html
        <SdkRunView v-if="t.session.kind === 'sdk'" :session-id="t.session.id" />
        <TerminalTab v-else-if="t.session.kind === 'pty'" :session-id="t.session.id" />
```

- [ ] **Step 4: Typecheck + build + commit**
```bash
pnpm e2e:typecheck && pnpm build
git add src/components/AgentsView.vue src/components/SdkRunView.vue
git commit -m "feat(m10b-2b): goal box + SdkRunView feed + kind dispatch"
```

---

## Task 10: Fake sidecar + e2e

**Files:** Modify `scripts/run-e2e.sh`, `wdio.conf.ts`; Create `e2e/specs/agent-sdk.e2e.ts`.

- [ ] **Step 1: Fake sidecar** — in `scripts/run-e2e.sh`, after the existing fake agent, add:
```bash
# Fake Claude Agent SDK sidecar: goal via argv ($1), emits canned NDJSON incl. a
# deliberate $ANTHROPIC_API_KEY echo (to prove redaction), a garbage line (to prove
# no-crash), a KEY:set/unset presence marker, then a result. Exits 0 (NOT exec cat).
cat >/tmp/uaw-fake-sdk <<'SDK'
#!/usr/bin/env bash
goal="$1"
km=KEY:unset; [ -n "${ANTHROPIC_API_KEY:-}" ] && km=KEY:set
printf '{"type":"assistant","text":"Planning: %s"}\n' "${goal//\"/}"
printf '{"type":"tool","name":"Read","summary":"README.md"}\n'
printf '{"type":"tool","name":"echo","summary":"%s"}\n' "${ANTHROPIC_API_KEY:-none}"
printf 'this line is not json\n'
printf '{"type":"tool","name":"probe","summary":"%s"}\n' "$km"
printf '{"type":"result","status":"success","summary":"Done"}\n'
SDK
chmod +x /tmp/uaw-fake-sdk
```

- [ ] **Step 2: wdio env** — in `wdio.conf.ts` `beforeSession`, after `UAW_AGENT_BIN`:
```ts
    process.env.UAW_AGENT_SDK_SIDECAR = "/tmp/uaw-fake-sdk";
```

- [ ] **Step 3: e2e spec** — create `e2e/specs/agent-sdk.e2e.ts`:
```ts
import { browser, $, $$, expect } from "@wdio/globals";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const KEY_VALUE = "sk-ant-e2e-SDK-SECRET";
const REPO = "/tmp/fixture-repo-sdk";

const feedText = () =>
  browser.execute(() => document.querySelector('[data-testid="agent-sdk-feed"]')?.textContent ?? "");

describe("claude agent sdk (plan-only)", () => {
  before(async () => {
    fs.rmSync(REPO, { recursive: true, force: true });
    fs.mkdirSync(REPO, { recursive: true });
    const git = (a: string[]) => execFileSync("git", ["-C", REPO, ...a], { stdio: "ignore" });
    execFileSync("git", ["init", "-b", "main", REPO], { stdio: "ignore" });
    git(["config", "user.email", "a@uaw.local"]); git(["config", "user.name", "UAW"]);
    fs.writeFileSync(path.join(REPO, "README.md"), "# sdk fixture\n");
    git(["add", "."]); git(["commit", "-m", "init"]);
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("sets up a project, repo, worktree, account", async () => {
    await (await $("button*=Projects")).click();
    await (await $('[aria-label="New project name"]')).setValue("SdkProj");
    await (await $('[aria-label="Project mode"]')).selectByAttribute("value", "code");
    await (await $("button*=Create")).click();
    await (await $('[data-testid="project-row"]')).waitForExist({ timeout: 10_000 });
    await (await $("button*=Sources")).click();
    await (await $('[aria-label="Repository name"]')).setValue("SdkFixture");
    await (await $('[aria-label="Repository path"]')).setValue(REPO);
    await (await $("button*=Attach")).click();
    await (await $('[data-testid="repository-row"]')).waitForExist({ timeout: 10_000 });
    await (await $("button*=Coding")).click();
    await (await $('[aria-label="Coding project"]')).selectByVisibleText("SdkProj");
    await (await $('[aria-label="Coding repository"]')).selectByVisibleText("SdkFixture");
    const base = await $('[aria-label="Base branch"]');
    await browser.waitUntil(async () => base.isEnabled(), { timeout: 10_000 });
    await base.selectByVisibleText("main");
    await (await $('[aria-label="New branch name"]')).setValue("feat/sdk");
    await (await $("button*=Create worktree")).click();
    await (await $('[data-testid="coding-row"]')).waitForExist({ timeout: 15_000 });
    await (await $("button*=Providers")).click();
    await (await $('[aria-label="Provider"]')).selectByAttribute("value", "anthropic");
    await (await $('[aria-label="Account display name"]')).setValue("SDK Acct");
    await (await $('[aria-label="API key"]')).setValue(KEY_VALUE);
    await (await $("button*=Add account")).click();
    await (await $('[data-testid="provider-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("runs a plan-only SDK session, streams the feed, never exposes the key", async () => {
    await (await $("button*=Agents")).click();
    await (await $('[aria-label="Agent worktree"]')).selectByVisibleText("feat/sdk");
    await (await $('[aria-label="Agent CLI"]')).selectByVisibleText("Claude Agent SDK");
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("SDK Acct");
    await (await $('[aria-label="Agent goal"]')).setValue("summarize the readme");
    await (await $("button*=New terminal")).click();

    await (await $('[data-testid="agent-sdk-feed"]')).waitForExist({ timeout: 10_000 });
    // assistant + result rows render
    await browser.waitUntil(async () => (await feedText()).includes("Planning:"), { timeout: 15_000, timeoutMsg: "assistant row" });
    await browser.waitUntil(async () => (await feedText()).includes("Done"), { timeout: 15_000, timeoutMsg: "result row" });
    // a tool row exists
    expect((await $$('[data-testid="sdk-event"][data-kind="tool"]')).length).toBeGreaterThan(0);
    // injection proven, value redacted
    const text = await feedText();
    expect(text).toContain("KEY:set");
    expect(text).not.toContain(KEY_VALUE);
  });

  it("requires an account for the SDK adapter", async () => {
    await (await $('[aria-label="Provider account"]')).selectByVisibleText("Default (no key)");
    await (await $('[aria-label="Agent goal"]')).setValue("x");
    // canStart is false without an account → the button is disabled.
    expect(await (await $("button*=New terminal")).isEnabled()).toBe(false);
  });
});
```

- [ ] **Step 4: Typecheck + commit**
```bash
pnpm e2e:typecheck
git add scripts/run-e2e.sh wdio.conf.ts e2e/specs/agent-sdk.e2e.ts
git commit -m "test(m10b-2b): fake SDK sidecar + plan-only e2e (redaction + require-account)"
```

---

## Final verification
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` — all green (redact/parse/pump/status, env-override, resolver, idempotency→12).
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — clean (watch `#[cfg(unix)]`-gated `libc` use; `AgentProc::Sdk` consumed).
- [ ] `pnpm build` + `pnpm e2e:typecheck` — clean.
- [ ] `git diff --stat origin/main -- Dockerfile.e2e pnpm-lock.yaml package.json pnpm-workspace.yaml` — **empty** (the SDK dep never entered the root install).
- [ ] `pnpm e2e:docker` — all specs pass incl. `agent-sdk.e2e.ts` (fake sidecar; no Node SDK / API key needed).
- [ ] Manual (macOS): `cd sidecar/claude-agent-sdk && npm install`; run a real plan-only session against a worktree with a bound Anthropic account; confirm the feed streams the agent's plan and the key never appears.

## Review findings folded in (traceability)
Goal via argv (sdk.rs spawn + sidecar argv[2] + fake `$1`); plan-only + require-account + redact-at-sink + clear ambient tokens + isolated CLAUDE_CONFIG_DIR + maxTurns; status from result (`sdk_status`); byte-oriented `pump_ndjson`; enum `AgentProc` + dispatch; process-group kill (`libc`); program-only `command`; persist `kind`; store-owned accumulator + replay; goal reset + sdk-only `canStart`; plain-text feed; SDK dep out of the pnpm workspace (Docker has Node 22 + frozen-lockfile) — `Dockerfile.e2e` unchanged.
