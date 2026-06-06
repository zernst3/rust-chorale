use std::sync::Arc;

/// All user-visible strings the table renders. Override any field to customise.
///
/// Construct with [`Labels::default`] and mutate the fields you want to
/// change — for example:
///
/// ```rust
/// # use chorale_core::Labels;
/// let mut labels = Labels::default();
/// labels.filter_placeholder = "Filtrer\u{2026}".into();
/// labels.export_csv_label = "Exporter CSV".into();
/// ```
///
/// `Labels` is `#[non_exhaustive]` so future minor releases can add new
/// fields without breaking callsites that use the field-mutation pattern.
#[non_exhaustive]
#[derive(Clone)]
pub struct Labels {
    // Filter bar
    /// Placeholder text for the per-column filter input. Default: `"Filter…"`.
    pub filter_placeholder: String,
    /// Title/tooltip for the "clear filter" button. Default: `"Clear Filter"`.
    pub clear_filter_label: String,

    // Pagination bar
    /// Label for the "previous page" button. Default: `"‹"`.
    pub previous_page_label: String,
    /// Label for the "next page" button. Default: `"›"`.
    pub next_page_label: String,
    /// Label preceding the goto-page input. Default: `"Go to"`.
    pub go_to_page_label: String,
    /// Label for the "show all rows" page-size option. Default: `"All"`.
    pub page_size_all_label: String,
    /// Formats the "page N of M" affordance in the goto-page control.
    ///
    /// Receives `(current_page_1indexed, total_pages)` and returns the
    /// display string. Default: `|p, t| format!("{p} of {t}")`.
    ///
    /// Override to support token-reordering languages:
    /// ```rust
    /// # use std::sync::Arc;
    /// # use chorale_core::Labels;
    /// let mut labels = Labels::default();
    /// // Japanese: "10ページ中3ページ目"
    /// labels.page_count = Arc::new(|p, t| format!("{t}\u{30da}\u{30fc}\u{30b8}\u{4e2d}{p}\u{30da}\u{30fc}\u{30b8}\u{76ee}"));
    /// ```
    pub page_count: Arc<dyn Fn(usize, usize) -> String + Send + Sync>,

    // Selection
    /// Label for the "select all" action (screen-reader / tooltip). Default: `"Select all"`.
    pub select_all_label: String,
    /// Label for the "deselect all" action. Default: `"Deselect all"`.
    pub deselect_all_label: String,

    // Column visibility toolbar
    /// Prefix label in the column visibility toolbar. Default: `"Columns"`.
    pub column_visibility_label: String,
    /// Label for the "show all columns" button. Default: `"Show all"`.
    pub show_all_columns_label: String,

    // CSV export
    /// Label for the CSV export button. Default: `"Export CSV"`.
    pub export_csv_label: String,

    // Sort (screen-reader text)
    /// Sort ascending aria-label. Default: `"Sort ascending"`.
    pub sort_ascending_label: String,
    /// Sort descending aria-label. Default: `"Sort descending"`.
    pub sort_descending_label: String,
    /// Unsorted aria-label. Default: `"Unsorted"`.
    pub sort_none_label: String,

    // Empty state
    /// Message shown when all rows are filtered out. Default: `"No rows match the current filter."`.
    pub no_rows_label: String,

    // Infinite scroll
    /// Indicator shown at the bottom of an infinite-scroll table when more
    /// rows remain to be loaded. Default: `"Scroll for more rows"`.
    ///
    /// Note: `load_more_rows` is synchronous, so this is a passive "more
    /// rows available" state indicator, not an active-fetching spinner.
    /// Consumer apps that do async fetching may override this via the
    /// `labels` prop to show `"Loading…"` (or similar) while a fetch is in
    /// flight.
    pub load_more_label: String,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            filter_placeholder: "Filter\u{2026}".into(),
            clear_filter_label: "Clear Filter".into(),
            previous_page_label: "\u{2039}".into(),
            next_page_label: "\u{203a}".into(),
            go_to_page_label: "Go to".into(),
            page_size_all_label: "All".into(),
            page_count: Arc::new(|p, t| format!("{p} of {t}")),
            select_all_label: "Select all".into(),
            deselect_all_label: "Deselect all".into(),
            column_visibility_label: "Columns".into(),
            show_all_columns_label: "Show all".into(),
            export_csv_label: "Export CSV".into(),
            sort_ascending_label: "Sort ascending".into(),
            sort_descending_label: "Sort descending".into(),
            sort_none_label: "Unsorted".into(),
            no_rows_label: "No rows match the current filter.".into(),
            load_more_label: "Scroll for more rows".into(),
        }
    }
}

