//! Pure state-transition functions for `TableState<TRow>`.
//!
//! Every function follows the CHORALE-CORE-2 immutable pattern:
//! `fn name(state: &TableState<TRow>, ...) -> TableState<TRow>`.
//! No `&mut self`. No signals. No async. Unit-testable without a framework.

use std::collections::HashMap;

use crate::error::StateError;
use crate::state::TableState;
use crate::types::{
    ColumnId, EditTarget, FilterValue, GroupKey, PaginationMode, PriorEdit, RowId, SortAction,
    SortDirection, SortState,
};

/// Cycle the sort state for `col` using `action` to determine replace vs append.
///
/// **`SortAction::Replace` (plain click):**
/// cycles: None → Asc → Desc → None, clearing all other sort columns.
///
/// **`SortAction::Append` (Shift+click):**
/// cycles: absent → Asc → Desc → removed. Other columns are unaffected.
///
/// Resets `scroll_top` and `page` to 0 so virtualization re-anchors after
/// reorder (recon-2 § 5). Clears `row_heights` cache (VIRT-2).
///
/// # Example
///
/// ```rust
/// use chorale_core::{TableState, ColumnId, SortAction, toggle_sort};
///
/// let state: TableState<String> = TableState::new(vec![], vec![]);
/// // Replace: None → Asc.
/// let s1 = toggle_sort(&state, ColumnId("name"), SortAction::Replace);
/// assert!(!s1.sort.is_empty());
/// // Replace again: Asc → Desc.
/// let s2 = toggle_sort(&s1, ColumnId("name"), SortAction::Replace);
/// // Replace again: Desc → None.
/// let s3 = toggle_sort(&s2, ColumnId("name"), SortAction::Replace);
/// assert!(s3.sort.is_empty());
/// ```
#[must_use]
pub fn toggle_sort<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
    action: SortAction,
) -> TableState<TRow> {
    let next_sort = match action {
        SortAction::Replace => match state.sort.first() {
            Some(s) if s.column == col => match s.direction {
                SortDirection::Asc => vec![SortState::new(col, SortDirection::Desc)],
                SortDirection::Desc => vec![],
            },
            _ => vec![SortState::new(col, SortDirection::Asc)],
        },
        SortAction::Append => {
            let mut next = state.sort.clone();
            if let Some(pos) = next.iter().position(|s| s.column == col) {
                match next[pos].direction {
                    SortDirection::Asc => next[pos] = SortState::new(col, SortDirection::Desc),
                    SortDirection::Desc => {
                        next.remove(pos);
                    }
                }
            } else {
                next.push(SortState::new(col, SortDirection::Asc));
            }
            next
        }
    };
    // Reset loaded_row_count when in InfiniteScroll mode (filter set changes the row set).
    let loaded_row_count = if state.pagination_mode == PaginationMode::InfiniteScroll {
        state.page_size
    } else {
        0
    };
    // Clear the variable-row-height cache: row indices shift on sort (VIRT-2).
    TableState {
        sort: next_sort,
        scroll_top: 0.0,
        page: 0,
        loaded_row_count,
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

/// Remove `col` from the sort list entirely. No-op if `col` is not sorted.
///
/// Does not disturb the priority of other sort columns.
#[must_use]
pub fn remove_sort<TRow: Clone>(state: &TableState<TRow>, col: ColumnId) -> TableState<TRow> {
    let mut next_sort = state.sort.clone();
    next_sort.retain(|s| s.column != col);
    let loaded_row_count = if state.pagination_mode == PaginationMode::InfiniteScroll {
        state.page_size
    } else {
        0
    };
    TableState {
        sort: next_sort,
        scroll_top: 0.0,
        page: 0,
        loaded_row_count,
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

/// Clear all active sort columns.
#[must_use]
pub fn clear_sort<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let loaded_row_count = if state.pagination_mode == PaginationMode::InfiniteScroll {
        state.page_size
    } else {
        0
    };
    TableState {
        sort: vec![],
        scroll_top: 0.0,
        page: 0,
        loaded_row_count,
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

/// Set or clear the active filter for `col`.
///
/// `filter = None` removes any existing filter on the column.
/// Resets `scroll_top` and `page` to 0 because the row count may change
/// (recon-2 § 5). In `PaginationMode::InfiniteScroll`, also resets
/// `loaded_row_count` to `page_size` so the scroll list starts fresh.
///
/// # Example
///
/// ```rust
/// use chorale_core::{TableState, ColumnId, FilterValue, set_filter};
///
/// let state: TableState<String> = TableState::new(vec![], vec![]);
/// // Apply a text filter.
/// let filtered = set_filter(&state, ColumnId("name"), Some(FilterValue::Text("alice".into())));
/// assert!(filtered.filters.contains_key(&ColumnId("name")));
/// assert_eq!(filtered.page, 0);
/// // Clear it.
/// let cleared = set_filter(&filtered, ColumnId("name"), None);
/// assert!(cleared.filters.is_empty());
/// ```
#[must_use]
pub fn set_filter<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
    filter: Option<FilterValue>,
) -> TableState<TRow> {
    let mut filters = state.filters.clone();
    match filter {
        Some(f) => {
            filters.insert(col, f);
        }
        None => {
            filters.remove(&col);
        }
    }
    let loaded_row_count = if state.pagination_mode == PaginationMode::InfiniteScroll {
        state.page_size
    } else {
        0
    };
    // Clear the variable-row-height cache: filtered row set (and indices) change (VIRT-2).
    TableState {
        filters,
        scroll_top: 0.0,
        page: 0,
        loaded_row_count,
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

/// Set the selection state of a single row.
///
/// `selected = true` adds `row_id` to the selection (idempotent).
/// `selected = false` removes it (idempotent).
#[must_use]
pub fn set_selection<TRow: Clone>(
    state: &TableState<TRow>,
    row_id: RowId,
    selected: bool,
) -> TableState<TRow> {
    let mut selection = state.selection.clone();
    if selected {
        if !selection.contains(&row_id) {
            selection.push(row_id);
        }
    } else {
        selection.retain(|id| *id != row_id);
    }
    TableState {
        selection,
        ..state.clone()
    }
}

/// Toggle between "select all visible rows" and "select none".
///
/// If all currently visible row IDs (post-sort/post-filter/post-pagination)
/// are already selected, this deselects all. Otherwise it selects all
/// visible rows.
#[must_use]
pub fn toggle_select_all<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let visible_ids: Vec<RowId> = crate::views::visible_row_ids(state);
    let all_selected =
        !visible_ids.is_empty() && visible_ids.iter().all(|id| state.selection.contains(id));

    let selection = if all_selected {
        // Deselect all visible rows; keep any out-of-page selections
        state
            .selection
            .iter()
            .copied()
            .filter(|id| !visible_ids.contains(id))
            .collect()
    } else {
        // Add all visible IDs that aren't already selected
        let mut sel = state.selection.clone();
        for id in &visible_ids {
            if !sel.contains(id) {
                sel.push(*id);
            }
        }
        sel
    };

    TableState {
        selection,
        ..state.clone()
    }
}

/// Jump to page `page` (zero-based).
///
/// # Errors
///
/// Returns `Err(StateError::PageOutOfRange)` if `page >= total_pages()`.
/// Returns `Err(StateError::InvalidModeForTransition)` in `PaginationMode::InfiniteScroll`.
pub fn set_page<TRow: Clone>(
    state: &TableState<TRow>,
    page: usize,
) -> Result<TableState<TRow>, StateError> {
    if state.pagination_mode == PaginationMode::InfiniteScroll {
        return Err(StateError::InvalidModeForTransition);
    }
    let total = state.total_pages();
    if page >= total {
        return Err(StateError::PageOutOfRange(page));
    }
    // Clear the variable-row-height cache: page boundary shifts all view indices (VIRT-2).
    Ok(TableState {
        page,
        scroll_top: 0.0,
        row_heights: HashMap::new(),
        ..state.clone()
    })
}

/// Change the number of rows per page.
///
/// Resets to page 0 because the new page boundaries are different.
///
/// # Errors
///
/// Returns `Err(StateError::PageSizeZero)` if `size == 0`.
pub fn set_page_size<TRow: Clone>(
    state: &TableState<TRow>,
    size: usize,
) -> Result<TableState<TRow>, StateError> {
    if size == 0 {
        return Err(StateError::PageSizeZero);
    }
    Ok(TableState {
        page_size: size,
        page: 0,
        scroll_top: 0.0,
        ..state.clone()
    })
}

/// Show or hide a column.
#[must_use]
pub fn set_column_visibility<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
    visible: bool,
) -> TableState<TRow> {
    let mut column_visibility = state.column_visibility.clone();
    column_visibility.insert(col, visible);
    TableState {
        column_visibility,
        ..state.clone()
    }
}

/// Override the width of a column in pixels (for resize handles).
///
/// # Errors
///
/// Returns `Err(StateError::InvalidColumnWidth)` if `width_px <= 0`.
pub fn set_column_width<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
    width_px: f64,
) -> Result<TableState<TRow>, StateError> {
    if width_px <= 0.0 {
        return Err(StateError::InvalidColumnWidth(width_px.to_string()));
    }
    let mut column_widths = state.column_widths.clone();
    column_widths.insert(col, width_px);
    Ok(TableState {
        column_widths,
        ..state.clone()
    })
}

/// Update the scroll position of the virtualized scroll container (px).
///
/// Called by the adapter's `onscroll` handler. Pure: the adapter owns the
/// DOM listener; this function just updates state (recon-2 § 3).
#[must_use]
pub fn set_scroll<TRow: Clone>(state: &TableState<TRow>, scroll_top: f64) -> TableState<TRow> {
    TableState {
        scroll_top,
        ..state.clone()
    }
}

/// Replace a row's data in-place, identified by `row_id`.
///
/// If `row_id` is not found, the state is returned unchanged.
/// Defined in recon-2 § 7d as the cell-editing escape valve.
#[must_use]
pub fn update_row<TRow: Clone>(
    state: &TableState<TRow>,
    row_id: RowId,
    new_row: TRow,
) -> TableState<TRow> {
    let mut rows = state.rows.clone();
    if let Some(slot) = rows.iter_mut().find(|(id, _)| *id == row_id) {
        slot.1 = new_row;
    }
    TableState {
        rows,
        ..state.clone()
    }
}

/// Record the measured height (px) for row at `index` in the current page view.
///
/// The adapter calls this after DOM measurement (VIRT-2). `index` is the row's
/// zero-based position in the post-filter/sort/paginated `visible_view` output
/// for the current page. The cache is invalidated automatically by
/// [`toggle_sort`], [`set_filter`], and [`set_page`].
#[must_use]
pub fn record_row_height<TRow: Clone>(
    state: &TableState<TRow>,
    index: usize,
    height: f64,
) -> TableState<TRow> {
    let mut row_heights = state.row_heights.clone();
    row_heights.insert(index, height);
    TableState {
        row_heights,
        ..state.clone()
    }
}

/// Merge a batch of measured row heights into the cache in a single transition.
///
/// Equivalent to calling [`record_row_height`] for every entry in `heights`
/// but produces only one clone of `TableState` instead of one per entry.
/// The adapter measurement loop uses this to avoid N signal writes for an
/// N-row virtual window.
#[must_use]
pub fn batch_record_row_heights<TRow: Clone, S: std::hash::BuildHasher>(
    state: &TableState<TRow>,
    heights: &HashMap<usize, f64, S>,
) -> TableState<TRow> {
    if heights.is_empty() {
        return state.clone();
    }
    let mut row_heights = state.row_heights.clone();
    row_heights.extend(heights.iter().map(|(k, v)| (*k, *v)));
    TableState {
        row_heights,
        ..state.clone()
    }
}

/// Clear the variable-row-height cache.
///
/// Called when the row set changes in a way that invalidates cached heights
/// (e.g., after a data reload or a transition not already covered by
/// [`toggle_sort`] / [`set_filter`] / [`set_page`]).
#[must_use]
pub fn clear_row_height_cache<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

// ---------------------------------------------------------------------------
// In-cell editing transitions (v0.2.0 Item 7)
// ---------------------------------------------------------------------------

/// Open an editor for the cell at (`row_id`, `column_id`).
///
/// Opening a second cell while one is already open implicitly cancels the
/// first (no orphaned edit lock).
///
/// # Errors
///
/// Returns `Err(StateError::ColumnNotEditable)` if the target column has no
/// `EditorKind` configured.
#[must_use = "returns a new state; the original is unchanged"]
pub fn start_edit<TRow: Clone>(
    state: &TableState<TRow>,
    row_id: RowId,
    column_id: ColumnId,
) -> Result<TableState<TRow>, StateError> {
    let has_editor = state
        .columns
        .iter()
        .any(|c| c.id == column_id && c.editor.is_some());
    if !has_editor {
        return Err(StateError::ColumnNotEditable(column_id));
    }
    Ok(TableState {
        editing: Some(EditTarget { row_id, column_id }),
        ..state.clone()
    })
}

/// Close the editor after a successful commit. Returns a state with
/// `editing: None`. Does **not** update row data; the caller is responsible
/// for calling `update_row` (or letting the host's `on_commit_edit` callback
/// handle persistence).
#[must_use]
pub fn commit_edit<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        editing: None,
        ..state.clone()
    }
}

/// Cancel the editor without persisting. Returns a state with `editing: None`.
/// No-op (returns a clone) if no edit is in progress.
#[must_use]
pub fn cancel_edit<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    if state.editing.is_none() {
        return state.clone();
    }
    TableState {
        editing: None,
        ..state.clone()
    }
}

