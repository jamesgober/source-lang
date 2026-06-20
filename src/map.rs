//! The source map: many sources laid out across one global position space.

use alloc::boxed::Box;
use alloc::vec::Vec;

use span_lang::{BytePos, Span};

use crate::{SourceFile, SourceId, SourceMapError};

/// A collection of sources laid out end to end in a single global position space.
///
/// A `SourceMap` is the multi-file coordinate layer of a front-end. Each source
/// added to it gets a stable [`SourceId`] and a non-overlapping range in one
/// shared position space, so a single global [`BytePos`] names a point across the
/// whole project. [`locate`](SourceMap::locate) maps such a position back to its
/// `(SourceId, local offset)` — the inverse of the layout — which is how a
/// diagnostic rendered against a global span knows *which file* to point at.
///
/// # Layout
///
/// Sources are placed in the order they are added: the first occupies
/// `0..len₀`, the next `len₀..len₀ + len₁`, and so on. Because the bases only
/// increase, the internal list is always sorted by start offset, so a lookup is a
/// binary search over it — `O(log files)` — with no separate index to maintain.
/// The whole space is 32 bits wide, the same envelope a single
/// [`BytePos`] addresses, so the combined length of every source is capped at
/// `u32::MAX`; overrunning it is the [`SpaceExhausted`] error, never a silent
/// wrap into a neighbour's range.
///
/// [`SpaceExhausted`]: SourceMapError::SpaceExhausted
///
/// # Examples
///
/// ```
/// use source_lang::{BytePos, SourceMap};
///
/// let mut map = SourceMap::new();
/// let main = map.add("main.rs", "fn main() {}").expect("fits"); // global 0..12
/// let util = map.add("util.rs", "fn helper() {}").expect("fits"); // global 12..26
///
/// // A global position resolves to the file it lands in and the local offset.
/// let (id, local) = map.locate(BytePos::new(13)).expect("inside util.rs");
/// assert_eq!(id, util);
/// assert_eq!(local, BytePos::new(1)); // 13 - 12
/// assert_eq!(map.source(id).unwrap().name(), "util.rs");
///
/// // Position 0 is the very start of the first file.
/// assert_eq!(map.locate(BytePos::new(0)).unwrap().0, main);
///
/// // Anything past the last byte belongs to no file.
/// assert_eq!(map.locate(BytePos::new(26)), None);
/// ```
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    /// Sources in insertion order; always sorted by `span().start()` because
    /// each new base is the previous high-water mark.
    files: Vec<SourceFile>,
    /// The next free global offset — the exclusive end of the last source's
    /// range, and the base the next source will be placed at.
    next_base: u32,
}

