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

[Unreleased]: https://github.com/jamesgober/source-lang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/source-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/source-lang/releases/tag/v0.1.0