/// Roll back a previously-committed edit. Restores the full prior row so the
/// host can call this from an async persistence-failure path.
///
/// No-op (returns a clone) if the row is no longer present (it was deleted
/// between commit and the persistence callback firing).
#[must_use]
pub fn revert_edit<TRow: Clone>(
    state: &TableState<TRow>,
    prior: &PriorEdit<TRow>,
) -> TableState<TRow> {
    if !state.rows.iter().any(|(id, _)| *id == prior.row_id) {
        return state.clone();
    }
    update_row(state, prior.row_id, prior.prior_row.clone())
}

/// Move the edit cursor to the next editable column in the same row (Tab).
///
/// Cycles within the row's editable columns: after the last editable column,
/// wraps to the first. No-op if no edit is currently in progress or if there
/// are no editable columns.
#[must_use]
pub fn next_editable_cell<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let Some(current) = state.editing else {
        return state.clone();
    };
    let editable: Vec<ColumnId> = state
        .columns
        .iter()
        .filter(|c| c.editor.is_some())
        .map(|c| c.id)
        .collect();
    if editable.is_empty() {
        return state.clone();
    }
    let next = match editable.iter().position(|&c| c == current.column_id) {
        Some(i) if i + 1 < editable.len() => editable[i + 1],
        _ => editable[0],
    };
    TableState {
        editing: Some(EditTarget {
            row_id: current.row_id,
            column_id: next,
        }),
        ..state.clone()
    }
}

