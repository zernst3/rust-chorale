# Item 15: Active-Cell Focus Model + Full Keyboard Navigation

## Problem

chorale v0.2.0 currently has no concept of a "focused" cell. Keyboard interaction with the
table is limited to the Tab/Shift+Tab in-cell-editing navigation introduced in Item 7, which
only fires when an editor is open. There is no way to move a selection anchor via keyboard,
no way to scroll the table to a specific row via arrow keys, and no visible focus ring that
tells a sighted user which cell the keyboard is operating on.

This is a significant accessibility gap. WCAG 2.1 requires keyboard operability for all
interactive UI; a data grid without arrow-key navigation fails SC 2.1.1 (Keyboard). It is
also a feature gap against every competitor: AG Grid, TanStack Table (with adapter), and
MUI X DataGrid all ship a focused-cell model as their baseline interactive primitive. Excel's
entire keyboard model — Shift+arrow range extension (Item 16), Ctrl+C copy (Item 17), Delete
to clear — is built on top of the active-cell concept. Items 16 and 17 therefore depend on
this item landing first.

The active-cell model is also the foundation for the Item 16 range selection anchor: when
the user presses Shift+Arrow, the anchor is the current active cell and the focus extends.
Establishing this model cleanly in chorale-core now prevents ad-hoc duplication in each adapter.

## Proposed Public API

### `chorale-core`

```rust
/// Which cell currently holds keyboard focus, if any.
/// Added to `TableState` as an additive field; defaults to `None` on `TableState::new`.
pub active_cell: Option<ActiveCell>,

/// Identifies the focused cell by visible-row position + column.
#[non_exhaustive]
pub struct ActiveCell {
    /// Post-filter, post-sort visible row index (0 = first visible row).
    pub row_idx: usize,
    pub column_id: ColumnId,
}

/// Direction for move transitions.
#[non_exhaustive]
pub enum NavDirection {
    Up,
    Down,
    Left,
    Right,
}

/// State transitions (pure, per CHORALE-CORE-2):

/// Set the active cell to a specific visible-row index and column.
/// Returns Err(StateError::RowIndexOutOfBounds) if row_idx >= visible row count.
/// Returns Err(StateError::ColumnNotFound) if column_id is not in visible columns.
pub fn set_active_cell(
    state: &TableState<TRow>,
    row_idx: usize,
    column_id: ColumnId,
) -> Result<TableState<TRow>, StateError>;

/// Move the active cell one step in the given direction.
/// Clamps at boundaries (no wrap). If active_cell is None, moves to the
/// first cell (top-left) for Down/Right and last cell (bottom-right) for Up/Left.
pub fn move_active_cell(
    state: &TableState<TRow>,
    direction: NavDirection,
) -> TableState<TRow>;

/// Move the active cell to the edge of the data in the given direction
/// (Ctrl+Arrow Excel behavior). Stops at the last non-empty cell in the run.
pub fn move_active_cell_to_edge(
    state: &TableState<TRow>,
    direction: NavDirection,
) -> TableState<TRow>;

/// Move the active cell by one "page" in the Up or Down direction.
/// Page size = visible_row_count (computed from viewport_height / row_height).
/// Horizontal directions are ignored (returns state unchanged).
pub fn move_active_cell_page(
    state: &TableState<TRow>,
    direction: NavDirection,
    page_size: usize,
) -> TableState<TRow>;

/// Move the active cell to the first column of the current row (Home key).
pub fn move_active_cell_home(state: &TableState<TRow>) -> TableState<TRow>;

/// Move the active cell to the last column of the current row (End key).
pub fn move_active_cell_end(state: &TableState<TRow>) -> TableState<TRow>;

/// Move the active cell to the absolute first cell (Ctrl+Home).
pub fn move_active_cell_first(state: &TableState<TRow>) -> TableState<TRow>;

/// Move the active cell to the absolute last visible cell (Ctrl+End).
pub fn move_active_cell_last(state: &TableState<TRow>) -> TableState<TRow>;

/// Clear the active cell (returns state with active_cell: None).
pub fn clear_active_cell(state: &TableState<TRow>) -> TableState<TRow>;
```

### Adapter wiring (chorale-dioxus / chorale-leptos)

