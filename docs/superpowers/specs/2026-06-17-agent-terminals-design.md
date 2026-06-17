# Milestone 10a — Interactive Agent Terminals

## Goal

Run real, interactive agent CLIs (Claude Code, Codex, Gemini) inside the app,
each in its own terminal tab, against an isolated git worktree — a
Conductor/Superset-style experience. Because the genuine CLI runs in a real
pseudo-terminal, the full feature set (slash commands, skills, subagents,
Shift+Tab permission cycling, MCP) works with no special handling. The agent's
work lands in the worktree and flows into the existing M9 review loop.

This is **M10a**, the first slice of the M10 "Agent Adapter MVP". Provider
accounts, OS keychain, and the API/Agent-SDK adapter are deferred to **M10b**
(the CLI adapters use the user's own local auth, so no secrets are handled here).

## Decisions

- **Real interactive terminal, not headless.** `claude -p`/headless one-shot is
  explicitly rejected — it loses the interactive Claude Code UX. We run the
  genuine CLI in a **PTY** (`portable-pty`) and render it with **xterm.js**.
- **Tab = terminal, bound to a worktree.** Each tab is one agent terminal tied to
  a coding workspace; a worktree can host several terminals. A top tab bar lists
  open terminals; a new tab picks a worktree + a CLI.
- **claude + codex + gemini**, via a built-in adapter registry. The runtime is
  CLI-agnostic; the program is overridable (`UAW_AGENT_BIN`) for tests.
- **Persist session row + transcript log.** Raw PTY output is appended to an
  on-disk transcript file; reopening a closed tab replays it. Reattaching to a
  still-running session after an app restart is deferred to M10b.
- **Review stays manual (M9).** The user runs the agent, then hits the existing
  *Complete and review*. No auto-review.

## Architecture

```
Frontend (xterm.js)                         Backend (Rust + portable-pty)
  TerminalTab.vue  --onData(bytes)------>    write_agent_session  --> PTY stdin
                   --onResize(cols,rows)-->  resize_agent_session --> PTY resize
                   <--listen(agent-output)--  reader thread: PTY out --> emit + transcript file
                   <--listen(agent-exit)----  reader thread: on EOF --> status + events row
  AgentsView.vue (top tab bar + new-terminal control + active TerminalTab)
```

A `start_agent_session` command resolves the worktree path + the adapter's
command, spawns the CLI in a PTY (`cwd = worktree`), inserts an `agent_sessions`
row, records a `session.started` event, registers the PTY handle for later
input/resize/stop, and starts a reader thread. The reader thread streams PTY
output bytes: each chunk is appended to the session's transcript file and emitted
as a Tauri `agent-output` event (base64). On EOF it reaps the child, sets the
session status + exit code, records an `agent.exited` event, and emits
`agent-exit`.

## Data model

Migration `0007_agent_sessions.sql`:

```sql
CREATE TABLE agent_sessions (
    id                   TEXT PRIMARY KEY NOT NULL,
    workspace_id         TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    coding_workspace_id  TEXT NOT NULL REFERENCES coding_workspaces(id) ON DELETE CASCADE,
    adapter_id           TEXT NOT NULL,          -- claude-code | codex | gemini
    command              TEXT NOT NULL,          -- resolved program, for audit
    status               TEXT NOT NULL DEFAULT 'running', -- running|exited|stopped|failed
    exit_code            INTEGER,
    transcript_path      TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
CREATE INDEX idx_agent_sessions_coding_workspace ON agent_sessions(coding_workspace_id);
CREATE INDEX idx_agent_sessions_workspace ON agent_sessions(workspace_id);
```

- **Transcript**: raw PTY output bytes appended to `<transcripts_dir>/<id>.log`,
  where `transcripts_dir` resolves `UAW_TRANSCRIPTS_DIR` env or
  `app_data_dir/transcripts`. The file path is stored on the row.
- **Event log**: lifecycle milestones (`session.started`, `agent.exited`) are
  recorded in the existing `events` table (M9) with the session id in the payload.

## Adapter descriptors

`services/agent/mod.rs`:

```rust
pub struct AgentCapabilities {
    pub streaming: bool, pub tool_use: bool, pub mcp: bool,
    pub file_edits: bool, pub shell_commands: bool, pub multi_turn: bool,
}
pub struct AgentAdapter {
    pub id: &'static str, pub name: &'static str,
    pub program: &'static str, pub args: Vec<String>,
    pub capabilities: AgentCapabilities,
}
```

Built-in registry: `claude-code` (`claude`), `codex` (`codex`), `gemini`
(`gemini`); all advertise full capabilities (it's a real interactive CLI). A pure
`resolve_program(adapter) -> String` returns `env UAW_AGENT_BIN || adapter.program`
so e2e/CI inject a fake interactive program. `list_agent_adapters()` exposes the
registry to the UI. Unknown `adapter_id` at start time is rejected.

This preserves the architecture's `AgentAdapter`/`AgentCapabilities` shape; the
richer `sendMessage`/structured-event surface grows in M10b for the API adapter.

## PTY service

`services/agent/pty.rs` wraps `portable-pty`:

- `spawn(program, args, cwd, cols, rows) -> SpawnedPty { child, writer, master, reader }`
  via `native_pty_system().openpty(...)` + `slave.spawn_command(CommandBuilder)`.
- A `Mutex<HashMap<String, PtyHandle>>` app-managed state (`AgentProcesses`)
  holds each running session's `writer` (Box<dyn Write + Send>), `master` (for
  resize), and `child` (for kill/wait).
- `pump(reader, mut on_chunk)` — the read loop (read into a buffer, call
  `on_chunk(&[u8])` until EOF). Pure of Tauri/DB, so it is unit-testable by
  feeding a reader and collecting chunks.
- Resize: `master.resize(PtySize { rows, cols, .. })`. Stop: `child.kill()`.

## Commands (`commands/agent_sessions.rs`)

- `list_agent_adapters() -> Vec<AgentAdapterDto>`
- `start_agent_session(app, state, coding_workspace_id, adapter_id, cols, rows) -> AgentSession`
- `write_agent_session(state, id, data: String)` — xterm `onData` gives a UTF-8
  string; the command writes its bytes straight to the PTY writer. (Output is
  base64 because mid-stream PTY bytes can split a UTF-8 codepoint; input from a
  keyboard does not, so a string is fine inbound.)
- `resize_agent_session(state, id, cols, rows)`
- `stop_agent_session(state, id)` — kill child, mark `stopped`
- `get_agent_session(state, id)`, `list_agent_sessions(state, coding_workspace_id)`
- `get_agent_session_transcript(state, id) -> String` — read the transcript file

Lock discipline mirrors the rest of the codebase: the DB `Mutex<Connection>` is
never held across the long-lived PTY work; the PTY registry is a separate mutex.

## Frontend

- New deps: `@xterm/xterm` + `@xterm/addon-fit` (DOM renderer — works headless).
- `stores/agentSessions.ts`: open sessions (tabs), active tab id, status per
  session; `listAdapters`, `start`, `write`, `resize`, `stop`, `loadForWorktree`.
- `components/AgentsView.vue`: a top **tab bar** of open terminals + a
  "New terminal" control (worktree picker + CLI picker via `list_agent_adapters`)
  + the active `TerminalTab`.
- `components/TerminalTab.vue`: owns an `xterm` `Terminal` + `FitAddon`; on mount
  it loads the transcript (replay) then attaches; `term.onData` →
  `write_agent_session`; a `ResizeObserver` + `fit()` → `resize_agent_session`;
  a global `listen("agent-output")`/`listen("agent-exit")` routes events by
  `session_id` to the matching terminal (`term.write(bytes)` / mark exited).
- New sidebar nav entry **Agents** (promoted from the planned-sections list).
- Coding rows gain a **Run agent** action that opens the Agents view with that
  worktree preselected in the new-terminal control.

## Error handling

- `start_agent_session`: unknown `coding_workspace_id` or `adapter_id` → `Err`;
  a PTY spawn failure (e.g. the CLI isn't installed) → `Err` surfaced as a toast.
- `write`/`resize`/`stop` on an unknown/closed session → `Err` (the tab shows the
  session has exited).
- The reader thread is resilient: a transcript write error is logged into the
  event payload but does not kill the stream; on reader error the session is
  marked `failed`.

## Security

- The CLI runs in the isolated worktree under the user's own local CLI auth — no
  API keys are handled in M10a. The spawned program + base args come from the
  built-in adapter registry (or the `UAW_AGENT_BIN` test override), never from
  repo content; `cwd` is the worktree. Interactive keystrokes are the user's
  input by design.
- An interactive agent can edit files and run commands in the worktree — that is
  the product. Isolation is the worktree boundary + the git review gate, not a
  command sandbox. Permission modes are handled interactively by the CLI itself
  (e.g. Claude Code's Shift+Tab), so UAW does not manage them.
- Transcripts are local files under the app data dir.

## Testing

- **Rust unit**: adapter registry + capabilities; `resolve_program` (default vs
  `UAW_AGENT_BIN` override); `agent_session` model CRUD + cascade; `pump` over a
  fixed reader; a PTY smoke test (spawn `sh -c 'echo hello'` in a PTY via the
  service, assert the captured output contains `hello` and the child exits 0).
- **e2e**: point `UAW_AGENT_BIN` at a fake interactive script (prints
  `AGENT-READY`, then `exec cat` to echo stdin). Open **Agents** → new terminal on
  a worktree with that adapter → assert the xterm shows `AGENT-READY`, type a line
  → assert it is echoed back in the terminal, then stop the session and assert the
  tab reflects `exited`/`stopped`. (xterm's DOM renderer exposes text in
  `.xterm-rows`, readable via the existing `textOf` helper.)

## Out of scope (M10b and later)

- Provider accounts, OS keychain, the Anthropic/OpenAI API (Agent SDK) adapter via
  a Node sidecar, the per-session model picker.
- Reattaching the frontend to a still-running PTY after an app restart (the
  backend session/transcript persist, but live re-attach is deferred).
- Structured `sendMessage`/typed agent events (the terminal carries raw bytes).
