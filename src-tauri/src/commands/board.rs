use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::services::board::{self, BoardCardBase};
use crate::services::git;

#[derive(Debug, Clone, Serialize)]
pub struct BoardCard {
    #[serde(flatten)]
    pub base: BoardCardBase,
    pub is_clean: bool,
    pub changed_files: usize,
    pub health: String, // "clean" | "dirty" | "unknown"
}

#[tauri::command]
pub fn get_board(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<BoardCard>, String> {
    // Gather all DB-side data under one lock, then release before any git.
    let bases = {
        let conn = state.lock().map_err(|e| e.to_string())?;
        board::assemble_cards(&conn, &workspace_id).map_err(|e| e.to_string())?
    };
    let mut cards = Vec::with_capacity(bases.len());
    for (base, worktree_path) in bases {
        let h = git::worktree_health(Path::new(&worktree_path));
        let health = if h.error.is_some() {
            "unknown"
        } else if h.is_clean {
            "clean"
        } else {
            "dirty"
        };
        cards.push(BoardCard {
            base,
            is_clean: h.error.is_none() && h.is_clean,
            changed_files: h.changed_files,
            health: health.to_string(),
        });
    }
    Ok(cards)
}
