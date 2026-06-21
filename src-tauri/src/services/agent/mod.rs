//! Agent CLI adapters: descriptors of the interactive coding CLIs UAW can launch
//! in a PTY. The runtime is identical for each; an adapter just names the program,
//! its base args, and its capabilities. The program is overridable via
//! `UAW_AGENT_BIN` (used by tests to inject a fake interactive program).

pub mod pty;
pub mod sdk;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AgentCapabilities {
    pub streaming: bool,
    pub tool_use: bool,
    pub mcp: bool,
    pub file_edits: bool,
    pub shell_commands: bool,
    pub multi_turn: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentAdapter {
    pub id: &'static str,
    pub name: &'static str,
    pub program: &'static str,
    pub args: Vec<&'static str>,
    /// Provider these accounts must match (None = no API-key account binding).
    pub provider: Option<&'static str>,
    /// Env var the CLI reads its API key from (None = key injection unsupported).
    pub api_key_env: Option<&'static str>,
    /// Higher-precedence ambient vars to blank when injecting, so a stale ambient
    /// credential can't beat the chosen account's key.
    pub clear_env: Vec<&'static str>,
    /// "pty" (interactive terminal) | "sdk" (headless Node sidecar).
    pub kind: &'static str,
    /// SDK adapters require a bound account (no silent ambient identity).
    pub requires_account: bool,
    pub capabilities: AgentCapabilities,
}

fn full_capabilities() -> AgentCapabilities {
    AgentCapabilities {
        streaming: true,
        tool_use: true,
        mcp: true,
        file_edits: true,
        shell_commands: true,
        multi_turn: true,
    }
}

/// The built-in interactive CLI adapters.
pub fn adapters() -> Vec<AgentAdapter> {
    vec![
        AgentAdapter {
            id: "claude-code",
            name: "Claude Code",
            program: "claude",
            args: vec![],
            provider: Some("anthropic"),
            api_key_env: Some("ANTHROPIC_API_KEY"),
            clear_env: vec!["ANTHROPIC_AUTH_TOKEN"],
            kind: "pty",
            requires_account: false,
            capabilities: full_capabilities(),
        },
        AgentAdapter {
            id: "codex",
            name: "Codex",
            program: "codex",
            args: vec![],
            provider: Some("openai"),
            api_key_env: Some("OPENAI_API_KEY"),
            clear_env: vec![],
            kind: "pty",
            requires_account: false,
            capabilities: full_capabilities(),
        },
        AgentAdapter {
            id: "gemini",
            name: "Gemini",
            program: "gemini",
            args: vec![],
            provider: None,
            api_key_env: None,
            clear_env: vec![],
            kind: "pty",
            requires_account: false,
            capabilities: full_capabilities(),
        },
        AgentAdapter {
            id: "claude-agent-sdk",
            name: "Claude Agent SDK",
            program: "", // resolved at runtime via resolve_sdk_sidecar()
            args: vec![],
            provider: Some("anthropic"),
            api_key_env: Some("ANTHROPIC_API_KEY"),
            clear_env: vec!["ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"],
            kind: "sdk",
            requires_account: true,
            capabilities: full_capabilities(),
        },
    ]
}

pub fn find_adapter(id: &str) -> Option<AgentAdapter> {
    adapters().into_iter().find(|a| a.id == id)
}

/// The program to actually spawn for an adapter: `UAW_AGENT_BIN` overrides every
/// adapter (so e2e can substitute a fake interactive program); otherwise the
/// adapter's default program.
pub fn resolve_program(adapter: &AgentAdapter) -> String {
    match std::env::var("UAW_AGENT_BIN") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => adapter.program.to_string(),
    }
}

/// Resolve a sidecar script path: an env override (trimmed, non-empty) wins; else the
/// bundled relative path made ABSOLUTE against the backend cwd (the child is spawned
/// with cwd=worktree, so a relative program path would resolve there and fail).
fn resolve_sidecar_script(env_var: &str, rel: &str) -> String {
    if let Ok(v) = std::env::var(env_var) {
        if !v.trim().is_empty() {
            return v;
        }
    }
    std::env::current_dir()
        .map(|d| d.join(rel).to_string_lossy().into_owned())
        .unwrap_or_else(|_| rel.to_string())
}

/// The Node sidecar entry for the SDK agent (`UAW_AGENT_SDK_SIDECAR` overrides).
pub fn resolve_sdk_sidecar() -> String {
    resolve_sidecar_script("UAW_AGENT_SDK_SIDECAR", "sidecar/claude-agent-sdk/index.mjs")
}

/// The Node helper that lists a provider's models (`UAW_AGENT_SDK_MODELS` overrides).
pub fn resolve_sdk_models_sidecar() -> String {
    resolve_sidecar_script("UAW_AGENT_SDK_MODELS", "sidecar/claude-agent-sdk/list-models.mjs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_the_three_clis() {
        let ids: Vec<_> = adapters().iter().map(|a| a.id).collect();
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"gemini"));
        assert!(find_adapter("claude-code").is_some());
        assert!(find_adapter("nope").is_none());

        let claude = find_adapter("claude-code").unwrap();
        assert_eq!(claude.provider, Some("anthropic"));
        assert_eq!(claude.api_key_env, Some("ANTHROPIC_API_KEY"));
        assert_eq!(claude.clear_env, vec!["ANTHROPIC_AUTH_TOKEN"]);

        let codex = find_adapter("codex").unwrap();
        assert_eq!(codex.provider, Some("openai"));
        assert_eq!(codex.api_key_env, Some("OPENAI_API_KEY"));

        // Gemini has no creatable account in this slice -> no key binding.
        let gemini = find_adapter("gemini").unwrap();
        assert_eq!(gemini.provider, None);
        assert_eq!(gemini.api_key_env, None);

        let sdk = find_adapter("claude-agent-sdk").unwrap();
        assert_eq!(sdk.kind, "sdk");
        assert!(sdk.requires_account);
        assert_eq!(sdk.provider, Some("anthropic"));
        assert_eq!(claude.kind, "pty");
        assert!(!claude.requires_account);
    }

    #[test]
    fn resolve_sdk_sidecar_prefers_env() {
        std::env::remove_var("UAW_AGENT_SDK_SIDECAR");
        assert!(resolve_sdk_sidecar().ends_with("index.mjs"));
        std::env::set_var("UAW_AGENT_SDK_SIDECAR", "/tmp/fake-sdk");
        assert_eq!(resolve_sdk_sidecar(), "/tmp/fake-sdk");
        std::env::remove_var("UAW_AGENT_SDK_SIDECAR");
    }

    #[test]
    fn resolve_sdk_models_sidecar_prefers_env() {
        std::env::remove_var("UAW_AGENT_SDK_MODELS");
        assert!(resolve_sdk_models_sidecar().ends_with("list-models.mjs"));
        std::env::set_var("UAW_AGENT_SDK_MODELS", "/tmp/fake-models");
        assert_eq!(resolve_sdk_models_sidecar(), "/tmp/fake-models");
        std::env::remove_var("UAW_AGENT_SDK_MODELS");
    }

    #[test]
    fn resolve_program_prefers_env_override() {
        let claude = find_adapter("claude-code").unwrap();
        // Default (no override) — guard against a leaked env var from another test.
        std::env::remove_var("UAW_AGENT_BIN");
        assert_eq!(resolve_program(&claude), "claude");
        std::env::set_var("UAW_AGENT_BIN", "/tmp/fake-agent");
        assert_eq!(resolve_program(&claude), "/tmp/fake-agent");
        std::env::remove_var("UAW_AGENT_BIN");
    }
}
