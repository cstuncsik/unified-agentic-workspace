# Agent & Terminal Configuration — Design

**Goal:** Let the user configure, per PTY agent, the CLI **binary** and **extra args**, and configure the embedded terminal's **theme** (ANSI palette + bg/fg/cursor) and **font size** — via a hand-editable JSON config file, with an in-app Settings page to follow.

**Status:** This spec covers **Slice ① (config file + backend/frontend wiring)** in full. **Slice ② (in-app Settings page)** is outlined at the end and gets its own spec.

**Storage decision:** A JSON **file**, not SQLite. Config is small, read-mostly, and the user explicitly wants to hand-edit it (as in their `kommand0` reference). SQLite stays for the relational domain data (workspaces/projects/sessions/reviews…); this feature does not touch it. The app's only high-volume data (agent transcripts) is already on the filesystem.

> Revised after `cst:plan-review` (2026-07-18): pure-fn seams redrawn (`pick_program`, `parse`, `config_path`); SDK exclusion made enforced (API-key-exfil guard); `warning` made dataless; two frontend races fixed; e2e config isolation added.

---

## Architecture

Loading is split into **pure functions** (`services/config.rs`, Tauri-free, the unit-test seam — mirroring `login_path.rs`) and a **thin impure reader at the command boundary** (which alone touches `AppHandle`/the filesystem — mirroring `transcripts_base()` and `resolve_sdk_sidecar(resource_dir)`, since every `services/` file is Tauri-free by convention).

Pure (in `services/config.rs`):
- `parse(contents: &str) -> (Config, Option<String>)` — parse JSON → `serde_json::Value`, then extract each field leniently (see Error handling). Whole-file failure → all defaults + a warning; valid JSON with a bad field → that field defaults, silently.
- `config_path(env_override: Option<OsString>, app_data_dir: &Path) -> PathBuf` — `UAW_CONFIG_PATH` else `<app_data_dir>/config.json`. No CWD component, ever.
- `pick_program(env_override: Option<&str>, cfg_bin: Option<&str>, default: &str) -> String` — precedence with a shared trim/non-empty guard applied to **both** env and `cfg_bin`: a set-but-empty/whitespace value is ignored.

Impure (at the command boundary, e.g. a `read_config` helper next to `transcripts_base`):
- Resolve the path via `config_path`, then classify: **absent** → `(defaults, no warning)`; **symlink / non-regular / unreadable / larger than the size cap** → `(defaults, warning)` (dataless — see Security); **present regular file** → read to a `String` and hand to `parse`.

Consumed in exactly two places, each already holding `AppHandle`:
1. **Agent spawn** (`commands/agent_sessions.rs::start_agent_session`) — reads config, resolves `bin`/`args`.
2. **`get_app_config` command** — returns `{ terminal, warning }` for the frontend.

**Read on demand** (each spawn / each `get_app_config`), not cached — the file is tiny, so hand-edits take effect for the next session/terminal without a restart. Every read is fail-safe (never throws/panics; any problem → defaults [+ warning]).

## Config file

- **Path:** `UAW_CONFIG_PATH` if set (tests/e2e escape hatch, like `UAW_DB_PATH`/`UAW_TRANSCRIPTS_DIR`), else `<app_data_dir>/config.json`. macOS: `~/Library/Application Support/io.n8n.uaw/config.json`.
- **App-data-dir only — never repo-local.** A repo must not be able to plant a `config.json` that changes which binary UAW spawns (a code-execution injection vector). The path is derived solely from the OS app-data dir / the explicit env var; no user/CWD component is joined.
- **fs guards** (impure reader): only a **regular file** is read — a **symlink or non-regular** path is refused (→ defaults + warning), which also blunts a symlink-to-a-secret read. Files over a **64 KiB** cap are refused (→ defaults + warning) — self-DoS bound. `serde_json`'s default recursion limit is kept (do **not** `disable_recursion_limit`).
- **Absent file is normal** (not a warning) → all defaults. Present-but-unreadable/invalid → defaults **+ a warning**.
- Unknown keys are ignored on read and **preserved** on write (Slice ②).

