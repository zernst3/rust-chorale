//! Leptos hooks for chorale tables.

use chorale_core::{
    clear_sort, collapse_all_groups, expand_all_groups, load_more_rows, move_column, remove_sort,
    set_column_visibility, set_column_width, set_filter, set_grouping, set_page, set_page_size,
    set_pagination_mode, set_scroll, set_selection, toggle_group, toggle_select_all, toggle_sort,
    update_row, ColumnId, FilterValue, GroupKey, PaginationMode, RowId, SortAction, StateError,
    TableState,
};
use leptos::prelude::*;

/// A reactive handle to a chorale table, returned by [`use_chorale_table`].
///
/// Wraps a [`RwSignal<TableState<TRow>>`] and exposes typed transition
/// helpers so call sites do not need to import `chorale_core::transitions`
/// directly.
///
/// `UseTableHandle<TRow>` is [`Copy`] because `RwSignal<T>` is `Copy` in
/// Leptos. Closures in Leptos event handlers can therefore capture the handle
/// without `.clone()`.
// Manual Copy impl so TRow is not required to be Copy (same pattern as chorale-dioxus).
// RwSignal<T> is Copy without T: Copy; the wrapper must follow suit.
#[derive(Clone)]
pub struct UseTableHandle<TRow>
where
    TRow: Clone + PartialEq + Send + Sync + 'static,
{
    pub(crate) signal: RwSignal<TableState<TRow>>,
}

impl<TRow: Clone + PartialEq + Send + Sync + 'static> Copy for UseTableHandle<TRow> {}

