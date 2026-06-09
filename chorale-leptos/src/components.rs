//! Leptos components for chorale tables.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chorale_core::{
    add_disjoint_range, clear_active_cell, clear_range_selection, extend_range_to,
    fill_handle_targets, frozen_left_columns, frozen_right_columns, move_active_cell,
    move_active_cell_end, move_active_cell_first, move_active_cell_home, move_active_cell_last,
    move_active_cell_page, move_active_cell_to_edge, scrollable_columns,
    select_all as select_all_range, start_range_selection, to_csv, visible_grouped_view,
    visible_view, visible_window, ActiveCell, Alignment, CellValue, ClipboardCopyEvent,
    ClipboardPasteEvent, ColumnDef, ColumnId, CommittedEdit, EditorKind, FilterKind, FilterValue,
    GroupKey, GroupedPaginationMode, GroupedRow, Labels, NaiveDate, NavDirection, PaginationMode,
    RangeSelection, RenderKind, RenderRow, RowId, SortAction, SortDirection, SortState, TableState,
    VirtualWindow,
};
#[cfg(target_arch = "wasm32")]
use chorale_core::{paste_tsv_into_range, to_clipboard_tsv};
use leptos::html;
use leptos::prelude::*;

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

/// Base64-encode raw bytes using the standard alphabet (A-Za-z0-9+/).
#[cfg(all(feature = "xlsx", target_arch = "wasm32"))]
fn to_base64(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(CHARS[((n >> 18) & 0x3F) as usize]));
        out.push(char::from(CHARS[((n >> 12) & 0x3F) as usize]));
        out.push(if chunk.len() > 1 {
            char::from(CHARS[((n >> 6) & 0x3F) as usize])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(CHARS[(n & 0x3F) as usize])
        } else {
            '='
        });
    }
    out
}

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
                    let opts = XlsxOptions { sheet_name: sheet_name.clone(), ..Default::default() };
                    let Ok(bytes) = to_xlsx(&state, &opts) else { return };
                    let b64 = to_base64(&bytes);
                    let href = format!(
                        "data:application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;base64,{b64}"
                    );
                    let Some(window) = web_sys::window() else { return };
                    let Some(document) = window.document() else { return };
                    let Ok(el) = document.create_element("a") else { return };
                    use wasm_bindgen::JsCast as _;
                    let Ok(a) = el.dyn_into::<web_sys::HtmlAnchorElement>() else { return };
                    a.set_href(&href);
                    a.set_download(&filename);
                    let _ = document.body().map(|b| b.append_child(&a));
                    a.click();
                    let _ = document.body().map(|b| b.remove_child(&a));
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

fn compute_window_slice<TRow: Clone>(
    state: &TableState<TRow>,
    view: &[RenderRow<TRow>],
) -> (VirtualWindow, Vec<RenderRow<TRow>>) {
    let total = view.len();
    let win = visible_window(
        state.scroll_top,
        state.viewport_height,
        state.row_height,
        total,
        state.buffer_rows,
    );
    if total == 0 {
        return (win, vec![]);
    }
    let win_end = win.end_index.min(total.saturating_sub(1));
    let slice = view[win.start_index..=win_end].to_vec();
    (win, slice)
}

// ---------------------------------------------------------------------------
// Badge and currency helpers
// ---------------------------------------------------------------------------

