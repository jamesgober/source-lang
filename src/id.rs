//! Stable handles identifying a source within a map.

/// A small, copyable handle to one source in a [`SourceMap`](crate::SourceMap).
///
/// A `SourceId` is a 32-bit index minted by the map when a source is added. It
/// is stable for the life of the map: the id returned by
/// [`SourceMap::add`](crate::SourceMap::add) keeps pointing at the same source
/// no matter how many more are added afterwards, because sources are only ever
/// appended. That stability is what lets a token, an AST node, or a cached
/// diagnostic store a `SourceId` and resolve it later.
///
/// The id is deliberately opaque — there is no public constructor — so an id can
/// only come from a map that actually holds the source it names. Pass it back to
/// [`SourceMap::source`](crate::SourceMap::source) to borrow the source, or to
/// the result of [`SourceMap::locate`](crate::SourceMap::locate) to identify
/// where a global position resolved.
///
/// # Examples
///
/// ```
/// use source_lang::SourceMap;
///
/// let mut map = SourceMap::new();
/// let first = map.add("a.txt", "alpha").expect("fits");
/// let second = map.add("b.txt", "beta").expect("fits");
///
/// // Ids are assigned in order and stay distinct.
/// assert_eq!(first.to_u32(), 0);
/// assert_eq!(second.to_u32(), 1);
/// assert_ne!(first, second);
/// ```
///
/// With the `serde` feature it serialises transparently as its `u32` index, so a
/// handle stored in an AST node or a cached diagnostic round-trips on its own.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(u32);

impl SourceId {
    /// Wraps a raw index. Internal: only a map may mint an id, so that every id
    /// in circulation names a source the map actually holds.
    #[inline]
    pub(crate) const fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// Returns the raw index this id wraps.
    ///
    /// The value is the source's insertion order, starting at `0`. It is useful
    /// as a dense array key — for a side table of per-source data — but the id
    /// itself should be preferred wherever an opaque handle will do.
    ///
    /// # Examples
    ///
    /// ```
    /// use source_lang::SourceMap;
    ///
    /// let mut map = SourceMap::new();
    /// let id = map.add("only.txt", "x").expect("fits");
    /// assert_eq!(id.to_u32(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_round_trips_through_its_index() {
        let id = SourceId::from_index(7);
        assert_eq!(id.to_u32(), 7);
    }

    #[test]
    fn test_ids_order_by_index() {
        assert!(SourceId::from_index(1) < SourceId::from_index(2));
        assert_eq!(SourceId::from_index(3), SourceId::from_index(3));
    }
}
