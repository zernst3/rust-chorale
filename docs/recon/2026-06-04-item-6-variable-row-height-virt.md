# Item 6: Variable-Row-Height Virtualization

## Problem

chorale v0.1.0 ships fixed-row-height virtualization via `visible_window(scroll_top,
viewport_height, row_height) -> VirtualWindow`. Every row is assumed to occupy exactly
`row_height` pixels. This assumption breaks when rows contain multi-line text, expandable
detail sections, or user-supplied cell renderers that vary in height based on content.

A v0.2.0 user who renders a `RenderKind::Custom` cell with variable content today has two
options: clamp all rows to the tallest possible height (wasted whitespace) or disable
virtualization entirely (OOM risk at 10k+ rows). Neither is acceptable once the data set
is large enough to require virtualization.

`table-rs` ships no virtualization at all. `leptos-struct-table` ships fixed-height
virtualization identical to chorale v0.1.0. Variable-height support is a differentiator
for both frameworks. TanStack Virtual (the JS reference implementation) uses a
measure-and-cache approach: row heights are unknown until the DOM renders them, so the
algorithm measures each rendered row, caches the result by index, and recomputes offsets
lazily.

## Proposed Public API

### `chorale-core`

```rust
/// Cached per-row height measurements. Keyed by row index (not RowId) because
/// the index is stable within a single `visible_view` page; RowId would require
/// a HashMap lookup per row on every window computation.
///
/// Added to `TableState` as an additive field (non-breaking; see §Backwards Compatibility).
pub row_heights: HashMap<usize, f64>,

/// New transition: record a measured height for a rendered row.
/// Returns a new TableState with the cache updated.
pub fn record_row_height(state: &TableState<TRow>, index: usize, height: f64) -> TableState<TRow>;

/// New transition: invalidate the height cache (call on data reload or filter change).
pub fn clear_row_height_cache(state: &TableState<TRow>) -> TableState<TRow>;

/// Variable-height virtual window. When `row_heights` is populated for the visible
/// range, uses prefix-sum offsets; for unmeasured rows, falls back to `default_row_height`.
pub fn visible_window_variable(
    state: &TableState<TRow>,
    scroll_top: f64,
    viewport_height: f64,
    default_row_height: f64,
) -> VirtualWindow;
```

`visible_window` (fixed-height, v0.1.0) is retained unchanged. Callers choose which
function to call; no signature is removed.

### `chorale-dioxus`

```rust
/// New prop on `Table`. When `Some`, the component measures each rendered row
/// after mount/update via a ResizeObserver and calls `record_row_height`.
/// When `None`, fixed-height path (v0.1.0 behavior, unchanged).
pub variable_row_height: bool,  // defaults to false

/// If `variable_row_height` is true, `row_height` is used as the estimate for
/// unmeasured rows (same field, repurposed as default/fallback).
```

Callsite shape:

```rust
rsx! {
    Table {
        handle: handle,
        variable_row_height: true,
        row_height: 40.0,       // fallback until rows are measured
        // ... other props unchanged
    }
}
```

## Internal Design

**Offset cache:** `visible_window_variable` builds a prefix-sum array on the fly from
`state.row_heights` for the post-filter-sort row count. For index `i`, the top offset is
`Σ height(j) for j in 0..i` where `height(j)` returns `row_heights.get(&j).copied()
.unwrap_or(default_row_height)`. Binary search over the prefix-sum array locates the
first visible row for a given `scroll_top`.

The prefix-sum is not cached in `TableState` (that would couple state to the total row
count in a way that becomes stale on filter change). It is computed in-function, O(n)
over the total post-filter row count. At 100k rows this is ~800 µs; acceptable. If
profiling shows cost above 2 ms at typical data sizes, a `row_offset_cache: Vec<f64>`
field can be added to `TableState` as a subsequent non-breaking addition.

