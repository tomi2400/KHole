//! File-system primitives that never overwrite and never delete.
//!
//! - A name conflict gets a counter: `track.mp3` → `track (2).mp3`, `track (3).mp3`, …
//! - Moves use `renamex_np(…, RENAME_EXCL)`, so the "is the name free?" check and the rename
//!   are one atomic step. Across volumes a move is a copy followed by removing the original,
//!   only after the copy is complete and has the original's size.
//! - Copies first reserve the target name with `O_CREAT | O_EXCL`.
//! - Deleting means the macOS Trash (`NSFileManager.trashItemAtURL:resultingItemURL:`),
//!   which also tells us where the file went, so it can be put back.

use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Where a file ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub path: PathBuf,
    /// The wanted name was taken, so it got a counter.
    pub renamed: bool,
}

/// Gives up after this many taken names (a folder full of `x (n).mp3` is a bug, not a use).
const MAX_CANDIDATES: usize = 10_000;

/// `name.ext`, `name (2).ext`, `name (3).ext`, … inside `dir`.
fn candidates<'a>(dir: &'a Path, file_name: &'a OsStr) -> impl Iterator<Item = PathBuf> + 'a {
    let name = Path::new(file_name);
    let stem = name.file_stem().unwrap_or(file_name);
    let ext = name.extension();
    (1..=MAX_CANDIDATES).map(move |n| {
        if n == 1 {
            return dir.join(file_name);
        }
        let mut s = OsString::from(stem);
        s.push(format!(" ({n})"));
        if let Some(ext) = ext {
            s.push(".");
            s.push(ext);
        }
        dir.join(s)
    })
}

fn c_path(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes()).map_err(|e| io::Error::new(ErrorKind::InvalidInput, e))
}

/// `rename` that fails with `AlreadyExists` instead of replacing the target.
fn rename_excl(from: &Path, to: &Path) -> io::Result<()> {
    let (from_c, to_c) = (c_path(from)?, c_path(to)?);
    // SAFETY: both are valid NUL-terminated strings that outlive the call.
    let rc = unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) };
    if rc == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        // Volumes without RENAME_EXCL (some network and FAT/exFAT drives): check, then
        // rename. Not atomic, but nothing else writes these names while we triage.
        Some(libc::ENOTSUP) | Some(libc::EINVAL) => {
            if to.symlink_metadata().is_ok() {
                Err(ErrorKind::AlreadyExists.into())
            } else {
                fs::rename(from, to)
            }
        }
        _ => Err(err),
    }
}

/// Moves `src` into `dir` as `file_name` (or the first free `file_name (n)`).
pub fn move_to(src: &Path, dir: &Path, file_name: &OsStr) -> Result<Placed> {
    for (i, target) in candidates(dir, file_name).enumerate() {
        match rename_excl(src, &target) {
            Ok(()) => {
                return Ok(Placed {
                    path: target,
                    renamed: i > 0,
                });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                return move_across_volumes(src, dir, file_name);
            }
            Err(e) => return Err(io_error(e, src)),
        }
    }
    Err(no_free_name(dir, file_name))
}

/// Moves `src` into `dir`, keeping its name if free.
pub fn move_into(src: &Path, dir: &Path) -> Result<Placed> {
    move_to(src, dir, file_name(src)?)
}

fn move_across_volumes(src: &Path, dir: &Path, file_name: &OsStr) -> Result<Placed> {
    let placed = copy_to(src, dir, file_name)?;
    // The copy is complete and verified; only now does the original go. This is the move
    // itself, not a delete: the file exists, at `placed.path`.
    fs::remove_file(src).map_err(|e| io_error(e, src))?;
    Ok(placed)
}

/// Copies `src` into `dir`, keeping its name if free.
pub fn copy_into(src: &Path, dir: &Path) -> Result<Placed> {
    copy_to(src, dir, file_name(src)?)
}

fn copy_to(src: &Path, dir: &Path, file_name: &OsStr) -> Result<Placed> {
    let expected = fs::metadata(src).map_err(|e| io_error(e, src))?.len();
    for (i, target) in candidates(dir, file_name).enumerate() {
        // Reserve the name atomically, then copy over our own empty placeholder.
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(f) => drop(f),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(e, &target)),
        }
        let copied = fs::copy(src, &target).and_then(|n| {
            if n == expected && fs::metadata(&target)?.len() == expected {
                Ok(())
            } else {
                Err(io::Error::other("the copy is incomplete"))
            }
        });
        return match copied {
            Ok(()) => Ok(Placed {
                path: target,
                renamed: i > 0,
            }),
            Err(e) => {
                // Our own partial copy (the name was free a moment ago), not a user file.
                let _ = fs::remove_file(&target);
                Err(io_error(e, src))
            }
        };
    }
    Err(no_free_name(dir, file_name))
}

