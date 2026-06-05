# Item 18: XLSX Export (Values-Only)

## Problem

chorale v0.1.0 ships CSV export (`to_csv`). CSV is the least-common-denominator export
format: no column widths, no bold headers, no number formatting, no multiple sheets. Every
business user who opens a CSV in Excel spends the first 30 seconds reformatting it. XLSX
is the universal expectation for data-grid export in enterprise software.

AG Grid ships `exportDataAsExcel()` (Enterprise tier). MUI X DataGrid ships XLSX export
behind a premium tier. TanStack Table has no built-in export; hosts must integrate a third-party
library. `leptos-struct-table` has no export. chorale will ship XLSX export in the open-source
tier for both Dioxus and Leptos adapters in v0.2.0, matching or exceeding the free-tier story
of every JS competitor.

v0.2.0 scope is deliberately values-only: header row (bold) + data rows with string values.
No cell styles, no number formats, no frozen panes, no multi-sheet — those depend on cell-style
infrastructure not yet in the library. They are explicit v0.3.0 follow-ups.

## Proposed Public API

### `chorale-core`

```rust
/// XLSX export error.
#[non_exhaustive]
pub enum XlsxError {
    /// rust_xlsxwriter returned an error during serialization.
    SerializationError(String),
}

/// Export the current visible view to an XLSX file.
/// Respects: current filter, sort, column visibility, column order — identical
/// semantics to `to_csv`.
/// Returns the raw XLSX file bytes, suitable for writing to a file or delivering
/// as a download response.
/// The sheet name defaults to "Sheet1" unless overridden via `XlsxOptions`.
pub fn to_xlsx(
    state: &TableState<TRow>,
    options: &XlsxOptions,
) -> Result<Vec<u8>, XlsxError>;

/// Options for XLSX export. Marked `#[non_exhaustive]` so v0.3.0 can add
/// cell-style, number-format, and multi-sheet options without breaking callers.
#[non_exhaustive]
pub struct XlsxOptions {
    /// Sheet tab name. Defaults to "Sheet1".
    pub sheet_name: String,
    /// Whether to render headers in bold. Defaults to `true`.
    pub bold_headers: bool,
}

impl Default for XlsxOptions {
    fn default() -> Self {
        XlsxOptions {
            sheet_name: "Sheet1".to_string(),
            bold_headers: true,
        }
    }
}
```

### Adapter download button (chorale-dioxus / chorale-leptos)

```rust
/// Optional convenience component. Hosts may use this or wire their own button.
/// When clicked: calls `to_xlsx`, creates a Blob, triggers a browser download.
#[component]
pub fn ExportXlsxButton(
    handle: UseTableHandle<TRow>,

    /// Label for the button. Defaults to "Export XLSX".
    #[props(default = "Export XLSX".into())]
    label: String,

    /// File name for the downloaded file (without extension).
    /// Defaults to "export".
    #[props(default = "export".into())]
    filename: String,

    /// Sheet name passed to `XlsxOptions`. Defaults to "Sheet1".
    #[props(default = "Sheet1".into())]
    sheet_name: String,

    /// Whether headers are bold. Defaults to true.
    #[props(default = true)]
    bold_headers: bool,

    /// CSS class for the button element.
    #[props(default = "".into())]
    class: String,
) -> Element;
```

### Callsite shape

```rust
// Option A: use the convenience component (Dioxus)
rsx! {
    Table { handle: handle }
    ExportXlsxButton {
        handle: handle,
        filename: "invoices-{today}",
        sheet_name: "Invoices",
    }
}

