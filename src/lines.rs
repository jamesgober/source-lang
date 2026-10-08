//! The per-source line table behind [`SourceMap::line_col`](crate::SourceMap::line_col).
//!
//! The map computes each source's line starts once, when the source is added,
//! and keeps them in a side table indexed by `SourceId` (not inside
//! [`SourceFile`](crate::SourceFile), so the records `locate` binary-searches stay
//! small). A lookup is then a binary search over the table plus a character
//! count within one line, instead of a scan of the whole source per call.

use alloc::boxed::Box;
use alloc::vec::Vec;

use span_lang::{BytePos, LineCol};

/// The byte offset at which every line of `text` begins: `0`, then one past each
/// `\n`. This is the same line model as `span-lang`'s `LineIndex`: `\r\n` is one
/// break, and a lone `\r` is not a break.
///
/// The newlines are counted first so the table is allocated once at its exact
/// size: no growth reallocations, and no slack kept alive for the life of the map.
pub(crate) fn line_starts(text: &str) -> Box<[u32]> {
    let bytes = text.as_bytes();
    let breaks = bytes.iter().filter(|&&b| b == b'\n').count();
    let mut starts = Vec::with_capacity(breaks + 1);
    starts.push(0u32);
    for (i, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' {
            // The map only stores sources of at most `u32::MAX` bytes, so
            // `i < u32::MAX` and `i + 1` fits.
            starts.push(i as u32 + 1);
        }
    }
    starts.into_boxed_slice()
}

/// Resolves local offset `local` in `text` to a 1-based [`LineCol`], given the
/// text's [`line_starts`]: `O(log lines)` to find the line, then a character
/// count within that one line.
///
/// The semantics are exactly `LineIndex::new(text).line_col(local)` (pinned by
/// unit and property tests): an offset past the end clamps to the end, and one
/// inside a multi-byte character floors to that character's start, so the result
/// is always a real position and the lookup never panics. `starts` must be the
/// table built from this `text`; it is never empty.
pub(crate) fn line_col(text: &str, starts: &[u32], local: BytePos) -> LineCol {
    let mut at = local.to_usize().min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }

    // `starts[0] == 0 <= at`, so the partition point is at least 1; `max(1)`
    // keeps the subtraction total even for a malformed (empty) table.
    let line_idx = starts.partition_point(|&start| start as usize <= at).max(1) - 1;
    let line_start = starts.get(line_idx).map_or(0, |&s| s as usize).min(at);

    // ASCII fast path: a byte count is a character count.
    let segment = &text[line_start..at];
    let col = if segment.is_ascii() {
        segment.len()
    } else {
        segment.chars().count()
    };

    LineCol::new(
        (line_idx as u32).saturating_add(1),
        (col as u32).saturating_add(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use span_lang::LineIndex;

    #[test]
    fn test_line_starts_follow_the_line_index_model() {
        assert_eq!(&*line_starts(""), &[0]);
        assert_eq!(&*line_starts("a"), &[0]);
        assert_eq!(&*line_starts("a\n"), &[0, 2]);
        // `\r\n` is one break; a lone `\r` is an ordinary character.
        assert_eq!(&*line_starts("a\r\nb\rc\n\n"), &[0, 3, 7, 8]);
    }

    #[test]
    fn test_cached_line_col_matches_line_index() {
        for text in ["", "abc", "αβ\n\tx\r\nlast", "\n\n", "a\rb\n世界\n"] {
            let starts = line_starts(text);
            let index = LineIndex::new(text);
            // Every offset, including mid-character and past-the-end ones.
            for at in 0..=text.len() as u32 + 3 {
                let pos = BytePos::new(at);
                assert_eq!(
                    line_col(text, &starts, pos),
                    index.line_col(pos),
                    "{text:?} at {at}"
                );
            }
        }
    }
}
