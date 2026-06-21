//! Integration tests for the disk-loading boundary, [`SourceMap::add_file`].
//!
//! These exercise the real filesystem: a written file round-trips, a missing
//! path and a directory are defined I/O errors, non-UTF-8 bytes are rejected, and
//! the size ceiling stops an oversize file from being read.

#![cfg(feature = "std")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use source_lang::{SourceMap, SourceMapError};

/// A temporary directory unique to one test, removed when the guard drops.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        // Process id plus a monotone counter keeps directories distinct across
        // concurrently running tests without needing randomness.
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("source-lang-it-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("write temp file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_add_file_loads_contents_and_names_by_path() {
    let dir = TempDir::new();
    let path = dir.write("main.rs", b"fn main() {}\n");

    let mut map = SourceMap::new();
    let id = map.add_file(&path).expect("file loads");

    let file = map.source(id).expect("just added");
    assert_eq!(file.text(), "fn main() {}\n");
    assert_eq!(file.name(), path.to_string_lossy());
}

#[test]
fn test_add_file_missing_path_is_io_error() {
    let dir = TempDir::new();
    let path = dir.path().join("does-not-exist.rs");

    let mut map = SourceMap::new();
    let err = map.add_file(&path).unwrap_err();
    match err {
        SourceMapError::Io { kind, .. } => {
            assert_eq!(kind, std::io::ErrorKind::NotFound);
        }
        other => panic!("expected Io(NotFound), got {other:?}"),
    }
    assert!(map.is_empty());
}

#[test]
fn test_add_file_on_a_directory_is_io_error() {
    let dir = TempDir::new();

    let mut map = SourceMap::new();
    // Opening or reading a directory as a file fails; the exact kind is
    // platform-dependent, so only the variant is asserted.
    let err = map.add_file(dir.path()).unwrap_err();
    assert!(matches!(err, SourceMapError::Io { .. }), "got {err:?}");
    assert!(map.is_empty());
}

#[test]
fn test_add_file_non_utf8_is_rejected() {
    let dir = TempDir::new();
    // 0xFF is never valid in UTF-8.
    let path = dir.write("blob.bin", &[0x00, 0xff, 0x10]);

    let mut map = SourceMap::new();
    let err = map.add_file(&path).unwrap_err();
    assert!(matches!(err, SourceMapError::NotUtf8 { .. }), "got {err:?}");
    assert!(map.is_empty());
}

#[test]
fn test_add_file_over_the_size_ceiling_is_rejected_unread() {
    let dir = TempDir::new();
    let path = dir.write("big.txt", b"0123456789"); // ten bytes

    let mut map = SourceMap::new();
    map.set_max_source_len(4); // ceiling below the file size

    let err = map.add_file(&path).unwrap_err();
    match err {
        SourceMapError::Oversize { len, .. } => assert_eq!(len, 10),
        other => panic!("expected Oversize, got {other:?}"),
    }
    assert!(map.is_empty());
}

#[test]
fn test_add_file_at_the_size_ceiling_loads() {
    let dir = TempDir::new();
    let path = dir.write("exact.txt", b"abcd"); // exactly four bytes

    let mut map = SourceMap::new();
    map.set_max_source_len(4);

    let id = map.add_file(&path).expect("a file at the ceiling loads");
    assert_eq!(map.source(id).unwrap().text(), "abcd");
}