```rust
// On the Table component:
// No new props required — keyboard handling is wired internally.
// The adapter adds tabindex="0" to the table container element and
// attaches an onkeydown handler that dispatches the appropriate transition.
//
// Adapter fires set_active_cell on cell click.
// Adapter fires clear_active_cell on Escape (unless a range is active,
// in which case Escape collapses the range first).
//
// CSS variable --chorale-active-cell-outline (default: 2px solid #0078d4)
// applied via a class on the active cell td. This variable is user-overridable.
```

### Callsite shape

```rust
// Host app doesn't need to wire anything extra for keyboard nav.
// Clicking a cell or pressing arrow keys sets the active cell automatically.
//
// To read the active cell (e.g., for a custom status bar):
let active = handle.signal().read().active_cell.clone();
if let Some(ActiveCell { row_idx, column_id }) = active {
    // render "Row {row_idx}, Column {column_id}" in a status bar
}
```

## Internal Design

**State machine:** `active_cell: Option<ActiveCell>` stores the visible-row index and
column ID of the focused cell. Visible-row index (not `RowId`) is used so that the
active cell concept tracks the visible position; if the user applies a filter that
removes the active row, the adapter clamps the index to the new visible row count on
the next state read (this is adapter logic, not core logic — core stores whatever index
was set; the adapter clamps before display).

**Column order:** the left/right arrow transitions respect `state.column_order` (Item 9),
`state.column_visibility`, and frozen column partitions (Item 10) because they simply
walk the ordered visible column list. No special-casing needed for frozen columns: Left
at the leftmost column of the scrollable partition stops at the rightmost frozen-left
column (if any); the adapter then scrolls accordingly.

**Scroll-into-view:** the adapter is responsible for ensuring the active cell is visible
after each transition. The transition returns the new state; the adapter reads the new
`active_cell` and, if the cell is outside the current virtual window, updates `scroll_top`
to bring it into view. For fixed-row-height tables this is `row_idx * row_height`; for
variable-height tables (Item 6) the adapter uses the measured offset cache.

**Integration with Item 7 (editing):** pressing Enter or F2 on the active cell opens the
editor if the column has an `EditorKind`. This is pure adapter key-handler logic; core
already provides `start_edit`. The Tab/Shift+Tab behavior from Item 7 (tab between editable
cells while in edit mode) is preserved; Tab while NOT in edit mode moves the active cell to
the next column (general navigation, not editor-scoped).

**Integration with Item 16 (range selection):** arrow key without Shift calls
`move_active_cell` and collapses any range to a single cell. Arrow key with Shift calls
`extend_range_to` on the Item 16 API. This dispatch is entirely in the adapter's
key-handler; core does not need to know about Shift state.

## Backwards Compatibility

`active_cell: Option<ActiveCell>` is an additive field on `TableState`. `TableState` is
`#[non_exhaustive]` in v0.1.0, so cross-crate callers cannot use struct-literal construction;
adding the field does not break compilation downstream. The field defaults to `None` in
`TableState::new`, so existing callers see no behavioral change.

`NavDirection` and `ActiveCell` are new `#[non_exhaustive]` enums/structs. No existing
matches break. The new transition functions (`move_active_cell`, etc.) are purely additive
exports; they do not modify existing function signatures.

The adapter gains a `tabindex="0"` on the table container and a `onkeydown` handler. This
is a behavior addition; existing callers do not pass conflicting keyboard handlers to the
`Table` component.

## Test Plan

Per TESTS-1:

**`set_active_cell` transitions (~5 tests):**
- Happy path: sets `active_cell` to the specified row+column.
- `row_idx` out of bounds → `Err(StateError::RowIndexOutOfBounds)`.
- `column_id` not in visible columns → `Err(StateError::ColumnNotFound)`.
- Hidden column (column_visibility = false) → `Err(StateError::ColumnNotFound)`.
- Replaces a previous `active_cell` cleanly.

**`move_active_cell` direction transitions (~15 tests):**
- All four directions: happy path one step.
- Left at column 0 → clamp (no wrap).
- Up at row 0 → clamp.
- Right at last column → clamp.
- Down at last visible row → clamp.
- `None` active cell + Down → first cell.
- `None` active cell + Up → last cell.
- Movement respects `column_order` (reordered columns).
- Movement skips hidden columns.

**`move_active_cell_to_edge` (~8 tests):**
- Down from mid-table: stops at last visible row.
- Right from mid-row: stops at last visible column.
- Up / Left: symmetric cases.
- Already at edge: no-op (returns state unchanged).

