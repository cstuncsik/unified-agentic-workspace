//! Repairs the process PATH at startup. A GUI-launched app (macOS Finder/Dock, some
//! Linux desktop launches) inherits a minimal PATH lacking the user's shell-configured
//! dirs (homebrew/nvm/asdf), so bare-name spawns — the PTY agents, the SDK's `node`,
//! `git` — can't find their binaries. `augment_process_path()` runs the user's LOGIN
//! shell once to recover the real PATH and merges it in. It is called from `run()`
//! BEFORE Tauri spawns any thread: `set_var` under a concurrent `getenv` is a data race.

/// Windows/non-unix GUI apps inherit the full registry/system PATH — nothing to repair.
#[cfg(not(unix))]
pub fn augment_process_path() {}

#[cfg(unix)]
pub use imp::augment_process_path;

#[cfg(unix)]
mod imp {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    const NONCE_ENV: &str = "UAW_PATH_NONCE";
    // The nonce is supplied via the env var above (no interpolation into this string → no
    // shell injection); printf brackets the real PATH so rc-file banners can be skipped.
    const PROBE: &str = r#"printf '%s%s%s' "$UAW_PATH_NONCE" "$PATH" "$UAW_PATH_NONCE""#;
    const ALLOWED_SHELLS: &[&str] = &["zsh", "bash", "sh", "dash", "ksh"];

    /// Repair the process PATH from the user's login shell. Fail-safe: any problem leaves
    /// PATH untouched. MUST run on the main thread before any other thread can `getenv`
    /// (edition 2021: `set_var` is a safe call; the race mitigation is purely positional —
    /// do NOT wrap it in `unsafe`, which trips `unused_unsafe` under clippy `-D warnings`).
    pub fn augment_process_path() {
        let shell = pick_login_shell();
        let nonce = crate::util::new_id();
        let Some(login) =
            run_login_shell(&shell, &std::env::temp_dir(), &nonce, Duration::from_secs(5))
        else {
            return;
        };
        let current = std::env::var("PATH").unwrap_or_default();
        let merged = merge_paths(&login, &current);
        if !merged.is_empty() {
            std::env::set_var("PATH", merged);
        }
    }

    /// `$SHELL` if it is an absolute path to an existing, executable, allowlisted POSIX
    /// shell; otherwise `/bin/zsh` (macOS) / `/bin/sh`. Skips fish/nu/unset/garbage.
    fn pick_login_shell() -> PathBuf {
        if let Some(sh) = std::env::var_os("SHELL") {
            let p = PathBuf::from(&sh);
            let ok_name = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| ALLOWED_SHELLS.contains(&n));
            if p.is_absolute() && ok_name && is_executable(&p) {
                return p;
            }
        }
        PathBuf::from(if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/sh" })
    }

