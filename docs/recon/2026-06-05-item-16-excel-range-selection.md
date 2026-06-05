# Item 16: Full Excel-Parity Range Selection (Multi-Rect + Fill Handle)

## Problem

chorale v0.2.0 ships row-level selection (checkboxes, `state.selection: Vec<RowId>`) from
the v0.1.0 baseline, but has no concept of a cell range. A "range" is a rectangular span of
cells — the fundamental unit of bulk operation in every spreadsheet and data-grid tool. Without
a range model, copy/paste (Item 17), Delete-to-clear, Ctrl+D fill-down, and fill-handle
drag-to-fill are all impossible to wire consistently.

AG Grid ships its range selection API (`CellRangeType`, `addCellRange`, `clearRangeSelection`)
as one of its most-used features — it appears in the top-5 community questions by volume.
MUI X DataGrid ships `selectedCellsInfo` and Shift+click range extension. TanStack Table
delegates range selection to the host app (it has no built-in range model), which is chorale's
current posture. Excel's range model is the universal reference implementation: drag-to-select,
Shift+click extend, Ctrl+click disjoint rect, Ctrl+A select-all, fill handle, header-click
whole-column/row.

This item establishes the range model that Items 17 (clipboard) and future items (aggregation
in status bar, conditional formatting) will consume. The design must handle multi-rect
disjoint selections (Ctrl+click), which are useful for visual selection and Delete-to-clear
but deliberately error on clipboard copy (matching Excel and AG Grid behavior).

## Proposed Public API

### `chorale-core`

```rust
/// The full range selection state.
/// Added to `TableState` as an additive field; defaults to `vec![]` on `TableState::new`.
pub range_selection: Vec<RangeSelection>,

/// A single contiguous rectangular range of cells.
/// `anchor` is where the selection began; `focus` is where it currently ends.
/// Rows are post-filter, post-sort visible indices. Columns are ColumnIds.
/// Either corner may have a larger index than the other — (3, "b") anchor + (1, "a")
/// focus represents the same rect as the reverse; consumers normalize via `normalized()`.
#[non_exhaustive]
pub struct RangeSelection {
    pub anchor: (usize, ColumnId),
    pub focus: (usize, ColumnId),
}

impl RangeSelection {
    /// Returns (min_row, max_row, ordered_columns) for iteration.
    pub fn normalized(&self, column_order: &[ColumnId]) -> NormalizedRange;
}

/// The resolved rectangular extent of a range.
pub struct NormalizedRange {
    pub min_row: usize,
    pub max_row: usize,
    /// Ordered left-to-right per column_order.
    pub columns: Vec<ColumnId>,
}

/// State transitions (pure, per CHORALE-CORE-2):

/// Begin a new range selection anchored at the given cell.
/// Replaces any existing range_selection.
pub fn start_range_selection(
    state: &TableState<TRow>,
    anchor_row: usize,
    anchor_col: ColumnId,
) -> TableState<TRow>;

/// Extend the last (active) range so its focus moves to the given cell.
/// If range_selection is empty, this is equivalent to start_range_selection
/// with the same cell as both anchor and focus.
pub fn extend_range_to(
    state: &TableState<TRow>,
    row_idx: usize,
    col: ColumnId,
) -> TableState<TRow>;

/// Add a disjoint range (Ctrl+click or Ctrl+Shift+arrow).
/// Appends a new RangeSelection with anchor == focus == the given cell.
/// Subsequent extend_range_to calls extend this newly-added range.
pub fn add_disjoint_range(
    state: &TableState<TRow>,
    anchor_row: usize,
    anchor_col: ColumnId,
) -> TableState<TRow>;

/// Select all visible rows × all visible columns (Ctrl+A).
/// Replaces any existing range_selection with a single range spanning all cells.
pub fn select_all(state: &TableState<TRow>) -> TableState<TRow>;

/// Clear all ranges (Escape key when no editor is open).
pub fn clear_range_selection(state: &TableState<TRow>) -> TableState<TRow>;

/// Fill the active single-rect range downward from its top row
/// (also used by fill-handle drag and Ctrl+D).
/// Source is the top row of the range; fills down through the remaining rows.
/// Errors if range_selection is empty, has more than one rect, or the range
/// is a single row (nothing to fill).
pub fn fill_down(
    state: &TableState<TRow>,
) -> Result<TableState<TRow>, RangeError>;

/// Fill the active single-rect range in a given direction via the fill handle.
/// Applies arithmetic progression detection: if source is [1, 2, 3], fill
/// extends with step=1. If source is a single value, repeats it.
/// Direction: Down (vertical) or Right (horizontal) only; Up/Left are not
/// supported in v0.2.0 (match Excel: you can drag the fill handle any direction
/// but Up/Left reduces the range rather than filling).
pub fn fill_handle_extend(
    state: &TableState<TRow>,
    direction: NavDirection,
    new_extent: usize,       // new max_row (Down) or new max_col_idx (Right)
) -> Result<TableState<TRow>, RangeError>;

/// Errors for range operations.
#[non_exhaustive]
pub enum RangeError {
    NoRangeSelected,
    MultiRectNotSupportedForThisOperation,
    RangeTooSmallToFill,
    IndexOutOfBounds,
}
```

