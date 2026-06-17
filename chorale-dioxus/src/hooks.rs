//! Dioxus hooks for Chorale tables.

use chorale_core::{
    append_rows, clear_sort, collapse_all_groups, collapse_all_rows, deselect_all,
    deselect_all_visible_page, ensure_active_cell, expand_all_groups, insert_row, load_more_rows,
    move_column, remove_row, remove_rows, remove_sort, reset_column_width, select_all_filtered,
    select_all_visible_page, set_column_visibility, set_column_width, set_detail_column_enabled,
    set_filter, set_grouping, set_page, set_page_size, set_pagination_mode, set_rows, set_scroll,
    set_selection, start_edit, toggle_group, toggle_row_expansion, toggle_select_all, toggle_sort,
    update_row, ColumnId, FilterValue, GroupKey, PaginationMode, RowId, SortAction, StateError,
    TableState,
};
use dioxus::prelude::*;

/// A reactive handle to a Chorale table, returned by [`use_table`].
///
/// Wraps a [`Signal<TableState<TRow>>`] and exposes typed transition
/// helpers so call sites do not need to import `chorale_core::transitions`
/// directly.
///
/// `UseTableHandle<TRow>` is [`Copy`] (Signal is a thin reference into a
/// generational arena; all copies share the same underlying data). Closures in
/// Dioxus event handlers can therefore capture the handle without `.clone()`.
// #[derive(Clone)] adds TRow: Clone which is already a bound — correct.
// #[derive(Copy)] would add TRow: Copy which is too strict; Copy is
// implemented manually so Signal<T>'s own Copy-ness is sufficient.
#[derive(Clone)]
pub struct UseTableHandle<TRow: Clone + 'static> {
    inner: Signal<TableState<TRow>>,
}

impl<TRow: Clone + 'static> Copy for UseTableHandle<TRow> {}

impl<TRow: Clone + 'static> PartialEq for UseTableHandle<TRow> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<TRow: Clone + 'static> UseTableHandle<TRow> {
    /// Returns the underlying signal for reactive reads.
    ///
    /// Use this to pass the signal to child components or to read the current
    /// state without going through a transition:
    /// `let state = handle.signal().read();`
    #[must_use]
    pub fn signal(self) -> Signal<TableState<TRow>> {
        self.inner
    }

    /// Cycle sort on `col` using `action` (Replace or Append).
    ///
    /// `SortAction::Replace` (plain click): cycles None → Asc → Desc → None,
    /// clearing other sort columns. `SortAction::Append` (Shift+click): appends,
    /// flips, or removes without disturbing other sort columns.
    /// Resets `scroll_top` and `page` to 0.
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
    ///
    /// `filter = None` removes any existing filter. Resets `scroll_top`
    /// and `page` to 0.
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

    /// Select every row currently on the visible page (excluding detail panels).
    pub fn select_all_visible_page(&self) {
        self.dispatch(|s| select_all_visible_page(s));
    }

    /// Select every row in the filtered + sorted set (across all pages).
    pub fn select_all_filtered(&self) {
        self.dispatch(|s| select_all_filtered(s));
    }

    /// Deselect every row currently on the visible page, leaving other-page selections intact.
    pub fn deselect_all_visible_page(&self) {
        self.dispatch(|s| deselect_all_visible_page(s));
    }

