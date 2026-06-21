//! Property tests for the source-map invariants.
//!
//! Each property is checked against a deliberately naive linear-scan resolver:
//! the fast binary-search [`SourceMap::locate`] is correct only if it agrees with
//! a full scan on every position, for every map.

#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use source_lang::{BytePos, LineIndex, SourceId, SourceMap};

/// The reference resolver: walk every source in order and return the first whose
/// range contains `pos`. `O(files)`, obviously correct, and the oracle the real
/// `O(log files)` lookup must match.
fn naive_locate(map: &SourceMap, pos: BytePos) -> Option<(SourceId, BytePos)> {
    for (id, file) in map.iter() {
        if file.span().contains(pos) {
            let local = pos.to_u32() - file.span().start().to_u32();
            return Some((id, BytePos::new(local)));
        }
    }
    None
}

/// Builds a map from a list of source texts, naming them by index. Lengths are
/// small, so the global space is never exhausted.
fn build(texts: &[String]) -> SourceMap {
    let mut map = SourceMap::new();
    for (i, text) in texts.iter().enumerate() {
        map.add(format!("f{i}"), text.as_str())
            .expect("small inputs always fit");
    }
    map
}

/// The exclusive end of the global space — the high-water mark across all spans.
fn space_end(map: &SourceMap) -> u32 {
    map.iter()
        .map(|(_, f)| f.span().end().to_u32())
        .max()
        .unwrap_or(0)
}

proptest! {
    /// `locate` agrees with the naive scan on every probed position, in range and
    /// just past the end.
    #[test]
    fn locate_matches_naive_resolver(
        texts in prop::collection::vec(".{0,40}", 0..40),
        raw in any::<u32>(),
    ) {
        let map = build(&texts);
        let span = space_end(&map);
        // Probe across the whole space plus a margin, so out-of-range hits too.
        let pos = BytePos::new(raw % (span + 8));
        prop_assert_eq!(map.locate(pos), naive_locate(&map, pos));
    }

    /// Every position in range round-trips: the located base plus local offset
    /// reconstructs the global position, and the local offset is inside the file.
    #[test]
    fn located_position_round_trips_to_its_source(
        texts in prop::collection::vec(".{0,40}", 0..40),
        raw in any::<u32>(),
    ) {
        let map = build(&texts);
        let span = space_end(&map);
        prop_assume!(span > 0);
        let pos = BytePos::new(raw % span); // strictly in range
        let (id, local) = map.locate(pos).expect("in-range position resolves");
        let file = map.source(id).expect("located id is valid");
        // base + local == original global position.
        prop_assert_eq!(file.span().start().to_u32() + local.to_u32(), pos.to_u32());
        // The local offset lands inside the file's text.
        prop_assert!((local.to_u32() as usize) < file.text().len());
    }

    /// Ranges never overlap: sorted by start, each begins no earlier than the
    /// previous one ends, and at most one source contains any given position.
    #[test]
    fn source_ranges_never_overlap(
        texts in prop::collection::vec(".{0,40}", 0..40),
        raw in any::<u32>(),
    ) {
        let map = build(&texts);

        let mut prev_end = 0u32;
        for (_, file) in map.iter() {
            prop_assert!(file.span().start().to_u32() >= prev_end);
            prev_end = file.span().end().to_u32();
        }

        // No position is claimed by two different sources.
        let span = space_end(&map);
        let pos = BytePos::new(raw % (span + 8));
        let hits = map.iter().filter(|(_, f)| f.span().contains(pos)).count();
        prop_assert!(hits <= 1);
    }

    /// Global `line_col` agrees with resolving the position per-file: locate the
    /// source the naive way, build a fresh line index over just that source, and
    /// the line/column must match what the map returns in one step.
    #[test]
    fn line_col_matches_per_file_resolution(
        texts in prop::collection::vec("(?s).{0,40}", 0..40),
        raw in any::<u32>(),
    ) {
        let map = build(&texts);
        let span = space_end(&map);
        prop_assume!(span > 0);
        let pos = BytePos::new(raw % span); // strictly in range

        let got = map.line_col(pos).expect("in-range position resolves");

        // Reference: find the source by naive scan, then index that source alone.
        let (id, local) = naive_locate(&map, pos).expect("same in-range position");
        let file = map.source(id).expect("located id is valid");
        let expected = LineIndex::new(file.text()).line_col(local);

        prop_assert_eq!(got, (id, expected));
    }

    /// Ids are unique and dense: `iter` yields exactly `0..len`, and `add`
    /// returns them in that order.
    #[test]
    fn ids_are_unique_and_stable(
        texts in prop::collection::vec(".{0,40}", 0..40),
    ) {
        let mut map = SourceMap::new();
        let mut returned = Vec::new();
        for (i, text) in texts.iter().enumerate() {
            returned.push(map.add(format!("f{i}"), text.as_str()).unwrap());
        }

        // `add` returns ids in insertion order, matching iteration order.
        let iterated: Vec<SourceId> = map.iter().map(|(id, _)| id).collect();
        prop_assert_eq!(&returned, &iterated);

        // The raw indices are exactly 0, 1, ..., len-1.
        let indices: Vec<u32> = iterated.iter().map(|id| id.to_u32()).collect();
        let expected: Vec<u32> = (0..map.len() as u32).collect();
        prop_assert_eq!(indices, expected);
    }
}
