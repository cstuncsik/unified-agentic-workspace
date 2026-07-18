//! User configuration (`<app_data_dir>/config.json`): per-PTY-agent binary/args
//! and terminal theme/font-size. Every function here is pure or path-parameterized
//! (no `AppHandle`, no `$SHELL`/env reads) so it is unit-testable + parallel-safe;
//! the command boundary supplies the path/`AppHandle` and reads `UAW_AGENT_BIN`.

use std::collections::BTreeMap;

/// Config `agents.<id>` is honored ONLY for these PTY adapters. Any other id
/// (the SDK adapter `claude-agent-sdk`, or a typo) is dropped on parse — the SDK
/// adapter injects the API key, so its program/args must never come from config.
pub const PTY_AGENT_IDS: &[&str] = &["claude-code", "codex", "gemini"];
pub const DEFAULT_FONT_SIZE: u16 = 13;

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
}
