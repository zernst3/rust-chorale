//! Pure state-transition functions for `TableState<TRow>`.
//!
//! Every function follows the CHORALE-CORE-2 immutable pattern:
//! `fn name(state: &TableState<TRow>, ...) -> TableState<TRow>`.
//! No `&mut self`. No signals. No async. Unit-testable without a framework.

use std::collections::HashMap;

use crate::error::StateError;
use crate::state::TableState;
use crate::types::{
    ActiveCell, ColumnId, EditTarget, FilterValue, GroupKey, NavDirection, PaginationMode,
    PriorEdit, RowId, SortAction, SortDirection, SortState,
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

/// Select every row currently on the visible page (excluding detail panels).
#[must_use]
pub fn select_all_visible_page<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let page_ids: Vec<RowId> = crate::views::visible_view(state)
        .into_iter()
        .filter_map(|r| match r {
            crate::views::RenderRow::Data { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    let mut sel = state.selection.clone();
    for id in page_ids {
        if !sel.contains(&id) {
            sel.push(id);
        }
    }
    TableState {
        selection: sel,
        ..state.clone()
    }
}

/// Select every row in the filtered + sorted set (across all pages).
#[must_use]
pub fn select_all_filtered<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let all_ids: Vec<RowId> = crate::views::filtered_sorted_pairs(state)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    TableState {
        selection: all_ids,
        ..state.clone()
    }
}

/// Deselect every row currently on the visible page, leaving other-page selections intact.
#[must_use]
pub fn deselect_all_visible_page<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let page_ids: std::collections::HashSet<RowId> = crate::views::visible_view(state)
        .into_iter()
        .filter_map(|r| match r {
            crate::views::RenderRow::Data { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    let kept: Vec<RowId> = state
        .selection
        .iter()
        .filter(|id| !page_ids.contains(id))
        .copied()
        .collect();
    TableState {
        selection: kept,
        ..state.clone()
    }
}

/// Clear the entire selection across all pages.
#[must_use]
pub fn deselect_all<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        selection: Vec::new(),
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

/// Remove the explicit width override for `col`, falling back to the
/// column's `initial_width` (if set) or the table default.
#[must_use]
pub fn reset_column_width<TRow: Clone>(
    state: &TableState<TRow>,
    col: ColumnId,
) -> TableState<TRow> {
    let mut column_widths = state.column_widths.clone();
    column_widths.remove(&col);
    TableState {
        column_widths,
        ..state.clone()
    }
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
        // Bump data_generation so adapter view caches that key on this
        // counter (e.g., the dioxus view memo) recompute on row-content
        // changes. Without this, in-cell edits land in state.rows but the
        // cached visible_view is never recomputed and the cell renders
        // stale text until an unrelated transition (sort/filter/page/
        // grouping/expansion) happens to bump a different key field.
        data_generation: state.data_generation.wrapping_add(1),
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

/// Toggle whether `row_id` is expanded (showing a detail panel below it).
///
/// Mirrors `toggle_group`: if `row_id` is in `expanded_rows`, it is removed
/// (row collapses). Otherwise it is inserted (row expands). No-op semantics
/// for unknown IDs (still inserts; renderer simply has no parent to anchor
/// to but state mutation is valid).
///
/// Pure. Per CHORALE-CORE-2.
#[must_use]
pub fn toggle_row_expansion<TRow: Clone>(
    state: &TableState<TRow>,
    row_id: RowId,
) -> TableState<TRow> {
    let mut expanded = state.expanded_rows.clone();
    if expanded.contains(&row_id) {
        expanded.remove(&row_id);
    } else {
        expanded.insert(row_id);
    }
    TableState {
        expanded_rows: expanded,
        ..state.clone()
    }
}

/// Collapse all expanded rows (clear `expanded_rows`).
#[must_use]
pub fn collapse_all_rows<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        expanded_rows: std::collections::HashSet::new(),
        ..state.clone()
    }
}

// ---------------------------------------------------------------------------
// Item 15: Active-cell + keyboard navigation transitions (CC-1, API-1)
// ---------------------------------------------------------------------------

/// Returns the ordered list of visible `ColumnId`s for keyboard navigation.
fn visible_column_ids<TRow: Clone>(state: &TableState<TRow>) -> Vec<ColumnId> {
    crate::views::effective_column_order(state)
        .into_iter()
        .filter(|c| state.is_column_visible(c.id))
        .map(|c| c.id)
        .collect()
}

/// Returns the visible row count (post-filter, post-sort, post-pagination).
fn visible_row_count<TRow: Clone>(state: &TableState<TRow>) -> usize {
    crate::views::visible_view(state).len()
}

/// Set the active cell to a specific visible-row index and column.
///
/// Returns `Err(StateError::RowIndexOutOfBounds)` if `row_idx >= visible_row_count`.
/// Returns `Err(StateError::ColumnNotFound)` if `column_id` is not a visible column.
///
/// # Example
///
/// ```rust
/// use chorale_core::{TableState, ColumnId, RowId, set_active_cell};
///
/// let state: TableState<String> = TableState::new(vec![(RowId::new(), "x".to_string())], vec![]);
/// // No columns defined, so ColumnNotFound is returned.
/// let result = set_active_cell(&state, 0, ColumnId("name"));
/// assert!(result.is_err());
/// ```
///
/// # Errors
///
/// Returns [`StateError::RowIndexOutOfBounds`] if `row_idx >= visible_row_count`.
/// Returns [`StateError::ColumnNotFound`] if `column_id` is not in the visible columns.
#[must_use = "transitions return a new TableState; the original is unchanged"]
pub fn set_active_cell<TRow: Clone>(
    state: &TableState<TRow>,
    row_idx: usize,
    column_id: ColumnId,
) -> Result<TableState<TRow>, crate::error::StateError> {
    let row_count = visible_row_count(state);
    if row_idx >= row_count && row_count > 0 {
        return Err(crate::error::StateError::RowIndexOutOfBounds);
    }
    let cols = visible_column_ids(state);
    if !cols.contains(&column_id) {
        return Err(crate::error::StateError::ColumnNotFound);
    }
    Ok(TableState {
        active_cell: Some(ActiveCell::new(row_idx, column_id)),
        range_selection: vec![], // plain set_active_cell clears any range
        ..state.clone()
    })
}

/// Move the active cell one step in `direction`. Clamps at boundaries (no wrap).
///
/// If `active_cell` is `None`, moves to the first cell (top-left) for Down/Right
/// and the last cell (bottom-right) for Up/Left. Returns the state unchanged when
/// there are no visible rows or columns.
#[must_use]
pub fn move_active_cell<TRow: Clone>(
    state: &TableState<TRow>,
    direction: NavDirection,
) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    let last_row = row_count - 1;
    let last_col_idx = cols.len() - 1;

    // When active_cell is None, pressing a key sets the cell to the logical
    // "starting corner" (first for Down/Right, last for Up/Left) without
    // applying an additional step.
    if state.active_cell.is_none() {
        let (r, ci) = match direction {
            NavDirection::Up | NavDirection::Left => (last_row, last_col_idx),
            _ => (0, 0),
        };
        return TableState {
            active_cell: Some(ActiveCell::new(r, cols[ci])),
            ..state.clone()
        };
    }

    let (row, col_idx) = {
        let ac = state.active_cell.as_ref().unwrap_or_else(|| unreachable!());
        let ci = cols.iter().position(|c| *c == ac.column_id).unwrap_or(0);
        (ac.row_idx.min(last_row), ci.min(last_col_idx))
    };

    let (new_row, new_col_idx) = match direction {
        NavDirection::Up => (row.saturating_sub(1), col_idx),
        NavDirection::Down => ((row + 1).min(last_row), col_idx),
        NavDirection::Left => (row, col_idx.saturating_sub(1)),
        NavDirection::Right => (row, (col_idx + 1).min(last_col_idx)),
    };

    TableState {
        active_cell: Some(ActiveCell::new(new_row, cols[new_col_idx])),
        ..state.clone()
    }
}

/// Move the active cell to the data edge in `direction` (Ctrl+Arrow).
///
/// Stops at the last row (Down), first row (Up), first column (Left), or last
/// column (Right). If already at the edge, returns the state unchanged.
#[must_use]
pub fn move_active_cell_to_edge<TRow: Clone>(
    state: &TableState<TRow>,
    direction: NavDirection,
) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    let last_row = row_count - 1;
    let last_col_idx = cols.len() - 1;

    let (row, col_idx) = match &state.active_cell {
        None => (0, 0),
        Some(ac) => {
            let ci = cols.iter().position(|c| *c == ac.column_id).unwrap_or(0);
            (ac.row_idx.min(last_row), ci.min(last_col_idx))
        }
    };

    let (new_row, new_col_idx) = match direction {
        NavDirection::Up => (0, col_idx),
        NavDirection::Down => (last_row, col_idx),
        NavDirection::Left => (row, 0),
        NavDirection::Right => (row, last_col_idx),
    };

    TableState {
        active_cell: Some(ActiveCell::new(new_row, cols[new_col_idx])),
        ..state.clone()
    }
}

/// Move the active cell by `page_size` rows in Up or Down direction (Page Up/Down).
///
/// Clamps at the first/last visible row. Horizontal directions are ignored
/// (state returned unchanged). `page_size` is caller-supplied, computed from
/// `(viewport_height / row_height).floor()` in the adapter.
#[must_use]
pub fn move_active_cell_page<TRow: Clone>(
    state: &TableState<TRow>,
    direction: NavDirection,
    page_size: usize,
) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    let last_row = row_count - 1;
    let last_col_idx = cols.len() - 1;

    let (row, col_idx) = match &state.active_cell {
        None => (0, 0),
        Some(ac) => {
            let ci = cols.iter().position(|c| *c == ac.column_id).unwrap_or(0);
            (ac.row_idx.min(last_row), ci.min(last_col_idx))
        }
    };

    let new_row = match direction {
        NavDirection::Up => row.saturating_sub(page_size),
        NavDirection::Down => (row + page_size).min(last_row),
        _ => return state.clone(),
    };

    TableState {
        active_cell: Some(ActiveCell::new(new_row, cols[col_idx])),
        ..state.clone()
    }
}