fn badge_style(color: &str) -> String {
    let (bg, fg) = match color {
        "green" => ("#d1fae5", "#065f46"),
        "yellow" => ("#fef3c7", "#92400e"),
        "red" => ("#fee2e2", "#991b1b"),
        "gray" => ("#f3f4f6", "#374151"),
        _ => ("#e5e7eb", "#1f2937"),
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
                CellValue::Integer(i) => format!("{symbol}{i}"),
                _ => format!("{symbol}{}", val.to_csv_string()),
            };
            view! { <span>{text}</span> }.into_any()
        }
        _ => {
            let text = cell_text(val);
            view! { <span>{text}</span> }.into_any()
        }
    }
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
                style="width:4rem;padding:0.125rem 0.25rem;border:1px solid #ddd;\
                       border-radius:3px;font-size:0.875rem;"
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
        <div style="padding: 0.5rem 1rem; border-bottom: 1px solid #ddd; \
                    display: flex; flex-wrap: wrap; gap: 0.5rem; align-items: center; \
                    font-size: 0.875rem; background: #fafafa;">
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
) -> AnyView {
    let val = match current {
        Some(FilterValue::Text(s)) => s.clone(),
        _ => String::new(),
    };
    let placeholder = placeholder.to_owned();
    view! {
        <input
            type="text"
            value=val
            placeholder=placeholder
            style="width: 100%; padding: 0.25rem; border: 1px solid #ddd; \
                   border-radius: 3px; font-size: 0.8rem; box-sizing: border-box;"
            on:input=move |ev| {
                let v = event_target_value(&ev);
                if v.is_empty() {
                    handle.set_filter(col_id, None);
                } else {
                    handle.set_filter(col_id, Some(FilterValue::Text(v)));
                }
            }
        />
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
            style="width:100%;padding:0.25rem;border:1px solid #ddd;\
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

fn numeric_range_filter<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
) -> AnyView {
    let (min_s, max_s) = match current {
        Some(FilterValue::NumericRange { min, max }) => (
            min.map(|v| v.to_string()).unwrap_or_default(),
            max.map(|v| v.to_string()).unwrap_or_default(),
        ),
        _ => (String::new(), String::new()),
    };

    view! {
        <div style="display:flex;gap:2px;">
            <input
                type="number"
                value=min_s
                placeholder="Min"
                style="width:50%;padding:0.25rem;border:1px solid #ddd;\
                       border-radius:3px;font-size:0.8rem;"
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    let new_min: Option<f64> = v.trim().parse().ok();
                    let cur = handle.signal().with_untracked(|s| s.filters.get(&col_id).cloned());
                    let cur_max = match cur {
                        Some(FilterValue::NumericRange { max, .. }) => max,
                        _ => None,
                    };
                    let filter = if new_min.is_none() && cur_max.is_none() {
                        None
                    } else {
                        Some(FilterValue::NumericRange { min: new_min, max: cur_max })
                    };
                    handle.set_filter(col_id, filter);
                }
            />
            <input
                type="number"
                value=max_s
                placeholder="Max"
                style="width:50%;padding:0.25rem;border:1px solid #ddd;\
                       border-radius:3px;font-size:0.8rem;"
                on:input=move |ev| {
                    let v = event_target_value(&ev);
                    let new_max: Option<f64> = v.trim().parse().ok();
                    let cur = handle.signal().with_untracked(|s| s.filters.get(&col_id).cloned());
                    let cur_min = match cur {
                        Some(FilterValue::NumericRange { min, .. }) => min,
                        _ => None,
                    };
                    let filter = if cur_min.is_none() && new_max.is_none() {
                        None
                    } else {
                        Some(FilterValue::NumericRange { min: cur_min, max: new_max })
                    };
                    handle.set_filter(col_id, filter);
                }
            />
        </div>
    }
    .into_any()
}

fn multiselect_filter<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    options: &[&str],
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
) -> AnyView {
    let selected: HashSet<String> = match current {
        Some(FilterValue::MultiSelect(v)) => v.iter().cloned().collect(),
        _ => HashSet::new(),
    };
    let is_open = RwSignal::new(false);
    let options: Vec<String> = options.iter().map(|s| (*s).to_owned()).collect();

    let _ = window_event_listener(leptos::ev::click, move |_| {
        is_open.set(false);
    });

    let count_label = if selected.is_empty() {
        "All".to_owned()
    } else {
        format!("{} selected", selected.len())
    };

    view! {
        <div style="position:relative;" on:click=move |ev| { ev.stop_propagation(); }>
            <button
                style="width:100%;padding:0.2rem 0.4rem;border:1px solid #ddd;\
                       border-radius:3px;font-size:0.8rem;text-align:left;cursor:pointer;\
                       background:white;"
                on:click=move |ev| {
                    ev.stop_propagation();
                    is_open.update(|v| *v = !*v);
                }
            >
                {count_label}
            </button>
            <Show when=move || is_open.get()>
                // z-index must be high enough to win against the table's
                // sticky-header cells (which create stacking contexts at
                // z-index: 1) AND against any frozen-column body cells
                // (which use frozen_column_z_index, default 2). 9999
                // guarantees the dropdown floats above the entire table.
                <div style="position:absolute;top:100%;left:0;z-index:9999;\
                             background:white;border:1px solid #ddd;border-radius:3px;\
                             padding:0.25rem;min-width:8rem;max-height:200px;\
                             overflow-y:auto;box-shadow:0 2px 8px rgba(0,0,0,0.15);">
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
    }
    .into_any()
}

