//! Leptos components for chorale tables.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chorale_core::{
    add_disjoint_range, cancel_edit, clear_active_cell, clear_range_selection, commit_edit,
    extend_range_to, fill_handle_targets, frozen_left_columns, frozen_right_columns,
    move_active_cell, move_active_cell_end, move_active_cell_first, move_active_cell_home,
    move_active_cell_last, move_active_cell_page, move_active_cell_to_edge, scrollable_columns,
    select_all as select_all_range, start_range_selection, theme_stylesheet, to_csv,
    visible_grouped_view, visible_view, visible_window, visible_window_variable, ActiveCell,
    Alignment, CellValue, ClipboardCopyEvent, ClipboardPasteEvent, ColumnDef, ColumnId,
    CommittedEdit, EditorKind, FilterKind, FilterValue, GroupKey, GroupedPaginationMode,
    GroupedRow, Labels, NaiveDate, NavDirection, PaginationMode, RangeSelection, RenderKind,
    RenderRow, RowId, SortAction, SortDirection, SortState, TableState, Theme, VirtualWindow,
    THEME_ROOT_CLASS,
};
#[cfg(target_arch = "wasm32")]
use chorale_core::{batch_record_row_heights, paste_tsv_into_range, to_clipboard_tsv};
use leptos::html;
use leptos::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

use crate::hooks::UseTableHandle;

/// Type-erased cell renderer: maps a [`CellValue`] to a Leptos [`AnyView`].
///
/// Build with `Arc::new(|val| view! { ... }.into_any())` and register
/// via [`CellRenderers::new`].
pub type CellRenderer = Arc<dyn Fn(&CellValue) -> AnyView + Send + Sync + 'static>;

/// Per-row detail renderer: takes ownership of a row and returns a Leptos [`AnyView`].
///
/// Used by the `detail_renderer` prop on [`Table`]. Build with
/// `Arc::new(|row: MyRow| view! { ... }.into_any())`.
pub type DetailRenderer<TRow> = Arc<dyn Fn(TRow) -> AnyView + Send + Sync + 'static>;

/// Per-column map of custom cell renderers; default is empty (all columns use `RenderKind`).
#[derive(Clone, Default)]
pub struct CellRenderers(Arc<HashMap<ColumnId, CellRenderer>>);

impl CellRenderers {
    /// Create a `CellRenderers` from a map of column-id to renderer closure.
    #[must_use]
    pub fn new(map: HashMap<ColumnId, CellRenderer>) -> Self {
        Self(Arc::new(map))
    }

    fn get(&self, col: ColumnId) -> Option<CellRenderer> {
        self.0.get(&col).cloned()
    }
}

impl PartialEq for CellRenderers {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Type-erased row-aware cell renderer: maps the full row plus the cell's
/// [`CellValue`] to a Leptos [`AnyView`].
///
/// Use this instead of [`CellRenderer`] when the cell needs data from other
/// fields on the row: composite cells (avatar + name), action columns that
/// need the row's id, link cells that build an href from a sibling field.
/// Build with `Arc::new(|row: &MyRow, val: &CellValue| view! { ... }.into_any())`
/// and register via [`RowCellRenderers::new`].
pub type RowCellRenderer<TRow> = Arc<dyn Fn(&TRow, &CellValue) -> AnyView + Send + Sync + 'static>;

/// Per-column map of row-aware cell renderers; default is empty.
///
/// Entries here take precedence over [`CellRenderers`] entries, which take
/// precedence over the column's `RenderKind`. Compared by pointer identity.
pub struct RowCellRenderers<TRow>(Arc<HashMap<ColumnId, RowCellRenderer<TRow>>>);

impl<TRow> RowCellRenderers<TRow> {
    /// Create a `RowCellRenderers` from a map of column-id to renderer closure.
    #[must_use]
    pub fn new(map: HashMap<ColumnId, RowCellRenderer<TRow>>) -> Self {
        Self(Arc::new(map))
    }

    fn get(&self, col: ColumnId) -> Option<RowCellRenderer<TRow>> {
        self.0.get(&col).cloned()
    }
}

// Manual impls: `#[derive(...)]` would add unwanted `TRow: Clone / Default`
// bounds; the Arc makes these free regardless of TRow.
impl<TRow> Clone for RowCellRenderers<TRow> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<TRow> Default for RowCellRenderers<TRow> {
    fn default() -> Self {
        Self(Arc::new(HashMap::new()))
    }
}

impl<TRow> PartialEq for RowCellRenderers<TRow> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Input passed to the `validate_edit` callback before a cell edit is committed.
#[derive(Clone, Debug, PartialEq)]
pub struct EditValidation {
    /// The row being edited.
    pub row_id: RowId,
    /// The column being edited.
    pub column_id: ColumnId,
    /// The raw string value the user typed.
    pub raw_value: String,
}

type ValidateClosure = Arc<dyn Fn(EditValidation) -> Result<(), String> + Send + Sync + 'static>;

/// Optional synchronous validation function for in-cell editing.
///
/// Build with `ValidateEditFn::new(|v| { ... })`. Default is "no validation"
/// (all commits are allowed). Compared by pointer identity for prop diffing.
#[derive(Clone, Default)]
pub struct ValidateEditFn(Option<ValidateClosure>);

impl ValidateEditFn {
    /// Wrap a validation closure.
    #[must_use]
    pub fn new(f: impl Fn(EditValidation) -> Result<(), String> + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(f)))
    }

    fn call(&self, v: EditValidation) -> Result<(), String> {
        self.0.as_ref().map_or(Ok(()), |f| f(v))
    }
}

impl PartialEq for ValidateEditFn {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// ExportXlsxButton
// ---------------------------------------------------------------------------

/// Button that exports the current filtered+sorted view as an XLSX file.
///
/// Requires the `xlsx` feature on both `chorale-leptos` and `chorale-core`,
/// plus a `wasm32` target — the click handler uses browser APIs
/// (`document.createElement`, `<a download>`) so the component is only
/// available in CSR builds. Native (SSR) builds can render the button
/// markup via plain `view!` if needed.
#[cfg(all(feature = "xlsx", target_arch = "wasm32"))]
#[component]
pub fn ExportXlsxButton<TRow: Clone + PartialEq + Send + Sync + 'static>(
    /// Table handle providing access to the current state.
    handle: UseTableHandle<TRow>,
    /// Sheet tab name written into the workbook. Defaults to `"Sheet1"`.
    #[prop(default = String::from("Sheet1"))]
    sheet_name: String,
    /// File name the browser prompts with. Defaults to `"export.xlsx"`.
    #[prop(default = String::from("export.xlsx"))]
    filename: String,
    /// Button label / child elements.
    children: Children,
) -> impl IntoView {
    view! {
        <button
            on:click=move |_| {
                #[cfg(target_arch = "wasm32")]
                {
                    use chorale_core::{XlsxOptions, to_xlsx};
                    let state = handle.signal.get_untracked();
                    let mut opts = XlsxOptions::default();
                    opts.sheet_name.clone_from(&sheet_name);
                    let Ok(bytes) = to_xlsx(&state, &opts) else { return };
                    // Blob + object URL (NOT a data: URL) so repeated exports
                    // auto-increment the filename like the Dioxus adapter.
                    trigger_xlsx_download(&bytes, &filename);
                }
                #[cfg(not(target_arch = "wasm32"))]
                let _ = (&handle, &sheet_name, &filename);
            }
        >
            {children()}
        </button>
    }
}

// ---------------------------------------------------------------------------
// Page-button helper
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum PageItem {
    Page(usize),
    Ellipsis,
}

fn page_button_range(current: usize, total: usize) -> Vec<PageItem> {
    if total <= 7 {
        return (0..total).map(PageItem::Page).collect();
    }
    let mut items = Vec::new();
    items.push(PageItem::Page(0));
    if current > 3 {
        items.push(PageItem::Ellipsis);
    }
    let start = current.saturating_sub(2).max(1);
    let end = (current + 3).min(total - 1);
    for p in start..end {
        items.push(PageItem::Page(p));
    }
    if end < total - 1 {
        items.push(PageItem::Ellipsis);
    }
    items.push(PageItem::Page(total - 1));
    items
}

// ---------------------------------------------------------------------------
// Width / alignment helpers
// ---------------------------------------------------------------------------

fn col_width_style(override_px: Option<f64>, initial: Option<f64>) -> String {
    if let Some(w) = override_px.or(initial) {
        format!("width: {w}px; min-width: {w}px; max-width: {w}px;")
    } else {
        String::new()
    }
}

fn alignment_css(a: Alignment) -> &'static str {
    match a {
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::Left | _ => "left",
    }
}

// ---------------------------------------------------------------------------
// Virtualization window slice
// ---------------------------------------------------------------------------

/// Given the already-computed `visible_view` and the current state, returns
/// the virtualization window plus the windowed row slice in a single pass.
///
/// When `variable` is `true`, dispatches to [`visible_window_variable`]
/// (VIRT-2): the window and its top/bottom spacer pads are computed from a
/// prefix sum over `state.row_heights` (measured per-row heights, with
/// `state.row_height` as the fallback estimate for unmeasured rows).
/// Otherwise uses the fixed-height [`visible_window`] (VIRT-1). Mirrors the
/// Dioxus adapter's `compute_window_slice` dispatch exactly.
fn compute_window_slice<TRow: Clone>(
    state: &TableState<TRow>,
    view: &[RenderRow<TRow>],
    variable: bool,
) -> (VirtualWindow, Vec<RenderRow<TRow>>) {
    let total = view.len();
    let win = if variable {
        visible_window_variable(
            &state.row_heights,
            state.scroll_top,
            state.viewport_height,
            state.row_height,
            total,
            state.buffer_rows,
        )
    } else {
        visible_window(
            state.scroll_top,
            state.viewport_height,
            state.row_height,
            total,
            state.buffer_rows,
        )
    };
    if total == 0 {
        return (win, vec![]);
    }
    let win_end = win.end_index.min(total.saturating_sub(1));
    let slice = view[win.start_index..=win_end].to_vec();
    (win, slice)
}

/// After a keyboard move changes the active cell, scroll the virtualized
/// container so the active row stays inside the viewport (arrow keys,
/// `PageUp`/`PageDown`, `Home`/`End` would otherwise walk the active cell
/// off-screen).
///
/// Row geometry comes from state, not the DOM — the target row may not be
/// rendered yet under virtualization (e.g. a `PageDown` jump past the
/// rendered window). Uniform mode uses `index * row_height`; variable mode (VIRT-2)
/// prefix-sums `state.row_heights` with `state.row_height` as the fallback
/// for unmeasured rows — the same math as `visible_window_variable`.
///
/// Writes the clamped offset to BOTH `TableState::scroll_top` (via
/// `handle.set_scroll`, so the virtualization window recomputes immediately)
/// and the DOM container's `scrollTop` (so the browser viewport follows).
#[cfg(target_arch = "wasm32")]
fn scroll_active_cell_into_view<TRow: Clone + PartialEq + Send + Sync + 'static>(
    handle: &UseTableHandle<TRow>,
    scroll_ref: NodeRef<html::Div>,
    variable_row_height: bool,
) {
    let Some(container) = scroll_ref.get_untracked() else {
        return;
    };
    let Some((row_top, row_h, cur_scroll, viewport_h)) = handle.signal.with_untracked(|s| {
        let ac = s.active_cell?;
        let idx = ac.row_idx;
        let default_h = s.row_height;
        let (top, h) = if variable_row_height {
            let top: f64 = (0..idx)
                .map(|i| s.row_heights.get(&i).copied().unwrap_or(default_h))
                .sum();
            let h = s.row_heights.get(&idx).copied().unwrap_or(default_h);
            (top, h)
        } else {
            #[allow(clippy::cast_precision_loss)]
            let top = idx as f64 * default_h;
            (top, default_h)
        };
        Some((top, h, s.scroll_top, s.viewport_height))
    }) else {
        return;
    };
    if viewport_h <= 0.0 || row_h <= 0.0 {
        return;
    }
    // The sticky <thead> scrolls inside the container and permanently covers
    // its top band, so the usable row viewport is `viewport_h - header_h`.
    // Row content offsets are shifted down by the same `header_h`, which
    // cancels out in the scroll-up branch (row_top alignment lands the row
    // exactly below the sticky header) but adds to the scroll-down branch
    // (the row bottom must clear both the container bottom edge and the
    // header-shifted content origin).
    let header_h = container
        .query_selector(":scope > table > thead")
        .ok()
        .flatten()
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        .map_or(0.0, |el| f64::from(el.offset_height()));
    let mut new_scroll = cur_scroll;
    if row_top < cur_scroll {
        // Active row is above the viewport: align its top to the top of the
        // visible row band (just below the sticky header).
        new_scroll = row_top;
    } else if row_top + row_h + header_h > cur_scroll + viewport_h {
        // Active row is below the viewport: align its bottom to the
        // container's bottom edge.
        new_scroll = row_top + row_h + header_h - viewport_h;
    }
    new_scroll = new_scroll.max(0.0);
    if (new_scroll - cur_scroll).abs() < f64::EPSILON {
        return;
    }
    handle.set_scroll(new_scroll);
    #[allow(clippy::cast_possible_truncation)]
    container.set_scroll_top(new_scroll.round() as i32);
}

/// Host (non-wasm) no-op: there is no DOM scroll container to adjust. Keeps
/// the keydown handler's call sites un-cfg'd so the host build type-checks
/// the same code path.
#[cfg(not(target_arch = "wasm32"))]
fn scroll_active_cell_into_view<TRow: Clone + PartialEq + Send + Sync + 'static>(
    _handle: &UseTableHandle<TRow>,
    _scroll_ref: NodeRef<html::Div>,
    _variable_row_height: bool,
) {
}

// ---------------------------------------------------------------------------
// Badge and currency helpers
// ---------------------------------------------------------------------------

fn badge_style(color: &str) -> String {
    let (bg, fg) = match color {
        "green" => (
            "var(--chorale-badge-green-bg, #d1fae5)",
            "var(--chorale-badge-green-text, #065f46)",
        ),
        "yellow" => (
            "var(--chorale-badge-yellow-bg, #fef3c7)",
            "var(--chorale-badge-yellow-text, #92400e)",
        ),
        "red" => (
            "var(--chorale-badge-red-bg, #fee2e2)",
            "var(--chorale-badge-red-text, #991b1b)",
        ),
        "gray" => (
            "var(--chorale-badge-gray-bg, #f3f4f6)",
            "var(--chorale-badge-gray-text, #374151)",
        ),
        _ => (
            "var(--chorale-badge-default-bg, #e5e7eb)",
            "var(--chorale-badge-default-text, #1f2937)",
        ),
    };
    format!(
        "display:inline-block;padding:0.125rem 0.5rem;border-radius:9999px;\
         background:{bg};color:{fg};font-size:0.75rem;font-weight:500;"
    )
}

fn currency_symbol(code: &chorale_core::CurrencyCode) -> &'static str {
    match code.0 {
        "USD" => "$",
        "EUR" => "\u{20ac}",
        "GBP" => "\u{00a3}",
        _ => "",
    }
}

fn format_thousands(n: i64) -> String {
    let abs = n.unsigned_abs();
    let s = abs.to_string();
    let with_commas = s
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(",");
    if n < 0 {
        format!("-{with_commas}")
    } else {
        with_commas
    }
}

fn cell_text(val: &CellValue) -> String {
    match val {
        CellValue::Boolean(b) => (if *b { "\u{2713}" } else { "\u{2717}" }).to_string(),
        _ => val.to_csv_string(),
    }
}

// ---------------------------------------------------------------------------
// Cell value rendering
// ---------------------------------------------------------------------------