impl<TRow: Clone + PartialEq + Send + Sync + 'static> UseTableHandle<TRow> {
    /// Returns the underlying `RwSignal` for reactive reads.
    ///
    /// Use this to pass the signal to child components or to read the current
    /// state without going through a transition:
    /// `let state = handle.signal().get_untracked();`
    #[must_use]
    pub fn signal(self) -> RwSignal<TableState<TRow>> {
        self.signal
    }

    /// Cycle sort on `col` using `action` (Replace or Append).
    pub fn toggle_sort(&self, col: ColumnId, action: SortAction) {
        self.dispatch(|s| toggle_sort(s, col, action));
    }

    /// Remove `col` from the sort list. No-op if not sorted.
    pub fn remove_sort(&self, col: ColumnId) {
        self.dispatch(|s| remove_sort(s, col));
    }

    /// Clear all active sort columns.
    pub fn clear_sort(&self) {
        self.dispatch(|s| clear_sort(s));
    }

    /// Set or clear the filter on `col`.
    pub fn set_filter(&self, col: ColumnId, filter: Option<FilterValue>) {
        self.dispatch(|s| set_filter(s, col, filter));
    }

    /// Jump to page `page` (zero-based).
    ///
    /// # Errors
    ///
    /// Returns [`StateError::PageOutOfRange`] if `page >= total_pages()`.
    pub fn set_page(&self, page: usize) -> Result<(), StateError> {
        self.try_dispatch(|s| set_page(s, page))
    }

    /// Change the number of rows per page.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::PageSizeZero`] if `size == 0`.
    pub fn set_page_size(&self, size: usize) -> Result<(), StateError> {
        self.try_dispatch(|s| set_page_size(s, size))
    }

    /// Set or clear the selection of a single row.
    pub fn set_selection(&self, row_id: RowId, selected: bool) {
        self.dispatch(|s| set_selection(s, row_id, selected));
    }

    /// Select all visible rows, or deselect all if all are already selected.
    pub fn toggle_select_all(&self) {
        self.dispatch(|s| toggle_select_all(s));
    }

    /// Show or hide `col`.
    pub fn set_column_visibility(&self, col: ColumnId, visible: bool) {
        self.dispatch(|s| set_column_visibility(s, col, visible));
    }

    /// Override the pixel width of `col` for resize handles.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::InvalidColumnWidth`] if `width_px <= 0`.
    pub fn set_column_width(&self, col: ColumnId, width_px: f64) -> Result<(), StateError> {
        self.try_dispatch(|s| set_column_width(s, col, width_px))
    }

    /// Returns a clone of the current selection as a `Vec<RowId>`.
    #[must_use]
    pub fn selected_ids(&self) -> Vec<RowId> {
        self.signal.with_untracked(|s| s.selection.clone())
    }

    /// Returns the number of currently selected rows.
    #[must_use]
    pub fn selection_count(&self) -> usize {
        self.signal.with_untracked(|s| s.selection.len())
    }

    /// Update the scroll offset of the virtualized container (px).
    ///
    /// Skips the dispatch when `scroll_top` equals the current state, preventing
    /// a render storm during continuous scroll events.
    pub fn set_scroll(&self, scroll_top: f64) {
        let current = self.signal.with_untracked(|s| s.scroll_top);
        if (current - scroll_top).abs() < f64::EPSILON {
            return;
        }
        self.dispatch(|s| set_scroll(s, scroll_top));
    }

    /// Replace `row_id`'s data in-place.
    pub fn update_row(&self, row_id: RowId, new_row: TRow) {
        self.dispatch(|s| update_row(s, row_id, new_row));
    }

    /// Move `column_id` to `to_index` in the render order.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::UnknownColumnId`] if `column_id` is not found.
    pub fn move_column(&self, column_id: ColumnId, to_index: usize) -> Result<(), StateError> {
        self.try_dispatch(|s| move_column(s, column_id, to_index))
    }

    /// Switch between `PaginationMode::Pages` and `PaginationMode::InfiniteScroll`.
    pub fn set_pagination_mode(&self, mode: PaginationMode) {
        self.dispatch(|s| set_pagination_mode(s, mode));
    }

    /// Increase `loaded_row_count` by `page_size`, capped at filtered row count.
    pub fn load_more_rows(&self) {
        self.try_dispatch(load_more_rows).ok();
    }

    /// Set the columns to group by (outermost-first). Clears collapsed state.
    pub fn set_grouping(&self, columns: Vec<ColumnId>) {
        self.dispatch(|s| set_grouping(s, columns));
    }

    /// Toggle the collapsed/expanded state of a group.
    #[allow(clippy::needless_pass_by_value)]
    pub fn toggle_group(&self, key: GroupKey) {
        self.dispatch(|s| toggle_group(s, &key));
    }

    /// Expand all groups (clear `collapsed_groups`).
    pub fn expand_all_groups(&self) {
        self.dispatch(|s| expand_all_groups(s));
    }

    /// Collapse all groups.
    pub fn collapse_all_groups(&self) {
        self.dispatch(|s| collapse_all_groups(s));
    }

    // -------------------------------------------------------------------------
    // Private dispatch helpers
    // -------------------------------------------------------------------------

    fn dispatch(&self, f: impl FnOnce(&TableState<TRow>) -> TableState<TRow>) {
        let new_state = self.signal.with_untracked(f);
        self.signal.set(new_state);
    }

    fn try_dispatch(
        &self,
        f: impl FnOnce(&TableState<TRow>) -> Result<TableState<TRow>, StateError>,
    ) -> Result<(), StateError> {
        let new_state = self.signal.with_untracked(f)?;
        self.signal.set(new_state);
        Ok(())
    }
}

/// Create a reactive chorale table handle backed by a Leptos `RwSignal`.
///
/// `rows` and `columns` define the initial table state. Each row is assigned
/// a new random [`RowId`]. The returned [`UseTableHandle`] is [`Copy`] and
/// exposes typed transition methods for all v0.2 operations.
///
/// # Example
///
/// ```rust,ignore
/// let table = use_chorale_table(rows, my_columns());
/// let row_count = table.signal().with_untracked(|s| s.rows.len());
/// ```
#[must_use]
pub fn use_chorale_table<TRow>(
    rows: Vec<TRow>,
    columns: Vec<chorale_core::ColumnDef<TRow>>,
) -> UseTableHandle<TRow>
where
    TRow: Clone + PartialEq + Send + Sync + 'static,
{
    let state = TableState::new(
        rows.into_iter().map(|r| (RowId::new(), r)).collect(),
        columns,
    );
    UseTableHandle {
        signal: RwSignal::new(state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_copy<T: Copy>() {}

    #[test]
    fn handle_is_copy() {
        assert_copy::<UseTableHandle<String>>();
    }
}
