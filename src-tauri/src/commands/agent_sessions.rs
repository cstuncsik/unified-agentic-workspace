use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::models::agent_session::{self, AgentSession};
use crate::models::provider_account::{self, ProviderAccount};
use crate::models::{coding_workspace, event};
use crate::services::agent::{self, pty, sdk, AgentAdapter};
use crate::services::keystore::{self, KeyStore, KeyStoreError};
use crate::util::new_id;

/// A live agent process: an interactive PTY or a headless SDK sidecar.
pub enum AgentProc {
    Pty(pty::PtyHandle),
    Sdk(sdk::SdkHandle),
}

/// Registry of live sessions, keyed by agent-session id.
#[derive(Default)]
pub struct AgentProcesses(pub Mutex<HashMap<String, AgentProc>>);

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

/// Max wall-clock time the model-list helper may run before it is killed.
const MODELS_TIMEOUT: Duration = Duration::from_secs(10);

/// List the models the given account can use, by running the dependency-free Node
/// helper with the account's key injected (key never returns to the frontend).
/// Anthropic-only; every failure is a fixed opaque error.
#[tauri::command]
pub fn list_account_models(
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    account_id: String,
) -> Result<Vec<sdk::ModelInfo>, String> {
    let account = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) =
            coding_workspace::get(&conn, &coding_workspace_id).map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{coding_workspace_id}' does not exist"
            ));
        };
        load_session_account(&conn, Some(&account_id), &cw.workspace_id)?
            .ok_or_else(|| "Selected account is not available in this workspace".to_string())?
    };
    if account.provider != "anthropic" {
        return Err("Model listing is only supported for Anthropic accounts".into());
    }
    let key = match keystore::resolve().get(&account.keychain_ref) {
        Ok(Some(k)) => k,
        Ok(None) => return Err("Stored key for this account is missing".into()),
        Err(KeyStoreError::NoBackend) => {
            return Err("No OS keychain is available on this system.".into())
        }
        Err(KeyStoreError::Failure) => return Err("Failed to load the account key".into()),
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"));
    let stdout = sdk::spawn_oneshot(
        &agent::resolve_sdk_models_sidecar(),
        &[],
        &cwd,
        &[
            ("ANTHROPIC_API_KEY".to_string(), key),
            ("ANTHROPIC_AUTH_TOKEN".to_string(), String::new()),
            ("CLAUDE_CODE_OAUTH_TOKEN".to_string(), String::new()),
        ],
        MODELS_TIMEOUT,
    )?;
    sdk::parse_models(&stdout).map_err(|_| "Failed to list models".to_string())
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
    // Raw PTY bytes are appended verbatim and can contain non-UTF-8 (a multibyte
    // codepoint split across a read boundary, or control bytes), so read as bytes
    // and lossily decode — read_to_string would error and drop the whole file.
    Ok(std::fs::read(&path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default())
}

/// The redacted NDJSON lines of an SDK session's transcript (relayable lines only)
/// for replay when a view (re)opens. The on-disk transcript is already masked.
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
    let bytes = std::fs::read(&path).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    Ok(text
        .lines()
        .filter(|l| !matches!(sdk::parse_sdk_line(l), sdk::SdkLine::Skip))
        .map(|l| l.to_string())
        .collect())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn start_agent_session(
    app: AppHandle,
    state: State<'_, Mutex<Connection>>,
    coding_workspace_id: String,
    adapter_id: String,
    account_id: Option<String>,
    prompt: Option<String>,
    mode: Option<String>,
    model: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<AgentSession, String> {
    let Some(adapter) = agent::find_adapter(&adapter_id) else {
        return Err(format!("Unknown agent adapter '{adapter_id}'"));
    };

    let store = keystore::resolve();

    // Resolve the worktree + workspace AND load/validate the chosen account under
    // one lock, then release before any keychain IO or spawn.
    let (workspace_id, worktree_path, account) = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let Some(cw) =
            coding_workspace::get(&conn, &coding_workspace_id).map_err(|e| e.to_string())?
        else {
            return Err(format!(
                "Coding workspace '{coding_workspace_id}' does not exist"
            ));
        };
        let account = load_session_account(&conn, account_id.as_deref(), &cw.workspace_id)?;
        (cw.workspace_id, cw.worktree_path, account)
    };

    // Resolve the account's key from the keychain (no lock held) and build the env.
    let env = resolve_session_env(&adapter, account.as_ref(), store.as_ref())?;
    let account_row_id = account.as_ref().map(|a| a.id.as_str());
    validate_account_required(&adapter, account.as_ref())?;

    let id = new_id();
    // Prepare the transcript file (shared by both runtimes).
    let base = transcripts_base(&app)?;
    std::fs::create_dir_all(&base).map_err(|e| format!("failed to create transcripts dir: {e}"))?;
    let transcript_path = base.join(format!("{id}.log"));
    let transcript_str = transcript_path.to_string_lossy().to_string();

    // Headless SDK adapters take a different runtime (piped child + NDJSON).
    if adapter.kind == "sdk" {
        return start_sdk_session(
            app,
            state,
            adapter,
            env,
            account_row_id.map(|s| s.to_string()),
            workspace_id,
            worktree_path,
            coding_workspace_id,
            prompt.unwrap_or_default(),
            sdk::normalize_sdk_mode(mode.as_deref()),
            model.as_deref(),
            id,
            transcript_path,
            transcript_str,
        );
    }

    // ---- PTY path ----
    let program = agent::resolve_program(&adapter);

    // Spawn the PTY.
    let args: Vec<&str> = adapter.args.clone();
    let spawned = pty::spawn(&program, &args, Path::new(&worktree_path), &env, cols, rows)?;
    let pty::Spawned {
        handle,
        reader,
        mut child,
    } = spawned;

    // Insert the session row.
    let session = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        agent_session::create(
            &conn,
            &id,
            &workspace_id,
            &coding_workspace_id,
            adapter.id,
            &program,
            &transcript_str,
            account_row_id,
            None,
            "pty",
            None,
        )
        .map_err(|e| e.to_string())?
    };

    // Register the handle for input/resize/stop.
    {
        let procs = app.state::<AgentProcesses>();
        procs
            .0
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id.clone(), AgentProc::Pty(handle));
    }

    // Record session.started.
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let payload = serde_json::json!({
            "agent_session_id": id,
            "adapter_id": adapter.id,
            "account_id": account_row_id,
        })
        .to_string();
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
                AgentOutput {
                    session_id: thread_id.clone(),
                    bytes: chunk.to_vec(),
                },
            );
        });

        let (wait_status, wait_code) = match child.wait() {
            Ok(s) if s.success() => ("exited".to_string(), Some(s.exit_code() as i64)),
            Ok(s) => ("failed".to_string(), Some(s.exit_code() as i64)),
            Err(_) => ("failed".to_string(), None),
        };

        // Emit the EFFECTIVE persisted status, not the raw wait result. A user
        // stop forces 'stopped' before killing, and kill makes wait() report
        // success()==false → "failed". mark_exited only moves a still-running
        // session, so re-read the row and surface that to the event log + UI —
        // otherwise a deliberate stop would be reported as a failure.
        let (status, code) = if let Some(conn) = thread_app.try_state::<Mutex<Connection>>() {
            if let Ok(conn) = conn.lock() {
                let _ = agent_session::mark_exited(&conn, &thread_id, &wait_status, wait_code);
                let row = agent_session::get(&conn, &thread_id).ok().flatten();
                let status = row.as_ref().map(|s| s.status.clone()).unwrap_or(wait_status);
                let code = row.as_ref().and_then(|s| s.exit_code);
                let payload =
                    serde_json::json!({ "agent_session_id": thread_id, "status": status })
                        .to_string();
                let _ = event::create(&conn, &new_id(), &thread_ws, "agent.exited", &payload);
                (status, code)
            } else {
                (wait_status, wait_code)
            }
        } else {
            (wait_status, wait_code)
        };
        let _ = thread_app.emit(
            "agent-exit",
            AgentExit {
                session_id: thread_id.clone(),
                status,
                exit_code: code,
            },
        );
        if let Some(procs) = thread_app.try_state::<AgentProcesses>() {
            if let Ok(mut map) = procs.0.lock() {
                map.remove(&thread_id);
            }
        }
    });

    Ok(session)
}

