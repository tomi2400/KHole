//! Tauri commands: the only way the UI reaches the file system, the DB or the sidecar.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::analysis::{self, SidecarInfo};
use crate::error::Result;
use crate::store::Store;

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
