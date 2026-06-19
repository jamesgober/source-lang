# source-lang &mdash; Engineering Directives

> Engineering standards and the definition of done for this project. Read alongside `REPS.md` (root, authoritative) and `dev/ROADMAP.md` (current phase). If anything here conflicts with `REPS.md`, `REPS.md` wins.

---

## 0. Philosophy

This library is built and maintained to a production standard and treated as a flagship piece of work. Plan the full path, then build one verified step at a time. "Good enough" is treated as a defect. source-lang is the layer a multi-file compiler reads through: every span a diagnostic renders is resolved back to a file here, so its coordinate bookkeeping has to be exact.

---

## 1. What this is

source-lang manages the text a front-end reads. It loads files and in-memory buffers, gives each a stable id, lays them out in one global position space, and maps any global position back to the file and local offset it came from. It is the multi-file coordinate layer above raw spans: `span-lang` says *where in a buffer*, source-lang says *which buffer*. It owns source storage and coordinate mapping only — no lexing, no diagnostics rendering, no parsing.

---

## 2. Engineering law (non-negotiable)

- **Performance** — peak is the baseline; global-position → file lookup is `O(log files)`, never a linear scan; loading a source is a single read, not repeated re-reads; no steady-state hot-path allocation in the lookup path; no "faster" claim without `criterion` numbers.
- **Correctness** — the invariants in section 4 are covered by property tests, cross-checked against a naive linear-scan resolver.
- **Security** — file loading validates paths and sizes; a hostile or truncated input is a defined error, never UB; global-offset arithmetic cannot overflow into another file's range.
- **Architecture** — SOLID, KISS, YAGNI; one responsibility; the storage backend (file vs. in-memory buffer) sits behind one seam.
- **Cross-platform** — Linux/macOS/Windows first-class, verified by CI; path and line-ending handling is explicit.
- **Error handling** — every fallible path (missing file, oversize source, out-of-range global position) returns `Result`/`Option` per the documented contract.
- **Production-ready** — `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]` from the first commit; no stray `println!`/`dbg!`; every public item has rustdoc with a runnable example.

---

## 3. Definition of done

1. Compiles clean on Linux/macOS/Windows, stable and MSRV 1.85.
2. `fmt`, `clippy -D warnings`, `test --all-features`, `cargo doc -D warnings` clean.
3. `cargo audit` + `cargo deny check` pass.
4. No `unwrap`/`expect`/`todo!`/`dbg!` in shipping code.
5. A Tier-1 API exists and headlines the docs.
6. Property tests cover every section-4 invariant.
7. Hot-path changes carry benchmarks; no regression over 5%.
8. Docs and `CHANGELOG.md` updated; the matching `docs/release/vX.Y.Z.md` written before the tag.

---

## 4. Project-specific invariants

- Every added source receives a unique, stable `SourceId` that never changes for the life of the map.
- Sources occupy non-overlapping ranges in the global position space; no global offset belongs to two files.
- A valid global position maps to exactly one `(SourceId, local offset)`, and that mapping round-trips — property-tested against a naive scan.
- Looking up a position outside every source's range is a defined result, never a panic or out-of-bounds read.
- Global-position → file resolution is `O(log files)`; resolved file contents are borrowed from the map, never copied on the hot path.
