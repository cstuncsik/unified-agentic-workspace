# Agent & Terminal Configuration (Slice ①) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A hand-editable `<app_data_dir>/config.json` that sets, per PTY agent, the CLI binary + extra args, and the embedded terminal's theme + font size — wired end-to-end (backend spawn + frontend xterm), with a vivid default palette.

**Architecture:** A Tauri-free `services/config.rs` holds pure/path-parameterized functions (`parse`, `config_path`, `pick_program`, `spawn_args`, `read_config_at`) — the unit-test seam. The command boundary (`commands/config.rs` + `commands/agent_sessions.rs`) supplies `AppHandle`/`app_data_dir` and reads `UAW_AGENT_BIN` from env. The frontend loads once via a `useAppConfig` store (seeded with the default palette so it's never muted pre-load) and reads it into xterm at mount.

**Tech Stack:** Rust (serde_json `Value` lenient extraction), Tauri 2 command, Vue 3 + Pinia, `@xterm/xterm` `ITheme`.

Spec: `docs/superpowers/specs/2026-07-16-agent-terminal-config-design.md`.

---

## File Structure

- **Create** `src-tauri/src/services/config.rs` — all pure/path-parameterized fns + their unit tests.
- **Modify** `src-tauri/src/services/mod.rs` — `pub mod config;`.
- **Modify** `src-tauri/src/services/agent/mod.rs` — delete `resolve_program` + its env-mutating test (replaced by `config::pick_program`).
- **Create** `src-tauri/src/commands/config.rs` — impure `load(&AppHandle)` + `#[tauri::command] get_app_config`.
- **Modify** `src-tauri/src/commands/mod.rs` — `pub mod config;`.
- **Modify** `src-tauri/src/lib.rs` — register `commands::config::get_app_config` in `generate_handler!`.
- **Modify** `src-tauri/src/commands/agent_sessions.rs` — resolve bin/args from config at the PTY spawn site.
- **Create** `src/types/appConfig.ts`, `src/api/appConfig.ts`, `src/stores/appConfig.ts`.
- **Modify** `src/App.vue` — load config + toast the warning.
- **Modify** `src/components/TerminalTab.vue` — construct xterm from the store's terminal config.
- **Modify** `wdio.conf.ts` — isolate `UAW_CONFIG_PATH`.
- **Modify** `e2e/specs/agent-terminal.e2e.ts` — clean-read smoke with a fixture config.

---

### Task 1: Config types, defaults, `pick_program`, `spawn_args`

**Files:** Create `src-tauri/src/services/config.rs`; Modify `src-tauri/src/services/mod.rs`.

- [ ] **Step 1: Register the module.** Add to `src-tauri/src/services/mod.rs` (after `pub mod completion;`):

```rust
pub mod config;
```

- [ ] **Step 2: Write `config.rs` with types, defaults, and the two pure combinators + their tests.**

```rust
//! User configuration (`<app_data_dir>/config.json`): per-PTY-agent binary/args
//! and terminal theme/font-size. Every function here is pure or path-parameterized
//! (no `AppHandle`, no `$SHELL`/env reads) so it is unit-testable + parallel-safe;
//! the command boundary supplies the path/`AppHandle` and reads `UAW_AGENT_BIN`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Config `agents.<id>` is honored ONLY for these PTY adapters. Any other id
/// (the SDK adapter `claude-agent-sdk`, or a typo) is dropped on parse — the SDK
/// adapter injects the API key, so its program/args must never come from config.
pub const PTY_AGENT_IDS: &[&str] = &["claude-code", "codex", "gemini"];
pub const DEFAULT_FONT_SIZE: u16 = 13;
const MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Default)]
pub struct AgentConfig {
    pub bin: Option<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TerminalConfig {
    pub font_size: u16,
    pub theme: BTreeMap<String, String>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self { font_size: DEFAULT_FONT_SIZE, theme: default_theme() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub agents: BTreeMap<String, AgentConfig>,
    pub terminal: TerminalConfig,
}

/// Vivid default palette (VS Code Dark+ terminal). camelCase xterm `ITheme` keys.
/// Kept in sync with the JS seed in `src/stores/appConfig.ts`.
pub fn default_theme() -> BTreeMap<String, String> {
    [
        ("background", "#000000"), ("foreground", "#cccccc"), ("cursor", "#ffffff"),
        ("black", "#000000"), ("red", "#cd3131"), ("green", "#0dbc79"), ("yellow", "#e5e510"),
        ("blue", "#2472c8"), ("magenta", "#bc3fbc"), ("cyan", "#11a8cd"), ("white", "#e5e5e5"),
        ("brightBlack", "#666666"), ("brightRed", "#f14c4c"), ("brightGreen", "#23d18b"),
        ("brightYellow", "#f5f543"), ("brightBlue", "#3b8eea"), ("brightMagenta", "#d670d6"),
        ("brightCyan", "#29b8db"), ("brightWhite", "#ffffff"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Program to spawn: `UAW_AGENT_BIN` (env, passed in) > config `bin` > adapter default.
/// A set-but-empty/whitespace value at either tier is ignored (falls through).
pub fn pick_program(env_override: Option<&str>, cfg_bin: Option<&str>, default: &str) -> String {
    for candidate in [env_override, cfg_bin] {
        if let Some(v) = candidate {
            if !v.trim().is_empty() {
                return v.to_string();
            }
        }
    }
    default.to_string()
}

/// Spawn argv: adapter base args, then config extra args (owned, order preserved).
pub fn spawn_args(base: &[&str], cfg_args: &[String]) -> Vec<String> {
    base.iter().map(|s| s.to_string()).chain(cfg_args.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_program_precedence() {
        assert_eq!(pick_program(Some("/env"), Some("/cfg"), "def"), "/env");
        assert_eq!(pick_program(None, Some("/cfg"), "def"), "/cfg");
        assert_eq!(pick_program(None, None, "def"), "def");
    }

    #[test]
    fn pick_program_ignores_empty_and_whitespace_at_both_tiers() {
        assert_eq!(pick_program(Some("  "), Some("/cfg"), "def"), "/cfg"); // empty env falls through
        assert_eq!(pick_program(None, Some(""), "def"), "def"); // empty cfg falls through (no spawn(""))
        assert_eq!(pick_program(Some(""), Some("   "), "def"), "def");
    }

    #[test]
    fn spawn_args_appends_after_a_nonempty_base_in_order() {
        let cfg = vec!["--model".to_string(), "sonnet".to_string()];
        assert_eq!(spawn_args(&["--foo"], &cfg), vec!["--foo", "--model", "sonnet"]);
        assert_eq!(spawn_args(&[], &cfg), vec!["--model", "sonnet"]);
        assert_eq!(spawn_args(&["--foo"], &[]), vec!["--foo"]);
    }
}
```

- [ ] **Step 3: Run the tests.**

Run: `cargo test --manifest-path src-tauri/Cargo.toml config::`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/services/config.rs src-tauri/src/services/mod.rs
git commit -m "feat(config): config types, defaults, pick_program + spawn_args"
```

---

### Task 2: `config_path` + `parse` (lenient, dataless warning)

**Files:** Modify `src-tauri/src/services/config.rs`.

- [ ] **Step 1: Add `config_path` + `parse` above the `#[cfg(test)]` block.**

```rust
/// The config path: `UAW_CONFIG_PATH` if set, else `<app_data_dir>/config.json`.
/// No CWD component — a repo/worktree can never influence which file is read.
pub fn config_path(env_override: Option<OsString>, app_data_dir: &Path) -> PathBuf {
    match env_override {
        Some(p) => PathBuf::from(p),
        None => app_data_dir.join("config.json"),
    }
}

/// Parse config JSON leniently: valid JSON object → extract known fields, bad
/// fields silently default (no warning); non-JSON / non-object → all defaults +
/// a DATALESS warning (line/col only — never echo the input, which may hold a
/// secret the user pasted). Unknown keys ignored; `agents` restricted to PTY ids.
pub fn parse(contents: &str) -> (Config, Option<String>) {
    let value: Value = match serde_json::from_str(contents) {
        Ok(v) => v,
        Err(e) => {
            return (
                Config::default(),
                Some(format!(
                    "config.json is invalid (line {}, col {}); using defaults.",
                    e.line(),
                    e.column()
                )),
            );
        }
    };
    let Some(obj) = value.as_object() else {
        return (
            Config::default(),
            Some("config.json must be a JSON object; using defaults.".to_string()),
        );
    };

    let mut cfg = Config::default();

    if let Some(agents) = obj.get("agents").and_then(Value::as_object) {
        for id in PTY_AGENT_IDS {
            if let Some(a) = agents.get(*id).and_then(Value::as_object) {
                let bin = a.get("bin").and_then(Value::as_str).map(str::to_string);
                let args = a
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                cfg.agents.insert((*id).to_string(), AgentConfig { bin, args });
            }
        }
    }

    if let Some(term) = obj.get("terminal").and_then(Value::as_object) {
        if let Some(fs) = term.get("fontSize").and_then(Value::as_u64) {
            if (6..=72).contains(&fs) {
                cfg.terminal.font_size = fs as u16;
            }
        }
        if let Some(theme) = term.get("theme").and_then(Value::as_object) {
            for (k, v) in theme {
                if let Some(s) = v.as_str() {
                    cfg.terminal.theme.insert(k.clone(), s.to_string());
                }
            }
        }
    }

    (cfg, None)
}
```

- [ ] **Step 2: Add tests inside the `tests` module.**

```rust
    #[test]
    fn config_path_env_wins_else_app_data() {
        let dir = Path::new("/data");
        assert_eq!(config_path(Some("/custom.json".into()), dir), PathBuf::from("/custom.json"));
        assert_eq!(config_path(None, dir), PathBuf::from("/data/config.json"));
        // never CWD-relative
        assert!(config_path(None, dir).is_absolute());
    }

    #[test]
    fn parse_bad_json_all_defaults_plus_warning() {
        let (cfg, w) = parse("{ not json");
        assert_eq!(cfg.terminal.font_size, DEFAULT_FONT_SIZE);
        assert!(cfg.agents.is_empty());
        assert!(w.unwrap().contains("using defaults"));
    }

    #[test]
    fn parse_non_object_top_level_defaults_plus_warning() {
        let (_cfg, w) = parse("[1,2,3]");
        assert!(w.is_some());
    }

    #[test]
    fn parse_lenient_one_bad_field_keeps_the_good_ones_no_warning() {
        // valid JSON, but fontSize is the wrong type: agents must survive AND fontSize defaults.
        let (cfg, w) = parse(r#"{"agents":{"codex":{"args":["-x"]}},"terminal":{"fontSize":"big"}}"#);
        assert_eq!(cfg.agents.get("codex").unwrap().args, vec!["-x".to_string()]);
        assert_eq!(cfg.terminal.font_size, DEFAULT_FONT_SIZE);
        assert!(w.is_none()); // per-field default is silent
    }

    #[test]
    fn parse_font_size_bounds() {
        for bad in ["5", "73", "0", "-1", "12.5", "\"big\""] {
            let (cfg, _) = parse(&format!(r#"{{"terminal":{{"fontSize":{bad}}}}}"#));
            assert_eq!(cfg.terminal.font_size, DEFAULT_FONT_SIZE, "fontSize {bad} should default");
        }
        for ok in [6u16, 13, 72] {
            let (cfg, _) = parse(&format!(r#"{{"terminal":{{"fontSize":{ok}}}}}"#));
            assert_eq!(cfg.terminal.font_size, ok);
        }
    }

    #[test]
    fn parse_theme_partial_merge_drops_non_strings_keeps_defaults() {
        let (cfg, _) = parse(r#"{"terminal":{"theme":{"background":"#123456","red":42,"custom":"#abc"}}}"#);
        assert_eq!(cfg.terminal.theme.get("background").unwrap(), "#123456"); // overridden
        assert_eq!(cfg.terminal.theme.get("green").unwrap(), "#0dbc79"); // default kept
        assert!(!cfg.terminal.theme.contains_key("red") || cfg.terminal.theme.get("red").unwrap() == "#cd3131"); // non-string dropped, default kept
        assert_eq!(cfg.terminal.theme.get("custom").unwrap(), "#abc"); // unknown string key passes through
    }

    #[test]
    fn parse_agents_whitelist_excludes_sdk_and_unknown_ids() {
        let (cfg, _) = parse(
            r#"{"agents":{"claude-agent-sdk":{"bin":"/evil"},"nope":{"bin":"/x"},"codex":{"bin":"/ok"}}}"#,
        );
        assert!(cfg.agents.get("claude-agent-sdk").is_none()); // SDK never config-sourced
        assert!(cfg.agents.get("nope").is_none());
        assert_eq!(cfg.agents.get("codex").unwrap().bin.as_deref(), Some("/ok"));
    }
```

- [ ] **Step 3: Run.** `cargo test --manifest-path src-tauri/Cargo.toml config::` → PASS.
- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/services/config.rs
git commit -m "feat(config): lenient parse + config_path with dataless warning"
```

---

### Task 3: `read_config_at` (fs classification, tempfile-tested)

**Files:** Modify `src-tauri/src/services/config.rs`.

- [ ] **Step 1: Add `read_config_at` above the tests.**

```rust
/// Read + parse the config at `path`. Impure (fs) but path-parameterized so it is
/// testable with real tempfiles (no `AppHandle`). Fail-safe: absent → defaults,
/// NO warning (first-run silence); symlink / non-regular / oversize / unreadable →
/// defaults + a dataless warning; regular file → `parse`.
pub fn read_config_at(path: &Path) -> (Config, Option<String>) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return (Config::default(), None), // absent
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return (Config::default(), Some("config.json must be a regular file; using defaults.".to_string()));
    }
    if meta.len() > MAX_BYTES {
        return (Config::default(), Some("config.json is too large; using defaults.".to_string()));
    }
    match std::fs::read_to_string(path) {
        Ok(s) => parse(&s),
        Err(_) => (Config::default(), Some("config.json is unreadable; using defaults.".to_string())),
    }
}
```

- [ ] **Step 2: Add tempfile tests (use `std::env::temp_dir()` + `crate::util::new_id()`, matching `login_path.rs` test helpers — no external crate).**

```rust
    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("uaw-cfg-{}-{name}", crate::util::new_id()))
    }

    #[test]
    fn read_absent_file_defaults_no_warning() {
        let (cfg, w) = read_config_at(&tmp_path("absent.json"));
        assert_eq!(cfg.terminal.font_size, DEFAULT_FONT_SIZE);
        assert!(w.is_none());
    }

    #[test]
    fn read_valid_file_applies_and_no_warning() {
        let p = tmp_path("valid.json");
        std::fs::write(&p, r#"{"terminal":{"fontSize":20}}"#).unwrap();
        let (cfg, w) = read_config_at(&p);
        let _ = std::fs::remove_file(&p);
        assert_eq!(cfg.terminal.font_size, 20);
        assert!(w.is_none());
    }

    #[test]
    fn read_oversize_file_defaults_plus_warning() {
        let p = tmp_path("big.json");
        std::fs::write(&p, "x".repeat((MAX_BYTES + 1) as usize)).unwrap();
        let (cfg, w) = read_config_at(&p);
        let _ = std::fs::remove_file(&p);
        assert_eq!(cfg.terminal.font_size, DEFAULT_FONT_SIZE);
        assert!(w.unwrap().contains("too large"));
    }

    #[test]
    #[cfg(unix)]
    fn read_symlink_is_refused() {
        let target = tmp_path("target.json");
        std::fs::write(&target, r#"{"terminal":{"fontSize":20}}"#).unwrap();
        let link = tmp_path("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let (cfg, w) = read_config_at(&link);
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_file(&target);
        assert_eq!(cfg.terminal.font_size, DEFAULT_FONT_SIZE); // NOT 20 — symlink refused
        assert!(w.unwrap().contains("regular file"));
    }
```

- [ ] **Step 3: Run.** `cargo test --manifest-path src-tauri/Cargo.toml config::` → PASS.
- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/services/config.rs
git commit -m "feat(config): read_config_at fs classification (regular-file/size guards)"
```

---

### Task 4: Wire bin/args into the PTY spawn; delete `resolve_program`

**Files:** Modify `src-tauri/src/commands/agent_sessions.rs`, `src-tauri/src/services/agent/mod.rs`, `src-tauri/src/commands/config.rs` (create), `src-tauri/src/commands/mod.rs`.

- [ ] **Step 1: Create `src-tauri/src/commands/config.rs` with the impure `load` helper** (the `#[tauri::command]` is added in Task 5; create the file now so the spawn site can call `load`).

```rust
//! Command-boundary glue for user config: resolves the path from `AppHandle` and
//! reads it. The pure logic lives in `services::config`.

use tauri::{AppHandle, Manager};

use crate::services::config::{self, Config};

/// Resolve `<app_data_dir>/config.json` (or `UAW_CONFIG_PATH`) and read it.
pub fn load(app: &AppHandle) -> (Config, Option<String>) {
    let dir = app.path().app_data_dir().unwrap_or_default();
    let path = config::config_path(std::env::var_os("UAW_CONFIG_PATH"), &dir);
    config::read_config_at(&path)
}
```

- [ ] **Step 2: Register the module.** Add to `src-tauri/src/commands/mod.rs` (keep alphabetical, after `pub mod coding_workspaces;`):

```rust
pub mod config;
```

- [ ] **Step 3: Replace the PTY-path program/args block in `agent_sessions.rs`.** Find (currently after the `if adapter.kind == "sdk" { ... }` early-return):

```rust
    // ---- PTY path ----
    let program = agent::resolve_program(&adapter);

    // Spawn the PTY.
    let args: Vec<&str> = adapter.args.clone();
    let spawned = pty::spawn(&program, &args, Path::new(&worktree_path), &env, cols, rows)?;
```

Replace with:

```rust
    // ---- PTY path ----
    // Per-agent bin/args from the user config. UAW_AGENT_BIN is read HERE (command
    // boundary, not in services/) and still overrides the bin so e2e can inject a
    // fake program. The SDK adapter returned above, so config never reaches it.
    let (cfg, _warning) = super::config::load(&app);
    let cfg_agent = cfg.agents.get(adapter.id);
    let env_bin = std::env::var("UAW_AGENT_BIN").ok();
    let program = config::pick_program(
        env_bin.as_deref(),
        cfg_agent.and_then(|a| a.bin.as_deref()),
        adapter.program,
    );
    let cfg_args: &[String] = cfg_agent.map(|a| a.args.as_slice()).unwrap_or(&[]);
    let args_owned = config::spawn_args(&adapter.args, cfg_args);
    let args: Vec<&str> = args_owned.iter().map(String::as_str).collect();
    let spawned = pty::spawn(&program, &args, Path::new(&worktree_path), &env, cols, rows)?;
```

- [ ] **Step 4: Add the `config` import to `agent_sessions.rs`.** With the other `use crate::services::...` lines, add:

```rust
use crate::services::config;
```
(If `use crate::services::agent;` becomes unused after Step 5, drop it — the compiler will flag it under `-D warnings`.)

- [ ] **Step 5: Delete `resolve_program` + its test from `src-tauri/src/services/agent/mod.rs`.** Remove the whole fn (the doc-comment `/// The program to actually spawn...` through the closing `}` of `resolve_program`) and its unit test (the `#[test] fn ...resolve_program...` that mutates `UAW_AGENT_BIN`). `pick_program`'s pure tests replace it.

- [ ] **Step 6: Build + full test + clippy.**

Run:
```bash
cargo test --manifest-path src-tauri/Cargo.toml 2>&1 | tail -20
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: all tests PASS; clippy clean (no dead-code/unused-import warnings).

- [ ] **Step 7: Commit.**

```bash
git add src-tauri/src/commands/config.rs src-tauri/src/commands/mod.rs src-tauri/src/commands/agent_sessions.rs src-tauri/src/services/agent/mod.rs
git commit -m "feat(config): resolve per-agent bin/args at PTY spawn; drop resolve_program"
```

---

### Task 5: `get_app_config` command

**Files:** Modify `src-tauri/src/commands/config.rs`, `src-tauri/src/lib.rs`.

- [ ] **Step 1: Add the command + serializable output to `commands/config.rs`.**

```rust
use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalOut {
    font_size: u16,
    theme: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct AppConfigOut {
    terminal: TerminalOut,
    warning: Option<String>,
}

/// Terminal theme/font-size + a parse warning for the frontend. Deliberately NOT
/// `agents` — no Slice ① reader, and Slice ② needs the raw file, not this merge.
#[tauri::command]
pub fn get_app_config(app: AppHandle) -> AppConfigOut {
    let (cfg, warning) = load(&app);
    AppConfigOut {
        terminal: TerminalOut { font_size: cfg.terminal.font_size, theme: cfg.terminal.theme },
        warning,
    }
}
```

- [ ] **Step 2: Register in `src-tauri/src/lib.rs`.** In the `tauri::generate_handler![ ... ]` list, add:

```rust
            commands::config::get_app_config,
```

- [ ] **Step 3: Build.** `cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5` → Finished.
- [ ] **Step 4: Commit.**

```bash
git add src-tauri/src/commands/config.rs src-tauri/src/lib.rs
git commit -m "feat(config): get_app_config command ({terminal, warning})"
```

---

### Task 6: Frontend types + api + store

**Files:** Create `src/types/appConfig.ts`, `src/api/appConfig.ts`, `src/stores/appConfig.ts`.

- [ ] **Step 1: `src/types/appConfig.ts`.**

```ts
import type { ITheme } from "@xterm/xterm";

export interface AppConfig {
  terminal: { fontSize: number; theme: ITheme };
  warning: string | null;
}
```

- [ ] **Step 2: `src/api/appConfig.ts`** (stores never call `invoke` directly — the api layer does).

```ts
import { invoke } from "@tauri-apps/api/core";
import type { AppConfig } from "../types/appConfig";

export function getAppConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_app_config");
}
```

- [ ] **Step 3: `src/stores/appConfig.ts`** — seed the default palette synchronously so a terminal mounting before `load()` resolves is never muted; `load()` is idempotent (load-once).

```ts
import { ref } from "vue";
import { defineStore } from "pinia";
import type { ITheme } from "@xterm/xterm";
import { getAppConfig } from "../api/appConfig";

// Kept in sync with `services/config.rs::default_theme()`. Duplicated JS-side so
// the pre-load terminal is correct without gating mount on the async load.
const DEFAULT_THEME: ITheme = {
  background: "#000000", foreground: "#cccccc", cursor: "#ffffff",
  black: "#000000", red: "#cd3131", green: "#0dbc79", yellow: "#e5e510",
  blue: "#2472c8", magenta: "#bc3fbc", cyan: "#11a8cd", white: "#e5e5e5",
  brightBlack: "#666666", brightRed: "#f14c4c", brightGreen: "#23d18b", brightYellow: "#f5f543",
  brightBlue: "#3b8eea", brightMagenta: "#d670d6", brightCyan: "#29b8db", brightWhite: "#ffffff",
};
const DEFAULT_FONT_SIZE = 13;

export const useAppConfig = defineStore("appConfig", () => {
  const terminal = ref<{ fontSize: number; theme: ITheme }>({
    fontSize: DEFAULT_FONT_SIZE,
    theme: DEFAULT_THEME,
  });
  const warning = ref<string | null>(null);
  let inflight: Promise<void> | null = null;

  function load() {
    if (!inflight) {
      inflight = getAppConfig()
        .then((cfg) => {
          terminal.value = cfg.terminal;
          warning.value = cfg.warning;
        })
        .catch(() => {
          /* keep the seeded defaults — a failed command must not blank the terminal */
        });
    }
    return inflight;
  }

  return { terminal, warning, load };
});
```

- [ ] **Step 4: Typecheck.** `pnpm typecheck 2>&1 | tail -5` → clean.
- [ ] **Step 5: Commit.**

```bash
git add src/types/appConfig.ts src/api/appConfig.ts src/stores/appConfig.ts
git commit -m "feat(config): frontend appConfig store/api/types (seeded default palette)"
```

---

### Task 7: Load config + toast the warning in `App.vue`

**Files:** Modify `src/App.vue`.

- [ ] **Step 1: Add imports** (with the other imports):

```ts
import { useToast } from "./composables/useToast";
import { useAppConfig } from "./stores/appConfig";
```

- [ ] **Step 2: Add the instances** (with the other store consts, e.g. after `const updater = useUpdater();`):

```ts
const toast = useToast();
const appConfig = useAppConfig();
```

- [ ] **Step 3: Load + toast in `onMounted`.** In the existing `onMounted(async () => { ... })`, add at the end (the `await` is required — a synchronous `warning` read after an un-awaited `load()` is always the pre-load `null`):

```ts
  await appConfig.load();
  if (appConfig.warning) toast.error(appConfig.warning);
```

- [ ] **Step 4: Typecheck + format.** `pnpm typecheck 2>&1 | tail -3 && npx prettier --check src/App.vue` → clean.
- [ ] **Step 5: Commit.**

```bash
git add src/App.vue
git commit -m "feat(config): load config on startup + toast an invalid-file warning"
```

---

### Task 8: Apply theme + fontSize in `TerminalTab.vue`

**Files:** Modify `src/components/TerminalTab.vue`.

- [ ] **Step 1: Import the store** (with the other imports):

```ts
import { useAppConfig } from "../stores/appConfig";
```

- [ ] **Step 2: Read it in setup + use it at construction.** Add near the top of `<script setup>` (after `const props = ...`):

```ts
const appConfig = useAppConfig();
```

Change the terminal construction in `onMounted` from:

```ts
  term = new Terminal({ convertEol: false, cursorBlink: true, fontSize: 13 });
```

to:

```ts
  const { fontSize, theme } = appConfig.terminal;
  term = new Terminal({ convertEol: false, cursorBlink: true, fontSize, theme });
```

- [ ] **Step 3: Typecheck + format.** `pnpm typecheck 2>&1 | tail -3 && npx prettier --check src/components/TerminalTab.vue` → clean.
- [ ] **Step 4: Commit.**

```bash
git add src/components/TerminalTab.vue
git commit -m "feat(config): construct xterm from the configured theme + fontSize"
```

---

### Task 9: e2e — isolate `UAW_CONFIG_PATH` + clean-read smoke

**Files:** Modify `wdio.conf.ts`, `e2e/specs/agent-terminal.e2e.ts`.

> Note: a deep config→argv/fontSize UI assertion is intentionally NOT added — `UAW_AGENT_BIN` always overrides `bin` (so bin isn't e2e-testable), xterm's canvas renderer makes a fontSize CSS assertion unreliable, and the fake agent doesn't echo argv. The fs→config→program/args chain is covered by the Task 1–3 unit tests (incl. `read_config_at` tempfiles); e2e's job here is hermeticity + proving a present config is read without breaking startup.

- [ ] **Step 1: Isolate the config path in `wdio.conf.ts` `beforeSession`.** After the `UAW_TRANSCRIPTS_DIR` line, add:

```ts
    process.env.UAW_CONFIG_PATH = path.join(sessionDir, "config.json");
```
(Nonexistent by default → all-defaults, no startup toast — matching the other on-disk isolators.)

- [ ] **Step 2: Add a clean-read smoke to `e2e/specs/agent-terminal.e2e.ts`.** After the worktree is created and before/around starting a terminal, add a test that writes a valid fixture config to `UAW_CONFIG_PATH` (read on-demand at the next spawn) and asserts the terminal starts with no error toast:

```ts
  it("reads a present config.json without breaking terminal startup", async () => {
    const fs = await import("node:fs");
    // Valid config: a font size + an arg. Read on-demand at the next spawn.
    fs.writeFileSync(
      process.env.UAW_CONFIG_PATH as string,
      JSON.stringify({ terminal: { fontSize: 16 }, agents: { "claude-code": { args: ["--uaw-e2e"] } } }),
    );

    await (await $('[aria-label="Worktree"]')).selectByIndex(0);
    await (await $('[aria-label="CLI"]')).selectByVisibleText("Claude Code");
    await (await $("button*=New terminal")).click();

    const term = await $('[data-testid="agent-terminal"]');
    await term.waitForExist({ timeout: 15_000 });
    // No invalid-config error toast (valid file → no warning).
    expect(await $('[data-testid="toast-error"]').isExisting()).toBe(false);
  });
```
Adjust the selectors (`aria-label`s / the CLI dropdown / the error-toast `data-testid`) to match `AgentsView.vue` + the design-system toast as needed while implementing; if a `New terminal` flow already exists earlier in the spec, fold the config write into that instead of duplicating the setup.

- [ ] **Step 3: Run the suite.** `pnpm e2e:docker 2>&1 | tail -30` → all specs PASS.
- [ ] **Step 4: Commit.**

```bash
git add wdio.conf.ts e2e/specs/agent-terminal.e2e.ts
git commit -m "test(config): isolate UAW_CONFIG_PATH in e2e + clean-read smoke"
```

---

## Self-Review

**Spec coverage:** per-agent `bin` (Task 4 `pick_program`) ✓; per-agent `args` (Task 4 `spawn_args`) ✓; terminal `theme` + `fontSize` (Tasks 2, 6, 8) ✓; vivid default palette (Task 1 `default_theme` + Task 6 seed) ✓; `UAW_AGENT_BIN` precedence at call site (Task 4) ✓; SDK exclusion enforced + tested (Task 2 whitelist) ✓; dataless warning (Task 2) ✓; frontend await-load toast (Task 7) + synchronous seed (Task 6) ✓; `get_app_config` = `{terminal, warning}` (Task 5) ✓; fs guards regular-file/64 KiB (Task 3) ✓; e2e isolation (Task 9) ✓; wiring `pub mod` ×2 + handler (Tasks 1,4,5) ✓.

**Type consistency:** `pick_program(env, cfg_bin, default)`, `spawn_args(base, cfg_args)->Vec<String>`, `config_path(env, app_data_dir)`, `read_config_at(path)`, `parse(&str)` used identically across tasks; `AppConfig { terminal:{fontSize,theme:ITheme}, warning }` matches the Rust `AppConfigOut` camelCase serialization; store `terminal` shape matches `TerminalTab` destructure.

**No placeholders:** every code step has complete code; commands have expected output. The only deliberately-loose item is the Task 9 e2e selectors (real UI labels resolved at implementation) — flagged inline, not a logic gap.
