//! Source folder handling: listing audio files (walkdir), content keys, tag reading (lofty)
//! and syncing a session's track list with the folder. The file watcher (notify) comes in M5.

pub mod file_key;
pub mod scan;
pub mod tags;

#[cfg(test)]
pub mod testutil;
