mod analysis;
mod audio;
mod commands;
mod error;
mod fileops;
mod finder;
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
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::Scans::default())
        .manage(audio::PeakRequests::default())
        .manage(library::watch::Watcher::default())
        .setup(|app| {
            let store = store::Store::open(&db_path(app.handle())?)?;
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::app_info,
            commands::analyzer_info,
            commands::pick_folder,
            commands::open_session,
            commands::recent_sessions,
            commands::set_include_subfolders,
            commands::list_tracks,
            commands::scan_session,
            commands::track_peaks,
            commands::set_start_at,
            commands::set_current_track,
            commands::set_mode,
            commands::set_auto_next,
            commands::list_destinations,
            commands::add_destination,
            commands::remove_destination,
            commands::set_destination_hotkey,
            commands::decide,
            commands::undo,
            commands::watch_session,
            commands::unwatch_session,
            commands::reveal_track,
            commands::track_apps,
            commands::open_track,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
