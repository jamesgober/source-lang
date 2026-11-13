# source-lang — Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

---

## v0.1.0 — Scaffold (DONE)

Compiles, CI green, structure correct, no domain logic.

- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt.
- [x] API surface sketched in `docs/API.md`.

---

## v0.2.0 — Source storage & global position space (THE HARD PART, NOT DEFERRED)

Wire the `span-lang` dependency and build the core: a `SourceMap` that holds many
`SourceFile`s, each with a stable `SourceId`, laid out across one global position
space. The hard part, front-loaded, is the coordinate bookkeeping — assigning each
source a non-overlapping global range and resolving a global position back to its
`(SourceId, local offset)` in `O(log files)`. Get this wrong and every multi-file
diagnostic points at the wrong place, so it is proven now, not after the easy
load/store surface.

Exit criteria:
- [ ] `span-lang` wired and used (not an unused dependency).
- [ ] Every public item has rustdoc + a runnable example.
- [ ] Non-overlap, unique-id, and global↔(id, local) round-trip property-tested against a naive linear-scan resolver.

---

## v0.3.0 — File loading & line integration

Load sources from disk and in-memory buffers behind one seam, with defined errors
for missing/oversize/invalid input, and integrate `span-lang`'s line index so a
global position resolves to file + line/column in one step.

Exit criteria:
- [ ] Missing/oversize/truncated inputs return defined errors, boundary-tested.
- [ ] Global position → (file, line, column) verified against per-file resolution.

---

## v0.4.0 — serde, feature freeze

Optional `serde` for the source-map metadata (ids and ranges, not necessarily
contents) and a declared frozen public surface.

Exit criteria:
- [ ] `serde` round-trips the map metadata under the feature.
- [ ] API surface documented as frozen in `docs/API.md`.

---

## v1.0.0 — API freeze

The source-map/coordinate surface is stable and frozen until 2.0. No new public
API, only documentation, tests, and internal optimisation.

Exit criteria:
- [ ] `docs/API.md` marked stable; SemVer promise recorded.
- [ ] Full property-test and benchmark suite green on all three platforms.

---

## v1.0.1 — Patch: end-of-input in the last source, cached lines, bounded reads (DONE)

No public API change and no layout change. Fixes from the issue ledger:
 H04 (the non-breaking part), M20, M21.

Delivered:
- [x] **H04 (non-breaking part)** `locate` / `line_col`: the end of the **last**
  source (the end of the space, previously `None`) resolves to it at its length, so
  end-of-input diagnostics in a single-source map and in the most recently added
  source resolve, and an empty last source is locatable. Every base and every other
  answer is identical to 1.0.0 (property `positions_before_the_end_are_unchanged_from_1_0_0`
  checks it against the 1.0.0 half-open rule). Tests `test_h04_*`, including the
  lang-forge case and a test pinning the known limitation below; the old boundary
  test is kept, with a comment on the one changed assertion.
- [x] **M20** line-start table computed once in `push` (exact-size allocation, side
  table on the map, private module `src/lines.rs`); `SourceMap::line_col` is
  `O(log files + log lines)` + one line's character count, pinned equal to
  `LineIndex::line_col` by unit and property tests. Bench `line_col_1m`.
- [x] `locate` searches a dense `starts: Vec<u32>`. An intermediate design that
  stored the line table inside `SourceFile` made `locate` 18–40% slower (larger
  records in the binary search) and was dropped.
- [x] **M21** `add_file` reads through `take(max + 1)`; over-long streams are
  `Oversize { len: max + 1 }`. Tested with an endless reader, exact-ceiling inputs
  with wrong hints, a zero ceiling, and (Linux CI) `/proc/self/status`.
- [x] Consumers checked against the patched pair (read/test only): repl-lang,
  module-lang, lexer-lang, lang-forge all pass unchanged.

Dependency wiring: unchanged (`span-lang = "0.4"`). No new dependencies.

Moved to 2.0 (anti-deferral record):
- **H04 remainder: end of input in a source that is not the last one.** Reason: in
  the contiguous layout that position *is* the next source's first byte, so no
  lookup rule can tell them apart without mislocating one of the two. The fix is one
  reserved end position per source (rustc's layout). It was built and gate-tested
  for 1.0.1, but the consumer check showed it breaks repl-lang (which predicts entry
  bases by summing lengths: wrong `Input::base`, `Entry::span`, and diagnostics after
  the first entry), so it is semver-major. The finished work is saved as Wave M:
  The planning notes and `M-source-lang-gap-layout.patch`.
- `SourceFile::line_index` still rebuilds a `LineIndex` per call. Serving it from
  the cached table needs a `LineIndex` constructor from precomputed line starts in
  `span-lang` (not in 0.4, and the 0.4 pin is family-wide). Additive, so a 1.1
  minor once span-lang offers it.