### Schema (all fields optional)

```jsonc
{
  "agents": {
    // keyed by adapter id; PTY adapters ONLY. Any other id (incl. the SDK adapter
    // "claude-agent-sdk", or a typo) is ignored — see "SDK exclusion" below.
    "claude-code": { "bin": "/abs/path/to/claude", "args": ["--model", "sonnet"] },
    "codex":       { "bin": null, "args": [] },
    "gemini":      { "bin": null, "args": [] }
  },
  "terminal": {
    "fontSize": 13,
    "theme": {                          // camelCase — xterm ITheme keys, verbatim
      "background": "#000000", "foreground": "#cccccc", "cursor": "#ffffff",
      "black": "…", "red": "…", "green": "…", "yellow": "…",
      "blue": "…", "magenta": "…", "cyan": "…", "white": "…",
      "brightBlack": "…", "brightRed": "…", "brightGreen": "…", "brightYellow": "…",
      "brightBlue": "…", "brightMagenta": "…", "brightCyan": "…", "brightWhite": "…"
    }
  }
}
```

- `agents[id].bin`: `string | null`. Absolute path recommended; a bare name resolves on the (already-augmented) PATH. `null`/absent/empty/whitespace → the adapter default (guarded identically to the env override).
- `agents[id].args`: `string[]`, **appended after** the adapter's base args.
- **`terminal.theme` is a string-keyed map merged verbatim over the default palette** — keys pass through untouched, so xterm's camelCase `ITheme` names (`brightBlack`, `selectionBackground`, `cursorAccent`, …) survive. Do **not** model it as a snake_case serde struct (that would silently drop `brightBlack` et al.). Any key whose value isn't a string is dropped (default kept); colour strings are passed to xterm **unvalidated** (xterm ignores a bad colour).
- `terminal.fontSize`: an **integer in `6..=72`**; anything else (out of range, non-integer, non-number) → default `13`.

### Default terminal theme (vivid, standard)

