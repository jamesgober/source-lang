<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>source-lang</b>
    <br>
    <sub><sup>SOURCE FILES & MAPS</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/source-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/source-lang"></a>
    <a href="https://crates.io/crates/source-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/source-lang?color=%230099ff"></a>
    <a href="https://docs.rs/source-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/source-lang"></a>
    <a href="https://github.com/jamesgober/source-lang/actions"><img alt="CI" src="https://github.com/jamesgober/source-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        source-lang manages the text a compiler front-end reads: it loads files and buffers, assigns each a stable id, and maps a global position back to the specific file and local offset it came from. It is the multi-file coordinate layer that sits above raw spans, so a diagnostic can say not just where in a buffer an error is, but which file.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition).
    </p>
    <blockquote>
        <strong>Status: pre-1.0, in active development.</strong> The public API is being designed across the 0.x series and frozen at <code>1.0.0</code>. See <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## Installation

```toml
[dependencies]
source-lang = "0.4"
```

Or from the terminal:

```bash
cargo add source-lang
```

<br>

## Usage

Add sources to a map and resolve a global position back to the file and local
offset it came from.

```rust
use source_lang::{BytePos, SourceMap};

let mut map = SourceMap::new();
let main = map.add("main.rs", "fn main() {}")?;   // global 0..12
let util = map.add("util.rs", "fn helper() {}")?; // global 12..26

// Which file does global position 13 belong to, and where inside it?
let (id, local) = map.locate(BytePos::new(13)).expect("inside util.rs");
assert_eq!(id, util);
assert_eq!(local, BytePos::new(1)); // 13 - 12

// The id is a stable handle back to the source for the life of the map.
assert_eq!(map.source(main).unwrap().name(), "main.rs");
# Ok::<(), source_lang::SourceMapError>(())
```

Read the located text back out of the resolved source:

```rust
use source_lang::{BytePos, SourceMap};

let mut map = SourceMap::new();
map.add("a", "let x = 1;")?;
let two = map.add("b", "let y = 2;")?;

let (id, local) = map.locate(BytePos::new(14)).expect("in range");
assert_eq!(id, two);
let file = map.source(id).unwrap();
assert_eq!(&file.text()[local.to_usize()..], "y = 2;");
# Ok::<(), source_lang::SourceMapError>(())
```

Resolve a global position to its file and 1-based line/column in one step — what a
diagnostic renderer needs to print `file:line:col`:

```rust
use source_lang::{BytePos, LineCol, SourceMap};

let mut map = SourceMap::new();
map.add("a.rs", "fn a() {}")?;                  // global 0..9
let b = map.add("b.rs", "let x = 1;\nlet y = 2;")?; // global 9..30

let (id, lc) = map.line_col(BytePos::new(20)).expect("in range");
assert_eq!(id, b);
assert_eq!(lc, LineCol::new(2, 1)); // second line of b.rs
# Ok::<(), source_lang::SourceMapError>(())
```

Load untrusted input — a file from disk or raw bytes from a buffer — through the
same checks, so bad input is a defined error rather than a panic:

```rust
use source_lang::{SourceMap, SourceMapError};

let mut map = SourceMap::new();
map.set_max_source_len(1 << 20); // cap any single source at 1 MiB

// Raw bytes are validated as UTF-8 before they are stored.
let id = map.add_bytes("config.toml", b"name = \"demo\"")?;
assert_eq!(map.source(id).unwrap().text(), "name = \"demo\"");

// Non-UTF-8 input is rejected, naming the source.
let err = map.add_bytes("blob.bin", &[0xff, 0xfe]).unwrap_err();
assert!(matches!(err, SourceMapError::NotUtf8 { .. }));
# Ok::<(), source_lang::SourceMapError>(())
```

With the default `std` feature, `map.add_file("src/main.rs")` reads a path from
disk through those same checks, rejecting an oversize file from its metadata before
a byte is read.

Walk every loaded source in order — id order is also global-offset order:

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
map.add("a.txt", "one")?;
map.add("b.txt", "two")?;

let names: Vec<_> = map.iter().map(|(_, f)| f.name()).collect();
assert_eq!(names, ["a.txt", "b.txt"]);
# Ok::<(), source_lang::SourceMapError>(())
```

With the `serde` feature, a whole `SourceMap` round-trips through any serde format —
its spans and ids are regenerated on load, so the layout is validated rather than
trusted:

```rust,ignore
let mut map = SourceMap::new();
map.add("main.rs", "fn main() {}")?;
let json = serde_json::to_string(&map)?;
let restored: SourceMap = serde_json::from_str(&json)?;
assert_eq!(map, restored);
```

See <a href="./docs/API.md"><code>docs/API.md</code></a> for the full reference.

<br>

## How it works

Sources are placed end to end in the order they are added: the first occupies
global offsets `0..len₀`, the next `len₀..len₀ + len₁`, and so on. The ranges never
overlap, and because each base is the running total of all earlier sources, the
internal list stays sorted by offset — so <code>locate</code> is a binary search,
<code>O(log files)</code>, that borrows the resolved source rather than copying it.
The shared space is 32 bits wide (the same envelope a single <code>BytePos</code>
addresses), so the combined length of every source is capped at 4&nbsp;GiB;
overrunning it is a defined error, never a silent wrap into a neighbour's range.

<br>

## Status

<code>v0.4.0</code> adds optional <code>serde</code> support and <strong>freezes the
public API</strong>: a <code>SourceMap</code> round-trips through any serde format,
and no public items will change before <code>1.0.0</code>. The surface this freezes
is the full source map — the <code>SourceMap</code>, stable <code>SourceId</code>s,
and the non-overlapping global position space with its <code>O(log files)</code>
resolver (from <code>v0.2.0</code>), plus disk and buffer loading and
<code>line_col</code> resolution (from <code>v0.3.0</code>) — each invariant
property-tested against a naive linear scan. <code>1.0.0</code> ratifies this surface
and holds it stable until <code>2.0</code>; see the
<a href="./dev/ROADMAP.md"><code>ROADMAP</code></a>.

<hr>
<br>

## Contributing

See <a href="./dev/DIRECTIVES.md"><code>dev/DIRECTIVES.md</code></a> for engineering standards and the definition of done. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober <me@jamesgober.com>.</strong></sup>
</div>