// Option B: call to_xlsx directly (host controls the download trigger)
let on_export = move |_| {
    let state = handle.signal().read().clone();
    let bytes = to_xlsx(&state, &XlsxOptions {
        sheet_name: "My Sheet".to_string(),
        bold_headers: true,
    });
    match bytes {
        Ok(b) => trigger_browser_download(&b, "export.xlsx"),
        Err(e) => log::error!("XLSX export failed: {:?}", e),
    }
};
```

### Download trigger (adapter internal)

```javascript
// WASM-side: called after `to_xlsx` returns bytes.
// Uses Blob + URL.createObjectURL + programmatic <a> click.
function triggerXlsxDownload(bytes: Uint8Array, filename: string) {
    const blob = new Blob([bytes], {
        type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename + ".xlsx";
    a.click();
    URL.revokeObjectURL(url);
}
```

In Rust/WASM this is wired via `web_sys::Blob`, `web_sys::Url::create_object_url`,
and `web_sys::HtmlAnchorElement`. The pattern is identical to how `to_csv` download
would be wired in the adapter.

## Internal Design

**Crate dependency:** `rust_xlsxwriter` (crates.io, BSD-2-Clause license). The author
(John McNamara) has maintained Excel::Writer::XLSX (Perl) for over 25 years; the Rust crate
is a faithful port with active maintenance and ~50 transitive dependencies, none controversial.
It is added as a dependency of `chorale-core` (not the adapter) because `to_xlsx` is a pure
function that produces bytes — it needs the serializer but no WASM/DOM machinery.

The dep is gated behind a `xlsx` feature flag in `chorale-core`'s `Cargo.toml`:

```toml
[features]
default = []
xlsx = ["rust_xlsxwriter"]
```

`to_xlsx` is conditionally compiled under `#[cfg(feature = "xlsx")]`. This keeps the
`rust_xlsxwriter` dep tree out of consumers who only need CSV export. The adapter crates
(`chorale-dioxus`, `chorale-leptos`) enable `chorale-core/xlsx` by default in their own
feature sets, so `ExportXlsxButton` is available without host configuration.

**Implementation sketch:**
```rust
pub fn to_xlsx<TRow: Clone>(
    state: &TableState<TRow>,
    options: &XlsxOptions,
) -> Result<Vec<u8>, XlsxError> {
    use rust_xlsxwriter::*;

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name(&options.sheet_name)?;

    let visible_columns = state.effective_column_order()
        .iter()
        .filter(|id| state.column_visibility.get(id).copied().unwrap_or(true))
        .cloned()
        .collect::<Vec<_>>();

    // Write header row
    let bold = Format::new().set_bold();
    for (col_idx, col_id) in visible_columns.iter().enumerate() {
        let header = state.columns.iter()
            .find(|c| c.id == *col_id)
            .map(|c| c.header.clone())
            .unwrap_or_default();
        if options.bold_headers {
            worksheet.write_with_format(0, col_idx as u16, &header, &bold)?;
        } else {
            worksheet.write_string(0, col_idx as u16, &header)?;
        }
    }

    // Write data rows (visible_view semantics: post-filter, post-sort)
    let view = state.visible_view();
    for (row_idx, row) in view.iter().enumerate() {
        for (col_idx, col_id) in visible_columns.iter().enumerate() {
            let col_def = state.columns.iter().find(|c| c.id == *col_id);
            let value = col_def.map(|c| (c.accessor)(&row.data)).unwrap_or(CellValue::Empty);
            let cell_str = value.to_display_string(); // same as CSV path
            worksheet.write_string((row_idx + 1) as u32, col_idx as u16, &cell_str)?;
        }
    }

    workbook.save_to_buffer().map_err(|e| XlsxError::SerializationError(e.to_string()))
}
```

**CHORALE-CORE-1 compliance:** `rust_xlsxwriter` is a pure Rust library with no framework
dependency. Adding it to `chorale-core` under the `xlsx` feature is consistent with the rule's
explicit allowlist ("MAY depend on `serde`, `thiserror`, `rust_decimal`, etc."). Serialization
libraries fall in the same category.

**DEPS-1 compliance:** `rust_xlsxwriter` is gated behind a feature flag, so it is opt-in
and adds zero compile cost to consumers who do not need XLSX. The feature name `xlsx` is
descriptive and stable. Document the dep addition in the commit body per DEPS-1.

**Semantics matching `to_csv`:** both functions call `visible_view()` internally, so filter,
sort, column visibility, and column order are all respected identically. Empty state (filter
excludes all rows) produces headers + empty data body, consistent with `to_csv`.

## Backwards Compatibility

`to_xlsx` is a new free function in `chorale-core`, conditionally compiled under the `xlsx`
feature flag. Callers who do not opt into the feature are unaffected. `XlsxError` and
`XlsxOptions` are new `#[non_exhaustive]` types. No existing types or functions are modified.

`ExportXlsxButton` is a new optional component in each adapter. Existing `Table` callsites
are unaffected.

The `rust_xlsxwriter` crate dependency is added to `chorale-core` behind the `xlsx` feature.
Consumers who compile `chorale-core` without enabling `xlsx` see no dep tree change. Consumers
who use `chorale-dioxus` or `chorale-leptos` (which enable `xlsx` by default) will see the
new transitive deps; this is an intentional tradeoff for out-of-the-box XLSX support.

## Test Plan

Per TESTS-1:

**`to_xlsx` core function (~10 tests):**
- Happy path: `to_xlsx` on a 3-row, 2-column state returns `Ok(bytes)` where `bytes`
  is non-empty and starts with the XLSX magic bytes (`PK\x03\x04`).
- Headers present: parse the returned bytes (using `rust_xlsxwriter` or `calamine` in the
  test) and assert row 0 contains the correct column headers.
- Data rows present: row 1..=N contain the correct cell values from `visible_view`.
- Filter applied: a state with 2 rows matching a filter and 3 total rows produces 2 data rows.
- Sort applied: rows appear in sorted order.
- Column visibility: hidden columns are excluded from both headers and data.
- Column order: columns appear in `column_order` order.
- Empty state (filter excludes all): headers row present, no data rows.
- `bold_headers: false`: no format applied to header row (bytes differ from bold case).
- `sheet_name` option: parsed XLSX has the expected sheet tab name.

**`XlsxOptions::default()` (~2 tests):**
- `sheet_name` is `"Sheet1"`.
- `bold_headers` is `true`.

**`to_xlsx` consistency with `to_csv` (~3 tests):**
- Same state produces same row count and column count in both formats.
- Same cell values appear in both outputs (accounting for format differences).
- Column order is identical between `to_xlsx` and `to_csv`.

**`ExportXlsxButton` adapter component (~3 integration tests):**
- Component renders a `<button>` element.
- Click triggers `to_xlsx` call and no panic (mocked download trigger).
- `filename` and `sheet_name` props flow through correctly.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **`rust_xlsxwriter` crate dep: confirm OK to add to `chorale-core`.**
   The author is the same John McNamara who maintains the definitive Perl Excel library
   (~25 years). BSD-2-Clause licensed. ~50 transitive deps, none controversial.
   Adding behind `xlsx` feature flag per DEPS-1. The feature flag keeps the dep opt-in
   for callers who don't need XLSX. Confirm dep addition.

2. **Function signature: `to_xlsx(state, options) -> Result<Vec<u8>, XlsxError>` (bytes)
   vs `write_xlsx(state, options, &mut impl Write)` (caller-supplied writer).**
   Recommendation: bytes version for v0.2.0. In a WASM context (browser download), the
   caller needs bytes to construct a `Blob`; a `Write` impl pointing at a JS stream adds
   complexity. A `write_to` variant can layer in additively in v0.3.0 for server-side
   streaming use cases. Confirm bytes version.

3. **`XlsxOptions` vs inline parameters.** Recommendation: `XlsxOptions` struct marked
   `#[non_exhaustive]` with `Default` impl. This allows v0.3.0 to add fields (number
   format, frozen pane row count, custom column widths) without breaking existing callers
   who construct via `XlsxOptions::default()` or `..Default::default()`. The alternative
   (adding parameters to `to_xlsx` directly) would be a breaking change each time v0.3.0
   adds an option. Confirm `XlsxOptions` struct.

4. **Sheet name: prop with default `"Sheet1"` vs hardcoded `"Sheet1"` (like `to_csv` has no
   name concept).** Recommendation: prop with `Default::default()` = `"Sheet1"`. It costs
   nothing to expose and is a commonly requested customization. `to_csv` has no filename
   concept because CSV has no internal name; XLSX does, so they're not comparable. Confirm
   prop with default.

5. **Empty state (filter excludes all rows): write headers + empty body vs error.**
   Recommendation: headers + empty body, consistent with `to_csv`. Erroring on empty export
   forces the host to check `visible_row_count()` before calling, which is unnecessary
   boilerplate. An empty XLSX is a valid result (the user filtered to nothing and exported;
   that's fine). Confirm headers + empty body.

6. **Bold headers: default `true` vs default `false` (matching the `to_csv` "values-only"
   posture).** Recommendation: default `true`. Bold headers are the one minimal style that
   makes the XLSX export usably readable without any other formatting. Users who want
   plain-data output set `bold_headers: false`. This is a quality-of-life default, not a
   style-system feature. Confirm bold-by-default.

7. **Feature flag name: `xlsx` vs `export-xlsx` vs enabled-by-default.** Recommendation:
   `xlsx` feature flag in `chorale-core`, opt-out. Adapter crates (`chorale-dioxus`,
   `chorale-leptos`) enable it in their default features. This gives "batteries included"
   behavior for adapter users while keeping the core dep tree thin for non-WASM consumers
   of chorale-core (e.g., a server-side Rust app that uses chorale-core for table logic
   but doesn't need XLSX). Confirm feature-flag approach and name.

## Decisions (signed off 2026-06-05)

All 7 recommendations accepted, with **a substantive scope upgrade**: XLSX
export ships with **auto-inferred styling** in v0.2.0 rather than
values-only. The memo title remains "XLSX Export (Values-Only)" for
historical traceability, but implementation goes beyond raw values.

**Reasoning:** Zach's pushback — "if it's only exporting data and not
styling, that can be deferred to v0.3.0 alongside CSV; otherwise it
duplicates CSV with no differentiation." The way to ship styled XLSX in
v0.2.0 without a declarative cell-style system is to **infer styles from
chorale's existing concepts**. No new public API; column definitions and
`CellValue` variants drive both the on-screen rendering AND the XLSX
output.

### Auto-inferred styling map (v0.2.0)

| Source | XLSX style applied |
|---|---|
| `CellValue::Number(f64)` | Default number format |
| `CellValue::Currency(f64, CurrencyCode)` | `"$#,##0.00"` for USD; locale-appropriate symbol for other ISO codes |
| `CellValue::Percentage(f64)` | `"0.00%"` |
| `CellValue::Date` / `chrono::NaiveDate` | `"yyyy-mm-dd"` |
| `CellValue::Boolean(b)` | Center-aligned `"TRUE"` / `"FALSE"` |
| `ColumnDef.alignment` | XLSX cell alignment (Left/Center/Right) |
| `ColumnDef.initial_width` | XLSX column width |
| `ColumnDef.frozen == FrozenSide::Left` | XLSX `freeze_panes` left of this column |
| `ColumnDef.frozen == FrozenSide::Right` | XLSX `freeze_panes` right of this column |
| Header row | Bold + background fill `#f8f9fa` (matches the on-screen sticky-header convention) |

This gives users an XLSX they can open in Excel with number formats,
currency symbols, frozen panes, and column widths preserved. Materially
different from CSV.

### What's NOT in v0.2.0 (deferred to v0.3.0+)
- Conditional formatting (e.g., "red cells where value > 100")
- Per-cell style overrides (declarative rule engine needed first)
- Custom number format strings
- Embedded images
- Multiple sheets per export
- Formulas
- Cell comments

Those require a declarative cell-style system in chorale-core that
v0.2.0 does not introduce.

### Per-question sign-off

1. ✅ `rust_xlsxwriter` crate dep approved. BSD-2-Clause, same author
   as Perl Excel::Writer::XLSX (25 years). Behind `xlsx` feature flag
   per DEPS-1.
2. ✅ Signature = `to_xlsx(state, options) -> Result<Vec<u8>, XlsxError>`
   (bytes). `write_xlsx(state, options, &mut impl Write)` can layer in
   additively in v0.3 for server-side streaming.
3. ✅ `XlsxOptions` struct marked `#[non_exhaustive]` with `Default`
   impl. Allows v0.3.0 to add fields without breaking callers.
4. ✅ Sheet name = prop on `XlsxOptions` with default `"Sheet1"`.
5. ✅ Empty state = headers + empty body (not error). Consistent w/
   `to_csv`. Filter-to-zero export is a valid result.
6. ✅ Bold headers default = `true`. Plus header-row background fill
   (per the auto-inferred styling map above). `bold_headers: false`
   opts out of bold but keeps the background fill.
7. ✅ Feature flag = `xlsx`, enabled by default in adapter crates
   (`chorale-dioxus`, `chorale-leptos`). Thin core for server-side
   consumers.

### Implementation note for the bot

The XLSX styling inference is mechanical — pattern-match on `CellValue`
variants and `ColumnDef` fields, map to `rust_xlsxwriter::Format` objects,
apply per-cell at write time. Reuse the existing column-iteration code
path from `to_csv`. The header-row styling is one-time at the top of the
sheet. Frozen panes: at most one freeze split per direction
(left-frozen + right-frozen are not simultaneously expressible in XLSX;
prefer left if both are set, log a doc-comment about the limitation).