### Fill-handle UI (adapter)

```rust
// The adapter renders a small draggable square in the bottom-right corner of the
// active range's bounding box when exactly one RangeSelection is present.
// On drag start: no state change (the current range is the source).
// On drag move: adapter calls fill_handle_extend with the dragged row/col as new_extent.
// On drag end: the state produced by fill_handle_extend is committed.
//
// The fill handle element is:
//   <div class="chorale-fill-handle" style="--chorale-fill-handle-size: 6px;" />
// CSS variable --chorale-fill-handle-color (default: #0078d4) is user-overridable.
```

### Callsite shape

```rust
// Typical host: no extra wiring needed.
// Range selection is driven entirely by mouse and keyboard handlers in the adapter.
// Hosts read the range for custom status bars:

let state = handle.signal().read();
let total_cells: usize = state.range_selection.iter()
    .map(|r| {
        let n = r.normalized(&state.effective_column_order());
        (n.max_row - n.min_row + 1) * n.columns.len()
    })
    .sum();
// display "3 cells selected" or "12 rows × 4 columns"
```

## Internal Design

**Data model:** `Vec<RangeSelection>` where each element is an anchor+focus pair of
`(usize, ColumnId)`. The active range (the one being extended by Shift+arrow or drag) is
always the last element of the Vec. Ctrl+click adds a new element; subsequent Shift+arrow
extends it. This mirrors the Excel mental model: the last-added range is the "hot" one.

**Normalization:** `anchor` and `focus` are stored as the user set them (potentially
"backwards" — anchor below focus). Consumers call `normalized()` which produces `(min_row,
max_row, columns_in_order)`. This avoids normalizing on every intermediate Shift+arrow
extension (O(1) store, normalize only when reading).

**Ctrl+Shift+Arrow extend-to-edge:** the adapter reads the active cell (Item 15) as the
current anchor reference, then calls `move_active_cell_to_edge` to find the target, then
calls `extend_range_to` with that target. This chains the two APIs without adding a new
transition. No core change needed.

**Select-all (Ctrl+A):** `select_all` produces a single `RangeSelection` with `anchor = (0,
first_column_id)` and `focus = (visible_row_count - 1, last_column_id)`. If called again
while all cells are selected, it is idempotent.

**Header click:**
- Column header click → `start_range_selection(0, column_id)` then `extend_range_to(visible_row_count - 1, column_id)`.
- Row header click → `start_range_selection(row_idx, first_column_id)` then `extend_range_to(row_idx, last_column_id)`.
- These produce single-column or single-row ranges, which are valid single-rect selections.

