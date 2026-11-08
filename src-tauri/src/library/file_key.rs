//! Content-based file identity used by the analysis and peaks caches.
//!
//! `file_key` = size + xxh3 of the first and last 64 KB. The mtime is deliberately left out
//! so the key survives moves and copies (a copy may get a new mtime).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use xxhash_rust::xxh3::Xxh3;

const CHUNK: u64 = 64 * 1024;

pub fn file_key(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut hasher = Xxh3::new();
    let mut buf = vec![0u8; CHUNK as usize];

    let head = size.min(CHUNK) as usize;
    file.read_exact(&mut buf[..head])?;
    hasher.update(&buf[..head]);

    if size > CHUNK {
        // The tail never overlaps the head, so small files are hashed exactly once.
        let tail_start = (size - CHUNK).max(CHUNK);
        let tail = (size - tail_start) as usize;
        file.seek(SeekFrom::Start(tail_start))?;
        file.read_exact(&mut buf[..tail])?;
        hasher.update(&buf[..tail]);
    }

    Ok(format!("{size:x}-{:016x}", hasher.digest()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn same_content_same_key_regardless_of_name() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        write(&dir.path().join("a.mp3"), &data);
        write(&dir.path().join("b.mp3"), &data);
        assert_eq!(
            file_key(&dir.path().join("a.mp3")).unwrap(),
            file_key(&dir.path().join("b.mp3")).unwrap()
        );
    }

    #[test]
    fn head_tail_and_size_changes_change_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let base: Vec<u8> = vec![7; 300_000];
        let p = dir.path().join("x");
        write(&p, &base);
        let k0 = file_key(&p).unwrap();

        let mut head = base.clone();
        head[10] = 8;
        write(&p, &head);
        assert_ne!(file_key(&p).unwrap(), k0);

        let mut tail = base.clone();
        *tail.last_mut().unwrap() = 8;
        write(&p, &tail);
        assert_ne!(file_key(&p).unwrap(), k0);

        write(&p, &base[..299_999]);
        assert_ne!(file_key(&p).unwrap(), k0);
    }

    #[test]
    fn small_and_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        write(&p, b"");
        assert!(file_key(&p).unwrap().starts_with("0-"));
        write(&p, &[1; 100_000]); // between one and two chunks
        assert!(file_key(&p).unwrap().starts_with("186a0-"));
    }
}