/// Create the per-session isolated HOME, fail-closed: error if the path already exists or
/// is a symlink — a stale or planted credentials.json in a reused dir could re-leak the
/// ambient login. The session uuid makes the path unguessable; 0700 keeps another local
/// user from reading the CLI-written credential. Errors map to the existing opaque string.
fn create_isolated_home(path: &Path) -> Result<(), String> {
    // create_dir (NOT create_dir_all) fails if the path exists or is a symlink.
    std::fs::create_dir(path).map_err(|_| "Failed to start the agent sidecar".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Failed to start the agent sidecar".to_string())?;
    }
    Ok(())
}

/// Route the agent's credential-home env at an app-private dir so the grandchild Claude
/// Code CLI cannot reach the user's ambient login (macOS Keychain at $HOME/Library/
/// Keychains, ~/.claude/.credentials.json, %APPDATA% on Windows). CLAUDE_CONFIG_DIR alone
/// does NOT sever the Keychain — HOME does. Each OS reads only the vars it uses; the rest
/// are inert. Appended last so they override any inherited value at spawn time.
fn with_isolated_home(mut env: Vec<(String, String)>, home_dir: &str) -> Vec<(String, String)> {
    for k in ["HOME", "USERPROFILE", "CLAUDE_CONFIG_DIR", "APPDATA", "LOCALAPPDATA"] {
        env.push((k.to_string(), home_dir.to_string()));
    }
    env
}

