<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>source-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [1.0.2] - 2026-11-12

A documentation patch. No code changes.

### Changed

- **A `SourceMap` is documented as per compilation.** The map only grows: it
  keeps every source's text and never reuses a range of the 4 GiB position
  space, so every id and span it has handed out keeps resolving. The crate docs,
  the `SourceMap` rustdoc and `docs/API.md` now say so, and advise editors and
  language servers to build a fresh map for each revision they analyse. Adding
  every revision of a file to one long-lived map holds all of them in memory and
  ends in `SpaceExhausted` after 4 GiB of cumulative text.

---

## [1.0.1] - 2026-10-08

A patch release: no public API changes and no layout changes (every source's base
and every interior `locate` answer is identical to 1.0.0). It lets end-of-input
positions in the last source resolve (ISSUES H04), makes `line_col` independent of
file size (M20), and bounds file reads (M21).

### Fixed

- **The end of the last source now resolves to it (H04).** `SourceMap::locate` (and
  with it `line_col`) resolves the position equal to the end of the **last** source
  (the end of the global space, previously `None`) to that source, with a local
  offset equal to its length (`line_col`: one column past the last character).
  That is where a "found end of input" diagnostic points, so end-of-input errors in
  a single-source map (lang-forge's case) and in the most recently added source now
  resolve, and an empty last source is locatable. Nothing else changes: membership
  is still half-open, so an interior boundary still resolves to the later source.
- **`add_file` reads are bounded (M21).** The read stops one byte past
  `max_source_len`, so a source whose metadata reports no length (a pipe, a `/proc`
  file) or a file that grows while it is read can no longer load without bound. Such
  an input fails with `SourceMapError::Oversize`, whose `len` is then the number of
  bytes read before stopping (`max_source_len + 1`). The buffer is pre-sized from
  the metadata length, so a regular file is still read with one allocation.

### Changed

- **`line_col` no longer rescans the source (M20).** Each source's line-start table
  is computed once, when it is added (one pass, allocated at its exact size: 4 bytes
  per line, kept in a side table on the map; `SourceFile` is unchanged), and
  `SourceMap::line_col` is now `O(log files + log lines)` plus a character count
  within the located line. One million lookups over 16 files of 1,000 lines:
  14.164 s before, 28.783 ms after.
- `locate` binary-searches a dense array of start offsets (4 bytes per source)
  instead of the `SourceFile` records; numbers in `docs/release/v1.0.1.md`.
  `SourceFile::line_index` still builds a fresh `LineIndex` per call: `span-lang`
  0.4's `LineIndex` has no constructor that could adopt the cached table.

### Known limitations

- **End of input in a source that is not the last one (H04, remainder).** Sources
  are laid out contiguously, so the end of an earlier source is the same position as
  the next source's first byte, and resolves to the next source at `1:1`. Fixing it
  needs one reserved end position per source (rustc's layout), which shifts every
  later base and breaks code that predicts bases from lengths (repl-lang does), so
  it is semver-major and planned for **2.0**. An empty source that is not the last
  one remains unlocatable for the same reason.

---

## [1.0.0] - 2026-06-20

The stable release. No new public API — this ratifies the surface frozen in `0.4.0`
as the `1.0` contract, with a recorded SemVer promise and the full property-test and
benchmark suite verified on Linux, macOS, and Windows.

### Changed

- The public API is **stable** and follows Semantic Versioning: no breaking changes
  before `2.0`. The promise is recorded in [`docs/API.md`](docs/API.md#semver-promise);
  `SourceMapError` stays `#[non_exhaustive]` so new error variants remain minor
  changes, and the MSRV (Rust 1.85) only rises in a minor release.

---

## [0.4.0] - 2026-06-20

Optional `serde` support and the public-surface freeze. The source map can be
serialised and restored, and the API is now declared frozen for the rest of the 0.x
series — only documentation, tests, and internal work land before `1.0.0`.

### Added

- `serde::Serialize` / `serde::Deserialize` for `SourceMap` under the `serde`
  feature. The wire form is the list of sources plus the size ceiling;
  deserialisation rebuilds the map through the same insertion path as `add`, so
  spans, ids, and the high-water mark are regenerated and the non-overlap invariant
  holds even for hand-edited input. The ceiling field is optional on read for
  forward compatibility.
- `serde` support for `SourceId`, which serialises transparently as its `u32` index.
- `PartialEq` / `Eq` for `SourceMap`.

### Changed

- The public API surface is **frozen** as of this release. No public items will be
  added or changed before `1.0.0`; see [`docs/API.md`](docs/API.md).

---

## [0.3.0] - 2026-06-20

File loading and line integration. Sources can now be loaded from disk and from
raw byte buffers behind one set of checks, every bad input is a defined error, and
a global position resolves to file plus 1-based line/column in one step.

### Added

- `SourceMap::add_bytes` — validates raw bytes as UTF-8 and adds them as a source,
  the in-memory counterpart to file loading.
- `SourceMap::add_file` (feature `std`) — reads a file from disk, checking its size
  from metadata before any bytes are read, then validating UTF-8.
- `SourceMap::line_col` — resolves a global position to its source and a `LineCol`
  in one step, composing `locate` with `span-lang`'s line index.
- `SourceFile::line_index` — builds a reusable `span-lang` `LineIndex` over a
  source's text, for resolving many positions in one source without re-scanning.
- `SourceMap::set_max_source_len` / `max_source_len` — a configurable per-source
  size ceiling (default `u32::MAX`) that bounds how much one untrusted input loads.
- `SourceMapError` variants `Oversize`, `NotUtf8`, and `Io` (the last under `std`),
  each naming the source it concerns.
- `LineCol` and `LineIndex` re-exported from the crate root.
- File-loading integration tests (`tests/loading.rs`) covering a missing path, a
  directory, non-UTF-8 contents, and the size ceiling on a real filesystem.
- A property test checking `line_col` against per-file `LineIndex` resolution, and a
  `criterion` benchmark for the `line_col` path.

### Changed

- **Breaking:** `SourceMapError` no longer derives `Copy`. Its file-loading variants
  carry the offending source's name (a `Box<str>`), so the type is `Clone` but not
  `Copy`. It remains `#[non_exhaustive]`, so a `match` already needed a wildcard arm.

---

## [0.2.0] - 2026-06-20

Source storage and the global position space. This release implements the core the
scaffold was built for: a `SourceMap` that holds many sources, gives each a stable
id, lays them out across one global position space, and resolves a global position
back to its source and local offset in `O(log files)`.

### Added

- `SourceMap` — holds many sources end to end in one global position space, with
  `new`, `with_capacity`, `add`, `locate`, `source`, `len`, `is_empty`, and `iter`.
- `SourceId` — a stable, opaque, `Copy` handle to a source, valid for the life of
  the map.
- `SourceFile` — a stored source's name, owned text, and global `Span`.
- `SourceMapError` — `#[non_exhaustive]` error type; the `SpaceExhausted` variant
  reports a source that overruns the 32-bit global space.
- `span-lang` (0.4) wired and used: positions and spans come from it, and `BytePos`
  and `Span` are re-exported from the crate root.
- Property tests (`tests/properties.rs`) checking unique/stable ids, non-overlap,
  and global ↔ (id, local) round-trip against a naive linear-scan resolver.
- A `criterion` benchmark for `locate`, the resolution hot path.

### Changed

- `clippy.toml` MSRV aligned to `1.85` to match `Cargo.toml`, removing the clippy
  configuration-mismatch warning.

### Fixed

- `deny.toml` header named the wrong crate (a copy-paste leftover).

---

## [0.1.0] - 2026-06-18

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/source-lang/compare/v1.0.2...HEAD
[1.0.2]: https://github.com/jamesgober/source-lang/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/jamesgober/source-lang/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/jamesgober/source-lang/compare/v0.4.0...v1.0.0
[0.4.0]: https://github.com/jamesgober/source-lang/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jamesgober/source-lang/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/source-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/source-lang/releases/tag/v0.1.0
