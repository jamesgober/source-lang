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

[Unreleased]: https://github.com/jamesgober/source-lang/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/jamesgober/source-lang/compare/v0.4.0...v1.0.0
[0.4.0]: https://github.com/jamesgober/source-lang/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jamesgober/source-lang/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/source-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/source-lang/releases/tag/v0.1.0