/// Headless Claude Agent SDK run: spawn the Node sidecar as a piped child, stream
/// its redacted NDJSON to the transcript + `agent-sdk-event`, derive status from
/// the terminal `result` event.
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
    mode: &str,
    model: Option<&str>,
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
    // Isolate the SDK agent's credential resolution: a fresh, app-private HOME so the
    // grandchild Claude Code CLI cannot fall back to the user's ambient `claude login`
    // (macOS Keychain / ~/.claude) when the injected key is bad. CLAUDE_CONFIG_DIR alone
    // does NOT sever the Keychain; HOME does. The sidecar's `...process.env` spread carries
    // these to the grandchild; `settingSources: []` already blocks the settings paths.
    let isolated_home = transcript_path.with_extension("home");
    create_isolated_home(&isolated_home)?;
    let sdk_env = with_isolated_home(env.clone(), &isolated_home.to_string_lossy());

    let sdk::SdkSpawned {
        stdout,
        mut child,
        handle,
    } = sdk::spawn(&sidecar, &goal, mode, model.unwrap_or(""), Path::new(&worktree_path), &sdk_env)?;

    let session = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        agent_session::create(
            &conn,
            &id,
            &workspace_id,
            &coding_workspace_id,
            adapter.id,
            &sidecar,
            &transcript_str,
            account_row_id.as_deref(),
            model,
            "sdk",
            Some(mode),
        )
        .map_err(|e| e.to_string())?
    };

    {
        let procs = app.state::<AgentProcesses>();
        procs
            .0
            .lock()
            .map_err(|e| e.to_string())?
            .insert(id.clone(), AgentProc::Sdk(handle));
    }
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let payload = serde_json::json!({
            "agent_session_id": id,
            "adapter_id": adapter.id,
            "account_id": account_row_id,
        })
        .to_string();
        let _ = event::create(&conn, &new_id(), &workspace_id, "session.started", &payload);
    }

    let thread_app = app.clone();
    let thread_id = id.clone();
    let thread_ws = workspace_id.clone();
    std::thread::spawn(move || {
        let mut transcript = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)
            .ok();
        let reader = std::io::BufReader::new(stdout);
        let outcome = sdk::pump_ndjson(reader, |parsed| {
            // Persist + emit only relayable lines, with the key masked out.
            let line = match parsed {
                sdk::SdkLine::Event { raw, .. } => sdk::redact(raw, &injected_key),
                sdk::SdkLine::Error(msg) => serde_json::json!({
                    "type": "error",
                    "message": sdk::redact(msg, &injected_key),
                })
                .to_string(),
                sdk::SdkLine::Skip => return,
            };
            if let Some(f) = transcript.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.write_all(b"\n");
            }
            let _ = thread_app.emit(
                "agent-sdk-event",
                AgentSdkEvent {
                    session_id: thread_id.clone(),
                    line,
                },
            );
        });
        let exit = child.wait().ok().and_then(|s| s.code()).map(|c| c as i64);
        let wait_status = sdk::sdk_status(outcome.saw_result, outcome.saw_error, exit).to_string();

        let (status, code) = if let Some(conn) = thread_app.try_state::<Mutex<Connection>>() {
            if let Ok(conn) = conn.lock() {
                let _ = agent_session::mark_exited(&conn, &thread_id, &wait_status, exit);
                let row = agent_session::get(&conn, &thread_id).ok().flatten();
                let status = row.as_ref().map(|s| s.status.clone()).unwrap_or(wait_status);
                let code = row.as_ref().and_then(|s| s.exit_code);
                let payload =
                    serde_json::json!({ "agent_session_id": thread_id, "status": status })
                        .to_string();
                let _ = event::create(&conn, &new_id(), &thread_ws, "agent.exited", &payload);
                (status, code)
            } else {
                (wait_status, exit)
            }
        } else {
            (wait_status, exit)
        };
        let _ = thread_app.emit(
            "agent-exit",
            AgentExit {
                session_id: thread_id.clone(),
                status,
                exit_code: code,
            },
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
    match map.get_mut(&id) {
        Some(AgentProc::Pty(h)) => {
            h.writer
                .write_all(data.as_bytes())
                .map_err(|e| e.to_string())?;
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
            .resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string()),
        _ => Ok(()), // SDK has no terminal; a finished session is a no-op
    }
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
    match map.get_mut(&id) {
        Some(AgentProc::Pty(h)) => {
            let _ = h.killer.kill();
        }
        Some(AgentProc::Sdk(h)) => h.kill(),
        None => {}
    }
    Ok(())
}

