//! One stored source: its name, its text, and its place in the global space.

use alloc::boxed::Box;

use span_lang::{LineIndex, Span};

/// A single source held by a [`SourceMap`](crate::SourceMap): a display name, the
/// owned source text, and the half-open [`Span`] the text occupies in the map's
/// global position space.
///
/// The map owns the text so that everything above it can borrow `&str` for the
/// life of the map without re-reading or copying — a line index, a lexer, a
/// diagnostic renderer all read through this borrow. The [`span`](SourceFile::span)
/// is the file's footprint in the shared coordinate space: its `start` is the
/// global offset of the first byte, and a local offset within the file plus that
/// `start` is the corresponding global position.
///
/// Construction is internal; a `SourceFile` only ever comes from
/// [`SourceMap::add`](crate::SourceMap::add) or
/// [`SourceMap::source`](crate::SourceMap::source), so its span is always
/// consistent with the map that produced it.
///
/// # Examples
///
/// ```
/// use source_lang::SourceMap;
///
/// let mut map = SourceMap::new();
/// let id = map.add("greeting.txt", "hello").expect("fits");
/// let file = map.source(id).expect("just added");
///
/// assert_eq!(file.name(), "greeting.txt");
/// assert_eq!(file.text(), "hello");
/// assert_eq!(file.span().len(), 5);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    name: Box<str>,
    text: Box<str>,
    span: Span,
}

impl SourceFile {
    /// Assembles a stored source. Internal: the span must match the slot the map
    /// assigned, so only the map constructs one.
    #[inline]
    pub(crate) const fn new(name: Box<str>, text: Box<str>, span: Span) -> Self {
        Self { name, text, span }
    }

    /// Returns the source's display name — the path or label it was added under.
    ///
    /// This is purely a label for diagnostics; the map does not interpret it, so
    /// two sources may share a name and still be distinct entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add("src/main.rs", "fn main() {}").expect("fits");
    /// assert_eq!(map.source(id).unwrap().name(), "src/main.rs");
    /// ```
    #[inline]
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the source text, borrowed for the life of the map.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add("note.txt", "first line\nsecond line").expect("fits");
    /// let text = map.source(id).unwrap().text();
    /// assert_eq!(text.lines().count(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the file's half-open range in the map's global position space.
    ///
    /// `span().start()` is the global offset of the file's first byte; the file
    /// covers `start..start + text().len()`. Subtracting `start` from a global
    /// position that falls in this range gives the local offset into
    /// [`text`](SourceFile::text) — which is exactly what
    /// [`SourceMap::locate`](crate::SourceMap::locate) returns.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// let first = map.add("a.txt", "abc").expect("fits");   // global 0..3
    /// let second = map.add("b.txt", "de").expect("fits");   // global 3..5
    ///
    /// assert_eq!(map.source(first).unwrap().span().start().to_u32(), 0);
    /// assert_eq!(map.source(second).unwrap().span().start().to_u32(), 3);
    /// ```
    #[inline]
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }

    /// Builds a reusable line index over this source's text.
    ///
    /// The returned [`LineIndex`] borrows the source for as long as the
    /// [`SourceFile`] is borrowed, so it can be kept and queried many times
    /// without re-scanning. Building it is the only `O(text len)` step; each
    /// `line_col` / `offset` lookup on it is sub-linear.
    ///
    /// Prefer this over [`SourceMap::line_col`](crate::SourceMap::line_col) when
    /// resolving several positions within one source — that convenience method
    /// builds a fresh index per call, whereas this builds it once.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::{BytePos, LineCol, SourceMap};
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add("m.rs", "let x = 1;\nlet y = 2;").expect("fits");
    /// let index = map.source(id).unwrap().line_index();
    ///
    /// // Resolve as many local positions as needed against the one index.
    /// assert_eq!(index.line_col(BytePos::new(0)), LineCol::new(1, 1));
    /// assert_eq!(index.line_col(BytePos::new(11)), LineCol::new(2, 1));
    /// assert_eq!(index.line_count(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn line_index(&self) -> LineIndex<'_> {
        LineIndex::new(&self.text)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::boxed::Box;

    use super::*;

    fn boxed(s: &str) -> Box<str> {
        Box::from(s)
    }

    #[test]
    fn test_accessors_return_stored_values() {
        let file = SourceFile::new(boxed("name"), boxed("body"), Span::new(4, 8));
        assert_eq!(file.name(), "name");
        assert_eq!(file.text(), "body");
        assert_eq!(file.span(), Span::new(4, 8));
    }

    #[test]
    fn test_empty_text_has_zero_width_span() {
        let file = SourceFile::new(boxed("empty"), boxed(""), Span::empty(12));
        assert_eq!(file.text(), "");
        assert!(file.span().is_empty());
    }
}