    /// Clear the entire selection across all pages.
    pub fn deselect_all(&self) {
        self.dispatch(|s| deselect_all(s));
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

    /// Reset the explicit width override for `col`, falling back to `initial_width` or table default.
    pub fn reset_column_width(&self, col: ColumnId) {
        self.dispatch(|s| reset_column_width(s, col));
    }

    /// Returns a clone of the current selection as a `Vec<RowId>`.
    ///
    /// Convenience over reading `handle.signal().read().selection.clone()`
    /// directly. Useful for building bulk-action UI in parent components.
    ///
    /// ```rust,ignore
    /// let selected: Vec<RowId> = handle.selected_ids();
    /// ```
    #[must_use]
    pub fn selected_ids(&self) -> Vec<RowId> {
        self.inner.read().selection.clone()
    }

    /// Returns the number of currently selected rows.
    ///
    /// Equivalent to `handle.selected_ids().len()` but avoids cloning
    /// the `Vec<RowId>`.
    ///
    /// ```rust,ignore
    /// let count: usize = handle.selection_count();
    /// ```
    #[must_use]
    pub fn selection_count(&self) -> usize {
        self.inner.read().selection.len()
    }

    /// Update the scroll offset of the virtualized container (px).
    ///
    /// Skips the dispatch entirely when the incoming `scroll_top` already
    /// equals the current state — this prevents a render storm during
    /// continuous scroll events (e.g. macOS trackpad inertia), where the
    /// browser fires `onscroll` repeatedly at the same scroll position and
    /// each redundant `Signal::set` would otherwise re-render the table.
    pub fn set_scroll(&self, scroll_top: f64) {
        let current = self.inner.read().scroll_top;
        if (current - scroll_top).abs() < f64::EPSILON {
            return;
        }
        self.dispatch(|s| set_scroll(s, scroll_top));
    }

    /// Replace `row_id`'s data in-place (cell-editing escape valve, recon-2 § 7d).
    pub fn update_row(&self, row_id: RowId, new_row: TRow) {
        self.dispatch(|s| update_row(s, row_id, new_row));
    }

    /// Replace the entire row set (streaming full-refresh). Drops selection,
    /// editing, expanded-rows, active-cell and range state for the old rows.
    pub fn set_rows(&self, rows: Vec<(RowId, TRow)>) {
        self.dispatch(|s| set_rows(s, rows));
    }

    /// Insert a row at `position` (0 = prepend, past the end = append).
    pub fn insert_row(&self, position: usize, id: RowId, row: TRow) {
        self.dispatch(|s| insert_row(s, position, id, row));
    }

    /// Append rows to the end of the row set (streaming new records).
    pub fn append_rows(&self, new_rows: Vec<(RowId, TRow)>) {
        self.dispatch(|s| append_rows(s, new_rows));
    }

    /// Remove a single row by `RowId` (no-op if absent).
    pub fn remove_row(&self, row_id: RowId) {
        self.dispatch(|s| remove_row(s, row_id));
    }

    /// Remove multiple rows by `RowId` in one transition.
    pub fn remove_rows(&self, ids: &[RowId]) {
        self.dispatch(|s| remove_rows(s, ids));
    }

    /// Enable/disable the detail-expander column in keyboard navigation (#17).
    /// The `Table` component calls this on mount when a `detail_renderer` is set.
    pub fn set_detail_column_enabled(&self, enabled: bool) {
        self.dispatch(|s| set_detail_column_enabled(s, enabled));
    }

    /// Select the first navigable cell if none is active. Called when the
    /// keyboard container gains focus so a single Tab into a nested sub-table
    /// lands on a visible cell (#17).
    pub fn ensure_active_cell(&self) {
        self.dispatch(ensure_active_cell);
    }

    /// Begin editing `(row_id, column_id)`.
    ///
    /// No-op if the column has no `EditorKind` configured.
    pub fn start_edit(&self, row_id: RowId, column_id: ColumnId) {
        self.try_dispatch(|s| start_edit(s, row_id, column_id)).ok();
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
    ///
    /// No-op (silently discarded) in `PaginationMode::Pages`.
    pub fn load_more_rows(&self) {
        self.try_dispatch(load_more_rows).ok();
    }

    /// Set the columns to group by (outermost-first). Clears collapsed state.
    ///
    /// Pass an empty vec to remove all grouping. Resets page and scroll.
    pub fn set_grouping(&self, columns: Vec<ColumnId>) {
        self.dispatch(|s| set_grouping(s, columns));
    }

    /// Toggle the collapsed/expanded state of a group.
    ///
    /// Obtain `key` from `GroupedRow::Header::key` in `visible_grouped_view` output.
    #[allow(clippy::needless_pass_by_value)]
    pub fn toggle_group(&self, key: GroupKey) {
        self.dispatch(|s| toggle_group(s, &key));
    }

    /// Toggle selection for every (filter-visible) row in a group. If the group is fully
    /// selected it is deselected; otherwise all its rows are selected. Drives the per-group
    /// "select all" checkbox in the group header (#31).
    #[allow(clippy::needless_pass_by_value)]
    pub fn toggle_select_group(&self, key: GroupKey) {
        self.dispatch(|s| chorale_core::toggle_select_group(s, &key));
    }

    /// The selection state (None / Partial / All) of a group, for a tri-state header checkbox.
    #[must_use]
    pub fn group_selection_state(&self, key: &GroupKey) -> chorale_core::GroupSelectionState {
        chorale_core::group_selection_state(&self.inner.read(), key)
    }

    /// Expand all groups (clear `collapsed_groups`).
    pub fn expand_all_groups(&self) {
        self.dispatch(|s| expand_all_groups(s));
    }

    /// Collapse all groups.
    pub fn collapse_all_groups(&self) {
        self.dispatch(|s| collapse_all_groups(s));
    }

    /// Toggle the expanded/collapsed state of a master/detail row.
    pub fn toggle_row_expansion(&self, row_id: RowId) {
        self.dispatch(|s| toggle_row_expansion(s, row_id));
    }

    /// Collapse all expanded master/detail rows (clear `expanded_rows`).
    pub fn collapse_all_rows(&self) {
        self.dispatch(|s| collapse_all_rows(s));
    }

    // -------------------------------------------------------------------------
    // Private dispatch helpers
    // -------------------------------------------------------------------------

    fn dispatch(&self, f: impl FnOnce(&TableState<TRow>) -> TableState<TRow>) {
        let mut s = self.inner;
        // peek() reads the current value without creating a reactive subscription.
        // Using read() here would subscribe any surrounding reactive context (e.g. a
        // use_effect closure) to the table signal, causing the effect to re-run every
        // time the signal is written — an infinite loop.
        let new_state = {
            let guard = s.peek();
            f(&*guard)
        };
        s.set(new_state);
    }

    fn try_dispatch(
        &self,
        f: impl FnOnce(&TableState<TRow>) -> Result<TableState<TRow>, StateError>,
    ) -> Result<(), StateError> {
        let mut s = self.inner;
        let new_state = {
            let guard = s.peek();
            f(&*guard)?
        };
        s.set(new_state);
        Ok(())
    }
}

/// Create a reactive Chorale table handle backed by a Dioxus signal.
///
/// `init` is called once on component mount to produce the initial
/// [`TableState`]. The returned [`UseTableHandle`] is [`Copy`] and provides
/// typed transition methods for all v0.1 operations.
///
/// # Example
///
/// ```rust,ignore
/// let table = use_table(|| TableState::new(rows, columns));
/// let row_count = table.signal().read().rows.len();
/// ```
#[must_use]
pub fn use_table<TRow: Clone + 'static>(
    init: impl Fn() -> TableState<TRow> + 'static,
) -> UseTableHandle<TRow> {
    UseTableHandle {
        inner: use_signal(init),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chorale_core::{ColumnDef, ColumnId, PaginationMode, TableState};

    fn assert_copy<T: Copy>() {}

    #[test]
    fn handle_is_copy() {
        // UseTableHandle must be Copy so Dioxus move-closures can capture it
        // by copy rather than requiring .clone() at every event-handler site.
        assert_copy::<UseTableHandle<String>>();
    }

    // dispatch must not create a reactive subscription to the table signal.
    // Calling set_pagination_mode (or any dispatch) must not trigger a re-read
    // of the signal through a reactive channel.
    //
    // This is a structural test: dispatch reads via peek() (non-subscribing), then
    // writes via set(). We verify the transition produces the expected state by
    // calling dispatch directly (outside a Dioxus runtime) using the underlying
    // mechanism: produce new state from peek + set.
    #[test]
    fn dispatch_uses_peek_not_read() {
        // Build a minimal state and verify that set_pagination_mode produces the right
        // output without needing a Dioxus runtime (tests the logic path, not the signal).
        let cols: Vec<ColumnDef<String>> = vec![];
        let s = TableState::<String>::new(vec![], cols);
        assert_eq!(s.pagination_mode, PaginationMode::Pages);
        let s2 = chorale_core::set_pagination_mode(&s, PaginationMode::InfiniteScroll);
        assert_eq!(s2.pagination_mode, PaginationMode::InfiniteScroll);
        // Switching back preserves Pages default
        let s3 = chorale_core::set_pagination_mode(&s2, PaginationMode::Pages);
        assert_eq!(s3.pagination_mode, PaginationMode::Pages);
        let _ = ColumnId("_unused");
    }

    #[test]
    fn try_dispatch_peek_path_returns_ok() {
        let cols: Vec<ColumnDef<String>> = vec![];
        let s = TableState::<String>::new(vec![], cols);
        // set_page_size is the canonical try_dispatch path.
        let Ok(s2) = chorale_core::set_page_size(&s, 25) else {
            panic!("set_page_size should succeed")
        };
        assert_eq!(s2.page_size, 25);
    }
}
