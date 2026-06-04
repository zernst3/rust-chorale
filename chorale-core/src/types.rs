use std::collections::HashSet;

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

/// Opaque identifier for a row. Uses a UUID internally for global uniqueness.
///
/// Per ROBUSTNESS-1: newtype over bare `Uuid` so call sites cannot accidentally
/// pass a wrong UUID where a `RowId` is expected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowId(Uuid);

impl RowId {
    /// Create a new random `RowId`.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Expose the inner UUID (e.g. for serialization).
    #[must_use]
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for RowId {
    fn default() -> Self {
        Self::new()
    }
}

/// Opaque column identifier. Uses a `&'static str` so column IDs are
/// zero-cost to copy and usable as `HashMap` keys without heap allocation.
///
/// Per ROBUSTNESS-1: newtype so column IDs and arbitrary strings don't
/// accidentally substitute for each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColumnId(pub &'static str);

impl ColumnId {
    /// Return the underlying string slice.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for ColumnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Sort direction for a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending order (smallest first).
    Asc,
    /// Descending order (largest first).
    Desc,
}

/// Active sort on a single column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortState {
    /// The column being sorted.
    pub column: ColumnId,
    /// Whether the sort is ascending or descending.
    pub direction: SortDirection,
}

/// A filter value that can be applied to a column.
///
/// The filter type is matched against the `CellValue` returned by the
/// column's accessor; mismatched types are treated as "no filter" (all rows
/// pass).
///
/// The variant is paired with a [`crate::column::FilterKind`] on the column
/// definition: the column declares which UI to render and how to interpret
/// the filter; the `FilterValue` carries the user's current selection.
#[derive(Clone, Debug, PartialEq)]
pub enum FilterValue {
    /// Free-text substring match (case-insensitive). Paired with `FilterKind::Text`.
    Text(String),
    /// Numeric range bounds. Either bound is optional. Paired with `FilterKind::NumericRange`.
    NumericRange {
        /// Inclusive lower bound. `None` means no lower bound.
        min: Option<f64>,
        /// Inclusive upper bound. `None` means no upper bound.
        max: Option<f64>,
    },
    /// Date range bounds. Either bound is optional. Paired with `FilterKind::DateRange`.
    DateRange {
        /// Inclusive start date. `None` means no lower bound.
        min: Option<NaiveDate>,
        /// Inclusive end date. `None` means no upper bound.
        max: Option<NaiveDate>,
    },
    /// Set of allowed text values. Paired with `FilterKind::MultiSelect`. An empty
    /// set passes all rows (treated as "no filter active").
    MultiSelect(HashSet<String>),
    /// Match cells whose boolean value equals `want`. Paired with `FilterKind::Boolean`.
    Boolean(bool),
}

/// Horizontal alignment for a column's cells and header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Alignment {
    /// Left-align cell content (the default).
    #[default]
    Left,
    /// Center-align cell content.
    Center,
    /// Right-align cell content.
    Right,
}

/// ISO 4217 currency code. Used by `RenderKind::Currency`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrencyCode(pub &'static str);

impl CurrencyCode {
    /// US Dollar.
    pub const USD: Self = Self("USD");
    /// Euro.
    pub const EUR: Self = Self("EUR");
    /// British Pound Sterling.
    pub const GBP: Self = Self("GBP");
}

/// The typed value returned by a column's accessor closure.
///
/// The adapter uses `CellValue` together with `RenderKind` to decide how to
/// display a cell. `chorale-core` also uses it for sort comparisons, filter
/// matching, and CSV serialization.
///
/// Defined in recon-2 § 7a.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    /// UTF-8 text.
    Text(String),
    /// 64-bit signed integer.
    Integer(i64),
    /// 64-bit IEEE 754 floating-point number.
    Float(f64),
    /// Boolean true/false.
    Boolean(bool),
    /// Calendar date (no time component).
    Date(NaiveDate),
    /// Date + time in UTC.
    DateTime(DateTime<Utc>),
    /// No value / missing data. Sorts last; renders as an empty string.
    Empty,
}

