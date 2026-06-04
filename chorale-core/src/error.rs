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
}
