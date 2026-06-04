# chorale Conventions

This document is the canonical rule library for the chorale table library. It is **scoped to this repository only** and is decoupled from the agora-rs port. Citation discipline mirrors agora-rs CONVENTIONS: every rule has a stable ID; commits cite the IDs they apply.

The overnight routine MAY add new rules under the clear-winner test (a documented convention beats none; a new clear winner is documented in this file, cited in the commit, and appended to `.overnight-chorale-auto-calls-ledger.md`). Structural / topology / public-API changes ROUTE to Zach.

---

## CC-1: Cite rule IDs in commits

Every commit that applies a convention from this file MUST cite the rule ID in the commit body. Format:

```
feat(core): add TableState pagination

Applied CHORALE-CORE-1 (pure logic, no UI deps) and CHORALE-CORE-2
(state transitions are pure functions). Chose Vec<RowId> over
HashSet<RowId> for selection because ordering is preserved for CSV export.
```

---

## CHORALE-CORE-1: chorale-core has zero UI / framework deps

`chorale-core` is pure logic. It MAY depend on `serde`, `thiserror`, `rust_decimal`, etc. It MAY NOT depend on `dioxus`, `leptos`, `yew`, `egui`, or any rendering framework. Adapters live in their own crate (e.g. `chorale-dioxus`).

**Why:** the load-bearing architectural wedge against `table-rs` and similar wrapper-style libraries. Anyone wanting a Leptos adapter writes `chorale-leptos` against the same core; no fork needed.

**How to apply:** if a feature seems to require framework awareness, refactor until the framework-specific piece lives in the adapter and the core operates on pure data + closures.

---

## CHORALE-CORE-2: state transitions are pure immutable functions

State transitions on `TableState<TRow>` take an immutable `&TableState<TRow>` and return a fresh `TableState<TRow>` (or a `Result<TableState<TRow>, _>` for fallible transitions). They do NOT mutate the receiver, do NOT spawn tasks, do NOT take signals, and do NOT call into any rendering layer. Every transition must be unit-testable without any framework runtime.

```rust
// ✓ correct
pub fn toggle_sort(state: &TableState<TRow>, col: ColumnId) -> TableState<TRow> { ... }
pub fn set_page(state: &TableState<TRow>, page: usize) -> Result<TableState<TRow>, StateError> { ... }

// ✗ incorrect — mutable receiver
pub fn toggle_sort(&mut self, col: ColumnId) { ... }
```

**Why:** matches the TanStack Table convention chorale is positioning against ("TanStack Table for Rust"). TanStack's whole API is immutable-state-returns-new (`setSorting`, `setFiltering`, etc.) so reactive systems can compare old-and-new by value-equality and trigger renders only when state actually changes. With a `&mut self` signature, Dioxus `Signal` (and Leptos `RwSignal`) can't tell whether the state changed because the mutation happens in-place — they'd over-render. The immutable signature gives reactivity systems first-class change detection without leaking signal types into core.

A second consequence: time-travel debugging and undo/redo come almost for free. `Vec<TableState>` is the history; no serialize/diff machinery required.

**How to apply:** if you find yourself wanting to write `&mut self` on a state transition, stop. Convert the signature to `&TableState<TRow> -> TableState<TRow>`. The cost (one struct clone per transition) is negligible for a typical table state and is paid for by the cleaner reactive integration.

Established in the recon-1 differentiation memo (2026-05-31). The original CHORALE-CORE-2 specified `&mut self`; revised here to immutable-functional to match the TanStack Table positioning the memo locks in.

---

## CHORALE-DIOXUS-1: adapter owns rendering only

`chorale-dioxus` consumes `chorale_core::TableState<TRow>` (or a `Signal<TableState<TRow>>`) and renders. It MUST NOT duplicate state-mutation logic that belongs in core. State mutations go through core functions; the adapter wires them to events.

**Why:** prevents drift between adapters. If a Leptos adapter is added, the same `TableState::toggle_sort(col)` works there; only the rendering and event wiring differ.

---