/// Move the edit cursor to the previous editable column in the same row
/// (Shift+Tab).
///
/// Cycles within the row's editable columns: before the first editable column,
/// wraps to the last. No-op if no edit is currently in progress or if there
/// are no editable columns.
#[must_use]
pub fn prev_editable_cell<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let Some(current) = state.editing else {
        return state.clone();
    };
    let editable: Vec<ColumnId> = state
        .columns
        .iter()
        .filter(|c| c.editor.is_some())
        .map(|c| c.id)
        .collect();
    if editable.is_empty() {
        return state.clone();
    }
    let prev = match editable.iter().position(|&c| c == current.column_id) {
        Some(0) | None => editable[editable.len() - 1],
        Some(i) => editable[i - 1],
    };
    TableState {
        editing: Some(EditTarget {
            row_id: current.row_id,
            column_id: prev,
        }),
        ..state.clone()
    }
}

// ---------------------------------------------------------------------------
// Column order transitions (Item 9)
// ---------------------------------------------------------------------------

/// Set an explicit column render order.
///
/// Columns absent from `order` are appended at the end in definition order.
/// Resets to definition order by passing an empty vec (or use
/// [`reset_column_order`]).
///
/// # Errors
///
/// Returns [`StateError::UnknownColumnId`] if any id in `order` is not in
/// `state.columns`. Returns [`StateError::DuplicateColumnId`] if `order`
/// contains a duplicate id.
pub fn set_column_order<TRow: Clone>(
    state: &TableState<TRow>,
    order: Vec<ColumnId>,
) -> Result<TableState<TRow>, StateError> {
    let mut seen = std::collections::HashSet::new();
    for &id in &order {
        if !state.columns.iter().any(|c| c.id == id) {
            return Err(StateError::UnknownColumnId(id));
        }
        if !seen.insert(id) {
            return Err(StateError::DuplicateColumnId(id));
        }
    }
    let mut next = state.clone();
    next.column_order = order;
    Ok(next)
}

/// Move column `column_id` to `to_index` in the render order.
///
/// If `column_order` is currently empty it is initialized from definition
/// order first. Out-of-bounds `to_index` is clamped to the last valid
/// position.
///
/// # Errors
///
/// Returns [`StateError::UnknownColumnId`] if `column_id` is not found in
/// `state.columns`.
pub fn move_column<TRow: Clone>(
    state: &TableState<TRow>,
    column_id: ColumnId,
    to_index: usize,
) -> Result<TableState<TRow>, StateError> {
    if !state.columns.iter().any(|c| c.id == column_id) {
        return Err(StateError::UnknownColumnId(column_id));
    }
    let mut order: Vec<ColumnId> = if state.column_order.is_empty() {
        state.columns.iter().map(|c| c.id).collect()
    } else {
        // Preserve the user-set order but ensure all columns are present.
        let mut o = state.column_order.clone();
        for col in &state.columns {
            if !o.contains(&col.id) {
                o.push(col.id);
            }
        }
        o
    };
    if let Some(pos) = order.iter().position(|id| *id == column_id) {
        order.remove(pos);
    }
    let clamped = to_index.min(order.len());
    order.insert(clamped, column_id);
    let mut next = state.clone();
    next.column_order = order;
    Ok(next)
}

/// Reset to definition order by clearing `column_order`.
#[must_use]
pub fn reset_column_order<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let mut next = state.clone();
    next.column_order = vec![];
    next
}

// ---------------------------------------------------------------------------
// Pagination-mode transitions (Item 11.0b)
// ---------------------------------------------------------------------------

/// Switch between `PaginationMode::Pages` and `PaginationMode::InfiniteScroll`.
///
/// **Switching to `InfiniteScroll`:** `loaded_row_count` is initialised to
/// `page_size` (so the first batch is visible immediately), `page` and
/// `scroll_top` are reset to 0, and the row-height cache is cleared.
///
/// **Switching to `Pages`:** `loaded_row_count` is cleared, `page` is reset
/// to 0, `scroll_top` to 0, and the row-height cache is cleared.
#[must_use]
pub fn set_pagination_mode<TRow: Clone>(
    state: &TableState<TRow>,
    mode: PaginationMode,
) -> TableState<TRow> {
    let (loaded_row_count, page) = match mode {
        PaginationMode::InfiniteScroll => (state.page_size, 0),
        PaginationMode::Pages => (0, 0),
    };
    TableState {
        pagination_mode: mode,
        loaded_row_count,
        page,
        scroll_top: 0.0,
        row_heights: HashMap::new(),
        ..state.clone()
    }
}

/// Increase the number of loaded rows by `page_size`, capped at the total
/// filtered row count. Used by adapters to implement "load more on scroll".
///
/// Only valid in `PaginationMode::InfiniteScroll`.
///
/// # Errors
///
/// Returns `Err(StateError::InvalidModeForTransition)` in `PaginationMode::Pages`.
pub fn load_more_rows<TRow: Clone>(
    state: &TableState<TRow>,
) -> Result<TableState<TRow>, StateError> {
    if state.pagination_mode == PaginationMode::Pages {
        return Err(StateError::InvalidModeForTransition);
    }
    let total = state.filtered_row_count();
    let next = (state.loaded_row_count + state.page_size).min(total);
    Ok(TableState {
        loaded_row_count: next,
        ..state.clone()
    })
}

// ---------------------------------------------------------------------------
// Grouping transitions (Item 8)
// ---------------------------------------------------------------------------

/// Set the active grouping columns.
///
/// `columns = vec![]` clears grouping. Setting grouping always clears
/// `collapsed_groups` so stale collapse state does not carry over when the
/// column set changes.
#[must_use]
pub fn set_grouping<TRow: Clone>(
    state: &TableState<TRow>,
    columns: Vec<ColumnId>,
) -> TableState<TRow> {
    TableState {
        grouping: columns,
        collapsed_groups: std::collections::HashSet::new(),
        ..state.clone()
    }
}

/// Toggle a group's collapsed state.
///
/// If `key` is in `collapsed_groups`, it is removed (group expands).
/// Otherwise it is inserted (group collapses). No-op on an unknown key.
#[must_use]
pub fn toggle_group<TRow: Clone>(state: &TableState<TRow>, key: &GroupKey) -> TableState<TRow> {
    let mut collapsed = state.collapsed_groups.clone();
    if collapsed.contains(key) {
        collapsed.remove(key);
    } else {
        collapsed.insert(key.clone());
    }
    TableState {
        collapsed_groups: collapsed,
        ..state.clone()
    }
}

