# source-lang &mdash; API Reference

> Complete reference for every public item in `source-lang`, with examples.
> **Status: pre-1.0 — the surface below is the planned design and is being built across the 0.x series.** Items marked _(planned)_ are not yet implemented; see [`dev/ROADMAP.md`](../dev/ROADMAP.md).

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [`SourceId`](#sourceid) _(planned, v0.2.0)_
- [`SourceFile`](#sourcefile) _(planned, v0.2.0)_
- [`SourceMap`](#sourcemap) _(planned, v0.2.0)_
- [Feature flags](#feature-flags)

---

## Overview

source-lang manages the text a compiler front-end reads. It loads files and
buffers, gives each a stable [`SourceId`](#sourceid), lays them out in one global
position space, and resolves any global position back to the file and local
offset it came from. It is the multi-file layer above `span-lang`: spans say
*where in a buffer*, source-lang says *which buffer*.

It owns source storage and coordinate mapping only. Tokenising is `lexer-lang`;
rendering an error at a resolved location is `diag-lang`.

---

## Installation

```toml
[dependencies]
source-lang = "0.1"
```

The crate uses `std` for file loading; an in-memory-only buffer mode is available
with `std` disabled.

---

## `SourceId`

_(planned, v0.2.0)_ A small `Copy` handle identifying one source in a `SourceMap`.
Stable for the life of the map.

## `SourceFile`

_(planned, v0.2.0)_ One loaded source: its name, its contents, and the base offset
of its range within the map's global position space.

## `SourceMap`

_(planned, v0.2.0)_ Holds many sources across one global position space. Add a
file or buffer to get a `SourceId`; resolve a global position to its
`(SourceId, local offset)` in `O(log files)`.

```rust,ignore
use source_lang::SourceMap;

let mut map = SourceMap::new();
let main = map.add_file("main.kr", "fn main() {}");
let util = map.add_file("util.kr", "fn helper() {}");

let (id, local) = map.locate(global_pos); // -> which file + local offset
assert!(id == main || id == util);
```

---

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | File loading from disk. With it disabled, only in-memory buffers are supported. |
| `serde` | no | Serialise/deserialise source-map metadata (ids and ranges). |

source-lang depends on `span-lang` for positions and spans.

---

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