**`move_active_cell_page` (~6 tests):**
- Down by `page_size` rows: correct new row_idx.
- Page at bottom: clamps to last row.
- Horizontal direction: no-op.

**`move_active_cell_home` / `_end` / `_first` / `_last` (~8 tests):**
- Home: moves to column 0 of current row.
- End: moves to last column of current row.
- First: row 0, column 0.
- Last: last row, last column.
- Each with `None` active cell: sets to the appropriate corner.

**`clear_active_cell` (~2 tests):**
- Returns state with `active_cell: None`.
- Idempotent: calling on already-`None` state is a no-op.

**Invariants (~3 tests):**
- `clear_active_cell(clear_active_cell(s)) == clear_active_cell(s)`.
- `move_active_cell` never produces an out-of-bounds `row_idx`.
- `active_cell.column_id` after any move is always in `visible_columns`.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **`active_cell` field type: `Option<ActiveCell>` with `None` on mount.** Recommendation:
   `None` on `TableState::new`; becomes `Some` on first arrow key press OR first cell click.
   There is no default "first cell is focused on mount" behavior — that would steal focus from
   other elements on the page. Confirm.

2. **Row index storage: `row_idx: usize` (post-filter-sort visible index) vs `RowId`
   (stable across filter/sort changes).** Recommendation: `row_idx` (visible position).
   When filter or sort changes, the active row may move or disappear; the adapter clamps to
   bounds rather than trying to preserve the logical row identity. This keeps core simple
   and avoids a `RowId` lookup in every move transition. If Zach wants sticky-row-across-sort
   behavior, we can add a `RowId`-based variant in v0.3. Confirm `row_idx` for v0.2.0.

3. **Tab general-navigation vs Tab editing-only.** Recommendation: merge into one Tab
   handler. Tab moves the active cell to the next column (general navigation); if that column
   has an `EditorKind`, the adapter may optionally open the editor via an
   `on_tab_to_editable: Option<EventHandler<ActiveCell>>` prop. This supersedes Item 7's
   Tab-within-editor-only behavior. Shift+Tab moves backwards. Confirm.

4. **Page Up/Down: visible-window-derived page size vs constant N=20.** Recommendation:
   `move_active_cell_page` takes an explicit `page_size: usize` parameter computed by the
   adapter from `(viewport_height / row_height).floor() as usize`. This makes core testable
   without a DOM and lets each adapter compute page size from its own scroll geometry.
   Confirm parameter is adapter-supplied (not hardcoded in core).

5. **Ctrl+Home / Ctrl+End: absolute first/last cell vs filter-aware first/last visible.**
   Recommendation: absolute — row 0 column 0 and last-visible-row last-visible-column in the
   current filter/sort view (which is the "absolute" within the visible data). Excel behavior:
   Ctrl+Home goes to A1 regardless of filter. Confirm we match Excel (visible-data-absolute,
   not raw-data-absolute).

6. **CSS variable for the active-cell focus ring.** Recommendation: expose
   `--chorale-active-cell-outline: 2px solid #0078d4` as a CSS custom property on the
   table container. Hosts override it with their design system color without re-theming the
   whole component. Confirm this approach (vs a prop-based class override).

## Decisions (signed off 2026-06-05)

All 6 recommendations accepted as written. Implementation may proceed.

1. ✅ `active_cell: Option<ActiveCell>` starts as `None` on mount; becomes
   `Some` on first arrow key OR first click. No auto-focus on mount.
2. ✅ Row index = `row_idx: usize` (visible position). On filter/sort
   change, adapter clamps to bounds. Sticky-row-across-sort deferrable
   to v0.3 if requested.
3. ✅ Tab merges: single Tab handler moves active cell to next column.
   Optional `on_tab_to_editable: Option<EventHandler<ActiveCell>>` prop
   lets host open editor automatically. Supersedes Item 7's
   editing-only Tab behavior.
4. ✅ `move_active_cell_page` takes adapter-supplied `page_size: usize`
   computed from `(viewport_height / row_height).floor() as usize`.
   Keeps core testable without DOM.
5. ✅ Ctrl+Home / Ctrl+End = "visible-data-absolute" — first/last cell
   in the current filter/sort view. Matches Excel filter-aware behavior.
6. ✅ Focus ring via CSS custom property
   `--chorale-active-cell-outline: 2px solid #0078d4` on the table
   container. Hosts override without re-theming.
