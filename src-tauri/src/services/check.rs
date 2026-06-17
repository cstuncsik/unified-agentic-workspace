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