impl CellValue {
    /// Compare two `CellValue`s for sort ordering.
    ///
    /// Same-type values use their natural ordering. Mixed types fall back to
    /// lexicographic comparison of their debug strings so sorting never panics.
    #[must_use]
    pub fn cmp_for_sort(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            (Self::Integer(a), Self::Integer(b)) => a.cmp(b),
            (Self::Float(a), Self::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Self::Boolean(a), Self::Boolean(b)) => a.cmp(b),
            (Self::Date(a), Self::Date(b)) => a.cmp(b),
            (Self::DateTime(a), Self::DateTime(b)) => a.cmp(b),
            (Self::Empty, Self::Empty) => Ordering::Equal,
            // Empty sorts last
            (Self::Empty, _) => Ordering::Greater,
            (_, Self::Empty) => Ordering::Less,
            // Cross-type: fall back to display string comparison
            _ => format!("{self:?}").cmp(&format!("{other:?}")),
        }
    }

    /// Return true if this value matches `filter`.
    #[must_use]
    pub fn matches_filter(&self, filter: &FilterValue) -> bool {
        match (self, filter) {
            (Self::Text(s), FilterValue::Text(q)) => s.to_lowercase().contains(&q.to_lowercase()),
            (Self::Text(s), FilterValue::MultiSelect(set)) => set.is_empty() || set.contains(s),
            (Self::Integer(n), FilterValue::NumericRange { min, max }) => {
                #[allow(clippy::cast_precision_loss)]
                let n = *n as f64;
                min.map_or(true, |m| n >= m) && max.map_or(true, |m| n <= m)
            }
            (Self::Float(n), FilterValue::NumericRange { min, max }) => {
                min.map_or(true, |m| *n >= m) && max.map_or(true, |m| *n <= m)
            }
            (Self::Date(d), FilterValue::DateRange { min, max }) => {
                min.map_or(true, |m| *d >= m) && max.map_or(true, |m| *d <= m)
            }
            (Self::DateTime(dt), FilterValue::DateRange { min, max }) => {
                let date = dt.date_naive();
                min.map_or(true, |m| date >= m) && max.map_or(true, |m| date <= m)
            }
            (Self::Boolean(b), FilterValue::Boolean(want)) => *b == *want,
            // No matching filter type → pass (don't filter this cell)
            _ => true,
        }
    }

    /// Render the value as a CSV-safe string (no quoting; caller handles that).
    #[must_use]
    pub fn to_csv_string(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Integer(n) => n.to_string(),
            Self::Float(n) => n.to_string(),
            Self::Boolean(b) => b.to_string(),
            Self::Date(d) => d.format("%Y-%m-%d").to_string(),
            Self::DateTime(dt) => dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            Self::Empty => String::new(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::approx_constant)]
mod tests {
    use super::*;

    #[test]
    fn multi_select_empty_set_passes_all() {
        let cell = CellValue::Text("Active".into());
        let filter = FilterValue::MultiSelect(HashSet::new());
        assert!(cell.matches_filter(&filter));
    }

    #[test]
    fn multi_select_passes_when_value_in_set() {
        let cell = CellValue::Text("Active".into());
        let mut set = HashSet::new();
        set.insert("Active".to_string());
        set.insert("Pending".to_string());
        assert!(cell.matches_filter(&FilterValue::MultiSelect(set)));
    }

    #[test]
    fn multi_select_blocks_when_value_not_in_set() {
        let cell = CellValue::Text("Suspended".into());
        let mut set = HashSet::new();
        set.insert("Active".to_string());
        set.insert("Pending".to_string());
        assert!(!cell.matches_filter(&FilterValue::MultiSelect(set)));
    }

    #[test]
    fn boolean_filter_matches_only_same_polarity() {
        assert!(CellValue::Boolean(true).matches_filter(&FilterValue::Boolean(true)));
        assert!(!CellValue::Boolean(true).matches_filter(&FilterValue::Boolean(false)));
        assert!(CellValue::Boolean(false).matches_filter(&FilterValue::Boolean(false)));
    }