impl std::fmt::Debug for Labels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Labels")
            .field("filter_placeholder", &self.filter_placeholder)
            .field("clear_filter_label", &self.clear_filter_label)
            .field("previous_page_label", &self.previous_page_label)
            .field("next_page_label", &self.next_page_label)
            .field("go_to_page_label", &self.go_to_page_label)
            .field("page_size_all_label", &self.page_size_all_label)
            .field("page_count", &"<fn>")
            .field("select_all_label", &self.select_all_label)
            .field("deselect_all_label", &self.deselect_all_label)
            .field("column_visibility_label", &self.column_visibility_label)
            .field("show_all_columns_label", &self.show_all_columns_label)
            .field("export_csv_label", &self.export_csv_label)
            .field("sort_ascending_label", &self.sort_ascending_label)
            .field("sort_descending_label", &self.sort_descending_label)
            .field("sort_none_label", &self.sort_none_label)
            .field("no_rows_label", &self.no_rows_label)
            .field("load_more_label", &self.load_more_label)
            .finish()
    }
}

impl PartialEq for Labels {
    fn eq(&self, other: &Self) -> bool {
        self.filter_placeholder == other.filter_placeholder
            && self.clear_filter_label == other.clear_filter_label
            && self.previous_page_label == other.previous_page_label
            && self.next_page_label == other.next_page_label
            && self.go_to_page_label == other.go_to_page_label
            && self.page_size_all_label == other.page_size_all_label
            && Arc::ptr_eq(&self.page_count, &other.page_count)
            && self.select_all_label == other.select_all_label
            && self.deselect_all_label == other.deselect_all_label
            && self.column_visibility_label == other.column_visibility_label
            && self.show_all_columns_label == other.show_all_columns_label
            && self.export_csv_label == other.export_csv_label
            && self.sort_ascending_label == other.sort_ascending_label
            && self.sort_descending_label == other.sort_descending_label
            && self.sort_none_label == other.sort_none_label
            && self.no_rows_label == other.no_rows_label
            && self.load_more_label == other.load_more_label
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn labels_default_english_strings() {
        let l = Labels::default();
        assert_eq!(l.filter_placeholder, "Filter\u{2026}");
        assert_eq!(l.clear_filter_label, "Clear Filter");
        assert_eq!(l.previous_page_label, "\u{2039}");
        assert_eq!(l.next_page_label, "\u{203a}");
        assert_eq!(l.go_to_page_label, "Go to");
        assert_eq!(l.page_size_all_label, "All");
        assert_eq!(l.export_csv_label, "Export CSV");
        assert_eq!(l.column_visibility_label, "Columns");
        assert_eq!(l.no_rows_label, "No rows match the current filter.");
        assert_eq!(l.select_all_label, "Select all");
        assert_eq!(l.deselect_all_label, "Deselect all");
        assert_eq!(l.sort_ascending_label, "Sort ascending");
        assert_eq!(l.sort_descending_label, "Sort descending");
        assert_eq!(l.sort_none_label, "Unsorted");
    }

    #[test]
    fn labels_page_count_default_formats_page_of_total() {
        let l = Labels::default();
        assert_eq!((l.page_count)(3, 10), "3 of 10");
        assert_eq!((l.page_count)(1, 1), "1 of 1");
        assert_eq!((l.page_count)(0, 0), "0 of 0");
    }

    #[test]
    fn labels_clone_shares_arc_and_is_equal() {
        let a = Labels::default();
        let b = a.clone();
        assert_eq!(a, b, "cloned Labels should compare equal via Arc::ptr_eq");
    }

    #[test]
    fn labels_two_defaults_are_not_equal() {
        let a = Labels::default();
        let b = Labels::default();
        assert_ne!(
            a, b,
            "independently constructed defaults have different Arc pointers for page_count"
        );
    }

    #[test]
    fn labels_string_field_mutation_reflects_in_eq() {
        let a = Labels::default();
        let mut b = a.clone();
        b.filter_placeholder = "Search…".into();
        assert_ne!(a, b);
    }
}
