# source-lang &mdash; API Reference

> Complete reference for every public item in `source-lang`, with examples.
> **Status: frozen.** As of `0.4.0` the public surface below is complete and will
> not change before `1.0.0` — only documentation, tests, and internal work follow.
> `1.0.0` ratifies this surface and holds it stable until `2.0`. See
> [`dev/ROADMAP.md`](../dev/ROADMAP.md).

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [The model](#the-model)
- [`SourceMap`](#sourcemap)
  - [`SourceMap::new`](#sourcemapnew)
  - [`SourceMap::with_capacity`](#sourcemapwith_capacity)
  - [`SourceMap::add`](#sourcemapadd)
  - [`SourceMap::add_bytes`](#sourcemapadd_bytes)
  - [`SourceMap::add_file`](#sourcemapadd_file)
  - [`SourceMap::locate`](#sourcemaplocate)
  - [`SourceMap::line_col`](#sourcemapline_col)
  - [`SourceMap::source`](#sourcemapsource)
  - [`SourceMap::max_source_len` / `set_max_source_len`](#sourcemapmax_source_len--set_max_source_len)
  - [`SourceMap::len` / `is_empty`](#sourcemaplen--is_empty)
  - [`SourceMap::iter`](#sourcemapiter)
- [`SourceFile`](#sourcefile)
  - [`SourceFile::line_index`](#sourcefileline_index)
- [`SourceId`](#sourceid)
- [`SourceMapError`](#sourcemaperror)
- [Re-exported coordinate types](#re-exported-coordinate-types)
- [Serialization](#serialization)
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
source-lang = "0.4"
```

Or from the terminal:

```bash
cargo add source-lang
```

The crate is `no_std`-friendly: it needs `alloc` but not the full standard
library. The default `std` feature enables disk file-loading
([`add_file`](#sourcemapadd_file)); with `default-features = false` the crate stays
`no_std`, and in-memory sources — [`add`](#sourcemapadd),
[`add_bytes`](#sourcemapadd_bytes), and all resolution — work exactly as documented
here.

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
        // `SourceMapError` is `#[non_exhaustive]`, so a wildcard is required.
        Err(e) => eprintln!("{name}: {e}"),
    }
}
```

### `SourceMap::add_bytes`

```rust
pub fn add_bytes(
    &mut self,
    name: impl Into<Box<str>>,
    bytes: &[u8],
) -> Result<SourceId, SourceMapError>
```

Validates raw bytes as UTF-8 and adds them as a source. This is the in-memory
counterpart to [`add_file`](#sourcemapadd_file): a buffer from the network and a
file on disk pass through the same checks, so they succeed and fail the same way.

**Parameters**

- `name` — a display name for diagnostics, as for [`add`](#sourcemapadd).
- `bytes` — the candidate source content. Borrowed, not taken: it is copied into
  owned storage only once validation passes.

**Errors**

- [`SourceMapError::NotUtf8`](#sourcemaperror) if `bytes` are not valid UTF-8.
- [`SourceMapError::Oversize`](#sourcemaperror) if they exceed
  [`max_source_len`](#sourcemapmax_source_len--set_max_source_len).
- [`SourceMapError::SpaceExhausted`](#sourcemaperror) if they do not fit in the
  remaining global space.

On any error the map is left unchanged.

```rust
use source_lang::SourceMap;

let mut map = SourceMap::new();
let id = map.add_bytes("greeting.txt", b"hello").expect("valid UTF-8");
assert_eq!(map.source(id).unwrap().text(), "hello");
```

Rejecting binary input rather than storing it as corrupt text:

```rust
use source_lang::{SourceMap, SourceMapError};

let mut map = SourceMap::new();
let err = map.add_bytes("blob.bin", &[0xff, 0xfe]).unwrap_err();
assert!(matches!(err, SourceMapError::NotUtf8 { .. }));
```

### `SourceMap::add_file`

```rust
#[cfg(feature = "std")]
pub fn add_file(
    &mut self,
    path: impl AsRef<std::path::Path>,
) -> Result<SourceId, SourceMapError>
```

Reads a file from disk and adds its contents as a source named by `path`. Requires
the default `std` feature. The file's size is checked against
[`max_source_len`](#sourcemapmax_source_len--set_max_source_len) from its metadata
*before* any byte is read, so an oversize file is rejected without being loaded into
memory. The contents are then validated as UTF-8 and stored.

**Parameters**

- `path` — the file to read. Anything that is `AsRef<Path>` (a `&str`, `String`,
  `Path`, or `PathBuf`). The source's name is the path as given.

**Errors**

- [`SourceMapError::Oversize`](#sourcemaperror) if the file's metadata length
  exceeds [`max_source_len`](#sourcemapmax_source_len--set_max_source_len).
- [`SourceMapError::Io`](#sourcemaperror) if the path cannot be opened or read — a
  missing file, a directory, a permission error.
- [`SourceMapError::NotUtf8`](#sourcemaperror) if the contents are not valid UTF-8.
- [`SourceMapError::SpaceExhausted`](#sourcemaperror) if they do not fit in the
  remaining global space.

On any error the map is left unchanged.

```rust,no_run
use source_lang::SourceMap;

let mut map = SourceMap::new();
let id = map.add_file("src/main.rs")?;
assert_eq!(map.source(id).unwrap().name(), "src/main.rs");
# Ok::<(), source_lang::SourceMapError>(())
```

Capping how much one file may load before reading it:

```rust,no_run
use source_lang::{SourceMap, SourceMapError};

let mut map = SourceMap::new();
map.set_max_source_len(64 * 1024); // 64 KiB per source

match map.add_file("maybe-huge.txt") {
    Ok(id) => { let _ = id; }
    Err(SourceMapError::Oversize { name, len }) => {
        eprintln!("{name}: {len} bytes exceeds the limit; skipped");
    }
    Err(e) => return Err(e),
}
# Ok::<(), source_lang::SourceMapError>(())
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

### `SourceMap::line_col`

```rust
pub fn line_col(&self, pos: BytePos) -> Option<(SourceId, LineCol)>
```

Resolves a global position to its source and 1-based line/column in one step. This
is [`locate`](#sourcemaplocate) composed with `span-lang`'s line index: the position
is mapped to its source and local offset, then that offset is turned into a
[`LineCol`](#re-exported-coordinate-types) within the source's text. The column
counts Unicode scalar values, so a multi-byte character advances the column by one,
not by its byte width.

**Parameters**

- `pos` — a global position in the map's shared coordinate space.

**Returns**

`Some((id, line_col))`, or `None` exactly when [`locate`](#sourcemaplocate) returns
`None` — a position past the end of the last source, or at a zero-width source.

> **Cost.** Each call builds a line index over the located source, an
> `O(source len)` scan. To resolve many positions in the same source, take a
> reusable index once with [`SourceFile::line_index`](#sourcefileline_index)
> instead of calling `line_col` repeatedly.

```rust
use source_lang::{BytePos, LineCol, SourceMap};

let mut map = SourceMap::new();
map.add("a.rs", "fn a() {}").expect("fits");            // 0..9
let b = map.add("b.rs", "let x = 1;\nlet y = 2;").expect("fits"); // 9..30

// Global 20 lands on the second line of b.rs.
let (id, lc) = map.line_col(BytePos::new(20)).expect("in range");
assert_eq!(id, b);
assert_eq!(lc, LineCol::new(2, 1));

// Formats as the editors-and-compilers `line:col`.
assert_eq!(lc.to_string(), "2:1");
```

Columns count characters, not bytes:

```rust
use source_lang::{BytePos, LineCol, SourceMap};

let mut map = SourceMap::new();
let id = map.add("greek.txt", "αβγ").expect("fits"); // 6 bytes, 3 chars

// Byte 4 is the third character despite being the fifth byte.
assert_eq!(map.line_col(BytePos::new(4)), Some((id, LineCol::new(1, 3))));
```

### `SourceMap::max_source_len` / `set_max_source_len`

```rust
pub const fn max_source_len(&self) -> u32
pub fn set_max_source_len(&mut self, max: u32)
```

The largest a single source may be, in bytes, and a setter for it. A source longer
than the ceiling is rejected with [`SourceMapError::Oversize`](#sourcemaperror)
before it consumes any global space; for a file the limit is checked against the
path's metadata before any byte is read. The default is `u32::MAX` — the addressing
limit of the position space — so by default only a structurally impossible source is
rejected this way.

**Parameters**

- `max` (setter) — the new per-source ceiling in bytes. Applies to every later
  [`add`](#sourcemapadd), [`add_bytes`](#sourcemapadd_bytes), and
  [`add_file`](#sourcemapadd_file); sources already in the map are unaffected.

Use it to bound how much one untrusted input — a path from a command line, a buffer
from the network — can pull into memory.

```rust
use source_lang::{SourceMap, SourceMapError};

let mut map = SourceMap::new();
assert_eq!(map.max_source_len(), u32::MAX);

map.set_max_source_len(8);
assert!(map.add("ok", "12345678").is_ok()); // exactly 8 bytes
let err = map.add("big", "123456789").unwrap_err(); // 9 bytes
assert!(matches!(err, SourceMapError::Oversize { len: 9, .. }));
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
| `line_index` | `fn line_index(&self) -> LineIndex<'_>` | A reusable line index over the source's text. |

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

### `SourceFile::line_index`

```rust
pub fn line_index(&self) -> LineIndex<'_>
```

Builds a reusable [`LineIndex`](#re-exported-coordinate-types) over this source's
text. The index borrows the source for as long as the `SourceFile` is borrowed, so
it can be kept and queried many times without re-scanning. Building it is the only
`O(text len)` step; each `line_col` / `offset` lookup on it is sub-linear.

Prefer this over [`SourceMap::line_col`](#sourcemapline_col) when resolving several
positions in one source — that convenience method builds a fresh index per call,
whereas this builds it once and the caller reuses it.

```rust
use source_lang::{BytePos, LineCol, SourceMap};

let mut map = SourceMap::new();
let id = map.add("m.rs", "let x = 1;\nlet y = 2;").expect("fits");
let index = map.source(id).unwrap().line_index();

// Resolve as many local positions as needed against the one index.
assert_eq!(index.line_col(BytePos::new(0)), LineCol::new(1, 1));
assert_eq!(index.line_col(BytePos::new(11)), LineCol::new(2, 1));
assert_eq!(index.line_count(), 2);
```

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
    Oversize { name: Box<str>, len: u64 },
    SpaceExhausted { needed: u64, available: u64 },
    NotUtf8 { name: Box<str> },
    #[cfg(feature = "std")]
    Io { name: Box<str>, kind: std::io::ErrorKind },
}
```

The reason a source could not be added. The enum is `#[non_exhaustive]`, so a
downstream `match` must include a wildcard arm. It derives `Clone`, `Debug`,
`PartialEq`, and `Eq` (it is **not** `Copy`, because the file-loading variants carry
the offending source's name), and implements `core::error::Error` and `Display`.

**`Oversize { name, len }`** — the source is larger than the map's per-source
ceiling ([`max_source_len`](#sourcemapmax_source_len--set_max_source_len)).

- `name` — display name of the rejected source.
- `len` — its byte length.

For a file the length comes from the path's metadata, so an oversize file is rejected
before its bytes are read.

**`SpaceExhausted { needed, available }`** — the source is within the per-source
ceiling but does not fit in what remains of the shared global space.

- `needed` — byte length of the source that was rejected.
- `available` — bytes of global position space that remained.

Returned when earlier sources have consumed the remainder of the 32-bit space, or
when the map already holds the maximum number of sources. The same source cannot be
retried against the same map; split the input or start a fresh map.

**`NotUtf8 { name }`** — the source's bytes are not valid UTF-8.

- `name` — display name of the source whose bytes failed validation.

Returned by [`add_bytes`](#sourcemapadd_bytes) and [`add_file`](#sourcemapadd_file)
for a truncated multi-byte sequence or a stray binary byte.

**`Io { name, kind }`** (feature `std`) — a file's contents could not be read.

- `name` — the path that was requested.
- `kind` — the [`std::io::ErrorKind`] category (e.g. `NotFound`, `PermissionDenied`).

Returned by [`add_file`](#sourcemapadd_file). The `ErrorKind` is stored instead of a
full `std::io::Error` so the type stays `Clone` and `Eq` like the rest.

```rust
use source_lang::SourceMapError;

let err = SourceMapError::SpaceExhausted { needed: 10, available: 4 };
assert_eq!(
    err.to_string(),
    "source of 10 bytes does not fit in the 4 bytes remaining in the global position space",
);

let err = SourceMapError::NotUtf8 { name: "blob.bin".into() };
assert_eq!(err.to_string(), "source `blob.bin` is not valid UTF-8");
```

Handling every failure mode of a file load:

```rust,no_run
use source_lang::{SourceMap, SourceMapError};

fn load(map: &mut SourceMap, path: &str) {
    match map.add_file(path) {
        Ok(id) => { let _ = id; /* track it */ }
        Err(SourceMapError::Oversize { name, len }) => {
            eprintln!("{name}: {len} bytes is too large; skipped");
        }
        Err(SourceMapError::Io { name, kind }) => {
            eprintln!("{name}: could not read ({kind})");
        }
        Err(SourceMapError::NotUtf8 { name }) => {
            eprintln!("{name}: not valid UTF-8");
        }
        Err(e) => eprintln!("{path}: {e}"),
    }
}
```

---

## Re-exported coordinate types

source-lang re-exports the `span-lang` types its API speaks in, so a consumer does
not also have to name `span-lang` as a dependency:

```rust
pub use span_lang::{BytePos, LineCol, LineIndex, Span};
```

- **`BytePos`** — a 32-bit byte offset. In this crate it is used for both *global*
  positions (passed to [`locate`](#sourcemaplocate)) and the *local* offsets it
  returns. `BytePos::new(n)`, `.to_u32()`, `.to_usize()`.
- **`Span`** — a half-open `start..end` byte range, returned by
  [`SourceFile::span`](#sourcefile). `.start()`, `.end()`, `.len()`,
  `.contains(pos)`.
- **`LineCol`** — a 1-based `line:col` coordinate, returned by
  [`line_col`](#sourcemapline_col). Public `.line` and `.col` fields;
  `LineCol::new(line, col)`; `Display` formats as `line:col`.
- **`LineIndex`** — a byte-offset → line/column index over one source, returned by
  [`SourceFile::line_index`](#sourcefileline_index). `.line_col(pos)`,
  `.offset(line_col)`, `.line_span(line)`, `.line_count()`.

See the [`span-lang` docs](https://docs.rs/span-lang) for the full surface of all four.

```rust
use source_lang::{BytePos, Span, SourceMap};

let mut map = SourceMap::new();
let id = map.add("a", "abcdef").expect("fits");
let span: Span = map.source(id).unwrap().span();
assert!(span.contains(BytePos::new(0)));
assert!(!span.contains(BytePos::new(6))); // half-open: end excluded
```

---

## Serialization

With the `serde` feature, [`SourceMap`](#sourcemap) and [`SourceId`](#sourceid)
implement `serde::Serialize` and `serde::Deserialize`, and the re-exported
coordinate types carry their `span-lang` serde implementations.

A `SourceMap` is serialised as its list of sources — each a `{ name, text }` — plus
the size ceiling. The derived state (each source's span, its id, and the global
high-water mark) is **not** stored: it is regenerated on load by replaying the
sources through the same insertion path as [`add`](#sourcemapadd). One consequence
is that deserialisation validates its input — overlapping ranges or a corrupt layout
cannot be smuggled in, because the layout is rebuilt rather than trusted — and a
source list whose combined length overruns the 32-bit space is a deserialisation
error rather than a broken map. The `max_source_len` field is optional on read
(defaulting to `u32::MAX`), so a map written by an older version still loads.

```rust,ignore
use source_lang::{BytePos, SourceMap};

let mut map = SourceMap::new();
map.add("main.rs", "fn main() {}").expect("fits");
map.add("util.rs", "let x = 1;\nlet y = 2;").expect("fits");

// Round-trips through any serde format; spans, ids, and resolution survive.
let json = serde_json::to_string(&map)?;
let restored: SourceMap = serde_json::from_str(&json)?;

assert_eq!(map, restored);
assert_eq!(restored.line_col(BytePos::new(23)).unwrap().1.line, 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

A [`SourceId`](#sourceid) serialises transparently as its `u32` index, so a handle
stored in an AST node or a cached diagnostic round-trips on its own.

---

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std`   | yes     | Pulls in the standard library and enables `span-lang/std`. Required for disk file-loading ([`add_file`](#sourcemapadd_file)); in-memory sources do not need it. |
| `serde` | no      | Derives `Serialize`/`Deserialize` for `SourceMap` and `SourceId`, and forwards to `span-lang/serde` for the coordinate types. See [Serialization](#serialization). |

Disabling `std` keeps the crate `no_std` (it always needs `alloc`):

```toml
[dependencies]
source-lang = { version = "0.2", default-features = false }
```

---

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
