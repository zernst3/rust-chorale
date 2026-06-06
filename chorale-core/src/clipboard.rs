//! Clipboard copy / paste primitives: `to_clipboard_tsv` and `paste_tsv_into_range`.
//!
//! TSV format (minimal Excel-compatible):
//! - Cells separated by `\t`, rows separated by `\n` (Unix newline).
//! - Cells whose value contains a tab character are wrapped in double-quotes.
//! - Cells whose value contains a newline have the newline replaced with a space.
//!
//! Paste shape rules (matching Excel):
//! - Payload **smaller** than target range: range stays as-is; the host tiles
//!   the payload when applying writes (the returned state reflects the original range).
//! - Payload **larger** than target range: the range is **expanded** so its focus
//!   is `(anchor_row + payload_rows - 1, col_at_anchor + payload_cols - 1)`, clamped
//!   to table bounds.

use crate::range::RangeSelection;
use crate::state::TableState;
use crate::types::ColumnId;
use crate::views::{effective_column_order, visible_view};

// ---------------------------------------------------------------------------
// Event payload types (used by adapter on_copy / on_paste callbacks)
// ---------------------------------------------------------------------------

/// Payload delivered to an adapter's `on_copy` callback after a successful Ctrl+C.
#[derive(Clone, Debug)]
pub struct ClipboardCopyEvent {
    /// The TSV string that was written to the system clipboard.
    pub tsv: String,
    /// The range that was serialized.
    pub range: RangeSelection,
}

/// Payload delivered to an adapter's `on_paste` callback after a successful Ctrl+V.
#[derive(Clone, Debug)]
pub struct ClipboardPasteEvent {
    /// The TSV string that was read from the system clipboard.
    pub tsv: String,
    /// The effective target range (may be expanded from the original selection).
    pub range: RangeSelection,
}

// ---------------------------------------------------------------------------
// Public error type
// ---------------------------------------------------------------------------

/// Errors from clipboard operations.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardError {
    /// Copy was attempted on a multi-rect (disjoint) selection.
    MultiRectCopyNotSupported,
    /// Paste or copy was attempted with no active range.
    NoRangeSelected,
    /// Paste was attempted on a multi-rect selection (not supported).
    MultiRectPasteNotSupported,
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MultiRectCopyNotSupported => {
                write!(
                    f,
                    "copy is not supported for disjoint (multi-rect) selections"
                )
            }
            Self::NoRangeSelected => write!(f, "no range is selected"),
            Self::MultiRectPasteNotSupported => {
                write!(
                    f,
                    "paste is not supported for disjoint (multi-rect) selections"
                )
            }
        }
    }
}

impl std::error::Error for ClipboardError {}

// ---------------------------------------------------------------------------
// to_clipboard_tsv
// ---------------------------------------------------------------------------