fn render_cell_value(
    val: &CellValue,
    render_kind: &RenderKind,
    renderer: Option<&CellRenderer>,
) -> AnyView {
    if let Some(r) = renderer {
        return r(val);
    }
    match render_kind {
        RenderKind::Badge(map) => {
            let text = val.to_csv_string();
            if let Some(variant) = map.resolve(&text) {
                let label = variant.label.clone();
                let style = badge_style(&variant.color);
                view! { <span style=style>{label}</span> }.into_any()
            } else {
                view! { <span>{text}</span> }.into_any()
            }
        }
        RenderKind::Currency(code) => {
            let symbol = currency_symbol(code);
            let text = match val {
                CellValue::Float(f) => format!("{symbol}{f:.2}"),
                CellValue::Integer(i) => {
                    format!("{symbol}{}.00", format_thousands(*i))
                }
                _ => format!("{symbol}{}", val.to_csv_string()),
            };
            view! { <span>{text}</span> }.into_any()
        }
        RenderKind::Number => {
            let text = match val {
                CellValue::Integer(n) => format_thousands(*n),
                CellValue::Float(f) => format!("{f:.0}"),
                _ => val.to_csv_string(),
            };
            view! { <span>{text}</span> }.into_any()
        }
        RenderKind::DateTime => {
            let text = match val {
                CellValue::DateTime(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
                _ => val.to_csv_string(),
            };
            view! { <span>{text}</span> }.into_any()
        }
        _ => {
            let text = cell_text(val);
            view! { <span>{text}</span> }.into_any()
        }
    }
}

/// Resolve a data cell's content with the renderer precedence chain:
/// row-aware renderer > value-only renderer > the column's `RenderKind`.
fn resolve_cell_content<TRow>(
    row: &TRow,
    val: &CellValue,
    render_kind: &RenderKind,
    row_renderer: Option<&RowCellRenderer<TRow>>,
    value_renderer: Option<&CellRenderer>,
) -> AnyView {
    if let Some(rr) = row_renderer {
        rr(row, val)
    } else {
        render_cell_value(val, render_kind, value_renderer)
    }
}

/// A plain left-click on a data cell is a "row click"; Ctrl/Cmd/Shift
/// clicks are range-selection operations and must not fire `on_row_click`.
fn should_fire_row_click(ctrl: bool, shift: bool) -> bool {
    !ctrl && !shift
}

// ---------------------------------------------------------------------------
// GotoPage sub-component
// ---------------------------------------------------------------------------

#[component]
fn GotoPageInput<TRow: Clone + PartialEq + Send + Sync + 'static>(
    handle: UseTableHandle<TRow>,
    total_pages: usize,
    labels: Arc<Labels>,
) -> impl IntoView {
    let input_val = RwSignal::new(String::new());
    view! {
        <span style="display:inline-flex;align-items:center;gap:0.25rem;font-size:0.875rem;">
            {labels.go_to_page_label.clone()}
            <input
                type="number"
                min="1"
                max=total_pages.to_string()
                value=move || input_val.get()
                style="width:4rem;padding:0.125rem 0.25rem;border:1px solid var(--chorale-border, #ddd);\
                       border-radius:3px;font-size:0.875rem;"
                on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                on:input=move |ev| {
                    input_val.set(event_target_value(&ev));
                }
                on:keydown=move |ev| {
                    if ev.key() == "Enter" {
                        let val = input_val.get_untracked();
                        if let Ok(p) = val.trim().parse::<usize>() {
                            handle.set_page(p.saturating_sub(1)).ok();
                            input_val.set(String::new());
                        }
                    }
                }
            />
            {(labels.page_count)(1, total_pages).split_once(' ').map_or_else(
                || format!("of {total_pages}"),
                |(_, rest)| rest.to_owned(),
            )}
        </span>
    }
}

// ---------------------------------------------------------------------------
// Column visibility toolbar
// ---------------------------------------------------------------------------

fn column_visibility_toolbar<TRow: Clone + PartialEq + Send + Sync + 'static>(
    all_cols: &[(ColumnId, String)],
    visibility: &HashMap<ColumnId, bool>,
    handle: UseTableHandle<TRow>,
    labels: &Labels,
) -> AnyView {
    let items: Vec<(ColumnId, String, bool)> = all_cols
        .iter()
        .map(|(id, hdr)| (*id, hdr.clone(), *visibility.get(id).unwrap_or(&true)))
        .collect();
    let title = labels.column_visibility_label.clone();
    view! {
        <div style="padding: 0.5rem 1rem; border-bottom: 1px solid var(--chorale-border, #ddd); \
                    display: flex; flex-wrap: wrap; gap: 0.5rem; align-items: center; \
                    font-size: 0.875rem; background: var(--chorale-toolbar-bg, #fafafa);">
            <span style="font-weight: 600; margin-right: 0.25rem;">{title}</span>
            {items.into_iter().map(|(col_id, header, visible)| {
                view! {
                    <label style="display:inline-flex;align-items:center;gap:0.25rem;cursor:pointer;">
                        <input
                            type="checkbox"
                            checked=visible
                            on:change=move |ev| {
                                handle.set_column_visibility(col_id, event_target_checked(&ev));
                            }
                        />
                        {header}
                    </label>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Filter cell helpers
// ---------------------------------------------------------------------------

fn text_filter_input<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
    placeholder: &str,
    clear_label: &str,
) -> AnyView {
    let val = match current {
        Some(FilterValue::Text(s)) => s.clone(),
        _ => String::new(),
    };
    let has_filter = current.is_some();
    let placeholder = placeholder.to_owned();
    let clear_label = clear_label.to_owned();
    view! {
        <div style="display:flex;align-items:center;gap:2px;">
            <input
                type="text"
                value=val
                placeholder=placeholder
                style="flex:1;min-width:0;padding:0.25rem;border:1px solid var(--chorale-border, #ddd);\
                       border-radius:3px;font-size:0.8rem;box-sizing:border-box;"
                on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    if v.is_empty() {
                        handle.set_filter(col_id, None);
                    } else {
                        handle.set_filter(col_id, Some(FilterValue::Text(v)));
                    }
                }
            />
            {has_filter.then(|| view! {
                <button
                    type="button"
                    title=clear_label
                    style="border:0;background:transparent;padding:0 4px;\
                           cursor:pointer;color:var(--chorale-text-subtle, #888);font-size:0.95rem;line-height:1;flex-shrink:0;"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        handle.set_filter(col_id, None);
                    }
                >
                    "\u{00d7}"
                </button>
            })}
        </div>
    }
    .into_any()
}

fn boolean_filter_input<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
) -> AnyView {
    let is_true = matches!(current, Some(FilterValue::Boolean(true)));
    let is_false = matches!(current, Some(FilterValue::Boolean(false)));
    let is_all = !is_true && !is_false;

    view! {
        <select
            style="width:100%;padding:0.25rem;border:1px solid var(--chorale-border, #ddd);\
                   border-radius:3px;font-size:0.8rem;"
            on:change=move |ev| {
                let v = event_target_value(&ev);
                let filter = match v.as_str() {
                    "true" => Some(FilterValue::Boolean(true)),
                    "false" => Some(FilterValue::Boolean(false)),
                    _ => None,
                };
                handle.set_filter(col_id, filter);
            }
        >
            <option value="" selected=is_all>"All"</option>
            <option value="true" selected=is_true>"True"</option>
            <option value="false" selected=is_false>"False"</option>
        </select>
    }
    .into_any()
}

fn numeric_range_to_filter(
    min: f64,
    max: f64,
    bound_min: f64,
    bound_max: f64,
) -> Option<FilterValue> {
    let min_at_bound = (min - bound_min).abs() < f64::EPSILON;
    let max_at_bound = (max - bound_max).abs() < f64::EPSILON;
    if min_at_bound && max_at_bound {
        None
    } else {
        Some(FilterValue::NumericRange {
            min: if min_at_bound { None } else { Some(min) },
            max: if max_at_bound { None } else { Some(max) },
        })
    }
}

fn commit_numeric_range<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    min: f64,
    max: f64,
    bound_min: f64,
    bound_max: f64,
    handle: &UseTableHandle<TRow>,
) {
    handle.set_filter(
        col_id,
        numeric_range_to_filter(min, max, bound_min, bound_max),
    );
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn format_compact_number(n: f64) -> String {
    let abs = n.abs();
    if abs >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.0}k", n / 1_000.0)
    } else {
        format!("{n:.0}")
    }
}

fn numeric_range_filter<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    bound_min: f64,
    bound_max: f64,
    step: f64,
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
    clear_label: &str,
) -> AnyView {
    let (cur_min, cur_max) = match current {
        Some(FilterValue::NumericRange { min, max }) => {
            (min.unwrap_or(bound_min), max.unwrap_or(bound_max))
        }
        _ => (bound_min, bound_max),
    };
    let min_display = format_compact_number(cur_min);
    let max_display = format_compact_number(cur_max);
    let has_filter = current.is_some();
    let clear_label = clear_label.to_owned();
    let bound_min_s = bound_min.to_string();
    let bound_max_s = bound_max.to_string();
    let step_s = step.to_string();

    view! {
        <div style="display:flex;flex-direction:column;gap:2px;font-size:0.75rem;"
            on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()>
            <div style="display:flex;justify-content:space-between;color:var(--chorale-text-muted, #555);">
                <span>{min_display}</span>
                <span>{max_display}</span>
            </div>
            <input
                type="range"
                min=bound_min_s.clone()
                max=bound_max_s.clone()
                step=step_s.clone()
                value=cur_min.to_string()
                style="width:100%;margin:0;"
                on:input=move |ev| {
                    if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                        let new_min = v.min(cur_max);
                        commit_numeric_range(col_id, new_min, cur_max, bound_min, bound_max, &handle);
                    }
                }
            />
            <input
                type="range"
                min=bound_min_s
                max=bound_max_s
                step=step_s
                value=cur_max.to_string()
                style="width:100%;margin:0;"
                on:input=move |ev| {
                    if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                        let new_max = v.max(cur_min);
                        commit_numeric_range(col_id, cur_min, new_max, bound_min, bound_max, &handle);
                    }
                }
            />
            {has_filter.then(|| view! {
                <button
                    type="button"
                    title=clear_label
                    style="border:0;background:transparent;padding:0 4px;\
                           cursor:pointer;color:var(--chorale-text-subtle, #888);font-size:0.95rem;line-height:1;align-self:flex-end;"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        handle.set_filter(col_id, None);
                    }
                >
                    "\u{00d7}"
                </button>
            })}
        </div>
    }
    .into_any()
}

