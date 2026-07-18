//! Agent CLI adapters: descriptors of the interactive coding CLIs UAW can launch
//! in a PTY. The runtime is identical for each; an adapter just names the program,
//! its base args, and its capabilities. The program is overridable via
//! `UAW_AGENT_BIN` (used by tests to inject a fake interactive program).

pub mod pty;
pub mod sdk;

use serde::Serialize;
use std::path::Path;

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
        // Ambient-login PTY agent. NO provider/api_key_env/clear_env: interactive `claude`
        // ignores an injected ANTHROPIC_API_KEY (cli.js's customApiKeyResponses approval
        // gate) and runs as the user's own `claude login`. Injecting a per-account key was
        // misleading and spilled a live key into a process that ignores it. Per-account
        // binding lives on the SDK adapter (`claude-agent-sdk`).
        AgentAdapter {
            id: "claude-code",
            name: "Claude Code",
            program: "claude",
            args: vec![],
            provider: None,
            api_key_env: None,
            clear_env: vec![],
            kind: "pty",
            requires_account: false,
            capabilities: full_capabilities(),
        },
        // Ambient-login PTY agent (see claude-code). Interactive `codex` ignores an
        // injected OPENAI_API_KEY and uses its stored login (auth_mode: chatgpt).
        AgentAdapter {
            id: "codex",
            name: "Codex",
            program: "codex",
            args: vec![],
            provider: None,
            api_key_env: None,
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

/// Resolve a sidecar script path. Precedence: an env override (trimmed, non-empty) wins;
/// then in DEV the repo sidecar via cwd (its node_modules is the working-tree install);
/// in RELEASE the bundled resource ONLY — never cwd (the agent-writable worktree, a
/// script-hijack/key-exfil vector). A missing release resource -> a non-existent path ->
/// spawn fails closed (the post-build assertion guarantees a correctly-bundled app has it).
fn resolve_sidecar_script(env_var: &str, rel: &str, resource_dir: Option<&Path>, dev: bool) -> String {
    if let Ok(v) = std::env::var(env_var) {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if dev {
        return std::env::current_dir()
            .map(|d| d.join(rel).to_string_lossy().into_owned())
            .unwrap_or_else(|_| rel.to_string());
    }
    resource_dir
        .map(|d| d.join(rel).to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("/nonexistent/uaw-bundled-sidecar/{rel}"))
}

/// The Node sidecar entry for the SDK agent (`UAW_AGENT_SDK_SIDECAR` overrides).
pub fn resolve_sdk_sidecar(resource_dir: Option<&Path>) -> String {
    resolve_sidecar_script(
        "UAW_AGENT_SDK_SIDECAR",
        "sidecar/claude-agent-sdk/index.mjs",
        resource_dir,
        cfg!(debug_assertions),
    )
}

/// The Node helper that lists a provider's models (`UAW_AGENT_SDK_MODELS` overrides).
pub fn resolve_sdk_models_sidecar(resource_dir: Option<&Path>) -> String {
    resolve_sidecar_script(
        "UAW_AGENT_SDK_MODELS",
        "sidecar/claude-agent-sdk/list-models.mjs",
        resource_dir,
        cfg!(debug_assertions),
    )
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

        // PTY agents are ambient-login: no provider/api_key_env/clear_env (uniform with
        // gemini). Interactive claude/codex ignore an injected key and use the user's login.
        let claude = find_adapter("claude-code").unwrap();
        assert_eq!(claude.provider, None);
        assert_eq!(claude.api_key_env, None);
        assert!(claude.clear_env.is_empty());

        let codex = find_adapter("codex").unwrap();
        assert_eq!(codex.provider, None);
        assert_eq!(codex.api_key_env, None);
        assert!(codex.clear_env.is_empty());

        let gemini = find_adapter("gemini").unwrap();
        assert_eq!(gemini.provider, None);
        assert_eq!(gemini.api_key_env, None);

        // The SDK adapter is the lone account-bearing adapter (the per-account path).
        let sdk = find_adapter("claude-agent-sdk").unwrap();
        assert_eq!(sdk.kind, "sdk");
        assert!(sdk.requires_account);
        assert_eq!(sdk.provider, Some("anthropic"));
        assert_eq!(sdk.api_key_env, Some("ANTHROPIC_API_KEY"));
        assert_eq!(claude.kind, "pty");
        assert!(!claude.requires_account);
    }

    #[test]
    fn pty_adapter_ids_match_the_config_allowlist() {
        // Guards a future PTY adapter being silently un-configurable: config.rs's
        // PTY_AGENT_IDS allowlist must stay in sync with the registry's "pty" adapters.
        let mut pty_ids: Vec<_> =
            adapters().iter().filter(|a| a.kind == "pty").map(|a| a.id).collect();
        let mut allowlist: Vec<_> = crate::services::config::PTY_AGENT_IDS.to_vec();
        pty_ids.sort_unstable();
        allowlist.sort_unstable();
        assert_eq!(pty_ids, allowlist);
    }

    #[test]
    fn resolve_sdk_sidecar_prefers_env() {
        std::env::remove_var("UAW_AGENT_SDK_SIDECAR");
        assert!(resolve_sdk_sidecar(None).ends_with("index.mjs"));
        std::env::set_var("UAW_AGENT_SDK_SIDECAR", "/tmp/fake-sdk");
        assert_eq!(resolve_sdk_sidecar(None), "/tmp/fake-sdk");
        std::env::remove_var("UAW_AGENT_SDK_SIDECAR");
    }

    #[test]
    fn resolve_sdk_models_sidecar_prefers_env() {
        std::env::remove_var("UAW_AGENT_SDK_MODELS");
        assert!(resolve_sdk_models_sidecar(None).ends_with("list-models.mjs"));
        std::env::set_var("UAW_AGENT_SDK_MODELS", "/tmp/fake-models");
        let resolved = resolve_sdk_models_sidecar(None);
        std::env::remove_var("UAW_AGENT_SDK_MODELS");
        assert_eq!(resolved, "/tmp/fake-models");
    }

    #[test]
    fn resolve_sidecar_script_precedence() {
        use std::fs;
        let rel = "sidecar/claude-agent-sdk/index.mjs";
        let env_var = "UAW_TEST_SIDECAR_PREC"; // unique name -> no shared-var race
        std::env::remove_var(env_var);

        // A resource dir WITH the script present.
        let res = std::env::temp_dir().join(format!("uaw-res-{}", crate::util::new_id()));
        fs::create_dir_all(res.join("sidecar/claude-agent-sdk")).unwrap();
        fs::write(res.join(rel), b"").unwrap();
        let res_str = res.to_string_lossy().into_owned();

        // release + Some(dir) with the file -> the resource path (FULL path, contains the dir).
        let r = resolve_sidecar_script(env_var, rel, Some(&res), false);
        assert!(r.contains(&res_str), "release should use the resource dir: {r}");
        assert!(r.ends_with("index.mjs"));

        // release + None -> a non-cwd sentinel (fail closed, never the worktree cwd).
        let r = resolve_sidecar_script(env_var, rel, None, false);
        assert!(!r.contains(&res_str));
        assert!(r.starts_with("/nonexistent/"), "release+no-resource must fail closed: {r}");

        // dev -> cwd, ignores the resource dir.
        let cwd = std::env::current_dir().unwrap().to_string_lossy().into_owned();
        let r = resolve_sidecar_script(env_var, rel, Some(&res), true);
        assert!(!r.contains(&res_str), "dev must use cwd, not the resource dir: {r}");
        assert!(r.starts_with(&cwd), "dev must use cwd: {r}");
        assert!(r.ends_with("index.mjs"));

        // env override wins over a present resource, in both modes.
        std::env::set_var(env_var, "/tmp/override.mjs");
        assert_eq!(resolve_sidecar_script(env_var, rel, Some(&res), false), "/tmp/override.mjs");
        assert_eq!(resolve_sidecar_script(env_var, rel, Some(&res), true), "/tmp/override.mjs");
        std::env::remove_var(env_var);

        let _ = fs::remove_dir_all(&res);
    }
}
