//! Command-boundary glue for user config: resolves the path from `AppHandle` and
//! reads it. The pure logic lives in `services::config`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::services::config::{self, Config};

/// The config path, or None if `app_data_dir()` can't resolve (never CWD-relative).
fn config_file_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    Some(config::config_path(std::env::var_os("UAW_CONFIG_PATH"), &dir))
}

pub fn load(app: &AppHandle) -> (Config, Option<String>) {
    match config_file_path(app) {
        Some(path) => config::read_config_at(&path),
        None => (Config::default(), None),
    }
}

#[derive(Serialize)]
// Intentional camelCase: matches xterm's `Terminal({ fontSize, theme })` option names — the
// only camelCase serde boundary in src-tauri. Don't "fix" this to snake_case.
#[serde(rename_all = "camelCase")]
pub struct TerminalOut {
    font_size: u16,
    theme: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub struct AppConfigOut {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigForEdit {
    agents: BTreeMap<String, config::AgentEdit>,
    font_size: u16,
    warning: Option<String>,
}

/// Editable config for the Settings form (per-agent bin/args + fontSize) + a
/// parse warning so the form can flag an unparseable file up front.
#[tauri::command]
pub fn get_config_for_edit(app: AppHandle) -> ConfigForEdit {
    let (cfg, warning) = load(&app);
    let (agents, font_size) = config::edit_view(&cfg);
    ConfigForEdit { agents, font_size, warning }
}

/// Merge the edited fields into config.json (preserving theme/unknown keys) and
/// return the merged terminal config for live-apply. Err (validation or fs)
/// rejects the invoke; the frontend catches it.
#[tauri::command]
pub fn save_config(app: AppHandle, edits: config::EditConfig) -> Result<TerminalOut, String> {
    let path = config_file_path(&app)
        .ok_or_else(|| "could not resolve the config directory".to_string())?;
    let terminal = config::save_at(&path, &edits)?;
    Ok(TerminalOut { font_size: terminal.font_size, theme: terminal.theme })
}