/// Move the active cell to the first column of the current row (Home key).
///
/// If `active_cell` is `None`, moves to row 0, column 0.
#[must_use]
pub fn move_active_cell_home<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    let row = if let Some(ac) = state.active_cell {
        ac.row_idx.min(row_count - 1)
    } else {
        0
    };
    TableState {
        active_cell: Some(ActiveCell::new(row, cols[0])),
        ..state.clone()
    }
}

/// Move the active cell to the last column of the current row (End key).
///
/// If `active_cell` is `None`, moves to row 0, last column.
#[must_use]
pub fn move_active_cell_end<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    let row = if let Some(ac) = state.active_cell {
        ac.row_idx.min(row_count - 1)
    } else {
        0
    };
    let last_col = *cols.last().unwrap_or(&cols[0]);
    TableState {
        active_cell: Some(ActiveCell::new(row, last_col)),
        ..state.clone()
    }
}

/// Move the active cell to the absolute first visible cell (Ctrl+Home).
#[must_use]
pub fn move_active_cell_first<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    TableState {
        active_cell: Some(ActiveCell::new(0, cols[0])),
        ..state.clone()
    }
}

/// Move the active cell to the absolute last visible cell (Ctrl+End).
#[must_use]
pub fn move_active_cell_last<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    let last_col = *cols.last().unwrap_or(&cols[0]);
    TableState {
        active_cell: Some(ActiveCell::new(row_count - 1, last_col)),
        ..state.clone()
    }
}