fn multiselect_filter<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    options: &[&str],
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
    clear_label: &str,
    open_filter_col: RwSignal<Option<ColumnId>>,
) -> AnyView {
    let selected: HashSet<String> = match current {
        Some(FilterValue::MultiSelect(v)) => v.iter().cloned().collect(),
        _ => HashSet::new(),
    };
    let has_filter = current.is_some();
    let options: Vec<String> = options.iter().map(|s| (*s).to_owned()).collect();
    let clear_label = clear_label.to_owned();

    // Open state lives in the Table-scoped `open_filter_col` signal, NOT a
    // local RwSignal: this function re-runs on every filter change (checking
    // a box mutates s.filters -> view_key -> header re-render), and a local
    // signal would reset to closed after each pick. Outside-click closing is
    // handled by the single CAPTURE-PHASE document listener in the Table
    // body (capture so inner `stop_propagation()` — e.g. on data cells —
    // cannot suppress it); that listener exempts clicks inside
    // `.chorale-filter-dropdown` (keep open) and `.chorale-filter-toggle`
    // (this button's handler owns toggling).

    let count_label = if selected.is_empty() {
        "All".to_owned()
    } else {
        format!("{} selected", selected.len())
    };

    view! {
        <div
            style="display:flex;align-items:center;gap:2px;"
            on:click=move |ev| { ev.stop_propagation(); }
        >
            <div style="flex:1;min-width:0;position:relative;">
                // The `chorale-filter-toggle` class is load-bearing: the
                // capture-phase document listener in the Table body skips
                // clicks landing here so this handler keeps exclusive
                // ownership of open/close/switch toggling.
                <button
                    class="chorale-filter-toggle"
                    style="width:100%;padding:0.2rem 0.4rem;border:1px solid var(--chorale-border, #ddd);\
                           border-radius:3px;font-size:0.8rem;text-align:left;cursor:pointer;\
                           background:var(--chorale-input-bg, white);"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        open_filter_col.update(|c| {
                            *c = if *c == Some(col_id) { None } else { Some(col_id) };
                        });
                    }
                >
                    {count_label}
                </button>
                <Show when=move || open_filter_col.get() == Some(col_id)>
                    // z-index must beat sticky-header cells (z-index:1) and
                    // frozen-column body cells (z-index:2). See Bug 2 fix for
                    // why filter_th now carries z-index:3.
                    //
                    // The `chorale-filter-dropdown` class is load-bearing:
                    // the capture-phase document listener in the Table body
                    // uses `target.closest(...)` against it to keep clicks
                    // inside the dropdown (checkboxes, scrollbar) from
                    // closing it.
                    <div class="chorale-filter-dropdown"
                        style="position:absolute;top:100%;left:0;z-index:9999;\
                                 background:var(--chorale-popover-bg, white);border:1px solid var(--chorale-border, #ddd);border-radius:3px;\
                                 padding:0.25rem;min-width:8rem;max-height:200px;\
                                 overflow-y:auto;box-shadow:var(--chorale-popover-shadow, 0 2px 8px rgba(0,0,0,0.15));"
                        on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    >
                        {options.iter().map(|opt| {
                            let opt_clone = opt.clone();
                            let sel_clone = selected.clone();
                            let is_checked = sel_clone.contains(opt);
                            view! {
                                <label style="display:flex;align-items:center;gap:0.25rem;\
                                             padding:0.2rem 0.25rem;cursor:pointer;font-size:0.8rem;">
                                    <input
                                        type="checkbox"
                                        checked=is_checked
                                        on:change=move |ev| {
                                            ev.stop_propagation();
                                            let checked = event_target_checked(&ev);
                                            let cur = handle.signal().with_untracked(|s|
                                                s.filters.get(&col_id).cloned()
                                            );
                                            let mut cur_set: HashSet<String> = match cur {
                                                Some(FilterValue::MultiSelect(v)) =>
                                                    v.into_iter().collect(),
                                                _ => HashSet::new(),
                                            };
                                            if checked {
                                                cur_set.insert(opt_clone.clone());
                                            } else {
                                                cur_set.remove(&opt_clone);
                                            }
                                            let filter = if cur_set.is_empty() {
                                                None
                                            } else {
                                                Some(FilterValue::MultiSelect(cur_set))
                                            };
                                            handle.set_filter(col_id, filter);
                                        }
                                    />
                                    {opt.clone()}
                                </label>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                </Show>
            </div>
            {has_filter.then(|| view! {
                <button
                    type="button"
                    title=clear_label
                    style="border:0;background:transparent;padding:0 4px;\
                           cursor:pointer;color:var(--chorale-text-subtle, #888);font-size:0.95rem;line-height:1;flex-shrink:0;"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        handle.set_filter(col_id, None);
                    }
                >
                    "\u{00d7}"
                </button>
            })}
        </div>
    }
    .into_any()
}

fn date_range_filter<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
    clear_label: &str,
) -> AnyView {
    let (min_s, max_s) = match current {
        Some(FilterValue::DateRange { min, max }) => (
            min.map(|d| d.to_string()).unwrap_or_default(),
            max.map(|d| d.to_string()).unwrap_or_default(),
        ),
        _ => (String::new(), String::new()),
    };
    let has_filter = current.is_some();
    let clear_label = clear_label.to_owned();

    view! {
        <div style="display:flex;align-items:center;gap:2px;">
            <input
                type="date"
                value=min_s
                style="flex:1;min-width:0;padding:0.25rem;border:1px solid var(--chorale-border, #ddd);\
                       border-radius:3px;font-size:0.8rem;"
                on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    let new_min: Option<NaiveDate> = v.parse().ok();
                    let cur = handle.signal().with_untracked(|s| s.filters.get(&col_id).cloned());
                    let cur_max = match cur {
                        Some(FilterValue::DateRange { max, .. }) => max,
                        _ => None,
                    };
                    let filter = if new_min.is_none() && cur_max.is_none() {
                        None
                    } else {
                        Some(FilterValue::DateRange { min: new_min, max: cur_max })
                    };
                    handle.set_filter(col_id, filter);
                }
            />
            <input
                type="date"
                value=max_s
                style="flex:1;min-width:0;padding:0.25rem;border:1px solid var(--chorale-border, #ddd);\
                       border-radius:3px;font-size:0.8rem;"
                on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    let new_max: Option<NaiveDate> = v.parse().ok();
                    let cur = handle.signal().with_untracked(|s| s.filters.get(&col_id).cloned());
                    let cur_min = match cur {
                        Some(FilterValue::DateRange { min, .. }) => min,
                        _ => None,
                    };
                    let filter = if cur_min.is_none() && new_max.is_none() {
                        None
                    } else {
                        Some(FilterValue::DateRange { min: cur_min, max: new_max })
                    };
                    handle.set_filter(col_id, filter);
                }
            />
            {has_filter.then(|| view! {
                <button
                    type="button"
                    title=clear_label
                    style="border:0;background:transparent;padding:0 4px;\
                           cursor:pointer;color:var(--chorale-text-subtle, #888);font-size:0.95rem;line-height:1;flex-shrink:0;"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        handle.set_filter(col_id, None);
                    }
                >
                    "\u{00d7}"
                </button>
            })}
        </div>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Header th
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn header_th<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col: &ColumnDef<TRow>,
    override_width: Option<f64>,
    handle: UseTableHandle<TRow>,
    sort_enabled: bool,
    current_sort: &[SortState],
    resize_enabled: bool,
    drag_state: RwSignal<Option<(ColumnId, f64, f64)>>,
    column_reorder_enabled: bool,
    drag_col_id: RwSignal<Option<ColumnId>>,
    sticky_css: &str,
    sticky_header: bool,
) -> AnyView {
    let w = col_width_style(override_width, col.initial_width);
    let align = alignment_css(col.alignment);
    let header = col.header.clone();
    let col_id = col.id;
    let is_sortable = sort_enabled && col.sortable;
    let initial_width = col.initial_width;

    let sort_entry = current_sort
        .iter()
        .enumerate()
        .find(|(_, s)| s.column == col_id);
    let sort_arrow = if is_sortable {
        match sort_entry.map(|(_, s)| s.direction) {
            Some(SortDirection::Asc) => " \u{2191}",
            Some(SortDirection::Desc) => " \u{2193}",
            None => "",
        }
    } else {
        ""
    };
    let sort_badge = if is_sortable && current_sort.len() > 1 {
        sort_entry.map(|(pos, _)| format!("{}", pos + 1))
    } else {
        None
    };

    let sort_cursor = if is_sortable { "pointer" } else { "default" };
    let drag_cursor = if column_reorder_enabled {
        "grab"
    } else {
        sort_cursor
    };
    let is_drag_over =
        column_reorder_enabled && drag_col_id.get_untracked().is_some_and(|id| id != col_id);
    let drag_over_style = if is_drag_over {
        "outline: 2px dashed var(--chorale-accent, #4a90e2); outline-offset: -2px; "
    } else {
        ""
    };

    // Emit explicit values in both branches so Leptos reactive attr diff
    // always performs a concrete swap rather than dropping the declaration.
    // position:sticky also acts as a containing block for the absolute resize
    // span; position:relative ensures the same in the non-sticky case.
    let sticky_top_decl = if sticky_header {
        "position:sticky;top:0;z-index:1;"
    } else {
        "position:relative;top:auto;z-index:auto;"
    };
    let sticky_css = sticky_css.to_owned();
    view! {
        <th
            style=format!(
                "cursor:{drag_cursor};padding:0.5rem 1rem;border-bottom:1px solid var(--chorale-border, #ddd);\
                 text-align:{align};white-space:nowrap;overflow:hidden;\
                 text-overflow:ellipsis;background:var(--chorale-header-bg, #f8f9fa);\
                 {sticky_top_decl}{w}{sticky_css}{drag_over_style}"
            )
            draggable=if column_reorder_enabled { "true" } else { "false" }
            on:click=move |ev| {
                if is_sortable {
                    let action = if ev.shift_key() {
                        SortAction::Append
                    } else {
                        SortAction::Replace
                    };
                    handle.toggle_sort(col_id, action);
                }
            }
            on:dragstart=move |ev| {
                if column_reorder_enabled {
                    // Some browsers (notably Firefox) refuse to initiate an
                    // HTML5 drag unless dataTransfer.setData() is called in
                    // dragstart. The payload is benign; the real source
                    // column travels via the `drag_col_id` signal. web-sys is
                    // a wasm32-only dependency, so the DataTransfer call is
                    // gated to that target (matching the rest of this file).
                    #[cfg(target_arch = "wasm32")]
                    if let Some(dt) = ev.data_transfer() {
                        let _ = dt.set_data("text/plain", col_id.0);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    let _ = &ev;
                    drag_col_id.set(Some(col_id));
                }
            }
            on:dragover=move |ev| {
                if column_reorder_enabled {
                    ev.prevent_default();
                }
            }
            on:drop=move |ev| {
                ev.prevent_default();
                if column_reorder_enabled {
                    if let Some(src_id) = drag_col_id.get_untracked() {
                        if src_id != col_id {
                            // `s.column_order` defaults to EMPTY (the
                            // renderer falls back to definition order when
                            // empty), so the drop target's index must be
                            // resolved against the same effective order the
                            // renderer uses — searching the raw vec found
                            // nothing and made every drop a no-op.
                            let to_idx = handle.signal().with_untracked(|s| {
                                if s.column_order.is_empty() {
                                    s.columns.iter().position(|c| c.id == col_id)
                                } else {
                                    s.column_order.iter().position(|&id| id == col_id)
                                }
                            });
                            if let Some(to_idx) = to_idx {
                                // Core `move_column` initializes an empty
                                // `column_order` from definition order,
                                // removes `src_id`, and inserts at `to_idx`
                                // (clamped) — landing the dragged column at
                                // the drop target's original visual slot in
                                // both drag directions.
                                handle.move_column(src_id, to_idx).ok();
                            }
                        }
                        drag_col_id.set(None);
                    }
                }
            }
        >
            {header.clone()}
            {sort_arrow.to_owned()}
            {sort_badge.map(|b| view! {
                <sup style="font-size:0.65rem;color:var(--chorale-accent, #4a90e2);margin-left:0.1rem;">{b}</sup>
            })}
            {if resize_enabled {
                Some(view! {
                    <span
                        style="position:absolute;right:0;top:0;bottom:0;width:4px;\
                               cursor:col-resize;background:transparent;"
                        on:mousedown=move |ev| {
                            let start_x = f64::from(ev.client_x());
                            let start_w = initial_width.unwrap_or(150.0);
                            drag_state.set(Some((col_id, start_x, start_w)));
                            ev.prevent_default();
                        }
                        on:dblclick=move |_| handle.reset_column_width(col_id)
                    />
                })
            } else {
                None
            }}
        </th>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Filter th
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn filter_th<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col: &ColumnDef<TRow>,
    override_width: Option<f64>,
    handle: UseTableHandle<TRow>,
    filters: &HashMap<ColumnId, FilterValue>,
    labels: &Labels,
    sticky_css: &str,
    open_filter_col: RwSignal<Option<ColumnId>>,
) -> AnyView {
    let w = col_width_style(override_width, col.initial_width);
    let col_id = col.id;
    let current = filters.get(&col_id);
    let sticky_css = sticky_css.to_owned();

    let inner = match &col.filter {
        FilterKind::None => view! { <span /> }.into_any(),
        FilterKind::Text => text_filter_input(
            col_id,
            current,
            handle,
            &labels.filter_placeholder,
            &labels.clear_filter_label,
        ),
        FilterKind::Boolean => boolean_filter_input(col_id, current, handle),
        FilterKind::NumericRange { min, max, step } => numeric_range_filter(
            col_id,
            *min,
            *max,
            *step,
            current,
            handle,
            &labels.clear_filter_label,
        ),
        FilterKind::DateRange => {
            date_range_filter(col_id, current, handle, &labels.clear_filter_label)
        }
        FilterKind::MultiSelect { options } => {
            let opts: Vec<&str> = options.iter().map(String::as_str).collect();
            multiselect_filter(
                col_id,
                &opts,
                current,
                handle,
                &labels.clear_filter_label,
                open_filter_col,
            )
        }
        _ => view! { <span /> }.into_any(),
    };

    view! {
        <th
            style=format!(
                "padding:0.25rem;border-bottom:1px solid var(--chorale-divider, #eee);position:sticky;top:0;z-index:3;\
                 background:var(--chorale-surface, #fff);{w}{sticky_css}"
            )
            // Keystrokes inside any filter widget (text input, range slider,
            // date input, multi-select) must not bubble to the table-root
            // keydown handler — Enter would start a cell edit and arrow keys
            // would move the active cell instead of the caret/slider.
            on:keydown=|ev: leptos::ev::KeyboardEvent| ev.stop_propagation()
        >
            {inner}
        </th>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Data row rendering
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn data_td<TRow: Clone + PartialEq + Send + Sync + 'static>(
    row: &TRow,
    row_id: RowId,
    col: &ColumnDef<TRow>,
    override_width: Option<f64>,
    row_height: f64,
    variable_row_height: bool,
    cell_renderers: &CellRenderers,
    row_cell_renderers: &RowCellRenderers<TRow>,
    editing_col: Option<ColumnId>,
    editing_text: RwSignal<String>,
    edit_error: RwSignal<Option<String>>,
    validate_fn: &ValidateEditFn,
    on_commit_edit: Option<&Callback<CommittedEdit<TRow>>>,
    sticky_css: &str,
    is_active_cell: bool,
    is_in_range: bool,
    row_index: usize,
    handle: UseTableHandle<TRow>,
    is_focus_cell: bool,
    fill_drag_active: RwSignal<bool>,
    fill_hover: RwSignal<Option<(usize, ColumnId)>>,
    on_row_click: Option<Callback<RowId>>,
    // The table's tabindex="0" keyboard container. Editors refocus it after a
    // keyboard-initiated commit/cancel so arrow-key navigation resumes
    // immediately (otherwise focus falls to <body> when the editor unmounts).
    kb_ref: NodeRef<html::Div>,
) -> AnyView {
    let val = (col.accessor)(row);
    let col_id = col.id;
    let align = alignment_css(col.alignment);
    let w = col_width_style(override_width, col.initial_width);
    let sticky_css = sticky_css.to_owned();
    // VIRT-2: with variable_row_height on, cells drop their fixed height
    // (and the display td additionally drops nowrap/ellipsis clipping) so
    // content can wrap and the row can grow to its natural height; the
    // measurement loop then records the real height in state.row_heights.
    // Mirrors the dioxus data_td / editor_td style branches.
    let editor_h_css = if variable_row_height {
        String::new()
    } else {
        format!("height:{row_height}px;")
    };
    let is_editing = editing_col == Some(col_id);
    let render_kind = col.render_kind.clone();
    let renderer = cell_renderers.get(col_id);
    let row_renderer = row_cell_renderers.get(col_id);
    let validate_fn = validate_fn.clone();
    let on_commit_edit_cb = on_commit_edit.copied();
    let row_clone = row.clone();

    // Active cell outline and range background (placed after sticky_css to override frozen bg).
    // The active cell gets the same light-blue fill as range cells in addition
    // to the outline — mirrors the Dioxus adapter, where the range overlay
    // (var(--chorale-range-bg, rgba(0, 120, 212, 0.1))) renders under the active-cell outline overlay.
    let active_css = if is_active_cell {
        "outline:2px solid var(--chorale-active-cell-outline,#0078d4);outline-offset:-2px;\
         background:var(--chorale-range-bg, rgba(0, 120, 212, 0.1));"
    } else {
        ""
    };
    let range_css = if is_in_range && !is_active_cell {
        "background:var(--chorale-range-bg, rgba(0, 120, 212, 0.1));"
    } else {
        ""
    };

    if is_editing {
        if let Some(EditorKind::Text) = &col.editor {
            {
                let input_ref: NodeRef<html::Input> = NodeRef::new();
                Effect::new(move |_| {
                    if let Some(el) = input_ref.get() {
                        let _ = el.focus();
                    }
                });
                // Separate clones for the blur closure; the keydown closure
                // takes ownership of the originals.
                let validate_blur = validate_fn.clone();
                let row_blur = row_clone.clone();
                return view! {
                    <td style=format!(
                        "padding:0;border-bottom:1px solid var(--chorale-divider, #eee);\
                         text-align:{align};{editor_h_css}\
                         overflow:hidden;{w}{sticky_css}"
                    )>
                        <div style="display:flex;flex-direction:column;height:100%;">
                            <input
                                type="text"
                                node_ref=input_ref
                                value=move || editing_text.get()
                                // Inherit the cell font and mirror the display
                                // td's text-align + horizontal padding
                                // (0.5rem 1rem) so the text does not shrink,
                                // shift, or re-justify when entering edit mode.
                                style=format!(
                                    "flex:1;width:100%;box-sizing:border-box;\
                                     padding:0.5rem 1rem;border:none;\
                                     outline:2px solid var(--chorale-accent, #4a90e2);outline-offset:-2px;\
                                     font-size:inherit;font-family:inherit;\
                                     line-height:inherit;text-align:{align};\
                                     background:transparent;"
                                )
                                on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                                on:mousedown=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                                on:input=move |ev| {
                                    editing_text.set(event_target_value(&ev));
                                    edit_error.set(None);
                                }
                                // Commit on blur: clicking away or into another
                                // cell must commit and exit edit mode (same
                                // validate -> on_commit_edit -> commit_edit
                                // path as the Enter branch below). Mirrors the
                                // dioxus editor_td onblur.
                                on:blur=move |_| {
                                    // Guard: blur can fire after Escape/Enter
                                    // already cleared the edit; bail so we
                                    // don't double-commit a stale value.
                                    let still_editing = handle
                                        .signal
                                        .try_with_untracked(|s| {
                                            s.editing.as_ref().is_some_and(|t| {
                                                t.row_id == row_id
                                                    && t.column_id == col_id
                                            })
                                        })
                                        .unwrap_or(false);
                                    if !still_editing {
                                        return;
                                    }
                                    let text = editing_text.get_untracked();
                                    let validation = EditValidation {
                                        row_id,
                                        column_id: col_id,
                                        raw_value: text.clone(),
                                    };
                                    if validate_blur.call(validation).is_ok() {
                                        edit_error.set(None);
                                        if let Some(cb) = on_commit_edit_cb.as_ref() {
                                            cb.run(CommittedEdit::new(
                                                row_id,
                                                col_id,
                                                text,
                                                row_blur.clone(),
                                            ));
                                        }
                                        let ns = handle
                                            .signal
                                            .with_untracked(|s| commit_edit(s));
                                        handle.signal.set(ns);
                                    }
                                    // On validation error, leave editing open
                                    // so the user can fix the value.
                                }
                                on:keydown=move |ev| {
                                    // While editing, no key may leak to the
                                    // table-level keydown handler (it would
                                    // re-enter edit mode on Enter or move the
                                    // active cell on arrows).
                                    ev.stop_propagation();
                                    let key = ev.key();
                                    if key == "Escape" {
                                        let ns = handle.signal.with_untracked(|s| cancel_edit(s));
                                        handle.signal.set(ns);
                                        // Return focus to the table's keyboard
                                        // container: the <input> is about to
                                        // unmount, and without this focus falls
                                        // to <body>, so arrow keys scroll the
                                        // page instead of navigating cells.
                                        if let Some(el) = kb_ref.get() {
                                            let _ = el.focus();
                                        }
                                    } else if key == "Enter" || key == "Tab" {
                                        let text = editing_text.get_untracked();
                                        let validation = EditValidation {
                                            row_id,
                                            column_id: col_id,
                                            raw_value: text.clone(),
                                        };
                                        match validate_fn.call(validation) {
                                            Ok(()) => {
                                                edit_error.set(None);
                                                if let Some(cb) = on_commit_edit_cb.as_ref() {
                                                    cb.run(CommittedEdit::new(
                                                        row_id,
                                                        col_id,
                                                        text.clone(),
                                                        row_clone.clone(),
                                                    ));
                                                }
                                                let ns = handle.signal.with_untracked(|s| commit_edit(s));
                                                handle.signal.set(ns);
                                                // Successful keyboard commit:
                                                // refocus the table container
                                                // so navigation continues from
                                                // the (still-set) active cell.
                                                // Deliberately NOT done in
                                                // on:blur — blur is an
                                                // intentional click-away and
                                                // must not steal focus back.
                                                if let Some(el) = kb_ref.get() {
                                                    let _ = el.focus();
                                                }
                                            }
                                            Err(msg) => {
                                                edit_error.set(Some(msg));
                                                ev.prevent_default();
                                            }
                                        }
                                    }
                                }
                            />
                            {move || edit_error.get().map(|e| view! {
                                <span style="font-size:0.7rem;color:var(--chorale-error, #dc2626);padding:0 0.25rem;">
                                    {e}
                                </span>
                            })}
                        </div>
                    </td>
                }
                .into_any();
            }
        }
        // Select editor: native <select> constrained to the column's options.
        if let Some(EditorKind::Select { options }) = &col.editor {
            let options = options.clone();
            // Auto-focus the <select> on mount, mirroring the Text editor's
            // input auto-focus above. This is load-bearing for exit-on-blur:
            // an unfocused element can never fire on:blur, so without this
            // clicking away / Tab left the cell stuck in edit mode, and the
            // keyboard could not drive the select at all.
            let select_ref: NodeRef<html::Select> = NodeRef::new();
            Effect::new(move |_| {
                if let Some(el) = select_ref.get() {
                    let _ = el.focus();
                }
            });
            // Separate clones for the blur closure; the change closure takes
            // ownership of the originals.
            let validate_blur = validate_fn.clone();
            let row_blur = row_clone.clone();
            return view! {
                <td style=format!(
                    "padding:0;border-bottom:1px solid var(--chorale-divider, #eee);\
                     text-align:{align};{editor_h_css}\
                     overflow:hidden;{w}{sticky_css}"
                )>
                    <div style="display:flex;flex-direction:column;height:100%;">
                        <select
                            node_ref=select_ref
                            prop:value=move || editing_text.get()
                            // Inherit the cell font and mirror the display
                            // td's text-align + horizontal padding so the
                            // text does not shrink or shift in edit mode.
                            style=format!(
                                "flex:1;width:100%;box-sizing:border-box;\
                                 padding:0.5rem 1rem;border:none;\
                                 outline:2px solid var(--chorale-accent, #4a90e2);outline-offset:-2px;\
                                 font-size:inherit;font-family:inherit;\
                                 line-height:inherit;text-align:{align};\
                                 background:transparent;"
                            )
                            on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                            on:mousedown=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                            // While editing, no key may leak to the
                            // table-level keydown handler. Escape cancels,
                            // mirroring the dioxus select editor.
                            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                ev.stop_propagation();
                                if ev.key() == "Escape" {
                                    let ns = handle.signal.with_untracked(|s| cancel_edit(s));
                                    handle.signal.set(ns);
                                    // The <select> is about to unmount; return
                                    // focus to the table container so arrow
                                    // keys keep navigating cells instead of
                                    // scrolling the page.
                                    if let Some(el) = kb_ref.get() {
                                        let _ = el.focus();
                                    }
                                }
                            }
                            // A native <select> fires NO change event when the
                            // user re-picks the already-selected value, so
                            // without this the cell would stay stuck in edit
                            // mode. Blur always commits the current value and
                            // returns to display mode.
                            on:blur=move |_| {
                                let still_editing = handle
                                    .signal
                                    .try_with_untracked(|s| {
                                        s.editing.as_ref().is_some_and(|t| {
                                            t.row_id == row_id && t.column_id == col_id
                                        })
                                    })
                                    .unwrap_or(false);
                                if !still_editing {
                                    return;
                                }
                                let text = editing_text.get_untracked();
                                let validation = EditValidation {
                                    row_id,
                                    column_id: col_id,
                                    raw_value: text.clone(),
                                };
                                if validate_blur.call(validation).is_ok() {
                                    edit_error.set(None);
                                    if let Some(cb) = on_commit_edit_cb.as_ref() {
                                        cb.run(CommittedEdit::new(
                                            row_id,
                                            col_id,
                                            text,
                                            row_blur.clone(),
                                        ));
                                    }
                                    let ns = handle.signal.with_untracked(|s| commit_edit(s));
                                    handle.signal.set(ns);
                                }
                            }
                            on:change=move |ev| {
                                let text = event_target_value(&ev);
                                editing_text.set(text.clone());
                                edit_error.set(None);
                                if let Some(cb) = on_commit_edit_cb.as_ref() {
                                    cb.run(CommittedEdit::new(
                                        row_id,
                                        col_id,
                                        text,
                                        row_clone.clone(),
                                    ));
                                }
                                let ns = handle.signal.with_untracked(|s| commit_edit(s));
                                handle.signal.set(ns);
                                // Commit unmounts the <select>; refocus the
                                // table container so a keyboard pick (arrows +
                                // Enter) flows straight back into cell
                                // navigation. Deliberately NOT done in on:blur
                                // — blur is an intentional click-away and must
                                // not steal focus back.
                                if let Some(el) = kb_ref.get() {
                                    let _ = el.focus();
                                }
                            }
                        >
                            {options
                                .into_iter()
                                .map(|opt| {
                                    let label = opt.clone();
                                    view! { <option value=opt>{label}</option> }
                                })
                                .collect_view()}
                        </select>
                        {move || edit_error.get().map(|e| view! {
                            <span style="font-size:0.7rem;color:var(--chorale-error, #dc2626);padding:0 0.25rem;">
                                {e}
                            </span>
                        })}
                    </div>
                </td>
            }
            .into_any();
        }
    }

    let cell_content = resolve_cell_content(
        row,
        &val,
        &render_kind,
        row_renderer.as_ref(),
        renderer.as_ref(),
    );
    // Fixed-height display cells clip to one line (nowrap + ellipsis); with
    // variable_row_height the clamp is dropped entirely so content wraps and
    // the row grows. Mirrors the dioxus data_td variable/uniform branches.
    let clamp_css = if variable_row_height {
        String::new()
    } else {
        format!(
            "height:{row_height}px;overflow:hidden;\
             white-space:nowrap;text-overflow:ellipsis;"
        )
    };
    view! {
        <td
            style=format!(
                "padding:0.5rem 1rem;border-bottom:1px solid var(--chorale-divider, #eee);\
                 text-align:{align};{clamp_css}cursor:default;\
                 position:relative;\
                 {w}{sticky_css}{range_css}{active_css}"
            )
            on:click=move |ev: leptos::ev::MouseEvent| {
                let ctrl = ev.ctrl_key() || ev.meta_key();
                let shift = ev.shift_key();
                let new_s = if ctrl {
                    handle.signal.with_untracked(|s| add_disjoint_range(s, row_index, col_id))
                } else if shift {
                    handle.signal.with_untracked(|s| extend_range_to(s, row_index, col_id))
                } else {
                    handle.signal.with_untracked(|s| start_range_selection(s, row_index, col_id))
                };
                handle.signal.set(new_s);
                if should_fire_row_click(ctrl, shift) {
                    if let Some(cb) = on_row_click {
                        cb.run(row_id);
                    }
                }
                ev.stop_propagation();
            }
            on:dblclick=move |_| {
                handle.start_edit(row_id, col_id);
            }
            on:mouseenter=move |_| {
                if fill_drag_active.get_untracked() {
                    fill_hover.set(Some((row_index, col_id)));
                }
            }
        >
            {cell_content}
            {is_focus_cell.then(|| view! {
                <div
                    style="position: absolute; bottom: 0; right: 0; width: 6px; height: 6px; \
                           background: var(--chorale-active-cell-outline, #0078d4); cursor: crosshair; z-index: 10; \
                           pointer-events: none;"
                    on:mousedown=move |ev| {
                        ev.stop_propagation();
                        fill_drag_active.set(true);
                    }
                />
            })}
        </td>
    }
    .into_any()
}

#[allow(clippy::too_many_arguments)]
fn render_data_row<TRow: Clone + PartialEq + Send + Sync + 'static>(
    row: &TRow,
    row_id: RowId,
    row_index: usize,
    visible_cols: &[ColumnDef<TRow>],
    widths: &HashMap<ColumnId, f64>,
    row_height: f64,
    variable_row_height: bool,
    selection_enabled: bool,
    is_selected: bool,
    handle: UseTableHandle<TRow>,
    cell_renderers: &CellRenderers,
    row_cell_renderers: &RowCellRenderers<TRow>,
    editing_col: Option<ColumnId>,
    editing_text: RwSignal<String>,
    edit_error: RwSignal<Option<String>>,
    validate_fn: &ValidateEditFn,
    on_commit_edit: Option<&Callback<CommittedEdit<TRow>>>,
    sticky_body_css: &HashMap<ColumnId, String>,
    active_cell: Option<ActiveCell>,
    range_cells: &HashSet<(usize, ColumnId)>,
    fill_focus_cell: Option<(usize, ColumnId)>,
    fill_drag_active: RwSignal<bool>,
    fill_hover: RwSignal<Option<(usize, ColumnId)>>,
    has_detail: bool,
    is_expanded: bool,
    on_row_click: Option<Callback<RowId>>,
    kb_ref: NodeRef<html::Div>,
) -> AnyView {
    let bg = if is_selected {
        "var(--chorale-row-selected-bg, #eff6ff)"
    } else {
        "var(--chorale-surface, white)"
    };
    let cells: Vec<AnyView> = visible_cols
        .iter()
        .map(|col| {
            let is_active =
                active_cell.is_some_and(|ac| ac.row_idx == row_index && ac.column_id == col.id);
            let is_in_range = range_cells.contains(&(row_index, col.id));
            let is_focus_cell = fill_focus_cell == Some((row_index, col.id));
            data_td(
                row,
                row_id,
                col,
                widths.get(&col.id).copied(),
                row_height,
                variable_row_height,
                cell_renderers,
                row_cell_renderers,
                editing_col,
                editing_text,
                edit_error,
                validate_fn,
                on_commit_edit,
                sticky_body_css.get(&col.id).map_or("", String::as_str),
                is_active,
                is_in_range,
                row_index,
                handle,
                is_focus_cell,
                fill_drag_active,
                fill_hover,
                on_row_click,
                kb_ref,
            )
        })
        .collect();

    view! {
        <tr
            data-chorale-index=row_index.to_string()
            style=format!(
                "background:{bg};cursor:default;\
                 box-shadow:inset 0 -1px 0 var(--chorale-divider, #eee);"
            )
            on:click=move |_| {
                if selection_enabled {
                    handle.set_selection(row_id, !is_selected);
                }
            }
        >
            {if selection_enabled {
                Some(view! {
                    <td style="padding:0.5rem;border-bottom:1px solid var(--chorale-divider, #eee);width:2.5rem;">
                        <input
                            type="checkbox"
                            prop:checked=is_selected
                            on:click=move |ev| { ev.stop_propagation(); }
                            on:change=move |ev| {
                                handle.set_selection(row_id, event_target_checked(&ev));
                            }
                        />
                    </td>
                })
            } else {
                None
            }}
            {if has_detail {
                let chevron = if is_expanded { "▼" } else { "▶" };
                let aria = if is_expanded { "Collapse row" } else { "Expand row" };
                Some(view! {
                    <td
                        class="chorale-cell chorale-detail-chevron"
                        style="width:24px;cursor:pointer;user-select:none;text-align:center;\
                               border-bottom:1px solid var(--chorale-divider, #eee);"
                        aria-label=aria
                        on:click=move |ev| {
                            ev.stop_propagation();
                            handle.toggle_row_expansion(row_id);
                        }
                    >
                        {chevron}
                    </td>
                })
            } else {
                None
            }}
            {cells}
        </tr>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Group header row
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
fn render_group_header<TRow: Clone + PartialEq + Send + Sync + 'static>(
    key: GroupKey,
    label: String,
    depth: usize,
    row_count: usize,
    is_collapsed: bool,
    effective_col_count: usize,
    handle: UseTableHandle<TRow>,
    group_header_class: &str,
) -> AnyView {
    let indent = format!("{}rem", depth as f64 * 1.5);
    let icon = if is_collapsed { "\u{25b6}" } else { "\u{25bc}" };
    let group_header_class = group_header_class.to_owned();
    view! {
        <tr
            class=group_header_class
            style="background:var(--chorale-group-header-bg, #f0f4ff);cursor:pointer;"
            on:click=move |_| { handle.toggle_group(key.clone()); }
        >
            <td
                colspan=effective_col_count.to_string()
                style=format!(
                    "padding:0.4rem 1rem 0.4rem {indent};\
                     border-bottom:1px solid var(--chorale-group-header-border, #d1d5db);\
                     font-weight:600;font-size:0.875rem;"
                )
            >
                <span style="margin-right:0.5rem;">{icon}</span>
                {label}
                <span style="margin-left:0.5rem;font-size:0.75rem;\
                              font-weight:normal;color:var(--chorale-text-muted, #6b7280);">
                    {format!("({row_count})")}
                </span>
            </td>
        </tr>
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Sticky CSS computation for frozen columns
// ---------------------------------------------------------------------------

fn build_sticky_css<TRow: Clone>(
    state: &TableState<TRow>,
    frozen_z: i32,
) -> (HashMap<ColumnId, String>, HashMap<ColumnId, String>) {
    let widths = &state.column_widths;
    let left_frozen: Vec<ColumnDef<TRow>> =
        frozen_left_columns(state).into_iter().cloned().collect();
    let right_frozen: Vec<ColumnDef<TRow>> =
        frozen_right_columns(state).into_iter().cloned().collect();
    let header_z = frozen_z + 1;
    let body_z = frozen_z;

    let mut header_css: HashMap<ColumnId, String> = HashMap::new();
    let mut body_css: HashMap<ColumnId, String> = HashMap::new();

    let left_count = left_frozen.len();
    let mut left_off = 0.0f64;
    for (k, col) in left_frozen.iter().enumerate() {
        let w = widths
            .get(&col.id)
            .copied()
            .or(col.initial_width)
            .unwrap_or(150.0);
        let divider = if k + 1 == left_count {
            " box-shadow: var(--chorale-frozen-divider-shadow, 3px 0 4px -2px rgba(0,0,0,0.15));"
        } else {
            ""
        };
        header_css.insert(
            col.id,
            format!("position:sticky;left:{left_off}px;z-index:{header_z};{divider}"),
        );
        body_css.insert(
            col.id,
            format!(
                "position:sticky;left:{left_off}px;z-index:{body_z};\
                 background:var(--chorale-surface, #fff);{divider}"
            ),
        );
        left_off += w;
    }

    let mut right_off = 0.0f64;
    for (j, col) in right_frozen.iter().enumerate().rev() {
        let w = widths
            .get(&col.id)
            .copied()
            .or(col.initial_width)
            .unwrap_or(150.0);
        let divider = if j == 0 {
            " box-shadow: var(--chorale-frozen-divider-shadow, -3px 0 4px -2px rgba(0,0,0,0.15));"
        } else {
            ""
        };
        header_css.insert(
            col.id,
            format!("position:sticky;right:{right_off}px;z-index:{header_z};{divider}"),
        );
        body_css.insert(
            col.id,
            format!(
                "position:sticky;right:{right_off}px;z-index:{body_z};\
                 background:var(--chorale-surface, #fff);{divider}"
            ),
        );
        right_off += w;
    }

    (header_css, body_css)
}

// ---------------------------------------------------------------------------
// CSV download helper (WASM only)
// ---------------------------------------------------------------------------

/// Object-URL + anchor-click download, shared by the CSV and XLSX export
/// paths. Mirrors the Dioxus adapter's eval script exactly:
/// `URL.createObjectURL` → `<a download>` appended to `<body>` → `click()` →
/// remove → `URL.revokeObjectURL`.
///
/// Runs SYNCHRONOUSLY inside the click handler — the transient user
/// activation from the click must still be live when `a.click()` fires, or
/// the browser downgrades the download (e.g. forces a save-as / overwrite
/// prompt instead of silently auto-incrementing the filename). This is why
/// the old `spawn_local`-wrapped CSV path produced overwrite prompts while
/// the Dioxus adapter auto-incremented ("export (4).csv").
#[cfg(target_arch = "wasm32")]
fn trigger_blob_download(blob: &web_sys::Blob, filename: &str) {
    let Ok(url) = web_sys::Url::create_object_url_with_blob(blob) else {
        return;
    };
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Ok(anchor) = doc.create_element("a") {
        let anchor: web_sys::HtmlAnchorElement = anchor.unchecked_into();
        anchor.set_href(&url);
        anchor.set_download(filename);
        if let Some(body) = doc.body() {
            body.append_child(&anchor).ok();
            anchor.click();
            body.remove_child(&anchor).ok();
        }
    }
    web_sys::Url::revoke_object_url(&url).ok();
}

#[cfg(target_arch = "wasm32")]
fn trigger_csv_download(csv: &str) {
    let array = js_sys::Array::new();
    array.push(&wasm_bindgen::JsValue::from_str(csv));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("text/csv;charset=utf-8;");
    if let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(&array, &options) {
        trigger_blob_download(&blob, "chorale-export.csv");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn trigger_csv_download(_csv: &str) {}

/// Build a Blob from raw bytes and trigger a download. Used by the XLSX
/// export paths; matches the Dioxus adapter's `Uint8Array` → Blob →
/// object-URL mechanism (a `data:` URL here previously caused the browser to
/// reuse the filename with an overwrite prompt instead of auto-incrementing).
#[cfg(all(feature = "xlsx", target_arch = "wasm32"))]
fn trigger_xlsx_download(bytes: &[u8], filename: &str) {
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet");
    if let Ok(blob) = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options) {
        trigger_blob_download(&blob, filename);
    }
}

// ---------------------------------------------------------------------------
// The main Table component
// ---------------------------------------------------------------------------

/// The primary chorale Leptos table component.
///
/// Renders column headers, an optional filter row, virtualized data rows,
/// pagination controls, and optional selection checkboxes. All features are
/// opt-in via props; the minimal form shows a read-only sorted table.
///
/// ```rust,ignore
/// view! {
///     <Table
///         handle=handle
///         sort_enabled=true
///         filter_enabled=true
///         selection_enabled=true
///     />
/// }
/// ```
#[allow(
    clippy::too_many_lines,
    clippy::fn_params_excessive_bools,
    clippy::needless_pass_by_value
)]
#[component]
pub fn Table<TRow>(
    handle: UseTableHandle<TRow>,
    #[prop(default = true)] sort_enabled: bool,
    #[prop(default = false)] filter_enabled: bool,
    #[prop(default = false)] selection_enabled: bool,
    #[prop(default = CellRenderers::default())] cell_renderers: CellRenderers,
    /// Per-column custom renderers that receive the **full row** plus the
    /// cell value (`Fn(&TRow, &CellValue) -> AnyView`). Entries here take
    /// precedence over `cell_renderers` and the column's `RenderKind`.
    #[prop(default = RowCellRenderers::default())]
    row_cell_renderers: RowCellRenderers<TRow>,
    #[prop(default = false)] column_toolbar: bool,
    #[prop(default = false)] csv_export: bool,
    #[prop(default = false)] xlsx_export: bool,
    #[prop(default = false)] resize_enabled: bool,
    #[prop(default = ValidateEditFn::default())] validate_edit: ValidateEditFn,
    on_commit_edit: Option<Callback<CommittedEdit<TRow>>>,
    /// Fired with the row's [`RowId`] when a data row receives a plain
    /// (unmodified) left-click on one of its data cells. Not fired for the
    /// selection checkbox, the detail-expander chevron, in-edit cells,
    /// group headers, or Ctrl/Cmd/Shift-modified clicks (range-selection
    /// operations). Default `None` = no behavior change.
    #[prop(optional)]
    on_row_click: Option<Callback<RowId>>,
    /// Fired when Tab key moves focus to a cell whose column has `EditorKind` configured.
    #[prop(optional)]
    on_tab_to_editable: Option<Callback<ActiveCell>>,
    /// Fired after Ctrl+C successfully writes the selected range to the system clipboard.
    #[prop(optional)]
    on_copy: Option<Callback<ClipboardCopyEvent>>,
    /// Fired after Ctrl+V reads from the system clipboard and adjusts the active range.
    /// The host should apply the per-cell writes from `evt.tsv` via its persistence layer.
    #[prop(optional)]
    on_paste: Option<Callback<ClipboardPasteEvent>>,
    #[prop(optional)] selection_toolbar: Option<ChildrenFn>,
    /// Optional per-row detail renderer. When `Some`, a 24px chevron column is
    /// prepended; clicking it calls `toggle_row_expansion`. `RenderRow::DetailPanel`
    /// rows render as `<tr><td colspan>` containing the returned `AnyView`.
    ///
    /// Takes the full `Option` (`optional_no_strip`) so consumers can wire a
    /// conditional renderer directly, e.g.
    /// `detail_renderer=toggle.get().then(|| renderer.clone())` — mirroring
    /// the `Option`-typed prop on the Dioxus adapter.
    ///
    /// Per CHANGELOG Item N (master/detail, MD-B).
    #[prop(optional_no_strip)]
    detail_renderer: Option<DetailRenderer<TRow>>,
    #[prop(optional)] labels: Option<Labels>,
    #[prop(default = false)] column_reorder_enabled: bool,
    /// When `true` (default), the header row sticks to the top of the scroll
    /// container. Set `false` to let it scroll with the body.
    #[prop(default = true)]
    sticky_header: bool,
    /// Enable variable-row-height virtualization (VIRT-2). When `true`, the
    /// component measures each rendered data row's real height after render
    /// via the DOM (web target only) and caches it in `state.row_heights`;
    /// the non-grouped window and spacer math then run on the measured
    /// heights via [`visible_window_variable`], with `state.row_height` as
    /// the fallback estimate for rows not yet measured. Data cells also drop
    /// their fixed `height`/`nowrap` styling so content can wrap and grow.
    /// Forced on automatically whenever `detail_renderer` is set — detail
    /// panels are inherently variable-height. Mirrors the Dioxus adapter's
    /// prop of the same name.
    #[prop(default = false)]
    variable_row_height: bool,
    /// **Inline mode** (default `false`). When `true`, the `<Table>` does NOT
    /// render its own scroll container — the body renders at its natural full
    /// height and any overflow is handled by the parent's scroll context.
    /// Virtualization is disabled (all visible rows render in one batch).
    ///
    /// Use this when embedding a `<Table>` inside an outer scrolling element
    /// where a nested scroll context would otherwise produce wheel-event
    /// hand-off discontinuities (master/detail panels, sidebars, modals).
    /// The consumer should keep the dataset small enough that rendering every
    /// row at once is acceptable (typically <500 rows).
    #[prop(default = false)]
    inline: bool,
    /// CSS `z-index` applied to frozen column cells.
    #[prop(default = 2)]
    frozen_column_z_index: i32,
    /// CSS class applied to every group-header `<tr>`.
    #[prop(default = String::from("chorale-group-header"))]
    group_header_class: String,
    /// Distance from the scroll container bottom (px) at which to fire
    /// `load_more_rows` in `PaginationMode::InfiniteScroll`. Default is `200`.
    #[prop(default = 200.0)]
    infinite_scroll_threshold_px: f64,
    /// Visual theme applied to this table. `Theme::Light` (the default) and
    /// `Theme::Dark` resolve against the built-in injected stylesheet;
    /// `Theme::Custom` matches no built-in token block, so every
    /// `var(--chorale-*, <fallback>)` reference resolves to the consumer's own
    /// CSS-variable definitions or the inline light fallback. Mirrors the
    /// chorale-dioxus adapter.
    #[prop(default = Theme::Light)]
    theme: Theme,
) -> impl IntoView
where
    TRow: Clone + PartialEq + Send + Sync + 'static,
{
    let labels = Arc::new(labels.unwrap_or_default());

    // Master/detail rows are inherently variable-height: the parent table
    // cannot virtualize correctly assuming uniform row_height when one of
    // its rows is a detail panel that's 5-20× taller. Force variable-height
    // measurement on whenever detail_renderer is set, so the parent's
    // row_heights map tracks each row's actual rendered height and scroll
    // math stays consistent with layout. Mirrors chorale-dioxus.
    //
    // This shadow MUST come before the VIRT-2 measurement effect below and
    // before any downstream consumer of `variable_row_height`.
    let variable_row_height = variable_row_height || detail_renderer.is_some();

    // on_copy is only used inside #[cfg(target_arch = "wasm32")] blocks.
    // on_paste is also used by the fill handle drag (pure Rust, no cfg guard needed).
    // Silence unused-variable warnings on non-WASM targets.
    #[cfg(not(target_arch = "wasm32"))]
    let _ = on_copy;

    let drag_state: RwSignal<Option<(ColumnId, f64, f64)>> = RwSignal::new(None);
    let drag_col_id: RwSignal<Option<ColumnId>> = RwSignal::new(None);
    let editing_text: RwSignal<String> = RwSignal::new(String::new());
    let edit_error: RwSignal<Option<String>> = RwSignal::new(None);
    let fill_drag_active: RwSignal<bool> = RwSignal::new(false);
    let fill_hover: RwSignal<Option<(usize, ColumnId)>> = RwSignal::new(None);
    // Which column's multi-select filter dropdown is open. Lives in the
    // Table body (stable scope) rather than inside multiselect_filter so the
    // open state survives the filter-driven re-render that happens on every
    // checkbox pick; a per-render RwSignal would reset to closed each time.
    let open_filter_col: RwSignal<Option<ColumnId>> = RwSignal::new(None);

    let sig = handle.signal;

    // PERF-1: two-level memo — decouple expensive pipeline from scroll events.
    let view_key = Memo::new(move |_| {
        sig.with(|s| {
            (
                s.page,
                s.page_size,
                s.loaded_row_count,
                s.sort.clone(),
                s.filters.clone(),
                s.rows.len(),
                s.grouping.clone(),
                s.collapsed_groups.clone(),
                s.expanded_rows.clone(),
                // data_generation bumps every time update_row mutates row
                // content. Without this in the tuple, cell edits land in
                // state.rows but the memo's PartialEq short-circuits and
                // the cached visible_view is returned forever, so the cell
                // keeps rendering the pre-edit value until an unrelated
                // transition happens to bump some other field. Mirrors the
                // dioxus adapter's view_key.
                s.data_generation,
                // column_order changes on drag-and-drop column reorder; the
                // header/body render path reads it through this memo chain,
                // so it must invalidate the key too.
                s.column_order.clone(),
            )
        })
    });
    let visible = Memo::new(move |_| {
        let _ = view_key.get();
        sig.with_untracked(|s| visible_view(s))
    });
    let grouped_visible = Memo::new(move |_| {
        let _ = view_key.get();
        sig.with_untracked(|s| visible_grouped_view(s))
    });

    // Scroll container NodeRef for programmatic scrollTop reset on page change.
    let scroll_ref: NodeRef<html::Div> = NodeRef::new();
    let page_memo = Memo::new(move |_| sig.with(|s| s.page));
    Effect::new(move |_| {
        let _ = page_memo.get();
        if let Some(el) = scroll_ref.get() {
            el.set_scroll_top(0);
        }
    });

    // VIRT-2: variable-row-height measurement loop. The whole block is gated
    // behind wasm32 — the DOM reads below are web_sys calls and web-sys is a
    // wasm32-only dependency of this crate, so any un-gated browser call
    // breaks the host build. On the host, `variable_row_height` still drives
    // the (pure-Rust) variable window math; row_heights simply stays empty.
    //
    // `meas_trigger` is the reactive dependency that drives remeasurement:
    // scroll bucket (one re-measure per row_height of scrolling), viewport
    // size, expanded_rows (detail panels mount/unmount), data_generation
    // (cell edits can change wrap height), row count, page, and page size.
    // Mirrors the Dioxus adapter's VIRT-2 trigger memo field-for-field.
    #[cfg(target_arch = "wasm32")]
    {
        let meas_trigger = Memo::new(move |_| {
            sig.with(|s| {
                let row_h = if s.row_height > 0.0 {
                    s.row_height
                } else {
                    1.0
                };
                #[allow(clippy::cast_possible_truncation)]
                let scroll_bucket = (s.scroll_top / row_h).floor() as i64;
                #[allow(clippy::cast_possible_truncation)]
                let viewport_bucket = s.viewport_height.round() as i64;
                (
                    scroll_bucket,
                    viewport_bucket,
                    s.expanded_rows.clone(),
                    s.data_generation,
                    s.rows.len(),
                    s.page,
                    s.page_size,
                )
            })
        });
        Effect::new(move |_| {
            // Subscribe to the trigger BEFORE the early-return so the
            // dependency is registered on the very first run either way.
            let _ = meas_trigger.get();
            if !variable_row_height {
                return;
            }
            // Leptos runs user effects after the render effects for the same
            // signal change have applied their DOM updates, so the rows this
            // trigger change produced are already in the document, and
            // getBoundingClientRect (which forces layout) returns real
            // post-layout heights.
            let Some(container) = scroll_ref.get_untracked() else {
                return;
            };
            // Direct-child chain rather than a descendant selector:
            // `:scope > table > tbody > tr[data-chorale-index]`. When a
            // consumer renders a nested Table inside a detail panel, the
            // descendant form (`[data-chorale-index]` anywhere below) also
            // matches the child Table's rows, producing duplicate
            // `data-chorale-index` keys (child 0,1,2... collide with parent
            // 0,1,2...) that would corrupt the parent's row_heights map. The
            // direct-child chain stops at the parent's own tbody. `:scope`
            // anchors the selector at the scroll container itself — the same
            // role Dioxus's `#{scroll_id}` element id plays.
            let Ok(node_list) =
                container.query_selector_all(":scope > table > tbody > tr[data-chorale-index]")
            else {
                return;
            };
            let mut measurements: HashMap<usize, f64> = HashMap::new();
            for i in 0..node_list.length() {
                let Some(node) = node_list.item(i) else {
                    continue;
                };
                let Ok(el) = node.dyn_into::<web_sys::Element>() else {
                    continue;
                };
                let Some(idx) = el
                    .get_attribute("data-chorale-index")
                    .and_then(|a| a.parse::<usize>().ok())
                else {
                    continue;
                };
                measurements.insert(idx, el.get_bounding_client_rect().height());
            }
            if measurements.is_empty() {
                return;
            }
            let total = visible.with_untracked(Vec::len);
            // Compute the diff check, scroll anchoring, and the new state in
            // one untracked read. try_* accessors make a stale fire after the
            // table subtree is disposed a silent no-op instead of a panic.
            let Some(update) = sig.try_with_untracked(|cur| {
                // 0.5px diff threshold: prevents convergence loops caused by
                // sub-pixel float rounding (matches chorale-dioxus).
                let mut any_changed = false;
                for (k, v) in &measurements {
                    let old = cur.row_heights.get(k).copied().unwrap_or(cur.row_height);
                    if (v - old).abs() > 0.5 {
                        any_changed = true;
                        break;
                    }
                }
                if !any_changed {
                    return None;
                }
                // Scroll anchoring: when a row's measured height changes
                // (typically a freshly-rendered detail panel going from
                // estimate → real), the total height of all rows ABOVE the
                // current scroll position can change too. Applying the new
                // measurements without adjusting scroll_top shifts visible
                // content by the delta — the user sees that as a jump. Sum
                // the per-row height delta for every row whose top edge sits
                // above cur.scroll_top, bump scroll_top by that sum, and
                // write it back to the DOM scroll container so the visible
                // content stays visually anchored. Math is identical to the
                // chorale-dioxus VIRT-2 anchoring loop.
                let cur_scroll = cur.scroll_top;
                let default_h = cur.row_height;
                let mut row_top_with_old = 0.0_f64;
                let mut scroll_delta = 0.0_f64;
                for idx in 0..total {
                    let old_h = cur.row_heights.get(&idx).copied().unwrap_or(default_h);
                    if row_top_with_old >= cur_scroll {
                        break;
                    }
                    if let Some(new_h) = measurements.get(&idx).copied() {
                        let bounded_old = old_h.max(0.0);
                        let bounded_new = new_h.max(0.0);
                        scroll_delta += bounded_new - bounded_old;
                    }
                    row_top_with_old += old_h.max(0.0);
                }
                let new_scroll = (cur_scroll + scroll_delta).max(0.0);
                let mut new_state = batch_record_row_heights(cur, &measurements);
                new_state.scroll_top = new_scroll;
                Some((new_state, new_scroll, scroll_delta))
            }) else {
                return;
            };
            if let Some((new_state, new_scroll, scroll_delta)) = update {
                let _ = sig.try_set(new_state);
                if scroll_delta.abs() > 0.5 {
                    #[allow(clippy::cast_possible_truncation)]
                    container.set_scroll_top(new_scroll.round() as i32);
                }
            }
        });
    }

    // In-cell editing: reset editor text and error when active cell changes.
    let edit_target_memo = Memo::new(move |_| sig.with(|s| s.editing));
    Effect::new(move |_| {
        let target = edit_target_memo.get();
        if let Some(target) = target {
            let init_text = sig.with_untracked(|state| {
                state
                    .columns
                    .iter()
                    .find(|c| c.id == target.column_id)
                    .and_then(|col| {
                        state
                            .rows
                            .iter()
                            .find(|(id, _)| *id == target.row_id)
                            .map(|(_, row)| (col.accessor)(row).to_csv_string())
                    })
                    .unwrap_or_default()
            });
            editing_text.set(init_text);
            edit_error.set(None);
        }
    });

    let kb_ref: NodeRef<html::Div> = NodeRef::new();

    #[cfg(target_arch = "wasm32")]
    {
        // Both window listeners below are registered with on_cleanup and use
        // try_* accessors so a stale fire after the table subtree is disposed
        // (e.g. a QA-harness feature toggle rebuilding the component) is a
        // silent no-op instead of a reactive_graph "unreachable" panic.
        let mousedown_handle = window_event_listener(leptos::ev::mousedown, move |ev| {
            // NodeRef wraps a render-scoped signal: try_get returns None once
            // the owning scope is disposed — bail out, the table is gone.
            let Some(node_opt) = kb_ref.try_get() else {
                return;
            };
            let inside = node_opt.is_some_and(|node| {
                ev.target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                    .is_some_and(|el| node.contains(Some(&el)))
            });
            if !inside {
                let Some(editing) = sig.try_with_untracked(|s| s.editing.is_some()) else {
                    return;
                };
                if !editing {
                    let Some(new_s) = sig.try_with_untracked(|s| {
                        let s2 = clear_range_selection(s);
                        clear_active_cell(&s2)
                    }) else {
                        return;
                    };
                    sig.try_set(new_s);
                }
            }
        });
        on_cleanup(move || mousedown_handle.remove());

        // Single table-scoped outside-click closer for the multi-select
        // filter dropdowns, registered in the CAPTURE phase on `document`.
        //
        // Capture is load-bearing: data cells (and several other table
        // regions) call `stop_propagation()` in their bubble-phase click
        // handlers, so a bubble-phase window listener never hears clicks on
        // them and the dropdown stays stuck open when you click a cell. A
        // capture-phase listener runs on the way DOWN (document -> target)
        // before any bubble-phase `stop_propagation()` can fire, so every
        // click in the page reaches it. `window_event_listener` does not
        // expose capture options, hence the raw web_sys registration.
        //
        // Two click locations are exempt from the unconditional close:
        //   - inside `.chorale-filter-dropdown`: picking checkboxes or
        //     grabbing the dropdown scrollbar must keep it open;
        //   - inside `.chorale-filter-toggle`: the toggle button's own
        //     bubble-phase handler owns open/close/switch semantics. If
        //     capture closed first, the same-column toggle would observe
        //     `None` and re-open — turning "click toggle to close" into a
        //     no-op. (Clicking ANOTHER column's toggle still closes this
        //     dropdown: that handler sets `open_filter_col` to the other
        //     column, which unmounts this one's `<Show>`.)
        //
        // Cleanup mirrors the listener above: the closure is moved into
        // `on_cleanup` so it outlives the registration, and the handler
        // body uses `try_*` signal accessors so a stale fire after the
        // table subtree is disposed is a silent no-op, never a panic.
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let filter_close_cb = wasm_bindgen::closure::Closure::<dyn Fn(web_sys::Event)>::new(
                move |ev: web_sys::Event| {
                    let Some(is_open) = open_filter_col.try_get_untracked() else {
                        return;
                    };
                    if is_open.is_none() {
                        return;
                    }
                    let exempt = ev
                        .target()
                        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                        .is_some_and(|el| {
                            el.closest(".chorale-filter-dropdown, .chorale-filter-toggle")
                                .ok()
                                .flatten()
                                .is_some()
                        });
                    if !exempt {
                        open_filter_col.try_set(None);
                    }
                },
            );
            let registered = doc
                .add_event_listener_with_callback_and_bool(
                    "click",
                    filter_close_cb.as_ref().unchecked_ref(),
                    true, // capture phase
                )
                .is_ok();
            // `on_cleanup` requires Send + Sync, but JS handles (Closure,
            // Document) are neither. SendWrapper is sound here because wasm
            // is single-threaded — the same trick leptos uses internally.
            let cleanup_ctx = send_wrapper::SendWrapper::new((doc, filter_close_cb));
            on_cleanup(move || {
                let (doc, filter_close_cb) = &*cleanup_ctx;
                if registered {
                    let _ = doc.remove_event_listener_with_callback_and_bool(
                        "click",
                        filter_close_cb.as_ref().unchecked_ref(),
                        true,
                    );
                }
                // `filter_close_cb` is dropped here, releasing the JS shim.
            });
        }
    }

    view! {
        // Ship the built-in light+dark token stylesheet inline (matches the
        // dioxus adapter). `inner_html` injects the CSS verbatim so the
        // [data-chorale-theme="..."] selector quotes are not escaped.
        <style inner_html=theme_stylesheet()></style>
        <div
            node_ref=kb_ref
            class=THEME_ROOT_CLASS
            data-chorale-theme=theme.attribute_value()
            tabindex="0"
            style="border:1px solid var(--chorale-border, #ddd);border-radius:4px;overflow:hidden;user-select:none;outline:none;background:var(--chorale-surface, #fff);color:var(--chorale-text, #333);"
            on:mousemove=move |ev| {
                if let Some((col_id, start_x, start_w)) = drag_state.get_untracked() {
                    let delta = f64::from(ev.client_x()) - start_x;
                    handle.set_column_width(col_id, (start_w + delta).max(40.0)).ok();
                }
            }
            on:mouseup=move |_| {
                drag_state.set(None);
                if fill_drag_active.get_untracked() {
                    fill_drag_active.set(false);
                    if let Some((target_row, target_col)) = fill_hover.get_untracked() {
                        let state = sig.get_untracked();
                        if let Some(source_range) = state.range_selection.first() {
                            let writes = fill_handle_targets(&state, source_range, target_row, target_col);
                            if !writes.is_empty() {
                                let mut rows_map: std::collections::HashMap<usize, Vec<(ColumnId, CellValue)>> =
                                    std::collections::HashMap::new();
                                for (r, c, v) in &writes {
                                    rows_map.entry(*r).or_default().push((*c, v.clone()));
                                }
                                let mut row_idxs: Vec<usize> = rows_map.keys().copied().collect();
                                row_idxs.sort_unstable();
                                let tsv = row_idxs.iter().map(|ri| {
                                    let cols = &rows_map[ri];
                                    cols.iter().map(|(_, v)| v.to_csv_string()).collect::<Vec<_>>().join("\t")
                                }).collect::<Vec<_>>().join("\n");
                                let first_row = *row_idxs.first().unwrap_or(&target_row);
                                let last_row = *row_idxs.last().unwrap_or(&target_row);
                                let first_col = writes.first().map_or(target_col, |(_, c, _)| *c);
                                let last_col = writes.last().map_or(target_col, |(_, c, _)| *c);
                                let ext_range = RangeSelection::new(
                                    (first_row, first_col),
                                    (last_row, last_col),
                                );
                                sig.update(|s| s.range_selection = vec![ext_range.clone()]);
                                if let Some(cb) = on_paste {
                                    cb.run(ClipboardPasteEvent { tsv, range: ext_range });
                                }
                            }
                        }
                    }
                    fill_hover.set(None);
                }
            }
            on:mouseleave=move |_| {
                drag_state.set(None);
                if fill_drag_active.get_untracked() {
                    fill_drag_active.set(false);
                    fill_hover.set(None);
                }
            }
            on:click=move |_| {
                if let Some(el) = kb_ref.get_untracked() {
                    let _ = el.focus();
                }
            }
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                let shift = ev.shift_key();
                let ctrl = ev.ctrl_key() || ev.meta_key();
                let key = ev.key();
                match key.as_str() {
                    "ArrowDown" | "ArrowUp" | "ArrowLeft" | "ArrowRight" => {
                        ev.prevent_default();
                        let dir = match key.as_str() {
                            "ArrowDown" => NavDirection::Down,
                            "ArrowUp" => NavDirection::Up,
                            "ArrowLeft" => NavDirection::Left,
                            _ => NavDirection::Right,
                        };
                        if shift {
                            let new_s = handle.signal.with_untracked(|s| {
                                let vis_cols: Vec<ColumnId> = s
                                    .columns
                                    .iter()
                                    .filter(|c| {
                                        *s.column_visibility.get(&c.id).unwrap_or(&true)
                                    })
                                    .map(|c| c.id)
                                    .collect();
                                let total = s.filtered_row_count();
                                let focus = s
                                    .range_selection
                                    .last()
                                    .map(|r| r.focus)
                                    .or_else(|| s.active_cell.map(|ac| (ac.row_idx, ac.column_id)));
                                if let Some((row, col_id)) = focus {
                                    let col_idx = vis_cols.iter().position(|id| *id == col_id).unwrap_or(0);
                                    let last_row = total.saturating_sub(1);
                                    let last_col = vis_cols.len().saturating_sub(1);
                                    let (new_row, new_col_idx) = match dir {
                                        NavDirection::Up => (row.saturating_sub(1), col_idx),
                                        NavDirection::Down => ((row + 1).min(last_row), col_idx),
                                        NavDirection::Left => (row, col_idx.saturating_sub(1)),
                                        NavDirection::Right => (row, (col_idx + 1).min(last_col)),
                                        _ => (row, col_idx),
                                    };
                                    let new_col_id = vis_cols.get(new_col_idx).copied().unwrap_or(col_id);
                                    extend_range_to(s, new_row, new_col_id)
                                } else {
                                    s.clone()
                                }
                            });
                            handle.signal.set(new_s);
                        } else if ctrl {
                            let new_s = handle.signal.with_untracked(|s| move_active_cell_to_edge(s, dir));
                            handle.signal.set(new_s);
                            if !inline {
                                scroll_active_cell_into_view(&handle, scroll_ref, variable_row_height);
                            }
                        } else {
                            let new_s = handle.signal.with_untracked(|s| move_active_cell(s, dir));
                            handle.signal.set(new_s);
                            if !inline {
                                scroll_active_cell_into_view(&handle, scroll_ref, variable_row_height);
                            }
                        }
                    }
                    "Home" => {
                        ev.prevent_default();
                        let new_s = if ctrl {
                            handle.signal.with_untracked(move_active_cell_first)
                        } else {
                            handle.signal.with_untracked(move_active_cell_home)
                        };
                        handle.signal.set(new_s);
                        if !inline {
                            scroll_active_cell_into_view(&handle, scroll_ref, variable_row_height);
                        }
                    }
                    "End" => {
                        ev.prevent_default();
                        let new_s = if ctrl {
                            handle.signal.with_untracked(move_active_cell_last)
                        } else {
                            handle.signal.with_untracked(move_active_cell_end)
                        };
                        handle.signal.set(new_s);
                        if !inline {
                            scroll_active_cell_into_view(&handle, scroll_ref, variable_row_height);
                        }
                    }
                    "PageUp" => {
                        ev.prevent_default();
                        let page_sz = handle.signal.with_untracked(|s| s.page_size);
                        let new_s = handle.signal.with_untracked(|s| move_active_cell_page(s, NavDirection::Up, page_sz));
                        handle.signal.set(new_s);
                        if !inline {
                            scroll_active_cell_into_view(&handle, scroll_ref, variable_row_height);
                        }
                    }
                    "PageDown" => {
                        ev.prevent_default();
                        let page_sz = handle.signal.with_untracked(|s| s.page_size);
                        let new_s = handle.signal.with_untracked(|s| move_active_cell_page(s, NavDirection::Down, page_sz));
                        handle.signal.set(new_s);
                        if !inline {
                            scroll_active_cell_into_view(&handle, scroll_ref, variable_row_height);
                        }
                    }
                    "Escape" => {
                        let new_s = handle.signal.with_untracked(|s| {
                            let s2 = clear_range_selection(s);
                            clear_active_cell(&s2)
                        });
                        handle.signal.set(new_s);
                    }
                    // Enter / F2 starts in-cell editing on the active cell
                    // (mirrors the dioxus Enter|F2 handler). The editor
                    // input's own on:keydown stops propagation, so
                    // Enter-to-commit inside the editor never reaches this
                    // arm; the editing.is_none() guard is defense in depth
                    // against any keydown that still bubbles while an edit
                    // is in progress.
                    "Enter" | "F2" => {
                        let target = handle.signal.with_untracked(|s| {
                            if s.editing.is_some() {
                                return None;
                            }
                            s.active_cell.and_then(|ac| {
                                let editable = s
                                    .columns
                                    .iter()
                                    .any(|c| c.id == ac.column_id && c.editor.is_some());
                                if !editable {
                                    return None;
                                }
                                // active_cell holds a visible row INDEX;
                                // resolve it to a RowId through the same
                                // post-filter post-sort view the body renders.
                                let rows = visible_view(s);
                                match rows.get(ac.row_idx) {
                                    Some(RenderRow::Data { id, .. }) => Some((*id, ac.column_id)),
                                    _ => None,
                                }
                            })
                        });
                        if let Some((row_id, col_id)) = target {
                            ev.prevent_default();
                            handle.start_edit(row_id, col_id);
                        }
                    }
                    "a" | "A" if ctrl => {
                        ev.prevent_default();
                        let new_s = handle.signal.with_untracked(select_all_range);
                        handle.signal.set(new_s);
                    }
                    "c" | "C" if ctrl => {
                        ev.prevent_default();
                        #[cfg(target_arch = "wasm32")]
                        {
                            let tsv_result = handle.signal.with_untracked(to_clipboard_tsv);
                            if let Ok(tsv) = tsv_result {
                                if !tsv.is_empty() {
                                    let range =
                                        handle.signal.with_untracked(|s| {
                                            s.range_selection.first().cloned()
                                        });
                                    if let Some(range) = range {
                                        let tsv2 = tsv.clone();
                                        leptos::task::spawn_local(async move {
                                            if let Some(clipboard) = web_sys::window()
                                                .map(|w| w.navigator().clipboard())
                                            {
                                                let _ = wasm_bindgen_futures::JsFuture::from(
                                                    clipboard.write_text(&tsv2),
                                                )
                                                .await;
                                            }
                                        });
                                        if let Some(cb) = on_copy {
                                            cb.run(ClipboardCopyEvent { tsv, range });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "v" | "V" if ctrl => {
                        ev.prevent_default();
                        #[cfg(target_arch = "wasm32")]
                        leptos::task::spawn_local(async move {
                            let clipboard = web_sys::window().map(|w| w.navigator().clipboard());
                            if let Some(clipboard) = clipboard {
                                if let Ok(tsv_val) = wasm_bindgen_futures::JsFuture::from(
                                    clipboard.read_text(),
                                )
                                .await
                                {
                                    let tsv = tsv_val.as_string().unwrap_or_default();
                                    if !tsv.trim().is_empty() {
                                        let new_s = handle.signal.with_untracked(|s| {
                                            paste_tsv_into_range(s, &tsv)
                                        });
                                        if let Ok(new_state) = new_s {
                                            let range =
                                                new_state.range_selection.first().cloned();
                                            handle.signal.set(new_state);
                                            if let (Some(range), Some(cb)) = (range, on_paste) {
                                                cb.run(ClipboardPasteEvent { tsv, range });
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                    "Tab" => {
                        ev.prevent_default();
                        let tab_dir = if shift { NavDirection::Left } else { NavDirection::Right };
                        let (new_s, new_ac) = handle.signal.with_untracked(|s| {
                            let ns = move_active_cell(s, tab_dir);
                            let ac = ns.active_cell;
                            (ns, ac)
                        });
                        handle.signal.set(new_s);
                        if let (Some(ac), Some(cb)) = (new_ac, on_tab_to_editable) {
                            let is_editable = handle.signal.with_untracked(|s| {
                                s.columns.iter().any(|c| c.id == ac.column_id && c.editor.is_some())
                            });
                            if is_editable {
                                cb.run(ac);
                            }
                        }
                    }
                    _ => {}
                }
            }
        >
            // Column visibility toolbar
            {{
                let labels = labels.clone();
                move || {
                    if column_toolbar {
                        let s = sig.get();
                        let all_cols: Vec<(ColumnId, String)> = s.columns
                            .iter()
                            .map(|c| (c.id, c.header.clone()))
                            .collect();
                        let vis = s.column_visibility.clone();
                        Some(column_visibility_toolbar(&all_cols, &vis, handle, &labels))
                    } else {
                        None
                    }
                }
            }}

            // Selection toolbar slot. Renders whenever the slot is provided,
            // regardless of selection size — consumer-supplied toolbars
            // typically include "Select all" affordances that are useful
            // exactly in the empty-selection state. The wrapper carries
            // only structural styling; the slot's own content provides
            // visual treatment.
            {selection_toolbar.map(|slot| {
                view! {
                    <div
                        class="chorale-selection-toolbar"
                        style="width:100%;box-sizing:border-box;\
                               border-bottom:2px solid var(--chorale-accent-strong, #1d4ed8);"
                    >
                        {slot()}
                    </div>
                }
                .into_any()
            })}

            // Virtualized scroll container (or natural-height wrapper in
            // inline mode).
            <div
                node_ref=scroll_ref
                style=move || {
                    if inline {
                        // Inline mode: no own scroll, no height clamp. Body
                        // flows at natural size; parent's scroll context owns
                        // overflow. Wheel events bubble through cleanly with
                        // no nested-scroll handoff discontinuity.
                        "overflow:visible;height:auto;".to_string()
                    } else {
                        let h = sig.with(|s| s.viewport_height);
                        format!(
                            "overflow-y:auto;overflow-x:auto;overflow-anchor:none;\
                             height:{h}px;"
                        )
                    }
                }
                on:scroll=move |_| {
                    let st = scroll_ref
                        .get()
                        .map_or(0.0, |el| f64::from(el.scroll_top()));
                    handle.set_scroll(st);
                    // Infinite scroll: trigger load_more when near bottom.
                    let is_infinite = sig.with_untracked(|s| {
                        s.pagination_mode == PaginationMode::InfiniteScroll
                    });
                    if is_infinite {
                        let (total_h, loaded, row_h, vp_h) = sig.with_untracked(|s| {
                            #[allow(clippy::cast_precision_loss)]
                            let total = s.loaded_row_count as f64 * s.row_height;
                            (total, s.loaded_row_count, s.row_height, s.viewport_height)
                        });
                        let _ = (loaded, row_h);
                        let dist = total_h - st - vp_h;
                        if dist < infinite_scroll_threshold_px {
                            handle.load_more_rows();
                        }
                    }
                }
            >
                <table style="width:100%;border-collapse:collapse;table-layout:fixed;">
                    {{
                    let labels = labels.clone();
                    move || {
                        let s = sig.get();
                        let (sticky_header_css, sticky_body_css) =
                            build_sticky_css(&s, frozen_column_z_index);

                        let effective_order: Vec<ColumnId> = if s.column_order.is_empty() {
                            s.columns.iter().map(|c| c.id).collect()
                        } else {
                            let mut order: Vec<ColumnId> = s
                                .column_order
                                .iter()
                                .filter(|id| s.columns.iter().any(|c| c.id == **id))
                                .copied()
                                .collect();
                            for col in &s.columns {
                                if !s.column_order.contains(&col.id) {
                                    order.push(col.id);
                                }
                            }
                            order
                        };

                        let left_frozen: Vec<ColumnDef<TRow>> =
                            frozen_left_columns(&s).into_iter().cloned().collect();
                        let scrollable: Vec<ColumnDef<TRow>> =
                            scrollable_columns(&s).into_iter().cloned().collect();
                        let right_frozen: Vec<ColumnDef<TRow>> =
                            frozen_right_columns(&s).into_iter().cloned().collect();
                        let visible_cols: Vec<ColumnDef<TRow>> = left_frozen
                            .iter()
                            .chain(scrollable.iter())
                            .chain(right_frozen.iter())
                            .cloned()
                            .collect();

                        let widths = s.column_widths.clone();
                        let current_sort = s.sort.clone();
                        let filters = s.filters.clone();
                        let editing_target = s.editing;
                        let selection_set: HashSet<RowId> =
                            s.selection.iter().copied().collect();
                        let is_grouped = !s.grouping.is_empty();
                        let is_virtualized_grouped = is_grouped
                            && s.grouped_pagination == GroupedPaginationMode::Virtualized;
                        let row_height = s.row_height;
                        let has_detail = detail_renderer.is_some();
                        let effective_col_count =
                            visible_cols.len() + usize::from(selection_enabled) + usize::from(has_detail);
                        let _all_col_defs: Vec<(ColumnId, String)> = effective_order
                            .iter()
                            .filter_map(|id| s.columns.iter().find(|c| c.id == *id))
                            .map(|c| (c.id, c.header.clone()))
                            .collect();
                        let _col_visibility = s.column_visibility.clone();
                        let vis = visible.get();
                        let page_data_ids: Vec<RowId> = vis
                            .iter()
                            .filter_map(|r| if let RenderRow::Data { id, .. } = r { Some(*id) } else { None })
                            .collect();
                        let all_page_selected = !page_data_ids.is_empty()
                            && page_data_ids.iter().all(|id| selection_set.contains(id));
                        let total_pages = s.total_pages();
                        let page_idx = s.page;
                        let total_rows = s.filtered_row_count();
                        let is_infinite = s.pagination_mode == PaginationMode::InfiniteScroll;
                        let vis_data_count = vis.iter().filter(|r| matches!(r, RenderRow::Data { .. })).count();
                        let has_more = is_infinite && vis_data_count < total_rows;

                        // In inline mode we bypass virtualization entirely —
                        // every visible row renders in a single batch with no
                        // top/bottom spacer <tr>s. This makes the <Table>
                        // usable as a child of an outer scrolling element
                        // (e.g., master/detail panel) without creating a
                        // nested scroll context that would otherwise produce
                        // wheel-event hand-off discontinuities ("jumps") when
                        // the user scrolls past the edge of the inner view.
                        let (win, render_slice) = if inline {
                            // visible_window(0, MAX, ...) covers all rows with
                            // zero pad on either side, so the spacer-row
                            // branches below render nothing.
                            let full_slice: Vec<RenderRow<TRow>> = vis.clone();
                            let iwin = visible_window(
                                0.0,
                                f64::MAX,
                                row_height,
                                full_slice.len(),
                                0,
                            );
                            (iwin, full_slice)
                        } else {
                            // VIRT-2: when variable_row_height is on (set
                            // explicitly or forced by detail_renderer), the
                            // window + spacer pads come from the measured
                            // per-row heights instead of uniform row_height.
                            compute_window_slice(&s, &vis, variable_row_height)
                        };

                        // Active cell + range selection for highlighting.
                        let active_cell = s.active_cell;
                        let range_cells: HashSet<(usize, ColumnId)> = {
                            let col_refs: Vec<&ColumnDef<TRow>> = visible_cols.iter().collect();
                            let mut cells = HashSet::new();
                            for r in &s.range_selection {
                                let nr = r.normalized(&col_refs);
                                for row in nr.min_row..=nr.max_row {
                                    for &col_id in &nr.columns {
                                        cells.insert((row, col_id));
                                    }
                                }
                            }
                            cells
                        };

                        let fill_focus_cell: Option<(usize, ColumnId)> = if s.range_selection.len() == 1 {
                            let col_refs: Vec<&ColumnDef<TRow>> = visible_cols.iter().collect();
                            let nr = s.range_selection[0].normalized(&col_refs);
                            nr.columns.last().map(|&col_id| (nr.max_row, col_id))
                        } else {
                            None
                        };

                        // ---- table header ----
                        let header_row: Vec<AnyView> = visible_cols
                            .iter()
                            .map(|col| {
                                header_th(
                                    col,
                                    widths.get(&col.id).copied(),
                                    handle,
                                    sort_enabled,
                                    &current_sort,
                                    resize_enabled,
                                    drag_state,
                                    column_reorder_enabled,
                                    drag_col_id,
                                    sticky_header_css
                                        .get(&col.id)
                                        .map_or("", String::as_str),
                                    sticky_header,
                                )
                            })
                            .collect();

                        let filter_row: Option<Vec<AnyView>> = if filter_enabled {
                            Some(
                                visible_cols
                                    .iter()
                                    .map(|col| {
                                        filter_th(
                                            col,
                                            widths.get(&col.id).copied(),
                                            handle,
                                            &filters,
                                            &labels,
                                            sticky_header_css
                                                .get(&col.id)
                                                .map_or("", String::as_str),
                                            open_filter_col,
                                        )
                                    })
                                    .collect(),
                            )
                        } else {
                            None
                        };

                        // ---- tbody rows ----
                        let tbody_content: Vec<AnyView> = if is_grouped {
                            let grouped_rows = grouped_visible.get();
                            // Renders one grouped row at its ABSOLUTE index in
                            // the flat grouped list. The index feeds
                            // active-cell / range highlighting, so it must be
                            // the list position, not the window offset.
                            let render_grouped_row =
                                |i: usize, grouped_row: GroupedRow<TRow>| match grouped_row {
                                    GroupedRow::Header {
                                        key,
                                        label,
                                        depth,
                                        row_count,
                                        is_collapsed,
                                        ..
                                    } => render_group_header(
                                        key,
                                        label,
                                        depth,
                                        row_count,
                                        is_collapsed,
                                        effective_col_count,
                                        handle,
                                        &group_header_class,
                                    ),
                                    GroupedRow::Data(row_id, row) => render_data_row(
                                        &row,
                                        row_id,
                                        i,
                                        &visible_cols,
                                        &widths,
                                        row_height,
                                        variable_row_height,
                                        selection_enabled,
                                        selection_set.contains(&row_id),
                                        handle,
                                        &cell_renderers,
                                        &row_cell_renderers,
                                        editing_target
                                            .filter(|t| t.row_id == row_id)
                                            .map(|t| t.column_id),
                                        editing_text,
                                        edit_error,
                                        &validate_edit,
                                        on_commit_edit.as_ref(),
                                        &sticky_body_css,
                                        active_cell,
                                        &range_cells,
                                        fill_focus_cell,
                                        fill_drag_active,
                                        fill_hover,
                                        has_detail,
                                        false,
                                        on_row_click,
                                        kb_ref,
                                    ),
                                    _ => view! { <tr /> }.into_any(),
                                };
                            if is_virtualized_grouped && !inline {
                                // GroupedPaginationMode::Virtualized: core's
                                // visible_grouped_view returns the ENTIRE flat
                                // grouped list (no pagination), so rendering it
                                // all would put every row in the DOM and freeze
                                // the page on large datasets. Window it exactly
                                // like the non-grouped virtualized path below:
                                // grouped rows are uniform row_height, so the
                                // same fixed-height visible_window math applies,
                                // and top/bottom spacer rows
                                // (start_index * row_height and
                                // (total - end_index - 1) * row_height) keep the
                                // scrollbar geometry of the full list.
                                //
                                // VIRT-2 note: this path deliberately stays on
                                // the uniform window even when
                                // variable_row_height is on. Detail panels
                                // (the variable-height case) cannot appear
                                // here — core's GroupedRow has no DetailPanel
                                // variant, so grouping + master/detail cannot
                                // combine today. If core ever adds grouped
                                // detail panels, switch this to
                                // visible_window_variable like the non-grouped
                                // path above.
                                let total = grouped_rows.len();
                                let gwin = visible_window(
                                    s.scroll_top,
                                    s.viewport_height,
                                    row_height,
                                    total,
                                    s.buffer_rows,
                                );
                                let mut rows: Vec<AnyView> = Vec::new();
                                if gwin.top_pad_px > 0.0 {
                                    let p = gwin.top_pad_px;
                                    let c = effective_col_count;
                                    rows.push(view! {
                                        <tr>
                                            <td
                                                colspan=c.to_string()
                                                style=format!("height:{p}px;padding:0;border:0;")
                                            />
                                        </tr>
                                    }.into_any());
                                }
                                if total > 0 {
                                    let gwin_end = gwin.end_index.min(total - 1);
                                    for (offset, grouped_row) in grouped_rows
                                        [gwin.start_index..=gwin_end]
                                        .iter()
                                        .enumerate()
                                    {
                                        rows.push(render_grouped_row(
                                            gwin.start_index + offset,
                                            grouped_row.clone(),
                                        ));
                                    }
                                }
                                if gwin.bottom_pad_px > 0.0 {
                                    let p = gwin.bottom_pad_px;
                                    let c = effective_col_count;
                                    rows.push(view! {
                                        <tr>
                                            <td
                                                colspan=c.to_string()
                                                style=format!("height:{p}px;padding:0;border:0;")
                                            />
                                        </tr>
                                    }.into_any());
                                }
                                rows
                            } else {
                                // DataRowsOnly: core already paginates the
                                // grouped list; render it unchanged.
                                // Inline mode also lands here regardless of
                                // grouped-pagination mode: virtualization is
                                // bypassed, so the entire grouped list renders
                                // in one batch with no spacer rows.
                                grouped_rows
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, grouped_row)| render_grouped_row(i, grouped_row))
                                    .collect()
                            }
                        } else {
                            let mut rows: Vec<AnyView> = Vec::new();
                            if win.top_pad_px > 0.0 {
                                let p = win.top_pad_px;
                                let c = effective_col_count;
                                rows.push(view! {
                                    <tr>
                                        <td
                                            colspan=c.to_string()
                                            style=format!("height:{p}px;padding:0;border:0;")
                                        />
                                    </tr>
                                }.into_any());
                            }
                            for (i, render_row) in render_slice.iter().enumerate() {
                                match render_row {
                                    RenderRow::Data { id: row_id, row } => {
                                        let row_id = *row_id;
                                        let is_expanded = has_detail && s.expanded_rows.contains(&row_id);
                                        let editing_col = editing_target
                                            .filter(|t| t.row_id == row_id)
                                            .map(|t| t.column_id);
                                        rows.push(render_data_row(
                                            row,
                                            row_id,
                                            win.start_index + i,
                                            &visible_cols,
                                            &widths,
                                            row_height,
                                            variable_row_height,
                                            selection_enabled,
                                            selection_set.contains(&row_id),
                                            handle,
                                            &cell_renderers,
                                            &row_cell_renderers,
                                            editing_col,
                                            editing_text,
                                            edit_error,
                                            &validate_edit,
                                            on_commit_edit.as_ref(),
                                            &sticky_body_css,
                                            active_cell,
                                            &range_cells,
                                            fill_focus_cell,
                                            fill_drag_active,
                                            fill_hover,
                                            has_detail,
                                            is_expanded,
                                            on_row_click,
                                            kb_ref,
                                        ));
                                    }
                                    RenderRow::DetailPanel { parent_row_id } => {
                                        let pid = *parent_row_id;
                                        let parent = s.rows.iter()
                                            .find(|(rid, _)| *rid == pid)
                                            .map(|(_, r)| r.clone());
                                        let colspan = effective_col_count;
                                        // data-chorale-index lets the VIRT-2
                                        // measurement loop record this panel's
                                        // actual rendered height (e.g. 200px
                                        // for a 5-item child table) in
                                        // state.row_heights. Without it,
                                        // visible_window_variable would fall
                                        // back to state.row_height for the
                                        // panel slot — its prefix-sum would
                                        // underestimate content height by
                                        // (real - estimate) per expanded row,
                                        // and scroll math would drift then
                                        // snap at boundaries (the "jump").
                                        let abs_index = win.start_index + i;
                                        let view = match (parent, &detail_renderer) {
                                            (Some(prow), Some(renderer)) => {
                                                let content = renderer(prow);
                                                view! {
                                                    <tr
                                                        class="chorale-row chorale-detail-panel"
                                                        data-chorale-index=abs_index.to_string()
                                                    >
                                                        <td colspan=colspan.to_string()>
                                                            <div class="chorale-detail-panel-inner">
                                                                {content}
                                                            </div>
                                                        </td>
                                                    </tr>
                                                }.into_any()
                                            }
                                            _ => view! {
                                                <tr
                                                    class="chorale-row chorale-detail-panel-empty"
                                                    data-chorale-index=abs_index.to_string()
                                                />
                                            }.into_any(),
                                        };
                                        rows.push(view);
                                    }
                                }
                            }
                            if win.bottom_pad_px > 0.0 {
                                let p = win.bottom_pad_px;
                                let c = effective_col_count;
                                rows.push(view! {
                                    <tr>
                                        <td
                                            colspan=c.to_string()
                                            style=format!("height:{p}px;padding:0;border:0;")
                                        />
                                    </tr>
                                }.into_any());
                            }
                            if total_rows == 0 {
                                let lbl = labels.no_rows_label.clone();
                                let c = effective_col_count;
                                rows.push(view! {
                                    <tr>
                                        <td
                                            colspan=c.to_string()
                                            style="padding:2rem 1rem;text-align:center;\
                                                   color:var(--chorale-text-subtle, #999);font-style:italic;"
                                        >{lbl}</td>
                                    </tr>
                                }.into_any());
                            }
                            rows
                        };

                        // ---- select-all checkbox in thead ----
                        let select_all_th = if selection_enabled {
                            {
                                let sel_sticky = if sticky_header {
                                    "position:sticky;top:0;z-index:1;"
                                } else {
                                    "position:static;top:auto;z-index:auto;"
                                };
                                Some(view! {
                                    <th style=format!(
                                        "padding:0.5rem;border-bottom:1px solid var(--chorale-border, #ddd);\
                                         background:var(--chorale-header-bg, #f8f9fa);width:2.5rem;{sel_sticky}"
                                    )>
                                        <input
                                            type="checkbox"
                                            prop:checked=all_page_selected
                                            on:change=move |ev| {
                                                if event_target_checked(&ev) {
                                                    handle.select_all_filtered();
                                                } else {
                                                    handle.deselect_all();
                                                }
                                            }
                                        />
                                    </th>
                                })
                            }
                        } else {
                            None
                        };

                        let chevron_th = if has_detail {
                            let chev_sticky = if sticky_header {
                                "position:sticky;top:0;z-index:1;"
                            } else {
                                "position:static;top:auto;z-index:auto;"
                            };
                            Some(view! {
                                <th style=format!(
                                    "width:24px;padding:0;border-bottom:1px solid var(--chorale-border, #ddd);\
                                     background:var(--chorale-header-bg, #f8f9fa);{chev_sticky}"
                                ) />
                            })
                        } else {
                            None
                        };

                        let filter_empty_th = if selection_enabled && filter_enabled {
                            Some(view! {
                                <th style="padding:0.25rem;border-bottom:1px solid var(--chorale-divider, #eee);\
                                           background:var(--chorale-surface, #fff);width:2.5rem;" />
                            })
                        } else {
                            None
                        };

                        let filter_chevron_th = if has_detail && filter_enabled {
                            Some(view! {
                                <th style="width:24px;padding:0;border-bottom:1px solid var(--chorale-divider, #eee);background:var(--chorale-surface, #fff);" />
                            })
                        } else {
                            None
                        };

                        // ---- pagination bar (rendered in outer closure) ----
                        let _pagination: AnyView = if is_infinite {
                            if has_more {
                                let lbl = labels.load_more_label.clone();
                                view! {
                                    <div style="padding:0.75rem 1rem;text-align:center;\
                                                border-top:1px solid var(--chorale-border, #ddd);background:var(--chorale-toolbar-bg, #fafafa);\
                                                font-size:0.875rem;color:var(--chorale-text-subtle, #999);">
                                        {lbl}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <div /> }.into_any()
                            }
                        } else if !is_virtualized_grouped {
                            let nav_btn = "padding:0.25rem 0.6rem;border:1px solid var(--chorale-border, #ddd);\
                                border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                background:var(--chorale-button-bg, white);color:var(--chorale-text, #333);";
                            let nav_btn_dis = "padding:0.25rem 0.6rem;border:1px solid var(--chorale-border, #ddd);\
                                border-radius:3px;font-size:0.875rem;cursor:not-allowed;\
                                background:var(--chorale-button-disabled-bg, #f0f0f0);color:var(--chorale-text-disabled, #aaa);";
                            let prev_disabled = page_idx == 0;
                            let next_disabled = page_idx + 1 >= total_pages;
                            let page_buttons = page_button_range(page_idx, total_pages);
                            let lbl_prev = labels.previous_page_label.clone();
                            let lbl_next = labels.next_page_label.clone();

                            let page_btns: Vec<AnyView> = page_buttons
                                .into_iter()
                                .map(|item| match item {
                                    PageItem::Page(p) => {
                                        let style = if p == page_idx {
                                            "padding:0.25rem 0.6rem;border:1px solid var(--chorale-accent, #4a90e2);\
                                             border-radius:3px;font-size:0.875rem;\
                                             background:var(--chorale-accent, #4a90e2);color:var(--chorale-accent-contrast, white);cursor:pointer;"
                                        } else {
                                            nav_btn
                                        };
                                        view! {
                                            <button
                                                style=style
                                                on:click=move |_| { handle.set_page(p).ok(); }
                                            >
                                                {p + 1}
                                            </button>
                                        }
                                        .into_any()
                                    }
                                    PageItem::Ellipsis => {
                                        view! {
                                            <span style="padding:0.25rem 0.3rem;color:var(--chorale-text-subtle, #999);">
                                                "…"
                                            </span>
                                        }
                                        .into_any()
                                    }
                                })
                                .collect();

                            let csv_button: Option<AnyView> = if csv_export {
                                let lbl_csv = labels.export_csv_label.clone();
                                Some(view! {
                                    <button
                                        style="padding:0.25rem 0.75rem;border:1px solid var(--chorale-accent, #4a90e2);\
                                               border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                               background:var(--chorale-button-bg, white);color:var(--chorale-accent, #4a90e2);"
                                        on:click=move |_| {
                                            let csv = sig.with_untracked(|s| to_csv(s));
                                            trigger_csv_download(&csv);
                                        }
                                    >
                                        {lbl_csv}
                                    </button>
                                }.into_any())
                            } else {
                                None
                            };

                            let goto = if total_pages > 1 {
                                Some(view! {
                                    <GotoPageInput
                                        handle=handle
                                        total_pages=total_pages
                                        labels=labels.clone()
                                    />
                                })
                            } else {
                                None
                            };

                            view! {
                                <div style="padding:0.5rem 1rem;display:flex;align-items:center;\
                                            flex-wrap:wrap;gap:0.25rem;border-top:1px solid var(--chorale-border, #ddd);\
                                            background:var(--chorale-toolbar-bg, #fafafa);font-size:0.875rem;color:var(--chorale-text-muted, #555);">
                                    <button
                                        style=if prev_disabled { nav_btn_dis } else { nav_btn }
                                        disabled=prev_disabled
                                        on:click=move |_| {
                                            handle.set_page(page_idx.saturating_sub(1)).ok();
                                        }
                                    >
                                        {lbl_prev}
                                    </button>
                                    {page_btns}
                                    <button
                                        style=if next_disabled { nav_btn_dis } else { nav_btn }
                                        disabled=next_disabled
                                        on:click=move |_| {
                                            if page_idx + 1 < total_pages {
                                                handle.set_page(page_idx + 1).ok();
                                            }
                                        }
                                    >
                                        {lbl_next}
                                    </button>
                                    <span style="margin-left:0.5rem;color:var(--chorale-text-subtle, #999);">{"\u{00b7}"}</span>
                                    <span>{format!("{total_rows} rows")}</span>
                                    {goto}
                                    {csv_button.map(|b| view! {
                                        <span style="flex:1;" />
                                        {b}
                                    })}
                                </div>
                            }
                            .into_any()
                        } else {
                            view! { <div /> }.into_any()
                        };

                        let grouped_empty = if is_grouped && grouped_visible.get().is_empty() {
                            let lbl = labels.no_rows_label.clone();
                            let c = effective_col_count;
                            Some(view! {
                                <tr>
                                    <td
                                        colspan=c.to_string()
                                        style="padding:2rem 1rem;text-align:center;\
                                               color:var(--chorale-text-subtle, #999);font-style:italic;"
                                    >{lbl}</td>
                                </tr>
                            })
                        } else {
                            None
                        };

                        view! {
                            <thead>
                                <tr style="background:var(--chorale-header-bg, #f8f9fa);">
                                    {select_all_th}
                                    {chevron_th}
                                    {header_row}
                                </tr>
                                {filter_row.map(|cells| view! {
                                    <tr style="background:var(--chorale-surface, #fff);">
                                        {filter_empty_th}
                                        {filter_chevron_th}
                                        {cells}
                                    </tr>
                                })}
                            </thead>
                            <tbody>
                                {tbody_content}
                                {grouped_empty}
                            </tbody>
                        }
                    }}}
                </table>
            </div>

            // Pagination / infinite scroll indicator
            {move || {
                let s = sig.get();
                let is_infinite = s.pagination_mode == PaginationMode::InfiniteScroll;
                let vis_data_count = visible.get().iter().filter(|r| matches!(r, RenderRow::Data { .. })).count();
                let has_more = is_infinite && vis_data_count < s.filtered_row_count();
                let is_grouped = !s.grouping.is_empty();
                let is_virt_grouped =
                    is_grouped && s.grouped_pagination == GroupedPaginationMode::Virtualized;

                if is_infinite && has_more {
                    let lbl = labels.load_more_label.clone();
                    view! {
                        <div style="padding:0.75rem 1rem;text-align:center;\
                                    border-top:1px solid var(--chorale-border, #ddd);background:var(--chorale-toolbar-bg, #fafafa);\
                                    font-size:0.875rem;color:var(--chorale-text-subtle, #999);">
                            {lbl}
                        </div>
                    }.into_any()
                } else if !is_infinite && !is_virt_grouped {
                    let nav_btn = "padding:0.25rem 0.6rem;border:1px solid var(--chorale-border, #ddd);\
                        border-radius:3px;font-size:0.875rem;cursor:pointer;\
                        background:var(--chorale-button-bg, white);color:var(--chorale-text, #333);";
                    let nav_btn_dis = "padding:0.25rem 0.6rem;border:1px solid var(--chorale-border, #ddd);\
                        border-radius:3px;font-size:0.875rem;cursor:not-allowed;\
                        background:var(--chorale-button-disabled-bg, #f0f0f0);color:var(--chorale-text-disabled, #aaa);";
                    let total_pages = s.total_pages();
                    let page_idx = s.page;
                    let total_rows = s.filtered_row_count();
                    let prev_disabled = page_idx == 0;
                    let next_disabled = page_idx + 1 >= total_pages;
                    let page_buttons = page_button_range(page_idx, total_pages);
                    let lbl_prev = labels.previous_page_label.clone();
                    let lbl_next = labels.next_page_label.clone();

                    let page_btns: Vec<AnyView> = page_buttons
                        .into_iter()
                        .map(|item| match item {
                            PageItem::Page(p) => {
                                let style = if p == page_idx {
                                    "padding:0.25rem 0.6rem;border:1px solid var(--chorale-accent, #4a90e2);\
                                     border-radius:3px;font-size:0.875rem;\
                                     background:var(--chorale-accent, #4a90e2);color:var(--chorale-accent-contrast, white);cursor:pointer;"
                                } else {
                                    nav_btn
                                };
                                view! {
                                    <button style=style on:click=move |_| { handle.set_page(p).ok(); }>
                                        {p + 1}
                                    </button>
                                }.into_any()
                            }
                            PageItem::Ellipsis => {
                                view! { <span style="padding:0.25rem 0.3rem;color:var(--chorale-text-subtle, #999);">"…"</span> }.into_any()
                            }
                        })
                        .collect();

                    let csv_button: Option<AnyView> = if csv_export {
                        let lbl_csv = labels.export_csv_label.clone();
                        Some(view! {
                            <button
                                style="padding:0.25rem 0.75rem;border:1px solid var(--chorale-accent, #4a90e2);\
                                       border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                       background:var(--chorale-button-bg, white);color:var(--chorale-accent, #4a90e2);"
                                on:click=move |_| {
                                    let csv = sig.with_untracked(|s| to_csv(s));
                                    trigger_csv_download(&csv);
                                }
                            >
                                {lbl_csv}
                            </button>
                        }.into_any())
                    } else {
                        None
                    };

                    let xlsx_button: Option<AnyView> = if xlsx_export {
                        #[cfg(feature = "xlsx")]
                        {
                            let lbl = labels.export_xlsx_label.clone();
                            Some(view! {
                                <button
                                    style="padding:0.25rem 0.75rem;border:1px solid var(--chorale-accent, #4a90e2);\
                                           border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                           background:var(--chorale-button-bg, white);color:var(--chorale-accent, #4a90e2);"
                                    on:click=move |_| {
                                        #[cfg(target_arch = "wasm32")]
                                        {
                                            use chorale_core::{XlsxOptions, to_xlsx};
                                            let state = sig.get_untracked();
                                            let Ok(bytes) = to_xlsx(&state, &XlsxOptions::default()) else { return };
                                            // Blob + object URL (NOT a data: URL) so
                                            // repeated exports auto-increment the
                                            // filename like the Dioxus adapter.
                                            trigger_xlsx_download(&bytes, "export.xlsx");
                                        }
                                    }
                                >
                                    {lbl}
                                </button>
                            }.into_any())
                        }
                        #[cfg(not(feature = "xlsx"))]
                        { None }
                    } else {
                        None
                    };

                    let has_export = csv_button.is_some() || xlsx_button.is_some();
                    view! {
                        <div style="padding:0.5rem 1rem;display:flex;align-items:center;\
                                    flex-wrap:wrap;gap:0.25rem;border-top:1px solid var(--chorale-border, #ddd);\
                                    background:var(--chorale-toolbar-bg, #fafafa);font-size:0.875rem;color:var(--chorale-text-muted, #555);">
                            <button
                                style=if prev_disabled { nav_btn_dis } else { nav_btn }
                                disabled=prev_disabled
                                on:click=move |_| {
                                    handle.set_page(page_idx.saturating_sub(1)).ok();
                                }
                            >
                                {lbl_prev}
                            </button>
                            {page_btns}
                            <button
                                style=if next_disabled { nav_btn_dis } else { nav_btn }
                                disabled=next_disabled
                                on:click=move |_| {
                                    if page_idx + 1 < total_pages {
                                        handle.set_page(page_idx + 1).ok();
                                    }
                                }
                            >
                                {lbl_next}
                            </button>
                            <span style="margin-left:0.5rem;color:var(--chorale-text-subtle, #999);">{"\u{00b7}"}</span>
                            <span>{format!("{total_rows} rows")}</span>
                            {(total_pages > 1).then(|| view! {
                                <GotoPageInput handle=handle total_pages=total_pages labels=labels.clone() />
                            })}
                            {has_export.then(|| view! {
                                <span style="flex:1;" />
                                {csv_button}
                                {xlsx_button}
                            })}
                        </div>
                    }.into_any()
                } else {
                    view! { <div /> }.into_any()
                }
            }}
        </div>
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Clone, PartialEq)]
    struct RcrRow {
        name: String,
        email: String,
    }

    #[test]
    fn row_cell_renderer_receives_row() {
        let captured: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let captured2 = Arc::clone(&captured);
        let renderer: RowCellRenderer<RcrRow> = Arc::new(move |row: &RcrRow, _val: &CellValue| {
            *captured2.lock().unwrap() = Some(row.email.clone());
            ().into_any()
        });
        let renderers = RowCellRenderers::new(HashMap::from([(ColumnId("name"), renderer)]));
        let r = renderers.get(ColumnId("name")).unwrap();
        let row = RcrRow {
            name: "Ada".into(),
            email: "ada@example.com".into(),
        };
        r(&row, &CellValue::Text("Ada".into()));
        assert_eq!(captured.lock().unwrap().as_deref(), Some("ada@example.com"));
    }

    #[test]
    fn precedence_row_aware_wins_over_value_only() {
        let row_called = Arc::new(AtomicBool::new(false));
        let val_called = Arc::new(AtomicBool::new(false));
        let row_called2 = Arc::clone(&row_called);
        let val_called2 = Arc::clone(&val_called);

        let row_r: RowCellRenderer<RcrRow> = Arc::new(move |_: &RcrRow, _: &CellValue| {
            row_called2.store(true, Ordering::SeqCst);
            ().into_any()
        });
        let val_r: CellRenderer = Arc::new(move |_: &CellValue| {
            val_called2.store(true, Ordering::SeqCst);
            ().into_any()
        });
        let row = RcrRow {
            name: "x".into(),
            email: "x@x.com".into(),
        };
        resolve_cell_content(
            &row,
            &CellValue::Text("x".into()),
            &RenderKind::Text,
            Some(&row_r),
            Some(&val_r),
        );
        assert!(row_called.load(Ordering::SeqCst));
        assert!(!val_called.load(Ordering::SeqCst));
    }

    #[test]
    fn precedence_value_only_wins_over_render_kind() {
        let val_called = Arc::new(AtomicBool::new(false));
        let val_called2 = Arc::clone(&val_called);
        let val_r: CellRenderer = Arc::new(move |_: &CellValue| {
            val_called2.store(true, Ordering::SeqCst);
            ().into_any()
        });
        let row = RcrRow {
            name: "x".into(),
            email: "x@x.com".into(),
        };
        resolve_cell_content(
            &row,
            &CellValue::Text("x".into()),
            &RenderKind::Text,
            None,
            Some(&val_r),
        );
        assert!(val_called.load(Ordering::SeqCst));
    }

    #[test]
    fn precedence_render_kind_fallback() {
        let row = RcrRow {
            name: "x".into(),
            email: "x@x.com".into(),
        };
        // Should not panic; RenderKind path executes safely without DOM.
        let _ = resolve_cell_content(
            &row,
            &CellValue::Text("x".into()),
            &RenderKind::Text,
            None::<&RowCellRenderer<RcrRow>>,
            None,
        );
    }

    #[test]
    fn row_cell_renderers_default_is_empty() {
        assert!(RowCellRenderers::<RcrRow>::default()
            .get(ColumnId("name"))
            .is_none());
    }

    #[test]
    fn row_cell_renderers_partial_eq_is_pointer_identity() {
        let a = RowCellRenderers::<RcrRow>::new(HashMap::new());
        let b = a.clone();
        assert!(a == b);
        assert!(a != RowCellRenderers::new(HashMap::new()));
    }

    #[test]
    fn should_fire_row_click_only_on_unmodified_click() {
        assert!(should_fire_row_click(false, false));
        assert!(!should_fire_row_click(true, false));
        assert!(!should_fire_row_click(false, true));
    }
}
