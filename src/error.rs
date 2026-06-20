//! The error type returned when a source cannot be added to a map.

use core::fmt;

/// The reason a source could not be added to a [`SourceMap`](crate::SourceMap).
///
/// Every source in a map shares one 32-bit global position space, so the
/// combined byte length of all sources cannot exceed `u32::MAX`, and the map can
/// hold at most `u32::MAX` distinct sources. Adding a source that would cross
/// either limit fails with this error rather than wrapping a base offset into a
/// neighbour's range — the one way the coordinate bookkeeping could otherwise
/// corrupt silently.
///
/// The enum is `#[non_exhaustive]`: later phases add file-loading failures
/// (missing path, oversize file) alongside this variant, and a `match` on it
/// must already account for that.
///
/// # Examples
///
/// ```
/// use source_lang::{SourceMap, SourceMapError};
///
/// // A map whose global space is almost full rejects a source that overruns it.
/// let mut map = SourceMap::new();
/// # // (the boundary itself needs ~4 GiB to reach naturally; see the unit tests)
/// let id = map.add("ok.txt", "fits fine").expect("plenty of room");
/// assert_eq!(map.source(id).map(|f| f.name()), Some("ok.txt"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceMapError {
    /// The source did not fit in what remained of the map's capacity.
    ///
    /// Returned when the new source is larger than the bytes left in the global
    /// position space — either because the single source exceeds `u32::MAX`
    /// bytes, or because earlier sources have consumed the remainder — or when
    /// the map already holds the maximum number of sources. The caller cannot
    /// retry the same source against the same map; it must split the input or
    /// start a fresh map.
    SpaceExhausted {
        /// Byte length of the source that was rejected.
        needed: u64,
        /// Bytes of global position space that remained available.
        available: u64,
    },
}

impl fmt::Display for SourceMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::SpaceExhausted { needed, available } => write!(
                f,
                "source of {needed} bytes does not fit in the {available} bytes \
                 remaining in the global position space",
            ),
        }
    }
}

impl core::error::Error for SourceMapError {}

#[cfg(test)]
mod tests {
    extern crate alloc;
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
    fn test_error_is_copy_and_equatable() {
        let a = SourceMapError::SpaceExhausted {
            needed: 1,
            available: 0,
        };
        let b = a;
        assert_eq!(a, b);
    }
}
