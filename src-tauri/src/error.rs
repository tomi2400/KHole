use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("tauri: {0}")]
    Tauri(#[from] tauri::Error),
    #[error("analyzer sidecar: {0}")]
    Sidecar(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("cannot decode {0}")]
    Decode(String),
    /// A newer request superseded this one; the UI ignores it.
    #[error("cancelled")]
    Cancelled,
    #[error("a scan of this folder is already running")]
    ScanBusy,
    #[error("store is unavailable (lock poisoned)")]
    StorePoisoned,
}

pub type Result<T> = std::result::Result<T, Error>;

/// Commands return errors to the UI as plain strings.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
