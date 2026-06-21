//! The error type returned when a source cannot be added to a map.

use alloc::boxed::Box;
use core::fmt;

/// The reason a source could not be added to a [`SourceMap`](crate::SourceMap).
///
/// Adding a source can fail four ways, each a distinct, defined outcome rather
/// than a panic or a silent corruption of the coordinate bookkeeping:
///
/// - the source is larger than the map's per-source ceiling
///   ([`Oversize`](Self::Oversize)),
/// - it does not fit in what remains of the shared 32-bit position space
///   ([`SpaceExhausted`](Self::SpaceExhausted)),
/// - its bytes are not valid UTF-8 ([`NotUtf8`](Self::NotUtf8)), or
/// - the file behind a path could not be read ([`Io`](Self::Io), `std` only).
///
/// Every variant names the source it concerns so the failure is actionable when
/// it is logged far from the call that produced it.
///
/// The enum is `#[non_exhaustive]`: a downstream `match` must include a wildcard
/// arm, so later additions never force a breaking change on callers.
///
/// # Examples
///
/// ```
/// use source_lang::{SourceMap, SourceMapError};
///
/// let mut map = SourceMap::new();
/// // Raw bytes that are not valid UTF-8 are rejected, naming the source.
/// let err = map.add_bytes("blob.bin", &[0xff, 0xfe]).unwrap_err();
/// assert!(matches!(err, SourceMapError::NotUtf8 { .. }));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceMapError {
    /// The source is larger than the map's configured per-source ceiling.
    ///
    /// The ceiling defaults to `u32::MAX` — the addressing limit of the global
    /// position space — and can be lowered with
    /// [`SourceMap::set_max_source_len`](crate::SourceMap::set_max_source_len) to
    /// bound how much a single untrusted input may load. For a file, the size is
    /// checked against the path's metadata *before* the bytes are read, so an
    /// oversize file is never pulled into memory.
    Oversize {
        /// Display name of the source that was rejected.
        name: Box<str>,
        /// Byte length of the source.
        len: u64,
    },

    /// The source did not fit in what remained of the map's global space.
    ///
    /// Returned when the source is no larger than the per-source ceiling but
    /// still exceeds the bytes left in the shared 32-bit position space — because
    /// earlier sources have consumed the remainder — or when the map already
    /// holds the maximum number of sources. The map is left unchanged, so the
    /// caller may start a fresh map or split the input.
    SpaceExhausted {
        /// Byte length of the source that was rejected.
        needed: u64,
        /// Bytes of global position space that remained available.
        available: u64,
    },

    /// The source's bytes are not valid UTF-8.
    ///
    /// A `SourceMap` stores text, so input from
    /// [`add_bytes`](crate::SourceMap::add_bytes) or a file is validated before
    /// it is stored. A truncated multi-byte sequence or stray binary byte is
    /// reported here rather than stored as corrupt text.
    NotUtf8 {
        /// Display name of the source whose bytes failed validation.
        name: Box<str>,
    },

    /// A file's contents could not be read from disk.
    ///
    /// Returned by [`add_file`](crate::SourceMap::add_file) when opening or
    /// reading the path fails — a missing file, a directory, or a permission
    /// error. The [`std::io::ErrorKind`] distinguishes the cause without carrying
    /// a non-comparable [`std::io::Error`], so this variant stays `Clone` and
    /// `Eq` like the rest.
    #[cfg(feature = "std")]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    Io {
        /// Display name of the source (the path that was requested).
        name: Box<str>,
        /// The category of I/O failure.
        kind: std::io::ErrorKind,
    },
}

impl fmt::Display for SourceMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversize { name, len } => write!(
                f,
                "source `{name}` of {len} bytes exceeds the maximum source length",
            ),
            Self::SpaceExhausted { needed, available } => write!(
                f,
                "source of {needed} bytes does not fit in the {available} bytes \
                 remaining in the global position space",
            ),
            Self::NotUtf8 { name } => {
                write!(f, "source `{name}` is not valid UTF-8")
            }
            #[cfg(feature = "std")]
            Self::Io { name, kind } => write!(f, "source `{name}` could not be read: {kind}"),
        }
    }
}

impl core::error::Error for SourceMapError {}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::boxed::Box;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn test_space_exhausted_display_names_both_figures() {
        let err = SourceMapError::SpaceExhausted {
            needed: 10,
            available: 4,
        };
        let text = err.to_string();
        assert!(text.contains("10 bytes"), "{text}");
        assert!(text.contains("4 bytes"), "{text}");
    }

    #[test]
    fn test_oversize_display_names_source_and_length() {
        let err = SourceMapError::Oversize {
            name: Box::from("big.rs"),
            len: 5_000_000_000,
        };
        let text = err.to_string();
        assert!(text.contains("big.rs"), "{text}");
        assert!(text.contains("5000000000"), "{text}");
    }

    #[test]
    fn test_not_utf8_display_names_source() {
        let err = SourceMapError::NotUtf8 {
            name: Box::from("blob.bin"),
        };
        assert!(err.to_string().contains("blob.bin"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_io_display_names_source_and_kind() {
        let err = SourceMapError::Io {
            name: Box::from("missing.rs"),
            kind: std::io::ErrorKind::NotFound,
        };
        let text = err.to_string();
        assert!(text.contains("missing.rs"), "{text}");
    }

    #[test]
    fn test_error_is_clonable_and_equatable() {
        let a = SourceMapError::SpaceExhausted {
            needed: 1,
            available: 0,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
