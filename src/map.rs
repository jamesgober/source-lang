//! The source map: many sources laid out across one global position space.

use alloc::boxed::Box;
use alloc::vec::Vec;

use span_lang::{BytePos, LineCol, Span};

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMap {
    /// Sources in insertion order; always sorted by `span().start()` because
    /// each new base is the previous high-water mark.
    files: Vec<SourceFile>,
    /// The next free global offset — the exclusive end of the last source's
    /// range, and the base the next source will be placed at.
    next_base: u32,
    /// The largest a single source may be, in bytes. Defaults to `u32::MAX`; a
    /// smaller value bounds how much one untrusted input can load.
    max_source_len: u32,
}

impl Default for SourceMap {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl SourceMap {
    /// Creates an empty map whose global position space starts at `0`.
    ///
    /// The per-source size ceiling starts at `u32::MAX`; lower it with
    /// [`set_max_source_len`](SourceMap::set_max_source_len) to bound untrusted
    /// input.
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
            max_source_len: u32::MAX,
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
            max_source_len: u32::MAX,
        }
    }

    /// Returns the current per-source size ceiling, in bytes.
    ///
    /// A source longer than this is rejected with
    /// [`SourceMapError::Oversize`] before it consumes any global space. The
    /// default is `u32::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let map = SourceMap::new();
    /// assert_eq!(map.max_source_len(), u32::MAX);
    /// ```
    #[inline]
    #[must_use]
    pub const fn max_source_len(&self) -> u32 {
        self.max_source_len
    }

    /// Sets the largest a single source may be, in bytes.
    ///
    /// Use it to bound how much one untrusted input — a file named on a command
    /// line, a buffer from the network — can pull into memory. The limit applies
    /// to every later [`add`](SourceMap::add), [`add_bytes`](SourceMap::add_bytes),
    /// and [`add_file`](SourceMap::add_file); for a file it is checked against the
    /// path's metadata before any bytes are read. Sources already in the map are
    /// unaffected.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::{SourceMap, SourceMapError};
    ///
    /// let mut map = SourceMap::new();
    /// map.set_max_source_len(8);
    ///
    /// assert!(map.add("ok", "12345678").is_ok()); // exactly 8 bytes
    /// let err = map.add("big", "123456789").unwrap_err(); // 9 bytes
    /// assert!(matches!(err, SourceMapError::Oversize { len: 9, .. }));
    /// ```
    #[inline]
    pub fn set_max_source_len(&mut self, max: u32) {
        self.max_source_len = max;
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
        self.push(name.into(), text.into())
    }

    /// Validates raw bytes as UTF-8 and adds them as a source under `name`.
    ///
    /// This is the in-memory counterpart to [`add_file`](SourceMap::add_file):
    /// both turn untrusted bytes — from a buffer here, from disk there — into a
    /// stored source through the same checks, so a network buffer and a file on
    /// disk fail and succeed the same way.
    ///
    /// # Errors
    ///
    /// - [`SourceMapError::NotUtf8`] if `bytes` are not valid UTF-8.
    /// - [`SourceMapError::Oversize`] if they exceed
    ///   [`max_source_len`](SourceMap::max_source_len).
    /// - [`SourceMapError::SpaceExhausted`] if they do not fit in the remaining
    ///   global space.
    ///
    /// On any error the map is left unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::{SourceMap, SourceMapError};
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add_bytes("greeting.txt", b"hello").expect("valid UTF-8");
    /// assert_eq!(map.source(id).unwrap().text(), "hello");
    ///
    /// // A stray binary byte is rejected, not stored as corrupt text.
    /// let err = map.add_bytes("blob", &[0xff]).unwrap_err();
    /// assert!(matches!(err, SourceMapError::NotUtf8 { .. }));
    /// ```
    pub fn add_bytes(
        &mut self,
        name: impl Into<Box<str>>,
        bytes: &[u8],
    ) -> Result<SourceId, SourceMapError> {
        let name = name.into();
        match core::str::from_utf8(bytes) {
            Ok(text) => self.push(name, Box::from(text)),
            Err(_) => Err(SourceMapError::NotUtf8 { name }),
        }
    }

    /// Reads a file from disk and adds its contents as a source named by `path`.
    ///
    /// The file's size is checked against [`max_source_len`](SourceMap::max_source_len)
    /// from its metadata *before* a single byte is read, so an oversize file is
    /// rejected without being loaded into memory. The bytes are then validated as
    /// UTF-8 and stored. The source's name is the path as given.
    ///
    /// # Errors
    ///
    /// - [`SourceMapError::Oversize`] if the file's metadata length exceeds
    ///   [`max_source_len`](SourceMap::max_source_len).
    /// - [`SourceMapError::Io`] if the path cannot be opened or read (missing
    ///   file, a directory, permission denied).
    /// - [`SourceMapError::NotUtf8`] if the contents are not valid UTF-8.
    /// - [`SourceMapError::SpaceExhausted`] if they do not fit in the remaining
    ///   global space.
    ///
    /// On any error the map is left unchanged.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add_file("src/main.rs")?;
    /// assert_eq!(map.source(id).unwrap().name(), "src/main.rs");
    /// # Ok::<(), source_lang::SourceMapError>(())
    /// ```
    #[cfg(feature = "std")]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    pub fn add_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<SourceId, SourceMapError> {
        use std::io::Read;

        let path = path.as_ref();
        let name: Box<str> = Box::from(path.to_string_lossy().as_ref());

        let io_err = |e: std::io::Error| SourceMapError::Io {
            name: name.clone(),
            kind: e.kind(),
        };

        // Reject from metadata before reading, so an oversize file never lands in
        // memory. A file without a queryable length (a pipe, some virtual files)
        // falls through to the streaming read, which the push guard still bounds.
        let mut file = std::fs::File::open(path).map_err(io_err)?;
        if let Ok(meta) = file.metadata() {
            if meta.len() > u64::from(self.max_source_len) {
                return Err(SourceMapError::Oversize {
                    name,
                    len: meta.len(),
                });
            }
        }

        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes).map_err(io_err)?;
        match core::str::from_utf8(&bytes) {
            Ok(text) => self.push(name, Box::from(text)),
            Err(_) => Err(SourceMapError::NotUtf8 { name }),
        }
    }

    /// The single insertion seam every loader funnels through: enforce the size
    /// limits, assign a non-overlapping range, and append. Nothing is mutated
    /// until both limits pass, so a rejected add leaves the map untouched.
    fn push(&mut self, name: Box<str>, text: Box<str>) -> Result<SourceId, SourceMapError> {
        let needed = text.len() as u64;
        if needed > u64::from(self.max_source_len) {
            return Err(SourceMapError::Oversize { name, len: needed });
        }

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
        self.files.push(SourceFile::new(name, text, span));
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

    /// Resolves a global position to its source and 1-based line/column.
    ///
    /// This is [`locate`](SourceMap::locate) composed with `span-lang`'s line
    /// index: the position is mapped to its source and local offset, then that
    /// offset is turned into a [`LineCol`] within the source's own text. The
    /// column counts Unicode scalar values, so a multi-byte character advances
    /// the column by one, not by its byte width.
    ///
    /// Returns `None` exactly when [`locate`](SourceMap::locate) does — for a
    /// position past the end of the last source, or at a zero-width source.
    ///
    /// Each call builds a line index over the located source, an `O(source len)`
    /// scan. To resolve many positions in the same source, take a reusable index
    /// once with [`SourceFile::line_index`] instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::{BytePos, LineCol, SourceMap};
    ///
    /// let mut map = SourceMap::new();
    /// map.add("a.rs", "fn a() {}").expect("fits"); // 0..9
    /// let b = map.add("b.rs", "let x = 1;\nlet y = 2;").expect("fits"); // 9..30
    ///
    /// // Global 20 is the second line of b.rs ("let y = 2;").
    /// let (id, lc) = map.line_col(BytePos::new(20)).expect("in range");
    /// assert_eq!(id, b);
    /// assert_eq!(lc, LineCol::new(2, 1));
    /// ```
    #[must_use]
    pub fn line_col(&self, pos: BytePos) -> Option<(SourceId, LineCol)> {
        let (id, local) = self.locate(pos)?;
        // `locate` returned this id, so the source is present.
        let line_col = self.files[id.to_u32() as usize]
            .line_index()
            .line_col(local);
        Some((id, line_col))
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

#[cfg(feature = "serde")]
mod serde_support {
    //! `serde` for [`SourceMap`]. The wire form is the list of sources — name and
    //! text — plus the size ceiling; everything else (spans, ids, the high-water
    //! mark) is derived, so it is regenerated on load rather than trusted from the
    //! bytes. Deserialisation replays the sources through the same insertion path
    //! as [`SourceMap::add`], which keeps the non-overlap and unique-id invariants
    //! intact even if the input was hand-edited or corrupted.

    use alloc::string::String;
    use alloc::vec::Vec;

    use serde::de::Error as _;
    use serde::ser::SerializeStruct;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::SourceMap;

    /// Borrowed view of one source, so serialising copies no text.
    #[derive(Serialize)]
    struct SourceRef<'a> {
        name: &'a str,
        text: &'a str,
    }

    /// Owned form held only while deserialising, before the map is rebuilt.
    #[derive(Deserialize)]
    struct SourceOwned {
        name: String,
        text: String,
    }

    /// Default ceiling for input that predates the field, matching [`SourceMap::new`].
    fn unbounded() -> u32 {
        u32::MAX
    }

    #[derive(Deserialize)]
    struct MapData {
        sources: Vec<SourceOwned>,
        #[serde(default = "unbounded")]
        max_source_len: u32,
    }

    impl Serialize for SourceMap {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let sources: Vec<SourceRef<'_>> = self
                .files
                .iter()
                .map(|f| SourceRef {
                    name: f.name(),
                    text: f.text(),
                })
                .collect();
            let mut state = serializer.serialize_struct("SourceMap", 2)?;
            state.serialize_field("sources", &sources)?;
            state.serialize_field("max_source_len", &self.max_source_len)?;
            state.end()
        }
    }

    impl<'de> Deserialize<'de> for SourceMap {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            let data = MapData::deserialize(deserializer)?;
            // Rebuild through `push` so spans, ids, and the high-water mark are
            // regenerated and validated. The ceiling is applied only afterwards, so
            // sources accepted under a looser limit are not rejected on reload.
            let mut map = SourceMap::with_capacity(data.sources.len());
            for source in data.sources {
                let _ = map
                    .push(source.name.into(), source.text.into())
                    .map_err(D::Error::custom)?;
            }
            map.max_source_len = data.max_source_len;
            Ok(map)
        }
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

    #[test]
    fn test_add_bytes_stores_valid_utf8() {
        let mut map = SourceMap::new();
        let id = map
            .add_bytes("greeting", "héllo".as_bytes())
            .expect("valid");
        assert_eq!(map.source(id).unwrap().text(), "héllo");
    }

    #[test]
    fn test_add_bytes_rejects_invalid_utf8_and_leaves_map_unchanged() {
        let mut map = SourceMap::new();
        // A lone continuation byte is not valid UTF-8.
        let err = map.add_bytes("blob", &[0x68, 0xff, 0x69]).unwrap_err();
        match err {
            SourceMapError::NotUtf8 { name } => assert_eq!(&*name, "blob"),
            other => panic!("expected NotUtf8, got {other:?}"),
        }
        assert!(map.is_empty());
        assert_eq!(map.next_base, 0);
    }

    #[test]
    fn test_add_bytes_empty_is_a_zero_width_source() {
        let mut map = SourceMap::new();
        let id = map
            .add_bytes("empty", b"")
            .expect("zero bytes are valid utf8");
        assert!(map.source(id).unwrap().span().is_empty());
    }

    #[test]
    fn test_max_source_len_rejects_at_the_byte_boundary() {
        let mut map = SourceMap::new();
        map.set_max_source_len(4);
        assert_eq!(map.max_source_len(), 4);

        // Exactly the limit fits; one byte over does not.
        let ok = map.add("ok", "abcd").expect("exactly the limit");
        assert_eq!(map.source(ok).unwrap().span().len(), 4);

        let err = map.add("big", "abcde").unwrap_err();
        match err {
            SourceMapError::Oversize { name, len } => {
                assert_eq!(&*name, "big");
                assert_eq!(len, 5);
            }
            other => panic!("expected Oversize, got {other:?}"),
        }
        // The rejected add did not advance the space.
        assert_eq!(map.next_base, 4);
    }

    #[test]
    fn test_oversize_is_checked_before_space_exhaustion() {
        let mut map = SourceMap::new();
        map.set_max_source_len(2);
        map.next_base = u32::MAX; // no global space left at all
        // The per-source ceiling is reported, not space exhaustion.
        let err = map.add("x", "abc").unwrap_err();
        assert!(matches!(err, SourceMapError::Oversize { len: 3, .. }));
    }

    #[test]
    fn test_line_col_resolves_across_files_and_lines() {
        let mut map = SourceMap::new();
        let a = map.add("a", "ab\ncd").expect("fits"); // 0..5, lines 1-2
        let b = map.add("b", "wx\nyz").expect("fits"); // 5..10, lines 1-2

        // First file, first line.
        assert_eq!(map.line_col(BytePos::new(0)), Some((a, LineCol::new(1, 1))));
        // First file, second line, second column ('d').
        assert_eq!(map.line_col(BytePos::new(4)), Some((a, LineCol::new(2, 2))));
        // Second file resets to its own line 1, column 1.
        assert_eq!(map.line_col(BytePos::new(5)), Some((b, LineCol::new(1, 1))));
        // Second file, second line ('y').
        assert_eq!(map.line_col(BytePos::new(8)), Some((b, LineCol::new(2, 1))));
    }

    #[test]
    fn test_line_col_counts_characters_not_bytes() {
        let mut map = SourceMap::new();
        // "αβ" is two characters but four bytes; the second char starts at byte 2.
        let id = map.add("greek", "αβ").expect("fits");
        assert_eq!(
            map.line_col(BytePos::new(2)),
            Some((id, LineCol::new(1, 2)))
        );
    }

    #[test]
    fn test_line_col_out_of_range_is_none() {
        let mut map = SourceMap::new();
        map.add("a", "abc").expect("fits"); // 0..3
        assert_eq!(map.line_col(BytePos::new(3)), None);
        assert_eq!(map.line_col(BytePos::new(99)), None);
    }
}