/// Serialize the active range selection to a TSV string.
///
/// - Empty `range_selection` → `Ok("")`.
/// - `range_selection.len() > 1` → `Err(ClipboardError::MultiRectCopyNotSupported)`.
/// - Single rect: cells serialized in row-major order, columns in effective
///   column order (respects `column_order` and `column_visibility`).
///
/// Escaping (minimal Excel-compatible):
/// - A cell value containing a tab is wrapped in double-quotes.
/// - A cell value containing a newline has the newline replaced with a space.
///
/// # Errors
///
/// Returns [`ClipboardError::MultiRectCopyNotSupported`] when
/// `range_selection.len() > 1`.
#[must_use = "returns the TSV string; dropping it discards the clipboard data"]
pub fn to_clipboard_tsv<TRow: Clone>(state: &TableState<TRow>) -> Result<String, ClipboardError> {
    if state.range_selection.is_empty() {
        return Ok(String::new());
    }
    if state.range_selection.len() > 1 {
        return Err(ClipboardError::MultiRectCopyNotSupported);
    }

    let visible_cols: Vec<&crate::column::ColumnDef<TRow>> = effective_column_order(state)
        .into_iter()
        .filter(|c| state.is_column_visible(c.id))
        .collect();

    let normalized = state.range_selection[0].normalized(&visible_cols);
    let rows = visible_view(state);

    let mut out = String::new();
    for row_idx in normalized.min_row..=normalized.max_row {
        if row_idx >= rows.len() {
            break;
        }
        let row = match &rows[row_idx] {
            crate::views::RenderRow::Data { row, .. } => row,
            crate::views::RenderRow::DetailPanel { .. } => continue,
        };

        let mut first_col = true;
        for &col_id in &normalized.columns {
            if let Some(col_def) = visible_cols.iter().find(|c| c.id == col_id) {
                if !first_col {
                    out.push('\t');
                }
                first_col = false;
                let val = (col_def.accessor)(row);
                out.push_str(&tsv_escape(&val.to_csv_string()));
            }
        }

        out.push('\n');
    }

    // Strip trailing newline to match exact row count.
    if out.ends_with('\n') {
        out.pop();
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// paste_tsv_into_range
// ---------------------------------------------------------------------------

/// Parse a TSV string and adjust the active range to match the paste payload size.
///
/// Returns a new `TableState` whose `range_selection` reflects the effective
/// paste target (expanded when the payload is larger than the original selection).
/// **Row data is not mutated** — the returned state signals to the adapter what
/// range was affected; the host applies per-cell writes via the `on_paste` callback.
///
/// # Shape rules
///
/// - Payload **smaller** than the current range: the range stays unchanged; the
///   host is expected to tile the payload when iterating the range cells.
/// - Payload **larger** than the current range: the range is expanded so its
///   `focus` covers `(anchor_row + payload_rows - 1, col_at(anchor_col_idx + payload_cols - 1))`,
///   clamped to visible row / column bounds.
///
/// # Errors
///
/// - `NoRangeSelected` — `range_selection` is empty.
/// - `MultiRectPasteNotSupported` — `range_selection.len() > 1`.
///
/// Empty or all-whitespace `tsv` is a **no-op** (returns the state unchanged).
pub fn paste_tsv_into_range<TRow: Clone>(
    state: &TableState<TRow>,
    tsv: &str,
) -> Result<TableState<TRow>, ClipboardError> {
    // Empty paste → no-op.
    if tsv.trim().is_empty() {
        return Ok(state.clone());
    }

    if state.range_selection.is_empty() {
        return Err(ClipboardError::NoRangeSelected);
    }
    if state.range_selection.len() > 1 {
        return Err(ClipboardError::MultiRectPasteNotSupported);
    }

    let visible_cols: Vec<&crate::column::ColumnDef<TRow>> = effective_column_order(state)
        .into_iter()
        .filter(|c| state.is_column_visible(c.id))
        .collect();
    let visible_col_ids: Vec<ColumnId> = visible_cols.iter().map(|c| c.id).collect();
    let total_visible_rows = visible_view(state).len();

    let payload = parse_tsv(tsv);
    let payload_rows = payload.len();
    let payload_cols = payload.iter().map(Vec::len).max().unwrap_or(0);

    if payload_rows == 0 || payload_cols == 0 {
        return Ok(state.clone());
    }

    let current_range = &state.range_selection[0];
    let normalized = current_range.normalized(&visible_cols);

    let range_rows = normalized.max_row.saturating_sub(normalized.min_row) + 1;
    let range_cols = normalized.columns.len();

    // Determine the anchor position (top-left of the target).
    let anchor_row = normalized.min_row;
    let anchor_col_id = normalized
        .columns
        .first()
        .copied()
        .unwrap_or(current_range.anchor.1);
    let anchor_col_idx = visible_col_ids
        .iter()
        .position(|&id| id == anchor_col_id)
        .unwrap_or(0);

    let (focus_row, focus_col_id) = if payload_rows > range_rows || payload_cols > range_cols {
        // Expand: compute new focus from payload size, clamped to table bounds.
        let new_focus_row =
            (anchor_row + payload_rows - 1).min(total_visible_rows.saturating_sub(1));
        let new_focus_col_idx =
            (anchor_col_idx + payload_cols - 1).min(visible_col_ids.len().saturating_sub(1));
        let new_focus_col = visible_col_ids
            .get(new_focus_col_idx)
            .copied()
            .unwrap_or(anchor_col_id);
        (new_focus_row, new_focus_col)
    } else {
        // Keep original range (tile behavior is handled by the host).
        (
            normalized.max_row,
            *normalized.columns.last().unwrap_or(&anchor_col_id),
        )
    };

    let new_range = RangeSelection {
        anchor: (anchor_row, anchor_col_id),
        focus: (focus_row, focus_col_id),
    };

    Ok(TableState {
        range_selection: vec![new_range],
        ..state.clone()
    })
}

// ---------------------------------------------------------------------------
// TSV helpers
// ---------------------------------------------------------------------------

/// Minimal Excel-compatible TSV cell escaping.
///
/// - Values containing a tab are wrapped in double-quotes (with embedded
///   double-quotes doubled: `"` → `""`).
/// - Values containing a newline have the newline replaced with a space.
fn tsv_escape(s: &str) -> String {
    if s.contains('\t') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.replace('\n', " ")
    }
}

/// Parse a TSV string into a 2-D `Vec<Vec<String>>` (rows × cols).
///
/// - Splits on `\n` first; trailing empty lines are dropped.
/// - Splits each row on `\t`.
/// - Recognises the minimal Excel-compatible quoting: a cell that starts with
///   `"` is parsed as a quoted field, with `""` unescaped to `"` and the
///   surrounding quotes stripped. Tabs inside a quoted field are preserved.
pub(crate) fn parse_tsv(tsv: &str) -> Vec<Vec<String>> {
    let lines: Vec<&str> = tsv.split('\n').collect();
    // Drop trailing empty lines.
    let trimmed_end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);

    lines[..trimmed_end]
        .iter()
        .map(|line| parse_tsv_row(line))
        .collect()
}