/// Clear the active cell (returns state with `active_cell: None`). Idempotent.
#[must_use]
pub fn clear_active_cell<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        active_cell: None,
        ..state.clone()
    }
}

// ---------------------------------------------------------------------------
// Item 16: Range selection transitions (CC-1, API-1)
// ---------------------------------------------------------------------------

use crate::range::RangeSelection;

/// Begin a new range selection anchored at the given cell.
///
/// Replaces any existing `range_selection`. Also sets `active_cell` to the
/// anchor (so active cell and range anchor stay in sync).
/// Row index is clamped to `visible_row_count - 1` if out of bounds.
#[must_use]
pub fn start_range_selection<TRow: Clone>(
    state: &TableState<TRow>,
    anchor_row: usize,
    anchor_col: ColumnId,
) -> TableState<TRow> {
    let row_count = visible_row_count(state);
    let clamped_row = if row_count == 0 {
        0
    } else {
        anchor_row.min(row_count - 1)
    };
    TableState {
        range_selection: vec![RangeSelection::single(clamped_row, anchor_col)],
        active_cell: Some(ActiveCell::new(clamped_row, anchor_col)),
        ..state.clone()
    }
}

/// Extend the active (last) range so its focus moves to the given cell.
///
/// If `range_selection` is empty, behaves like `start_range_selection`.
/// Row index is clamped at boundary.
#[must_use]
pub fn extend_range_to<TRow: Clone>(
    state: &TableState<TRow>,
    row_idx: usize,
    col: ColumnId,
) -> TableState<TRow> {
    let row_count = visible_row_count(state);
    let clamped_row = if row_count == 0 {
        0
    } else {
        row_idx.min(row_count - 1)
    };
    if state.range_selection.is_empty() {
        return start_range_selection(state, clamped_row, col);
    }
    let mut ranges = state.range_selection.clone();
    if let Some(last) = ranges.last_mut() {
        last.focus = (clamped_row, col);
    }
    TableState {
        range_selection: ranges,
        active_cell: Some(ActiveCell::new(clamped_row, col)),
        ..state.clone()
    }
}

