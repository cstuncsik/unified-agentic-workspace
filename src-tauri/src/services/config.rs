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
    for v in [env_override, cfg_bin].into_iter().flatten() {
        if !v.trim().is_empty() {
            return v.to_string();
        }
    }
    default.to_string()
}

/// Spawn argv: adapter base args, then config extra args (owned, order preserved).
pub fn spawn_args(base: &[&str], cfg_args: &[String]) -> Vec<String> {
    base.iter().map(|s| s.to_string()).chain(cfg_args.iter().cloned()).collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn pick_program_precedence() {
        assert_eq!(pick_program(Some("/env"), Some("/cfg"), "def"), "/env");
        assert_eq!(pick_program(None, Some("/cfg"), "def"), "/cfg");
        assert_eq!(pick_program(None, None, "def"), "def");
    }

    #[test]
    fn pick_program_ignores_empty_and_whitespace_at_both_tiers() {
        assert_eq!(pick_program(Some("  "), Some("/cfg"), "def"), "/cfg");
        assert_eq!(pick_program(None, Some(""), "def"), "def");
        assert_eq!(pick_program(Some(""), Some("   "), "def"), "def");
    }

    #[test]
    fn spawn_args_appends_after_a_nonempty_base_in_order() {
        let cfg = vec!["--model".to_string(), "sonnet".to_string()];
        assert_eq!(spawn_args(&["--foo"], &cfg), vec!["--foo", "--model", "sonnet"]);
        assert_eq!(spawn_args(&[], &cfg), vec!["--model", "sonnet"]);
        assert_eq!(spawn_args(&["--foo"], &[]), vec!["--foo"]);
    }

    #[test]
    fn config_path_env_wins_else_app_data() {
        let dir = Path::new("/data");
        assert_eq!(config_path(Some("/custom.json".into()), dir), PathBuf::from("/custom.json"));
        assert_eq!(config_path(None, dir), PathBuf::from("/data/config.json"));
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
        let (cfg, w) = parse(r#"{"agents":{"codex":{"args":["-x"]}},"terminal":{"fontSize":"big"}}"#);
        assert_eq!(cfg.agents.get("codex").unwrap().args, vec!["-x".to_string()]);
        assert_eq!(cfg.terminal.font_size, DEFAULT_FONT_SIZE);
        assert!(w.is_none());
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
        let (cfg, _) = parse(r##"{"terminal":{"theme":{"background":"#123456","red":42,"custom":"#abc"}}}"##);
        assert_eq!(cfg.terminal.theme.get("background").unwrap(), "#123456");
        assert_eq!(cfg.terminal.theme.get("green").unwrap(), "#0dbc79");
        assert_eq!(cfg.terminal.theme.get("red").unwrap(), "#cd3131"); // non-string dropped, default kept
        assert_eq!(cfg.terminal.theme.get("custom").unwrap(), "#abc");
    }

    #[test]
    fn parse_agents_whitelist_excludes_sdk_and_unknown_ids() {
        let (cfg, _) = parse(
            r#"{"agents":{"claude-agent-sdk":{"bin":"/evil"},"nope":{"bin":"/x"},"codex":{"bin":"/ok"}}}"#,
        );
        assert!(!cfg.agents.contains_key("claude-agent-sdk"));
        assert!(!cfg.agents.contains_key("nope"));
        assert_eq!(cfg.agents.get("codex").unwrap().bin.as_deref(), Some("/ok"));
    }
}