fn parse_tsv_row(row: &str) -> Vec<String> {
    let mut cells: Vec<String> = Vec::new();
    let mut chars = row.chars().peekable();

    loop {
        if chars.peek().is_none() {
            // Trailing tab produced an empty trailing cell; we've consumed it above.
            break;
        }

        let cell = if chars.peek() == Some(&'"') {
            // Quoted cell.
            chars.next(); // consume opening quote
            let mut val = String::new();
            loop {
                match chars.next() {
                    None => break,
                    Some('"') => {
                        if chars.peek() == Some(&'"') {
                            // Escaped double-quote.
                            chars.next();
                            val.push('"');
                        } else {
                            // Closing quote.
                            break;
                        }
                    }
                    Some(c) => val.push(c),
                }
            }
            // Consume the tab (or end of row) after the quoted cell.
            if chars.peek() == Some(&'\t') {
                chars.next();
            }
            val
        } else {
            // Unquoted cell: consume until tab or end.
            let mut val = String::new();
            loop {
                match chars.peek().copied() {
                    None | Some('\t') => break,
                    Some(c) => {
                        chars.next();
                        val.push(c);
                    }
                }
            }
            // Consume the tab separator.
            if chars.peek() == Some(&'\t') {
                chars.next();
                // If we just consumed the last tab, add empty trailing cell.
                if chars.peek().is_none() {
                    cells.push(val);
                    cells.push(String::new());
                    break;
                }
            }
            val
        };

        cells.push(cell);
    }

    cells
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::column::ColumnDef;
    use crate::range::RangeSelection;
    use crate::state::TableState;
    use crate::transitions::start_range_selection;
    use crate::types::{CellValue, ColumnId, RowId};

    fn col_name() -> ColumnId {
        ColumnId("name")
    }
    fn col_score() -> ColumnId {
        ColumnId("score")
    }
    fn col_notes() -> ColumnId {
        ColumnId("notes")
    }

    #[derive(Clone, Debug)]
    struct Row {
        name: String,
        score: i64,
        notes: String,
    }

    fn make_columns() -> Vec<ColumnDef<Row>> {
        vec![
            ColumnDef::new(col_name(), "Name", |r: &Row| {
                CellValue::Text(r.name.clone())
            }),
            ColumnDef::new(col_score(), "Score", |r: &Row| CellValue::Integer(r.score)),
            ColumnDef::new(col_notes(), "Notes", |r: &Row| {
                CellValue::Text(r.notes.clone())
            }),
        ]
    }

    fn make_state() -> TableState<Row> {
        let rows: Vec<(RowId, Row)> = vec![
            (
                RowId::new(),
                Row {
                    name: "Alice".into(),
                    score: 100,
                    notes: "top".into(),
                },
            ),
            (
                RowId::new(),
                Row {
                    name: "Bob".into(),
                    score: 85,
                    notes: "good".into(),
                },
            ),
            (
                RowId::new(),
                Row {
                    name: "Carol".into(),
                    score: 90,
                    notes: "great".into(),
                },
            ),
        ]
        .into_iter()
        .collect();
        TableState::new(rows, make_columns())
    }

    // ---- to_clipboard_tsv ---------------------------------------------------

    #[test]
    fn copy_empty_range_returns_empty_string() {
        let state = make_state();
        assert_eq!(to_clipboard_tsv(&state).unwrap(), "");
    }

    #[test]
    fn copy_multi_rect_returns_error() {
        let mut state = make_state();
        state.range_selection = vec![
            RangeSelection::single(0, col_name()),
            RangeSelection::single(1, col_score()),
        ];
        assert_eq!(
            to_clipboard_tsv(&state).unwrap_err(),
            ClipboardError::MultiRectCopyNotSupported
        );
    }

    #[test]
    fn copy_single_cell() {
        let state = start_range_selection(&make_state(), 0, col_name());
        assert_eq!(to_clipboard_tsv(&state).unwrap(), "Alice");
    }

    #[test]
    fn copy_row_produces_tab_separated_values() {
        let mut state = make_state();
        state.range_selection = vec![RangeSelection::new((0, col_name()), (0, col_score()))];
        assert_eq!(to_clipboard_tsv(&state).unwrap(), "Alice\t100");
    }

    #[test]
    fn copy_multi_row_produces_newline_separated_rows() {
        let mut state = make_state();
        state.range_selection = vec![RangeSelection::new((0, col_name()), (1, col_name()))];
        assert_eq!(to_clipboard_tsv(&state).unwrap(), "Alice\nBob");
    }

    #[test]
    fn copy_rect_produces_tab_and_newline() {
        let mut state = make_state();
        state.range_selection = vec![RangeSelection::new((0, col_name()), (1, col_score()))];
        assert_eq!(to_clipboard_tsv(&state).unwrap(), "Alice\t100\nBob\t85");
    }

    #[test]
    fn copy_cell_with_tab_wrapped_in_quotes() {
        let rows = vec![(
            RowId::new(),
            Row {
                name: "A\tB".into(),
                score: 1,
                notes: String::new(),
            },
        )];
        let state = TableState::new(rows, make_columns());
        let state = start_range_selection(&state, 0, col_name());
        assert_eq!(to_clipboard_tsv(&state).unwrap(), "\"A\tB\"");
    }

    #[test]
    fn copy_cell_with_newline_replaced_with_space() {
        let rows = vec![(
            RowId::new(),
            Row {
                name: "A\nB".into(),
                score: 1,
                notes: String::new(),
            },
        )];
        let state = TableState::new(rows, make_columns());
        let state = start_range_selection(&state, 0, col_name());
        assert_eq!(to_clipboard_tsv(&state).unwrap(), "A B");
    }

    #[test]
    fn copy_output_has_exactly_rows_minus_one_newlines() {
        let mut state = make_state();
        state.range_selection = vec![RangeSelection::new((0, col_name()), (2, col_name()))];
        let tsv = to_clipboard_tsv(&state).unwrap();
        assert_eq!(tsv.matches('\n').count(), 2);
    }

    // ---- parse_tsv ----------------------------------------------------------

    #[test]
    fn parse_single_cell() {
        assert_eq!(parse_tsv("hello"), vec![vec!["hello".to_string()]]);
    }

    #[test]
    fn parse_one_row_two_cols() {
        assert_eq!(
            parse_tsv("a\tb"),
            vec![vec!["a".to_string(), "b".to_string()]]
        );
    }

    #[test]
    fn parse_two_rows() {
        assert_eq!(
            parse_tsv("a\tb\nc\td"),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string(), "d".to_string()],
            ]
        );
    }

    #[test]
    fn parse_trailing_newline_ignored() {
        assert_eq!(
            parse_tsv("a\tb\n"),
            vec![vec!["a".to_string(), "b".to_string()]]
        );
    }

    #[test]
    fn parse_quoted_cell_with_tab() {
        assert_eq!(
            parse_tsv("\"a\tb\"\tc"),
            vec![vec!["a\tb".to_string(), "c".to_string()]]
        );
    }

    #[test]
    fn parse_quoted_cell_with_escaped_quote() {
        assert_eq!(parse_tsv("\"a\"\"b\""), vec![vec!["a\"b".to_string()]]);
    }

    // ---- paste_tsv_into_range -----------------------------------------------

    #[test]
    fn paste_empty_tsv_is_noop() {
        let state = start_range_selection(&make_state(), 0, col_name());
        let new_state = paste_tsv_into_range(&state, "").unwrap();
        assert_eq!(new_state.range_selection, state.range_selection);
    }

    #[test]
    fn paste_no_range_returns_error() {
        let state = make_state();
        assert_eq!(
            paste_tsv_into_range(&state, "hello").unwrap_err(),
            ClipboardError::NoRangeSelected
        );
    }

    #[test]
    fn paste_multi_rect_returns_error() {
        let mut state = make_state();
        state.range_selection = vec![
            RangeSelection::single(0, col_name()),
            RangeSelection::single(1, col_score()),
        ];
        assert_eq!(
            paste_tsv_into_range(&state, "a\tb").unwrap_err(),
            ClipboardError::MultiRectPasteNotSupported
        );
    }

    #[test]
    fn paste_exact_size_range_unchanged() {
        // 2-row × 2-col payload into a 2×2 range → range stays as-is.
        let mut state = make_state();
        state.range_selection = vec![RangeSelection::new((0, col_name()), (1, col_score()))];
        let new_state = paste_tsv_into_range(&state, "X\t1\nY\t2").unwrap();
        // anchor and focus unchanged
        assert_eq!(new_state.range_selection[0].anchor, (0, col_name()));
        assert_eq!(new_state.range_selection[0].focus, (1, col_score()));
    }

    #[test]
    fn paste_smaller_payload_range_unchanged() {
        // 1×1 payload into a 2×2 range → range stays as-is (tiled by host).
        let mut state = make_state();
        state.range_selection = vec![RangeSelection::new((0, col_name()), (1, col_score()))];
        let new_state = paste_tsv_into_range(&state, "X").unwrap();
        assert_eq!(new_state.range_selection[0].anchor, (0, col_name()));
        assert_eq!(new_state.range_selection[0].focus, (1, col_score()));
    }

    #[test]
    fn paste_larger_payload_expands_range() {
        // 3-row × 1-col payload into a 1×1 range → expanded to 3 rows.
        let state = start_range_selection(&make_state(), 0, col_name());
        let new_state = paste_tsv_into_range(&state, "X\nY\nZ").unwrap();
        let range = &new_state.range_selection[0];
        assert_eq!(range.anchor, (0, col_name()));
        // Focus should be at row 2 (0-indexed), same col.
        assert_eq!(range.focus.0, 2);
        assert_eq!(range.focus.1, col_name());
    }

    #[test]
    fn paste_larger_payload_clamped_at_table_bounds() {
        // 10-row payload into a 3-row table → focus clamped at row 2.
        let state = start_range_selection(&make_state(), 0, col_name());
        let tsv = (0..10)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let new_state = paste_tsv_into_range(&state, &tsv).unwrap();
        let range = &new_state.range_selection[0];
        assert_eq!(range.focus.0, 2);
    }

    #[test]
    fn paste_wider_payload_expands_columns() {
        // 1-row × 3-col payload starting at col_name → focus extends to col_notes.
        let state = start_range_selection(&make_state(), 0, col_name());
        let new_state = paste_tsv_into_range(&state, "X\tY\tZ").unwrap();
        let range = &new_state.range_selection[0];
        assert_eq!(range.focus.1, col_notes());
    }

    #[test]
    fn paste_does_not_mutate_rows() {
        let state = start_range_selection(&make_state(), 0, col_name());
        let new_state = paste_tsv_into_range(&state, "NewName").unwrap();
        // Row data must be unchanged — paste in core is range-only.
        let rows = visible_view(&new_state);
        match &rows[0] {
            crate::views::RenderRow::Data { row, .. } => {
                assert_eq!(row.name.as_str(), "Alice");
            }
            _ => panic!("Expected RenderRow::Data"),
        }
    }

    #[test]
    fn paste_non_range_fields_unchanged() {
        let state = start_range_selection(&make_state(), 0, col_name());
        let new_state = paste_tsv_into_range(&state, "NewName").unwrap();
        assert_eq!(new_state.active_cell, state.active_cell);
    }
}
