//! Claude Agent SDK sidecar runtime. The sidecar (a Node process) runs the SDK's
//! `query()` headlessly and emits one NDJSON object per message; this module
//! parses those lines, masks the injected key, derives terminal status, and owns
//! the piped-child spawn + process-group kill. The pure functions below are the
//! unit-tested seams; the transcript-write/emit closure lives in the command.
//!
//! Bare-name spawns here rely on the process PATH being augmented at startup
//! (see `services::login_path`) so a GUI-launched bundle can find the binary.

use serde::Serialize;
use std::io::BufRead;
use std::io::Read;
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

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

/// One model the user can pick for an SDK session, from the provider's models API.
#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

/// Parse the Anthropic `/v1/models` body into pickable models. `Ok(vec![])` for an
/// empty `data`; `Err` for a non-`{data}` body (an API error) or malformed JSON.
/// `display_name` falls back to `id`; non-object `data` elements are skipped; never
/// panics. The `Err` value is a fixed, dataless reason — the command maps any `Err`
/// to a fixed opaque string, so the raw body is never surfaced.
pub fn parse_models(stdout: &str) -> Result<Vec<ModelInfo>, String> {
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).map_err(|_| "parse".to_string())?;
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "shape".to_string())?;
    Ok(data
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|x| x.as_str()).filter(|s| !s.is_empty())?;
            let display_name = m.get("display_name").and_then(|x| x.as_str()).unwrap_or(id);
            Some(ModelInfo {
                id: id.to_string(),
                display_name: display_name.to_string(),
            })
        })
        .collect())
}

/// Normalize a caller-supplied mode to the sidecar contract. Unknown/None → "plan"
/// (fail safe: never silently grant edit). Returns 'static so a caller cannot smuggle
/// arbitrary argv into the sidecar through the mode slot.
pub fn normalize_sdk_mode(mode: Option<&str>) -> &'static str {
    match mode {
        Some("edit") => "edit",
        _ => "plan",
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
        {
            let _ = self.pid;
        }
    }
}

pub struct SdkSpawned {
    pub stdout: ChildStdout,
    pub child: Child,
    pub handle: SdkHandle,
}

/// The Node binary used to run the sidecar scripts. Resolved from the injected env
/// first (so tests swap it per-call without mutating process-global state — keeping
/// the env-mutating tests parallel-safe), then the backend's own env, defaulting to
/// `node` on PATH (the documented prerequisite; a missing `node` yields an actionable
/// Node-prerequisite error — see `node_spawn_error`).
fn node_bin(env: &[(String, String)]) -> String {
    // Injected env first (test-swappable, parallel-safe), then process env, else `node`.
    // Mirror the `resolve_sidecar_script` idiom: a blank/whitespace override falls through.
    if let Some((_, v)) = env.iter().find(|(k, _)| k == "UAW_AGENT_NODE") {
        if !v.trim().is_empty() {
            return v.clone();
        }
    }
    match std::env::var("UAW_AGENT_NODE") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => "node".to_string(),
    }
}

/// Map a sidecar spawn failure to a message. A missing `node` (ENOENT) gets a specific,
/// actionable error so a shipped app surfaces the Node prerequisite instead of an opaque
/// failure; any other spawn error keeps the caller's opaque string.
fn node_spawn_error(e: &std::io::Error, opaque: &str) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "Node.js was not found on PATH. The SDK agent requires Node.js 18+ (PTY agents are unaffected).".to_string()
    } else {
        opaque.to_string()
    }
}

