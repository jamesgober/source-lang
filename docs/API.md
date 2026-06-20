# source-lang &mdash; API Reference

> Complete reference for every public item in `source-lang`, with examples.
> **Status: pre-1.0.** The core source-map surface below is implemented and stable
> within the 0.x series; it is frozen at `1.0.0`. See [`dev/ROADMAP.md`](../dev/ROADMAP.md)
> for what each later phase adds.

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [The model](#the-model)
- [`SourceMap`](#sourcemap)
  - [`SourceMap::new`](#sourcemapnew)
  - [`SourceMap::with_capacity`](#sourcemapwith_capacity)
  - [`SourceMap::add`](#sourcemapadd)
  - [`SourceMap::locate`](#sourcemaplocate)
  - [`SourceMap::source`](#sourcemapsource)
  - [`SourceMap::len` / `is_empty`](#sourcemaplen--is_empty)
  - [`SourceMap::iter`](#sourcemapiter)
- [`SourceFile`](#sourcefile)
- [`SourceId`](#sourceid)
- [`SourceMapError`](#sourcemaperror)
- [Re-exported coordinate types](#re-exported-coordinate-types)
- [Feature flags](#feature-flags)

---

## Overview

source-lang manages the text a compiler front-end reads. It holds many sources —
files and in-memory buffers — in one [`SourceMap`](#sourcemap), gives each a stable
[`SourceId`](#sourceid), lays them out across one global position space, and
resolves any global position back to the source and local offset it came from.

It is the multi-file layer above [`span-lang`](https://docs.rs/span-lang): a
`Span` says *where in a buffer* an error is, source-lang says *which buffer*. It
owns source storage and coordinate mapping only — tokenising and diagnostic
rendering live in other crates.

---

## Installation

```toml
[dependencies]
source-lang = "0.2"
```

Or from the terminal:

```bash
cargo add source-lang
```

The crate is `no_std`-friendly: it needs `alloc` but not the full standard
library. The default `std` feature is reserved for the disk file-loading that
lands in a later phase; with `default-features = false`, in-memory sources work
exactly as documented here.

---

## Quick start

```rust
use source_lang::{BytePos, SourceMap};

let mut map = SourceMap::new();
let main = map.add("main.rs", "fn main() {}")?;   // global 0..12
let util = map.add("util.rs", "fn helper() {}")?; // global 12..26

// Resolve a global position to its file and the local offset within it.
let (id, local) = map.locate(BytePos::new(13)).expect("inside util.rs");
assert_eq!(id, util);
assert_eq!(local, BytePos::new(1)); // 13 - 12

// The id is a stable handle back to the source.
assert_eq!(map.source(main).unwrap().name(), "main.rs");
# Ok::<(), source_lang::SourceMapError>(())
```

---

## The model

Sources are placed end to end in the order they are added. The first occupies
global offsets `0..len₀`, the next `len₀..len₀ + len₁`, and so on, so the ranges
never overlap and the whole project shares one position space. Because each base
is the running total of all earlier sources, the internal list stays sorted by
offset, and [`locate`](#sourcemaplocate) is a binary search over it — `O(log files)`
— that borrows the resolved source rather than copying it.

The space is 32 bits wide — the same envelope a single
[`BytePos`](#re-exported-coordinate-types) addresses — so the combined length of
every source is capped at `u32::MAX` (4 GiB). Overrunning that cap is the
[`SourceMapError::SpaceExhausted`](#sourcemaperror) error, never a silent wrap of
one source's base into a neighbour's range.

---

## `SourceMap`

A collection of sources laid out across one global position space. This is the
type you construct, add sources to, and query.

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
let id = map.add("hello.txt", "hello, world").expect("fits");
assert_eq!(map.len(), 1);
assert_eq!(map.source(id).unwrap().text(), "hello, world");
```

`SourceMap` derives `Clone`, `Debug`, and `Default` (`Default` is `new`).

### `SourceMap::new`

```rust
pub const fn new() -> SourceMap
```

Creates an empty map whose global position space starts at `0`. `const`, so it can
initialise a `static` or `const`.

```rust
use source_lang::SourceMap;

let map = SourceMap::new();
assert!(map.is_empty());
```

### `SourceMap::with_capacity`

```rust
pub fn with_capacity(capacity: usize) -> SourceMap
```

Creates an empty map with room for `capacity` sources preallocated.

**Parameters**

- `capacity` — the number of sources to reserve space for. A hint only: it sizes
  the internal list so that adding up to `capacity` sources does not reallocate.
  The global position space still starts empty.

Use it when the source count is known up front — for example, after globbing a
project's files — to avoid the incremental reallocation a growing `Vec` would do.

```rust
use source_lang::SourceMap;

let paths = ["a.rs", "b.rs", "c.rs"];
let mut map = SourceMap::with_capacity(paths.len());
for p in paths {
    map.add(p, "// ...").expect("fits");
}
assert_eq!(map.len(), 3);
```

### `SourceMap::add`

```rust
pub fn add(
    &mut self,
    name: impl Into<Box<str>>,
    text: impl Into<Box<str>>,
) -> Result<SourceId, SourceMapError>
```

Adds a source under `name` with the given `text`, appending it after every
existing source and returning its [`SourceId`](#sourceid).

**Parameters**

- `name` — a display name (a path or label) for diagnostics. The map does not
  interpret it, so two sources may share a name and remain distinct entries.
- `text` — the source contents. The map takes ownership (anything that converts
  into a `Box<str>` — a `String` or a `&str`), so callers can borrow the text back
  for the life of the map.

The new source takes the range `next..next + text.len()`, where `next` is the
current end of the global space. Adding empty `text` is allowed: it yields a valid
id whose source has a zero-width span and does not advance the space, so it can
never be the target of a [`locate`](#sourcemaplocate).

**Errors**

Returns [`SourceMapError::SpaceExhausted`](#sourcemaperror) if `text` does not fit
in the bytes left in the 32-bit global space, or if the map already holds the
maximum number of sources. On error the map is left unchanged.

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();

// A &str borrows; the map copies it into owned storage once.
let cfg = map.add("config.toml", "name = \"demo\"").expect("fits");
assert_eq!(map.source(cfg).unwrap().text(), "name = \"demo\"");

// A String is moved in without re-allocating its bytes.
let generated = String::from("// generated, do not edit\n");
let out = map.add("out.rs", generated).expect("fits");
assert_eq!(map.source(out).unwrap().span().start().to_u32(), 13);
```

Handling the error path explicitly:

```rust
use source_lang::{SourceMap, SourceMapError};

fn load(map: &mut SourceMap, name: &str, text: &str) {
    match map.add(name, text) {
        Ok(id) => { let _ = id; /* track it */ }
        Err(SourceMapError::SpaceExhausted { needed, available }) => {
            eprintln!("{name}: {needed} bytes did not fit ({available} left)");
        }
    }
}
```

### `SourceMap::locate`

```rust
pub fn locate(&self, pos: BytePos) -> Option<(SourceId, BytePos)>
```

Resolves a global position to the source it falls in and the local offset within
that source. This is the hot path: a binary search over the sources' start
offsets, `O(log files)`, allocation-free.

**Parameters**

- `pos` — a global position in the map's shared coordinate space.

**Returns**

`Some((id, local))` where `local` is `pos` minus the source's base — i.e. the
offset into [`SourceFile::text`](#sourcefile). Returns `None` when `pos` belongs to
no source: past the end of the last one, or at the exact offset of a zero-width
source. Membership is half-open: a source covering `start..end` contains `start`
but not `end`, so the boundary between two adjacent sources resolves to the second,
never to both.

```rust
use source_lang::{BytePos, SourceMap};

let mut map = SourceMap::new();
let a = map.add("a", "abc").expect("fits"); // 0..3
let b = map.add("b", "de").expect("fits");  // 3..5

assert_eq!(map.locate(BytePos::new(0)), Some((a, BytePos::new(0))));
assert_eq!(map.locate(BytePos::new(2)), Some((a, BytePos::new(2))));
// The shared boundary at 3 is the start of `b`.
assert_eq!(map.locate(BytePos::new(3)), Some((b, BytePos::new(0))));
// Past the last byte: no file.
assert_eq!(map.locate(BytePos::new(5)), None);
```

Reading the located text back out of the resolved source:

```rust
use source_lang::{BytePos, SourceMap};

let mut map = SourceMap::new();
map.add("a", "let x = 1;").expect("fits");
let two = map.add("b", "let y = 2;").expect("fits");

let (id, local) = map.locate(BytePos::new(14)).expect("in range");
assert_eq!(id, two);
let file = map.source(id).unwrap();
assert_eq!(&file.text()[local.to_usize()..], "y = 2;");
```

### `SourceMap::source`

```rust
pub fn source(&self, id: SourceId) -> Option<&SourceFile>
```

Borrows the source named by `id`, or `None` if the id is out of range for this map.

**Parameters**

- `id` — a [`SourceId`](#sourceid) previously returned by [`add`](#sourcemapadd) or
  [`locate`](#sourcemaplocate) / [`iter`](#sourcemapiter).

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
let id = map.add("readme.md", "# title").expect("fits");

let file = map.source(id).expect("just added");
assert_eq!(file.name(), "readme.md");
assert_eq!(file.text(), "# title");
```

### `SourceMap::len` / `is_empty`

```rust
pub fn len(&self) -> usize
pub fn is_empty(&self) -> bool
```

The number of sources in the map, and whether it holds none.

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
assert!(map.is_empty());
map.add("a", "x").expect("fits");
assert_eq!(map.len(), 1);
assert!(!map.is_empty());
```

### `SourceMap::iter`

```rust
pub fn iter(&self) -> impl ExactSizeIterator<Item = (SourceId, &SourceFile)>
```

Iterates over the sources in insertion order, pairing each with its id. The order
is also id order (`0`, `1`, …) and global-offset order, so the iterator walks the
global position space from start to end.

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
map.add("a.txt", "one").expect("fits");
map.add("b.txt", "two").expect("fits");

let names: Vec<_> = map.iter().map(|(_, f)| f.name()).collect();
assert_eq!(names, ["a.txt", "b.txt"]);

// Build a side table keyed by SourceId index.
let lengths: Vec<usize> = map.iter().map(|(_, f)| f.text().len()).collect();
assert_eq!(lengths, [3, 3]);
```

---

## `SourceFile`

One stored source: a display name, the owned text, and the half-open `Span` the
text occupies in the global position space. A `SourceFile` is only ever obtained by
borrowing it from a map — through [`source`](#sourcemapsource) or
[`iter`](#sourcemapiter) — so its span is always consistent with the map.

| Method | Signature | Description |
|--------|-----------|-------------|
| `name` | `fn name(&self) -> &str` | The display name the source was added under. |
| `text` | `fn text(&self) -> &str` | The source text, borrowed for the life of the map. |
| `span` | `const fn span(&self) -> Span` | The file's half-open range in the global space. |

`span().start()` is the global offset of the file's first byte; the file covers
`start..start + text().len()`. Subtracting `start` from an in-range global position
gives the local offset into `text()` — exactly what [`locate`](#sourcemaplocate)
returns.

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
let id = map.add("greeting.txt", "hello\nworld").expect("fits");
let file = map.source(id).expect("just added");

assert_eq!(file.name(), "greeting.txt");
assert_eq!(file.text().lines().count(), 2);
assert_eq!(file.span().len(), 11); // "hello\nworld" is 11 bytes
assert_eq!(file.span().start().to_u32(), 0);
```

`SourceFile` derives `Clone`, `Debug`, `PartialEq`, and `Eq`.

---

## `SourceId`

```rust
pub const fn to_u32(self) -> u32
```

A small, copyable handle to one source in a map. It is a 32-bit index minted when a
source is added and **stable for the life of the map**: the id keeps pointing at
the same source no matter how many more are added, because sources are only ever
appended. That stability is what lets a token, an AST node, or a cached diagnostic
store a `SourceId` and resolve it later.

The id is opaque — there is no public constructor — so it can only come from a map
that holds the source it names. `to_u32` exposes the raw insertion-order index,
which is useful as a dense array key.

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
let first = map.add("a.txt", "alpha").expect("fits");
let second = map.add("b.txt", "beta").expect("fits");

assert_eq!(first.to_u32(), 0);
assert_eq!(second.to_u32(), 1);
assert_ne!(first, second);

// Stable: `first` still resolves after `second` was added.
assert_eq!(map.source(first).unwrap().name(), "a.txt");
```

`SourceId` derives `Clone`, `Copy`, `Debug`, `PartialEq`, `Eq`, `PartialOrd`,
`Ord`, and `Hash`, so it works as a `HashMap` / `BTreeMap` key.

---

## `SourceMapError`

```rust
#[non_exhaustive]
pub enum SourceMapError {
    SpaceExhausted { needed: u64, available: u64 },
}
```

The reason a source could not be added. The enum is `#[non_exhaustive]`: later
phases add file-loading failures alongside this variant, so a `match` must include
a wildcard arm. It implements `core::error::Error` and `Display`.

**`SpaceExhausted { needed, available }`** — the source did not fit in what
remained of the map's capacity.

- `needed` — byte length of the source that was rejected.
- `available` — bytes of global position space that remained.

Returned when the new source is larger than the bytes left in the global space —
because the single source exceeds `u32::MAX` bytes, or because earlier sources
consumed the remainder — or when the map already holds the maximum number of
sources. The same source cannot be retried against the same map; split the input or
start a fresh map.

```rust
use source_lang::SourceMapError;

let err = SourceMapError::SpaceExhausted { needed: 10, available: 4 };
assert_eq!(
    err.to_string(),
    "source of 10 bytes does not fit in the 4 bytes remaining in the global position space",
);
```

---

## Re-exported coordinate types

source-lang re-exports the two `span-lang` types its API speaks in, so a consumer
does not also have to name `span-lang` as a dependency:

```rust
pub use span_lang::{BytePos, Span};
```

- **`BytePos`** — a 32-bit byte offset. In this crate it is used for both *global*
  positions (passed to [`locate`](#sourcemaplocate)) and the *local* offsets it
  returns. `BytePos::new(n)`, `.to_u32()`, `.to_usize()`.
- **`Span`** — a half-open `start..end` byte range, returned by
  [`SourceFile::span`](#sourcefile). `.start()`, `.end()`, `.len()`,
  `.contains(pos)`.

See the [`span-lang` docs](https://docs.rs/span-lang) for the full surface of both.

```rust
use source_lang::{BytePos, Span, SourceMap};

let mut map = SourceMap::new();
let id = map.add("a", "abcdef").expect("fits");
let span: Span = map.source(id).unwrap().span();
assert!(span.contains(BytePos::new(0)));
assert!(!span.contains(BytePos::new(6))); // half-open: end excluded
```

---

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std`   | yes     | Pulls in the standard library and enables `span-lang/std`. Reserved for the disk file-loading added in a later phase; in-memory sources do not require it. |
| `serde` | no      | Forwards to `span-lang/serde`. Serialisation of source-map metadata is part of a later phase. |

Disabling `std` keeps the crate `no_std` (it always needs `alloc`):

```toml
[dependencies]
source-lang = { version = "0.2", default-features = false }
```

---

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