Ships as the built-in default (backend default *and* the frontend store's seed constant — see Frontend), so colours look like a normal terminal even with no user config. VS Code Dark+ terminal palette:

```
background #000000  foreground #cccccc  cursor #ffffff
black   #000000  red #cd3131  green #0dbc79  yellow #e5e510
blue    #2472c8  magenta #bc3fbc  cyan #11a8cd  white #e5e5e5
brightBlack #666666  brightRed #f14c4c  brightGreen #23d18b  brightYellow #f5f543
brightBlue  #3b8eea  brightMagenta #d670d6  brightCyan #29b8db  brightWhite #ffffff
```

## Backend behavior

- **`resolve_program(adapter, bin_override: Option<&str>) -> String`** — widened to take the *resolved* override, not the whole `Config` (keeps the adapter registry decoupled + independently testable). It delegates to the pure `pick_program(env_override, bin_override, adapter.program)`. `UAW_AGENT_BIN` is read **only at the `agent_sessions.rs` call site** (`std::env::var`), never inside `services/`. Precedence: `UAW_AGENT_BIN` → `config.agents[id].bin` → adapter default.
- **Spawn args**: `adapter.args` (base) `++` `config.agents[id].args`, in order.
- **SDK exclusion — enforced, not documented.** `claude-agent-sdk` is the adapter that gets `ANTHROPIC_API_KEY`; a config-sourced program/args there would hand the key to an attacker-chosen binary. Enforcement is layered: (1) `start_agent_session` already early-returns to the SDK path *before* `resolve_program` (control-flow), and (2) config merge/lookup **whitelists PTY adapter ids** — an `agents` entry whose id is not a live PTY adapter (`kind == "pty"`) is dropped during parse, so `agents["claude-agent-sdk"]` can never take effect even if a future refactor routes the SDK program through the widened resolver. SDK program/args are **never** config-sourced.
- **`get_app_config() -> { terminal, warning }`** — returns only the merged `terminal` (`{ fontSize, theme }`) and `warning` (`string | null`). **No `agents`** — it has no Slice ① consumer, and Slice ② needs the *raw* file (to merge-preserve unknown keys), not this merged view.

## Frontend behavior

- **`src/api/appConfig.ts`** — `getAppConfig()` wrapping `invoke<AppConfig>("get_app_config")` (stores never call `invoke` directly — repo convention). **`src/types/appConfig.ts`** — `AppConfig` mirroring the Rust return; type `terminal.theme` as xterm's `ITheme` (from `@xterm/xterm`) so a key typo fails at compile time.
- **`useAppConfig` store** (`src/stores/appConfig.ts`): `terminal` is **seeded synchronously with the default constant** (fontSize 13 + the VS Code Dark+ palette — the same values the backend defaults to), so a terminal mounting before load is still correct. `load()` (idempotent, load-once; no `loadToken` — it's global, not workspace-scoped) calls `getAppConfig()` and overwrites `terminal` + sets `warning`.
- **`App.vue`** `onMounted`: `const toast = useToast()` (not imported today), then **`await appConfig.load(); if (appConfig.warning) toast.error(appConfig.warning)`** — the `await` is required or the synchronous `warning` read is always the pre-load `null` and the toast never fires.
- **`TerminalTab.vue`**: read `appConfig.terminal` at mount and construct `new Terminal({ fontSize, theme, convertEol: false, cursorBlink: true })`. `theme` is xterm `ITheme` → passed straight in, no transform. The store's synchronous seed makes this race-free without awaiting `load()`; construction stays at the top of `onMounted` before the existing sequence (transcript replay → `onData` → `listen` → ResizeObserver → rAF `doFit`), and `fontSize` in the constructor means `doFit`/`fonts.ready` measure the right metrics. Live re-apply to open terminals is Slice ②.

## Error handling

- **Absent file** → defaults, **no** warning (the first-run no-toast invariant).
- **Whole-file failure** (not valid JSON, not a JSON object, symlink/non-regular, oversize, unreadable) → all defaults **+ a dataless warning**.
- **Valid JSON, bad field** (wrong type / out-of-range) → that field defaults, **silently** (no warning; Slice ②'s editor prevents bad fields). Implemented by parsing to `serde_json::Value` then extracting each field with a type-checked fallback — this is what makes the merge lenient rather than all-or-nothing.
- **`warning` is dataless.** Never `serde_json::Error::to_string()` (it embeds input fragments — a hand-edited secret or symlink target would leak into the toast/logs; the repo's `sdk::parse_models` already maps errors to a fixed string for exactly this reason). At most include serde's `line`/`column` (safe): `"config.json is invalid (line N, col M); using defaults."`; the symlink/oversize cases get their own fixed strings.
- **Warning freshness asymmetry** (documented limitation): `bin`/`args` are re-read per spawn, but `warning` is fetched once at startup — hand-editing the file into invalid JSON *after* startup silently falls back on the next spawn with no new warning until restart. Slice ②'s save-time validation closes this.

## Testing

Backend pure-fn unit tests (`services/config.rs`), parallel-safe (no global-env mutation — the env override is a parameter, per the `login_path.rs` seam):
- `pick_program`: env-set → env wins; env unset + `cfg_bin` set → `cfg_bin`; both unset → default; `cfg_bin` `null`/`""`/whitespace → default (empty-bin must not `spawn("")`); env empty/whitespace → falls through to `cfg_bin`/default.
- `config_path`: honours `UAW_CONFIG_PATH`; else `<app_data_dir>/config.json`; **never** returns a CWD-relative path (the "never repo-local" invariant).
- args: base `vec!["--foo"]` + cfg `["--model","sonnet"]` → `["--foo","--model","sonnet"]` (use a **non-empty base** fixture — real PTY adapters ship `vec![]`, so an empty-base test is vacuous).
- merge: **mixed fixture** (valid `agents` + bad `terminal.fontSize`) → agents survive **and** fontSize defaults (proves per-field, not whole-file, leniency); separate bad-JSON fixture → all defaults + `warning.is_some()`; absent (`None` contents) → defaults + `warning.is_none()`.
- theme: provided keys override + missing keep defaults; a non-string value → that key dropped; unknown key ignored, siblings still merge; a bad-but-string colour passes through (no validation).
- fontSize bounds: `5`/`73`/`0`/`-1`/`12.5`/`"big"` → 13; `6`/`72`/`13` → kept.
- **SDK exclusion**: `agents["claude-agent-sdk"].bin` is ignored (never reaches `pick_program`).
- Warning assertions use `is_some()` + a stable substring (`"using defaults"`), not the exact template (brittle/tautology).

**No frontend unit tests** — the repo has no vitest/`@vue/test-utils` and gates on Rust unit tests + wdio e2e. The two real behaviours are covered by e2e instead:
- **e2e isolation (required):** `wdio.conf.ts` `beforeSession` must set `UAW_CONFIG_PATH` to a nonexistent path under `sessionDir` (mirroring `UAW_DB_PATH`/`UAW_TRANSCRIPTS_DIR`/`UAW_KEYSTORE_DIR`) → all-defaults, no startup toast. Without this, e2e reads the developer's real `config.json` (its args append to the fake agent; a malformed one fires a startup toast that — per the toast-overlay gotcha — intercepts clicks and breaks unrelated specs).
- **config→behaviour e2e** (in `e2e/specs/agent-terminal.e2e.ts`): with a fixture `UAW_CONFIG_PATH`, assert a configured `args` value reaches the spawned (fake) agent's argv and a configured `fontSize`/theme reaches the terminal — proving config actually changes behaviour (nothing does today).

## Wiring (connective tissue to include in the plan)

- `pub mod config;` in `src-tauri/src/services/mod.rs`.
- `#[tauri::command] get_app_config` under `commands::` (e.g. `commands/config.rs` or in `commands/agent_sessions.rs`), delegating to `services::config` — register it in the `tauri::generate_handler!` list in `src-tauri/src/lib.rs`.
- Frontend `src/api/appConfig.ts` + `src/types/appConfig.ts` (above).

## Out of scope (Slice ② — separate spec)

- The in-app **Settings page** (enable the disabled nav): edit per-agent `bin`/`args`, theme, fontSize; **merge-on-save preserving unknown/hand-edited keys** (reads the *raw* file, not the merged view); **live-apply** theme/fontSize to open terminals (`term.options.theme = …`); save-time validation (closes the warning-freshness gap); make `TerminalTab`'s host background/padding track the themed background so a non-black `theme.background` isn't framed in `#000`.

## Security considerations

- **`bin`/`args` are a code-execution control surface — intended** (choosing which CLI to run), running **as the user**, no elevation; same trust level as the PATH the app already spawns from. The mitigation that matters is **provenance**: config is read only from the app-data dir / explicit env var, **never a repo/worktree** (verified: the login-shell PATH probe runs with a neutral CWD and reads back only `PATH`, so `.envrc`/direnv can't reach UAW's env; the filename is constant, no path traversal).
- **SDK-key exfil is prevented by enforcement** (whitelist PTY ids + SDK-branch early-return), not by documentation — see Backend behavior. Highest-severity item; must have the `agents["claude-agent-sdk"].bin`-ignored test.
- **`warning` is dataless** — no file bytes reach the UI/logs (see Error handling). The **symlink/non-regular refusal** blunts a symlink-to-secret read of the config path.
- **Lateral-write vector (named, accepted):** the config is a user-writable file, so a PTY agent (which runs as the user with `file_edits`) could rewrite `<app_data_dir>/config.json` to hijack a sibling agent's `bin`. This is **same-uid, not an escalation**, and file permissions can't stop it (the same user can chmod/write), so we don't pretend `0600` mitigates it — provenance + the fact that it's the user's own trust boundary are the honest posture. Documented so Slice ②/future work can revisit if a sandbox is ever added.
- **`args` never reach a shell** — `pty::spawn` uses `CommandBuilder::new(program); cmd.args(args)` (exec/posix_spawn, no `sh -c`), so no shell-injection surface. A user setting `bin=/bin/sh, args=["-c",…]` is intended self-configuration.
- **`get_app_config` returns no secrets** (`terminal` + `warning` only); provider keys remain in the keychain, untouched.
