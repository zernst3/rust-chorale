/// Errors that can occur during fallible state transitions.
///
/// Per ROBUSTNESS-1: one variant per distinct failure mode, no catch-all.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StateError {
    #[error("page {0} is out of range")]
    PageOutOfRange(usize),

    #[error("page size cannot be zero")]
    PageSizeZero,

    #[error("column width must be positive, got {0}")]
    InvalidColumnWidth(String),
}
