# Item 11.0b: Infinite Scroll Mode

## Problem

chorale v0.1.0 paginates: rows are divided into pages; the user navigates pages via
previous/next buttons and the "Go to page" input. Pagination is the right default for
large data sets with explicit navigation intent. But for browsing-heavy workflows — a
news feed, a task list, an activity log — users expect to scroll down and see more rows
appear, not to click "Next Page."

`leptos-struct-table` ships infinite scroll as a first-class mode. `table-rs` does not.
v0.2.0 chorale adds an `InfiniteScroll` mode that accumulates rows as the user scrolls,
coexisting with the existing `Pages` default (so v0.1.0 behavior is unchanged for all
existing callers).

The distinction from virtualization: virtualization (Items 6 and the existing v0.1.0 fixed
mode) renders only the rows in the viewport, with spacers above and below. Infinite scroll
controls how many rows are *loaded* into the in-memory post-filter-sort list. In
`InfiniteScroll` mode, the "loaded rows" cursor grows as the user scrolls toward the
bottom, simulating a server-side cursor without requiring a server-side data source.

## Proposed Public API

### `chorale-core`

```rust
/// Selects how the table exposes rows to the user.
#[non_exhaustive]
pub enum PaginationMode {
    /// Rows divided into fixed-size pages (v0.1.0 behavior, default).
    Pages,
    /// Rows accumulate as the user scrolls; no explicit page boundary.
    InfiniteScroll,
}

/// Added to `TableState` as an additive field. Defaults to `PaginationMode::Pages`.
pub pagination_mode: PaginationMode,

/// In InfiniteScroll mode, how many post-filter-sort rows are currently "loaded"
/// (visible to the user). Grows by `page_size` each time the scroll threshold is reached.
/// In Pages mode, unused (always 0).
pub loaded_row_count: usize,

/// Transitions:
/// Set the pagination mode. Resets `loaded_row_count` to `page_size` when switching to
/// InfiniteScroll (initial viewport); resets `current_page` to 0 when switching back to Pages.
pub fn set_pagination_mode(
    state: &TableState<TRow>,
    mode: PaginationMode,
) -> TableState<TRow>;

/// InfiniteScroll only: load the next batch of rows (equivalent to "the user scrolled
/// to the threshold"). Increases `loaded_row_count` by `page_size`, capped at the
/// total post-filter-sort row count.
pub fn load_more_rows(state: &TableState<TRow>) -> Result<TableState<TRow>, StateError>;

/// View function: in InfiniteScroll mode, returns the first `loaded_row_count`
/// post-filter-sort rows (not page-sliced). In Pages mode, behaves identically to
/// `visible_view`.
pub fn visible_view(state: &TableState<TRow>) -> Vec<Row<TRow>>;  // updated semantics
```

Note: `visible_view` is extended to handle InfiniteScroll mode. This avoids a proliferation
of view function names; the semantics are conditioned on `state.pagination_mode`.

### `chorale-dioxus`

No new props required for basic infinite scroll. The adapter detects
`state.pagination_mode == PaginationMode::InfiniteScroll` and:
1. Hides the pagination bar (page controls, "Go to" input).
2. Registers a scroll-event listener on the table container that fires `load_more_rows`
   when the user scrolls within `threshold_px` of the bottom of the rendered content.

Optional prop for scroll threshold:

```rust
/// Distance from bottom of rendered content (in px) at which `load_more_rows` fires.
/// Defaults to 200.0. Only relevant in InfiniteScroll mode.
pub infinite_scroll_threshold_px: f64,
```

Callsite shape:

```rust
let handle = use_chorale_table(...);
// Switch to infinite scroll mode in setup:
handle.update(|s| set_pagination_mode(s, PaginationMode::InfiniteScroll));

rsx! {
    Table {
        handle: handle,
        infinite_scroll_threshold_px: 300.0,
    }
}
```

## Internal Design

**`visible_view` branching:** the function checks `state.pagination_mode`. In `Pages`
mode, behavior is unchanged (slices `current_page * page_size .. (current_page+1) *
page_size` from the filtered-sorted list). In `InfiniteScroll` mode, returns
`filtered_sorted[0..loaded_row_count.min(filtered_sorted.len())]`.

**Scroll threshold detection (adapter):** on each scroll event, the adapter computes
`scroll_height - scroll_top - client_height` (the distance from the current scroll
position to the bottom of the content). When this value falls below
`infinite_scroll_threshold_px`, the adapter fires `load_more_rows`. A debounce (one
frame via `request_animation_frame`) prevents duplicate fires on rapid scroll events.