/// Moves `path` to the Trash and returns where it is now.
pub fn trash(path: &Path) -> Result<PathBuf> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let utf8 = path
        .to_str()
        .ok_or_else(|| Error::Invalid(format!("not a UTF-8 path: {}", path.display())))?;
    if path.symlink_metadata().is_err() {
        return Err(Error::NotFound(utf8.to_owned()));
    }
    let url = NSURL::fileURLWithPath(&NSString::from_str(utf8));
    let mut resulting: Option<Retained<NSURL>> = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|e| Error::Invalid(format!("cannot move to Trash: {e}")))?;
    resulting
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string()))
        .ok_or_else(|| Error::Invalid("the Trash did not say where the file went".into()))
}

fn file_name(p: &Path) -> Result<&OsStr> {
    p.file_name()
        .ok_or_else(|| Error::Invalid(format!("not a file: {}", p.display())))
}

fn io_error(e: io::Error, path: &Path) -> Error {
    if e.kind() == ErrorKind::NotFound {
        Error::NotFound(path.display().to_string())
    } else {
        Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
    }
}

fn no_free_name(dir: &Path, file_name: &OsStr) -> Error {
    Error::Invalid(format!(
        "no free name for {} in {}",
        file_name.to_string_lossy(),
        dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, body: &str) {
        fs::write(p, body).unwrap();
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    #[test]
    fn candidate_names() {
        let names: Vec<_> = candidates(Path::new("/d"), OsStr::new("a b.tar.mp3"))
            .take(3)
            .collect();
        assert_eq!(
            names,
            [
                PathBuf::from("/d/a b.tar.mp3"),
                PathBuf::from("/d/a b.tar (2).mp3"),
                PathBuf::from("/d/a b.tar (3).mp3"),
            ]
        );
        let no_ext: Vec<_> = candidates(Path::new("/d"), OsStr::new("x"))
            .take(2)
            .collect();
        assert_eq!(no_ext[1], PathBuf::from("/d/x (2)"));
    }

    #[test]
    fn move_never_overwrites() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let a = src.path().join("t.wav");
        write(&a, "new");
        write(&dst.path().join("t.wav"), "old");
        write(&dst.path().join("t (2).wav"), "older");

        let placed = move_into(&a, dst.path()).unwrap();
        assert_eq!(placed.path, dst.path().join("t (3).wav"));
        assert!(placed.renamed);
        assert!(!a.exists());
        assert_eq!(read(&placed.path), "new");
        assert_eq!(read(&dst.path().join("t.wav")), "old");
        assert_eq!(read(&dst.path().join("t (2).wav")), "older");
    }

    #[test]
    fn move_keeps_the_name_when_free() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let a = src.path().join("Ünnep – mix.flac");
        write(&a, "x");
        let placed = move_into(&a, dst.path()).unwrap();
        assert_eq!(placed.path, dst.path().join("Ünnep – mix.flac"));
        assert!(!placed.renamed);
    }

    #[test]
    fn move_of_a_missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = move_into(&dir.path().join("gone.mp3"), dir.path()).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn copy_never_overwrites_and_keeps_the_original() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let a = src.path().join("t.aiff");
        write(&a, "new");
        write(&dst.path().join("t.aiff"), "old");

        let placed = copy_into(&a, dst.path()).unwrap();
        assert_eq!(placed.path, dst.path().join("t (2).aiff"));
        assert!(placed.renamed);
        assert_eq!(read(&a), "new");
        assert_eq!(read(&placed.path), "new");
        assert_eq!(read(&dst.path().join("t.aiff")), "old");
    }

    #[test]
    fn failed_copy_leaves_no_placeholder() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        // A directory "file" cannot be copied with fs::copy.
        let d = src.path().join("dir.wav");
        fs::create_dir(&d).unwrap();
        assert!(copy_into(&d, dst.path()).is_err());
        assert_eq!(fs::read_dir(dst.path()).unwrap().count(), 0);
    }

    /// Touches the real Trash (and puts the file back), so it is opt-in:
    /// `cargo test trash_and_restore -- --ignored`.
    #[test]
    #[ignore]
    fn trash_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("khole trash test ű.txt");
        write(&a, "hello");
        let in_trash = trash(&a).unwrap();
        assert!(!a.exists());
        assert!(in_trash.exists(), "{}", in_trash.display());
        let back = move_to(&in_trash, dir.path(), a.file_name().unwrap()).unwrap();
        assert_eq!(back.path, a);
        assert_eq!(read(&a), "hello");
        assert!(matches!(
            trash(&dir.path().join("nope")),
            Err(Error::NotFound(_))
        ));
    }
}
