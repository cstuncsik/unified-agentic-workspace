mod commands;
mod db;
mod models;
mod util;

use std::sync::Mutex;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // UAW_DB_PATH overrides the database location so e2e runs get a fresh,
            // isolated SQLite file instead of the shared per-user app data dir.
            let db_path = match std::env::var_os("UAW_DB_PATH") {
                Some(path) => std::path::PathBuf::from(path),
                None => {
                    let app_data_dir = app
                        .path()
                        .app_data_dir()
                        .expect("failed to resolve app data dir");
                    app_data_dir.join("uaw.sqlite")
                }
            };
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent).expect("failed to create database directory");
            }
            let conn = db::init_db(&db_path).expect("failed to initialize database");
            app.manage(Mutex::new(conn));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspaces::list_workspaces,
            commands::workspaces::get_workspace,
            commands::workspaces::create_workspace,
            commands::workspaces::update_workspace,
            commands::workspaces::delete_workspace,
            commands::projects::list_projects,
            commands::projects::get_project,
            commands::projects::create_project,
            commands::projects::update_project,
            commands::projects::delete_project,
            commands::sessions::list_sessions,
            commands::sessions::get_session,
            commands::sessions::create_session,
            commands::sessions::update_session,
            commands::sessions::update_session_status,
            commands::sessions::delete_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
