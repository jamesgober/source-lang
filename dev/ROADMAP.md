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