fn date_range_filter<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col_id: ColumnId,
    current: Option<&FilterValue>,
    handle: UseTableHandle<TRow>,
) -> AnyView {
    let (min_s, max_s) = match current {
        Some(FilterValue::DateRange { min, max }) => (
            min.map(|d| d.to_string()).unwrap_or_default(),
            max.map(|d| d.to_string()).unwrap_or_default(),
        ),
        _ => (String::new(), String::new()),
    };

    view! {
        <div style="display:flex;gap:2px;">
            <input
                type="date"
                value=min_s
                style="width:50%;padding:0.25rem;border:1px solid #ddd;\
                       border-radius:3px;font-size:0.8rem;"
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    let new_min: Option<NaiveDate> =
                        v.parse().ok();
                    let cur = handle.signal().with_untracked(|s|
                        s.filters.get(&col_id).cloned()
                    );
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
                style="width:50%;padding:0.25rem;border:1px solid #ddd;\
                       border-radius:3px;font-size:0.8rem;"
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    let new_max: Option<NaiveDate> = v.parse().ok();
                    let cur = handle.signal().with_untracked(|s|
                        s.filters.get(&col_id).cloned()
                    );
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
        "outline: 2px dashed #4a90e2; outline-offset: -2px; "
    } else {
        ""
    };

    // Emit explicit values in both branches so Leptos reactive attr diff
    // always performs a concrete swap rather than dropping the declaration.
    let sticky_top_decl = if sticky_header {
        "position:sticky;top:0;z-index:1;"
    } else {
        "position:static;top:auto;z-index:auto;"
    };
    let sticky_css = sticky_css.to_owned();
    view! {
        <th
            style=format!(
                "cursor:{drag_cursor};padding:0.5rem 1rem;border-bottom:1px solid #ddd;\
                 text-align:{align};white-space:nowrap;overflow:hidden;\
                 text-overflow:ellipsis;background:#f8f9fa;\
                 {sticky_top_decl}{w}{sticky_css}{drag_over_style}"
            )
            draggable=column_reorder_enabled
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
            on:dragstart=move |_| {
                if column_reorder_enabled {
                    drag_col_id.set(Some(col_id));
                }
            }
            on:dragover=move |ev| {
                if column_reorder_enabled {
                    ev.prevent_default();
                }
            }
            on:drop=move |_| {
                if column_reorder_enabled {
                    if let Some(src_id) = drag_col_id.get_untracked() {
                        if src_id != col_id {
                            let new_order = handle.signal().with_untracked(|s| {
                                let order: Vec<ColumnId> = s.column_order.clone();
                                if let Some(to_idx) = order.iter().position(|&id| id == col_id) {
                                    let mut o = order;
                                    if let Some(from_idx) = o.iter().position(|&id| id == src_id) {
                                        o.remove(from_idx);
                                        let insert_at = if from_idx < to_idx {
                                            to_idx.saturating_sub(1)
                                        } else {
                                            to_idx
                                        };
                                        o.insert(insert_at, src_id);
                                    }
                                    Some(o)
                                } else {
                                    None
                                }
                            });
                            if let Some(order) = new_order {
                                handle.signal().update(|s| s.column_order = order);
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
                <sup style="font-size:0.65rem;color:#4a90e2;margin-left:0.1rem;">{b}</sup>
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

fn filter_th<TRow: Clone + PartialEq + Send + Sync + 'static>(
    col: &ColumnDef<TRow>,
    override_width: Option<f64>,
    handle: UseTableHandle<TRow>,
    filters: &HashMap<ColumnId, FilterValue>,
    labels: &Labels,
    sticky_css: &str,
) -> AnyView {
    let w = col_width_style(override_width, col.initial_width);
    let col_id = col.id;
    let current = filters.get(&col_id);
    let sticky_css = sticky_css.to_owned();

    let inner = match &col.filter {
        FilterKind::None => view! { <span /> }.into_any(),
        FilterKind::Text => text_filter_input(col_id, current, handle, &labels.filter_placeholder),
        FilterKind::Boolean => boolean_filter_input(col_id, current, handle),
        FilterKind::NumericRange { .. } => numeric_range_filter(col_id, current, handle),
        FilterKind::DateRange => date_range_filter(col_id, current, handle),
        FilterKind::MultiSelect { options } => {
            let opts: Vec<&str> = options.iter().map(String::as_str).collect();
            multiselect_filter(col_id, &opts, current, handle)
        }
        _ => view! { <span /> }.into_any(),
    };

    view! {
        <th style=format!(
            "padding:0.25rem;border-bottom:1px solid #eee;position:sticky;top:0;\
             background:#fff;{w}{sticky_css}"
        )>
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
    cell_renderers: &CellRenderers,
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
) -> AnyView {
    let val = (col.accessor)(row);
    let col_id = col.id;
    let align = alignment_css(col.alignment);
    let w = col_width_style(override_width, col.initial_width);
    let sticky_css = sticky_css.to_owned();
    let is_editing = editing_col == Some(col_id);
    let render_kind = col.render_kind.clone();
    let renderer = cell_renderers.get(col_id);
    let validate_fn = validate_fn.clone();
    let _on_commit_edit = on_commit_edit.copied();
    let _row_clone = row.clone();

    // Active cell outline and range background (placed after sticky_css to override frozen bg).
    let active_css = if is_active_cell {
        "outline:2px solid var(--chorale-active-cell-outline,#0078d4);outline-offset:-2px;"
    } else {
        ""
    };
    let range_css = if is_in_range && !is_active_cell {
        "background:rgba(0,120,212,0.1);"
    } else {
        ""
    };

    if is_editing {
        if let Some(EditorKind::Text) = &col.editor {
            {
                return view! {
                    <td style=format!(
                        "padding:0;border-bottom:1px solid #eee;\
                         text-align:{align};height:{row_height}px;\
                         overflow:hidden;{w}{sticky_css}"
                    )>
                        <div style="display:flex;flex-direction:column;height:100%;">
                            <input
                                type="text"
                                value=move || editing_text.get()
                                style="flex:1;width:100%;padding:0.25rem;border:none;\
                                       outline:2px solid #4a90e2;font-size:0.875rem;"
                                on:input=move |ev| {
                                    editing_text.set(event_target_value(&ev));
                                    edit_error.set(None);
                                }
                                on:keydown=move |ev| {
                                    let key = ev.key();
                                    if key == "Escape" {
                                        // cancel handled by parent
                                    } else if key == "Enter" || key == "Tab" {
                                        let text = editing_text.get_untracked();
                                        let validation = EditValidation {
                                            row_id,
                                            column_id: col_id,
                                            raw_value: text.clone(),
                                        };
                                        match validate_fn.call(validation) {
                                            Ok(()) => {
                                                // Commit is handled via the signal externally
                                                let _ = text;
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
                                <span style="font-size:0.7rem;color:#dc2626;padding:0 0.25rem;">
                                    {e}
                                </span>
                            })}
                        </div>
                    </td>
                }
                .into_any();
            }
        }
    }

    let cell_content = render_cell_value(&val, &render_kind, renderer.as_ref());
    view! {
        <td
            style=format!(
                "padding:0.5rem 1rem;border-bottom:1px solid #eee;\
                 text-align:{align};height:{row_height}px;overflow:hidden;\
                 white-space:nowrap;text-overflow:ellipsis;cursor:default;\
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
                           background: #0078d4; cursor: crosshair; z-index: 10;"
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
    selection_enabled: bool,
    is_selected: bool,
    handle: UseTableHandle<TRow>,
    cell_renderers: &CellRenderers,
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
) -> AnyView {
    let bg = if is_selected { "#eff6ff" } else { "white" };
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
                cell_renderers,
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
            )
        })
        .collect();

    view! {
        <tr
            data-chorale-index=row_index.to_string()
            style=format!(
                "background:{bg};cursor:default;\
                 box-shadow:inset 0 -1px 0 #eee;"
            )
            on:click=move |_| {
                if selection_enabled {
                    handle.set_selection(row_id, !is_selected);
                }
            }
        >
            {if selection_enabled {
                Some(view! {
                    <td style="padding:0.5rem;border-bottom:1px solid #eee;width:2.5rem;">
                        <input
                            type="checkbox"
                            checked=is_selected
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
                               border-bottom:1px solid #eee;"
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
            style="background:#f0f4ff;cursor:pointer;"
            on:click=move |_| { handle.toggle_group(key.clone()); }
        >
            <td
                colspan=effective_col_count.to_string()
                style=format!(
                    "padding:0.4rem 1rem 0.4rem {indent};\
                     border-bottom:1px solid #d1d5db;\
                     font-weight:600;font-size:0.875rem;"
                )
            >
                <span style="margin-right:0.5rem;">{icon}</span>
                {label}
                <span style="margin-left:0.5rem;font-size:0.75rem;\
                              font-weight:normal;color:#6b7280;">
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
                 background:#fff;{divider}"
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
                 background:#fff;{divider}"
            ),
        );
        right_off += w;
    }

    (header_css, body_css)
}

// ---------------------------------------------------------------------------
// CSV download helper (WASM only)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
fn trigger_csv_download(csv: String) {
    use js_sys;
    use wasm_bindgen::JsCast;
    leptos::task::spawn_local(async move {
        let array = js_sys::Array::new();
        array.push(&wasm_bindgen::JsValue::from_str(&csv));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("text/csv;charset=utf-8;");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&array, &options);
        if let Ok(blob) = blob {
            if let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) {
                if let Some(window) = web_sys::window() {
                    if let Some(doc) = window.document() {
                        let anchor = doc.create_element("a");
                        if let Ok(anchor) = anchor {
                            let anchor: web_sys::HtmlAnchorElement = anchor.unchecked_into();
                            anchor.set_href(&url);
                            anchor.set_download("chorale-export.csv");
                            if let Some(body) = doc.body() {
                                body.append_child(&anchor).ok();
                                anchor.click();
                                body.remove_child(&anchor).ok();
                            }
                            web_sys::Url::revoke_object_url(&url).ok();
                        }
                    }
                }
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn trigger_csv_download(_csv: String) {}

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
#[allow(clippy::too_many_lines, clippy::fn_params_excessive_bools)]
#[component]
pub fn Table<TRow>(
    handle: UseTableHandle<TRow>,
    #[prop(default = true)] sort_enabled: bool,
    #[prop(default = false)] filter_enabled: bool,
    #[prop(default = false)] selection_enabled: bool,
    #[prop(default = CellRenderers::default())] cell_renderers: CellRenderers,
    #[prop(default = false)] column_toolbar: bool,
    #[prop(default = false)] csv_export: bool,
    #[prop(default = false)] xlsx_export: bool,
    #[prop(default = false)] resize_enabled: bool,
    #[prop(default = ValidateEditFn::default())] validate_edit: ValidateEditFn,
    on_commit_edit: Option<Callback<CommittedEdit<TRow>>>,
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
    /// Per CHANGELOG Item N (master/detail, MD-B).
    #[prop(optional)]
    detail_renderer: Option<DetailRenderer<TRow>>,
    #[prop(optional)] labels: Option<Labels>,
    #[prop(default = false)] column_reorder_enabled: bool,
    /// When `true` (default), the header row sticks to the top of the scroll
    /// container. Set `false` to let it scroll with the body.
    #[prop(default = true)]
    sticky_header: bool,
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
) -> impl IntoView
where
    TRow: Clone + PartialEq + Send + Sync + 'static,
{
    let labels = Arc::new(labels.unwrap_or_default());

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

    view! {
        <div
            node_ref=kb_ref
            tabindex="0"
            style="border:1px solid #ddd;border-radius:4px;overflow:hidden;user-select:none;outline:none;"
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
                        } else {
                            let new_s = handle.signal.with_untracked(|s| move_active_cell(s, dir));
                            handle.signal.set(new_s);
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
                    }
                    "End" => {
                        ev.prevent_default();
                        let new_s = if ctrl {
                            handle.signal.with_untracked(move_active_cell_last)
                        } else {
                            handle.signal.with_untracked(move_active_cell_end)
                        };
                        handle.signal.set(new_s);
                    }
                    "PageUp" => {
                        ev.prevent_default();
                        let page_sz = handle.signal.with_untracked(|s| s.page_size);
                        let new_s = handle.signal.with_untracked(|s| move_active_cell_page(s, NavDirection::Up, page_sz));
                        handle.signal.set(new_s);
                    }
                    "PageDown" => {
                        ev.prevent_default();
                        let page_sz = handle.signal.with_untracked(|s| s.page_size);
                        let new_s = handle.signal.with_untracked(|s| move_active_cell_page(s, NavDirection::Down, page_sz));
                        handle.signal.set(new_s);
                    }
                    "Escape" => {
                        let new_s = handle.signal.with_untracked(|s| {
                            let s2 = clear_range_selection(s);
                            clear_active_cell(&s2)
                        });
                        handle.signal.set(new_s);
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
                                                .and_then(|w| Some(w.navigator().clipboard()))
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
                            let clipboard = web_sys::window()
                                .and_then(|w| Some(w.navigator().clipboard()));
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
            {selection_toolbar.as_ref().map(|slot| {
                view! {
                    <div
                        class="chorale-selection-toolbar"
                        style="width:100%;box-sizing:border-box;\
                               border-bottom:2px solid #1d4ed8;"
                    >
                        {slot()}
                    </div>
                }
                .into_any()
            })}

            // Virtualized scroll container
            <div
                node_ref=scroll_ref
                style=move || {
                    let h = sig.with(|s| s.viewport_height);
                    format!(
                        "overflow-y:auto;overflow-x:auto;overflow-anchor:none;\
                         height:{h}px;"
                    )
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

                        let (win, render_slice) =
                            compute_window_slice(&s, &vis);

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
                                        )
                                    })
                                    .collect(),
                            )
                        } else {
                            None
                        };

                        // ---- tbody rows ----
                        let tbody_content: Vec<AnyView> = if is_grouped {
                            grouped_visible
                                .get()
                                .into_iter()
                                .enumerate()
                                .map(|(i, grouped_row)| match grouped_row {
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
                                        selection_enabled,
                                        selection_set.contains(&row_id),
                                        handle,
                                        &cell_renderers,
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
                                    ),
                                    _ => view! { <tr /> }.into_any(),
                                })
                                .collect()
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
                                            selection_enabled,
                                            selection_set.contains(&row_id),
                                            handle,
                                            &cell_renderers,
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
                                        ));
                                    }
                                    RenderRow::DetailPanel { parent_row_id } => {
                                        let pid = *parent_row_id;
                                        let parent = s.rows.iter()
                                            .find(|(rid, _)| *rid == pid)
                                            .map(|(_, r)| r.clone());
                                        let colspan = effective_col_count;
                                        let view = match (parent, &detail_renderer) {
                                            (Some(prow), Some(renderer)) => {
                                                let content = renderer(prow);
                                                view! {
                                                    <tr class="chorale-row chorale-detail-panel">
                                                        <td colspan=colspan.to_string()>
                                                            <div class="chorale-detail-panel-inner">
                                                                {content}
                                                            </div>
                                                        </td>
                                                    </tr>
                                                }.into_any()
                                            }
                                            _ => view! {
                                                <tr class="chorale-row chorale-detail-panel-empty" />
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
                                                   color:#999;font-style:italic;"
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
                                        "padding:0.5rem;border-bottom:1px solid #ddd;\
                                         background:#f8f9fa;width:2.5rem;{sel_sticky}"
                                    )>
                                        <input
                                            type="checkbox"
                                            checked=all_page_selected
                                            on:change=move |_| { handle.toggle_select_all(); }
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
                                    "width:24px;padding:0;border-bottom:1px solid #ddd;\
                                     background:#f8f9fa;{chev_sticky}"
                                ) />
                            })
                        } else {
                            None
                        };

                        let filter_empty_th = if selection_enabled && filter_enabled {
                            Some(view! {
                                <th style="padding:0.25rem;border-bottom:1px solid #eee;\
                                           background:#fff;width:2.5rem;" />
                            })
                        } else {
                            None
                        };

                        let filter_chevron_th = if has_detail && filter_enabled {
                            Some(view! {
                                <th style="width:24px;padding:0;border-bottom:1px solid #eee;background:#fff;" />
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
                                                border-top:1px solid #ddd;background:#fafafa;\
                                                font-size:0.875rem;color:#999;">
                                        {lbl}
                                    </div>
                                }.into_any()
                            } else {
                                view! { <div /> }.into_any()
                            }
                        } else if !is_virtualized_grouped {
                            let nav_btn = "padding:0.25rem 0.6rem;border:1px solid #ddd;\
                                border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                background:white;color:#333;";
                            let nav_btn_dis = "padding:0.25rem 0.6rem;border:1px solid #ddd;\
                                border-radius:3px;font-size:0.875rem;cursor:not-allowed;\
                                background:#f0f0f0;color:#aaa;";
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
                                            "padding:0.25rem 0.6rem;border:1px solid #4a90e2;\
                                             border-radius:3px;font-size:0.875rem;\
                                             background:#4a90e2;color:white;cursor:pointer;"
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
                                            <span style="padding:0.25rem 0.3rem;color:#999;">
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
                                        style="padding:0.25rem 0.75rem;border:1px solid #4a90e2;\
                                               border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                               background:white;color:#4a90e2;"
                                        on:click=move |_| {
                                            let csv = sig.with_untracked(|s| to_csv(s));
                                            trigger_csv_download(csv);
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
                                            flex-wrap:wrap;gap:0.25rem;border-top:1px solid #ddd;\
                                            background:#fafafa;font-size:0.875rem;color:#555;">
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
                                    <span style="margin-left:0.5rem;color:#999;">{"\u{00b7}"}</span>
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
                                               color:#999;font-style:italic;"
                                    >{lbl}</td>
                                </tr>
                            })
                        } else {
                            None
                        };

                        view! {
                            <thead>
                                <tr style="background:#f8f9fa;">
                                    {select_all_th}
                                    {chevron_th}
                                    {header_row}
                                </tr>
                                {filter_row.map(|cells| view! {
                                    <tr style="background:#fff;">
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
                                    border-top:1px solid #ddd;background:#fafafa;\
                                    font-size:0.875rem;color:#999;">
                            {lbl}
                        </div>
                    }.into_any()
                } else if !is_infinite && !is_virt_grouped {
                    let nav_btn = "padding:0.25rem 0.6rem;border:1px solid #ddd;\
                        border-radius:3px;font-size:0.875rem;cursor:pointer;\
                        background:white;color:#333;";
                    let nav_btn_dis = "padding:0.25rem 0.6rem;border:1px solid #ddd;\
                        border-radius:3px;font-size:0.875rem;cursor:not-allowed;\
                        background:#f0f0f0;color:#aaa;";
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
                                    "padding:0.25rem 0.6rem;border:1px solid #4a90e2;\
                                     border-radius:3px;font-size:0.875rem;\
                                     background:#4a90e2;color:white;cursor:pointer;"
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
                                view! { <span style="padding:0.25rem 0.3rem;color:#999;">"…"</span> }.into_any()
                            }
                        })
                        .collect();

                    let csv_button: Option<AnyView> = if csv_export {
                        let lbl_csv = labels.export_csv_label.clone();
                        Some(view! {
                            <button
                                style="padding:0.25rem 0.75rem;border:1px solid #4a90e2;\
                                       border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                       background:white;color:#4a90e2;"
                                on:click=move |_| {
                                    let csv = sig.with_untracked(|s| to_csv(s));
                                    trigger_csv_download(csv);
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
                                    style="padding:0.25rem 0.75rem;border:1px solid #4a90e2;\
                                           border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                           background:white;color:#4a90e2;"
                                    on:click=move |_| {
                                        #[cfg(target_arch = "wasm32")]
                                        {
                                            use chorale_core::{XlsxOptions, to_xlsx};
                                            let state = sig.with_untracked(|s| s.clone());
                                            let Ok(bytes) = to_xlsx(&state, &XlsxOptions::default()) else { return };
                                            let b64 = to_base64(&bytes);
                                            let href = format!(
                                                "data:application/vnd.openxmlformats-officedocument.\
                                                 spreadsheetml.sheet;base64,{b64}"
                                            );
                                            let Some(window) = web_sys::window() else { return };
                                            let Some(document) = window.document() else { return };
                                            let Ok(el) = document.create_element("a") else { return };
                                            use wasm_bindgen::JsCast as _;
                                            let Ok(a) = el.dyn_into::<web_sys::HtmlAnchorElement>() else { return };
                                            a.set_href(&href);
                                            a.set_download("export.xlsx");
                                            let _ = document.body().map(|b| b.append_child(&a));
                                            a.click();
                                            let _ = document.body().map(|b| b.remove_child(&a));
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
                                    flex-wrap:wrap;gap:0.25rem;border-top:1px solid #ddd;\
                                    background:#fafafa;font-size:0.875rem;color:#555;">
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
                            <span style="margin-left:0.5rem;color:#999;">{"\u{00b7}"}</span>
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