    #[test]
    fn mismatched_cell_and_filter_passes_silently() {
        // Numeric range applied to a text cell: cell shouldn't be filtered out
        // (matches_filter's catch-all `_ => true` documents this behavior).
        let cell = CellValue::Text("Active".into());
        let filter = FilterValue::NumericRange {
            min: Some(0.0),
            max: Some(10.0),
        };
        assert!(cell.matches_filter(&filter));
    }

    // ---- cmp_for_sort -----------------------------------------------------

    #[test]
    fn cmp_for_sort_text_alphabetical() {
        assert_eq!(
            CellValue::Text("alpha".into()).cmp_for_sort(&CellValue::Text("beta".into())),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            CellValue::Text("z".into()).cmp_for_sort(&CellValue::Text("a".into())),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn cmp_for_sort_integer_numeric() {
        assert_eq!(
            CellValue::Integer(1).cmp_for_sort(&CellValue::Integer(2)),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            CellValue::Integer(-5).cmp_for_sort(&CellValue::Integer(-10)),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn cmp_for_sort_empty_sorts_last() {
        assert_eq!(
            CellValue::Empty.cmp_for_sort(&CellValue::Text("anything".into())),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            CellValue::Text("anything".into()).cmp_for_sort(&CellValue::Empty),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            CellValue::Empty.cmp_for_sort(&CellValue::Empty),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn cmp_for_sort_cross_type_does_not_panic() {
        // Cross-type comparison uses debug-string fallback; must not panic.
        let result = CellValue::Integer(42).cmp_for_sort(&CellValue::Text("hello".into()));
        // We don't assert a specific ordering, just that it doesn't panic.
        let _ = result;
    }

    // ---- to_csv_string ----------------------------------------------------

    #[test]
    fn to_csv_string_text() {
        assert_eq!(CellValue::Text("hello".into()).to_csv_string(), "hello");
    }

    #[test]
    fn to_csv_string_integer() {
        assert_eq!(CellValue::Integer(42).to_csv_string(), "42");
        assert_eq!(CellValue::Integer(-7).to_csv_string(), "-7");
    }

    #[test]
    fn to_csv_string_float() {
        assert_eq!(CellValue::Float(3.14).to_csv_string(), "3.14");
    }

    #[test]
    fn to_csv_string_boolean() {
        assert_eq!(CellValue::Boolean(true).to_csv_string(), "true");
        assert_eq!(CellValue::Boolean(false).to_csv_string(), "false");
    }

    #[test]
    fn to_csv_string_empty_is_blank() {
        assert_eq!(CellValue::Empty.to_csv_string(), "");
    }

    // ---- date range filter ------------------------------------------------

    #[test]
    fn date_range_filter_passes_when_in_range() {
        use chrono::NaiveDate;
        let d = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let cell = CellValue::Date(d);
        let filter = FilterValue::DateRange {
            min: Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            max: Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
        };
        assert!(cell.matches_filter(&filter));
    }

    #[test]
    fn date_range_filter_blocks_when_out_of_range() {
        use chrono::NaiveDate;
        let d = NaiveDate::from_ymd_opt(2023, 12, 31).unwrap();
        let cell = CellValue::Date(d);
        let filter = FilterValue::DateRange {
            min: Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            max: None,
        };
        assert!(!cell.matches_filter(&filter));
    }

    // ---- numeric range filter edge cases ----------------------------------

    #[test]
    fn numeric_range_min_bound_is_inclusive() {
        assert!(
            CellValue::Float(5.0).matches_filter(&FilterValue::NumericRange {
                min: Some(5.0),
                max: None
            })
        );
    }

    #[test]
    fn numeric_range_max_bound_is_inclusive() {
        assert!(
            CellValue::Float(10.0).matches_filter(&FilterValue::NumericRange {
                min: None,
                max: Some(10.0)
            })
        );
    }

    #[test]
    fn numeric_range_open_bounds_passes_all() {
        assert!(
            CellValue::Float(f64::MAX).matches_filter(&FilterValue::NumericRange {
                min: None,
                max: None
            })
        );
    }
}
