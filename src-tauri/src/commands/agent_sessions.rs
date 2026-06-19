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
    // Raw PTY bytes are appended verbatim and can contain non-UTF-8 (a multibyte
    // codepoint split across a read boundary, or control bytes), so read as bytes
    // and lossily decode — read_to_string would error and drop the whole file.
    Ok(std::fs::read(&path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default())
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
            return Err(format!(
                "Coding workspace '{coding_workspace_id}' does not exist"
            ));
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
            None,
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
            .insert(id.clone(), handle);
    }

    // Record session.started.
    {
        let conn = state.lock().map_err(|e| e.to_string())?;
        let payload =
            serde_json::json!({ "agent_session_id": id, "adapter_id": adapter.id }).to_string();
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

#[tauri::command]
pub fn write_agent_session(app: AppHandle, id: String, data: String) -> Result<(), String> {
    let procs = app.state::<AgentProcesses>();
    let mut map = procs.0.lock().map_err(|e| e.to_string())?;
    let Some(handle) = map.get_mut(&id) else {
        return Err("Agent session is not running".into());
    };
    handle
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    handle.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resize_agent_session(app: AppHandle, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let procs = app.state::<AgentProcesses>();
    let map = procs.0.lock().map_err(|e| e.to_string())?;
    let Some(handle) = map.get(&id) else {
        return Ok(()); // resizing a finished session is a no-op
    };
    handle
        .master
        .resize(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
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