**Interaction with grouping (Item 8):** group header rows are not data rows; they do not
have a `row_idx` in the visible-row index. The adapter skips group header rows when computing
range extents. A drag that starts above a group header and ends below it selects the data rows
on both sides but excludes the group header row. `normalized()` receives only data-row indices.

**Interaction with active cell (Item 15):** `start_range_selection` sets the active cell to
the anchor as a side effect (one combined state return). Arrow without Shift calls
`move_active_cell` + `clear_range_selection` (collapses range to single cell = the new active
cell). Arrow with Shift calls `extend_range_to(new_active_row, new_active_col)` — the active
cell moves to the new focus position while keeping the anchor fixed.

**Auto-scroll on drag:** when a drag-to-select leaves the viewport boundary, the adapter
fires a `requestAnimationFrame` loop that scrolls the viewport by a fixed step (e.g. 8px /
frame) and calls `extend_range_to` with the extrapolated row/column. This is adapter logic;
core is stateless.

**Fill handle arithmetic detection algorithm:**
- Source range is a single column of N values.
- If all values are `CellValue::Number` and successive differences are equal (constant step),
  fill with step. E.g. `[1, 3, 5]` → step=2.
- If N=1 or no constant step detected, repeat the source values cyclically.
- Date/day-name/month-name progressions are **explicitly deferred to v0.3.0**.
- String values always repeat (no pattern detection).

## Backwards Compatibility

`range_selection: Vec<RangeSelection>` is an additive field on `TableState`. `TableState` is
`#[non_exhaustive]` in v0.1.0, so no cross-crate struct-literal construction breaks. The field
defaults to an empty `Vec` in `TableState::new`, so existing callers see no behavioral change;
an empty `range_selection` means no range is active, and all range-consuming code (Item 17
clipboard, Delete-to-clear) early-returns on an empty range.

`RangeSelection`, `NormalizedRange`, and `RangeError` are new types marked `#[non_exhaustive]`.
No existing matches break.

`RangeError` being `#[non_exhaustive]` means that when new error variants are added in v0.3.0,
cross-crate match arms require a wildcard arm already — non-breaking.

The fill-handle UI element and header-click range selection behaviors are additions to the
adapter's rendering and event handling. They do not alter existing prop signatures.

## Test Plan

Per TESTS-1:

**`start_range_selection` (~4 tests):**
- Happy path: `range_selection` becomes `vec![RangeSelection { anchor: (r, c), focus: (r, c) }]`.
- Replaces existing selection.
- Out-of-bounds row → `RangeError::IndexOutOfBounds` (or clamped? — see Open Questions).
- Returns state with `active_cell` set to the anchor cell.

**`extend_range_to` (~8 tests):**
- Extend down: focus row increases, anchor unchanged.
- Extend left: focus column moves left of anchor (anchor stays, focus changes column).
- Extend to same cell: no-op (anchor == focus).
- Extend from empty `range_selection`: equivalent to `start_range_selection`.
- Multi-rect: extends only the last rect.
- Cross-boundary (extend past last row): clamp.

**`add_disjoint_range` (~4 tests):**
- Appends a new rect; `range_selection.len()` increments by 1.
- Subsequent `extend_range_to` extends the new (last) rect only.
- Prior rects are unchanged.

**`select_all` (~4 tests):**
- Produces single range spanning all visible rows × all visible columns.
- Idempotent on a fully-selected table.
- Respects column visibility (hidden columns excluded from range).
- Respects active filter (only visible rows included).

**`clear_range_selection` (~2 tests):**
- Returns empty `range_selection`.
- Idempotent.

**`fill_down` (~8 tests):**
- Single-column range with 3 rows: top row value copied to rows 2 and 3.
- Multi-column range: all columns in the range filled down from their top cells.
- Single-row range → `Err(RangeError::RangeTooSmallToFill)`.
- Empty `range_selection` → `Err(RangeError::NoRangeSelected)`.
- Multi-rect → `Err(RangeError::MultiRectNotSupportedForThisOperation)`.
- Non-editable column in range → silent skip (per Item 15 open question).
- Read-only column: fill skips it, fills writable columns in range.