/// Load and workspace-scope-validate the chosen account. Connection-only — call
/// UNDER the lock. `None` -> no account; an account in another workspace or a
/// missing id -> a fixed opaque error.
pub fn load_session_account(
    conn: &Connection,
    account_id: Option<&str>,
    workspace_id: &str,
) -> Result<Option<ProviderAccount>, String> {
    let Some(account_id) = account_id else {
        return Ok(None);
    };
    match provider_account::get(conn, account_id) {
        Ok(Some(account)) if account.workspace_id == workspace_id => Ok(Some(account)),
        _ => Err("Selected account is not available in this workspace".into()),
    }
}

/// Build the PTY environment for a session. Reads the keychain — call OUTSIDE the
/// connection lock. Every error is a fixed, secret-free string; the key only ever
/// appears as the VALUE of the adapter's api_key_env.
pub fn resolve_session_env(
    adapter: &AgentAdapter,
    account: Option<&ProviderAccount>,
    store: &dyn KeyStore,
) -> Result<Vec<(String, String)>, String> {
    let Some(account) = account else {
        return Ok(Vec::new()); // no account -> inherit ambient env (legacy behavior)
    };
    let Some(api_key_env) = adapter.api_key_env else {
        return Err("This agent does not support API key accounts".into());
    };
    if adapter.provider != Some(account.provider.as_str()) {
        return Err("Selected account does not match this agent's provider".into());
    }
    let key = match store.get(&account.keychain_ref) {
        Ok(Some(key)) => key,
        Ok(None) => return Err("Stored key for this account is missing".into()),
        Err(KeyStoreError::NoBackend) => {
            return Err("No OS keychain is available on this system.".into())
        }
        Err(KeyStoreError::Failure) => return Err("Failed to load the account key".into()),
    };
    let mut env = vec![(api_key_env.to_string(), key)];
    for clear in &adapter.clear_env {
        env.push((clear.to_string(), String::new()));
    }
    Ok(env)
}

#[cfg(test)]
mod account_env_tests {
    use super::*;
    use crate::models::workspace;
    use crate::services::agent::find_adapter;
    use crate::services::keystore::FileKeyStore;

    const SENTINEL: &str = "SENTINEL_KEY_abc123";

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    fn temp_store() -> FileKeyStore {
        let mut d = std::env::temp_dir();
        d.push(format!("uaw-env-test-{}", new_id()));
        FileKeyStore::new(d)
    }

    fn account(conn: &Connection, ws: &str, provider: &str) -> ProviderAccount {
        let id = new_id();
        provider_account::insert(conn, &id, ws, provider, "api-key", "Acct", &id).unwrap()
    }

    #[test]
    fn no_account_yields_empty_env() {
        let claude = find_adapter("claude-code").unwrap();
        let store = temp_store();
        assert!(resolve_session_env(&claude, None, &store).unwrap().is_empty());
    }

    #[test]
    fn matching_account_injects_key_and_clears_collisions() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic");
        let store = temp_store();
        store.set(&acct.keychain_ref, SENTINEL).unwrap();

        let sdk = find_adapter("claude-agent-sdk").unwrap();
        let env = resolve_session_env(&sdk, Some(&acct), &store).unwrap();