## ROBUSTNESS-1: explicit > terse

Default toward explicit, robust code over clever-terse code. Examples:

- `RowId(Uuid)` newtype instead of bare `Uuid` for row keys.
- Distinct error variants per failure mode in `thiserror` enums, not a catch-all `Other(String)`.
- Named struct fields over multi-tuple types past two fields.

**Why:** AI agents and future-Zach both benefit from code that names its invariants. Boilerplate cost is low when AI writes it; debugging cost on cryptic code is high.

---

## API-1: public surface routes to Zach

Any change to a `pub` item in `chorale-core/src/lib.rs` or `chorale-dioxus/src/lib.rs` re-exports — or to a type re-exported from there — ROUTES to Zach. The overnight routine logs a pending decision to `.overnight-chorale-decisions_needed.md` and sets the pause flag.

**Why:** public-API surface of a library crate is a one-way door. Renames, signature changes, and removed items break every downstream consumer once published. Worth a Zach review per change.

**How to apply:** internal types may evolve freely. The moment a type is `pub use`'d from lib.rs (or the module path that lib.rs re-exports from), changes to it route.

---

## TESTS-1: state transitions are tested

Every pure state transition on `TableState<TRow>` MUST have a unit test that asserts the resulting state. Coverage gate: if you can't write a test, the transition isn't pure, and CHORALE-CORE-2 has been violated — fix that first.

---

## DEPS-1: minimal external deps

Prefer the standard library. Reach for a crate only when the standard library is genuinely insufficient. Document the choice in the commit body when adding any new workspace dep.

**Why:** library crates with thin dep trees compile fast and avoid downstream version conflicts.

---

## VIRT-1: fixed-row-height virtualization in v0.1

v0.1 ships fixed-row-height virtualization. The core exposes `visible_window(scroll_top, viewport_height, row_height) -> (start_index, end_index, top_pad_px, bottom_pad_px)`. The adapter renders the slice `[start_index..end_index]` plus top and bottom spacer divs.

Variable-row-height virtualization (measure-and-cache) is **deferred to v0.2** per the v0.1 scope memo. Do not attempt it inside v0.1 without a ROUTE-1 stop.

**Why:** keeps v0.1 tractable while still shipping the canonical feature that distinguishes chorale from `table-rs`.

---

## PERF-1: fine-grained reactivity for the filter/sort/paginate pipeline

The `view` memo in `chorale-dioxus` subscribes to a `view_key` intermediate
memo (page, page_size, sort, filters, rows.len()) rather than the full
`Signal<TableState>`. Scroll events, column resizes, and selection changes
do NOT trigger the expensive filter+sort+paginate pipeline.

**Why:** at 1M rows, a full pipeline re-run on each scroll event allocates ~30 MB
per tick and runs O(n log n) sort work unnecessarily. The two-level memo pattern
keeps the common scroll path at O(1) (only the virtual-window geometry changes).
See `docs/perf-2026-06-04-fine-grained-reactivity.md` for full rationale.

**How to apply:** when adding new fields to `TableState`, update the `view_key`
memo in `components.rs` if the field affects `visible_view` output. Do NOT add
fields that only affect rendering or virtualization geometry (e.g. `scroll_top`,
`viewport_height`, `column_widths`).

**Known limitation:** `update_row` transitions that change a row's value without
changing `rows.len()` do not trigger an immediate view recompute via `view_key`.
The view re-syncs on the next sort/filter/page change. Acceptable tradeoff for
the common-case scroll performance gain.

Established 2026-06-04, v0.2.0 Item 5. Auto-call under clear-winner test
(Strategy 2 beats Strategy 1 on DEPS-1 and CHORALE-CORE-2; no public API
surface change).

---

## Adding new rules

New rule IDs follow the pattern `<SCOPE>-<TOPIC>-<NUMBER>` (e.g. `CHORALE-CORE-3`, `CHORALE-DIOXUS-2`, `ROBUSTNESS-2`). Append the rule to the appropriate section, write the auto-calls ledger entry, cite the ID in the commit that first applies it.