/// Expand all groups (clear `collapsed_groups`).
#[must_use]
pub fn expand_all_groups<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        collapsed_groups: std::collections::HashSet::new(),
        ..state.clone()
    }
}

/// Collapse all top-level and nested groups.
///
/// Computes all group keys by temporarily expanding the full grouped tree, then
/// sets `collapsed_groups` to that key set.
#[must_use]
pub fn collapse_all_groups<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    // Expand everything to discover all group keys.
    let fully_expanded = TableState {
        collapsed_groups: std::collections::HashSet::new(),
        ..state.clone()
    };
    let all_keys: std::collections::HashSet<GroupKey> =
        crate::views::visible_grouped_view(&fully_expanded)
            .into_iter()
            .filter_map(|row| match row {
                crate::views::GroupedRow::Header { key, .. } => Some(key),
                crate::views::GroupedRow::Data(..) => None,
            })
            .collect();
    TableState {
        collapsed_groups: all_keys,
        ..state.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests (TESTS-1: every transition has a unit test asserting the result)
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss
)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::column::ColumnDef;
    use crate::state::TableState;
    use crate::types::{
        Alignment, CellValue, ColumnId, FilterValue, GroupedPaginationMode, PaginationMode, RowId,
        SortAction, SortDirection,
    };

    // ---- helpers -----------------------------------------------------------

    #[derive(Clone, Debug, PartialEq)]
    struct TestRow {
        name: String,
        score: f64,
    }

    fn col_name() -> ColumnId {
        ColumnId("name")
    }
    fn col_score() -> ColumnId {
        ColumnId("score")
    }

    fn make_columns() -> Vec<ColumnDef<TestRow>> {
        vec![
            ColumnDef::new(col_name(), "Name", |r: &TestRow| {
                CellValue::Text(r.name.clone())
            })
            .sortable()
            .filter(crate::column::FilterKind::Text),
            ColumnDef::new(col_score(), "Score", |r: &TestRow| {
                CellValue::Float(r.score)
            })
            .sortable()
            .filter(crate::column::FilterKind::Text)
            .alignment(Alignment::Right)
            .render_kind(crate::column::RenderKind::Number),
        ]
    }

    fn make_state() -> TableState<TestRow> {
        let rows = vec![
            (
                RowId::new(),
                TestRow {
                    name: "Alice".into(),
                    score: 90.0,
                },
            ),
            (
                RowId::new(),
                TestRow {
                    name: "Bob".into(),
                    score: 75.0,
                },
            ),
            (
                RowId::new(),
                TestRow {
                    name: "Charlie".into(),
                    score: 85.0,
                },
            ),
        ];
        TableState {
            rows,
            columns: make_columns(),
            sort: vec![],
            filters: HashMap::new(),
            selection: vec![],
            page: 0,
            page_size: 10,
            column_visibility: HashMap::new(),
            column_widths: HashMap::new(),
            column_order: vec![],
            editing: None,
            row_heights: HashMap::new(),
            scroll_top: 0.0,
            viewport_height: 500.0,
            row_height: 40.0,
            buffer_rows: 3,
            pagination_mode: PaginationMode::Pages,
            loaded_row_count: 0,
            grouping: vec![],
            collapsed_groups: std::collections::HashSet::new(),
            grouped_pagination: GroupedPaginationMode::DataRowsOnly,
        }
    }

    fn make_editable_columns() -> Vec<ColumnDef<TestRow>> {
        vec![
            ColumnDef::new(col_name(), "Name", |r: &TestRow| {
                CellValue::Text(r.name.clone())
            })
            .editor(crate::column::EditorKind::Text),
            ColumnDef::new(col_score(), "Score", |r: &TestRow| {
                CellValue::Float(r.score)
            })
            .editor(crate::column::EditorKind::Number {
                min: Some(0.0),
                max: Some(100.0),
                step: Some(1.0),
            }),
        ]
    }

    fn make_editable_state() -> TableState<TestRow> {
        let mut s = make_state();
        s.columns = make_editable_columns();
        s
    }

    // ---- toggle_sort -------------------------------------------------------

    #[test]
    fn toggle_sort_none_to_asc() {
        let s = make_state();
        let s2 = toggle_sort(&s, col_name(), SortAction::Replace);
        assert_eq!(
            s2.sort,
            vec![SortState::new(col_name(), SortDirection::Asc)]
        );
        assert_eq!(s2.scroll_top, 0.0);
        assert_eq!(s2.page, 0);
    }

    #[test]
    fn toggle_sort_asc_to_desc() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Replace);
        let s2 = toggle_sort(&s, col_name(), SortAction::Replace);
        assert_eq!(
            s2.sort,
            vec![SortState::new(col_name(), SortDirection::Desc)]
        );
    }

    #[test]
    fn toggle_sort_desc_to_none() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Replace);
        let s = toggle_sort(&s, col_name(), SortAction::Replace);
        let s2 = toggle_sort(&s, col_name(), SortAction::Replace);
        assert!(s2.sort.is_empty());
    }

    #[test]
    fn toggle_sort_different_column_resets_to_asc() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Replace);
        let s2 = toggle_sort(&s, col_score(), SortAction::Replace);
        assert_eq!(
            s2.sort,
            vec![SortState::new(col_score(), SortDirection::Asc)]
        );
    }

    // ---- toggle_sort multi-column (Item 11.0a) --------------------------------

    #[test]
    fn toggle_sort_replace_clears_other_columns() {
        let s = make_state();
        // Sort by name, then by score → sort has [name, score].
        let s = toggle_sort(&s, col_name(), SortAction::Append);
        let s = toggle_sort(&s, col_score(), SortAction::Append);
        assert_eq!(s.sort.len(), 2);
        // Replace with col_name → only col_name in sort.
        let s2 = toggle_sort(&s, col_name(), SortAction::Replace);
        assert_eq!(s2.sort.len(), 1);
        assert_eq!(s2.sort[0].column, col_name());
    }

    #[test]
    fn toggle_sort_append_on_unsorted_adds_asc() {
        let s = make_state();
        let s2 = toggle_sort(&s, col_name(), SortAction::Append);
        assert_eq!(s2.sort.len(), 1);
        assert_eq!(s2.sort[0].column, col_name());
        assert_eq!(s2.sort[0].direction, SortDirection::Asc);
    }

    #[test]
    fn toggle_sort_append_on_existing_flips_direction() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Append);
        let s2 = toggle_sort(&s, col_name(), SortAction::Append);
        assert_eq!(s2.sort[0].direction, SortDirection::Desc);
    }

    #[test]
    fn toggle_sort_append_on_desc_removes_column() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Append);
        let s = toggle_sort(&s, col_name(), SortAction::Append); // → Desc
        let s2 = toggle_sort(&s, col_name(), SortAction::Append); // → removed
        assert!(s2.sort.is_empty());
    }

    #[test]
    fn toggle_sort_append_does_not_disturb_other_columns() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Append);
        let s = toggle_sort(&s, col_score(), SortAction::Append);
        // Flip col_name from Asc to Desc; col_score should remain.
        let s2 = toggle_sort(&s, col_name(), SortAction::Append);
        assert_eq!(s2.sort.len(), 2);
        let name_entry = s2.sort.iter().find(|e| e.column == col_name()).unwrap();
        assert_eq!(name_entry.direction, SortDirection::Desc);
        let score_entry = s2.sort.iter().find(|e| e.column == col_score()).unwrap();
        assert_eq!(score_entry.direction, SortDirection::Asc);
    }

    #[test]
    fn remove_sort_removes_target_column() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Append);
        let s = toggle_sort(&s, col_score(), SortAction::Append);
        let s2 = remove_sort(&s, col_name());
        assert_eq!(s2.sort.len(), 1);
        assert_eq!(s2.sort[0].column, col_score());
    }

    #[test]
    fn remove_sort_is_noop_if_not_sorted() {
        let s = make_state();
        let s2 = remove_sort(&s, col_name());
        assert!(s2.sort.is_empty());
    }

    #[test]
    fn clear_sort_removes_all_columns() {
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Append);
        let s = toggle_sort(&s, col_score(), SortAction::Append);
        let s2 = clear_sort(&s);
        assert!(s2.sort.is_empty());
    }

    #[test]
    fn multi_column_sort_priority_order() {
        use crate::views::visible_view;
        // 4 rows: two with dept "A" (scores 10, 20), two with dept "B" (scores 5, 30).
        // Sort by dept ASC, then score DESC.
        // Expected: A/20, A/10, B/30, B/5.
        #[derive(Clone, PartialEq)]
        struct Row {
            dept: &'static str,
            score: i64,
        }
        let rows = vec![
            (
                RowId::new(),
                Row {
                    dept: "B",
                    score: 5,
                },
            ),
            (
                RowId::new(),
                Row {
                    dept: "A",
                    score: 20,
                },
            ),
            (
                RowId::new(),
                Row {
                    dept: "B",
                    score: 30,
                },
            ),
            (
                RowId::new(),
                Row {
                    dept: "A",
                    score: 10,
                },
            ),
        ];
        let cols = vec![
            crate::column::ColumnDef::new(ColumnId("dept"), "Dept", |r: &Row| {
                CellValue::Text(r.dept.to_string())
            })
            .sortable(),
            crate::column::ColumnDef::new(ColumnId("score"), "Score", |r: &Row| {
                CellValue::Integer(r.score)
            })
            .sortable(),
        ];
        let s = TableState::new(rows, cols);
        let s = toggle_sort(&s, ColumnId("dept"), SortAction::Append); // dept ASC
        let s = toggle_sort(&s, ColumnId("score"), SortAction::Append); // score ASC
                                                                        // Flip score to DESC.
        let s = toggle_sort(&s, ColumnId("score"), SortAction::Append);
        let view = visible_view(&s);
        let depts: Vec<&str> = view.iter().map(|(_, r)| r.dept).collect();
        let scores: Vec<i64> = view.iter().map(|(_, r)| r.score).collect();
        assert_eq!(depts, vec!["A", "A", "B", "B"]);
        assert_eq!(scores, vec![20, 10, 30, 5]);
    }

    // ---- set_filter --------------------------------------------------------

    #[test]
    fn set_filter_adds_filter() {
        let s = make_state();
        let s2 = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        assert_eq!(
            s2.filters.get(&col_name()),
            Some(&FilterValue::Text("ali".into()))
        );
        assert_eq!(s2.page, 0);
        assert_eq!(s2.scroll_top, 0.0);
    }

    #[test]
    fn set_filter_removes_filter() {
        let s = make_state();
        let s = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        let s2 = set_filter(&s, col_name(), None);
        assert!(!s2.filters.contains_key(&col_name()));
    }

    // ---- set_selection -----------------------------------------------------

    #[test]
    fn set_selection_adds_and_removes() {
        let s = make_state();
        let id = s.rows[0].0;
        let s2 = set_selection(&s, id, true);
        assert!(s2.selection.contains(&id));
        let s3 = set_selection(&s2, id, false);
        assert!(!s3.selection.contains(&id));
    }

    #[test]
    fn set_selection_idempotent_add() {
        let s = make_state();
        let id = s.rows[0].0;
        let s = set_selection(&s, id, true);
        let s2 = set_selection(&s, id, true);
        assert_eq!(s2.selection.iter().filter(|&&i| i == id).count(), 1);
    }

    // ---- toggle_select_all -------------------------------------------------

    #[test]
    fn toggle_select_all_selects_all_when_none_selected() {
        let s = make_state();
        let s2 = toggle_select_all(&s);
        assert_eq!(s2.selection.len(), s.rows.len());
    }

    #[test]
    fn toggle_select_all_deselects_all_when_all_selected() {
        let s = make_state();
        let s = toggle_select_all(&s); // select all
        let s2 = toggle_select_all(&s); // deselect all
        assert!(s2.selection.is_empty());
    }

    // ---- set_page ----------------------------------------------------------

    #[test]
    fn set_page_valid() {
        let mut s = make_state();
        s.page_size = 2; // 3 rows → 2 pages (0, 1)
        let s2 = set_page(&s, 1).expect("page 1 should be valid");
        assert_eq!(s2.page, 1);
        assert_eq!(s2.scroll_top, 0.0);
    }

    #[test]
    fn set_page_out_of_range() {
        let s = make_state();
        let err = set_page(&s, 99).unwrap_err();
        assert_eq!(err, StateError::PageOutOfRange(99));
    }

    // ---- set_page_size -----------------------------------------------------

    #[test]
    fn set_page_size_valid() {
        let s = make_state();
        let s2 = set_page_size(&s, 5).expect("size 5 is valid");
        assert_eq!(s2.page_size, 5);
        assert_eq!(s2.page, 0);
    }

    #[test]
    fn set_page_size_zero_is_error() {
        let s = make_state();
        let err = set_page_size(&s, 0).unwrap_err();
        assert_eq!(err, StateError::PageSizeZero);
    }

    // ---- set_column_visibility ---------------------------------------------

    #[test]
    fn set_column_visibility_hides_and_shows() {
        let s = make_state();
        assert!(s.is_column_visible(col_name()));
        let s2 = set_column_visibility(&s, col_name(), false);
        assert!(!s2.is_column_visible(col_name()));
        let s3 = set_column_visibility(&s2, col_name(), true);
        assert!(s3.is_column_visible(col_name()));
    }

    // ---- set_column_width --------------------------------------------------

    #[test]
    fn set_column_width_valid() {
        let s = make_state();
        let s2 = set_column_width(&s, col_name(), 200.0).expect("valid width");
        assert_eq!(s2.column_widths.get(&col_name()), Some(&200.0));
    }

    #[test]
    fn set_column_width_zero_is_error() {
        let s = make_state();
        assert!(set_column_width(&s, col_name(), 0.0).is_err());
    }

    #[test]
    fn set_column_width_negative_is_error() {
        let s = make_state();
        assert!(set_column_width(&s, col_name(), -10.0).is_err());
    }

    // ---- set_scroll --------------------------------------------------------

    #[test]
    fn set_scroll_updates_scroll_top() {
        let s = make_state();
        let s2 = set_scroll(&s, 120.5);
        assert!((s2.scroll_top - 120.5).abs() < f64::EPSILON);
    }

    // ---- update_row --------------------------------------------------------

    #[test]
    fn update_row_replaces_matching_row() {
        let s = make_state();
        let id = s.rows[0].0;
        let new_row = TestRow {
            name: "Alice (updated)".into(),
            score: 99.0,
        };
        let s2 = update_row(&s, id, new_row.clone());
        assert_eq!(s2.rows[0].1, new_row);
        // Other rows unchanged
        assert_eq!(s2.rows[1].1, s.rows[1].1);
    }

    #[test]
    fn update_row_unknown_id_leaves_state_unchanged() {
        let s = make_state();
        let unknown = RowId::new();
        let new_row = TestRow {
            name: "Ghost".into(),
            score: 0.0,
        };
        let s2 = update_row(&s, unknown, new_row);
        assert_eq!(s2.rows[0].1, s.rows[0].1);
    }

    // ---- toggle_select_all with active filter ----------------------------

    #[test]
    fn toggle_select_all_only_selects_visible_page_rows() {
        // 3 rows, page_size=2 → page 0 has rows[0] and rows[1], page 1 has rows[2].
        let s = make_state();
        let mut s2 = s.clone();
        s2.page_size = 2;
        let s3 = toggle_select_all(&s2);
        // Only the first 2 rows (the visible page) should be selected.
        assert_eq!(s3.selection.len(), 2);
        assert!(!s3.selection.contains(&s2.rows[2].0));
    }

    #[test]
    fn set_scroll_is_idempotent_at_same_position() {
        // set_scroll with the same value should return a state equal to
        // calling it once (the transition is deterministic and pure).
        let s = make_state();
        let s1 = set_scroll(&s, 100.0);
        let s2 = set_scroll(&s1, 100.0);
        assert!((s2.scroll_top - 100.0).abs() < f64::EPSILON);
    }

    // ---- record_row_height ------------------------------------------------

    #[test]
    fn record_row_height_inserts_entry() {
        let s = make_state();
        let s2 = record_row_height(&s, 3, 55.5);
        assert_eq!(s2.row_heights.get(&3), Some(&55.5));
        // All other fields unchanged.
        assert_eq!(s2.rows.len(), s.rows.len());
        assert!(s2.sort.is_empty());
    }

    #[test]
    fn record_row_height_overwrites_existing_entry() {
        let s = make_state();
        let s = record_row_height(&s, 0, 40.0);
        let s2 = record_row_height(&s, 0, 60.0);
        assert_eq!(s2.row_heights.get(&0), Some(&60.0));
    }

    #[test]
    fn record_row_height_does_not_mutate_input() {
        let s = make_state();
        let _ = record_row_height(&s, 0, 50.0);
        assert!(s.row_heights.is_empty());
    }

    // ---- clear_row_height_cache -------------------------------------------

    #[test]
    fn clear_row_height_cache_empties_map() {
        let s = make_state();
        let s = record_row_height(&s, 0, 40.0);
        let s = record_row_height(&s, 1, 80.0);
        assert_eq!(s.row_heights.len(), 2);
        let s2 = clear_row_height_cache(&s);
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn clear_row_height_cache_idempotent_on_empty() {
        let s = make_state();
        assert!(s.row_heights.is_empty());
        let s2 = clear_row_height_cache(&s);
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn clear_row_height_cache_preserves_other_fields() {
        let s = make_state();
        let s = record_row_height(&s, 0, 50.0);
        let s2 = clear_row_height_cache(&s);
        assert_eq!(s2.rows.len(), s.rows.len());
        assert_eq!(s2.page, s.page);
    }

    // ---- cache-invalidation on sort / filter / page -----------------------

    #[test]
    fn toggle_sort_clears_row_height_cache() {
        let s = make_state();
        let s = record_row_height(&s, 0, 60.0);
        let s2 = toggle_sort(&s, col_name(), SortAction::Replace);
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn set_filter_clears_row_height_cache() {
        let s = make_state();
        let s = record_row_height(&s, 0, 60.0);
        let s2 = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn set_page_clears_row_height_cache() {
        let mut s = make_state();
        s.page_size = 2; // 3 rows → 2 pages
        let s = record_row_height(&s, 0, 60.0);
        let s2 = set_page(&s, 1).expect("page 1 valid");
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn batch_record_row_heights_merges_entries() {
        let s = make_state();
        let heights: HashMap<usize, f64> = [(0, 40.0), (1, 60.0), (2, 50.0)].into();
        let s2 = batch_record_row_heights(&s, &heights);
        assert_eq!(s2.row_heights.get(&0), Some(&40.0));
        assert_eq!(s2.row_heights.get(&1), Some(&60.0));
        assert_eq!(s2.row_heights.get(&2), Some(&50.0));
    }

    #[test]
    fn batch_record_row_heights_empty_batch_is_no_op() {
        let s = record_row_height(&make_state(), 0, 55.0);
        let s2 = batch_record_row_heights(&s, &HashMap::new());
        assert_eq!(s2.row_heights, s.row_heights);
    }

    #[test]
    fn batch_record_row_heights_overwrites_existing_entries() {
        let s = record_row_height(&make_state(), 0, 40.0);
        let heights: HashMap<usize, f64> = [(0, 80.0)].into();
        let s2 = batch_record_row_heights(&s, &heights);
        assert_eq!(s2.row_heights.get(&0), Some(&80.0));
    }

    #[test]
    fn batch_record_row_heights_does_not_mutate_input() {
        let s = make_state();
        let heights: HashMap<usize, f64> = [(0, 40.0)].into();
        let _ = batch_record_row_heights(&s, &heights);
        assert!(s.row_heights.is_empty());
    }

    // ---- in-cell editing transitions (Item 7) -----------------------------

    #[test]
    fn start_edit_happy_path() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).expect("col_name is editable");
        assert_eq!(
            s2.editing,
            Some(EditTarget {
                row_id,
                column_id: col_name()
            })
        );
    }

    #[test]
    fn start_edit_non_editable_column_returns_err() {
        let s = make_state(); // columns have no editor
        let row_id = s.rows[0].0;
        let err = start_edit(&s, row_id, col_name()).unwrap_err();
        assert_eq!(err, StateError::ColumnNotEditable(col_name()));
    }

    #[test]
    fn start_edit_replaces_prior_edit_without_orphan() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = start_edit(&s2, row_id, col_score()).unwrap();
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_score()
            })
        );
    }

    #[test]
    fn commit_edit_clears_editing() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = commit_edit(&s2);
        assert_eq!(s3.editing, None);
        assert_eq!(s3.rows.len(), s.rows.len()); // other fields unchanged
    }

    #[test]
    fn cancel_edit_clears_editing() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = cancel_edit(&s2);
        assert_eq!(s3.editing, None);
    }

    #[test]
    fn cancel_edit_when_no_edit_in_progress_is_noop() {
        let s = make_state();
        assert!(s.editing.is_none());
        let s2 = cancel_edit(&s);
        assert!(s2.editing.is_none());
        assert_eq!(s2.rows.len(), s.rows.len());
    }

    #[test]
    fn commit_cancel_idempotent() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let cancelled = cancel_edit(&s2);
        let committed_then_cancelled = commit_edit(&cancel_edit(&s2));
        assert_eq!(cancelled.editing, committed_then_cancelled.editing);
    }

    #[test]
    fn revert_edit_restores_prior_row() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let prior_row = s.rows[0].1.clone();
        // Simulate: the host calls update_row with new data, then revert_edit to undo
        let modified = update_row(
            &s,
            row_id,
            TestRow {
                name: "Alice Updated".into(),
                score: 99.0,
            },
        );
        let prior = PriorEdit {
            row_id,
            column_id: col_name(),
            prior_row: prior_row.clone(),
        };
        let reverted = revert_edit(&modified, &prior);
        assert_eq!(reverted.rows[0].1, prior_row);
    }

    #[test]
    fn revert_edit_noop_when_row_missing() {
        let s = make_editable_state();
        let ghost_id = RowId::new(); // not in the table
        let prior = PriorEdit {
            row_id: ghost_id,
            column_id: col_name(),
            prior_row: TestRow {
                name: "Ghost".into(),
                score: 0.0,
            },
        };
        let s2 = revert_edit(&s, &prior);
        assert_eq!(s2.rows.len(), s.rows.len());
    }

    #[test]
    fn column_def_editor_builder() {
        use crate::column::EditorKind;
        let col = ColumnDef::new(col_name(), "Name", |r: &TestRow| {
            CellValue::Text(r.name.clone())
        })
        .editor(EditorKind::Text);
        assert!(matches!(col.editor, Some(EditorKind::Text)));
    }

    #[test]
    fn next_editable_cell_advances_within_row() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap();
        let s3 = next_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_score()
            })
        );
    }

    #[test]
    fn next_editable_cell_wraps_to_first_after_last() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_score()).unwrap(); // last editable
        let s3 = next_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_name()
            })
        );
    }

    #[test]
    fn prev_editable_cell_moves_backwards() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_score()).unwrap();
        let s3 = prev_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_name()
            })
        );
    }

    #[test]
    fn prev_editable_cell_wraps_to_last_before_first() {
        let s = make_editable_state();
        let row_id = s.rows[0].0;
        let s2 = start_edit(&s, row_id, col_name()).unwrap(); // first editable
        let s3 = prev_editable_cell(&s2);
        assert_eq!(
            s3.editing,
            Some(EditTarget {
                row_id,
                column_id: col_score()
            })
        );
    }

    #[test]
    fn next_editable_cell_noop_when_no_edit_in_progress() {
        let s = make_editable_state();
        assert!(s.editing.is_none());
        let s2 = next_editable_cell(&s);
        assert!(s2.editing.is_none());
    }

    // ---- column order (Item 9) ---------------------------------------------

    fn col_a() -> ColumnId {
        ColumnId("a")
    }
    fn col_b() -> ColumnId {
        ColumnId("b")
    }
    fn col_c() -> ColumnId {
        ColumnId("c")
    }

    fn make_order_state() -> TableState<TestRow> {
        let rows = vec![(
            RowId::new(),
            TestRow {
                name: "R".into(),
                score: 1.0,
            },
        )];
        let columns = vec![
            ColumnDef::new(col_a(), "A", |r: &TestRow| CellValue::Text(r.name.clone())),
            ColumnDef::new(col_b(), "B", |r: &TestRow| CellValue::Float(r.score)),
            ColumnDef::new(col_c(), "C", |r: &TestRow| CellValue::Float(r.score)),
        ];
        TableState::new(rows, columns)
    }

    #[test]
    fn set_column_order_happy_path() {
        let s = make_order_state();
        let s2 = set_column_order(&s, vec![col_c(), col_a(), col_b()]).unwrap();
        assert_eq!(s2.column_order, vec![col_c(), col_a(), col_b()]);
    }

    #[test]
    fn set_column_order_unknown_id_returns_err() {
        let s = make_order_state();
        let result = set_column_order(&s, vec![ColumnId("unknown")]);
        assert!(matches!(
            result,
            Err(crate::error::StateError::UnknownColumnId(_))
        ));
    }

    #[test]
    fn set_column_order_duplicate_returns_err() {
        let s = make_order_state();
        let result = set_column_order(&s, vec![col_a(), col_a()]);
        assert!(matches!(
            result,
            Err(crate::error::StateError::DuplicateColumnId(_))
        ));
    }

    #[test]
    fn set_column_order_empty_is_definition_order() {
        let s = make_order_state();
        let s2 = set_column_order(&s, vec![]).unwrap();
        assert!(s2.column_order.is_empty());
    }

    #[test]
    fn set_column_order_idempotent() {
        let s = make_order_state();
        let order = vec![col_b(), col_a(), col_c()];
        let s2 = set_column_order(&s, order.clone()).unwrap();
        let s3 = set_column_order(&s2, order.clone()).unwrap();
        assert_eq!(s2.column_order, s3.column_order);
    }

    #[test]
    fn move_column_from_index_2_to_0() {
        let s = make_order_state();
        // move col_c (index 2 in definition order) to index 0
        let s2 = move_column(&s, col_c(), 0).unwrap();
        assert_eq!(s2.column_order[0], col_c());
    }

    #[test]
    fn move_column_unknown_id_returns_err() {
        let s = make_order_state();
        let result = move_column(&s, ColumnId("nope"), 0);
        assert!(matches!(
            result,
            Err(crate::error::StateError::UnknownColumnId(_))
        ));
    }

    #[test]
    fn move_column_same_position_is_noop() {
        let s = make_order_state();
        // col_a is at index 0; moving to 0 should produce same order.
        let s2 = move_column(&s, col_a(), 0).unwrap();
        assert_eq!(s2.column_order[0], col_a());
        assert_eq!(s2.column_order[1], col_b());
        assert_eq!(s2.column_order[2], col_c());
    }

    #[test]
    fn move_column_out_of_bounds_index_clamped() {
        let s = make_order_state();
        // to_index = 999 should be clamped to last valid position (2).
        let s2 = move_column(&s, col_a(), 999).unwrap();
        assert_eq!(*s2.column_order.last().unwrap(), col_a());
    }

    #[test]
    fn reset_column_order_clears_vec() {
        let s = make_order_state();
        let s2 = set_column_order(&s, vec![col_b(), col_a(), col_c()]).unwrap();
        assert!(!s2.column_order.is_empty());
        let s3 = reset_column_order(&s2);
        assert!(s3.column_order.is_empty());
    }

    #[test]
    fn move_column_with_existing_partial_order_appends_missing() {
        // column_order has only [a, b]; c is not listed.
        // moving c to index 0 should initialize order as [a, b, c] first,
        // then remove c and insert at 0 → [c, a, b].
        let s = make_order_state();
        let s2 = set_column_order(&s, vec![col_a(), col_b()]).unwrap();
        let s3 = move_column(&s2, col_c(), 0).unwrap();
        assert_eq!(s3.column_order[0], col_c());
        assert_eq!(s3.column_order[1], col_a());
        assert_eq!(s3.column_order[2], col_b());
    }

    // ---- set_pagination_mode (Item 11.0b) ------------------------------------

    #[test]
    fn set_pagination_mode_pages_to_infinite_scroll() {
        let s = make_state(); // page_size=10
        let s2 = set_pagination_mode(&s, PaginationMode::InfiniteScroll);
        assert_eq!(s2.pagination_mode, PaginationMode::InfiniteScroll);
        assert_eq!(s2.loaded_row_count, s.page_size); // initialised to page_size
        assert_eq!(s2.page, 0);
        assert!((s2.scroll_top - 0.0).abs() < f64::EPSILON);
        assert!(s2.row_heights.is_empty());
    }

    #[test]
    fn set_pagination_mode_infinite_scroll_to_pages() {
        let mut s = make_state();
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 30;
        s.page = 2;
        let s2 = set_pagination_mode(&s, PaginationMode::Pages);
        assert_eq!(s2.pagination_mode, PaginationMode::Pages);
        assert_eq!(s2.loaded_row_count, 0);
        assert_eq!(s2.page, 0);
    }

    #[test]
    fn set_pagination_mode_clears_row_height_cache() {
        let s = record_row_height(&make_state(), 0, 55.0);
        let s2 = set_pagination_mode(&s, PaginationMode::InfiniteScroll);
        assert!(s2.row_heights.is_empty());
    }

    // ---- load_more_rows (Item 11.0b) -----------------------------------------

    #[test]
    fn load_more_rows_errors_in_pages_mode() {
        let s = make_state();
        assert_eq!(s.pagination_mode, PaginationMode::Pages);
        let err = load_more_rows(&s).unwrap_err();
        assert_eq!(err, StateError::InvalidModeForTransition);
    }

    #[test]
    fn load_more_rows_increases_by_page_size() {
        let mut s = make_state(); // 3 rows, page_size=10
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 0;
        let s2 = load_more_rows(&s).unwrap();
        // capped at filtered_row_count (3) since page_size (10) > 3
        assert_eq!(s2.loaded_row_count, 3);
    }

    #[test]
    fn load_more_rows_caps_at_filtered_row_count() {
        let mut s = make_state(); // 3 rows
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.page_size = 2;
        s.loaded_row_count = 2; // already loaded first batch
        let s2 = load_more_rows(&s).unwrap();
        assert_eq!(s2.loaded_row_count, 3); // 2+2=4 capped at 3
    }

    #[test]
    fn load_more_rows_at_max_does_not_exceed() {
        let mut s = make_state(); // 3 rows
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 3; // already at max
        let s2 = load_more_rows(&s).unwrap();
        assert_eq!(s2.loaded_row_count, 3);
    }

    // ---- set_page in InfiniteScroll mode (Item 11.0b) ------------------------

    #[test]
    fn set_page_errors_in_infinite_scroll_mode() {
        let mut s = make_state();
        s.pagination_mode = PaginationMode::InfiniteScroll;
        let err = set_page(&s, 0).unwrap_err();
        assert_eq!(err, StateError::InvalidModeForTransition);
    }

    // ---- set_filter resets loaded_row_count in InfiniteScroll (Item 11.0b) --

    #[test]
    fn set_filter_resets_loaded_row_count_in_infinite_scroll() {
        let mut s = make_state(); // page_size=10
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 30;
        let s2 = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        assert_eq!(s2.loaded_row_count, s.page_size); // reset to page_size
        assert_eq!(s2.page, 0);
    }

    #[test]
    fn set_filter_loaded_row_count_unchanged_in_pages_mode() {
        let s = make_state();
        assert_eq!(s.pagination_mode, PaginationMode::Pages);
        let s2 = set_filter(&s, col_name(), Some(FilterValue::Text("ali".into())));
        assert_eq!(s2.loaded_row_count, 0); // Pages mode: always 0
    }

    // ---- toggle_sort resets loaded_row_count in InfiniteScroll (Item 11.0b) -

    #[test]
    fn toggle_sort_resets_loaded_row_count_in_infinite_scroll() {
        let mut s = make_state(); // page_size=10
        s.pagination_mode = PaginationMode::InfiniteScroll;
        s.loaded_row_count = 30;
        let s2 = toggle_sort(&s, col_name(), SortAction::Replace);
        assert_eq!(s2.loaded_row_count, s.page_size);
    }

    // ---- grouping transitions (Item 8) ----------------------------------------

    fn make_grouped_state() -> TableState<TestRow> {
        let rows = vec![
            (
                RowId::new(),
                TestRow {
                    name: "Alice".into(),
                    score: 90.0,
                },
            ),
            (
                RowId::new(),
                TestRow {
                    name: "Bob".into(),
                    score: 75.0,
                },
            ),
            (
                RowId::new(),
                TestRow {
                    name: "Alice".into(),
                    score: 85.0,
                },
            ),
        ];
        TableState {
            rows,
            columns: make_columns(),
            ..make_state()
        }
    }

    #[test]
    fn set_grouping_sets_columns_and_clears_collapsed() {
        let s = make_grouped_state();
        // pre-collapse a group key
        let key = GroupKey::from_values(&["Alice"]);
        let s = toggle_group(&s, &key);
        assert!(!s.collapsed_groups.is_empty());
        // set_grouping must clear collapsed_groups
        let s2 = set_grouping(&s, vec![col_name()]);
        assert_eq!(s2.grouping, vec![col_name()]);
        assert!(s2.collapsed_groups.is_empty());
    }

    #[test]
    fn set_grouping_empty_clears_grouping() {
        let s = make_grouped_state();
        let s = set_grouping(&s, vec![col_name()]);
        let s2 = set_grouping(&s, vec![]);
        assert!(s2.grouping.is_empty());
    }

    #[test]
    fn toggle_group_collapses_and_expands() {
        let s = make_grouped_state();
        let key = GroupKey::from_values(&["Alice"]);
        // collapse
        let s2 = toggle_group(&s, &key);
        assert!(s2.collapsed_groups.contains(&key));
        // expand
        let s3 = toggle_group(&s2, &key);
        assert!(!s3.collapsed_groups.contains(&key));
    }

    #[test]
    fn expand_all_groups_clears_all_collapsed() {
        let s = make_grouped_state();
        let k1 = GroupKey::from_values(&["Alice"]);
        let k2 = GroupKey::from_values(&["Bob"]);
        let s = toggle_group(&s, &k1);
        let s = toggle_group(&s, &k2);
        assert_eq!(s.collapsed_groups.len(), 2);
        let s2 = expand_all_groups(&s);
        assert!(s2.collapsed_groups.is_empty());
    }

    #[test]
    fn collapse_all_groups_fills_collapsed_set() {
        let s = set_grouping(&make_grouped_state(), vec![col_name()]);
        let s2 = collapse_all_groups(&s);
        // Alice and Bob are the two group keys
        assert_eq!(s2.collapsed_groups.len(), 2);
        assert!(s2
            .collapsed_groups
            .contains(&GroupKey::from_values(&["Alice"])));
        assert!(s2
            .collapsed_groups
            .contains(&GroupKey::from_values(&["Bob"])));
    }

    #[test]
    fn collapse_all_groups_on_empty_grouping_produces_empty_set() {
        let s = make_grouped_state(); // grouping is empty
        let s2 = collapse_all_groups(&s);
        assert!(s2.collapsed_groups.is_empty());
    }
}
