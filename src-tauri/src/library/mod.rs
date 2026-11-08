//! Source folder handling: listing audio files (walkdir), content keys, tag reading (lofty)
//! and syncing a session's track list with the folder. The file watcher (notify) tells the UI when to rescan.

pub mod file_key;
pub mod scan;
pub mod tags;
pub mod watch;

#[cfg(test)]
pub mod testutil;