        // Key present EXACTLY once, only as the value of ANTHROPIC_API_KEY — never as a
        // key, never in any other entry (e.g. a clear_env slot).
        assert_eq!(
            env.iter()
                .filter(|(_, v)| v == SENTINEL)
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["ANTHROPIC_API_KEY"],
        );
        assert!(env.iter().all(|(k, _)| k != SENTINEL));
        // The SDK adapter blanks BOTH ambient OAuth tokens.
        assert!(env.iter().any(|(k, v)| k == "ANTHROPIC_AUTH_TOKEN" && v.is_empty()));
        assert!(env.iter().any(|(k, v)| k == "CLAUDE_CODE_OAUTH_TOKEN" && v.is_empty()));
    }

    #[test]
    fn provider_mismatch_is_rejected_without_leak() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let openai_acct = account(&conn, &ws, "openai");
        let store = temp_store();
        store.set(&openai_acct.keychain_ref, SENTINEL).unwrap();

        let claude = find_adapter("claude-agent-sdk").unwrap();
        let err = resolve_session_env(&claude, Some(&openai_acct), &store).unwrap_err();
        assert_eq!(err, "Selected account does not match this agent's provider");
        assert!(!err.contains(SENTINEL));
    }

    #[test]
    fn pty_adapters_reject_an_account_and_never_inject() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic");
        let store = temp_store();
        store.set(&acct.keychain_ref, SENTINEL).unwrap();

        // The whole point of ambient-PTY: a key bound to a PTY agent must NEVER reach the
        // env. With api_key_env: None, an account is rejected (not silently injected).
        for id in ["claude-code", "codex", "gemini"] {
            let a = find_adapter(id).unwrap();
            let err = resolve_session_env(&a, Some(&acct), &store).unwrap_err();
            assert!(!err.contains(SENTINEL), "{id} leaked the key");
            assert_eq!(err, "This agent does not support API key accounts");
        }
    }

    #[test]
    fn missing_key_fails_closed() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic"); // key never stored
        let store = temp_store();

        let claude = find_adapter("claude-agent-sdk").unwrap();
        let err = resolve_session_env(&claude, Some(&acct), &store).unwrap_err();
        assert_eq!(err, "Stored key for this account is missing");
    }

    #[test]
    fn load_session_account_scopes_to_workspace() {
        let conn = migrated_conn();
        let ws_a = workspace::create(&conn, "A", "mixed").unwrap().id;
        let ws_b = workspace::create(&conn, "B", "mixed").unwrap().id;
        let acct = account(&conn, &ws_a, "anthropic");

        assert!(load_session_account(&conn, None, &ws_a).unwrap().is_none());
        assert!(load_session_account(&conn, Some(&acct.id), &ws_a)
            .unwrap()
            .is_some());
        // Account belongs to ws_a, not ws_b -> rejected.
        assert!(load_session_account(&conn, Some(&acct.id), &ws_b).is_err());
        // Nonexistent id -> rejected.
        assert!(load_session_account(&conn, Some("nope"), &ws_a).is_err());
    }

    #[test]
    fn create_isolated_home_is_fresh_0700_and_fails_on_reuse() {
        let mut p = std::env::temp_dir();
        p.push(format!("uaw-home-{}.home", new_id()));

        create_isolated_home(&p).expect("fresh dir created");
        assert!(p.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "isolated home must be 0700");
        }

        assert!(create_isolated_home(&p).is_err());

        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn isolated_home_points_credential_vars_at_the_dir_and_keeps_the_key() {
        let base = vec![
            ("ANTHROPIC_API_KEY".to_string(), "sk-secret".to_string()),
            ("ANTHROPIC_AUTH_TOKEN".to_string(), String::new()),
        ];
        let out = with_isolated_home(base, "/tmp/uaw-xyz.home");
        for k in ["HOME", "USERPROFILE", "CLAUDE_CONFIG_DIR", "APPDATA", "LOCALAPPDATA"] {
            assert!(
                out.iter().any(|(kk, v)| kk == k && v == "/tmp/uaw-xyz.home"),
                "missing isolated var {k}"
            );
        }
        assert_eq!(
            out.iter()
                .filter(|(_, v)| v == "sk-secret")
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["ANTHROPIC_API_KEY"],
        );
    }

    #[test]
    fn requires_account_gate() {
        let sdk = find_adapter("claude-agent-sdk").unwrap();
        let pty = find_adapter("claude-code").unwrap();
        // SDK + no account -> fail closed, fixed secret-free string.
        let err = validate_account_required(&sdk, None).unwrap_err();
        assert_eq!(err, "This agent requires a provider account");
        // PTY + no account -> allowed (legacy ambient behavior).
        assert!(validate_account_required(&pty, None).is_ok());
        // SDK + an account -> allowed.
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "W", "mixed").unwrap().id;
        let acct = account(&conn, &ws, "anthropic");
        assert!(validate_account_required(&sdk, Some(&acct)).is_ok());
    }
}
