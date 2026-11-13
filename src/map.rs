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
/// # One map per compilation
///
/// A map only grows: it never frees a range of the position space or drops a
/// source's text, because every id and global span it has handed out must keep
/// resolving. Build one map per compilation and drop it afterwards. An editor or
/// language server should build a fresh map for each revision it analyses rather
/// than adding every revision of a file to one long-lived map, which would keep
/// all of them in memory and reach [`SpaceExhausted`] after 4 GiB of cumulative
/// text (a 1 MB file edited about 4,000 times).
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
/// // The end of the last file is still in it (its end-of-input position)...
/// assert_eq!(map.locate(BytePos::new(26)), Some((util, BytePos::new(14))));
/// // ...but anything past it belongs to no file.
/// assert_eq!(map.locate(BytePos::new(27)), None);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMap {
    /// Sources in insertion order; always sorted by `span().start()` because
    /// each new base is the previous high-water mark.
    files: Vec<SourceFile>,
    /// Each source's global start offset, parallel to `files`. `locate` binary
    /// searches this dense `u32` array rather than the larger `SourceFile`
    /// records, so a lookup touches four bytes per probe.
    starts: Vec<u32>,
    /// Each source's line-start table, parallel to `files`, built once in
    /// `push` (see the `lines` module). Pure functions of the texts, so the
    /// derived equality is unaffected.
    lines: Vec<Box<[u32]>>,
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
            starts: Vec::new(),
            lines: Vec::new(),
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
            starts: Vec::with_capacity(capacity),
            lines: Vec::with_capacity(capacity),
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
    /// zero-width span and does not advance the global space. It is located only
    /// while it is the last source (its position is then the end of the space,
    /// see [`locate`](SourceMap::locate)); otherwise its offset is the next
    /// source's first byte.
    ///
    /// This is also where the source's line table is built, in one pass, so that
    /// [`line_col`](SourceMap::line_col) never rescans the text afterwards.
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
    /// rejected without being loaded into memory. The read itself is bounded too:
    /// it stops one byte past the ceiling, so a source whose metadata reports no
    /// length (a pipe, a `/proc` file) or a file that grows while it is read can
    /// never pull more than `max_source_len + 1` bytes into memory. The bytes are
    /// then validated as UTF-8 and stored. The source's name is the path as given.
    ///
    /// # Errors
    ///
    /// - [`SourceMapError::Oversize`] if the file's metadata length exceeds
    ///   [`max_source_len`](SourceMap::max_source_len), or if the read reaches
    ///   past it (then `len` is the number of bytes read before stopping,
    ///   `max_source_len + 1`, a lower bound on the true size).
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
        let path = path.as_ref();
        let name: Box<str> = Box::from(path.to_string_lossy().as_ref());

        // Reject from metadata before reading, so an oversize file never lands in
        // memory. A file without a queryable length (a pipe, some virtual files)
        // falls through to the bounded read below.
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) => {
                return Err(SourceMapError::Io {
                    name,
                    kind: e.kind(),
                });
            }
        };
        let hint = match file.metadata() {
            Ok(meta) if meta.len() > u64::from(self.max_source_len) => {
                return Err(SourceMapError::Oversize {
                    name,
                    len: meta.len(),
                });
            }
            Ok(meta) => meta.len(),
            Err(_) => 0,
        };

        let bytes = match read_bounded(file, self.max_source_len, hint) {
            Ok(Bounded::Within(bytes)) => bytes,
            Ok(Bounded::Over(len)) => return Err(SourceMapError::Oversize { name, len }),
            Err(e) => {
                return Err(SourceMapError::Io {
                    name,
                    kind: e.kind(),
                });
            }
        };
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
        self.lines.push(crate::lines::line_starts(&text));
        self.starts.push(base);
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
    /// Membership is half-open: a source covering `start..end` contains `start`
    /// but not `end`, so the boundary between two adjacent sources resolves to the
    /// second, never to both. One extension (since 1.0.1): the end of the **last**
    /// source, the current end of the global space, resolves to that source with a
    /// local offset equal to its length. That is where an end-of-input diagnostic
    /// (a zero-width span at the end of the text) points, so end-of-input errors in
    /// the most recently added source (in particular in a single-source map)
    /// resolve to it, and an empty last source is locatable. Up to 1.0.0 that
    /// position returned `None`.
    ///
    /// Returns `None` past the end of the space, on an empty map, and at the
    /// offset of an empty source that is not the last one (that offset is the next
    /// source's first byte).
    ///
    /// # Known limitation
    ///
    /// Sources are laid out contiguously, so the end of a source that is **not**
    /// the last one is the same position as the next source's first byte. That
    /// position resolves to the next source, at local offset 0: an end-of-input
    /// diagnostic for an earlier source is attributed to the following source's
    /// `1:1`. Telling the two apart needs a layout change (one reserved end
    /// position per source, as rustc does), which moves every later source's base
    /// and so waits for 2.0.
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
    /// // The end of the last source is its end-of-input position (since 1.0.1).
    /// assert_eq!(map.locate(BytePos::new(5)), Some((b, BytePos::new(2))));
    /// assert_eq!(map.locate(BytePos::new(6)), None);
    /// ```
    #[must_use]
    pub fn locate(&self, pos: BytePos) -> Option<(SourceId, BytePos)> {
        let at = pos.to_u32();

        // The list is sorted by start offset, so the last source whose range
        // begins at or before `at` is the only one that can contain it.
        let after = self.starts.partition_point(|&start| start <= at);
        let index = after.checked_sub(1)?;
        // Contiguous layout: a source ends where the next begins, and the last
        // one at the high-water mark. Half-open membership, plus the last
        // source's end position.
        let is_last = after == self.starts.len();
        let end = if is_last {
            self.next_base
        } else {
            self.starts[after]
        };
        if at < end || (is_last && at == end) {
            let local = at - self.starts[index];
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
    /// The source is chosen exactly as [`locate`](SourceMap::locate) chooses it,
    /// so the end of the last source resolves to it, one column past its last
    /// character. Returns `None` exactly when `locate` does.
    ///
    /// Each call is `O(log files + log lines)` plus a character count within the
    /// one located line: since 1.0.1 the source's line table is built once, when
    /// the source is added, never per call.
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
        let index = id.to_u32() as usize;
        let line_col = crate::lines::line_col(self.files[index].text(), &self.lines[index], local);
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

/// The outcome of a [`read_bounded`]: the bytes, or proof that the input runs
/// past the ceiling.
#[cfg(feature = "std")]
#[derive(Debug)]
enum Bounded {
    /// The whole input, no longer than the ceiling.
    Within(Vec<u8>),
    /// The input exceeded the ceiling; the number of bytes read before stopping.
    Over(u64),
}

/// Reads `reader` to its end, but never more than `max + 1` bytes: the extra
/// byte is how an over-long input is told apart from one exactly at the
/// ceiling without reading the rest of it. This is what keeps a pipe, a
/// zero-length-reporting virtual file, or a file growing under the reader from
/// loading without bound when its metadata could not vouch for its size.
///
/// `hint` (the metadata length, or `0`) only pre-sizes the buffer, capped at the
/// read limit so a wrong hint can never reserve more than the limit allows.
#[cfg(feature = "std")]
fn read_bounded(reader: impl std::io::Read, max: u32, hint: u64) -> std::io::Result<Bounded> {
    use std::io::Read;

    let limit = u64::from(max) + 1;
    let capacity = usize::try_from(hint.min(limit)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    let _ = reader.take(limit).read_to_end(&mut bytes)?;
    let read = bytes.len() as u64;
    if read > u64::from(max) {
        Ok(Bounded::Over(read))
    } else {
        Ok(Bounded::Within(bytes))
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
        // The boundary belongs to the second file (unchanged in 1.0.1; see the
        // known limitation on `locate`).
        assert_eq!(map.locate(BytePos::new(3)), Some((b, BytePos::new(0))));
        assert_eq!(map.locate(BytePos::new(4)), Some((b, BytePos::new(1))));
        // Behaviour change in 1.0.1 (ISSUES H04): the end of the space used to be
        // unmapped. It is the last file's end-of-input position, where a "found
        // end of input" diagnostic points, so it resolves to that file at its
        // length. Beyond it is still unmapped.
        assert_eq!(map.locate(BytePos::new(5)), Some((b, BytePos::new(2))));
        assert_eq!(map.locate(BytePos::new(6)), None);
    }

    #[test]
    fn test_h04_end_of_last_file_is_locatable() {
        // The lang-forge case: one source, an error at `Span::empty(len)`.
        let src = "rule = 'a' |";
        let mut map = SourceMap::new();
        let id = map.add("g.sketch", src).expect("fits");
        let eof = Span::empty(src.len() as u32);
        assert_eq!(
            map.locate(eof.start()),
            Some((id, BytePos::new(src.len() as u32)))
        );
        // One column past the last character, on the last line.
        assert_eq!(map.line_col(eof.start()), Some((id, LineCol::new(1, 13))));
        assert_eq!(map.locate(BytePos::new(src.len() as u32 + 1)), None);
    }

    #[test]
    fn test_h04_end_of_the_most_recent_file_is_locatable() {
        let mut map = SourceMap::new();
        let _a = map.add("a", "x = 1\n").expect("fits"); // 0..6
        let b = map.add("b", "y = 2\n").expect("fits"); // 6..12
        // EOF of the last file, after its trailing newline: `b` 2:1.
        assert_eq!(map.locate(BytePos::new(12)), Some((b, BytePos::new(6))));
        assert_eq!(
            map.line_col(BytePos::new(12)),
            Some((b, LineCol::new(2, 1)))
        );
        // Adding a third file moves the end of the space; 12 is now its start.
        let c = map.add("c", "z").expect("fits"); // 12..13
        assert_eq!(map.locate(BytePos::new(12)), Some((c, BytePos::new(0))));
        assert_eq!(map.locate(BytePos::new(13)), Some((c, BytePos::new(1))));
    }

    #[test]
    fn test_h04_known_limitation_inner_end_is_the_next_start() {
        // Documented limitation of the contiguous layout (fixed in 2.0): the end
        // of a file that is not the last one is the next file's first byte, and
        // resolves there. This test pins 1.0.0's answer so 1.0.1 cannot drift.
        let mut map = SourceMap::new();
        let _a = map.add("a", "x = 1\n").expect("fits"); // 0..6
        let b = map.add("b", "y = 2").expect("fits"); // 6..11
        assert_eq!(map.locate(BytePos::new(6)), Some((b, BytePos::new(0))));
        assert_eq!(map.line_col(BytePos::new(6)), Some((b, LineCol::new(1, 1))));
    }

    #[test]
    fn test_h04_empty_last_file_is_locatable() {
        // A lone empty source owns the end of the (empty) space.
        let mut map = SourceMap::new();
        let only = map.add("empty", "").expect("fits");
        assert_eq!(map.locate(BytePos::new(0)), Some((only, BytePos::new(0))));
        assert_eq!(
            map.line_col(BytePos::new(0)),
            Some((only, LineCol::new(1, 1)))
        );
        assert_eq!(map.locate(BytePos::new(1)), None);

        // An empty source after a real one: the end of the space is now the empty
        // source's position, so it wins over the previous source's end.
        let mut map = SourceMap::new();
        let _a = map.add("a", "ab").expect("fits"); // 0..2
        let empty = map.add("empty", "").expect("fits"); // 2..2
        assert_eq!(map.locate(BytePos::new(2)), Some((empty, BytePos::new(0))));
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
    #[cfg(feature = "std")]
    fn test_read_bounded_stops_an_endless_stream_one_byte_past_the_ceiling() {
        // `repeat` never ends: an unbounded `read_to_end` would never return.
        let got = read_bounded(std::io::repeat(b'a'), 16, 0).expect("in-memory read");
        assert!(matches!(got, Bounded::Over(17)), "got {got:?}");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_read_bounded_accepts_exactly_the_ceiling_whatever_the_hint() {
        // A hint that under- or over-states the size changes nothing but capacity.
        for hint in [0, 3, 4, 1_000_000] {
            match read_bounded(&b"abcd"[..], 4, hint).expect("in-memory read") {
                Bounded::Within(bytes) => assert_eq!(bytes, b"abcd"),
                other => panic!("hint {hint}: expected Within, got {other:?}"),
            }
        }
        // One byte more is over, reported as the bytes read (max + 1).
        let got = read_bounded(&b"abcde"[..], 4, 0).expect("in-memory read");
        assert!(matches!(got, Bounded::Over(5)), "got {got:?}");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_read_bounded_with_a_zero_ceiling() {
        assert!(matches!(
            read_bounded(&b""[..], 0, 0).expect("read"),
            Bounded::Within(ref b) if b.is_empty()
        ));
        assert!(matches!(
            read_bounded(&b"x"[..], 0, 0).expect("read"),
            Bounded::Over(1)
        ));
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
        // Its end, the last position of the space, is its end-of-input position.
        assert_eq!(
            map.locate(BytePos::new(u32::MAX)),
            Some((id, BytePos::new(4)))
        );
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
        let a = map.add("a", "abc").expect("fits"); // 0..3
        // Behaviour change in 1.0.1 (H04): the end of the last source is its
        // end-of-input position (one column past `c`), no longer `None`.
        assert_eq!(map.line_col(BytePos::new(3)), Some((a, LineCol::new(1, 4))));
        assert_eq!(map.line_col(BytePos::new(4)), None);
        assert_eq!(map.line_col(BytePos::new(99)), None);
    }
}
