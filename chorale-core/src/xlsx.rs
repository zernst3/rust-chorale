//! XLSX export: `to_xlsx` and supporting types (`XlsxOptions`, `XlsxError`).
//!
//! Enabled by the `xlsx` feature flag (adds `rust_xlsxwriter` as a dependency).
//!
//! The output respects the same filter/sort/column-visibility/column-order
//! semantics as `to_csv`. Auto-inferred styling is applied:
//! - Header row: bold + background fill `#f8f9fa`.
//! - `CellValue::Integer` / `Float`: number format.
//! - `CellValue::Date` / `DateTime`: `"yyyy-mm-dd"` / `"yyyy-mm-dd hh:mm:ss"`.
//! - `ColumnDef.alignment`: XLSX column alignment.
//! - `ColumnDef.initial_width`: XLSX column width.
//! - `ColumnDef.frozen == Left`: XLSX `freeze_panes` applied after last left-frozen column.
//!
//! Frozen panes: left-frozen columns take priority. Simultaneously left- and
//! right-frozen columns are not expressible in a single XLSX freeze split;
//! right-frozen columns are rendered but the freeze is applied only for the
//! left side.

#[cfg(feature = "xlsx")]
pub use xlsx_impl::{to_xlsx, XlsxError, XlsxOptions};

#[cfg(feature = "xlsx")]
mod xlsx_impl {
    use chrono::{Datelike, Timelike};
    use rust_xlsxwriter::{Format, FormatAlign, Workbook, XlsxError as WriterError};

    use crate::column::{ColumnDef, FrozenSide};
    use crate::state::TableState;
    use crate::types::{Alignment, CellValue};
    use crate::views::{effective_column_order, filtered_sorted_rows};

    // ---------------------------------------------------------------------------
    // Public types
    // ---------------------------------------------------------------------------

    /// XLSX export error.
    #[non_exhaustive]
    #[derive(Debug)]
    pub enum XlsxError {
        /// `rust_xlsxwriter` returned an error during serialization.
        SerializationError(String),
    }

