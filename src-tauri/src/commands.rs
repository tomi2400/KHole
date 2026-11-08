//! Tauri commands: the only way the UI reaches the file system, the DB or the sidecar.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::analysis::{self, SidecarInfo};
use crate::audio::{self, PeakRequests};
use crate::error::{Error, Result};
use crate::fileops::{self, Decision, Outcome, Undone};
use crate::finder::{self, App};
use crate::library::scan::{self, ScanSummary};
use crate::library::watch::Watcher;
use crate::store::destinations::{self, Destination};
use crate::store::{self, Store, sessions, sessions::RecentSession, sessions::Session, tracks};

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    db_path: String,
    schema_version: i64,
}

#[tauri::command]
pub fn app_info(app: AppHandle, store: State<'_, Store>) -> Result<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        db_path: crate::db_path(&app)?.display().to_string(),
        schema_version: store.schema_version()?,
    })
}

#[tauri::command]
pub async fn analyzer_info(app: AppHandle) -> Result<SidecarInfo> {
    analysis::sidecar_info(&app).await
}

/// Native folder picker; `None` if the user cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, title: Option<String>) -> Result<Option<String>> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(title.as_deref().unwrap_or("Import Folder"))
            .blocking_pick_folder()
    })
    .await?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

/// Opens (or creates) the session of a folder picked or dropped by the user, and lets the
/// WebView stream audio from it. A dropped file opens its folder.
#[tauri::command]
pub fn open_session(app: AppHandle, store: State<'_, Store>, path: String) -> Result<Session> {
    let folder = source_folder(Path::new(&path))?;
    let folder_str = folder
        .to_str()
        .ok_or_else(|| Error::Invalid("The folder name is not valid UTF-8.".into()))?;
    allow_streaming(&app, &folder)?;
    let conn = store.conn()?;
    let session = sessions::open(&conn, folder_str, store::now())?;
    // Moved tracks play from their destination (↑ back to one already sorted).
    for dest in destinations::paths(&conn, session.id)? {
        let _ = allow_streaming(&app, Path::new(&dest));
    }
    Ok(session)
}

/// Lets the WebView stream audio from `folder` (asset protocol scope).
fn allow_streaming(app: &AppHandle, folder: &Path) -> Result<()> {
    app.asset_protocol_scope()
        .allow_directory(folder, true)
        .map_err(|e| Error::Invalid(e.to_string()))
}

fn source_folder(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .map_err(|_| Error::NotFound(path.display().to_string()))?;
    if path.is_dir() {
        return Ok(path);
    }
    match path.parent() {
        Some(parent) if scan::is_audio(&path) => Ok(parent.to_path_buf()),
        _ => Err(Error::Invalid(
            "Drop a folder (or a track inside one).".into(),
        )),
    }
}

#[tauri::command]
pub fn recent_sessions(store: State<'_, Store>) -> Result<Vec<RecentSession>> {
    sessions::recent(&*store.conn()?, 5)
}

#[tauri::command]
pub fn set_include_subfolders(
    store: State<'_, Store>,
    session_id: i64,
    value: bool,
) -> Result<Session> {
    sessions::set_include_sub(&*store.conn()?, session_id, value, store::now())
}

#[tauri::command]
pub fn list_tracks(store: State<'_, Store>, session_id: i64) -> Result<Vec<tracks::Track>> {
    tracks::list(&*store.conn()?, session_id)
}

/// Sessions with a scan in flight; a second scan of the same session is refused.
#[derive(Default)]
pub struct Scans(Mutex<HashSet<i64>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgress {
    session_id: i64,
    done: usize,
    total: usize,
}

const PROGRESS_EVERY: Duration = Duration::from_millis(50);

/// Syncs the session's tracks with its folder on a blocking thread, emitting throttled
/// `scan_progress` events. Resolves when the scan is finished.
#[tauri::command]
pub async fn scan_session(app: AppHandle, session_id: i64) -> Result<ScanSummary> {
    let (session, exclude) = {
        let store = app.state::<Store>();
        let conn = store.conn()?;
        let exclude: Vec<PathBuf> = destinations::paths(&conn, session_id)?
            .into_iter()
            .map(PathBuf::from)
            .collect();
        (sessions::get(&conn, session_id)?, exclude)
    };
    let root = PathBuf::from(&session.source_path);
    if !root.is_dir() {
        return Err(Error::NotFound(session.source_path));
    }

    let scans = app.state::<Scans>();
    if !scans
        .0
        .lock()
        .map_err(|_| Error::StorePoisoned)?
        .insert(session_id)
    {
        return Err(Error::ScanBusy);
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut last = Instant::now() - PROGRESS_EVERY;
        scan::sync_session(
            &worker.state::<Store>(),
            session_id,
            &root,
            session.include_sub,
            &exclude,
            |done, total| {
                if done == total || last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    let _ = worker.emit(
                        "scan_progress",
                        ScanProgress {
                            session_id,
                            done,
                            total,
                        },
                    );
                }
            },
        )
    })
    .await;
    if let Ok(mut s) = scans.0.lock() {
        s.remove(&session_id);
    }
    result?
}

#[tauri::command]
pub fn set_start_at(store: State<'_, Store>, session_id: i64, value: f64) -> Result<Session> {
    sessions::set_start_at(&*store.conn()?, session_id, value, store::now())
}

#[tauri::command]
pub fn set_current_track(
    store: State<'_, Store>,
    session_id: i64,
    track_id: Option<i64>,
) -> Result<()> {
    sessions::set_current_track(&*store.conn()?, session_id, track_id)
}