**`fill_handle_extend` arithmetic detection (~10 tests):**
- Source `[1, 2, 3]`, extend to 5 rows → `[1, 2, 3, 4, 5]`.
- Source `[2, 4, 6]`, extend → `[2, 4, 6, 8, 10]`.
- Source `[1, 3, 7]` (no constant step) → repeats: `[1, 3, 7, 1, 3]`.
- Source single value `[42]` → repeats: `[42, 42, 42]`.
- String source → repeats unconditionally.
- Horizontal fill (Right direction): mirrors Down tests.

**`normalized()` (~6 tests):**
- Anchor below focus: min/max correct, columns in order.
- Anchor left of focus: correct column ordering.
- Single-cell range: `min_row == max_row`, one column.
- Multi-column range spanning hidden columns: hidden columns excluded.

**Interaction invariants (~5 tests):**
- After `select_all`, `normalized()` min_row=0, max_row=visible_count-1.
- Arrow without Shift clears `range_selection` and moves `active_cell`.
- Arrow with Shift calls `extend_range_to` and updates `active_cell` to focus.
- Group header rows are never included in normalized range extents.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **`Vec<RangeSelection>` vs a different multi-rect shape.** Recommendation: `Vec<RangeSelection>`
   — it is the natural representation, mirrors AG Grid's internal model, and supports
   iteration cleanly. An `Option<RangeSelection>` (single-rect-only) would be simpler but
   would require a breaking change when multi-rect support is added. Confirm `Vec`.

2. **Multi-rect copy behavior.** Recommendation: `to_clipboard_tsv` (Item 17) returns
   `Err(PasteError::MultiRectCopyNotSupported)` when `range_selection.len() > 1`. Multi-rect
   is supported for Delete-to-clear and visual selection only. This matches Excel and AG Grid
   exactly. Confirm this constraint is locked in the API contract (not a runtime warning).

3. **Fill handle pattern detection scope for v0.2.0.** Recommendation: arithmetic progression
   on numeric values only. Date/day-name/month-name progressions (Excel features) are
   explicitly deferred to v0.3.0. Confirm scope cut so the v0.2.0 implementation is not
   blocked on date-arithmetic logic.

4. **Shift+Arrow at row/column boundary behavior.** Recommendation: clamp at boundary (no wrap,
   no error) + scroll the extended cell into view. This matches Excel. The alternative
   (scroll-and-extend when the viewport boundary is hit) is handled by the auto-scroll-on-drag
   mechanism and does not apply to keyboard Shift+arrow. Confirm clamp.

5. **Drag-to-select auto-scroll when dragging beyond viewport.** Recommendation: yes, implement
   auto-scroll in the adapter (requestAnimationFrame loop, scrolls 8px/frame while cursor is
   outside the table rect). This is standard Excel/AG Grid behavior. Confirm yes.

6. **Header click selecting whole column/row: respects current filter or absolute?**
   Recommendation: respects current filter — the range spans `(0, col_id)` to
   `(visible_row_count - 1, col_id)` using visible indices. This is consistent with
   `visible_view` semantics and matches the user's mental model (you see what you select).
   Confirm filter-aware.

7. **Interaction with grouping (Item 8): can you select across group header rows?**
   Recommendation: no. Group headers are excluded from range selection (they are not data
   rows). Dragging through a group header row skips it; the range contains only data rows
   on both sides. Confirm exclusion.

8. **`start_range_selection` with an out-of-bounds row index: clamp or error?**
   Recommendation: clamp to the nearest valid index (rather than error) because this
   transition is typically called from mouse click coordinates that the adapter maps to row
   indices — the mapping should be robust to off-by-one in the adapter's coordinate math.
   Confirm clamp (vs error).

9. **Active cell synchronization with range anchor.** Recommendation: `start_range_selection`
   implicitly sets `active_cell` to the anchor as a single combined return. This keeps
   active-cell and range anchor in sync without the adapter having to call two transitions.
   Confirm combined update in one returned state.