**Filter/sort change resets `loaded_row_count`:** when `set_filter` or `toggle_sort` is
called, `loaded_row_count` resets to `page_size`. This is consistent with the behavior
for Pages mode (filter change resets to page 0). The reset is applied inside the core
transition functions.

**Interaction with `to_csv`:** in InfiniteScroll mode, `to_csv` exports all post-filter-
sort rows (not just the loaded ones). This matches the Pages behavior where `to_csv`
exports all filtered-sorted rows, not just the current page.

**Interaction with `set_page` / `set_page_size` in InfiniteScroll mode:** `set_page`
returns `Err(StateError::InvalidModeForTransition)` in InfiniteScroll mode (the concept
of "page number" doesn't apply). `set_page_size` is permitted in InfiniteScroll mode
and updates the batch size for future `load_more_rows` calls.

## Backwards Compatibility

`pagination_mode: PaginationMode` and `loaded_row_count: usize` are additive fields on
`TableState`. `TableState` is `#[non_exhaustive]` in v0.1.0, so cross-crate callers
cannot use struct-literal construction; adding these fields does not break compilation
downstream.

`PaginationMode` is a new `#[non_exhaustive]` enum defaulting to `PaginationMode::Pages`.
All existing callers use `Pages` behavior implicitly.

**`visible_view` semantics change in InfiniteScroll mode is NOT a breaking change for
v0.1.0 callers**, because `pagination_mode` defaults to `Pages` and the `Pages` branch
is unchanged. Callers that explicitly opt into `InfiniteScroll` mode are new callers
choosing the new semantics.

**`set_page` returning `Err` in InfiniteScroll mode:** callers that call `set_page` on
a state that is always in `Pages` mode (the default) are unaffected. The error variant
`StateError::InvalidModeForTransition` is new; `StateError` is `#[non_exhaustive]` in
v0.1.0, so adding a variant is non-breaking.

## Test Plan

Per TESTS-1:

- `set_pagination_mode(InfiniteScroll)`: `pagination_mode == InfiniteScroll`;
  `loaded_row_count == page_size`; `current_page == 0`.
- `set_pagination_mode(Pages)` from InfiniteScroll: `pagination_mode == Pages`;
  `loaded_row_count == 0`; `current_page == 0`.
- `load_more_rows` in InfiniteScroll: `loaded_row_count` increases by `page_size`.
- `load_more_rows` at total row count: `loaded_row_count` capped at total; no overflow.
- `load_more_rows` in Pages mode: returns `Err(StateError::InvalidModeForTransition)`.
- `visible_view` in InfiniteScroll: returns `loaded_row_count` rows, not a page slice.
- `visible_view` in Pages: unchanged behavior.
- Filter change resets `loaded_row_count` to `page_size` in InfiniteScroll mode.
- Sort change resets `loaded_row_count` to `page_size` in InfiniteScroll mode.
- `set_page` in InfiniteScroll: returns `Err(StateError::InvalidModeForTransition)`.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **`visible_view` semantics branch vs a separate `visible_scroll_view` function.**
   Recommendation: branch inside `visible_view` — one function with consistent naming
   across modes. A separate function would require callers to switch call sites when
   toggling mode at runtime; branching is invisible to the caller.

2. **`loaded_row_count` initial value when entering InfiniteScroll: `page_size` vs 1
   vs 0.** Recommendation: `page_size` — same as the first "page" a user would see in
   Pages mode. Starting at 0 would render an empty table before the first scroll event.

3. **Should `load_more_rows` take a `count` parameter (load N rows) or always use
   `page_size`?** Recommendation: always `page_size`. Callers who want a different
   batch size change `page_size` before entering InfiniteScroll mode. A `count` parameter
   adds complexity without a clear use case in v0.2.0.

4. **Interaction with virtualization: does InfiniteScroll mode disable fixed-height
   virtualization?** Recommendation: no — the two are orthogonal. `visible_view` returns
   the loaded rows; `visible_window` then virtualizes that slice. The scroll-threshold
   detector must account for the virtual window's `bottom_pad_px` when computing
   "distance from bottom." This is an adapter implementation detail.

5. **Should the "loading" state (between threshold hit and `load_more_rows` completing)
   show a spinner?** Recommendation: optional prop `infinite_scroll_loading_indicator:
   Option<Element>`. Rendered at the bottom of the table between threshold hit and the
   next render cycle with the new rows. Zach should decide if this is v0.2.0 or v0.3.
