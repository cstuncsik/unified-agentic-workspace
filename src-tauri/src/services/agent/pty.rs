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