**Measurement loop (adapter):** After each render, the Dioxus component iterates its
rendered row refs and fires a ResizeObserver (or reads `getBoundingClientRect`) for each.
Measurements are batched into a single `record_row_height` call per frame to avoid
repeated signal writes. The adapter uses `use_effect` to wire measurement to post-render.

**Scroll restoration:** when `clear_row_height_cache` is called (filter or sort change),
the scroll position resets to 0 to avoid a stale offset landing outside the new virtual
bounds.

**Interaction with Item 8 (grouping):** group-header rows will have a different height
than data rows. `row_heights` is keyed by render index (position in the interleaved
group+data list), so group headers are measured alongside data rows with no special
handling required.

## Backwards Compatibility

The new field `row_heights: HashMap<usize, f64>` is additive. `TableState` is
`#[non_exhaustive]` in v0.1.0, so cross-crate callers cannot use struct-literal
construction; adding the field does not break compilation downstream. The field defaults
to an empty `HashMap` (via a `Default`-derived construction in `TableState::new`), so
existing callers that pass `TableState::new(rows, columns)` see no change.

`visible_window` (fixed-height) is not modified. Variable-height is opt-in via a separate
function. Existing callers of `visible_window` are unaffected.

The `variable_row_height` prop on `Table` defaults to `false`. Existing call sites with no
`variable_row_height` prop use the v0.1.0 fixed-height path unchanged (per ROBUSTNESS-1).

## Test Plan

Per TESTS-1, every new transition is unit-tested:

- `record_row_height`: verify the returned state has the updated HashMap entry and all
  other fields are unchanged.
- `clear_row_height_cache`: verify the returned state has an empty `row_heights` map.
- `visible_window_variable` with all rows measured: assert `start_index`, `end_index`,
  `top_pad_px`, `bottom_pad_px` match hand-computed prefix-sum values.
- `visible_window_variable` with partial measurements (some rows use fallback): assert
  the window is computed correctly for the mixed-height case.
- Boundary: all rows same height as `default_row_height` → result identical to
  `visible_window` for the same inputs.
- Edge: `scroll_top` beyond the total content height → returns last valid window.
- Edge: 0-row table → returns `VirtualWindow { start: 0, end: 0, top_pad_px: 0.0, bottom_pad_px: 0.0 }`.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **Function name: `visible_window_variable` vs a unified `visible_window` with an
   optional `row_heights` param vs a new struct `VirtualizerConfig`.** Recommendation:
   keep as a separate `visible_window_variable` function to preserve the simplicity of
   the v0.1.0 fixed-height call for callers who don't need variable heights. A unified
   overload would require an `Option<&HashMap<usize, f64>>` parameter that is always
   `None` for 99% of callers.

2. **Key type for `row_heights`: row index (usize) vs `RowId`.** Recommendation: row
   index, because the index is stable within a rendered page and avoids a per-row
   `RowId` lookup during the prefix-sum computation. Trade-off: the cache is invalidated
   by any sort/filter/page change (indices shift), so `clear_row_height_cache` must be
   called on those transitions. The adapter can wire this automatically.

3. **Measurement API: ResizeObserver vs `getBoundingClientRect` in `use_effect`.**
   Recommendation: `getBoundingClientRect` in `use_effect` post-render, wrapped in a
   `request_animation_frame` to ensure layout has settled. ResizeObserver is more
   correct for dynamic content but requires holding a JS object across renders; deferred
   to v0.3 if needed.

4. **Should `clear_row_height_cache` be called automatically inside `set_filter`,
   `toggle_sort`, and `set_page` transitions?** Recommendation: yes, call it implicitly
   inside those transitions so callers don't have to remember. Precedent: `set_filter`
   already resets `current_page` to 0; the same logic applies to the height cache.
   This is an internal-transition concern (no public API change).

5. **Scroll-position reset on cache clear: reset to 0 in core or in the adapter?**
   Recommendation: the adapter owns scroll position (it lives in the DOM, not in
   `TableState`), so the adapter resets `scroll_top` to 0 when it detects a cleared
   cache. Core should not manage DOM state.