    fn is_executable(p: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    /// Run `<shell> -lc <PROBE>` from `cwd` (neutral, so no repo-local rc is sourced) with
    /// the nonce in env; return the captured PATH or None on any failure/timeout. The shell
    /// is a param (no `$SHELL` read, no `set_var`) so this is unit-testable + parallel-safe.
    fn run_login_shell(
        shell: &Path,
        cwd: &Path,
        nonce: &str,
        timeout: Duration,
    ) -> Option<String> {
        let child = Command::new(shell)
            .arg("-lc")
            .arg(PROBE)
            .env(NONCE_ENV, nonce)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let (tx, rx) = mpsc::channel();
        // Collect on a thread; bound the wait. On timeout the child is a harmless one-shot
        // (orphaned, no resources we hold) and the app proceeds with the un-augmented PATH.
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        match rx.recv_timeout(timeout) {
            Ok(Ok(out)) if out.status.success() => {
                extract_path(&String::from_utf8_lossy(&out.stdout), nonce)
            }
            _ => None,
        }
    }

    /// The PATH bracketed by the LAST pair of `nonce` markers (so a banner printed before
    /// the real `printf` can't win). None if not bracketed by two markers or empty between.
    fn extract_path(stdout: &str, nonce: &str) -> Option<String> {
        let end = stdout.rfind(nonce)?;
        let start = stdout[..end].rfind(nonce)?;
        let path = &stdout[start + nonce.len()..end];
        (!path.is_empty()).then(|| path.to_string())
    }

    /// Merge `login` dirs ahead of `current`, first-occurrence wins, empty segments dropped.
    fn merge_paths(login: &str, current: &str) -> String {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        let mut out: Vec<PathBuf> = Vec::new();
        for p in std::env::split_paths(login).chain(std::env::split_paths(current)) {
            if p.as_os_str().is_empty() {
                continue;
            }
            if seen.insert(p.clone()) {
                out.push(p);
            }
        }
        std::env::join_paths(out)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|_| current.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        #[test]
        fn merge_prepends_login_first_occurrence_wins() {
            assert_eq!(
                merge_paths("/opt/homebrew/bin:/usr/bin", "/usr/bin:/sbin"),
                "/opt/homebrew/bin:/usr/bin:/sbin"
            );
        }
        #[test]
        fn merge_dedups_within_a_side_and_drops_empty_segments() {
            assert_eq!(merge_paths("/a:/b:/a", "/c::/b"), "/a:/b:/c");
        }
        #[test]
        fn merge_preserves_full_order() {
            assert_eq!(merge_paths("/l1:/l2:/l3", "/c1:/c2"), "/l1:/l2:/l3:/c1:/c2");
        }
        #[test]
        fn merge_empty_login_returns_current_and_vice_versa() {
            assert_eq!(merge_paths("", "/usr/bin:/bin"), "/usr/bin:/bin");
            assert_eq!(merge_paths("/usr/bin:/bin", ""), "/usr/bin:/bin");
        }
        #[test]
        fn extract_takes_the_last_nonce_pair_past_banner_noise() {
            let n = "NONCEXYZ";
            let out = format!("Welcome {n} fake banner\n{n}/real/bin:/usr/bin{n}");
            assert_eq!(extract_path(&out, n).as_deref(), Some("/real/bin:/usr/bin"));
        }
        #[test]
        fn extract_none_when_unbracketed_or_empty() {
            assert_eq!(extract_path("no markers here", "NONCE"), None);
            assert_eq!(extract_path("oneNONCEmarker", "NONCE"), None);
            assert_eq!(extract_path("NONCENONCE", "NONCE"), None);
        }

        fn write_fake_shell(body: &str) -> PathBuf {
            let p = std::env::temp_dir().join(format!("uaw-fake-shell-{}", crate::util::new_id()));
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        #[test]
        fn run_login_shell_returns_the_nonced_path() {
            let sh = write_fake_shell(r#"printf '%s%s%s' "$UAW_PATH_NONCE" "/fake/bin:/usr/bin" "$UAW_PATH_NONCE""#);
            let got = run_login_shell(&sh, &std::env::temp_dir(), "NONCE123", Duration::from_secs(5));
            let _ = std::fs::remove_file(&sh);
            assert_eq!(got.as_deref(), Some("/fake/bin:/usr/bin"));
        }
        #[test]
        fn run_login_shell_none_on_nonzero_exit() {
            let sh = write_fake_shell("exit 1");
            let got = run_login_shell(&sh, &std::env::temp_dir(), "NONCE123", Duration::from_secs(5));
            let _ = std::fs::remove_file(&sh);
            assert_eq!(got, None);
        }
        #[test]
        fn run_login_shell_none_on_timeout() {
            let sh = write_fake_shell("sleep 5");
            let got = run_login_shell(&sh, &std::env::temp_dir(), "NONCE123", Duration::from_millis(100));
            let _ = std::fs::remove_file(&sh);
            assert_eq!(got, None);
        }
        #[test]
        fn is_executable_true_only_for_exec_regular_file() {
            let f = std::env::temp_dir().join(format!("uaw-exec-{}", crate::util::new_id()));
            std::fs::write(&f, "x").unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(!is_executable(&f), "non-exec file");
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(is_executable(&f), "exec file");
            let _ = std::fs::remove_file(&f);
            assert!(!is_executable(&std::env::temp_dir()), "a directory is not an executable program");
        }
    }
}
