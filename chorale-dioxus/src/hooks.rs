//! Dioxus hooks for chorale tables.

use chorale_core::{
    clear_sort, move_column, remove_sort, set_column_visibility, set_column_width, set_filter,
    set_page, set_page_size, set_scroll, set_selection, toggle_select_all, toggle_sort,
    update_row, ColumnId, FilterValue, RowId, SortAction, StateError, TableState,
};
use dioxus::prelude::*;

/// A reactive handle to a chorale table, returned by [`use_table`].
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

    /// Move `column_id` to `to_index` in the render order.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::UnknownColumnId`] if `column_id` is not found.
    pub fn move_column(&self, column_id: ColumnId, to_index: usize) -> Result<(), StateError> {
        self.try_dispatch(|s| move_column(s, column_id, to_index))
    }

    // -------------------------------------------------------------------------
    // Private dispatch helpers
    // -------------------------------------------------------------------------

    fn dispatch(&self, f: impl FnOnce(&TableState<TRow>) -> TableState<TRow>) {
        let mut s = self.inner;
        // Read guard dropped at the end of the inner block so `set` can write.
        let new_state = {
            let guard = s.read();
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
            let guard = s.read();
            f(&*guard)?
        };
        s.set(new_state);
        Ok(())
    }
}

/// Create a reactive chorale table handle backed by a Dioxus signal.
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

    fn assert_copy<T: Copy>() {}

    #[test]
    fn handle_is_copy() {
        // UseTableHandle must be Copy so Dioxus move-closures can capture it
        // by copy rather than requiring .clone() at every event-handler site.
        assert_copy::<UseTableHandle<String>>();
    }
}
