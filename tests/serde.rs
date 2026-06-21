//! Round-trip tests for the optional `serde` support.
//!
//! The map is serialised as its sources plus the size ceiling; deserialisation
//! rebuilds it, regenerating spans and ids. These tests confirm the rebuilt map
//! equals the original, that resolution still works through it, and that the wire
//! form tolerates a missing ceiling field.

#![cfg(feature = "serde")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use source_lang::{BytePos, LineCol, SourceId, SourceMap};

fn sample_map() -> SourceMap {
    let mut map = SourceMap::new();
    map.set_max_source_len(4096);
    map.add("main.rs", "fn main() {}").expect("fits");
    map.add("util.rs", "let x = 1;\nlet y = 2;").expect("fits");
    map.add_bytes("notes.txt", "αβγ\nδε".as_bytes())
        .expect("fits");
    map.add("empty.txt", "").expect("fits"); // zero-width source
    map
}

#[test]
fn test_round_trip_through_json_preserves_the_map() {
    let map = sample_map();

    let json = serde_json::to_string(&map).expect("serialize");
    let back: SourceMap = serde_json::from_str(&json).expect("deserialize");

    // The whole map compares equal: same sources, same layout, same ceiling.
    assert_eq!(map, back);
    assert_eq!(back.max_source_len(), 4096);
}

#[test]
fn test_round_trip_preserves_ids_spans_and_resolution() {
    let map = sample_map();
    let json = serde_json::to_string(&map).expect("serialize");
    let back: SourceMap = serde_json::from_str(&json).expect("deserialize");

    // Ids and spans regenerate identically.
    for ((id_a, file_a), (id_b, file_b)) in map.iter().zip(back.iter()) {
        assert_eq!(id_a, id_b);
        assert_eq!(file_a.name(), file_b.name());
        assert_eq!(file_a.text(), file_b.text());
        assert_eq!(file_a.span(), file_b.span());
    }

    // Resolution still works through the rebuilt map, line/column included.
    // "util.rs" is global 12..33; its newline sits at global 22, so global 23 is
    // the first byte of the second line.
    let probe = BytePos::new(23);
    assert_eq!(map.locate(probe), back.locate(probe));
    assert_eq!(back.line_col(probe), map.line_col(probe));
    assert_eq!(back.line_col(probe).unwrap().1, LineCol::new(2, 1));
}

#[test]
fn test_source_id_serializes_as_its_index() {
    let mut map = SourceMap::new();
    let _ = map.add("a", "x").expect("fits");
    let id = map.add("b", "y").expect("fits");

    let json = serde_json::to_string(&id).expect("serialize id");
    assert_eq!(json, "1"); // transparent newtype over u32

    let back: SourceId = serde_json::from_str(&json).expect("deserialize id");
    assert_eq!(back, id);
}

#[test]
fn test_missing_ceiling_field_defaults_to_unbounded() {
    // A wire form without `max_source_len` (e.g. written by an older version).
    let json = r#"{"sources":[{"name":"a","text":"hello"}]}"#;
    let map: SourceMap = serde_json::from_str(json).expect("deserialize");

    assert_eq!(map.len(), 1);
    assert_eq!(map.max_source_len(), u32::MAX);
    let (id, _) = map.iter().next().unwrap();
    assert_eq!(map.source(id).unwrap().text(), "hello");
}

#[test]
fn test_empty_map_round_trips() {
    let map = SourceMap::new();
    let json = serde_json::to_string(&map).expect("serialize");
    let back: SourceMap = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(map, back);
    assert!(back.is_empty());
}
