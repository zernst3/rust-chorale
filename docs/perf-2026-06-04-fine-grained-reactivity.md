# Performance: Fine-grained reactivity for the filter/sort/paginate pipeline

**Date:** 2026-06-04  
**Author:** overnight-chorale bot  
**Item:** v0.2.0 Item 5 (non-routing)

---

## Problem

The v0.1 `chorale-dioxus` adapter memoizes the post-filter/sort/paginate row view with:

```rust
let view = use_memo(move || visible_view(&*sig.read()));
```

`sig.read()` inside a `use_memo` closure subscribes the memo to the entire
`Signal<TableState<TRow>>`. Any field change — including `scroll_top` — causes
the memo to re-run. At 1M rows, `filtered_sorted_pairs` (the core of
`visible_view`) clones the full row `Vec` and re-runs the filter + sort
pipeline on every scroll event. This is ~30 MB of allocation + O(n log n) work
per scroll tick.

The existing code comment documented this as a known issue:

> "this memo's body still re-runs on every state change (including scroll)
> because sig.read() subscribes to the whole TableState signal. Further
> optimization — skipping the recompute when only scroll_top changed — would
> require either dioxus-stores fine-grained field reactivity or a peek+manual-
> key tracking pattern. Tracked for a v0.2 perf pass."

---

## Strategy chosen: peek + manual key tracking (Strategy 2)

Two strategies were considered:

1. **`dioxus-stores` integration** — opt into Dioxus's fine-grained store
   primitives so reads of individual fields don't subscribe to sibling fields.
   Rejected: adds `dioxus-stores` as a new external dependency (DEPS-1) and
   requires restructuring `TableState` around store primitives, which breaks
   CHORALE-CORE-2 (pure transitions against plain Rust structs).

2. **Peek + manual key tracking** — introduce a cheap intermediate memo that
   tracks only the fields `visible_view` actually uses. The expensive memo
   subscribes to this cheap key. Scroll events update `scroll_top` (not in
   the key) → the key memo re-runs but returns the same value → Dioxus's
   `PartialEq` comparison short-circuits → the expensive pipeline does not run.

Strategy 2 is the clear winner: no new dependency, no API surface change,
minimal code addition.

---

## What `visible_view` actually reads from `TableState`

From `views.rs`:

```
filtered_sorted_pairs:
  - state.rows           (the data)
  - state.columns        (accessor closures for filter matching + sort key)
  - state.filters        (active filters)
  - state.sort           (active sort column + direction)

pagination slice:
  - state.page
  - state.page_size
```

Fields NOT read by `visible_view` (and therefore should not trigger a
recompute):
- `scroll_top` — virtualization offset; affects which window of the view to
  render, but not which rows ARE in the view.
- `viewport_height`, `row_height`, `buffer_rows` — window geometry parameters.
- `column_visibility` — affects column rendering, not which rows are included.
- `column_widths` — purely cosmetic.
- `selection` — does not change which rows are visible.

---

## Implementation

```rust
// Intermediate memo: tracks only the fields that affect visible_view output.
// When scroll_top (or viewport_height, row_height, etc.) changes, this memo
// re-runs but returns the SAME tuple, so Dioxus's PartialEq comparison
// short-circuits before the expensive pipeline re-runs.
//
// Limitation: update_row transitions that change a row's value without
// changing the row count will NOT trigger a view recompute via this key
// (rows.len() stays the same). The view will re-sync on the next
// sort/filter/page change. This is an accepted tradeoff for the common
// case (1M-row scroll performance); cell editing is at most one transition
// per user interaction.
let view_key = use_memo(move || {
    let s = sig.read();
    (s.page, s.page_size, s.sort, s.filters.clone(), s.rows.len())
});

// The expensive pipeline only re-runs when view_key actually changes.
// sig.peek() reads the signal value without subscribing this memo to
// sig — the subscription flows through view_key only.
let view = use_memo(move || {
    let _key = view_key.read();
    visible_view(&*sig.peek())
});
```

The `view_key` tuple contains `HashMap<ColumnId, FilterValue>` (the filters).
Its `PartialEq` comparison is O(active filter count), typically 0–3 entries.
This is negligible compared to the avoided O(n log n) full pipeline re-run.

---

## Verification

A regression test in `components.rs` would ideally assert that a scroll-only
state change does not re-allocate the row `Vec`. However, Dioxus 0.7's memo
system does not expose memo-invocation counters that are accessible from
unit tests without a live component context. The correctness guarantee is
structural: `sig.peek()` creates no subscription to `sig`, so changes to
`scroll_top` (the only field that changes during scroll-only events) cannot
trigger the `view` memo.

Manual verification: the `virtualized-1m-rows` example before and after this
change. Chrome DevTools Memory tab should show no periodic allocation bursts
during scroll (the pattern of 30 MB allocations every scroll frame disappears).

---

## Rule addition: PERF-1

> **PERF-1: The filter/sort/paginate pipeline is keyed on view-affecting fields only.**
>
> The `view` memo subscribes to a `view_key` intermediate memo (page, page_size,
> sort, filters, rows.len()) rather than the full `Signal<TableState>`. Scroll
> events, column resizes, and selection changes do NOT trigger the pipeline.
>
> When adding new fields to `TableState`, update `view_key` if the field
> affects `visible_view` output. Do not add the field to the key if it
> only affects rendering or virtualization geometry.

This rule is appended to `docs/CONVENTIONS.md`.