/// Add a disjoint range (Ctrl+click). Subsequent `extend_range_to` extends the new range.
#[must_use]
pub fn add_disjoint_range<TRow: Clone>(
    state: &TableState<TRow>,
    anchor_row: usize,
    anchor_col: ColumnId,
) -> TableState<TRow> {
    let row_count = visible_row_count(state);
    let clamped_row = if row_count == 0 {
        0
    } else {
        anchor_row.min(row_count - 1)
    };
    let mut ranges = state.range_selection.clone();
    ranges.push(RangeSelection::single(clamped_row, anchor_col));
    TableState {
        range_selection: ranges,
        active_cell: Some(ActiveCell::new(clamped_row, anchor_col)),
        ..state.clone()
    }
}

/// Select all visible rows × all visible columns (Ctrl+A).
///
/// Produces a single range spanning all cells. Idempotent.
#[must_use]
pub fn select_all<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    let cols = visible_column_ids(state);
    let row_count = visible_row_count(state);
    if cols.is_empty() || row_count == 0 {
        return state.clone();
    }
    let first_col = cols[0];
    let last_col = *cols.last().unwrap_or(&cols[0]);
    let range = RangeSelection::new((0, first_col), (row_count - 1, last_col));
    TableState {
        range_selection: vec![range],
        ..state.clone()
    }
}

