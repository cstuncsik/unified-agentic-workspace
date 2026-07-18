//! Command-boundary glue for user config: resolves the path from `AppHandle` and
//! reads it. The pure logic lives in `services::config`.

use std::collections::BTreeMap;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::services::config::{self, Config};

/// Resolve `<app_data_dir>/config.json` (or `UAW_CONFIG_PATH`) and read it.
pub fn load(app: &AppHandle) -> (Config, Option<String>) {
    // `app_data_dir()` effectively never fails on desktop; if it does, fall back to
    // defaults rather than let `config_path` resolve a bare, CWD-relative "config.json"
    // (which would break the "config is never repo/worktree-local" provenance guarantee).
    let Ok(dir) = app.path().app_data_dir() else {
        return (Config::default(), None);
    };
    let path = config::config_path(std::env::var_os("UAW_CONFIG_PATH"), &dir);
    config::read_config_at(&path)
}

#[derive(Serialize)]
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