/// Waveform peaks of a track as raw bytes (see `Peaks::to_bytes`). A request superseded by a
/// newer one fails with "cancelled".
#[tauri::command]
pub async fn track_peaks(app: AppHandle, track_id: i64) -> Result<Response> {
    let path = PathBuf::from(tracks::path(&*app.state::<Store>().conn()?, track_id)?);
    let request = app.state::<PeakRequests>().begin();
    let peaks = tauri::async_runtime::spawn_blocking(move || {
        let requests = app.state::<PeakRequests>();
        audio::peaks_for(&app.state::<Store>(), &path, || {
            !requests.is_current(request)
        })
    })
    .await??;
    Ok(Response::new(peaks.to_bytes()))
}

#[tauri::command]
pub fn set_mode(store: State<'_, Store>, session_id: i64, mode: String) -> Result<Session> {
    sessions::set_mode(&*store.conn()?, session_id, &mode, store::now())
}

#[tauri::command]
pub fn set_auto_next(store: State<'_, Store>, session_id: i64, value: bool) -> Result<Session> {
    sessions::set_auto_next(&*store.conn()?, session_id, value, store::now())
}

#[tauri::command]
pub fn list_destinations(store: State<'_, Store>, session_id: i64) -> Result<Vec<Destination>> {
    destinations::list(&*store.conn()?, session_id)
}

/// Adds a destination folder (it gets the first free number key) and returns the new list.
#[tauri::command]
pub fn add_destination(
    app: AppHandle,
    store: State<'_, Store>,
    session_id: i64,
    path: String,
) -> Result<Vec<Destination>> {
    let folder = Path::new(&path)
        .canonicalize()
        .map_err(|_| Error::NotFound(path.clone()))?;
    if !folder.is_dir() {
        return Err(Error::Invalid("A destination must be a folder.".into()));
    }
    let folder_str = folder
        .to_str()
        .ok_or_else(|| Error::Invalid("The folder name is not valid UTF-8.".into()))?;
    let conn = store.conn()?;
    let session = sessions::get(&conn, session_id)?;
    if folder == Path::new(&session.source_path) {
        return Err(Error::Invalid(
            "The source folder cannot be its own destination.".into(),
        ));
    }
    destinations::add(&conn, session_id, folder_str)?;
    allow_streaming(&app, &folder)?;
    destinations::list(&conn, session_id)
}

#[tauri::command]
pub fn remove_destination(
    store: State<'_, Store>,
    session_id: i64,
    destination_id: i64,
) -> Result<Vec<Destination>> {
    let conn = store.conn()?;
    destinations::remove(&conn, session_id, destination_id)?;
    destinations::list(&conn, session_id)
}

/// Sets (or clears) a destination's number key; a key held by another destination swaps.
#[tauri::command]
pub fn set_destination_hotkey(
    store: State<'_, Store>,
    session_id: i64,
    destination_id: i64,
    hotkey: Option<i64>,
) -> Result<Vec<Destination>> {
    let mut conn = store.conn()?;
    destinations::set_hotkey(&mut conn, session_id, destination_id, hotkey)?;
    destinations::list(&conn, session_id)
}

/// Keep / trash / move or copy a track. Runs on a blocking thread: a copy to another
/// drive can take a while.
#[tauri::command]
pub async fn decide(
    app: AppHandle,
    session_id: i64,
    track_id: i64,
    decision: Decision,
) -> Result<Outcome> {
    tauri::async_runtime::spawn_blocking(move || {
        fileops::decide(&app.state::<Store>(), session_id, track_id, decision)
    })
    .await?
}

/// Reverts the newest action of the session; `None` if there is nothing to undo.
#[tauri::command]
pub async fn undo(app: AppHandle, session_id: i64) -> Result<Option<Undone>> {
    tauri::async_runtime::spawn_blocking(move || {
        fileops::undo_last(&app.state::<Store>(), session_id)
    })
    .await?
}

/// Watches the session's source folder; a change made outside the app emits
/// `folder_changed` (the UI rescans). Replaces the watch of another session.
#[tauri::command]
pub fn watch_session(
    app: AppHandle,
    store: State<'_, Store>,
    watcher: State<'_, Watcher>,
    session_id: i64,
) -> Result<()> {
    let session = sessions::get(&*store.conn()?, session_id)?;
    watcher.start(&app, session_id, PathBuf::from(session.source_path))
}

#[tauri::command]
pub fn unwatch_session(watcher: State<'_, Watcher>) {
    watcher.stop();
}

fn track_path(store: &Store, session_id: i64, track_id: i64) -> Result<PathBuf> {
    Ok(PathBuf::from(
        tracks::get(&*store.conn()?, session_id, track_id)?.path,
    ))
}

/// Shows the track's file in Finder (⌘R).
#[tauri::command]
pub fn reveal_track(store: State<'_, Store>, session_id: i64, track_id: i64) -> Result<()> {
    finder::reveal(&track_path(&store, session_id, track_id)?)
}

/// The apps that can open the track's file, the default one first.
#[tauri::command]
pub async fn track_apps(app: AppHandle, session_id: i64, track_id: i64) -> Result<Vec<App>> {
    let path = track_path(&app.state::<Store>(), session_id, track_id)?;
    tauri::async_runtime::spawn_blocking(move || finder::apps_for(&path)).await?
}

/// Opens the track's file in `app_path` (one of `track_apps`), or in its default app.
#[tauri::command]
pub async fn open_track(
    app: AppHandle,
    session_id: i64,
    track_id: i64,
    app_path: Option<String>,
) -> Result<()> {
    let path = track_path(&app.state::<Store>(), session_id, track_id)?;
    tauri::async_runtime::spawn_blocking(move || finder::open_with(&path, app_path.as_deref()))
        .await?
}
