//! Dioxus components for chorale tables.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chorale_core::{
    add_disjoint_range, batch_record_row_heights, cancel_edit, clear_active_cell,
    clear_range_selection, commit_edit, extend_range_to, fill_handle_targets, frozen_left_columns,
    frozen_right_columns, move_active_cell, move_active_cell_end, move_active_cell_first,
    move_active_cell_home, move_active_cell_last, move_active_cell_page, move_active_cell_to_edge,
    next_editable_cell, paste_tsv_into_range, prev_editable_cell, scrollable_columns,
    select_all as select_all_range, start_range_selection, theme_stylesheet, to_clipboard_tsv,
    to_csv, visible_grouped_view, visible_view, visible_window, visible_window_variable,
    ActiveCell, Alignment, BadgeVariantMap, CellValue, ClipboardCopyEvent, ClipboardPasteEvent,
    ColumnDef, ColumnId, CommittedEdit, CurrencyCode, EditorKind, FilterKind, FilterValue,
    GroupKey, GroupedPaginationMode, GroupedRow, Labels, NavDirection, PaginationMode, RenderKind,
    RenderRow, RowId, SortAction, SortDirection, SortState, TableState, Theme, VirtualWindow,
    THEME_ROOT_CLASS,
};
use dioxus::prelude::*;

use crate::hooks::UseTableHandle;

/// Type-erased cell renderer: maps a [`CellValue`] to a rendered [`Element`].
pub type CellRenderer = Arc<dyn Fn(&CellValue) -> Element + Send + Sync + 'static>;

/// Per-process counter for scroll-container DOM ids. Each mounted `<Table>`
/// gets a unique id so a `use_effect` reset can target the right element.
static SCROLL_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

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
/// [`CellValue`] to a rendered [`Element`].
///
/// Use this instead of [`CellRenderer`] when the cell needs data from other
/// fields on the row: composite cells (avatar + name), action columns that
/// need the row's id, link cells that build an href from a sibling field.
/// Build with `Arc::new(|row: &MyRow, val: &CellValue| rsx! { ... })` and
/// register via [`RowCellRenderers::new`].
pub type RowCellRenderer<TRow> = Arc<dyn Fn(&TRow, &CellValue) -> Element + Send + Sync + 'static>;

/// Per-column map of row-aware cell renderers; default is empty.
///
/// Entries here take precedence over [`CellRenderers`] entries, which take
/// precedence over the column's `RenderKind`. Compared by pointer identity
/// for prop diffing (rebuild the map only when the renderers change).
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
///
/// Return `Ok(())` to allow the commit, or `Err(msg)` to show `msg` as an inline
/// validation error and keep the editor open.
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

/// Encode `s` as a JSON string literal suitable for embedding in a JavaScript expression.
///
/// Wraps the value in double-quotes and escapes special characters so the result
/// is safe to pass directly to `navigator.clipboard.writeText(...)`.
fn js_string_literal(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Base64-encode raw bytes using the standard alphabet (A-Za-z0-9+/).
#[cfg(feature = "xlsx")]
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
/// Requires the `xlsx` feature on both `chorale-dioxus` and `chorale-core`.
/// On click, calls [`chorale_core::to_xlsx`] and triggers a browser download
/// via `document.createElement('a')` in a `dioxus::document::eval` script.
#[cfg(feature = "xlsx")]
#[component]
pub fn ExportXlsxButton<TRow: Clone + 'static>(
    /// Table handle providing access to the current state.
    handle: UseTableHandle<TRow>,
    /// Sheet tab name written into the workbook. Defaults to `"Sheet1"`.
    #[props(default = String::from("Sheet1"))]
    sheet_name: String,
    /// File name the browser prompts with. Defaults to `"export.xlsx"`.
    #[props(default = String::from("export.xlsx"))]
    filename: String,
    /// Button label / child elements.
    children: Element,
) -> Element {
    use chorale_core::{to_xlsx, XlsxOptions};

    let onclick = move |_: Event<MouseData>| {
        let sig = handle.signal();
        let state = sig.peek();
        let mut opts = XlsxOptions::default();
        opts.sheet_name.clone_from(&sheet_name);
        let Ok(bytes) = to_xlsx(&*state, &opts) else {
            return;
        };
        let b64 = to_base64(&bytes);
        let dl = js_string_literal(&filename);
        // atob → Uint8Array → Blob → object URL → anchor click
        let js = format!(
            r"(()=>{{
                var r=atob('{b64}'),n=r.length,u=new Uint8Array(n);
                for(var i=0;i<n;i++)u[i]=r.charCodeAt(i);
                var bl=new Blob([u],{{type:'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'}});
                var url=URL.createObjectURL(bl),a=document.createElement('a');
                a.href=url;a.download={dl};a.click();
                setTimeout(()=>URL.revokeObjectURL(url),100);
            }})()"
        );
        dioxus::document::eval(&js);
    };

    rsx! {
        button { onclick, {children} }
    }
}

