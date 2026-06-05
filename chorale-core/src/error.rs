/// Errors that can occur during fallible state transitions.
///
/// Per ROBUSTNESS-1: one variant per distinct failure mode, no catch-all.
#[non_exhaustive]
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StateError {
    /// The requested page index exceeds the number of available pages.
    #[error("page {0} is out of range")]
    PageOutOfRange(usize),

    /// `set_page_size` was called with a value of zero.
    #[error("page size cannot be zero")]
    PageSizeZero,

    /// `set_column_width` was called with a non-positive value.
    #[error("column width must be positive, got {0}")]
    InvalidColumnWidth(String),

    /// `start_edit` was called for a column that has no `EditorKind` configured.
    #[error("column {0} is not editable")]
    ColumnNotEditable(crate::types::ColumnId),

    /// `set_column_order` or `move_column` received a `ColumnId` not found in `state.columns`.
    #[error("unknown column {0}")]
    UnknownColumnId(crate::types::ColumnId),

    /// `set_column_order` received a duplicate `ColumnId`.
    #[error("duplicate column {0}")]
    DuplicateColumnId(crate::types::ColumnId),

    /// A transition was called in a mode that does not support it.
    ///
    /// Example: `set_page` in `PaginationMode::InfiniteScroll`,
    /// or `load_more_rows` in `PaginationMode::Pages`.
    #[error("transition not valid in current pagination mode")]
    InvalidModeForTransition,
}