impl SourceMap {
    /// Creates an empty map whose global position space starts at `0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let map = SourceMap::new();
    /// assert!(map.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            files: Vec::new(),
            next_base: 0,
        }
    }

    /// Creates an empty map with room for `capacity` sources preallocated.
    ///
    /// A hint only: it sizes the internal list so that adding up to `capacity`
    /// sources does not reallocate, which matters when the source count is known
    /// up front. The global position space still starts empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::with_capacity(2);
    /// map.add("a", "x").expect("fits");
    /// map.add("b", "y").expect("fits");
    /// assert_eq!(map.len(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            files: Vec::with_capacity(capacity),
            next_base: 0,
        }
    }

    /// Adds a source under `name` with the given `text`, returning its
    /// [`SourceId`].
    ///
    /// The source is appended after every existing one: it takes the range
    /// `next..next + text.len()` where `next` is the current end of the global
    /// space. Both `name` and `text` are taken by value (anything that converts
    /// into a `Box<str>` — a `String` or a `&str`), so the map owns the text and
    /// callers can borrow it back for the life of the map.
    ///
    /// Adding an empty `text` is allowed: it yields a valid id whose source has a
    /// zero-width span and does not advance the global space, so it can never be
    /// the target of a [`locate`](SourceMap::locate).
    ///
    /// # Errors
    ///
    /// Returns [`SourceMapError::SpaceExhausted`] if `text` does not fit in the
    /// bytes left in the 32-bit global space, or if the map already holds the
    /// maximum number of sources. The map is left unchanged, so the failure is
    /// recoverable.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add("config.toml", "name = \"demo\"").expect("fits");
    /// assert_eq!(map.source(id).unwrap().text(), "name = \"demo\"");
    ///
    /// // A String works just as well as a &str.
    /// let owned = String::from("generated");
    /// let _ = map.add("out.txt", owned).expect("fits");
    /// ```
    pub fn add(
        &mut self,
        name: impl Into<Box<str>>,
        text: impl Into<Box<str>>,
    ) -> Result<SourceId, SourceMapError> {
        let text = text.into();
        let needed = text.len() as u64;
        let base = self.next_base;
        // `base` never exceeds `u32::MAX`, so this is the bytes still free.
        let available = u64::from(u32::MAX - base);

        // The source must fit in the remaining bytes, and the map must have an
        // unused id left to mint. Both are checked before anything is mutated.
        let index = u32::try_from(self.files.len());
        let (len, index) = match (needed <= available, index) {
            // `needed <= available <= u32::MAX`, so the narrowing is lossless.
            (true, Ok(index)) => (needed as u32, index),
            _ => return Err(SourceMapError::SpaceExhausted { needed, available }),
        };

        // `base + len <= base + available == u32::MAX`, so this cannot overflow.
        let end = base + len;
        let span = Span::new(base, end);
        let id = SourceId::from_index(index);
        self.files.push(SourceFile::new(name.into(), text, span));
        self.next_base = end;
        Ok(id)
    }

    /// Resolves a global position to the source it falls in and the local offset
    /// within that source.
    ///
    /// The returned [`BytePos`] is `pos` minus the source's base, i.e. the offset
    /// into [`SourceFile::text`]. Resolution is a binary search over the sources'
    /// start offsets, so it is `O(log files)` and borrows the located source
    /// rather than copying it.
    ///
    /// Returns `None` when `pos` belongs to no source: past the end of the last
    /// one, or — since a zero-width source contains no position — at the exact
    /// offset of an empty source. The membership is half-open: a source covering
    /// `start..end` contains `start` but not `end`, so the boundary between two
    /// adjacent sources resolves to the second, never to both.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::{BytePos, SourceMap};
    ///
    /// let mut map = SourceMap::new();
    /// let a = map.add("a", "abc").expect("fits");  // 0..3
    /// let b = map.add("b", "de").expect("fits");   // 3..5
    ///
    /// assert_eq!(map.locate(BytePos::new(2)), Some((a, BytePos::new(2))));
    /// // The shared boundary at 3 is the start of `b`, not the end of `a`.
    /// assert_eq!(map.locate(BytePos::new(3)), Some((b, BytePos::new(0))));
    /// assert_eq!(map.locate(BytePos::new(5)), None);
    /// ```
    #[must_use]
    pub fn locate(&self, pos: BytePos) -> Option<(SourceId, BytePos)> {
        let at = pos.to_u32();

        // The list is sorted by start offset, so the last source whose range
        // begins at or before `at` is the only one that can contain it.
        let after = self
            .files
            .partition_point(|f| f.span().start().to_u32() <= at);
        let index = after.checked_sub(1)?;
        let file = &self.files[index];

        if file.span().contains(pos) {
            let local = at - file.span().start().to_u32();
            // `index < files.len() <= u32::MAX`, so the cast is lossless.
            Some((SourceId::from_index(index as u32), BytePos::new(local)))
        } else {
            None
        }
    }

    /// Borrows the source named by `id`, or `None` if the id is not from this map.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add("readme.md", "# title").expect("fits");
    /// assert_eq!(map.source(id).unwrap().name(), "readme.md");
    /// ```
    #[inline]
    #[must_use]
    pub fn source(&self, id: SourceId) -> Option<&SourceFile> {
        self.files.get(id.to_u32() as usize)
    }

    /// Returns the number of sources in the map.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// assert_eq!(map.len(), 0);
    /// map.add("a", "x").expect("fits");
    /// assert_eq!(map.len(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Returns `true` if the map holds no sources.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// assert!(map.is_empty());
    /// map.add("a", "x").expect("fits");
    /// assert!(!map.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Iterates over the sources in insertion order, pairing each with its id.
    ///
    /// The order is also id order (`0`, `1`, …) and global-offset order, so the
    /// iterator walks the global position space from start to end. Useful for
    /// listing the loaded files or building a side table keyed by `SourceId`.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// map.add("a.txt", "one").expect("fits");
    /// map.add("b.txt", "two").expect("fits");
    ///
    /// let names: Vec<_> = map.iter().map(|(_, f)| f.name()).collect();
    /// assert_eq!(names, ["a.txt", "b.txt"]);
    /// ```
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (SourceId, &SourceFile)> + '_ {
        self.files
            .iter()
            .enumerate()
            // `i < files.len() <= u32::MAX`, so the cast is lossless.
            .map(|(i, file)| (SourceId::from_index(i as u32), file))
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::format;
    use alloc::vec::Vec;

    use super::*;

    #[test]
    fn test_add_assigns_sequential_stable_ids() {
        let mut map = SourceMap::new();
        let a = map.add("a", "x").expect("fits");
        let b = map.add("b", "yy").expect("fits");
        let c = map.add("c", "zzz").expect("fits");
        assert_eq!((a.to_u32(), b.to_u32(), c.to_u32()), (0, 1, 2));
        // Earlier ids still resolve to their original source after later adds.
        assert_eq!(map.source(a).unwrap().name(), "a");
        assert_eq!(map.source(b).unwrap().name(), "b");
    }

    #[test]
    fn test_layout_is_contiguous_and_non_overlapping() {
        let mut map = SourceMap::new();
        map.add("a", "abc").expect("fits"); // 0..3
        map.add("b", "de").expect("fits"); // 3..5
        map.add("c", "fghi").expect("fits"); // 5..9
        let spans: Vec<_> = map.iter().map(|(_, f)| f.span()).collect();
        assert_eq!(spans[0], Span::new(0, 3));
        assert_eq!(spans[1], Span::new(3, 5));
        assert_eq!(spans[2], Span::new(5, 9));
    }

    #[test]
    fn test_locate_at_boundaries_zero_one_and_past_end() {
        let mut map = SourceMap::new();
        let a = map.add("a", "abc").expect("fits"); // 0..3
        let b = map.add("b", "de").expect("fits"); // 3..5
        assert_eq!(map.locate(BytePos::new(0)), Some((a, BytePos::new(0))));
        assert_eq!(map.locate(BytePos::new(2)), Some((a, BytePos::new(2))));
        // The boundary belongs to the second file.
        assert_eq!(map.locate(BytePos::new(3)), Some((b, BytePos::new(0))));
        assert_eq!(map.locate(BytePos::new(4)), Some((b, BytePos::new(1))));
        // End of the space and beyond are unmapped.
        assert_eq!(map.locate(BytePos::new(5)), None);
        assert_eq!(map.locate(BytePos::new(6)), None);
    }

    #[test]
    fn test_locate_on_empty_map_is_none() {
        let map = SourceMap::new();
        assert_eq!(map.locate(BytePos::new(0)), None);
    }

    #[test]
    fn test_empty_source_does_not_advance_space_and_is_unlocatable() {
        let mut map = SourceMap::new();
        let a = map.add("a", "ab").expect("fits"); // 0..2
        let empty = map.add("empty", "").expect("fits"); // 2..2
        let b = map.add("b", "cd").expect("fits"); // 2..4

        assert!(map.source(empty).unwrap().span().is_empty());
        // Position 2 is the start of `b`, never the zero-width `empty`.
        assert_eq!(map.locate(BytePos::new(2)), Some((b, BytePos::new(0))));
        assert_eq!(map.locate(BytePos::new(1)), Some((a, BytePos::new(1))));
    }

    #[test]
    fn test_source_rejects_foreign_id() {
        let mut map = SourceMap::new();
        let _ = map.add("a", "x").expect("fits");
        let mut other = SourceMap::new();
        let foreign = other.add("b", "y").expect("fits");
        // `foreign` has index 0, which exists here too, so cross-map ids are not
        // distinguishable by value — but an out-of-range index is rejected.
        let beyond = other.add("c", "z").expect("fits");
        assert!(map.source(beyond).is_none());
        assert!(map.source(foreign).is_some());
    }

    #[test]
    fn test_add_at_space_boundary_accepts_exact_fit() {
        let mut map = SourceMap::new();
        // Drive the high-water mark to four bytes below the ceiling without
        // allocating gigabytes; the field is private to this module's tests.
        map.next_base = u32::MAX - 4;
        let id = map.add("edge", "abcd").expect("exactly fills the space");
        assert_eq!(
            map.source(id).unwrap().span(),
            Span::new(u32::MAX - 4, u32::MAX)
        );
        assert_eq!(map.next_base, u32::MAX);
    }

    #[test]
    fn test_add_past_space_boundary_is_rejected() {
        let mut map = SourceMap::new();
        map.next_base = u32::MAX - 4;
        let err = map.add("edge", "abcde").expect_err("one byte too many");
        assert_eq!(
            err,
            SourceMapError::SpaceExhausted {
                needed: 5,
                available: 4,
            },
        );
        // The map is unchanged after a rejected add.
        assert!(map.is_empty());
        assert_eq!(map.next_base, u32::MAX - 4);
    }

    #[test]
    fn test_add_empty_source_at_full_space_still_succeeds() {
        let mut map = SourceMap::new();
        map.next_base = u32::MAX;
        // Zero bytes fit even when no space remains; one byte does not.
        let empty = map.add("nothing", "").expect("zero bytes always fit");
        assert!(map.source(empty).unwrap().span().is_empty());
        assert!(map.add("one", "x").is_err());
    }

    #[test]
    fn test_iter_reports_exact_len_and_pairs_ids_in_order() {
        let mut map = SourceMap::new();
        for i in 0..5 {
            map.add(format!("f{i}"), "..").expect("fits");
        }
        let mut iter = map.iter();
        assert_eq!(iter.len(), 5);
        let collected: Vec<_> = iter.by_ref().map(|(id, _)| id.to_u32()).collect();
        assert_eq!(collected, [0, 1, 2, 3, 4]);
    }
}
