mod analysis;
mod audio;
mod commands;
mod error;
mod fileops;
mod library;
mod store;

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// `~/Library/Application Support/<identifier>/khole.db`
pub(crate) fn db_path(app: &AppHandle) -> error::Result<PathBuf> {
    Ok(app.path().app_data_dir()?.join("khole.db"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let store = store::Store::open(&db_path(app.handle())?)?;
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::app_info,
            commands::analyzer_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