/// Spawn the sidecar via `node` as a piped child in `cwd`; goal as argv[2], mode as argv[3],
/// model as argv[4] (empty = SDK default), env injected, stdin null, stderr discarded.
pub fn spawn(
    program: &str,
    goal: &str,
    mode: &str,
    model: &str,
    cwd: &Path,
    env: &[(String, String)],
) -> Result<SdkSpawned, String> {
    // Run the sidecar as `node <script> <goal> <mode> <model>`. Direct-exec of the
    // .mjs (relying on the shebang) is dead on Windows and needs the exec bit on Unix;
    // `node <script>` works on all three OSes. The script still sees goal/mode/model
    // at argv[2..=4] (node is argv[0], the script argv[1]).
    let mut cmd = Command::new(node_bin(env));
    cmd.arg(program)
        .arg(goal)
        .arg(mode)
        .arg(model)
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
        .map_err(|e| node_spawn_error(&e, "Failed to start the agent sidecar"))?;
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

/// Run a short-lived helper, capture all stdout, enforce a wall-clock timeout.
/// Unlike `spawn` (which streams + owns a process group), this is request/response:
/// stderr is discarded, no handle is returned. A watcher thread waits on a condvar
/// up to `timeout`; the reader signals it the instant the child's stdout closes, so
/// a fast helper returns immediately (no full-timeout wait). On a real timeout the
/// watcher kills the child (it can't hit a reused PID — the kill happens under the
/// `done` lock, before `child.wait()` reaps). A killed run returns `Err` regardless
/// of captured stdout. Non-zero exit / spawn failure → `Err`. Every `Err` is the
/// fixed opaque "Failed to list models". The timeout is a backstop for a wedged
/// helper; the intended caller writes its output then exits immediately.
pub fn spawn_oneshot(
    program: &str,
    args: &[&str],
    cwd: &Path,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<String, String> {
    const ERR: &str = "Failed to list models";
    // Same `node <script> <args…>` invocation as `spawn` (see node_bin).
    let mut cmd = Command::new(node_bin(env));
    cmd.arg(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| node_spawn_error(&e, ERR))?;
    let mut stdout = child.stdout.take().ok_or_else(|| ERR.to_string())?;
    let pid = child.id();

    // (done, cvar): `done` becomes true when the reader finishes (child exited) OR
    // when the watcher kills on timeout — whichever first, under the lock.
    let state = Arc::new((Mutex::new(false), Condvar::new()));
    let killed = Arc::new(Mutex::new(false));
    let watcher = {
        let (state, killed) = (state.clone(), killed.clone());
        std::thread::spawn(move || {
            let (lock, cvar) = &*state;
            let done = lock.lock().unwrap();
            let (mut done, wait_res) = cvar
                .wait_timeout_while(done, timeout, |d| !*d)
                .unwrap();
            if wait_res.timed_out() && !*done {
                *killed.lock().unwrap() = true;
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                *done = true;
            }
        })
    };

    let mut out = String::new();
    let read_res = stdout.read_to_string(&mut out);
    {
        let (lock, cvar) = &*state;
        *lock.lock().unwrap() = true;
        cvar.notify_all();
    }
    let status = child.wait();
    let _ = watcher.join();

    if *killed.lock().unwrap() {
        return Err(ERR.to_string());
    }
    match (read_res, status) {
        (Ok(_), Ok(s)) if s.success() => Ok(out),
        _ => Err(ERR.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Read};

    #[test]
    fn normalize_mode_fails_safe_to_plan() {
        assert_eq!(normalize_sdk_mode(Some("edit")), "edit");
        assert_eq!(normalize_sdk_mode(Some("plan")), "plan");
        assert_eq!(normalize_sdk_mode(None), "plan");
        // Fail safe: case-sensitive, and anything unrecognized is plan, never edit.
        assert_eq!(normalize_sdk_mode(Some("EDIT")), "plan");
        assert_eq!(normalize_sdk_mode(Some("garbage")), "plan");
    }

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
        assert!(matches!(
            parse_sdk_line("{\"type\":\"assistant\",\"text\":\"hi\"}"),
            SdkLine::Event { .. }
        ));
        assert!(matches!(
            parse_sdk_line("{\"type\":\"result\",\"status\":\"success\"}"),
            SdkLine::Event { .. }
        ));
        assert_eq!(
            parse_sdk_line("{\"type\":\"error\",\"message\":\"boom\"}"),
            SdkLine::Error("boom".into())
        );
    }

    #[test]
    fn pump_skips_garbage_flags_result_and_error() {
        let canned = b"{\"type\":\"assistant\",\"text\":\"hi\"}\n\n\
                       garbage line\n\
                       {\"type\":\"tool\",\"name\":\"Read\"}\n\
                       {\"type\":\"result\",\"status\":\"success\"}\n";
        let mut events = 0;
        let out = pump_ndjson(&canned[..], |_| events += 1);
        assert_eq!(events, 5); // every line delivered (incl. Skips); 3 are Events
        assert!(out.saw_result);
        assert!(!out.saw_error);

        let err = b"{\"type\":\"result\",\"status\":\"error\"}\n";
        let out2 = pump_ndjson(&err[..], |_| {});
        assert!(out2.saw_result && out2.saw_error);
    }

    #[test]
    fn status_table() {
        assert_eq!(sdk_status(true, false, Some(0)), "exited");
        assert_eq!(sdk_status(false, false, Some(0)), "failed"); // exited 0, no result
        assert_eq!(sdk_status(true, true, Some(0)), "failed"); // error result
        assert_eq!(sdk_status(true, false, Some(1)), "failed"); // non-zero exit
        assert_eq!(sdk_status(true, false, None), "failed");
    }

    #[test]
    fn spawn_injects_env_overriding_inherited() {
        std::env::set_var("UAW_SDK_PROBE", "PARENT");
        let dir = std::env::temp_dir();
        // Swap "node" for `printenv` via the injected env. The script slot becomes
        // printenv's VAR arg (the var it echoes); goal/mode/model are empty (skipped).
        // The injected UAW_SDK_PROBE=INJECTED must override the inherited PARENT.
        let mut sp = spawn(
            "UAW_SDK_PROBE",
            "",
            "",
            "",
            &dir,
            &[
                ("UAW_AGENT_NODE".into(), "printenv".into()),
                ("UAW_SDK_PROBE".into(), "INJECTED".into()),
            ],
        )
        .expect("spawn printenv");
        let mut out = String::new();
        BufReader::new(&mut sp.stdout).read_to_string(&mut out).unwrap();
        sp.child.wait().unwrap();
        std::env::remove_var("UAW_SDK_PROBE");
        assert_eq!(out.trim(), "INJECTED"); // injected beats the inherited "PARENT"
    }

    #[test]
    fn spawn_forwards_mode_as_second_arg() {
        let dir = std::env::temp_dir();
        // Swap "node" for echo; empty script slot → echo receives "" GOAL edit m1 →
        // " GOAL edit m1", and trim() yields "GOAL edit m1" — proving goal/mode/model
        // arrive in order after the script arg.
        let mut sp = spawn(
            "",
            "GOAL",
            "edit",
            "m1",
            &dir,
            &[("UAW_AGENT_NODE".into(), "echo".into())],
        )
        .expect("spawn echo");
        let mut out = String::new();
        BufReader::new(&mut sp.stdout).read_to_string(&mut out).unwrap();
        sp.child.wait().unwrap();
        assert_eq!(out.trim(), "GOAL edit m1");
    }

    #[test]
    fn spawn_missing_node_reports_node_not_found() {
        // A missing `node` (ENOENT) must surface the Node prerequisite, not an opaque error,
        // so a shipped app tells the user what to install.
        let err = match spawn(
            "goal",
            "plan",
            "",
            "",
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "/no/such/node-xyz".into())],
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected spawn to fail"),
        };
        assert!(err.contains("Node.js was not found on PATH"), "got: {err}");
    }

    #[test]
    fn spawn_oneshot_captures_stdout() {
        // `echo "" hello` → " hello" → trimmed "hello".
        let out = spawn_oneshot(
            "",
            &["hello"],
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "echo".into())],
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn spawn_oneshot_nonzero_exit_is_err() {
        // `false` ignores its args and exits non-zero.
        assert!(spawn_oneshot(
            "",
            &[],
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "false".into())],
            std::time::Duration::from_secs(5),
        )
        .is_err());
    }

    #[test]
    fn spawn_oneshot_times_out() {
        // `sleep 10` (script slot = "10") outlasts the 50 ms timeout → killed → Err.
        let r = spawn_oneshot(
            "10",
            &[],
            &std::env::temp_dir(),
            &[("UAW_AGENT_NODE".into(), "sleep".into())],
            std::time::Duration::from_millis(50),
        );
        assert!(r.is_err());
    }

    #[test]
    fn parse_models_valid() {
        let json = r#"{"data":[{"id":"claude-opus-4-5","display_name":"Claude Opus 4.5"},{"id":"claude-sonnet-4-5","display_name":"Claude Sonnet 4.5"}]}"#;
        let m = parse_models(json).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].id, "claude-opus-4-5");
        assert_eq!(m[0].display_name, "Claude Opus 4.5");
    }
    #[test]
    fn parse_models_empty_data_is_ok_empty() {
        assert!(parse_models(r#"{"data":[]}"#).unwrap().is_empty());
    }
    #[test]
    fn parse_models_error_body_is_err() {
        assert!(parse_models(r#"{"error":{"type":"authentication_error"}}"#).is_err());
    }
    #[test]
    fn parse_models_truncated_is_err() {
        assert!(parse_models(r#"{"data":[{"id":"#).is_err());
    }
    #[test]
    fn parse_models_missing_display_name_falls_back_to_id() {
        let m = parse_models(r#"{"data":[{"id":"m1"}]}"#).unwrap();
        assert_eq!(m[0].display_name, "m1");
    }
    #[test]
    fn parse_models_skips_non_object_elements() {
        let m = parse_models(r#"{"data":[null,42,{"id":"x"}]}"#).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "x");
    }
    #[test]
    fn parse_models_skips_empty_id() {
        let m = parse_models(r#"{"data":[{"id":""},{"id":"real"}]}"#).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, "real");
    }
}