/// Clear all ranges (Escape key when no editor is open). Idempotent.
#[must_use]
pub fn clear_range_selection<TRow: Clone>(state: &TableState<TRow>) -> TableState<TRow> {
    TableState {
        range_selection: vec![],
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
        Alignment, CellValue, ColumnId, FilterValue, NavDirection, PaginationMode, RowId,
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
    fn col_age() -> ColumnId {
        ColumnId("age")
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
            ..TableState::new(vec![], vec![])
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
    fn toggle_sort_append_supports_three_plus_columns() {
        // TESTS-1: 3 consecutive Append calls must grow state.sort to length 3.
        // col_age is not defined in make_columns(); toggle_sort(Append) does
        // not validate column existence — it only manages the sort list.
        let s = make_state();
        let s = toggle_sort(&s, col_name(), SortAction::Append);
        let s = toggle_sort(&s, col_score(), SortAction::Append);
        let s = toggle_sort(&s, col_age(), SortAction::Append);
        assert_eq!(
            s.sort.len(),
            3,
            "expected 3 sort columns, got {}",
            s.sort.len()
        );
        assert_eq!(s.sort[0].column, col_name());
        assert_eq!(s.sort[1].column, col_score());
        assert_eq!(s.sort[2].column, col_age());
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
        let depts: Vec<&str> = view
            .iter()
            .filter_map(|r| match r {
                crate::views::RenderRow::Data { row, .. } => Some(row.dept),
                _ => None,
            })
            .collect();
        let scores: Vec<i64> = view
            .iter()
            .filter_map(|r| match r {
                crate::views::RenderRow::Data { row, .. } => Some(row.score),
                _ => None,
            })
            .collect();
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

    #[test]
    fn set_selection_deselect_last_row_empties_selection() {
        let s = make_state();
        let id = s.rows[0].0;
        let s2 = set_selection(&s, id, true);
        assert!(!s2.selection.is_empty());
        let s3 = set_selection(&s2, id, false);
        assert!(s3.selection.is_empty());
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

    // ---- select_all_visible_page / select_all_filtered / deselect_all_visible_page / deselect_all ---

    #[test]
    fn select_all_visible_page_covers_current_page_only() {
        let mut s = make_state();
        s.page_size = 2; // 3 rows → pages [0,1], [2]
        let s2 = select_all_visible_page(&s);
        assert_eq!(s2.selection.len(), 2); // only page 0
    }

    #[test]
    fn select_all_filtered_covers_all_pages() {
        let mut s = make_state();
        s.page_size = 2;
        let s2 = select_all_filtered(&s);
        assert_eq!(s2.selection.len(), 3); // all rows across all pages
    }

    #[test]
    fn deselect_all_visible_page_leaves_other_pages_intact() {
        let mut s = make_state();
        s.page_size = 2;
        // Select all rows first
        let s = select_all_filtered(&s);
        assert_eq!(s.selection.len(), 3);
        // Deselect just the current page (page 0: rows[0], rows[1])
        let s2 = deselect_all_visible_page(&s);
        assert_eq!(s2.selection.len(), 1); // 3 - 2 = 1
        // Verify the remaining selected row is rows[2] (from page 1)
        assert!(!s2.selection.contains(&s.rows[0].0));
        assert!(!s2.selection.contains(&s.rows[1].0));
        assert!(s2.selection.contains(&s.rows[2].0));
    }

    #[test]
    fn deselect_all_clears_everything() {
        let mut s = make_state();
        s.page_size = 2;
        let s = select_all_filtered(&s);
        assert_eq!(s.selection.len(), 3);
        let s2 = deselect_all(&s);
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

    // ---- reset_column_width ------------------------------------------------

    #[test]
    fn reset_column_width_removes_override() {
        let s = make_state();
        let col = col_name();
        let s2 = set_column_width(&s, col, 250.0).expect("valid width");
        assert_eq!(s2.column_widths.get(&col), Some(&250.0));
        let s3 = reset_column_width(&s2, col);
        assert!(!s3.column_widths.contains_key(&col));
    }

    #[test]
    fn reset_column_width_unknown_col_is_noop() {
        let s = make_state();
        let unknown = ColumnId("unknown_col");
        let s2 = reset_column_width(&s, unknown);
        assert_eq!(s2.column_widths, s.column_widths);
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

    // ── Item 15: set_active_cell ─────────────────────────────────────────────

    #[test]
    fn set_active_cell_happy_path() {
        let s = make_state();
        let s2 = set_active_cell(&s, 0, col_name()).unwrap();
        assert_eq!(
            s2.active_cell,
            Some(crate::types::ActiveCell::new(0, col_name()))
        );
    }

    #[test]
    fn set_active_cell_row_out_of_bounds_errors() {
        let s = make_state();
        let result = set_active_cell(&s, 999, col_name());
        assert!(matches!(
            result,
            Err(crate::error::StateError::RowIndexOutOfBounds)
        ));
    }

    #[test]
    fn set_active_cell_unknown_column_errors() {
        let s = make_state();
        let result = set_active_cell(&s, 0, ColumnId("unknown"));
        assert!(matches!(
            result,
            Err(crate::error::StateError::ColumnNotFound)
        ));
    }

    #[test]
    fn set_active_cell_hidden_column_errors() {
        let mut s = make_state();
        s.column_visibility.insert(col_name(), false);
        let result = set_active_cell(&s, 0, col_name());
        assert!(matches!(
            result,
            Err(crate::error::StateError::ColumnNotFound)
        ));
    }

    #[test]
    fn set_active_cell_replaces_previous() {
        let s = make_state();
        let s = set_active_cell(&s, 0, col_name()).unwrap();
        let s2 = set_active_cell(&s, 1, col_score()).unwrap();
        assert_eq!(
            s2.active_cell,
            Some(crate::types::ActiveCell::new(1, col_score()))
        );
    }

    #[test]
    fn set_active_cell_clears_range_selection() {
        let s = make_state();
        let s2 = start_range_selection(&s, 0, col_name());
        let s3 = extend_range_to(&s2, 1, col_score());
        assert!(!s3.range_selection.is_empty());
        let s4 = set_active_cell(&s3, 0, col_name()).unwrap();
        assert!(s4.range_selection.is_empty(), "set_active_cell must clear range_selection");
        assert_eq!(s4.active_cell.map(|ac| ac.row_idx), Some(0));
    }

    // ── Item 15: move_active_cell ────────────────────────────────────────────

    #[test]
    fn move_active_cell_down_one_step() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Down);
        assert_eq!(s2.active_cell.unwrap().row_idx, 1);
    }

    #[test]
    fn move_active_cell_up_one_step() {
        let s = set_active_cell(&make_state(), 2, col_name()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Up);
        assert_eq!(s2.active_cell.unwrap().row_idx, 1);
    }

    #[test]
    fn move_active_cell_left_one_step() {
        let s = set_active_cell(&make_state(), 0, col_score()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Left);
        assert_eq!(s2.active_cell.unwrap().column_id, col_name());
    }

    #[test]
    fn move_active_cell_right_one_step() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Right);
        assert_eq!(s2.active_cell.unwrap().column_id, col_score());
    }

    #[test]
    fn move_active_cell_clamps_at_top() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Up);
        assert_eq!(s2.active_cell.unwrap().row_idx, 0);
    }

    #[test]
    fn move_active_cell_clamps_at_bottom() {
        let s = make_state(); // 3 rows
        let s = set_active_cell(&s, 2, col_name()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Down);
        assert_eq!(s2.active_cell.unwrap().row_idx, 2);
    }

    #[test]
    fn move_active_cell_clamps_at_left_column() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Left);
        assert_eq!(s2.active_cell.unwrap().column_id, col_name());
    }

    #[test]
    fn move_active_cell_clamps_at_right_column() {
        let s = set_active_cell(&make_state(), 0, col_score()).unwrap();
        let s2 = move_active_cell(&s, NavDirection::Right);
        assert_eq!(s2.active_cell.unwrap().column_id, col_score());
    }

    #[test]
    fn move_active_cell_none_down_goes_to_first() {
        let s = make_state(); // active_cell is None
        let s2 = move_active_cell(&s, NavDirection::Down);
        let ac = s2.active_cell.unwrap();
        assert_eq!(ac.row_idx, 0);
        assert_eq!(ac.column_id, col_name());
    }

    #[test]
    fn move_active_cell_none_up_goes_to_last() {
        let s = make_state(); // 3 rows, active_cell None
        let s2 = move_active_cell(&s, NavDirection::Up);
        let ac = s2.active_cell.unwrap();
        assert_eq!(ac.row_idx, 2);
        assert_eq!(ac.column_id, col_score());
    }

    // ── Item 15: move_active_cell_to_edge ────────────────────────────────────

    #[test]
    fn move_active_cell_to_edge_down_goes_to_last_row() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = move_active_cell_to_edge(&s, NavDirection::Down);
        assert_eq!(s2.active_cell.unwrap().row_idx, 2);
    }

    #[test]
    fn move_active_cell_to_edge_right_goes_to_last_col() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = move_active_cell_to_edge(&s, NavDirection::Right);
        assert_eq!(s2.active_cell.unwrap().column_id, col_score());
    }

    #[test]
    fn move_active_cell_to_edge_up_goes_to_row_zero() {
        let s = set_active_cell(&make_state(), 2, col_name()).unwrap();
        let s2 = move_active_cell_to_edge(&s, NavDirection::Up);
        assert_eq!(s2.active_cell.unwrap().row_idx, 0);
    }

    #[test]
    fn move_active_cell_to_edge_left_goes_to_first_col() {
        let s = set_active_cell(&make_state(), 0, col_score()).unwrap();
        let s2 = move_active_cell_to_edge(&s, NavDirection::Left);
        assert_eq!(s2.active_cell.unwrap().column_id, col_name());
    }

    // ── Item 15: move_active_cell_page ───────────────────────────────────────

    #[test]
    fn move_active_cell_page_down_by_page_size() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap(); // 3 rows
        let s2 = move_active_cell_page(&s, NavDirection::Down, 2);
        assert_eq!(s2.active_cell.unwrap().row_idx, 2); // clamped at 2
    }

    #[test]
    fn move_active_cell_page_up_clamped_at_zero() {
        let s = set_active_cell(&make_state(), 1, col_name()).unwrap();
        let s2 = move_active_cell_page(&s, NavDirection::Up, 10);
        assert_eq!(s2.active_cell.unwrap().row_idx, 0);
    }

    #[test]
    fn move_active_cell_page_horizontal_is_noop() {
        let s = set_active_cell(&make_state(), 1, col_name()).unwrap();
        let s2 = move_active_cell_page(&s, NavDirection::Left, 2);
        assert_eq!(s2.active_cell, s.active_cell);
    }

    // ── Item 15: home / end / first / last ───────────────────────────────────

    #[test]
    fn move_active_cell_home_goes_to_first_col_same_row() {
        let s = set_active_cell(&make_state(), 1, col_score()).unwrap();
        let s2 = move_active_cell_home(&s);
        let ac = s2.active_cell.unwrap();
        assert_eq!(ac.row_idx, 1);
        assert_eq!(ac.column_id, col_name());
    }

    #[test]
    fn move_active_cell_end_goes_to_last_col_same_row() {
        let s = set_active_cell(&make_state(), 1, col_name()).unwrap();
        let s2 = move_active_cell_end(&s);
        let ac = s2.active_cell.unwrap();
        assert_eq!(ac.row_idx, 1);
        assert_eq!(ac.column_id, col_score());
    }

    #[test]
    fn move_active_cell_first_goes_to_row0_col0() {
        let s = set_active_cell(&make_state(), 2, col_score()).unwrap();
        let s2 = move_active_cell_first(&s);
        let ac = s2.active_cell.unwrap();
        assert_eq!(ac.row_idx, 0);
        assert_eq!(ac.column_id, col_name());
    }

    #[test]
    fn move_active_cell_last_goes_to_last_row_last_col() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = move_active_cell_last(&s);
        let ac = s2.active_cell.unwrap();
        assert_eq!(ac.row_idx, 2);
        assert_eq!(ac.column_id, col_score());
    }

    // ── Item 15: clear_active_cell ───────────────────────────────────────────

    #[test]
    fn clear_active_cell_sets_none() {
        let s = set_active_cell(&make_state(), 0, col_name()).unwrap();
        let s2 = clear_active_cell(&s);
        assert!(s2.active_cell.is_none());
    }

    #[test]
    fn clear_active_cell_idempotent() {
        let s = make_state();
        let s2 = clear_active_cell(&s);
        let s3 = clear_active_cell(&s2);
        assert_eq!(s2.active_cell, s3.active_cell);
    }

    #[test]
    fn move_active_cell_never_produces_out_of_bounds_row() {
        let s = make_state(); // 3 rows
                              // Move to row 2 (last) then try Down — must stay at 2
        let s = set_active_cell(&s, 2, col_name()).unwrap();
        for _ in 0..10 {
            let s2 = move_active_cell(&s, NavDirection::Down);
            assert!(s2.active_cell.unwrap().row_idx <= 2);
        }
    }

    // ── Item 16: range selection ─────────────────────────────────────────────

    #[test]
    fn start_range_selection_creates_single_cell_range() {
        let s = make_state();
        let s2 = start_range_selection(&s, 1, col_name());
        assert_eq!(s2.range_selection.len(), 1);
        let r = &s2.range_selection[0];
        assert_eq!(r.anchor, (1, col_name()));
        assert_eq!(r.focus, (1, col_name()));
        assert_eq!(
            s2.active_cell,
            Some(crate::types::ActiveCell::new(1, col_name()))
        );
    }

    #[test]
    fn start_range_selection_replaces_existing() {
        let s = make_state();
        let s = start_range_selection(&s, 0, col_name());
        let s2 = start_range_selection(&s, 2, col_score());
        assert_eq!(s2.range_selection.len(), 1);
        assert_eq!(s2.range_selection[0].anchor.0, 2);
    }

    #[test]
    fn start_range_selection_clamps_out_of_bounds_row() {
        let s = make_state(); // 3 rows
        let s2 = start_range_selection(&s, 999, col_name());
        assert_eq!(s2.range_selection[0].anchor.0, 2); // clamped to last
    }

    #[test]
    fn extend_range_to_changes_focus() {
        let s = start_range_selection(&make_state(), 0, col_name());
        let s2 = extend_range_to(&s, 2, col_score());
        let r = &s2.range_selection[0];
        assert_eq!(r.anchor, (0, col_name()));
        assert_eq!(r.focus, (2, col_score()));
    }

    #[test]
    fn extend_range_to_same_cell_anchor_equals_focus() {
        let s = start_range_selection(&make_state(), 1, col_name());
        let s2 = extend_range_to(&s, 1, col_name());
        let r = &s2.range_selection[0];
        assert_eq!(r.anchor, r.focus);
    }

    #[test]
    fn extend_range_to_from_empty_creates_range() {
        let s = make_state(); // range_selection is empty
        let s2 = extend_range_to(&s, 1, col_name());
        assert_eq!(s2.range_selection.len(), 1);
    }

    #[test]
    fn add_disjoint_range_appends() {
        let s = start_range_selection(&make_state(), 0, col_name());
        let s2 = add_disjoint_range(&s, 2, col_score());
        assert_eq!(s2.range_selection.len(), 2);
    }

    #[test]
    fn extend_range_to_extends_last_rect_only() {
        let s = start_range_selection(&make_state(), 0, col_name());
        let s = add_disjoint_range(&s, 2, col_score());
        let s2 = extend_range_to(&s, 1, col_score());
        // first rect anchor unchanged
        assert_eq!(s2.range_selection[0].anchor, (0, col_name()));
        // second rect focus changed
        assert_eq!(s2.range_selection[1].focus.0, 1);
    }

    #[test]
    fn select_all_spans_all_rows_and_columns() {
        let s = make_state(); // 3 rows, 2 columns
        let s2 = select_all(&s);
        assert_eq!(s2.range_selection.len(), 1);
        let r = &s2.range_selection[0];
        assert_eq!(r.anchor, (0, col_name()));
        assert_eq!(r.focus, (2, col_score()));
    }

    #[test]
    fn clear_range_selection_empties_vec() {
        let s = start_range_selection(&make_state(), 0, col_name());
        let s2 = clear_range_selection(&s);
        assert!(s2.range_selection.is_empty());
    }

    #[test]
    fn clear_range_selection_idempotent() {
        let s = make_state();
        let s2 = clear_range_selection(&s);
        let s3 = clear_range_selection(&s2);
        assert!(s3.range_selection.is_empty());
    }

    // ── Item MD-A: toggle_row_expansion ──────────────────────────────────────

    #[test]
    fn toggle_row_expansion_adds_and_removes() {
        let s = make_state();
        let id = s.rows[0].0;
        let s2 = toggle_row_expansion(&s, id);
        assert!(s2.expanded_rows.contains(&id));
        let s3 = toggle_row_expansion(&s2, id);
        assert!(!s3.expanded_rows.contains(&id));
    }

    #[test]
    fn toggle_row_expansion_is_pure() {
        let s = make_state();
        let id = s.rows[0].0;
        let _ = toggle_row_expansion(&s, id);
        assert!(s.expanded_rows.is_empty());
    }

    #[test]
    fn collapse_all_rows_clears() {
        let s = make_state();
        let id0 = s.rows[0].0;
        let id1 = s.rows.get(1).map(|r| r.0).unwrap_or(id0);
        let s2 = toggle_row_expansion(&s, id0);
        let s3 = toggle_row_expansion(&s2, id1);
        let s4 = collapse_all_rows(&s3);
        assert!(s4.expanded_rows.is_empty());
    }
}