/// The primary chorale Dioxus table component.
///
/// Renders column headers, an optional filter row, virtualized data rows,
/// pagination controls, and optional selection checkboxes. All features are
/// opt-in via props; the minimal form shows a read-only sorted table.
/// Per-prop docs live on each field below.
#[component]
pub fn Table<TRow: Clone + PartialEq + 'static>(
    /// Required. Reactive handle returned by `use_table`.
    handle: UseTableHandle<TRow>,
    /// Show sort-direction arrows and make sortable headers clickable.
    /// Setting this to `false` renders headers as plain text without
    /// clearing any existing sort state.
    #[props(default = true)]
    sort_enabled: bool,
    /// Show a filter input row below the column headers. Each column
    /// renders its `FilterKind` UI: text input, numeric range, date
    /// range, multi-select dropdown, or boolean radio group.
    #[props(default = false)]
    filter_enabled: bool,
    /// Show a checkbox column on the left. The header checkbox toggles
    /// selection for all visible rows on the current page. Read the
    /// selection via `handle.signal().read().selection`.
    #[props(default = false)]
    selection_enabled: bool,
    /// Per-column custom renderers that override the column's `RenderKind`.
    /// Pass `CellRenderers::new(map)` with a `HashMap<ColumnId, CellRenderer>`.
    #[props(default)]
    cell_renderers: CellRenderers,
    /// Per-column custom renderers that receive the **full row** plus the
    /// cell value (`Fn(&TRow, &CellValue) -> Element`). Entries here take
    /// precedence over `cell_renderers` and the column's `RenderKind`.
    /// Pass `RowCellRenderers::new(map)` with a
    /// `HashMap<ColumnId, RowCellRenderer<TRow>>`. Default: empty.
    #[props(default)]
    row_cell_renderers: RowCellRenderers<TRow>,
    /// Show a column visibility toolbar above the table. Each column gets
    /// a toggle checkbox.
    #[props(default = false)]
    column_toolbar: bool,
    /// Show an "Export CSV" button in the pagination footer. Exports the
    /// full post-filter / post-sort dataset (not just the current page) in
    /// RFC 4180 format.
    #[props(default = false)]
    csv_export: bool,
    /// Show an "Export Excel" button next to the CSV export in the pagination
    /// footer. Exports the full post-filter / post-sort dataset as an .xlsx
    /// workbook with the active filters baked in. **No-op unless the
    /// `xlsx` Cargo feature is enabled on `chorale-dioxus` (which
    /// transitively enables `chorale-core/xlsx`).** Without that feature
    /// the prop compiles fine but the button does not render.
    #[props(default = false)]
    xlsx_export: bool,
    /// Show drag handles on column header borders. Dragging adjusts the
    /// `column_widths` map in the table state.
    #[props(default = false)]
    resize_enabled: bool,
    /// When `true`, the header row stays pinned to the top of the scroll
    /// container as the user scrolls vertically (`position: sticky; top: 0`
    /// on every header TH). When `false`, the header scrolls with the body.
    /// Frozen columns retain their left/right stickiness regardless of this
    /// flag; this controls only the vertical sticky of the header row.
    /// Default: `true` (matches standard data-grid UX).
    #[props(default = true)]
    sticky_header: bool,
    /// Enable variable-row-height virtualization (VIRT-2). When `true`, the
    /// component measures each rendered row's height after mount via a DOM
    /// eval and caches it in `state.row_heights`. The `row_height` field on
    /// `TableState` is used as the fallback for unmeasured rows. Web target
    /// only.
    #[props(default = false)]
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
    #[props(default = false)]
    inline: bool,
    /// Optional synchronous validator called before a cell edit is
    /// committed. Return `Ok(())` to allow, `Err(msg)` to display `msg` as
    /// an inline error and leave the editor open.
    #[props(default)]
    validate_edit: ValidateEditFn,
    /// Fired after a cell edit successfully commits. Receives the new raw
    /// value and a `PriorEdit` snapshot suitable for rollback.
    on_commit_edit: Option<EventHandler<CommittedEdit<TRow>>>,
    /// Optional bulk-action toolbar slot rendered above the table whenever
    /// it is `Some`, regardless of selection size. Include affordances like
    /// "Select all" that are useful in the empty-selection state.
    /// Wrapped in `div.chorale-selection-toolbar`.
    #[props(default)]
    selection_toolbar: Option<Element>,
    /// Optional per-row detail renderer. When `Some`, a 24px chevron column
    /// is prepended; clicking it calls `toggle_row_expansion`. Detail rows
    /// render as `<tr><td colspan>` containing the returned `Element`.
    #[props(default)]
    detail_renderer: Option<Callback<TRow, Element>>,
    /// Fired with the row's [`RowId`] when a data row receives a plain
    /// (unmodified) left-click on one of its data cells. Not fired for:
    /// clicks on the selection checkbox or its cell, the detail-expander
    /// chevron, a cell currently in edit mode, group-header rows, or
    /// Ctrl/Cmd/Shift-modified clicks (those remain range-selection
    /// operations). Plain clicks still also update the active cell /
    /// range selection as before. Default `None` = behavior identical to
    /// previous versions.
    ///
    /// Note: a double-click that starts inline editing fires this handler
    /// for each constituent click first (standard data-grid behavior);
    /// avoid combining whole-row navigation with editable columns.
    #[props(default)]
    on_row_click: Option<Callback<RowId>>,
    /// Override every user-visible string (filter placeholder, pagination
    /// labels, export buttons, etc.). `None` uses English defaults.
    #[props(default)]
    labels: Option<Labels>,
    /// Show drag handles on column headers. Successful drops fire
    /// `move_column` on the handle and trigger `on_column_order_change`.
    #[props(default = false)]
    column_reorder_enabled: bool,
    /// Called with the new `column_order` vec after a successful column
    /// drag-and-drop.
    on_column_order_change: Option<EventHandler<Vec<ColumnId>>>,
    /// Fired when the Tab key moves focus to a cell whose column has `EditorKind` configured.
    /// The parent can use this to call `handle.start_edit(row_id, col_id)`.
    on_tab_to_editable: Option<EventHandler<ActiveCell>>,
    /// Fired after Ctrl+C successfully writes the selected range to the system clipboard.
    on_copy: Option<EventHandler<ClipboardCopyEvent>>,
    /// Fired after Ctrl+V reads from the system clipboard and adjusts the active range.
    /// The host should apply the per-cell writes from `evt.tsv` via its persistence layer.
    on_paste: Option<EventHandler<ClipboardPasteEvent>>,
    /// CSS `z-index` applied to frozen column cells (header, filter row, and body).
    /// Raise if custom cell renderers use `z-index` internally.
    /// Default is `2` (above scrollable columns, which use no explicit z-index).
    #[props(default = 2)]
    frozen_column_z_index: i32,
    /// CSS class applied to every group-header `<tr>` when grouping is active.
    /// Default: `"chorale-group-header"`.
    #[props(default = String::from("chorale-group-header"))]
    group_header_class: String,
    /// Distance from the scroll container bottom (px) at which to fire
    /// `load_more_rows` in `PaginationMode::InfiniteScroll`. Default is `200`.
    #[props(default = 200.0_f64)]
    infinite_scroll_threshold_px: f64,
    /// Visual theme applied to this table. `Theme::Light` (the default) and
    /// `Theme::Dark` resolve against the built-in injected stylesheet;
    /// `Theme::Custom` matches no built-in token block, so every
    /// `var(--chorale-*, <fallback>)` reference resolves to the consumer's
    /// own CSS-variable definitions or the inline light fallback.
    #[props(default = Theme::Light)]
    theme: Theme,
) -> Element {
    let labels = labels.clone().unwrap_or_default();

    // Master/detail rows are inherently variable-height: the parent table
    // cannot virtualize correctly assuming uniform row_height when one of
    // its rows is a detail panel that's 5-20× taller. Force variable-height
    // measurement on whenever detail_renderer is set, so the parent's
    // row_heights map tracks each row's actual rendered height and scroll
    // math stays consistent with layout.
    //
    // This shadow MUST come before the VIRT-2 measurement use_effect (which
    // captures variable_row_height by move at hook-construction time) and
    // before any consumer of `variable_row_height` downstream.
    let has_detail = detail_renderer.is_some();
    let variable_row_height = variable_row_height || has_detail;

    // drag_state: Some((col_id, start_x_px, start_width_px)) while a resize is active.
    let mut drag_state: Signal<Option<(ColumnId, f64, f64)>> = use_signal(|| None);
    // drag_col_id: column being dragged for column-reorder (None when not reordering).
    let drag_col_id: Signal<Option<ColumnId>> = use_signal(|| None);
    // drag_over_col: the column currently under the cursor during a column-reorder drag.
    // Only this column shows the blue dashed drop-target outline.
    let drag_over_col: Signal<Option<ColumnId>> = use_signal(|| None);
    // In-cell editing state: current editor text and optional validation error.
    let mut editing_text: Signal<String> = use_signal(String::new);
    let mut edit_error: Signal<Option<String>> = use_signal(|| None);
    // Fill handle drag state.
    let mut fill_drag_active: Signal<bool> = use_signal(|| false);
    let mut fill_hover: Signal<Option<(usize, ColumnId)>> = use_signal(|| None);

    let sig = handle.signal();

    // Unique DOM id for the scroll container so a `use_effect` keyed on page
    // changes can reset the browser's `scrollTop` back to 0. Required because
    // `set_page` resets `state.scroll_top` to 0 in the reducer but the DOM's
    // native scrollTop is independent — without this reset, clicking "next
    // page" leaves the browser scrolled into what is now `bottom_pad` empty
    // space until the user scrolls, generating an `onscroll` that reconciles.
    let scroll_id = use_hook(|| {
        format!(
            "chorale-scroll-{}",
            SCROLL_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    });
    let kb_id = format!("{scroll_id}-kb");

    // PERF-1: Two-level memo to decouple the expensive pipeline from scroll.
    //
    // view_key tracks only the fields that affect visible_view output:
    // page, page_size, sort, filters, and row count. When scroll_top (or
    // viewport_height, row_height, column_widths, selection) changes, this
    // key runs but returns the same tuple → Dioxus PartialEq short-circuits
    // → the view memo does NOT re-run the filter+sort+paginate pipeline.
    //
    // At 1M rows this eliminates ~30 MB of allocation per scroll tick.
    //
    // Known limitation: update_row changes a row's value without changing
    // rows.len(), so view won't recompute immediately. The view re-syncs on
    // the next sort/filter/page transition. Cell editing is at most one
    // transition per user interaction, so this tradeoff is acceptable.
    let view_key = use_memo(move || {
        let s = sig.read();
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
            // state.rows but the view memo's PartialEq short-circuits
            // and the cached visible_view is returned forever, so the
            // cell continues rendering the pre-edit value until an
            // unrelated transition (sort/filter/page/expand) happens to
            // bump some other field.
            s.data_generation,
        )
    });
    // sig.peek() reads without subscribing this memo to sig directly;
    // the dependency flows through view_key only.
    let view = use_memo(move || {
        let _key = view_key.read();
        visible_view(&*sig.peek())
    });
    let grouped_view = use_memo(move || {
        let _key = view_key.read();
        visible_grouped_view(&*sig.peek())
    });

    // Memo over just the page index so `use_effect` re-runs only on page
    // transitions, not on every state change. set_page resets
    // state.scroll_top to 0 in the reducer; the effect reaches into the DOM
    // and snaps the native scrollTop to match — without it, the browser
    // would keep showing the previous page's scroll position and the user
    // would see blank `bottom_pad` until they manually scrolled.
    let page_memo = use_memo(move || sig.read().page);
    let scroll_id_for_effect = scroll_id.clone();
    use_effect(move || {
        let _p = page_memo.read();
        let id = scroll_id_for_effect.clone();
        dioxus::document::eval(&format!(
            "const el = document.getElementById('{id}'); if (el) el.scrollTop = 0;"
        ));
    });

    // Column reorder cleanup: when state.column_order changes (= a drop just
    // applied), force-clear drag_col_id + drag_over_col.
    //
    // The ondragend handler lives on the SOURCE <th>, but after a successful
    // drop the columns are re-rendered in a new arrangement, so the source
    // <th>'s element identity may not survive long enough to fire ondragend
    // reliably. Without this effect, drag_col_id stays set and the dashed
    // drop-target outline persists on whichever column ended up in the slot,
    // even though no drag is in progress. Watching column_order catches
    // every successful reorder and resets both signals.
    {
        let column_order_memo = use_memo(move || sig.read().column_order.clone());
        let mut drag_col_id_w = drag_col_id;
        let mut drag_over_col_w = drag_over_col;
        use_effect(move || {
            let _o = column_order_memo.read();
            drag_col_id_w.set(None);
            drag_over_col_w.set(None);
        });
    }

    // In-cell editing: reset editor text and error when the active cell changes.
    // Unconditional use_effect (Dioxus hook ordering rules); no-op when no edit target.
    {
        let edit_target_memo = use_memo(move || sig.read().editing);
        let kb_id_editor_focus = kb_id.clone();
        use_effect(move || {
            let target = *edit_target_memo.read();
            if let Some(target) = target {
                let state = sig.read();
                let init_text = state
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
                    .unwrap_or_default();
                editing_text.set(init_text);
                edit_error.set(None);
                // Auto-focus the freshly-mounted editor element (the
                // `data-chorale-editor` attribute is set on both the text
                // <input> and the <select> in editor_td). Load-bearing for
                // two behaviors:
                //   1. exit-on-blur — an unfocused element can never fire
                //      onblur, so without this a <select> cell could not be
                //      exited by clicking away / tabbing out;
                //   2. keyboard operability — Enter/F2 on the active cell
                //      must land keystrokes in the editor, not the grid.
                // requestAnimationFrame defers one frame so the editor's DOM
                // node exists even if this effect runs before the render
                // patch that mounts it has been applied.
                let id = kb_id_editor_focus.clone();
                dioxus::document::eval(&format!(
                    "requestAnimationFrame(()=>{{\
                        var el=document.querySelector('#{id} [data-chorale-editor]');\
                        if(el)el.focus();\
                    }});"
                ));
            }
        });
    }

    // Clear active cell + range when the keyboard container loses focus to
    // an element outside the table. focusin/out bubble so we catch them on
    // the outer div via a capturing document-level mousedown listener that
    // checks relatedTarget vs the container boundary. Edit commits are
    // handled separately in editor_td's onblur; this path only cleans up
    // range/active state.
    {
        let kb_id_focus = kb_id.clone();
        let click_outside_counter: Signal<u32> = use_signal(|| 0);
        // Spawn an async eval that listens for a custom "chorale-blur" event
        // dispatched from the onmousedown on the document.
        {
            let id = kb_id_focus.clone();
            let mut counter = click_outside_counter;
            use_effect(move || {
                let id2 = id.clone();
                spawn(async move {
                    // Register a capturing mousedown listener on the document.
                    // Removes any prior listener for this id first to avoid duplicates.
                    let mut eval = dioxus::document::eval(&format!(
                        "(function(){{\
                            var cid='{id2}';\
                            var el=document.getElementById(cid);\
                            if(window['_chh_'+cid])\
                                document.removeEventListener('mousedown',window['_chh_'+cid],true);\
                            window['_chh_'+cid]=function(e){{\
                                if(el&&!el.contains(e.target))dioxus.send(1);\
                            }};\
                            document.addEventListener('mousedown',window['_chh_'+cid],true);\
                        }})();"
                    ));
                    while eval.recv::<i32>().await.is_ok() {
                        let next = *counter.peek() + 1;
                        counter.set(next);
                    }
                });
            });
        }
        use_effect(move || {
            // Re-run only when a click-outside has been detected.
            let n = *click_outside_counter.read();
            if n > 0 {
                let mut sig_w = handle.signal();
                // Click-outside handles active_cell + range_selection cleanup
                // ONLY when no edit is in progress. When state.editing is Some,
                // the editor input's onblur handler owns the full cleanup
                // (commit → clear editing → clear active → clear range). If we
                // ran here too, we'd race with onblur's writes and the typed
                // value would get lost in the order interaction.
                let new_state_opt = {
                    let s = sig_w.peek();
                    if s.editing.is_some() {
                        // Defer to onblur — do nothing.
                        None
                    } else if s.active_cell.is_some() || !s.range_selection.is_empty() {
                        let s2 = clear_range_selection(&*s);
                        Some(clear_active_cell(&s2))
                    } else {
                        None
                    }
                };
                if let Some(new_state) = new_state_opt {
                    sig_w.set(new_state);
                }
            }
        });
    }

    // VIRT-2: variable-row-height measurement loop. Hooks must be
    // unconditional, so this is always called and no-ops when
    // variable_row_height=false. The 0.5px diff threshold below the
    // dispatch site prevents convergence loops caused by sub-pixel float
    // rounding.
    //
    // `meas_trigger` is the reactive dependency that drives remeasurement.
    // It must be read at the TOP of the effect body — a `sig.read()` inside
    // the spawned future runs after the effect completes and does not count
    // as a Dioxus dependency, so without this read the effect would only
    // fire once at initial mount and row_heights would stay empty forever.
    let meas_trigger = use_memo(move || {
        let s = sig.read();
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
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
    });
    {
        let scroll_id_meas = scroll_id.clone();
        use_effect(move || {
            // Register the meas_trigger subscription (see comment above).
            let _ = meas_trigger.read();
            let cid = scroll_id_meas.clone();
            let mut sig2 = handle.signal();
            spawn(async move {
                // Direct-child chain rather than a descendant selector:
                // `#{cid} > table > tbody > tr[data-chorale-index]`. When a
                // consumer renders a nested Table inside a detail panel, the
                // descendant form (`#{cid} [data-chorale-index]`) also
                // matched the child Table's rows, producing duplicate
                // `data-chorale-index` keys (child 0,1,2... collide with
                // parent 0,1,2...) that corrupted the parent's row_heights
                // HashMap. The direct-child chain stops at the parent's
                // own tbody.
                let mut js = dioxus::document::eval(&format!(
                    r"const rs=document.querySelectorAll('#{cid} > table > tbody > tr[data-chorale-index]');
const parts=[];
rs.forEach(r=>{{parts.push(r.getAttribute('data-chorale-index')+':'+r.getBoundingClientRect().height);}});
dioxus.send(parts.join('\n'));"
                ));
                if let Ok(data) = js.recv::<String>().await {
                    let measurements: std::collections::HashMap<usize, f64> = data
                        .lines()
                        .filter_map(|line| {
                            let mut it = line.splitn(2, ':');
                            let k = it.next()?.parse::<usize>().ok()?;
                            let v = it.next()?.parse::<f64>().ok()?;
                            Some((k, v))
                        })
                        .collect();
                    if measurements.is_empty() {
                        return;
                    }
                    let cur = sig2.read();
                    let mut any_changed = false;
                    for (k, v) in &measurements {
                        let old = cur.row_heights.get(k).copied().unwrap_or(cur.row_height);
                        if (v - old).abs() > 0.5 {
                            any_changed = true;
                            break;
                        }
                    }
                    if any_changed {
                        // Scroll anchoring: when a row's measured height
                        // changes (typically a freshly-rendered detail panel
                        // going from estimate → real), the total height of
                        // all rows ABOVE the current scroll position can
                        // change too. Applying the new measurements without
                        // adjusting scroll_top shifts visible content by the
                        // delta — the user sees that as a jump. Sum the
                        // per-row height delta for every row whose top edge
                        // sits above cur.scroll_top, bump scroll_top by that
                        // sum, and write it back to the DOM so the visible
                        // content stays visually anchored.
                        let cur_scroll = cur.scroll_top;
                        let default_h = cur.row_height;
                        let mut row_top_with_old = 0.0_f64;
                        let mut scroll_delta = 0.0_f64;
                        let total = view.peek().len();
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

                        let mut new_state = batch_record_row_heights(&cur, &measurements);
                        new_state.scroll_top = new_scroll;
                        drop(cur);
                        sig2.set(new_state);

                        if scroll_delta.abs() > 0.5 {
                            let cid_scroll = cid.clone();
                            spawn(async move {
                                let _ = dioxus::document::eval(&format!(
                                    "const el=document.getElementById('{cid_scroll}');if(el){{el.scrollTop={new_scroll};}}"
                                )).recv::<i32>().await;
                            });
                        }
                    }
                }
            });
        });
    }

    let view_read = view.read();
    let grouped_view_read = grouped_view.read();
    let state = sig.read();

    // Compute effective column order: explicit order first, then any unlisted columns appended.
    let effective_order: Vec<ColumnId> = if state.column_order.is_empty() {
        state.columns.iter().map(|c| c.id).collect()
    } else {
        let mut order: Vec<ColumnId> = state
            .column_order
            .iter()
            .filter(|id| state.columns.iter().any(|c| c.id == **id))
            .copied()
            .collect();
        for col in &state.columns {
            if !state.column_order.contains(&col.id) {
                order.push(col.id);
            }
        }
        order
    };

    // Read widths early so the sticky CSS computation can use them.
    let widths = state.column_widths.clone();

    // Split into frozen-left, scrollable, frozen-right zones. Render order:
    // left-frozen | scrollable | right-frozen. This is required for CSS
    // `position: sticky` to work correctly (Decision #2 from Item 10 spec).
    let left_frozen: Vec<ColumnDef<TRow>> =
        frozen_left_columns(&state).into_iter().cloned().collect();
    let scrollable: Vec<ColumnDef<TRow>> =
        scrollable_columns(&state).into_iter().cloned().collect();
    let right_frozen: Vec<ColumnDef<TRow>> =
        frozen_right_columns(&state).into_iter().cloned().collect();

    // Per-column sticky CSS (position + offset + z-index + optional divider shadow).
    // Two maps: header cells keep their own background (from base style) so we inject
    // only the offset; body cells need an explicit background to cover scrolled content.
    // Fallback width when no initial_width and no measured width: 150px (Decision 4).
    let header_z = frozen_column_z_index + 1; // corner cells are above both axes
    let body_z = frozen_column_z_index;
    let mut sticky_header_css: HashMap<ColumnId, String> = HashMap::new();
    let mut sticky_body_css: HashMap<ColumnId, String> = HashMap::new();
    {
        let mut left_offset = 0.0f64;
        let left_count = left_frozen.len();
        for (k, col) in left_frozen.iter().enumerate() {
            let col_w = widths
                .get(&col.id)
                .copied()
                .or(col.initial_width)
                .unwrap_or(150.0);
            let is_last = k + 1 == left_count;
            let divider = if is_last {
                " box-shadow: var(--chorale-frozen-divider-shadow, 3px 0 4px -2px rgba(0,0,0,0.15));"
            } else {
                ""
            };
            sticky_header_css.insert(
                col.id,
                format!("position: sticky; left: {left_offset}px; z-index: {header_z};"),
            );
            sticky_body_css.insert(
                col.id,
                format!(
                    "position: sticky; left: {left_offset}px; z-index: {body_z}; background: var(--chorale-surface, #fff);{divider}"
                ),
            );
            left_offset += col_w;
        }
    }
    {
        let mut right_offset = 0.0f64;
        for (j, col) in right_frozen.iter().enumerate().rev() {
            let col_w = widths
                .get(&col.id)
                .copied()
                .or(col.initial_width)
                .unwrap_or(150.0);
            let is_first = j == 0;
            let divider = if is_first {
                " box-shadow: var(--chorale-frozen-divider-shadow, -3px 0 4px -2px rgba(0,0,0,0.15));"
            } else {
                ""
            };
            sticky_header_css.insert(
                col.id,
                format!("position: sticky; right: {right_offset}px; z-index: {header_z};"),
            );
            sticky_body_css.insert(
                col.id,
                format!(
                    "position: sticky; right: {right_offset}px; z-index: {body_z}; background: var(--chorale-surface, #fff);{divider}"
                ),
            );
            right_offset += col_w;
        }
    }

    let visible_cols: Vec<ColumnDef<TRow>> = left_frozen
        .iter()
        .chain(scrollable.iter())
        .chain(right_frozen.iter())
        .cloned()
        .collect();

    // Active cell + range selection snapshots for rendering and keyboard handler.
    let active_cell = state.active_cell;
    let range_selection = state.range_selection.clone();
    // Snapshot column IDs for the keyboard handler closure (stale-on-reorder is acceptable).
    let visible_col_ids_for_kb: Vec<ColumnId> = visible_cols.iter().map(|c| c.id).collect();
    let total_rows_for_kb = view_read
        .iter()
        .filter(|r| matches!(r, RenderRow::Data { .. }))
        .count();
    // Pre-compute the set of all (row, col) cells covered by any range rectangle so that
    // per-cell rendering is O(1) lookup rather than O(ranges * cells).
    let range_cells: HashSet<(usize, ColumnId)> = {
        let col_refs: Vec<&ColumnDef<TRow>> = visible_cols.iter().collect();
        let mut cells = HashSet::new();
        for r in &range_selection {
            let nr = r.normalized(&col_refs);
            for row in nr.min_row..=nr.max_row {
                for &col_id in &nr.columns {
                    cells.insert((row, col_id));
                }
            }
        }
        cells
    };

    // Focus cell for fill handle: only when single range selected.
    let fill_focus_cell: Option<(usize, ColumnId)> = if range_selection.len() == 1 {
        let col_refs: Vec<&ColumnDef<TRow>> = visible_cols.iter().collect();
        let nr = range_selection[0].normalized(&col_refs);
        nr.columns.last().map(|&col_id| (nr.max_row, col_id))
    } else {
        None
    };

    // In inline mode we bypass virtualization entirely — every visible row
    // renders in a single batch with no top/bottom spacer <tr>s. This makes
    // the <Table> usable as a child of an outer scrolling element (e.g.,
    // master/detail panel) without creating a nested scroll context that
    // would otherwise produce wheel-event hand-off discontinuities ("jumps")
    // when the user scrolls past the edge of the inner view.
    let (win, render_slice) = if inline {
        // Inline mode: render the entire view at natural height; no
        // virtualization, no spacers. `visible_window(0, MAX, ...)` returns a
        // window that covers all rows with zero pad on either side.
        let full_slice: Vec<RenderRow<TRow>> = view_read.iter().cloned().collect();
        let win = visible_window(0.0, f64::MAX, state.row_height, full_slice.len(), 0);
        (win, full_slice)
    } else {
        compute_window_slice(&state, &view_read, variable_row_height)
    };
    let total_pages = state.total_pages();
    let page_idx = state.page; // zero-based
    let total_rows = state.filtered_row_count();
    let is_infinite_scroll = state.pagination_mode == PaginationMode::InfiniteScroll;
    let has_more_rows = is_infinite_scroll && state.loaded_row_count < total_rows;
    let is_grouped = !state.grouping.is_empty();
    let is_virtualized_grouped =
        is_grouped && state.grouped_pagination == GroupedPaginationMode::Virtualized;
    let col_count = visible_cols.len();
    let effective_col_count = col_count + usize::from(selection_enabled) + usize::from(has_detail);
    let row_height = state.row_height;
    let viewport_height = state.viewport_height;
    let current_sort: &[SortState] = &state.sort;
    let filters = state.filters.clone();
    let editing_target = state.editing;

    let all_col_defs: Vec<(ColumnId, String)> = effective_order
        .iter()
        .filter_map(|id| state.columns.iter().find(|c| c.id == *id))
        .map(|c| (c.id, c.header.clone()))
        .collect();
    let col_visibility = state.column_visibility.clone();

    let selection_set: HashSet<RowId> = state.selection.iter().copied().collect();
    let page_data_ids: Vec<RowId> = view_read
        .iter()
        .filter_map(|r| {
            if let RenderRow::Data { id, .. } = r {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    let all_page_selected =
        !page_data_ids.is_empty() && page_data_ids.iter().all(|id| selection_set.contains(id));

    let page_buttons = page_button_range(page_idx, total_pages);
    let prev_disabled = page_idx == 0;
    let next_disabled = page_idx + 1 >= total_pages;
    let nav_btn = "padding:0.25rem 0.6rem;border:1px solid var(--chorale-border, #ddd);border-radius:3px;\
                   font-size:0.875rem;cursor:pointer;background:var(--chorale-button-bg, white);color:var(--chorale-text, #333);";
    let nav_btn_dis = "padding:0.25rem 0.6rem;border:1px solid var(--chorale-border, #ddd);border-radius:3px;\
                       font-size:0.875rem;cursor:not-allowed;background:var(--chorale-button-disabled-bg, #f0f0f0);color:var(--chorale-text-disabled, #aaa);";

    // Build the XLSX export button as an Option<Element> outside the main
    // rsx so a `#[cfg(feature = "xlsx")]` attribute can gate the whole
    // expression (rsx! doesn't accept cfg attributes on inline children).
    // None when the feature is off, the `xlsx_export` prop is missing,
    // or the prop is false. Same blue-outline styling as the CSV button
    // with a `margin-left` so the two sit side by side at the right edge
    // of the pagination footer.
    let xlsx_button_el: Option<Element> = {
        #[cfg(feature = "xlsx")]
        {
            if xlsx_export {
                let label = labels.export_xlsx_label.clone();
                Some(rsx! {
                    button {
                        style: "margin-left: 0.5rem; padding:0.25rem 0.75rem;\
                                border:1px solid var(--chorale-accent, #4a90e2); border-radius:3px;\
                                font-size:0.875rem; cursor:pointer;\
                                background:var(--chorale-button-bg, white); color:var(--chorale-accent, #4a90e2);",
                        onclick: move |_| {
                            use chorale_core::{to_xlsx, XlsxOptions};
                            let sig = handle.signal();
                            let Ok(bytes) = to_xlsx(&*sig.read(), &XlsxOptions::default()) else {
                                return;
                            };
                            let b64 = to_base64(&bytes);
                            let js = format!(
                                r"(()=>{{
                                    var r=atob('{b64}'),n=r.length,u=new Uint8Array(n);
                                    for(var i=0;i<n;i++)u[i]=r.charCodeAt(i);
                                    var bl=new Blob([u],{{type:'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet'}});
                                    var url=URL.createObjectURL(bl),a=document.createElement('a');
                                    a.href=url;a.download='chorale-export.xlsx';a.click();
                                    setTimeout(()=>URL.revokeObjectURL(url),100);
                                }})()"
                            );
                            dioxus::document::eval(&js);
                        },
                        "{label}"
                    }
                })
            } else {
                None
            }
        }
        #[cfg(not(feature = "xlsx"))]
        {
            None
        }
    };

    rsx! {
        // Inject the built-in light+dark token stylesheet. `dangerous_inner_html`
        // keeps the CSS verbatim (the [data-chorale-theme="..."] quotes must not
        // be HTML-escaped). Duplicate identical blocks across multiple tables are
        // harmless. Theme::Custom still renders this, but its attribute value
        // ("custom") matches no block, so the inline var() fallbacks win.
        style { dangerous_inner_html: theme_stylesheet() }
        div {
            id: "{kb_id}",
            class: "{THEME_ROOT_CLASS}",
            "data-chorale-theme": theme.attribute_value(),
            tabindex: "0",
            style: "border: 1px solid var(--chorale-border, #ddd); border-radius: 4px; overflow: hidden; \
                    user-select: none; outline: none;",
            onmousemove: move |e| {
                if let Some((col_id, start_x, start_w)) = *drag_state.read() {
                    let delta = e.client_coordinates().x - start_x;
                    handle.set_column_width(col_id, (start_w + delta).max(40.0)).ok();
                }
            },
            onmouseup: move |_| {
                drag_state.set(None);
                if *fill_drag_active.peek() {
                    fill_drag_active.set(false);
                    if let Some((target_row, target_col)) = *fill_hover.peek() {
                        let mut sig = handle.signal();
                        let state = sig.peek();
                        if let Some(source_range) = state.range_selection.first() {
                            let writes = fill_handle_targets(&*state, source_range, target_row, target_col);
                            if !writes.is_empty() {
                                // Build TSV from writes
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

                                // Determine extension range for on_paste
                                let first_row = *row_idxs.first().unwrap_or(&target_row);
                                let last_row = *row_idxs.last().unwrap_or(&target_row);
                                let first_col = writes.first().map_or(target_col, |(_, c, _)| *c);
                                let last_col = writes.last().map_or(target_col, |(_, c, _)| *c);
                                let ext_range = chorale_core::RangeSelection::new(
                                    (first_row, first_col),
                                    (last_row, last_col),
                                );
                                // Update state range to extension
                                drop(state);
                                sig.write().range_selection = vec![ext_range.clone()];
                                if let Some(cb) = on_paste {
                                    cb.call(chorale_core::ClipboardPasteEvent { tsv, range: ext_range });
                                }
                            }
                        }
                    }
                    fill_hover.set(None);
                }
            },
            onmouseleave: move |_| {
                drag_state.set(None);
                if *fill_drag_active.peek() {
                    fill_drag_active.set(false);
                    fill_hover.set(None);
                }
            },
            onclick: {
                let kb_id = kb_id.clone();
                move |_| {
                    let id = kb_id.clone();
                    // Only steal focus to the keyboard container when the click was NOT on
                    // an interactive child element (input, select, textarea, button).
                    // Checking document.activeElement after the click event fires works
                    // because the browser sets focus during mousedown — before onclick.
                    dioxus::document::eval(&format!(
                        "var ae=document.activeElement;
                         var tag=ae&&ae.nodeName||'';
                         if(!['INPUT','SELECT','TEXTAREA','BUTTON'].includes(tag)){{
                           var el=document.getElementById('{id}');if(el)el.focus();
                         }}"
                    ));
                }
            },
            onkeydown: {
                let scroll_id_kb = scroll_id.clone();
                move |e: KeyboardEvent| {
                let shift = e.modifiers().contains(Modifiers::SHIFT);
                let ctrl = e.modifiers().contains(Modifiers::CONTROL)
                    || e.modifiers().contains(Modifiers::META);
                let dir_opt: Option<NavDirection> = match e.key() {
                    Key::ArrowDown => Some(NavDirection::Down),
                    Key::ArrowUp => Some(NavDirection::Up),
                    Key::ArrowLeft => Some(NavDirection::Left),
                    Key::ArrowRight => Some(NavDirection::Right),
                    _ => None,
                };
                if let Some(dir) = dir_opt {
                    e.prevent_default();
                    let mut sig_w = handle.signal();
                    if shift {
                        let cols = visible_col_ids_for_kb.clone();
                        let total = total_rows_for_kb;
                        let new_s = {
                            let s = sig_w.peek();
                            let focus = s
                                .range_selection
                                .last()
                                .map(|r| r.focus)
                                .or_else(|| s.active_cell.map(|ac| (ac.row_idx, ac.column_id)));
                            if let Some((row, col_id)) = focus {
                                let col_idx = cols.iter().position(|id| *id == col_id).unwrap_or(0);
                                let last_row = total.saturating_sub(1);
                                let last_col = cols.len().saturating_sub(1);
                                let (new_row, new_col_idx) = match dir {
                                    NavDirection::Up => (row.saturating_sub(1), col_idx),
                                    NavDirection::Down => ((row + 1).min(last_row), col_idx),
                                    NavDirection::Left => (row, col_idx.saturating_sub(1)),
                                    NavDirection::Right => (row, (col_idx + 1).min(last_col)),
                                    _ => (row, col_idx),
                                };
                                let new_col_id = cols.get(new_col_idx).copied().unwrap_or(col_id);
                                extend_range_to(&*s, new_row, new_col_id)
                            } else {
                                s.clone()
                            }
                        };
                        sig_w.set(new_s);
                    } else if ctrl {
                        let new_s = move_active_cell_to_edge(&*sig_w.peek(), dir);
                        sig_w.set(new_s);
                        ensure_active_row_visible(
                            handle, &scroll_id_kb, variable_row_height, inline,
                        );
                    } else {
                        let new_s = move_active_cell(&*sig_w.peek(), dir);
                        sig_w.set(new_s);
                        ensure_active_row_visible(
                            handle, &scroll_id_kb, variable_row_height, inline,
                        );
                    }
                } else {
                    match e.key() {
                        Key::Home => {
                            e.prevent_default();
                            let mut sig_w = handle.signal();
                            let new_s = if ctrl {
                                move_active_cell_first(&*sig_w.peek())
                            } else {
                                move_active_cell_home(&*sig_w.peek())
                            };
                            sig_w.set(new_s);
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline,
                            );
                        }
                        Key::End => {
                            e.prevent_default();
                            let mut sig_w = handle.signal();
                            let new_s = if ctrl {
                                move_active_cell_last(&*sig_w.peek())
                            } else {
                                move_active_cell_end(&*sig_w.peek())
                            };
                            sig_w.set(new_s);
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline,
                            );
                        }
                        Key::PageUp => {
                            e.prevent_default();
                            let mut sig_w = handle.signal();
                            let page_sz = sig_w.peek().page_size;
                            let new_s = move_active_cell_page(&*sig_w.peek(), NavDirection::Up, page_sz);
                            sig_w.set(new_s);
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline,
                            );
                        }
                        Key::PageDown => {
                            e.prevent_default();
                            let mut sig_w = handle.signal();
                            let page_sz = sig_w.peek().page_size;
                            let new_s = move_active_cell_page(&*sig_w.peek(), NavDirection::Down, page_sz);
                            sig_w.set(new_s);
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline,
                            );
                        }
                        Key::Escape => {
                            let mut sig_w = handle.signal();
                            let new_s = {
                                let s = sig_w.peek();
                                let s2 = clear_range_selection(&*s);
                                clear_active_cell(&s2)
                            };
                            sig_w.set(new_s);
                        }
                        // Item 7: F2 (or Enter) starts in-cell editing on the
                        // active cell. The `editing.is_none()` guard matters:
                        // keydowns from an open editor bubble to this handler
                        // (e.g. Enter on a <select> picking an option), and
                        // without the guard they would restart the edit and
                        // wipe the in-progress editor text.
                        Key::F2 | Key::Enter => {
                            let mut sig_w = handle.signal();
                            let s = sig_w.peek();
                            if s.editing.is_none() {
                                if let Some(ac) = s.active_cell {
                                    let rows = visible_view(&*s);
                                    if let Some(RenderRow::Data { id: row_id, .. }) = rows.get(ac.row_idx) {
                                        if let Ok(new_s) = chorale_core::start_edit(&*s, *row_id, ac.column_id) {
                                            drop(s);
                                            sig_w.set(new_s);
                                        }
                                    }
                                }
                            }
                        }
                        Key::Character(ref ch) if ch.to_lowercase() == "a" && ctrl => {
                            e.prevent_default();
                            let mut sig_w = handle.signal();
                            let new_s = select_all_range(&*sig_w.peek());
                            sig_w.set(new_s);
                        }
                        Key::Character(ref ch) if ch.to_lowercase() == "c" && ctrl => {
                            e.prevent_default();
                            let sig_r = handle.signal();
                            let s = sig_r.peek();
                            if let Ok(tsv) = to_clipboard_tsv(&*s) {
                                if !tsv.is_empty() {
                                    let range = s.range_selection.first().cloned();
                                    drop(s);
                                    let js = format!(
                                        "navigator.clipboard.writeText({}).catch(()=>{{}})",
                                        js_string_literal(&tsv)
                                    );
                                    dioxus::document::eval(&js);
                                    if let (Some(range), Some(cb)) = (range, on_copy) {
                                        cb.call(ClipboardCopyEvent { tsv, range });
                                    }
                                }
                            }
                        }
                        Key::Character(ref ch) if ch.to_lowercase() == "v" && ctrl => {
                            e.prevent_default();
                            spawn(async move {
                                let mut eval = dioxus::document::eval(
                                    "navigator.clipboard.readText()\
                                     .then(t=>dioxus.send(t))\
                                     .catch(()=>dioxus.send(''))",
                                );
                                if let Ok(tsv) = eval.recv::<String>().await {
                                    if !tsv.trim().is_empty() {
                                        let mut sig_w = handle.signal();
                                        let new_s = {
                                            let s = sig_w.peek();
                                            paste_tsv_into_range(&*s, &tsv)
                                        };
                                        if let Ok(new_state) = new_s {
                                            let range =
                                                new_state.range_selection.first().cloned();
                                            sig_w.set(new_state);
                                            if let (Some(range), Some(cb)) = (range, on_paste) {
                                                cb.call(ClipboardPasteEvent { tsv, range });
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        Key::Tab => {
                            e.prevent_default();
                            let tab_dir = if shift { NavDirection::Left } else { NavDirection::Right };
                            let mut sig_w = handle.signal();
                            let new_s = move_active_cell(&*sig_w.peek(), tab_dir);
                            let new_ac = new_s.active_cell;
                            sig_w.set(new_s);
                            if let (Some(ac), Some(cb)) = (new_ac, on_tab_to_editable) {
                                let s = sig_w.peek();
                                if s.columns.iter().any(|c| c.id == ac.column_id && c.editor.is_some()) {
                                    cb.call(ac);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }},

            if column_toolbar {
                {column_visibility_toolbar(&all_col_defs, &col_visibility, handle, &labels)}
            }

            // Toolbar renders whenever the slot is filled. Empty-selection
            // gating was a footgun: the toolbar typically includes the
            // "Select all" / "Select page" actions that the user needs to
            // REACH the non-empty state. Hiding it on empty selection
            // hid those affordances exactly when they were most useful.
            // Consumers wanting empty-vs-non-empty styling can branch
            // inside their toolbar Element by reading state.selection
            // through the handle.
            if let Some(toolbar) = selection_toolbar {
                div {
                    class: "chorale-selection-toolbar",
                    style: "width: 100%; box-sizing: border-box; border-bottom: 2px solid var(--chorale-accent-strong, #1d4ed8);",
                    {toolbar}
                }
            }

            // Virtualized scroll container. scroll_top is kept in TableState so
            // visible_window math + spacers stay aligned with the DOM scroll.
            //
            // PERF: ScrollData::scroll_top() reads the value synchronously from
            // the event. Routing through dioxus::document::eval (async) caused
            // visible scroll-lag: state.scroll_top fell behind the DOM during
            // fast scrolling, so the rendered window (computed from the stale
            // scroll_top) was sometimes entirely above the viewport — the user
            // saw bottom_pad (empty space) instead of rows.
            //
            // CRITICAL: `overflow-anchor: none` disables browser scroll
            // anchoring. With it ON (the default in Chrome/Firefox), every
            // scroll event triggers a render that swaps the rendered TRs in
            // tbody — the browser sees DOM mutations above the viewport and
            // auto-adjusts scrollTop to "preserve visible position". That
            // adjustment fires another onscroll → another render → another
            // mutation → another adjustment, producing a runaway scroll that
            // continues until it hits the top or bottom of the content.
            // Virtualized lists must always opt out of scroll anchoring.
            div {
                id: "{scroll_id}",
                style: if inline {
                    // Inline mode: no own scroll, no height clamp. Body
                    // flows at natural size; parent's scroll context owns
                    // overflow. Wheel events bubble through cleanly with
                    // no nested-scroll handoff discontinuity.
                    "overflow: visible; height: auto;".to_string()
                } else {
                    format!(
                        "overflow-y: auto; overflow-x: auto; overflow-anchor: none; \
                         height: {viewport_height}px;"
                    )
                },
                onscroll: move |e| {
                    let st = e.scroll_top();
                    handle.set_scroll(st);
                    // Infinite scroll: trigger load_more_rows when within threshold of the bottom.
                    let sig_for_scroll = handle.signal();
                    let s = sig_for_scroll.read();
                    if s.pagination_mode == PaginationMode::InfiniteScroll {
                        #[allow(clippy::cast_precision_loss)]
                        let total_h = s.loaded_row_count as f64 * s.row_height;
                        let dist = total_h - st - s.viewport_height;
                        if dist < infinite_scroll_threshold_px {
                            drop(s);
                            handle.load_more_rows();
                        }
                    }
                },

                table {
                    style: "width: 100%; border-collapse: collapse; table-layout: fixed;",

                    thead {
                        tr {
                            style: "background: var(--chorale-header-bg, #f8f9fa);",
                            if selection_enabled {
                                {select_all_th(handle, all_page_selected, sticky_header)}
                            }
                            if has_detail {
                                // Mirrors header_th's sticky toggling +
                                // explicit-override pattern (see comment
                                // in header_th). border-bottom matches
                                // both header_th and select_all_th so the
                                // line under the header is continuous
                                // across all columns.
                                th {
                                    style: if sticky_header {
                                        "width: 24px; padding: 0; \
                                         border-bottom: 1px solid var(--chorale-border, #ddd); \
                                         position: sticky; top: 0; \
                                         background: var(--chorale-header-bg, #f8f9fa); z-index: 1;"
                                    } else {
                                        "width: 24px; padding: 0; \
                                         border-bottom: 1px solid var(--chorale-border, #ddd); \
                                         position: static; top: auto; \
                                         z-index: auto; \
                                         background: var(--chorale-header-bg, #f8f9fa);"
                                    }
                                }
                            }
                            for col in &visible_cols {
                                {header_th(col, widths.get(&col.id).copied(), handle, sort_enabled, current_sort, resize_enabled, drag_state, column_reorder_enabled, drag_col_id, drag_over_col, on_column_order_change, sticky_header_css.get(&col.id).map_or("", String::as_str), sticky_header)}
                            }
                        }
                        if filter_enabled {
                            tr {
                                style: "background: var(--chorale-surface, #fff);",
                                if selection_enabled {
                                    th {
                                        style: "padding: 0.25rem; border-bottom: 1px solid var(--chorale-divider, #eee); \
                                                background: var(--chorale-surface, #fff); width: 2.5rem;",
                                    }
                                }
                                if has_detail {
                                    th { style: "width: 24px; padding: 0; border-bottom: 1px solid var(--chorale-divider, #eee); background: var(--chorale-surface, #fff);" }
                                }
                                for col in &visible_cols {
                                    {filter_th(col, widths.get(&col.id).copied(), handle, &filters, &labels, sticky_header_css.get(&col.id).map_or("", String::as_str))}
                                }
                            }
                        }
                    }

                    tbody {
                        if is_grouped {
                            // GroupedPaginationMode::Virtualized slices to a scroll-driven
                            // window — start_index / end_index from scroll_top /
                            // viewport_height / row_height, with buffer rows for smooth
                            // scrolling. Without slicing, a 10k-row grouped dataset
                            // (~10k data rows + N group headers) would render every row
                            // at once and freeze the browser under the DOM weight.
                            // DataRowsOnly mode is already paginated, so no slicing.
                            {
                                let grouped_len = grouped_view_read.len();
                                let (start_idx, end_idx) = if is_virtualized_grouped && grouped_len > 0 {
                                    let buf = state.buffer_rows;
                                    let raw_start =
                                        (state.scroll_top / state.row_height).floor() as usize;
                                    let visible = (state.viewport_height / state.row_height).ceil()
                                        as usize;
                                    let start = raw_start.saturating_sub(buf);
                                    let end = (raw_start + visible + buf).min(grouped_len);
                                    (start, end)
                                } else {
                                    (0, grouped_len)
                                };
                                let top_pad_px =
                                    (start_idx as f64 * state.row_height).max(0.0);
                                let bottom_pad_px =
                                    ((grouped_len.saturating_sub(end_idx)) as f64
                                        * state.row_height)
                                        .max(0.0);
                                rsx! {
                                    if top_pad_px > 0.0 {
                                        tr {
                                            td {
                                                colspan: "{effective_col_count}",
                                                style: "padding: 0; height: {top_pad_px}px;",
                                            }
                                        }
                                    }
                                    for (offset, grouped_row) in grouped_view_read[start_idx..end_idx].iter().cloned().enumerate() {
                                        {render_grouped_row(grouped_row, start_idx + offset, effective_col_count, selection_enabled, has_detail, handle, &group_header_class, &visible_cols, row_height, &widths, variable_row_height, &cell_renderers, &row_cell_renderers, editing_target, editing_text, edit_error, &validate_edit, on_commit_edit, &sticky_body_css, &selection_set, active_cell, &range_cells, fill_focus_cell, fill_drag_active, fill_hover, on_row_click, &kb_id)}
                                    }
                                    if bottom_pad_px > 0.0 {
                                        tr {
                                            td {
                                                colspan: "{effective_col_count}",
                                                style: "padding: 0; height: {bottom_pad_px}px;",
                                            }
                                        }
                                    }
                                }
                            }
                            if grouped_view_read.is_empty() {
                                tr {
                                    td {
                                        colspan: "{effective_col_count}",
                                        style: "padding: 2rem 1rem; text-align: center; \
                                                color: var(--chorale-text-subtle, #999); font-style: italic;",
                                        "{labels.no_rows_label}"
                                    }
                                }
                            }
                        } else {
                            if win.top_pad_px > 0.0 {
                                tr {
                                    td {
                                        colspan: "{effective_col_count}",
                                        style: "height: {win.top_pad_px}px; padding: 0; border: 0;",
                                    }
                                }
                            }
                            for (i, render_row) in render_slice.iter().enumerate() {
                                {
                                    match render_row {
                                        RenderRow::Data { id: row_id, row } => {
                                            let row_id = *row_id;
                                            let is_expanded = has_detail && state.expanded_rows.contains(&row_id);
                                            let editing_col = editing_target
                                                .filter(|t| t.row_id == row_id)
                                                .map(|t| t.column_id);
                                            data_tr(row, row_id, win.start_index + i, variable_row_height, &visible_cols, row_height, &widths, selection_enabled, selection_set.contains(&row_id), handle, &cell_renderers, &row_cell_renderers, editing_col, editing_text, edit_error, &validate_edit, on_commit_edit, &sticky_body_css, active_cell, &range_cells, fill_focus_cell, fill_drag_active, fill_hover, has_detail, is_expanded, on_row_click, &kb_id)
                                        }
                                        RenderRow::DetailPanel { parent_row_id } => {
                                            let pid = *parent_row_id;
                                            let parent = state.rows.iter()
                                                .find(|(rid, _)| *rid == pid)
                                                .map(|(_, r)| r.clone());
                                            detail_panel_tr(pid, parent, detail_renderer.as_ref(), effective_col_count, win.start_index + i)
                                        }
                                    }
                                }
                            }
                            if win.bottom_pad_px > 0.0 {
                                tr {
                                    td {
                                        colspan: "{effective_col_count}",
                                        style: "height: {win.bottom_pad_px}px; padding: 0; border: 0;",
                                    }
                                }
                            }
                            if total_rows == 0 {
                                tr {
                                    td {
                                        colspan: "{effective_col_count}",
                                        style: "padding: 2rem 1rem; text-align: center; \
                                                color: var(--chorale-text-subtle, #999); font-style: italic;",
                                        "{labels.no_rows_label}"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if is_infinite_scroll {
                if has_more_rows {
                    div {
                        style: "padding: 0.75rem 1rem; text-align: center; \
                                border-top: 1px solid var(--chorale-border, #ddd); background: var(--chorale-toolbar-bg, #fafafa); \
                                font-size: 0.875rem; color: var(--chorale-text-subtle, #999);",
                        "{labels.load_more_label}"
                    }
                }
            } else if !is_virtualized_grouped {
                div {
                    style: "padding: 0.5rem 1rem; display: flex; align-items: center; \
                            flex-wrap: wrap; gap: 0.25rem; border-top: 1px solid var(--chorale-border, #ddd); \
                            background: var(--chorale-toolbar-bg, #fafafa); font-size: 0.875rem; color: var(--chorale-text-muted, #555);",
                    button {
                        style: if prev_disabled { "{nav_btn_dis}" } else { "{nav_btn}" },
                        disabled: prev_disabled,
                        onclick: move |_| { handle.set_page(page_idx.saturating_sub(1)).ok(); },
                        "{labels.previous_page_label}"
                    }
                    for item in page_buttons {
                        {render_page_btn(item, page_idx, handle)}
                    }
                    button {
                        style: if next_disabled { "{nav_btn_dis}" } else { "{nav_btn}" },
                        disabled: next_disabled,
                        onclick: move |_| {
                            if page_idx + 1 < total_pages {
                                handle.set_page(page_idx + 1).ok();
                            }
                        },
                        "{labels.next_page_label}"
                    }
                    span { style: "margin-left: 0.5rem; color: var(--chorale-text-subtle, #999);", "\u{00b7}" }
                    span { "{total_rows} rows" }
                    if total_pages > 1 {
                        span { style: "margin-left: 0.5rem; color: var(--chorale-text-subtle, #999);", "\u{00b7}" }
                        GotoPageInput::<TRow> { handle, total_pages, labels: labels.clone() }
                    }
                    // `flex: 1` spacer pushes both export buttons to the
                    // right of the pagination/goto controls. Emitted when
                    // EITHER export is enabled so the buttons end up
                    // grouped at the right edge of the same flexbox row.
                    if csv_export || xlsx_button_el.is_some() {
                        span { style: "flex: 1;" }
                    }
                    if csv_export {
                        button {
                            style: "padding:0.25rem 0.75rem;border:1px solid var(--chorale-accent, #4a90e2);\
                                    border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                    background:var(--chorale-button-bg, white);color:var(--chorale-accent, #4a90e2);",
                            onclick: move |_| {
                                let sig = handle.signal();
                                let csv = to_csv(&*sig.read());
                                spawn(async move {
                                    let js = dioxus::document::eval(r"
                                        const csv = await dioxus.recv();
                                        const blob = new Blob([csv], {type:'text/csv;charset=utf-8;'});
                                        const url = URL.createObjectURL(blob);
                                        const a = document.createElement('a');
                                        a.href = url;
                                        a.download = 'chorale-export.csv';
                                        document.body.appendChild(a);
                                        a.click();
                                        document.body.removeChild(a);
                                        URL.revokeObjectURL(url);
                                    ");
                                    let _ = js.send(csv);
                                });
                            },
                            "{labels.export_csv_label}"
                        }
                    }
                    {xlsx_button_el}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// View-and-window slicing — single source of truth for the dedupe.
// ---------------------------------------------------------------------------

/// Given a memoized `visible_view` and the current state, returns the
/// virtualization window plus the windowed row and id slices in a single pass.
///
/// When `variable` is `true`, dispatches to [`visible_window_variable`]
/// (VIRT-2); otherwise uses the fixed-height [`visible_window`] (VIRT-1).
///
/// **Wiring-bug regression guard.** Before this helper existed, the table
/// component called `visible_window_for_state` (which internally computes
/// the filtered/sorted/paginated view) AND `visible_row_ids` (which does
/// the same pipeline again) per render — two passes of the full pipeline
/// per scroll tick. This function takes the already-computed view as a
/// borrow and only does the cheap O(1) window math + slicing on top.
///
/// The accompanying test `compute_window_slice_matches_legacy_api`
/// asserts the output of this helper is identical to what the old
/// `visible_window_for_state` + `visible_row_ids` pair produced. If a
/// future refactor reintroduces the double pass, that test still passes
/// but the harness perf regression would resurface. The structural
/// `#[deprecated]` is intentionally NOT used on `visible_window_for_state`
/// so existing tests for chorale-core's pure functions keep passing.
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

/// Compute the `scroll_top` required to bring the row at `row_idx` fully
/// into the virtualized viewport, or `None` when the row is already fully
/// visible (no scroll needed).
///
/// Operates in the same coordinate space as the virtualization window math
/// (`visible_window` / `visible_window_variable`): row N's top edge sits at
/// `N * row_height` in fixed mode, or at the sum of the measured heights of
/// rows `0..N` in variable mode, with `row_height` as the fallback for rows
/// that have not been measured yet.
///
/// Rules (mirroring standard grid keyboard-navigation behavior):
/// - row top above the viewport  → scroll so the row top is at the viewport top
/// - row bottom below the viewport → scroll so the row bottom is at the
///   viewport bottom
/// - otherwise → `None`
#[must_use]
fn scroll_top_to_reveal_row<S: std::hash::BuildHasher>(
    row_idx: usize,
    scroll_top: f64,
    viewport_height: f64,
    row_height: f64,
    row_heights: &HashMap<usize, f64, S>,
    variable_row_height: bool,
) -> Option<f64> {
    let (row_top, row_h) = if variable_row_height {
        let mut top = 0.0_f64;
        for i in 0..row_idx {
            top += row_heights.get(&i).copied().unwrap_or(row_height);
        }
        let h = row_heights.get(&row_idx).copied().unwrap_or(row_height);
        (top, h)
    } else {
        #[allow(clippy::cast_precision_loss)]
        let top = row_idx as f64 * row_height;
        (top, row_height)
    };
    let row_bottom = row_top + row_h;
    if row_top < scroll_top {
        Some(row_top)
    } else if row_bottom > scroll_top + viewport_height {
        Some((row_bottom - viewport_height).max(0.0))
    } else {
        None
    }
}

/// After a keyboard-navigation transition lands a new `active_cell`, scroll
/// the virtualized container so the active row sits inside the viewport.
///
/// Two synchronized writes, in this order:
/// 1. `handle.set_scroll(..)` updates `TableState.scroll_top` so the next
///    render (already queued by the nav transition) computes the visible
///    window from the post-scroll position — without this the rendered
///    window lags one keypress behind the DOM scroll.
/// 2. A `document.getElementById(..).scrollTop = ..` eval moves the actual
///    DOM scroll container. The resulting native `onscroll` event calls
///    `handle.set_scroll` with the same value, which is a no-op.
///
/// Inline mode is a deliberate no-op: there is no internal scroll container
/// (`overflow: visible; height: auto`), the parent owns scrolling and this
/// component has no authority over the parent's `scrollTop`.
fn ensure_active_row_visible<TRow: Clone + PartialEq + 'static>(
    handle: UseTableHandle<TRow>,
    scroll_container_id: &str,
    variable_row_height: bool,
    inline: bool,
) {
    if inline {
        return;
    }
    let sig = handle.signal();
    let target = {
        let s = sig.peek();
        let Some(ac) = s.active_cell else {
            return;
        };
        scroll_top_to_reveal_row(
            ac.row_idx,
            s.scroll_top,
            s.viewport_height,
            s.row_height,
            &s.row_heights,
            variable_row_height,
        )
    };
    if let Some(new_scroll) = target {
        handle.set_scroll(new_scroll);
        dioxus::document::eval(&format!(
            "var el = document.getElementById('{scroll_container_id}'); \
             if (el) el.scrollTop = {new_scroll};"
        ));
    }
}

// ---------------------------------------------------------------------------
// Row and cell helpers (not components — plain functions returning Element)
// ---------------------------------------------------------------------------

fn detail_panel_tr<TRow: Clone + PartialEq + 'static>(
    parent_row_id: RowId,
    parent_row: Option<TRow>,
    detail_renderer: Option<&Callback<TRow, Element>>,
    colspan: usize,
    row_index: usize,
) -> Element {
    let key = format!("{parent_row_id:?}");
    // data-chorale-index lets the VIRT-2 measurement loop record this
    // panel's actual rendered height (e.g. 200 px for a 5-item child table)
    // in state.row_heights. Without it, visible_window_variable would fall
    // back to state.row_height (40 px) for the panel slot — its prefix-sum
    // would underestimate content height by (real - estimate) per expanded
    // row, and scroll math would drift then snap at boundaries (the
    // "jump"). With the attribute, detail panels are measured like any
    // other variable-height row.
    match (parent_row, detail_renderer) {
        (Some(prow), Some(renderer)) => {
            let content = renderer.call(prow);
            rsx! {
                tr {
                    key: "{key}",
                    class: "chorale-row chorale-detail-panel",
                    "data-chorale-index": "{row_index}",
                    td {
                        colspan: "{colspan}",
                        div { class: "chorale-detail-panel-inner", {content} }
                    }
                }
            }
        }
        _ => rsx! {
            tr {
                key: "{key}",
                class: "chorale-row chorale-detail-panel-empty",
                "data-chorale-index": "{row_index}",
            }
        },
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn header_th<TRow: Clone + PartialEq + 'static>(
    col: &ColumnDef<TRow>,
    override_width: Option<f64>,
    handle: UseTableHandle<TRow>,
    sort_enabled: bool,
    current_sort: &[SortState],
    resize_enabled: bool,
    mut drag_state: Signal<Option<(ColumnId, f64, f64)>>,
    column_reorder_enabled: bool,
    mut drag_col_id: Signal<Option<ColumnId>>,
    mut drag_over_col: Signal<Option<ColumnId>>,
    on_column_order_change: Option<EventHandler<Vec<ColumnId>>>,
    sticky_css: &str,
    sticky_header: bool,
) -> Element {
    let w = col_width_style(override_width, col.initial_width);
    let align = alignment_css(col.alignment);
    let header = col.header.clone();
    let col_id = col.id;
    let is_sortable = sort_enabled && col.sortable;
    let initial_width = col.initial_width;
    // Emit explicit position/top/z-index in BOTH branches. Dioxus 0.7's
    // inline-style diff is unreliable when transitioning from a non-empty
    // declaration to nothing — the prior `position: sticky` can persist in
    // the DOM. Writing explicit values on the off branch forces a concrete
    // attribute swap. When a column is ALSO frozen, `sticky_css` appears
    // later in the style string and its `position: sticky; left/right;
    // z-index` correctly overrides this baseline via standard
    // cascade-last-wins.
    //
    // position:sticky also acts as the containing block for the absolutely
    // positioned resize handle below; position:relative (NOT static)
    // ensures the same in the non-sticky case — with `static` the handle
    // resolves against some positioned ancestor outside the <th> and the
    // resize grip never lands on the column edge (mirrors the leptos
    // adapter's header_th).
    let sticky_top_decl = if sticky_header {
        "position: sticky; top: 0; z-index: 1;"
    } else {
        "position: relative; top: auto; z-index: auto;"
    };

    // Find this column's sort entry (if any) across the whole multi-sort list.
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
    // Show a priority badge (1-based) only when multiple columns are sorted.
    let sort_badge = if is_sortable && current_sort.len() > 1 {
        sort_entry.map(|(pos, _)| format!("{}", pos + 1))
    } else {
        None
    };

    let drag_cursor = if column_reorder_enabled { "grab; " } else { "" };
    let sort_cursor = if is_sortable { "pointer; " } else { "" };
    let extra = format!("cursor: {drag_cursor}{sort_cursor}");
    // Show the drop-target indicator only on the specific column currently under
    // the cursor, not on all non-drag columns (which caused the "stuck" look).
    let is_drag_over = column_reorder_enabled
        && drag_col_id.read().is_some_and(|id| id != col_id)
        && *drag_over_col.read() == Some(col_id);

    // Drop-target dashed outline renders as a child <div>, not an inline
    // `outline:` style on the <th>. Dioxus 0.7's inline-style diff is
    // unreliable on header THs after column reorder — the outline would
    // persist on the column that occupied the source slot post-drop.
    // Adding or removing a child node forces a structural DOM mutation
    // that the diff can't drop.

    rsx! {
        th {
            style: "{extra}padding: 0.5rem 1rem; border-bottom: 1px solid var(--chorale-border, #ddd); \
                    text-align: {align}; white-space: nowrap; overflow: hidden; \
                    text-overflow: ellipsis; {sticky_top_decl} \
                    background: var(--chorale-header-bg, #f8f9fa); {w} {sticky_css}",
            draggable: column_reorder_enabled,
            onclick: move |e| {
                if is_sortable {
                    let action = if e.modifiers().contains(Modifiers::SHIFT) {
                        SortAction::Append
                    } else {
                        SortAction::Replace
                    };
                    handle.toggle_sort(col_id, action);
                }
            },
            ondragstart: move |e| {
                if column_reorder_enabled {
                    e.stop_propagation();
                    drag_col_id.set(Some(col_id));
                }
            },
            ondragenter: move |e| {
                if column_reorder_enabled && drag_col_id.read().is_some_and(|id| id != col_id) {
                    e.prevent_default();
                    drag_over_col.set(Some(col_id));
                }
            },
            ondragover: move |e| {
                if column_reorder_enabled {
                    e.prevent_default();
                }
            },
            ondragleave: move |_| {
                if column_reorder_enabled && *drag_over_col.read() == Some(col_id) {
                    drag_over_col.set(None);
                }
            },
            ondrop: move |e| {
                if column_reorder_enabled {
                    e.prevent_default();
                    if let Some(dragged_id) = *drag_col_id.read() {
                        if dragged_id != col_id {
                            let sig = handle.signal();
                            let state = sig.read();
                            // Find the index of the drop target column in the effective order.
                            let effective: Vec<ColumnId> = if state.column_order.is_empty() {
                                state.columns.iter().map(|c| c.id).collect()
                            } else {
                                let mut order: Vec<ColumnId> = state
                                    .column_order
                                    .iter()
                                    .filter(|id| state.columns.iter().any(|c| c.id == **id))
                                    .copied()
                                    .collect();
                                for c in &state.columns {
                                    if !state.column_order.contains(&c.id) {
                                        order.push(c.id);
                                    }
                                }
                                order
                            };
                            if let Some(to_idx) = effective.iter().position(|id| *id == col_id) {
                                drop(state);
                                if handle.move_column(dragged_id, to_idx).is_ok() {
                                    if let Some(cb) = on_column_order_change {
                                        let new_order = sig.read().column_order.clone();
                                        cb.call(new_order);
                                    }
                                }
                            }
                        }
                    }
                    drag_col_id.set(None);
                    drag_over_col.set(None);
                }
            },
            ondragend: move |_| {
                // Reset both signals: ondragend fires on the source column regardless
                // of whether drop landed on a valid target, so this is the reliable
                // cleanup path. Without it, aborting mid-drag (Escape or drop outside)
                // leaves the blue dashed outline stuck on the last-hovered column.
                drag_col_id.set(None);
                drag_over_col.set(None);
            },
            // Drop-target outline as a structural overlay so Dioxus adds/removes
            // a DOM node when is_drag_over flips rather than diffing an inline
            // style attribute (which proved unreliable post-reorder).
            if is_drag_over {
                div {
                    style: "position: absolute; inset: 0; \
                            outline: 2px dashed var(--chorale-accent, #4a90e2); outline-offset: -2px; \
                            pointer-events: none; z-index: 3;",
                }
            }
            "{header}{sort_arrow}"
            if let Some(badge) = sort_badge {
                sup {
                    class: "chorale-sort-badge",
                    style: "font-size: 0.65em; margin-left: 2px; color: var(--chorale-accent, #4a90e2); \
                            font-weight: 700; vertical-align: super;",
                    "{badge}"
                }
            }
            if resize_enabled {
                div {
                    style: "position: absolute; right: 0; top: 0; bottom: 0; width: 5px; \
                            cursor: col-resize; background: transparent;",
                    onmousedown: move |e| {
                        e.stop_propagation();
                        // prevent_default keeps the browser from starting a
                        // native HTML5 drag of the (draggable, when column
                        // reorder is on) parent <th> or a text selection —
                        // either of which would swallow the mousemove stream
                        // the resize drag depends on (mirrors leptos).
                        e.prevent_default();
                        let current_w = override_width.or(initial_width).unwrap_or(100.0);
                        drag_state.set(Some((col_id, e.client_coordinates().x, current_w)));
                    },
                    ondoubleclick: move |_| handle.reset_column_width(col_id),
                }
            }
        }
    }
}

fn column_visibility_toolbar<TRow: Clone + PartialEq + 'static>(
    all_cols: &[(ColumnId, String)],
    visibility: &HashMap<ColumnId, bool>,
    handle: UseTableHandle<TRow>,
    labels: &Labels,
) -> Element {
    let col_vis_label = labels.column_visibility_label.clone();
    rsx! {
        div {
            style: "padding: 0.5rem 1rem; background: var(--chorale-group-header-bg, #f0f4ff); border-bottom: 1px solid var(--chorale-border, #ddd); \
                    display: flex; gap: 0.75rem; flex-wrap: wrap; align-items: center; \
                    font-size: 0.8rem; color: var(--chorale-text, #444);",
            span { style: "font-weight: 600;", "{col_vis_label}" }
            for (col_id, header) in all_cols {
                {column_vis_checkbox(*col_id, header, visibility.get(col_id).copied().unwrap_or(true), handle)}
            }
        }
    }
}

fn column_vis_checkbox<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    header: &str,
    is_visible: bool,
    handle: UseTableHandle<TRow>,
) -> Element {
    rsx! {
        label {
            style: "display: flex; align-items: center; gap: 0.25rem; cursor: pointer;",
            input {
                r#type: "checkbox",
                checked: is_visible,
                onchange: move |_| handle.set_column_visibility(col_id, !is_visible),
            }
            "{header}"
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn filter_th<TRow: Clone + PartialEq + 'static>(
    col: &ColumnDef<TRow>,
    override_width: Option<f64>,
    handle: UseTableHandle<TRow>,
    filters: &HashMap<ColumnId, FilterValue>,
    labels: &Labels,
    sticky_css: &str,
) -> Element {
    let w = col_width_style(override_width, col.initial_width);
    let col_id = col.id;
    let current = filters.get(&col_id).cloned();
    let filter_placeholder = labels.filter_placeholder.clone();
    let clear_label = labels.clear_filter_label.clone();
    let all_label = labels.page_size_all_label.clone();

    let th_style = format!(
        "padding: 0.25rem 0.5rem; border-bottom: 1px solid var(--chorale-divider, #eee); background: var(--chorale-surface, #fff); {w} {sticky_css}"
    );
    let empty_th_style = format!(
        "padding: 0.25rem; border-bottom: 1px solid var(--chorale-divider, #eee); background: var(--chorale-surface, #fff); {w} {sticky_css}"
    );

    match &col.filter {
        FilterKind::None => rsx! { th { style: "{empty_th_style}" } },
        FilterKind::Text => {
            let text = match &current {
                Some(FilterValue::Text(s)) => s.clone(),
                _ => String::new(),
            };
            let has_filter = current.is_some();
            rsx! {
                th { style: "{th_style}",
                    div {
                        style: "display: flex; align-items: center; gap: 2px;",
                        input {
                            r#type: "text",
                            placeholder: "{filter_placeholder}",
                            value: "{text}",
                            style: "flex: 1; min-width: 0; box-sizing: border-box; \
                                    padding: 2px 4px; border: 1px solid var(--chorale-input-border, #ccc); \
                                    border-radius: 2px; font-size: 0.8rem;",
                            oninput: move |e| {
                                let v = e.value();
                                if v.is_empty() {
                                    handle.set_filter(col_id, None);
                                } else {
                                    handle.set_filter(col_id, Some(FilterValue::Text(v)));
                                }
                            },
                        }
                        if has_filter {
                            {clear_filter_button(col_id, handle, &clear_label)}
                        }
                    }
                }
            }
        }
        FilterKind::MultiSelect { options } => {
            let has_filter = current.is_some();
            rsx! {
                th { style: "{th_style}",
                    div {
                        style: "display: flex; align-items: center; gap: 2px;",
                        div { style: "flex: 1; min-width: 0;",
                            MultiSelectFilter::<TRow> {
                                col_id,
                                options: options.clone(),
                                current: current.clone(),
                                handle,
                                all_label: all_label.clone(),
                            }
                        }
                        if has_filter {
                            {clear_filter_button(col_id, handle, &clear_label)}
                        }
                    }
                }
            }
        }
        FilterKind::NumericRange { min, max, step } => {
            let has_filter = current.is_some();
            rsx! {
                th { style: "{th_style}",
                    div {
                        style: "display: flex; align-items: center; gap: 2px;",
                        div { style: "flex: 1; min-width: 0;",
                            NumericRangeFilter::<TRow> {
                                col_id,
                                bound_min: *min,
                                bound_max: *max,
                                step: *step,
                                current: current.clone(),
                                handle,
                            }
                        }
                        if has_filter {
                            {clear_filter_button(col_id, handle, &clear_label)}
                        }
                    }
                }
            }
        }
        FilterKind::DateRange => {
            let has_filter = current.is_some();
            rsx! {
                th { style: "{th_style}",
                    div {
                        style: "display: flex; align-items: center; gap: 2px;",
                        div { style: "flex: 1; min-width: 0;",
                            DateRangeFilter::<TRow> {
                                col_id,
                                current: current.clone(),
                                handle,
                            }
                        }
                        if has_filter {
                            {clear_filter_button(col_id, handle, &clear_label)}
                        }
                    }
                }
            }
        }
        FilterKind::Boolean => {
            let has_filter = current.is_some();
            rsx! {
                th { style: "{th_style}",
                    div {
                        style: "display: flex; align-items: center; gap: 2px;",
                        div { style: "flex: 1; min-width: 0;",
                            BooleanFilter::<TRow> {
                                col_id,
                                current: current.clone(),
                                handle,
                            }
                        }
                        if has_filter {
                            {clear_filter_button(col_id, handle, &clear_label)}
                        }
                    }
                }
            }
        }
        _ => rsx! { th { style: "{empty_th_style}" } },
    }
}

// ---------------------------------------------------------------------------
// Per-FilterKind UI components
// ---------------------------------------------------------------------------

/// Small `×` button that clears the filter for `col_id` when clicked.
///
/// Rendered only when there's an active filter on the column (call sites
/// gate this on `current.is_some()` for Text/MultiSelect/Date/Numeric,
/// or on `is_some()` AND non-default state for ranges).
fn clear_filter_button<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    handle: UseTableHandle<TRow>,
    clear_label: &str,
) -> Element {
    let clear_label = clear_label.to_owned();
    rsx! {
        button {
            r#type: "button",
            title: "{clear_label}",
            style: "border: 0; background: transparent; padding: 0 4px; \
                    cursor: pointer; color: var(--chorale-text-subtle, #888); font-size: 0.95rem; \
                    line-height: 1; flex-shrink: 0;",
            onclick: move |e| {
                e.stop_propagation();
                handle.set_filter(col_id, None);
            },
            "\u{00d7}"
        }
    }
}

#[component]
fn MultiSelectFilter<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    options: Vec<String>,
    current: Option<FilterValue>,
    handle: UseTableHandle<TRow>,
    all_label: String,
) -> Element {
    // Install a one-time document-level pointerdown listener that closes any
    // open chorale dropdown when the click lands outside it. We tag each
    // `<details>` with `data-chorale-dropdown` and let the listener iterate
    // all currently-open ones. The `window.__chorale*` guard makes the
    // install idempotent: mounting many MultiSelectFilter components still
    // results in exactly one global listener.
    use_hook(|| {
        dioxus::document::eval(
            r"
            if (!window.__choraleDropdownOutsideClickWired) {
                window.__choraleDropdownOutsideClickWired = true;
                document.addEventListener('pointerdown', (e) => {
                    document
                        .querySelectorAll('details[data-chorale-dropdown][open]')
                        .forEach((d) => {
                            if (!d.contains(e.target)) {
                                d.open = false;
                            }
                        });
                }, true);
            }
            ",
        );
    });

    let selected: HashSet<String> = match &current {
        Some(FilterValue::MultiSelect(s)) => s.clone(),
        _ => HashSet::new(),
    };
    let summary_label = if selected.is_empty() || selected.len() == options.len() {
        all_label
    } else {
        format!("{} selected", selected.len())
    };

    rsx! {
        details {
            "data-chorale-dropdown": "true",
            style: "position: relative; font-size: 0.8rem;",
            summary {
                style: "cursor: pointer; padding: 2px 4px; border: 1px solid var(--chorale-input-border, #ccc); \
                        border-radius: 2px; background: var(--chorale-popover-bg, #fff); list-style: none;",
                "{summary_label} \u{25be}"
            }
            div {
                style: "position: absolute; left: 0; top: 100%; z-index: 10; \
                        background: var(--chorale-input-bg, #fff); border: 1px solid var(--chorale-input-border, #ccc); border-radius: 2px; \
                        padding: 4px 6px; min-width: 100%; white-space: nowrap; \
                        box-shadow: var(--chorale-popover-shadow, 0 2px 6px rgba(0,0,0,0.08)); max-height: 240px; \
                        overflow-y: auto;",
                for opt in options.iter() {
                    {multi_select_option(col_id, opt.clone(), selected.clone(), handle)}
                }
            }
        }
    }
}

fn multi_select_option<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    opt: String,
    selected: HashSet<String>,
    handle: UseTableHandle<TRow>,
) -> Element {
    let is_checked = selected.contains(&opt);
    let opt_label = opt.clone();
    rsx! {
        label {
            style: "display: flex; align-items: center; gap: 0.35rem; padding: 2px 0; \
                    cursor: pointer;",
            input {
                r#type: "checkbox",
                checked: is_checked,
                onchange: move |_| {
                    let mut next = selected.clone();
                    if is_checked {
                        next.remove(&opt);
                    } else {
                        next.insert(opt.clone());
                    }
                    if next.is_empty() {
                        handle.set_filter(col_id, None);
                    } else {
                        handle.set_filter(col_id, Some(FilterValue::MultiSelect(next)));
                    }
                },
            }
            "{opt_label}"
        }
    }
}

#[component]
fn NumericRangeFilter<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    bound_min: f64,
    bound_max: f64,
    step: f64,
    current: Option<FilterValue>,
    handle: UseTableHandle<TRow>,
) -> Element {
    let (cur_min, cur_max) = match &current {
        Some(FilterValue::NumericRange { min, max }) => {
            (min.unwrap_or(bound_min), max.unwrap_or(bound_max))
        }
        _ => (bound_min, bound_max),
    };
    let min_display = format_compact_number(cur_min);
    let max_display = format_compact_number(cur_max);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: 2px; font-size: 0.75rem;",
            div {
                style: "display: flex; justify-content: space-between; color: var(--chorale-text-muted, #555);",
                span { "{min_display}" }
                span { "{max_display}" }
            }
            input {
                r#type: "range",
                min: "{bound_min}",
                max: "{bound_max}",
                step: "{step}",
                value: "{cur_min}",
                style: "width: 100%; margin: 0;",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        let new_min = v.min(cur_max);
                        commit_numeric_range(col_id, new_min, cur_max, bound_min, bound_max, &handle);
                    }
                },
            }
            input {
                r#type: "range",
                min: "{bound_min}",
                max: "{bound_max}",
                step: "{step}",
                value: "{cur_max}",
                style: "width: 100%; margin: 0;",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<f64>() {
                        let new_max = v.max(cur_min);
                        commit_numeric_range(col_id, cur_min, new_max, bound_min, bound_max, &handle);
                    }
                },
            }
        }
    }
}

/// Decide what `FilterValue` corresponds to a numeric-range UI state.
///
/// Pure helper, extracted from `commit_numeric_range` so it can be unit-tested
/// without a Dioxus runtime. Returns `None` when both bounds are exactly at
/// the configured min/max — meaning the user has selected the full range,
/// equivalent to "no filter active." Otherwise returns a `NumericRange` with
/// each bound replaced by `None` when it sits exactly at the configured
/// extent (so the filter doesn't gratuitously over-specify).
#[must_use]
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

fn commit_numeric_range<TRow: Clone + PartialEq + 'static>(
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

#[component]
fn DateRangeFilter<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    current: Option<FilterValue>,
    handle: UseTableHandle<TRow>,
) -> Element {
    let (cur_min, cur_max) = match &current {
        Some(FilterValue::DateRange { min, max }) => (*min, *max),
        _ => (None, None),
    };
    let min_str = cur_min.map_or_else(String::new, |d| d.format("%Y-%m-%d").to_string());
    let max_str = cur_max.map_or_else(String::new, |d| d.format("%Y-%m-%d").to_string());

    rsx! {
        div {
            style: "display: flex; gap: 4px; font-size: 0.75rem;",
            input {
                r#type: "date",
                value: "{min_str}",
                style: "flex: 1; min-width: 0; padding: 1px 2px; border: 1px solid var(--chorale-input-border, #ccc); \
                        border-radius: 2px; font-size: 0.75rem;",
                // onchange (NOT oninput): the native date picker fires an
                // input event for every intermediate value while the user
                // scrolls the wheel — committing on each one re-filters the
                // table live mid-gesture. change fires only once a date is
                // actually selected/committed, matching the leptos adapter's
                // on:change date-filter semantics.
                onchange: move |e| {
                    let parsed = chorale_core::NaiveDate::parse_from_str(&e.value(), "%Y-%m-%d").ok();
                    commit_date_range(col_id, parsed, cur_max, &handle);
                },
            }
            input {
                r#type: "date",
                value: "{max_str}",
                style: "flex: 1; min-width: 0; padding: 1px 2px; border: 1px solid var(--chorale-input-border, #ccc); \
                        border-radius: 2px; font-size: 0.75rem;",
                // See the min input above — change-only commit, per leptos.
                onchange: move |e| {
                    let parsed = chorale_core::NaiveDate::parse_from_str(&e.value(), "%Y-%m-%d").ok();
                    commit_date_range(col_id, cur_min, parsed, &handle);
                },
            }
        }
    }
}

/// Decide what `FilterValue` corresponds to a date-range UI state.
///
/// Pure helper, extracted from `commit_date_range` so it can be unit-tested
/// without a Dioxus runtime. Returns `None` when both endpoints are absent
/// (equivalent to "no filter active") and `Some(DateRange { … })` otherwise.
#[must_use]
fn date_range_to_filter(
    min: Option<chorale_core::NaiveDate>,
    max: Option<chorale_core::NaiveDate>,
) -> Option<FilterValue> {
    if min.is_none() && max.is_none() {
        None
    } else {
        Some(FilterValue::DateRange { min, max })
    }
}

fn commit_date_range<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    min: Option<chorale_core::NaiveDate>,
    max: Option<chorale_core::NaiveDate>,
    handle: &UseTableHandle<TRow>,
) {
    handle.set_filter(col_id, date_range_to_filter(min, max));
}

#[component]
fn BooleanFilter<TRow: Clone + PartialEq + 'static>(
    col_id: ColumnId,
    current: Option<FilterValue>,
    handle: UseTableHandle<TRow>,
) -> Element {
    let cur = match &current {
        Some(FilterValue::Boolean(b)) => Some(*b),
        _ => None,
    };
    let selected_value = match cur {
        None => "all",
        Some(true) => "yes",
        Some(false) => "no",
    };
    rsx! {
        select {
            value: "{selected_value}",
            style: "width: 100%; box-sizing: border-box; padding: 2px 4px; \
                    border: 1px solid var(--chorale-input-border, #ccc); border-radius: 2px; font-size: 0.8rem; background: var(--chorale-input-bg, #fff);",
            onchange: move |e| {
                match e.value().as_str() {
                    "yes" => { handle.set_filter(col_id, Some(FilterValue::Boolean(true))); }
                    "no"  => { handle.set_filter(col_id, Some(FilterValue::Boolean(false))); }
                    _     => { handle.set_filter(col_id, None); }
                }
            },
            option { value: "all", "All" }
            option { value: "yes", "Yes" }
            option { value: "no",  "No" }
        }
    }
}

fn select_all_th<TRow: Clone + PartialEq + 'static>(
    handle: UseTableHandle<TRow>,
    all_page_selected: bool,
    sticky_header: bool,
) -> Element {
    // Explicit position/top/z-index in BOTH branches — see header_th's
    // comment about Dioxus 0.7's unreliable inline-style diff when a
    // declaration disappears entirely.
    let style = if sticky_header {
        "padding: 0.25rem 0.5rem; border-bottom: 1px solid var(--chorale-border, #ddd); position: sticky; \
         top: 0; background: var(--chorale-header-bg, #f8f9fa); z-index: 1; width: 2.5rem; text-align: center;"
    } else {
        "padding: 0.25rem 0.5rem; border-bottom: 1px solid var(--chorale-border, #ddd); position: static; \
         top: auto; z-index: auto; background: var(--chorale-header-bg, #f8f9fa); width: 2.5rem; text-align: center;"
    };
    rsx! {
        th {
            style: "{style}",
            input {
                r#type: "checkbox",
                checked: all_page_selected,
                // The header select-all operates on the FULL filtered set,
                // not just the rendered page: checking it selects every row
                // that passes the current filters (across all pages),
                // unchecking clears the entire selection. This mirrors the
                // leptos adapter's header checkbox, which calls
                // handle.select_all_filtered() / handle.deselect_all().
                // Page-scoped selection remains available through the
                // selection-toolbar affordances (select_all_visible_page).
                onchange: move |e| {
                    if e.checked() {
                        handle.select_all_filtered();
                    } else {
                        handle.deselect_all();
                    }
                },
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn data_tr<TRow: Clone + PartialEq + 'static>(
    row: &TRow,
    row_id: RowId,
    row_index: usize,
    variable_row_height: bool,
    visible_cols: &[ColumnDef<TRow>],
    row_height: f64,
    widths: &HashMap<ColumnId, f64>,
    selection_enabled: bool,
    is_selected: bool,
    handle: UseTableHandle<TRow>,
    cell_renderers: &CellRenderers,
    row_cell_renderers: &RowCellRenderers<TRow>,
    editing_col: Option<ColumnId>,
    editing_text: Signal<String>,
    edit_error: Signal<Option<String>>,
    validate_edit: &ValidateEditFn,
    on_commit_edit: Option<EventHandler<CommittedEdit<TRow>>>,
    sticky_css_map: &HashMap<ColumnId, String>,
    active_cell: Option<ActiveCell>,
    range_cells: &HashSet<(usize, ColumnId)>,
    fill_focus_cell: Option<(usize, ColumnId)>,
    fill_drag_active: Signal<bool>,
    fill_hover: Signal<Option<(usize, ColumnId)>>,
    has_detail: bool,
    is_expanded: bool,
    on_row_click: Option<Callback<RowId>>,
    // DOM id of the table's tabindex="0" keyboard container, threaded down to
    // editor_td so editors can refocus it after a keyboard commit/cancel.
    kb_id: &str,
) -> Element {
    // Row separator is rendered as a 1px inset box-shadow on each TD instead
    // of `border-bottom: 1px` on the TR. Reason: with `border-collapse: collapse`
    // a TR's `border-bottom` consumes 1px of layout per data row, while the
    // top_pad / bottom_pad spacer TRs have no border. The window math assumes
    // exactly `total_rows * row_height` of tbody content, but with TR borders
    // the actual tbody is `total_rows * row_height + N_rendered` px — and
    // N_rendered shifts as the user scrolls (different windows render
    // different counts). The resulting scroll-extent drift caused a runaway
    // scroll feedback loop. `box-shadow` is purely paint, never layout, so
    // the rendered row is exactly `row_height` px regardless of borders.
    // Note the explicit `background: transparent` on the deselected branch
    // rather than an empty string. Dioxus's attribute diff does not reliably
    // clear a previously-set inline style when the new value is `""`; the
    // tr keeps its old `background: var(--chorale-row-selected-bg, #eff6ff)` and the row stays blue after
    // the checkbox toggles off. Always emitting a concrete background
    // value forces the override.
    let (row_bg, separator_color) = if is_selected && selection_enabled {
        (
            "background: var(--chorale-row-selected-bg, #eff6ff);",
            "var(--chorale-row-selected-divider, #dbeafe)",
        )
    } else {
        (
            "background: transparent;",
            "var(--chorale-button-disabled-bg, #f0f0f0)",
        )
    };
    let row_style = format!("{row_bg}--chorale-separator-color: {separator_color};");
    rsx! {
        tr {
            style: "{row_style}",
            "data-chorale-index": "{row_index}",
            if selection_enabled {
                td {
                    style: "padding: 0.25rem 0.5rem; width: 2.5rem; text-align: center; \
                            box-shadow: inset 0 -1px 0 {separator_color};",
                    input {
                        r#type: "checkbox",
                        checked: is_selected,
                        onchange: move |_| handle.set_selection(row_id, !is_selected),
                    }
                }
            }
            if has_detail {
                td {
                    class: "chorale-cell chorale-detail-chevron",
                    style: "width: 24px; cursor: pointer; user-select: none; text-align: center; \
                            box-shadow: inset 0 -1px 0 {separator_color};",
                    "aria-label": if is_expanded { "Collapse row" } else { "Expand row" },
                    onclick: move |_| handle.toggle_row_expansion(row_id),
                    if is_expanded { "▼" } else { "▶" }
                }
            }
            for col in visible_cols {
                if editing_col == Some(col.id) {
                    {editor_td(
                        row,
                        row_id,
                        col,
                        row_height,
                        variable_row_height,
                        widths.get(&col.id).copied(),
                        separator_color,
                        editing_text,
                        edit_error,
                        validate_edit,
                        on_commit_edit,
                        handle,
                        sticky_css_map.get(&col.id).map_or("", String::as_str),
                        kb_id,
                    )}
                } else {
                    {
                        let is_active = active_cell.is_some_and(|ac| ac.row_idx == row_index && ac.column_id == col.id);
                        let is_in_range = range_cells.contains(&(row_index, col.id));
                        let is_focus_cell = fill_focus_cell == Some((row_index, col.id));
                        let cell_renderer = cell_renderers.get(col.id);
                        let row_renderer = row_cell_renderers.get(col.id);
                        data_td(row, col, row_height, variable_row_height, widths.get(&col.id).copied(), cell_renderer.as_ref(), row_renderer.as_ref(), separator_color, sticky_css_map.get(&col.id).map_or("", String::as_str), is_active, is_in_range, row_index, row_id, handle, is_focus_cell, fill_drag_active, fill_hover, on_row_click)
                    }
                }
            }
        }
    }
}

/// Dispatch a single `GroupedRow` to either `group_header_tr` or `data_tr`.
#[allow(clippy::too_many_arguments)]
fn render_grouped_row<TRow: Clone + PartialEq + 'static>(
    grouped_row: GroupedRow<TRow>,
    row_index: usize,
    effective_col_count: usize,
    selection_enabled: bool,
    has_detail: bool,
    handle: UseTableHandle<TRow>,
    group_header_class: &str,
    visible_cols: &[ColumnDef<TRow>],
    row_height: f64,
    widths: &HashMap<ColumnId, f64>,
    variable_row_height: bool,
    cell_renderers: &CellRenderers,
    row_cell_renderers: &RowCellRenderers<TRow>,
    editing_target: Option<chorale_core::EditTarget>,
    editing_text: Signal<String>,
    edit_error: Signal<Option<String>>,
    validate_edit: &ValidateEditFn,
    on_commit_edit: Option<EventHandler<CommittedEdit<TRow>>>,
    sticky_body_css: &HashMap<ColumnId, String>,
    selection_set: &HashSet<RowId>,
    active_cell: Option<ActiveCell>,
    range_cells: &HashSet<(usize, ColumnId)>,
    fill_focus_cell: Option<(usize, ColumnId)>,
    fill_drag_active: Signal<bool>,
    fill_hover: Signal<Option<(usize, ColumnId)>>,
    on_row_click: Option<Callback<RowId>>,
    // DOM id of the table's tabindex="0" keyboard container, threaded down to
    // editor_td so editors can refocus it after a keyboard commit/cancel.
    kb_id: &str,
) -> Element {
    match grouped_row {
        GroupedRow::Header {
            key,
            label,
            depth,
            row_count,
            is_collapsed,
            aggregates,
        } => group_header_tr(
            key,
            label,
            depth,
            row_count,
            is_collapsed,
            aggregates,
            effective_col_count,
            selection_enabled,
            handle,
            group_header_class,
        ),
        GroupedRow::Data(row_id, row) => {
            let editing_col = editing_target
                .filter(|t| t.row_id == row_id)
                .map(|t| t.column_id);
            data_tr(
                &row,
                row_id,
                row_index,
                variable_row_height,
                visible_cols,
                row_height,
                widths,
                selection_enabled,
                selection_set.contains(&row_id),
                handle,
                cell_renderers,
                row_cell_renderers,
                editing_col,
                editing_text,
                edit_error,
                validate_edit,
                on_commit_edit,
                sticky_body_css,
                active_cell,
                range_cells,
                fill_focus_cell,
                fill_drag_active,
                fill_hover,
                has_detail,
                false,
                on_row_click,
                kb_id,
            )
        }
        _ => rsx! {},
    }
}

/// Render a single group-header `<tr>`.
///
/// Clicking the row (or the toggle button) calls `toggle_group` on the handle.
/// Depth is expressed as left-padding on the first cell (8px per level).
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn group_header_tr<TRow: Clone + PartialEq + 'static>(
    key: GroupKey,
    label: String,
    depth: usize,
    row_count: usize,
    is_collapsed: bool,
    _aggregates: Vec<Option<CellValue>>,
    col_count: usize,
    selection_enabled: bool,
    handle: UseTableHandle<TRow>,
    extra_class: &str,
) -> Element {
    let indent = depth * 16;
    let toggle_icon = if is_collapsed { "\u{25b6}" } else { "\u{25bc}" };
    let extra_class = extra_class.to_owned();
    rsx! {
        tr {
            class: "{extra_class}",
            style: "background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;",
            onclick: move |_| { handle.toggle_group(key.clone()); },
            if selection_enabled {
                td { style: "padding: 0.25rem 0.5rem; width: 2.5rem;" }
            }
            td {
                colspan: "{col_count - usize::from(selection_enabled)}",
                style: "padding: 0.4rem 1rem 0.4rem {indent}px; \
                        border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;",
                span {
                    style: "margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);",
                    "{toggle_icon}"
                }
                "{label}"
                span {
                    style: "margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; \
                            color: var(--chorale-text-subtle, #888);",
                    "({row_count})"
                }
            }
        }
    }
}

/// Return keyboard focus to the table's keyboard container (the
/// `tabindex="0"` div that owns the grid-level keydown handler).
///
/// Editors call this after a keyboard-initiated commit/cancel: the editor
/// element is about to unmount, and removing a focused element does NOT fire
/// blur — the browser silently drops focus to `<body>`, so arrow keys would
/// scroll the page instead of navigating cells. Deliberately NOT called from
/// onblur — blur is an intentional click-away and must not steal focus back.
fn refocus_keyboard_container(kb_id: &str) {
    dioxus::document::eval(&format!(
        "var el=document.getElementById('{kb_id}');if(el)el.focus();"
    ));
}

#[allow(clippy::too_many_arguments, clippy::if_not_else)]
fn editor_td<TRow: Clone + PartialEq + 'static>(
    row: &TRow,
    row_id: RowId,
    col: &ColumnDef<TRow>,
    row_height: f64,
    variable_row_height: bool,
    override_width: Option<f64>,
    separator_color: &str,
    mut editing_text: Signal<String>,
    mut edit_error: Signal<Option<String>>,
    validate: &ValidateEditFn,
    on_commit_edit: Option<EventHandler<CommittedEdit<TRow>>>,
    handle: UseTableHandle<TRow>,
    sticky_css: &str,
    // DOM id of the table's tabindex="0" keyboard container; editors refocus
    // it after a keyboard commit/cancel so arrow-key navigation resumes.
    kb_id: &str,
) -> Element {
    let col_id = col.id;
    let editor_kind = col.editor.clone().unwrap_or(EditorKind::Text);
    let align = alignment_css(col.alignment);
    let w = col_width_style(override_width, col.initial_width);
    let style = if variable_row_height {
        format!(
            "padding: 0.25rem 0.5rem; box-sizing: border-box; \
             box-shadow: inset 0 -1px 0 {separator_color}; {w} {sticky_css}"
        )
    } else {
        format!(
            "padding: 0.25rem 0.5rem; height: {row_height}px; box-sizing: border-box; \
             box-shadow: inset 0 -1px 0 {separator_color}; {w} {sticky_css}"
        )
    };
    let input_type = match &editor_kind {
        EditorKind::Number { .. } => "number",
        EditorKind::Date => "date",
        EditorKind::BoolToggle => "checkbox",
        _ => "text",
    };
    let (num_min, num_max, num_step) = match &editor_kind {
        EditorKind::Number { min, max, step } => (
            min.map(|v| v.to_string()).unwrap_or_default(),
            max.map(|v| v.to_string()).unwrap_or_default(),
            step.map(|v| v.to_string()).unwrap_or_default(),
        ),
        _ => (String::new(), String::new(), String::new()),
    };
    let prior_row = row.clone();
    let text_val = editing_text.read().clone();
    let err_val = edit_error.read().clone();
    let validate = validate.clone();
    // Clone for the onblur closure; onkeydown also needs validate/prior_row.
    let validate_blur = validate.clone();
    let prior_row_blur = row.clone();

    let validate_select = validate.clone();
    let prior_row_select = row.clone();
    let select_options: Vec<String> = if let EditorKind::Select { options } = &editor_kind {
        options.clone()
    } else {
        Vec::new()
    };
    let is_select = matches!(&editor_kind, EditorKind::Select { .. });
    let selected_val = text_val.clone();

    // Owned copies of the keyboard-container id, one per closure that hands
    // focus back to the grid after a keyboard-initiated commit/cancel.
    let kb_for_select_change = kb_id.to_owned();
    let kb_for_select_keys = kb_id.to_owned();
    let kb_for_input_keys = kb_id.to_owned();

    // Editor visual parity with the display cell (data_td): the display td
    // pads 0.5rem 1rem and aligns text per column alignment. The editor td
    // pads 0.25rem 0.5rem, so the editor element's own padding tops it up to
    // the same text inset: 0.25rem(td) + 1px(border) + calc(0.25rem - 1px)
    // = 0.5rem vertically, 0.5rem + 1px + calc(0.5rem - 1px) = 1rem
    // horizontally. font: inherit + text-align: {align} keep the glyphs the
    // same size and justification, so entering edit mode doesn't shift text.
    let editor_style = format!(
        "width: 100%; box-sizing: border-box; font: inherit; text-align: {align}; \
         padding: calc(0.25rem - 1px) calc(0.5rem - 1px); \
         border: 1px solid var(--chorale-accent, #4a90e2); border-radius: 2px;"
    );

    // Select editor: a native <select> constrained to the column's options.
    // Commits on change (a pick IS the commit), mirroring the text editor's
    // validate -> on_commit_edit -> commit_edit/sig.set flow; Esc cancels.
    let editor_el = if is_select {
        rsx! {
            select {
                "data-chorale-editor": "true",
                value: "{selected_val}",
                style: "{editor_style}",
                onchange: move |e| {
                    let raw = e.value();
                    let result = validate_select.call(EditValidation {
                        row_id,
                        column_id: col_id,
                        raw_value: raw.clone(),
                    });
                    match result {
                        Ok(()) => {
                            edit_error.set(None);
                            if let Some(handler) = &on_commit_edit {
                                handler.call(CommittedEdit::new(
                                    row_id,
                                    col_id,
                                    raw.clone(),
                                    prior_row_select.clone(),
                                ));
                            }
                            let mut sig = handle.signal();
                            let new_state = commit_edit(&*sig.read());
                            sig.set(new_state);
                            // Commit unmounts the <select>; hand focus back
                            // to the grid container so a keyboard pick
                            // (arrows / typing a letter) flows straight back
                            // into cell navigation. Without this, focus
                            // silently drops to <body> when the element is
                            // removed, and arrow keys scroll the page.
                            refocus_keyboard_container(&kb_for_select_change);
                        }
                        Err(msg) => edit_error.set(Some(msg)),
                    }
                },
                onblur: move |_| {
                    // Guard: blur also fires when a keyboard commit/cancel
                    // refocuses the grid container while the <select> is
                    // still mounted. In that case `editing` no longer points
                    // at this cell and the cleanup below must NOT run — it
                    // would wipe the still-needed active_cell.
                    {
                        let sig_r = handle.signal();
                        let still_editing = sig_r
                            .peek()
                            .editing
                            .is_some_and(|t| t.row_id == row_id && t.column_id == col_id);
                        if !still_editing {
                            return;
                        }
                    }
                    // A native <select> fires NO change event when the user
                    // re-picks the already-selected option, so without a blur
                    // exit the cell would be stuck in edit mode. Any real pick
                    // has already committed via onchange (which runs before
                    // blur), so blur only needs to leave edit mode — mirror
                    // the text editor's onblur clear (editing + active cell +
                    // range) without firing a spurious on_commit_edit.
                    let mut sig = handle.signal();
                    let new_state = {
                        let snap = sig.peek();
                        let mut copy = snap.clone();
                        copy.editing = None;
                        copy.active_cell = None;
                        copy.range_selection.clear();
                        copy
                    };
                    sig.set(new_state);
                },
                onkeydown: move |e: KeyboardEvent| {
                    // While editing, no key may leak to the grid-level keydown
                    // handler: it would prevent_default the arrow keys (which
                    // must change the <select> value) and move the active
                    // cell behind the editor's back.
                    e.stop_propagation();
                    match e.key() {
                        Key::Escape => {
                            let mut sig = handle.signal();
                            let new_state = cancel_edit(&*sig.read());
                            sig.set(new_state);
                            refocus_keyboard_container(&kb_for_select_keys);
                        }
                        Key::Enter => {
                            // A real pick has already committed via onchange;
                            // Enter on the still-open editor (e.g. after
                            // re-picking the same value, which fires no
                            // change event) just closes it. commit_edit
                            // clears `editing` but keeps `active_cell`, so
                            // navigation resumes from this cell.
                            let mut sig = handle.signal();
                            let new_state = commit_edit(&*sig.read());
                            sig.set(new_state);
                            refocus_keyboard_container(&kb_for_select_keys);
                        }
                        Key::Tab => {
                            // Mirror the text editor: Tab cycles the edit
                            // cursor through the row's editable columns. The
                            // next editor auto-focuses via the edit-target
                            // effect, so no container refocus here.
                            e.prevent_default();
                            let mut sig = handle.signal();
                            let new_state = if e.modifiers().contains(Modifiers::SHIFT) {
                                prev_editable_cell(&*sig.read())
                            } else {
                                next_editable_cell(&*sig.read())
                            };
                            sig.set(new_state);
                        }
                        _ => {}
                    }
                },
                for opt in select_options.iter() {
                    option { value: "{opt}", "{opt}" }
                }
            }
        }
    } else {
        rsx! {
            input {
                "data-chorale-editor": "true",
                r#type: "{input_type}",
                value: "{text_val}",
                min: if !num_min.is_empty() { "{num_min}" },
                max: if !num_max.is_empty() { "{num_max}" },
                step: if !num_step.is_empty() { "{num_step}" },
                style: "{editor_style}",
                oninput: move |e| editing_text.set(e.value()),
                onblur: move |_| {
                    // Guard: blur also fires when a keyboard commit/cancel
                    // (Enter/Escape below) refocuses the grid container while
                    // this <input> is still mounted, and when Tab has already
                    // moved `editing` to another cell. In both cases the
                    // commit below must NOT run again — it would fire a
                    // duplicate on_commit_edit and wipe the still-needed
                    // active_cell.
                    {
                        let sig_r = handle.signal();
                        let still_editing = sig_r
                            .peek()
                            .editing
                            .is_some_and(|t| t.row_id == row_id && t.column_id == col_id);
                        if !still_editing {
                            return;
                        }
                    }
                    let raw = editing_text.peek().clone();
                    let result = validate_blur.call(EditValidation {
                        row_id,
                        column_id: col_id,
                        raw_value: raw.clone(),
                    });
                    if result.is_ok() {
                        edit_error.set(None);
                        if let Some(handler) = &on_commit_edit {
                            handler.call(CommittedEdit::new(
                                row_id,
                                col_id,
                                raw.clone(),
                                prior_row_blur.clone(),
                            ));
                        }
                        // Clone-mutate-set, NOT sig.write(). The view memo's
                        // view_key intentionally does not track row content
                        // (perf optimisation: at 1M rows it avoids re-hashing
                        // the full dataset per scroll tick). sig.write() only
                        // invalidates fine-grained subscribers of the fields
                        // it touched (editing, active_cell, range_selection)
                        // and DOES NOT invalidate the view memo — so even
                        // though state.rows holds the new row, the cell would
                        // keep rendering the stale memoised view. sig.set()
                        // on a fresh clone triggers the broader invalidation
                        // the view memo picks up. The data_generation bump
                        // inside update_row carries the cell-edit signal
                        // through view_key for callers that already update
                        // via the transition.
                        let mut sig = handle.signal();
                        let new_state = {
                            let snap = sig.peek();
                            let mut copy = snap.clone();
                            copy.editing = None;
                            copy.active_cell = None;
                            copy.range_selection.clear();
                            copy
                        };
                        sig.set(new_state);
                    }
                    // On validation error, leave editing open so user can fix.
                },
                onkeydown: move |e: KeyboardEvent| {
                    // While editing, no key may leak to the grid-level keydown
                    // handler. Two concrete failure modes without this:
                    // Enter would hit the container's Enter-opens-editor arm
                    // (commit_edit keeps `active_cell`, so it would re-open
                    // the editor we just committed), and arrow keys would be
                    // prevent_default-ed by the container's navigation arm
                    // (freezing the caret) while moving the active cell
                    // behind the editor's back.
                    e.stop_propagation();
                    match e.key() {
                        Key::Enter => {
                            let raw = editing_text.read().clone();
                            let result = validate.call(EditValidation {
                                row_id,
                                column_id: col_id,
                                raw_value: raw.clone(),
                            });
                            match result {
                                Ok(()) => {
                                    edit_error.set(None);
                                    if let Some(handler) = &on_commit_edit {
                                        handler.call(CommittedEdit::new(
                                            row_id,
                                            col_id,
                                            raw,
                                            prior_row.clone(),
                                        ));
                                    }
                                    let mut sig = handle.signal();
                                    let new_state = commit_edit(&*sig.read());
                                    sig.set(new_state);
                                    // Successful keyboard commit: the input is
                                    // about to unmount, which silently drops
                                    // focus to <body>. Refocus the grid
                                    // container so navigation resumes from the
                                    // (still-set) active cell. Deliberately
                                    // NOT done in onblur — blur is an
                                    // intentional click-away and must not
                                    // steal focus back.
                                    refocus_keyboard_container(&kb_for_input_keys);
                                }
                                Err(msg) => edit_error.set(Some(msg)),
                            }
                        }
                        Key::Escape => {
                            let mut sig = handle.signal();
                            let new_state = cancel_edit(&*sig.read());
                            sig.set(new_state);
                            // Same refocus rationale as the Enter arm above.
                            refocus_keyboard_container(&kb_for_input_keys);
                        }
                        Key::Tab => {
                            // Tab cycles `editing` to the adjacent editable
                            // column; the next editor auto-focuses via the
                            // edit-target effect, so no container refocus.
                            e.prevent_default();
                            let mut sig = handle.signal();
                            let new_state = if e.modifiers().contains(Modifiers::SHIFT) {
                                prev_editable_cell(&*sig.read())
                            } else {
                                next_editable_cell(&*sig.read())
                            };
                            sig.set(new_state);
                        }
                        _ => {}
                    }
                },
            }
        }
    };

    rsx! {
        td {
            style: "{style}",
            {editor_el}
            if let Some(err) = err_val {
                div {
                    style: "color: var(--chorale-error, #c0392b); font-size: 0.75rem; margin-top: 2px;",
                    "{err}"
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn data_td<TRow: Clone + PartialEq + 'static>(
    row: &TRow,
    col: &ColumnDef<TRow>,
    row_height: f64,
    variable_row_height: bool,
    override_width: Option<f64>,
    custom_renderer: Option<&CellRenderer>,
    row_renderer: Option<&RowCellRenderer<TRow>>,
    separator_color: &str,
    sticky_css: &str,
    is_active_cell: bool,
    is_in_range: bool,
    row_index: usize,
    row_id: RowId,
    handle: UseTableHandle<TRow>,
    _is_focus_cell: bool,
    fill_drag_active: Signal<bool>,
    mut fill_hover: Signal<Option<(usize, ColumnId)>>,
    on_row_click: Option<Callback<RowId>>,
) -> Element {
    let val = (col.accessor)(row);
    let align = alignment_css(col.alignment);
    let w = col_width_style(override_width, col.initial_width);
    // is_in_range + is_active_cell visuals render as conditional overlay
    // <div>s, NOT as inline-style declarations on the <td>. Dioxus 0.7's
    // attribute diff doesn't reliably re-emit a changed `style` on <td>s
    // when range_selection clears, so the blue highlight visually persists
    // even though state is correctly updated. A child overlay node that
    // either exists or doesn't forces a structural DOM mutation that the
    // diff can't drop.
    let style = if variable_row_height {
        format!(
            "position: relative; padding: 0.5rem 1rem; text-align: {align}; \
             box-sizing: border-box; box-shadow: inset 0 -1px 0 {separator_color}; \
             cursor: default; {w} {sticky_css}"
        )
    } else {
        format!(
            "position: relative; padding: 0.5rem 1rem; height: {row_height}px; text-align: {align}; \
             white-space: nowrap; overflow: hidden; text-overflow: ellipsis; \
             box-sizing: border-box; box-shadow: inset 0 -1px 0 {separator_color}; \
             cursor: default; {w} {sticky_css}"
        )
    };
    let content = resolve_cell_content(row, &val, &col.render_kind, row_renderer, custom_renderer);
    let col_id = col.id;
    rsx! {
        td {
            style: "{style}",
            onclick: move |e: MouseEvent| {
                let ctrl = e.modifiers().contains(Modifiers::CONTROL)
                    || e.modifiers().contains(Modifiers::META);
                let shift = e.modifiers().contains(Modifiers::SHIFT);
                let mut sig_w = handle.signal();
                // Plain click (no modifier): explicitly clear range_selection
                // FIRST, then apply the new single-cell range as a separate
                // signal write. start_range_selection already replaces the
                // range in state, but a single combined write doesn't reliably
                // re-render the previously-highlighted cells in Dioxus 0.7 —
                // the two-pass clear + set forces a clean signal transition
                // that downstream renders see unambiguously.
                if !ctrl && !shift {
                    let cleared = clear_range_selection(&*sig_w.peek());
                    sig_w.set(cleared);
                }
                let new_s = if ctrl {
                    add_disjoint_range(&*sig_w.peek(), row_index, col_id)
                } else if shift {
                    extend_range_to(&*sig_w.peek(), row_index, col_id)
                } else {
                    start_range_selection(&*sig_w.peek(), row_index, col_id)
                };
                sig_w.set(new_s);
                if should_fire_row_click(ctrl, shift) {
                    if let Some(cb) = on_row_click {
                        cb.call(row_id);
                    }
                }
            },
            ondoubleclick: move |_| {
                handle.start_edit(row_id, col_id);
            },
            onmouseenter: move |_| {
                if *fill_drag_active.peek() {
                    fill_hover.set(Some((row_index, col_id)));
                }
            },
            // Range-cell overlay (children must come after attributes per rsx!
            // ordering rules). pointer-events:none so clicks pass through.
            if is_in_range {
                div {
                    style: "position: absolute; inset: 0; \
                            background: var(--chorale-range-bg, rgba(0, 120, 212, 0.1)); \
                            pointer-events: none; z-index: 1;",
                }
            }
            // Active-cell outline overlay. Drawn inset; sits over the range
            // background but below interactive content.
            if is_active_cell {
                div {
                    style: "position: absolute; inset: 0; \
                            outline: 2px solid var(--chorale-active-cell-outline, #0078d4); \
                            outline-offset: -2px; \
                            pointer-events: none; z-index: 2;",
                }
            }
            {content}
            // No fill-handle visual: a 6×6 blue square at the bottom-right
            // of the active cell is too small to be a reliable drag target
            // on a 40 px row, and Shift+click already covers the
            // "extend a range to here" use case Excel-style. is_focus_cell
            // is still computed at the call site so it's available if a
            // visual cue is added later.
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
) -> Element {
    if let Some(rr) = row_renderer {
        rr(row, val)
    } else if let Some(vr) = value_renderer {
        vr(val)
    } else {
        cell_element(val, render_kind)
    }
}

/// A plain left-click on a data cell is a "row click"; Ctrl/Cmd/Shift
/// clicks are range-selection operations and must not fire `on_row_click`.
fn should_fire_row_click(ctrl: bool, shift: bool) -> bool {
    !ctrl && !shift
}

fn cell_element(val: &CellValue, kind: &RenderKind) -> Element {
    match (val, kind) {
        (CellValue::Boolean(b), RenderKind::Boolean) => {
            let icon = if *b { "\u{2713}" } else { "\u{2717}" };
            rsx! { span { "{icon}" } }
        }
        (CellValue::Text(s), RenderKind::Badge(map)) => badge_span(s, map),
        _ => {
            let text = cell_text(val, kind);
            rsx! { span { "{text}" } }
        }
    }
}

fn badge_span(text: &str, map: &BadgeVariantMap) -> Element {
    if let Some(v) = map.resolve(text) {
        let label = v.label.clone();
        let style = badge_style(&v.color);
        rsx! { span { style: "{style}", "{label}" } }
    } else {
        rsx! { span { "{text}" } }
    }
}

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

fn cell_text(val: &CellValue, kind: &RenderKind) -> String {
    match (val, kind) {
        (CellValue::Text(s), _) => s.clone(),
        (CellValue::Integer(n), RenderKind::Number) => format_thousands(*n),
        (CellValue::Integer(n), RenderKind::Currency(code)) => {
            format!("{}{}.00", currency_symbol(code), format_thousands(*n))
        }
        (CellValue::Float(f), RenderKind::Number) => format!("{f:.0}"),
        #[allow(clippy::cast_precision_loss)]
        (CellValue::Float(f), RenderKind::Currency(code)) => {
            format!("{}{f:.2}", currency_symbol(code))
        }
        (CellValue::Date(d), RenderKind::Date) => d.format("%Y-%m-%d").to_string(),
        (CellValue::DateTime(dt), RenderKind::DateTime) => dt.format("%Y-%m-%d %H:%M").to_string(),
        (CellValue::Boolean(b), _) => (if *b { "\u{2713}" } else { "\u{2717}" }).to_string(),
        _ => val.to_csv_string(),
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

fn currency_symbol(code: &CurrencyCode) -> &'static str {
    match code.0 {
        "USD" => "$",
        "EUR" => "\u{20ac}",
        "GBP" => "\u{00a3}",
        _ => "",
    }
}

#[allow(clippy::match_same_arms)]
fn alignment_css(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        // `Alignment` is #[non_exhaustive]; default unknown variants to left.
        _ => "left",
    }
}

fn col_width_style(override_px: Option<f64>, initial_px: Option<f64>) -> String {
    let w = override_px.or(initial_px);
    w.map_or_else(String::new, |px| {
        format!("width: {px}px; min-width: {px}px; max-width: {px}px;")
    })
}

// ---------------------------------------------------------------------------
// Pagination helpers
// ---------------------------------------------------------------------------

/// Returns a page-button descriptor list for rendering.
///
/// `None` entries represent an ellipsis ("…") between non-contiguous page
/// groups. All page indices are zero-based.
fn page_button_range(current: usize, total: usize) -> Vec<Option<usize>> {
    if total == 0 {
        return vec![];
    }
    if total <= 7 {
        return (0..total).map(Some).collect();
    }
    let mut result = Vec::with_capacity(9);
    let mut prev_shown = false;
    for p in 0..total {
        let show = p == 0 || p + 1 == total || p.abs_diff(current) <= 2;
        if show {
            if !prev_shown && p > 0 {
                result.push(None);
            }
            result.push(Some(p));
            prev_shown = true;
        } else {
            prev_shown = false;
        }
    }
    result
}

/// Number input that jumps to an arbitrary page. Use case: with 200+ pages,
/// the windowed page-button list is not enough to navigate. The input
/// commits on Enter or blur (the `onchange` event on a number input fires on
/// both). Out-of-range entries are clamped to `[1, total_pages]`;
/// non-numeric entries snap back to the current page.
#[component]
fn GotoPageInput<TRow: Clone + PartialEq + 'static>(
    handle: UseTableHandle<TRow>,
    total_pages: usize,
    labels: Labels,
) -> Element {
    let sig = handle.signal();
    // Memo over JUST the page index so the use_effect re-syncs the draft
    // value only on actual page transitions, not on every state change.
    let page_memo = use_memo(move || sig.read().page);
    let mut draft = use_signal(|| (*page_memo.read() + 1).to_string());

    use_effect(move || {
        let p = *page_memo.read();
        draft.set((p + 1).to_string());
    });

    let max_page = total_pages.max(1);
    let page_count_str = (labels.page_count)(*page_memo.read() + 1, max_page);

    rsx! {
        span {
            style: "display: inline-flex; align-items: center; gap: 0.25rem; \
                    color: var(--chorale-text-muted, #555); font-size: 0.875rem;",
            "{labels.go_to_page_label}"
            input {
                r#type: "number",
                min: "1",
                max: "{max_page}",
                value: "{draft.read()}",
                style: "width: 4.5em; padding: 2px 4px; border: 1px solid var(--chorale-input-border, #ccc); \
                        border-radius: 2px; font-size: 0.875rem; text-align: center;",
                oninput: move |e| draft.set(e.value()),
                onchange: move |e| {
                    if let Ok(n) = e.value().parse::<usize>() {
                        let clamped = n.clamp(1, max_page);
                        handle.set_page(clamped - 1).ok();
                    } else {
                        // Non-numeric or empty input — snap back.
                        draft.set((*page_memo.read() + 1).to_string());
                    }
                },
            }
            "{page_count_str}"
        }
    }
}

fn render_page_btn<TRow: Clone + PartialEq + 'static>(
    item: Option<usize>,
    current_idx: usize,
    handle: UseTableHandle<TRow>,
) -> Element {
    let Some(p) = item else {
        return rsx! {
            span {
                style: "padding: 0 0.25rem; color: var(--chorale-text-disabled, #aaa); font-size: 0.875rem;",
                "\u{2026}"
            }
        };
    };
    let is_active = p == current_idx;
    let style = if is_active {
        "padding:0.25rem 0.5rem;border:1px solid var(--chorale-accent, #4a90e2);background:var(--chorale-accent, #4a90e2);\
         color:var(--chorale-accent-contrast, white);border-radius:3px;cursor:default;font-size:0.875rem;"
    } else {
        "padding:0.25rem 0.5rem;border:1px solid var(--chorale-border, #ddd);background:var(--chorale-button-bg, white);\
         color:var(--chorale-text, #333);border-radius:3px;cursor:pointer;font-size:0.875rem;"
    };
    rsx! {
        button {
            style: "{style}",
            onclick: move |_| { handle.set_page(p).ok(); },
            "{p + 1}"
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::compute_window_slice;
    use chorale_core::{
        visible_row_ids, visible_view, visible_window_for_state, Alignment, CellValue, ColumnDef,
        ColumnId, RenderKind, RenderRow, RowId, SortDirection, SortState, TableState,
    };

    #[derive(Clone, Debug, PartialEq)]
    struct R {
        name: String,
        score: i64,
    }

    fn make_state(scroll_top: f64, row_height: f64, viewport: f64) -> TableState<R> {
        let rows: Vec<(RowId, R)> = (0..50)
            .map(|i| {
                (
                    RowId::new(),
                    R {
                        name: format!("Row {i}"),
                        score: i,
                    },
                )
            })
            .collect();
        let columns = vec![ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
            CellValue::Integer(r.score)
        })
        .sortable()
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Number)];
        let mut s = TableState::new(rows, columns);
        s.sort = vec![SortState::new(ColumnId("score"), SortDirection::Asc)];
        s.page_size = 100;
        s.scroll_top = scroll_top;
        s.viewport_height = viewport;
        s.row_height = row_height;
        s.buffer_rows = 2;
        s
    }

    /// **Wiring-bug regression guard.** Asserts that `compute_window_slice`
    /// (the post-dedupe helper) produces a window + row slice + id slice
    /// that match what `visible_window_for_state` + `visible_row_ids`
    /// (the pre-dedupe pair) would have produced. A future refactor that
    /// drifts the helper out of sync with the legacy API surface will fail
    /// here.
    #[test]
    fn compute_window_slice_matches_legacy_api() {
        let state = make_state(200.0, 40.0, 300.0);
        let view = visible_view(&state);

        let (helper_win, helper_slice) = compute_window_slice(&state, &view, false);

        // Extract ids and rows from RenderRow::Data entries (no DetailPanel rows in
        // this plain state, but the extraction logic must be correct for parity).
        let helper_ids: Vec<RowId> = helper_slice
            .iter()
            .filter_map(|r| {
                if let RenderRow::Data { id, .. } = r {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        let helper_rows: Vec<R> = helper_slice
            .iter()
            .filter_map(|r| {
                if let RenderRow::Data { row, .. } = r {
                    Some(row.clone())
                } else {
                    None
                }
            })
            .collect();

        // Legacy reference path: two independent calls into chorale-core that
        // collectively did the work compute_window_slice now does once.
        let (legacy_win, legacy_rows) = visible_window_for_state(&state);
        let legacy_all_ids = visible_row_ids(&state);
        let win_end = legacy_win
            .end_index
            .min(legacy_all_ids.len().saturating_sub(1));
        let legacy_ids: Vec<RowId> = if legacy_all_ids.is_empty() || legacy_rows.is_empty() {
            vec![]
        } else {
            legacy_all_ids[legacy_win.start_index..=win_end].to_vec()
        };

        assert_eq!(helper_win, legacy_win, "window math drifted");
        assert_eq!(helper_rows, legacy_rows, "row slice drifted");
        assert_eq!(helper_ids, legacy_ids, "id slice drifted");
    }

    #[test]
    fn compute_window_slice_handles_empty_view() {
        let mut state = make_state(0.0, 40.0, 300.0);
        Arc::make_mut(&mut state.rows).clear();
        let view = visible_view(&state);
        let (win, slice) = compute_window_slice(&state, &view, false);
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 0);
        assert!(slice.is_empty());
    }

    /// Asserts `compute_window_slice` is deterministic given the same view
    /// and state — a regression here would suggest hidden non-determinism
    /// (e.g. iteration-order-dependent logic in the slicing).
    #[test]
    fn compute_window_slice_is_deterministic() {
        let state = make_state(120.0, 30.0, 200.0);
        let view = visible_view(&state);
        let (w1, s1) = compute_window_slice(&state, &view, false);
        let (w2, s2) = compute_window_slice(&state, &view, false);
        assert_eq!(w1, w2);
        assert_eq!(s1, s2);
    }

    /// Page count = 1 → single button rendered for page 0.
    #[test]
    fn compute_window_slice_clamps_scroll_past_content() {
        // A stale scroll_top can outrun the page content after a sort/filter
        // shrinks the view. The window math should not panic and should not
        // produce a negative-arithmetic out-of-bounds slice.
        let state = make_state(10_000.0, 40.0, 300.0);
        let view = visible_view(&state);
        let (win, slice) = compute_window_slice(&state, &view, false);
        assert!(win.end_index < view.len());
        assert!(!slice.is_empty());
    }

    // ---- scroll_top_to_reveal_row -------------------------------------------

    /// Row fully inside the viewport → no scroll adjustment.
    #[test]
    fn reveal_row_returns_none_when_row_already_visible() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [100, 400): row 5 spans [200, 240) — fully visible.
        let r = super::scroll_top_to_reveal_row(5, 100.0, 300.0, 40.0, &heights, false);
        assert_eq!(r, None);
    }

    /// Row top above `scroll_top` → scroll up so the row top hits the viewport top.
    #[test]
    fn reveal_row_scrolls_up_to_row_top() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [400, 700): row 3 spans [120, 160) — above.
        let r = super::scroll_top_to_reveal_row(3, 400.0, 300.0, 40.0, &heights, false);
        assert_eq!(r, Some(120.0));
    }

    /// Row bottom below the viewport → scroll down so the row bottom hits
    /// the viewport bottom.
    #[test]
    fn reveal_row_scrolls_down_to_row_bottom_minus_viewport() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [0, 300): row 10 spans [400, 440) — below.
        // Expected: 440 - 300 = 140.
        let r = super::scroll_top_to_reveal_row(10, 0.0, 300.0, 40.0, &heights, false);
        assert_eq!(r, Some(140.0));
    }

    /// Row partially clipped at the bottom edge also triggers a scroll.
    #[test]
    fn reveal_row_scrolls_when_row_straddles_bottom_edge() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [0, 300): row 7 spans [280, 320) — straddles the bottom.
        let r = super::scroll_top_to_reveal_row(7, 0.0, 300.0, 40.0, &heights, false);
        assert_eq!(r, Some(20.0));
    }

    /// Variable mode sums measured heights for rows above, falling back to
    /// `row_height` for unmeasured rows.
    #[test]
    fn reveal_row_variable_mode_sums_measured_heights() {
        let mut heights: HashMap<usize, f64> = HashMap::new();
        heights.insert(0, 100.0);
        heights.insert(1, 60.0);
        // Row 2 unmeasured (falls back to 40). Row 3 top = 100 + 60 + 40 = 200,
        // measured height 80 → bottom 280. Viewport [400, 700) → above →
        // scroll to row top.
        heights.insert(3, 80.0);
        let r = super::scroll_top_to_reveal_row(3, 400.0, 300.0, 40.0, &heights, true);
        assert_eq!(r, Some(200.0));
    }

    /// Row 0 above the viewport scrolls to exactly 0 (never negative).
    #[test]
    fn reveal_row_zero_clamps_at_top() {
        let heights: HashMap<usize, f64> = HashMap::new();
        let r = super::scroll_top_to_reveal_row(0, 250.0, 300.0, 40.0, &heights, false);
        assert_eq!(r, Some(0.0));
    }

    // ---- page_button_range -------------------------------------------------

    #[test]
    fn page_button_range_empty_when_total_zero() {
        assert_eq!(super::page_button_range(0, 0), vec![]);
    }

    #[test]
    fn page_button_range_lists_all_pages_when_small() {
        // total <= 7 → every page rendered, no ellipses.
        let buttons = super::page_button_range(2, 5);
        assert_eq!(buttons, vec![Some(0), Some(1), Some(2), Some(3), Some(4)]);
    }

    #[test]
    fn page_button_range_uses_ellipsis_in_middle_for_large_total() {
        // total > 7 with current in the middle → first + ellipsis + window + ellipsis + last.
        let buttons = super::page_button_range(10, 20);
        // Expect first page, ellipsis, neighbors of 10, ellipsis, last page.
        assert_eq!(buttons.first(), Some(&Some(0)));
        assert_eq!(buttons.last(), Some(&Some(19)));
        // The current page 10 must be present.
        assert!(buttons.contains(&Some(10)));
        // At least one ellipsis (None) on each side of the current page window.
        let none_count = buttons.iter().filter(|b| b.is_none()).count();
        assert!(
            none_count >= 2,
            "expected ellipses on both sides, got {none_count}"
        );
    }

    #[test]
    fn page_button_range_no_left_ellipsis_when_current_is_near_start() {
        // Current near start: page 1 of 20. Should NOT have a left ellipsis
        // (pages 0, 1, 2, 3 are all within range), but SHOULD have a right one.
        let buttons = super::page_button_range(1, 20);
        // First few buttons should be contiguous from 0.
        assert_eq!(buttons[0], Some(0));
        assert_eq!(buttons[1], Some(1));
        // Somewhere there's a right-side ellipsis.
        assert!(buttons.iter().any(Option::is_none));
        assert_eq!(*buttons.last().unwrap(), Some(19));
    }

    // ---- numeric_range_to_filter ------------------------------------------

    #[test]
    fn numeric_range_to_filter_both_at_bound_returns_none() {
        // Slider thumbs at the configured extents = "no filter active".
        assert!(super::numeric_range_to_filter(40_000.0, 200_000.0, 40_000.0, 200_000.0).is_none());
    }

    #[test]
    fn numeric_range_to_filter_min_only_inside_bounds() {
        // Min raised above bound_min, max still at bound_max → only `min` is set.
        let result =
            super::numeric_range_to_filter(60_000.0, 200_000.0, 40_000.0, 200_000.0).unwrap();
        match result {
            chorale_core::FilterValue::NumericRange { min, max } => {
                assert_eq!(min, Some(60_000.0));
                assert_eq!(max, None, "max at upper bound should be None");
            }
            _ => panic!("expected NumericRange variant"),
        }
    }

    #[test]
    fn numeric_range_to_filter_max_only_inside_bounds() {
        let result =
            super::numeric_range_to_filter(40_000.0, 150_000.0, 40_000.0, 200_000.0).unwrap();
        match result {
            chorale_core::FilterValue::NumericRange { min, max } => {
                assert_eq!(min, None);
                assert_eq!(max, Some(150_000.0));
            }
            _ => panic!("expected NumericRange variant"),
        }
    }

    #[test]
    fn numeric_range_to_filter_both_inside_bounds() {
        let result =
            super::numeric_range_to_filter(60_000.0, 150_000.0, 40_000.0, 200_000.0).unwrap();
        match result {
            chorale_core::FilterValue::NumericRange { min, max } => {
                assert_eq!(min, Some(60_000.0));
                assert_eq!(max, Some(150_000.0));
            }
            _ => panic!("expected NumericRange variant"),
        }
    }

    // ---- date_range_to_filter ---------------------------------------------

    #[test]
    fn date_range_to_filter_both_none_returns_none() {
        assert!(super::date_range_to_filter(None, None).is_none());
    }

    #[test]
    fn date_range_to_filter_min_only() {
        let d = chorale_core::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let result = super::date_range_to_filter(Some(d), None).unwrap();
        match result {
            chorale_core::FilterValue::DateRange { min, max } => {
                assert_eq!(min, Some(d));
                assert_eq!(max, None);
            }
            _ => panic!("expected DateRange variant"),
        }
    }

    #[test]
    fn date_range_to_filter_both_set() {
        let lo = chorale_core::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let hi = chorale_core::NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let result = super::date_range_to_filter(Some(lo), Some(hi)).unwrap();
        match result {
            chorale_core::FilterValue::DateRange { min, max } => {
                assert_eq!(min, Some(lo));
                assert_eq!(max, Some(hi));
            }
            _ => panic!("expected DateRange variant"),
        }
    }

    // ---- format_compact_number --------------------------------------------

    #[test]
    fn format_compact_number_renders_under_thousand_as_integer() {
        assert_eq!(super::format_compact_number(0.0), "0");
        assert_eq!(super::format_compact_number(42.0), "42");
        assert_eq!(super::format_compact_number(999.0), "999");
    }

    #[test]
    fn format_compact_number_renders_thousands_with_k_suffix() {
        assert_eq!(super::format_compact_number(1_000.0), "1k");
        assert_eq!(super::format_compact_number(40_000.0), "40k");
        assert_eq!(super::format_compact_number(200_000.0), "200k");
    }

    #[test]
    fn format_compact_number_renders_millions_with_one_decimal() {
        assert_eq!(super::format_compact_number(1_000_000.0), "1.0M");
        assert_eq!(super::format_compact_number(2_500_000.0), "2.5M");
    }

    // ---- format_thousands --------------------------------------------------

    #[test]
    fn format_thousands_handles_zero_and_small() {
        assert_eq!(super::format_thousands(0), "0");
        assert_eq!(super::format_thousands(42), "42");
        assert_eq!(super::format_thousands(999), "999");
    }

    #[test]
    fn format_thousands_inserts_commas_above_thousand() {
        assert_eq!(super::format_thousands(1_000), "1,000");
        assert_eq!(super::format_thousands(12_345), "12,345");
        assert_eq!(super::format_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn format_thousands_preserves_negative_sign() {
        assert_eq!(super::format_thousands(-1), "-1");
        assert_eq!(super::format_thousands(-1_234_567), "-1,234,567");
    }

    // ---- currency_symbol --------------------------------------------------

    #[test]
    fn currency_symbol_known_codes() {
        use chorale_core::CurrencyCode;
        assert_eq!(super::currency_symbol(&CurrencyCode::USD), "$");
        assert_eq!(super::currency_symbol(&CurrencyCode::EUR), "\u{20ac}");
        assert_eq!(super::currency_symbol(&CurrencyCode::GBP), "\u{00a3}");
    }

    #[test]
    fn currency_symbol_unknown_code_falls_back_to_empty_string() {
        use chorale_core::CurrencyCode;
        // CurrencyCode is constructible with arbitrary &'static str; unknown
        // codes get no symbol prefix so the formatter is forward-compatible.
        assert_eq!(super::currency_symbol(&CurrencyCode("XYZ")), "");
    }

    // ---- alignment_css -----------------------------------------------------

    #[test]
    fn alignment_css_maps_each_variant() {
        assert_eq!(super::alignment_css(Alignment::Left), "left");
        assert_eq!(super::alignment_css(Alignment::Center), "center");
        assert_eq!(super::alignment_css(Alignment::Right), "right");
    }

    // ---- col_width_style ---------------------------------------------------

    #[test]
    fn col_width_style_empty_when_neither_set() {
        assert_eq!(super::col_width_style(None, None), "");
    }

    #[test]
    fn col_width_style_uses_override_when_present() {
        // Override wins over initial_width.
        let s = super::col_width_style(Some(120.0), Some(200.0));
        assert!(s.contains("width: 120px"));
        assert!(s.contains("min-width: 120px"));
        assert!(s.contains("max-width: 120px"));
        assert!(
            !s.contains("200px"),
            "override should suppress initial_width"
        );
    }

    #[test]
    fn col_width_style_falls_back_to_initial_when_no_override() {
        let s = super::col_width_style(None, Some(200.0));
        assert!(s.contains("width: 200px"));
    }

    // ---- cell_text ---------------------------------------------------------

    #[test]
    fn cell_text_formats_integer_with_thousands() {
        let s = super::cell_text(&CellValue::Integer(1_234_567), &RenderKind::Number);
        assert_eq!(s, "1,234,567");
    }

    #[test]
    fn cell_text_formats_currency_with_symbol_and_decimals() {
        let s = super::cell_text(
            &CellValue::Integer(40_000),
            &RenderKind::Currency(chorale_core::CurrencyCode::USD),
        );
        assert_eq!(s, "$40,000.00");
    }

    #[test]
    fn cell_text_formats_date_iso() {
        let d = chorale_core::NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let s = super::cell_text(&CellValue::Date(d), &RenderKind::Date);
        assert_eq!(s, "2024-06-15");
    }

    #[test]
    fn cell_text_boolean_renders_check_or_cross() {
        let yes = super::cell_text(&CellValue::Boolean(true), &RenderKind::Boolean);
        let no = super::cell_text(&CellValue::Boolean(false), &RenderKind::Boolean);
        assert_eq!(yes, "\u{2713}");
        assert_eq!(no, "\u{2717}");
    }

    #[test]
    fn cell_text_text_passes_through() {
        let s = super::cell_text(&CellValue::Text("hello".into()), &RenderKind::Text);
        assert_eq!(s, "hello");
    }

    #[test]
    fn cell_text_empty_returns_blank() {
        let s = super::cell_text(&CellValue::Empty, &RenderKind::Text);
        assert_eq!(s, "");
    }

    #[test]
    fn cell_text_float_with_number_render_no_decimals() {
        let s = super::cell_text(&CellValue::Float(3.9), &RenderKind::Number);
        assert_eq!(s, "4"); // .0 format rounds
    }

    #[test]
    fn cell_text_float_currency_two_decimals() {
        let s = super::cell_text(
            &CellValue::Float(99.5),
            &RenderKind::Currency(chorale_core::CurrencyCode::EUR),
        );
        assert_eq!(s, "\u{20ac}99.50");
    }

    #[test]
    fn cell_text_date_formats_correctly() {
        let d = chorale_core::NaiveDate::from_ymd_opt(2024, 3, 15).unwrap();
        let s = super::cell_text(&CellValue::Date(d), &RenderKind::Date);
        assert_eq!(s, "2024-03-15");
    }

    #[test]
    fn cell_text_text_with_number_render_passes_through() {
        // Text cell regardless of render kind → text is returned unchanged.
        let s = super::cell_text(&CellValue::Text("abc".into()), &RenderKind::Number);
        assert_eq!(s, "abc");
    }

    #[test]
    fn currency_symbol_eur_and_gbp() {
        use chorale_core::CurrencyCode;
        assert_eq!(super::currency_symbol(&CurrencyCode::EUR), "\u{20ac}");
        assert_eq!(super::currency_symbol(&CurrencyCode::GBP), "\u{00a3}");
    }

    #[test]
    fn format_thousands_large_negative() {
        assert_eq!(super::format_thousands(-1_234_567), "-1,234,567");
    }

    #[test]
    fn format_thousands_single_digit() {
        assert_eq!(super::format_thousands(5), "5");
    }

    #[test]
    fn page_button_range_current_near_end_no_right_ellipsis() {
        // total=10, current=8 → no right ellipsis expected.
        let buttons = super::page_button_range(8, 10);
        let has_trailing_none = buttons.last() == Some(&None);
        assert!(
            !has_trailing_none,
            "should have no right ellipsis when near end"
        );
    }

    #[test]
    fn page_button_range_last_page_is_always_included() {
        let buttons = super::page_button_range(0, 20);
        let has_last_page = buttons.contains(&Some(19));
        assert!(has_last_page, "last page (index 19) should always appear");
    }

    #[test]
    fn page_button_range_current_page_is_always_included() {
        for current in [0, 5, 10, 15, 19] {
            let buttons = super::page_button_range(current, 20);
            assert!(
                buttons.contains(&Some(current)),
                "current page {current} should appear in button list"
            );
        }
    }

    #[test]
    fn numeric_range_to_filter_none_when_min_equals_config_min_and_max_equals_config_max() {
        // Slider at default (both at extremes) → no filter.
        let filter = super::numeric_range_to_filter(0.0, 100.0, 0.0, 100.0);
        assert!(filter.is_none());
    }

    #[test]
    fn date_range_to_filter_max_only() {
        use chorale_core::NaiveDate;
        let max = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let filter = super::date_range_to_filter(None, Some(max));
        assert!(filter.is_some());
        if let Some(chorale_core::FilterValue::DateRange { min, max: fmax }) = filter {
            assert!(min.is_none());
            assert!(fmax.is_some());
        }
    }

    #[test]
    fn col_width_style_with_only_initial_produces_width_css() {
        let s = super::col_width_style(None, Some(150.0));
        assert!(s.contains("150px"));
    }

    #[test]
    fn compute_window_slice_returns_all_when_short_list() {
        // Large viewport compared to row count → all rows returned.
        let s = make_state(0.0, 40.0, 10_000.0);
        let view = visible_view(&s);
        let (_win, slice) = super::compute_window_slice(&s, &view, false);
        assert_eq!(slice.len(), view.len());
    }

    // ---- badge_style -------------------------------------------------------

    #[test]
    fn badge_style_unknown_color_falls_back_to_default() {
        let s = super::badge_style("hotpink");
        assert!(
            s.contains("var(--chorale-badge-default-bg, #e5e7eb)"),
            "unknown color should use fallback bg"
        );
        assert!(
            s.contains("var(--chorale-badge-default-text, #1f2937)"),
            "unknown color should use fallback fg"
        );
    }

    // ---- additional visible_view correctness (adapter-level) ---------------

    #[test]
    fn visible_view_empty_dataset_produces_empty_view() {
        use chorale_core::TableState;
        let s: TableState<String> = TableState::new(vec![], vec![]);
        let view = visible_view(&s);
        assert!(view.is_empty());
    }

    // ---- multi-sort 3+ columns ---------------------------------------------

    #[test]
    fn multi_sort_append_grows_to_three_columns() {
        use chorale_core::{toggle_sort, SortAction, SortDirection};
        let rows: Vec<(RowId, R)> = (0..5)
            .map(|i| {
                (
                    RowId::new(),
                    R {
                        name: format!("r{i}"),
                        score: i,
                    },
                )
            })
            .collect();
        let columns = vec![
            ColumnDef::new(ColumnId("name"), "Name", |r: &R| {
                CellValue::Text(r.name.clone())
            })
            .sortable(),
            ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
                CellValue::Integer(r.score)
            })
            .sortable(),
            ColumnDef::new(ColumnId("extra"), "Extra", |r: &R| {
                CellValue::Integer(r.score * 2)
            })
            .sortable(),
        ];
        let s0 = TableState::new(rows, columns);
        // Plain click col A → sort = [A:Asc]
        let s1 = toggle_sort(&s0, ColumnId("name"), SortAction::Replace);
        assert_eq!(s1.sort.len(), 1);
        // Shift+click col B → sort = [A:Asc, B:Asc]
        let s2 = toggle_sort(&s1, ColumnId("score"), SortAction::Append);
        assert_eq!(s2.sort.len(), 2);
        // Shift+click col C → sort = [A:Asc, B:Asc, C:Asc]
        let s3 = toggle_sort(&s2, ColumnId("extra"), SortAction::Append);
        assert_eq!(s3.sort.len(), 3);
        assert_eq!(s3.sort[0].column, ColumnId("name"));
        assert_eq!(s3.sort[0].direction, SortDirection::Asc);
        assert_eq!(s3.sort[1].column, ColumnId("score"));
        assert_eq!(s3.sort[2].column, ColumnId("extra"));
    }

    // ---- range selection includes anchor and focus -------------------------

    #[test]
    fn range_selection_single_cell_covers_one_cell() {
        use chorale_core::start_range_selection;
        let rows: Vec<(RowId, R)> = (0..5)
            .map(|i| {
                (
                    RowId::new(),
                    R {
                        name: format!("r{i}"),
                        score: i,
                    },
                )
            })
            .collect();
        let columns = vec![
            ColumnDef::new(ColumnId("name"), "Name", |r: &R| {
                CellValue::Text(r.name.clone())
            }),
            ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
                CellValue::Integer(r.score)
            }),
        ];
        let s0 = TableState::new(rows, columns);
        let s1 = start_range_selection(&s0, 0, ColumnId("name"));
        assert_eq!(s1.range_selection.len(), 1);
        let col_defs: Vec<&ColumnDef<R>> = s1.columns.iter().collect();
        let nr = s1.range_selection[0].normalized(&col_defs);
        assert_eq!(nr.min_row, 0);
        assert_eq!(nr.max_row, 0);
        assert_eq!(nr.columns.len(), 1);
    }

    #[test]
    fn range_selection_3x2_covers_six_cells() {
        use chorale_core::{extend_range_to, start_range_selection};
        use std::collections::HashSet;
        let rows: Vec<(RowId, R)> = (0..5)
            .map(|i| {
                (
                    RowId::new(),
                    R {
                        name: format!("r{i}"),
                        score: i,
                    },
                )
            })
            .collect();
        let columns = vec![
            ColumnDef::new(ColumnId("name"), "Name", |r: &R| {
                CellValue::Text(r.name.clone())
            }),
            ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
                CellValue::Integer(r.score)
            }),
        ];
        let s0 = TableState::new(rows, columns);
        let s1 = start_range_selection(&s0, 0, ColumnId("name"));
        // Extend to row 2, col score → 3 rows × 2 cols = 6 cells
        let s2 = extend_range_to(&s1, 2, ColumnId("score"));
        let col_defs: Vec<&ColumnDef<R>> = s2.columns.iter().collect();
        let nr = s2.range_selection[0].normalized(&col_defs);
        assert_eq!(nr.min_row, 0);
        assert_eq!(nr.max_row, 2, "focus row 2 must be INCLUSIVE");
        assert_eq!(nr.columns.len(), 2);
        // Enumerate all cells via the same loop as the adapter render uses.
        let mut cells: HashSet<(usize, ColumnId)> = HashSet::new();
        for row in nr.min_row..=nr.max_row {
            for &col_id in &nr.columns {
                cells.insert((row, col_id));
            }
        }
        assert_eq!(
            cells.len(),
            6,
            "3 rows × 2 cols must produce 6 highlighted cells"
        );
        assert!(
            cells.contains(&(2, ColumnId("score"))),
            "focus cell (2, score) must be in range"
        );
    }

    // ── Row-aware renderer tests ─────────────────────────────────────────────

    #[derive(Clone, PartialEq)]
    struct RcrRow {
        name: String,
        email: String,
    }

    #[test]
    fn row_cell_renderer_receives_row() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let captured2 = Arc::clone(&captured);
        let renderer: super::RowCellRenderer<RcrRow> =
            Arc::new(move |row: &RcrRow, _val: &CellValue| {
                *captured2.lock().unwrap() = Some(row.email.clone());
                dioxus::prelude::VNode::empty()
            });
        let mut m: HashMap<ColumnId, super::RowCellRenderer<RcrRow>> = HashMap::new();
        m.insert(ColumnId("name"), renderer);
        let renderers = super::RowCellRenderers::new(m);
        // unwrap is allowed in tests via #[allow(clippy::unwrap_used)] above
        let r = renderers.get(ColumnId("name")).unwrap();
        let row = RcrRow {
            name: "Ada".into(),
            email: "ada@example.com".into(),
        };
        let _ = r(&row, &CellValue::Text("Ada".into()));
        let got = captured.lock().unwrap().clone();
        assert_eq!(got, Some("ada@example.com".to_string()));
    }

    #[test]
    fn precedence_row_aware_wins_over_value_only() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let row_flag = Arc::new(AtomicBool::new(false));
        let val_flag = Arc::new(AtomicBool::new(false));
        let rf = Arc::clone(&row_flag);
        let vf = Arc::clone(&val_flag);
        let row_r: super::RowCellRenderer<RcrRow> =
            Arc::new(move |_row: &RcrRow, _val: &CellValue| {
                rf.store(true, Ordering::SeqCst);
                dioxus::prelude::VNode::empty()
            });
        let vf2 = Arc::clone(&val_flag);
        let val_r: super::CellRenderer = Arc::new(move |_val: &CellValue| {
            vf2.store(true, Ordering::SeqCst);
            dioxus::prelude::VNode::empty()
        });
        let row = RcrRow {
            name: "Ada".into(),
            email: "ada@example.com".into(),
        };
        let _ = super::resolve_cell_content(
            &row,
            &CellValue::Text("x".into()),
            &RenderKind::Text,
            Some(&row_r),
            Some(&val_r),
        );
        assert!(
            row_flag.load(Ordering::SeqCst),
            "row renderer must be called"
        );
        assert!(
            !vf.load(Ordering::SeqCst),
            "value renderer must NOT be called"
        );
    }

    #[test]
    fn precedence_value_only_wins_over_render_kind() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let val_flag = Arc::new(AtomicBool::new(false));
        let vf = Arc::clone(&val_flag);
        let val_r: super::CellRenderer = Arc::new(move |_val: &CellValue| {
            vf.store(true, Ordering::SeqCst);
            dioxus::prelude::VNode::empty()
        });
        let row = RcrRow {
            name: "Ada".into(),
            email: "ada@example.com".into(),
        };
        let _ = super::resolve_cell_content(
            &row,
            &CellValue::Text("x".into()),
            &RenderKind::Text,
            None,
            Some(&val_r),
        );
        assert!(
            val_flag.load(Ordering::SeqCst),
            "value renderer must be called when no row renderer"
        );
    }

    #[test]
    fn precedence_render_kind_fallback() {
        let row = RcrRow {
            name: "Ada".into(),
            email: "ada@example.com".into(),
        };
        // Should not panic; just falls through to cell_element
        let _el = super::resolve_cell_content::<RcrRow>(
            &row,
            &CellValue::Text("x".into()),
            &RenderKind::Text,
            None,
            None,
        );
    }

    #[test]
    fn row_cell_renderers_default_is_empty() {
        assert!(super::RowCellRenderers::<RcrRow>::default()
            .get(ColumnId("name"))
            .is_none());
    }

    #[test]
    fn row_cell_renderers_partial_eq_is_pointer_identity() {
        let a = super::RowCellRenderers::<RcrRow>::new(HashMap::new());
        let b = a.clone();
        assert!(a == b, "clone must eq original (same Arc)");
        let c = super::RowCellRenderers::<RcrRow>::new(HashMap::new());
        assert!(a != c, "independently constructed maps must not eq");
    }

    #[test]
    fn should_fire_row_click_only_on_unmodified_click() {
        assert!(super::should_fire_row_click(false, false));
        assert!(!super::should_fire_row_click(true, false));
        assert!(!super::should_fire_row_click(false, true));
        assert!(!super::should_fire_row_click(true, true));
    }
}
