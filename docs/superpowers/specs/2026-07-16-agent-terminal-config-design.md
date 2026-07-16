# Agent & Terminal Configuration — Design

**Goal:** Let the user configure, per PTY agent, the CLI **binary** and **extra args**, and configure the embedded terminal's **theme** (ANSI palette + bg/fg/cursor) and **font size** — via a hand-editable JSON config file, with an in-app Settings page to follow.

**Status:** This spec covers **Slice ① (config file + backend/frontend wiring)** in full. **Slice ② (in-app Settings page)** is outlined at the end and gets its own spec.

**Storage decision:** A JSON **file**, not SQLite. Config is small, read-mostly, and the user explicitly wants to hand-edit it (as in their `kommand0` reference). SQLite stays for the relational domain data (workspaces/projects/sessions/reviews…); this feature does not touch it. The app's only high-volume data (agent transcripts) is already on the filesystem.

---

## Architecture

One backend **config service** (`services/config.rs`) owns loading + validation. It is consulted in two places:

1. **Agent spawn** (`commands/agent_sessions.rs`) — for the per-agent `bin`/`args`.
2. **A `get_app_config` command** — returns the merged config (defaults + user overrides) plus an optional `warning`, consumed by the frontend for the terminal theme/fontSize and to surface a parse error.

The config is **read on demand** (each spawn / each `get_app_config` call), not cached at startup — the file is tiny, and this makes hand-edits take effect for the next session/terminal without an app restart. Every read is **fail-safe**: any error (missing file, bad JSON, wrong types) falls back to built-in defaults and yields a `warning` string rather than throwing.

## Config file

- **Location:** `UAW_CONFIG_PATH` if set (tests/e2e), else `<app_data_dir>/config.json` — mirroring `transcripts_base()`. macOS: `~/Library/Application Support/io.n8n.uaw/config.json`.
- **App-data-dir only — never repo-local.** A repo must not be able to plant a `config.json` that changes which binary UAW spawns (that would be a code-execution injection vector, the same class the login-shell fix guards against with a neutral CWD). The path is derived solely from the OS app-data dir / an explicit env var.
- **Absent file is normal** (not a warning) → all defaults. A present-but-invalid file → defaults **+ a warning**.
- Format: JSON. Unknown keys are ignored on read and **preserved** on write (Slice ②).

### Schema (all fields optional)

```jsonc
{
  "agents": {
    // keyed by adapter id; PTY adapters only. The SDK adapter (claude-agent-sdk)
    // is intentionally excluded — its program is resolved at runtime and its args
    // are SDK-managed.
    "claude-code": { "bin": "/abs/path/to/claude", "args": ["--model", "sonnet"] },
    "codex":       { "bin": null, "args": [] },
    "gemini":      { "bin": null, "args": [] }
  },
  "terminal": {
    "fontSize": 13,
    "theme": {
      "background": "#000000", "foreground": "#cccccc", "cursor": "#ffffff",
      "black": "…", "red": "…", "green": "…", "yellow": "…",
      "blue": "…", "magenta": "…", "cyan": "…", "white": "…",
      "brightBlack": "…", "brightRed": "…", "brightGreen": "…", "brightYellow": "…",
      "brightBlue": "…", "brightMagenta": "…", "brightCyan": "…", "brightWhite": "…"
    }
  }
}
```

- `agents[id].bin`: `string | null`. Absolute path recommended; a bare name is resolved on the (already-augmented) PATH.
- `agents[id].args`: `string[]`, **appended** after the adapter's base args (empty for PTY today). No UAW-managed args exist for PTY agents, so nothing to protect yet; documented so Slice ② can warn if that changes.
- `terminal.fontSize`: positive number; out-of-range/invalid → default 13.
- `terminal.theme`: any subset of xterm's `ITheme`; each provided key overrides the default palette, missing keys keep the default. Non-string values for a key → that key ignored (default kept).

### Default terminal theme (vivid, standard)

Ships as the built-in default so colors look like a normal terminal even with no user config (fixes the muted-palette issue). VS Code Dark+ terminal palette:

```
background #000000  foreground #cccccc  cursor #ffffff
black   #000000  red #cd3131  green #0dbc79  yellow #e5e510
blue    #2472c8  magenta #bc3fbc  cyan #11a8cd  white #e5e5e5
brightBlack #666666  brightRed #f14c4c  brightGreen #23d18b  brightYellow #f5f543
brightBlue  #3b8eea  brightMagenta #d670d6  brightCyan #29b8db  brightWhite #ffffff
```

## Backend behavior

- **`resolve_program`** precedence (widen the existing fn to take the config): `UAW_AGENT_BIN` env (global override, keeps e2e's fake-program injection) → `config.agents[id].bin` (if non-null) → `adapter.program` default.
- **Spawn args**: `adapter.args` (base) `++` `config.agents[id].args`. The PTY `spawn(...)` already takes `args: &[&str]`; the start command builds the vec.
- **`get_app_config()` command**: returns `{ agents, terminal, warning }` where `agents`/`terminal` are the merged (defaults + user) values and `warning` is `null` or a human-readable parse-error message.
- Config parsing/merging/precedence live in **pure functions** (given the file contents as a string), so they are unit-testable without the filesystem — following the `login_path.rs` seam.

## Frontend behavior

- A small **`useAppConfig` store** (`stores/appConfig.ts`): `load()` calls `get_app_config` once; exposes `terminal` (`{ fontSize, theme }`) and `warning`.
- **`App.vue`** `onMounted`: `appConfig.load()`; if `warning`, `toast.error(warning)` once (reuses the existing `useToast`).
- **`TerminalTab.vue`**: on mount, read `appConfig.terminal` and construct `new Terminal({ fontSize, theme, convertEol: false, cursorBlink: true })`. (Live re-apply to already-open terminals is Slice ②.)

## Error handling

- Missing file → defaults, no warning.
- Unreadable / invalid JSON / wrong top-level type → defaults + `warning: "config.json is invalid (<detail>); using defaults."`.
- Per-field invalid value → that field falls back to its default; the rest of the config still applies (lenient merge, not all-or-nothing) — except a whole-file parse failure, which is all-defaults.

## Testing

- Backend pure-fn unit tests (`services/config.rs`):
  - empty/absent config → all defaults; `resolve_program` returns the adapter default.
  - `bin` set → used; `UAW_AGENT_BIN` set → wins over `bin`.
  - `args` appended after base args, in order.
  - invalid JSON → defaults + a warning; per-field bad value → default for that field only.
  - theme merge: provided keys override, missing keys keep defaults.
- Frontend: `useAppConfig` maps the command result; `App.vue` toasts on `warning`. (xterm's rendered colors aren't unit-testable; verified manually.)

## Out of scope (Slice ② — separate spec)

- The in-app **Settings page** (enable the disabled nav): edit per-agent `bin`/`args`, theme, fontSize; **merge-on-save preserving unknown/hand-edited keys**; **live-apply** theme/fontSize to open terminals (`term.options.theme = …`).

## Security considerations

- `bin` lets the user point an adapter at any executable — this is **intended** (choosing which CLI to run), and runs **as the user**, no elevation; it is the same trust level as the PATH the app already spawns from. The mitigation that matters is **provenance**: the config is read only from the app-data dir / explicit env var, **never from a repo/worktree**, so opening an untrusted repo cannot change what UAW executes.
- `args` are passed as a `CommandBuilder` arg vector (no shell), so there is no shell-injection surface.
- `get_app_config` returns only config values (no secrets); provider keys remain in the keychain and are untouched by this feature.