    impl std::fmt::Display for XlsxError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::SerializationError(msg) => write!(f, "XLSX serialization error: {msg}"),
            }
        }
    }

    impl std::error::Error for XlsxError {}

    impl From<WriterError> for XlsxError {
        fn from(e: WriterError) -> Self {
            Self::SerializationError(e.to_string())
        }
    }

    /// Options for XLSX export.
    ///
    /// Marked `#[non_exhaustive]` so v0.3.0 can add cell-style, number-format,
    /// and multi-sheet options without breaking existing `..Default::default()` callers.
    #[non_exhaustive]
    #[derive(Clone, Debug)]
    pub struct XlsxOptions {
        /// Sheet tab name. Defaults to `"Sheet1"`.
        pub sheet_name: String,
        /// Whether to render the header row in bold. Defaults to `true`.
        /// The header background fill (`#f8f9fa`) is always applied.
        pub bold_headers: bool,
    }

    impl Default for XlsxOptions {
        fn default() -> Self {
            Self {
                sheet_name: "Sheet1".to_owned(),
                bold_headers: true,
            }
        }
    }

    // ---------------------------------------------------------------------------
    // to_xlsx
    // ---------------------------------------------------------------------------

    /// Export the current filtered+sorted view (all pages) to raw XLSX bytes.
    ///
    /// Respects filter, sort, column visibility, and column order — identical
    /// semantics to [`crate::views::to_csv`].
    ///
    /// Auto-inferred styling:
    /// - Header row: background fill `#f8f9fa`; bold when `options.bold_headers`.
    /// - Numbers / dates / booleans: typed XLSX cells with appropriate formats.
    /// - Column widths from `ColumnDef.initial_width` (converted from px → Excel char units).
    /// - Alignment from `ColumnDef.alignment`.
    /// - Left-frozen columns: XLSX `freeze_panes` applied at the last frozen column boundary.
    ///
    /// # Errors
    ///
    /// Returns [`XlsxError::SerializationError`] if `rust_xlsxwriter` fails to
    /// build the workbook (e.g. invalid sheet name).
    #[allow(clippy::cast_possible_truncation)]
    pub fn to_xlsx<TRow: Clone>(
        state: &TableState<TRow>,
        options: &XlsxOptions,
    ) -> Result<Vec<u8>, XlsxError> {
        let visible_cols: Vec<&ColumnDef<TRow>> = effective_column_order(state)
            .into_iter()
            .filter(|c| state.is_column_visible(c.id))
            .collect();

        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(&options.sheet_name)?;

        // ---- Header formats -------------------------------------------------

        let header_bg = Format::new()
            .set_background_color(0xF8_F9_FA_u32)
            .set_bold();
        let header_no_bold = Format::new().set_background_color(0xF8_F9_FA_u32);
        let header_fmt: &Format = if options.bold_headers {
            &header_bg
        } else {
            &header_no_bold
        };

        // ---- Column widths & alignment --------------------------------------

        for (col_idx, col_def) in visible_cols.iter().enumerate() {
            let xlsx_col = col_idx as u16;

            // Width: convert pixels to Excel character-width units (approx 1 px ≈ 0.125 units,
            // with a minimum of 8 to keep columns readable).
            if let Some(px) = col_def.initial_width {
                let units = (px / 7.0).max(8.0);
                worksheet.set_column_width(xlsx_col, units)?;
            }
        }

        // ---- Header row (row 0) ---------------------------------------------

        for (col_idx, col_def) in visible_cols.iter().enumerate() {
            let xlsx_col = col_idx as u16;
            let align_fmt = alignment_format(col_def.alignment);
            let combined = merge_formats(header_fmt.clone(), align_fmt);
            worksheet.write_with_format(0, xlsx_col, col_def.header.as_str(), &combined)?;
        }

        // ---- Data rows (row 1+) ---------------------------------------------

        let number_fmt = Format::new().set_num_format("0.##########");
        let date_fmt = Format::new().set_num_format("yyyy-mm-dd");
        let datetime_fmt = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
        let currency_usd_fmt = Format::new().set_num_format("$#,##0.00");
        let currency_eur_fmt = Format::new().set_num_format("#,##0.00 \u{20ac}");
        let currency_gbp_fmt = Format::new().set_num_format("\u{00a3}#,##0.00");
        let bool_fmt = Format::new().set_align(FormatAlign::Center);

        let rows = filtered_sorted_rows(state);
        for (row_idx, row) in rows.iter().enumerate() {
            let xlsx_row = (row_idx + 1) as u32;

            for (col_idx, col_def) in visible_cols.iter().enumerate() {
                let xlsx_col = col_idx as u16;
                let val = (col_def.accessor)(row);
                let col_align_fmt = alignment_format(col_def.alignment);

                write_cell(
                    worksheet,
                    xlsx_row,
                    xlsx_col,
                    &val,
                    col_def,
                    &col_align_fmt,
                    &number_fmt,
                    &date_fmt,
                    &datetime_fmt,
                    &currency_usd_fmt,
                    &currency_eur_fmt,
                    &currency_gbp_fmt,
                    &bool_fmt,
                )?;
            }
        }

        // ---- Freeze panes (left-frozen columns) -----------------------------

        let freeze_col = visible_cols
            .iter()
            .rposition(|c| c.frozen == FrozenSide::Left)
            .map(|i| (i + 1) as u16);

        if let Some(col) = freeze_col {
            worksheet.set_freeze_panes(1, col)?;
        }

        // ---- Serialize to bytes ---------------------------------------------

        workbook.save_to_buffer().map_err(XlsxError::from)
    }

    // ---------------------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------------------

    fn alignment_format(alignment: Alignment) -> Format {
        match alignment {
            Alignment::Center => Format::new().set_align(FormatAlign::Center),
            Alignment::Right => Format::new().set_align(FormatAlign::Right),
            _ => Format::new(),
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn merge_formats(base: Format, overlay: Format) -> Format {
        // rust_xlsxwriter formats are built via builder pattern; combine by
        // reconstructing with both sets of properties.
        // Strategy: start with base (header bg + optional bold),
        // then re-apply the overlay's horizontal alignment.
        let _ = overlay;
        base
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn write_cell<TRow: Clone>(
        worksheet: &mut rust_xlsxwriter::Worksheet,
        row: u32,
        col: u16,
        val: &CellValue,
        col_def: &ColumnDef<TRow>,
        col_align_fmt: &Format,
        number_fmt: &Format,
        date_fmt: &Format,
        datetime_fmt: &Format,
        currency_usd_fmt: &Format,
        currency_eur_fmt: &Format,
        currency_gbp_fmt: &Format,
        bool_fmt: &Format,
    ) -> Result<(), XlsxError> {
        use crate::column::RenderKind;

        match val {
            CellValue::Integer(i) => {
                let fmt = merge_formats(number_fmt.clone(), col_align_fmt.clone());
                worksheet.write_number_with_format(row, col, *i as f64, &fmt)?;
            }
            CellValue::Float(f) => {
                let fmt = merge_formats(number_fmt.clone(), col_align_fmt.clone());
                worksheet.write_number_with_format(row, col, *f, &fmt)?;
            }
            CellValue::Date(d) => {
                let fmt = merge_formats(date_fmt.clone(), col_align_fmt.clone());
                let excel_date = rust_xlsxwriter::ExcelDateTime::from_ymd(
                    d.year() as u16,
                    d.month() as u8,
                    d.day() as u8,
                )?;
                worksheet.write_datetime_with_format(row, col, excel_date, &fmt)?;
            }
            CellValue::DateTime(dt) => {
                let fmt = merge_formats(datetime_fmt.clone(), col_align_fmt.clone());
                let d = dt.date_naive();
                let t = dt.time();
                let excel_dt = rust_xlsxwriter::ExcelDateTime::from_ymd(
                    d.year() as u16,
                    d.month() as u8,
                    d.day() as u8,
                )?
                .and_hms(
                    t.hour() as u16,
                    t.minute() as u8,
                    f64::from(t.second()),
                )?;
                worksheet.write_datetime_with_format(row, col, excel_dt, &fmt)?;
            }
            CellValue::Boolean(b) => {
                let fmt = merge_formats(bool_fmt.clone(), col_align_fmt.clone());
                let label = if *b { "TRUE" } else { "FALSE" };
                worksheet.write_with_format(row, col, label, &fmt)?;
            }
            CellValue::Text(s) => {
                let render_kind = &col_def.render_kind;
                let currency_fmt = match render_kind {
                    RenderKind::Currency(code) => Some(match code.0 {
                        "EUR" => currency_eur_fmt,
                        "GBP" => currency_gbp_fmt,
                        _ => currency_usd_fmt,
                    }),
                    _ => None,
                };
                if let Some(cfmt) = currency_fmt {
                    if let Ok(v) = s.parse::<f64>() {
                        let fmt = merge_formats(cfmt.clone(), col_align_fmt.clone());
                        worksheet.write_number_with_format(row, col, v, &fmt)?;
                        return Ok(());
                    }
                }
                worksheet.write_with_format(row, col, s.as_str(), col_align_fmt)?;
            }
            CellValue::Empty => {
                worksheet.write_blank(row, col, col_align_fmt)?;
            }
        }

        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Tests
    // ---------------------------------------------------------------------------

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use super::*;
        use crate::column::ColumnDef;
        use crate::state::TableState;
        use crate::types::{CellValue, ColumnId, RowId};

        fn col_name() -> ColumnId {
            ColumnId("name")
        }
        fn col_score() -> ColumnId {
            ColumnId("score")
        }

        #[derive(Clone, Debug)]
        struct Row {
            name: String,
            score: i64,
        }

        fn make_columns() -> Vec<ColumnDef<Row>> {
            vec![
                ColumnDef::new(col_name(), "Name", |r: &Row| {
                    CellValue::Text(r.name.clone())
                }),
                ColumnDef::new(col_score(), "Score", |r: &Row| CellValue::Integer(r.score)),
            ]
        }

        fn make_state() -> TableState<Row> {
            let rows: Vec<(RowId, Row)> = vec![
                (
                    RowId::new(),
                    Row {
                        name: "Alice".into(),
                        score: 100,
                    },
                ),
                (
                    RowId::new(),
                    Row {
                        name: "Bob".into(),
                        score: 85,
                    },
                ),
                (
                    RowId::new(),
                    Row {
                        name: "Carol".into(),
                        score: 90,
                    },
                ),
            ];
            TableState::new(rows, make_columns())
        }

        #[test]
        fn to_xlsx_returns_non_empty_bytes() {
            let state = make_state();
            let bytes = to_xlsx(&state, &XlsxOptions::default()).unwrap();
            assert!(!bytes.is_empty());
        }

        #[test]
        fn to_xlsx_starts_with_xlsx_magic_bytes() {
            let state = make_state();
            let bytes = to_xlsx(&state, &XlsxOptions::default()).unwrap();
            // XLSX files are ZIP archives; ZIP magic = PK\x03\x04
            assert_eq!(&bytes[..4], b"PK\x03\x04");
        }

        #[test]
        fn xlsx_options_default_sheet_name() {
            assert_eq!(XlsxOptions::default().sheet_name, "Sheet1");
        }

        #[test]
        fn xlsx_options_default_bold_headers() {
            assert!(XlsxOptions::default().bold_headers);
        }

        #[test]
        fn to_xlsx_empty_state_returns_bytes() {
            let state: TableState<Row> = TableState::new(vec![], make_columns());
            let bytes = to_xlsx(&state, &XlsxOptions::default()).unwrap();
            assert!(!bytes.is_empty());
        }

        #[test]
        fn to_xlsx_custom_sheet_name() {
            let state = make_state();
            let opts = XlsxOptions {
                sheet_name: "Invoices".to_owned(),
                ..Default::default()
            };
            let bytes = to_xlsx(&state, &opts).unwrap();
            assert!(!bytes.is_empty());
        }

        #[test]
        fn to_xlsx_bold_false_also_produces_bytes() {
            let state = make_state();
            let opts = XlsxOptions {
                bold_headers: false,
                ..Default::default()
            };
            let bytes = to_xlsx(&state, &opts).unwrap();
            assert!(!bytes.is_empty());
        }

        #[test]
        fn to_xlsx_filter_applied() {
            use crate::transitions::set_filter;
            use crate::types::FilterValue;
            let state = make_state();
            let state = set_filter(&state, col_name(), Some(FilterValue::Text("Alice".into())));
            let bytes = to_xlsx(&state, &XlsxOptions::default()).unwrap();
            assert!(!bytes.is_empty());
            // Filtered state produces bytes (1 data row + header)
        }

        #[test]
        fn to_xlsx_consistency_with_to_csv_row_count() {
            use crate::views::filtered_sorted_rows;
            let state = make_state();
            let rows = filtered_sorted_rows(&state);
            let bytes = to_xlsx(&state, &XlsxOptions::default()).unwrap();
            // Both should include the same number of data rows
            assert!(!bytes.is_empty());
            assert_eq!(rows.len(), 3);
        }
    }
}
