//! Dioxus components for Chorale tables.

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
    ActiveCell, AggregatorKind, Alignment, BadgeVariantMap, CellValue, ClipboardCopyEvent,
    ClipboardPasteEvent, ColumnDef, ColumnId, CommittedEdit, CurrencyCode, EditorKind, FilterKind,
    FilterValue, GroupKey, GroupedPaginationMode, GroupedRow, Labels, NavDirection, PaginationMode,
    RenderKind, RenderRow, RowId, SortAction, SortDirection, SortState, TableState, Theme,
    VirtualWindow, THEME_ROOT_CLASS,
};
use dioxus::prelude::*;

use crate::hooks::UseTableHandle;

/// Type-erased cell renderer: maps a [`CellValue`] to a rendered [`Element`].
///
/// # The `Send + Sync` bound and interactive cells
///
/// The closure is `Send + Sync` because the renderer map is stored on the table
/// state and threaded across the component tree. Dioxus signals use
/// `UnsyncStorage` (they are `!Sync`), so **a renderer closure cannot CAPTURE a
/// signal** — the natural "click this cell to open a modal / mutate state"
/// pattern (capturing a `Signal` in the cell's `onclick`) fails to compile with
/// a confusing `Sync` error.
///
/// The working pattern: have the renderer return a small `#[component]` that
/// reads the signal from **context** ([`use_context`]) rather than capturing
/// it. The closure then captures only plain data and stays `Send + Sync`, while
/// the component does the reactive work on click:
///
/// ```ignore
/// // Host provides the signal once, above the table:
/// use_context_provider(|| Signal::new(None::<MyRow>));
///
/// // Renderer captures only plain data; the inner component reads context:
/// let r: CellRenderer = Arc::new(|val: &CellValue| {
///     let label = val.to_string();
///     rsx! { OpenDetailCell { label } } // component calls use_context::<Signal<_>>()
/// });
/// ```
///
/// This is also the cleaner Dioxus idiom — long-lived `Arc` closures that
/// capture signals are discouraged. For row-aware needs use [`RowCellRenderer`].
///
/// Keyboard: a control the renderer puts in a cell (a button, input or
/// select) is host content. Stop `keydown` propagation in it for the keys it
/// handles, or the grid's key handler acts on them too (moves the cursor,
/// starts an edit, prevents the default).
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
/// register via [`RowCellRenderers::new`]. Controls it renders follow the
/// keyboard rule on [`CellRenderer`]: stop `keydown` propagation for the keys
/// they handle.
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

/// Consumer predicate mapping a row to an optional CSS class, for data-driven row styling
/// (e.g. tint rows that match a condition). The predicate is consumer code (data -> style
/// intent); the adapter applies the returned class to the rendered row. Theming stays in
/// CSS — style the class via your own stylesheet or `--chorale-*` variables.
pub type RowClassFn<TRow> = Arc<dyn Fn(&TRow) -> Option<String> + Send + Sync + 'static>;

/// Per-row conditional styling hook (a `Table` prop). Default: no extra class. The returned
/// class is appended to each data row's `<tr>`, composing with selection (the selection
/// background still applies), grouping, and virtualization (only visible rows are styled).
/// Compared by pointer identity for prop diffing.
pub struct RowClass<TRow>(Option<RowClassFn<TRow>>);

impl<TRow> RowClass<TRow> {
    /// Build from a predicate `Fn(&TRow) -> Option<String>` (None = no class for that row).
    pub fn new(f: impl Fn(&TRow) -> Option<String> + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(f)))
    }

    /// The class for a given row, if the predicate returned one.
    fn class_for(&self, row: &TRow) -> Option<String> {
        self.0.as_ref().and_then(|f| f(row))
    }
}

// Manual impls (see RowCellRenderers): avoid spurious TRow bounds.
impl<TRow> Default for RowClass<TRow> {
    fn default() -> Self {
        Self(None)
    }
}

impl<TRow> Clone for RowClass<TRow> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<TRow> PartialEq for RowClass<TRow> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
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
    /// Per-row conditional styling: a predicate mapping each row to an optional CSS class,
    /// appended to that row's `<tr>`. Build with `RowClass::new(|row| ...)`. Composes with
    /// selection (selection background still applies), grouping, and virtualization. Default:
    /// no extra class.
    #[props(default)]
    row_class: RowClass<TRow>,
    /// Show a column visibility toolbar above the table. Each column gets
    /// a toggle checkbox.
    #[props(default = false)]
    column_toolbar: bool,
    /// Show an expand-all / collapse-all toggle in the first column header
    /// when the table is grouped. Clicking it expands every group (all
    /// depths); clicking again collapses them all. No-op when the table is
    /// not grouped. Off by default.
    #[props(default = false)]
    group_expand_toggle: bool,
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
    ///
    /// Keyboard scroll-follow does not run in this mode: the host's scroller
    /// owns scrolling.
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
    /// Optional host renderer for the content of every group-header row.
    ///
    /// When `Some`, chorale keeps the header `<tr>` (its key, class,
    /// selection checkbox cell and depth indentation) and replaces the label,
    /// row count and aggregates with the returned `Element`, wrapped in
    /// `span.chorale-group-header-content`. The collapse chevron becomes a
    /// real `<button class="chorale-group-toggle" aria-expanded>` (activated
    /// with Enter or Space). The row-level click toggle
    /// is NOT attached: a host that wants its own element to collapse the
    /// group calls `handle.toggle_group(ctx.key.clone())`. The wrapper span
    /// stops `keydown` propagation, so keys typed in host `<select>` or
    /// `<input>` elements never reach the grid's key handler.
    ///
    /// In `GroupedPaginationMode::Virtualized` the header row is given a
    /// fixed `height` of `row_height` px and the content span clips, so host
    /// content must fit one row height. A `Callback` may capture host
    /// signals and callbacks. Build it with `use_callback` so its identity is
    /// stable across renders. Focus outlines inside host content may clip in
    /// virtualized mode. The header controls are focusable, reached by click,
    /// by Tab while the keyboard cursor is on the header row, or Shift+Tab
    /// from below, then Tab moves among the header controls (selection
    /// checkbox, chevron, host controls); Tab from a data cell does not enter
    /// the header, and each header adds its own tab stops. After a
    /// click on a button inside the host content, focus stays there and every
    /// key goes to it until a data cell is clicked. In this mode the host
    /// replaces the column aggregates in the header too, so the host renders
    /// its own. The class names `chorale-group-toggle` and
    /// `chorale-group-header-content` are a styling contract and will not
    /// change in a patch release. Default `None` renders what 0.2.3 did,
    /// plus the treegrid attributes every grouped table now has.
    ///
    /// Keyboard (both `Some` and `None`): every group header row is an arrow
    /// key stop (ARIA treegrid). With the cursor on a header row, Left
    /// collapses the group, Right expands it, Enter and Space toggle it and
    /// Tab moves focus to the header's first control (with none, and for
    /// Shift+Tab, the browser moves focus to the next or previous focusable
    /// element, which can be another control in the table). Arrow keys on the
    /// group checkbox or the chevron put the cursor on that header and return
    /// focus to the grid; Space, Enter and Tab stay with the control. The
    /// header row under the cursor carries `data-chorale-active-row="true"`
    /// and the shipped stylesheet outlines it with
    /// `--chorale-active-cell-outline` (restyle with a more specific
    /// selector). The grouped `<table>` has `role="treegrid"` and
    /// `aria-rowcount`; rows carry `aria-rowindex` and an `id`, header rows
    /// also `aria-level`, `aria-expanded` and `data-chorale-group-index`,
    /// grouped data rows `aria-level`. The keyboard container is a
    /// `role="group"` whose `aria-activedescendant` names the active row or
    /// cell. See `docs/keyboard-navigation.md`.
    #[props(default)]
    group_header_renderer: Option<Callback<GroupHeaderContext, Element>>,
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
    ///
    /// In a grouped table `ActiveCell::row_idx` is an index into
    /// `visible_grouped_view` (header rows included); resolve the row through
    /// that list, not `visible_view`.
    on_tab_to_editable: Option<EventHandler<ActiveCell>>,
    /// Fired after Ctrl+C successfully writes the selected range to the system clipboard.
    ///
    /// In a grouped table the range's rows are indexes into
    /// `visible_grouped_view` (header rows included), and the copied text holds
    /// the data rows inside the range only.
    on_copy: Option<EventHandler<ClipboardCopyEvent>>,
    /// Fired after Ctrl+V reads from the system clipboard and adjusts the active range.
    /// The host should apply the per-cell writes from `evt.tsv` via its persistence layer.
    ///
    /// In a grouped table the range's rows are indexes into
    /// `visible_grouped_view`: the pasted lines go to consecutive data rows
    /// from the range's first row, skipping header rows (so the range can
    /// span a header the host must skip). A paste starting on a header row
    /// does nothing and fires no event.
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

    // Keep the keyboard-navigable detail-expander column (#17) in sync with
    // whether a detail renderer is configured. `use_reactive!` re-runs the
    // effect when `has_detail` changes (the harness flips `detail_renderer` as
    // a prop on the same component), not just on table-state changes;
    // `set_detail_column_enabled` no-ops when the flag already matches.
    use_effect(use_reactive!(|has_detail| {
        handle.set_detail_column_enabled(has_detail);
    }));

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
            // set_grouped_pagination keeps page 0 at 0, so without the mode
            // in the key a runtime switch to Virtualized kept the first
            // page's grouped view (issue #51).
            s.grouped_pagination,
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
    // The last scrollTop this table wrote itself (page effect, VIRT-2 anchor,
    // keyboard reveal). Keyboard scroll-follow treats any other value it
    // reads as a user scroll; a value written here is not (issue #51 review
    // round 2, variable row heights).
    let programmatic_scroll: ProgrammaticScroll =
        use_hook(|| std::rc::Rc::new(std::cell::Cell::new(None)));
    let programmatic_page = programmatic_scroll.clone();
    let page_memo = use_memo(move || sig.read().page);
    let scroll_id_for_effect = scroll_id.clone();
    let prev_page = use_hook(|| std::rc::Rc::new(std::cell::Cell::new(None::<usize>)));
    use_effect(move || {
        let page = *page_memo.read();
        // Peek, not read: only a page change re-runs the effect. The saved offset
        // applies on mount only; a later page change always lands at the top, even
        // if a late scroll event or a host write through `handle.signal()` left a
        // stale `scroll_top` in state.
        let saved = sig.peek().scroll_top;
        let target = page_scroll_target(prev_page.get(), page, saved);
        prev_page.set(Some(page));
        if let Some(top) = target {
            programmatic_page.set(Some(top));
            dioxus::document::eval(&sync_scroll_js(&scroll_id_for_effect, top));
        }
    });

    // Grouped cursor follows its row (issue #51): when the grouped view changes
    // under an unmoved cursor (a group collapsed or expanded by mouse or by the
    // host), move the cursor to the same row, or to its enclosing header when
    // that header was collapsed. A key press or click that moves the cursor
    // just updates the anchor.
    {
        let active_memo = use_memo(move || sig.read().active_cell);
        let anchor = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(None::<CursorAnchor>)));
        use_effect(move || {
            let view = grouped_view.read();
            let active = *active_memo.read();
            let grouped = !sig.peek().grouping.is_empty();
            let Some(ac) = active.filter(|_| grouped) else {
                *anchor.borrow_mut() = None;
                return;
            };
            let previous = anchor.borrow().clone();
            let here = cursor_anchor_at(&view, ac.row_idx);
            if let Some(prev) = previous.filter(|p| p.row_idx == ac.row_idx) {
                let same_row = here.as_ref().is_some_and(|h| h.row == prev.row);
                if !same_row {
                    if let Some(to) = relocate_cursor(&view, &prev).filter(|to| *to != ac.row_idx) {
                        drop(view);
                        *anchor.borrow_mut() = None;
                        let mut sig_w = handle.signal();
                        let mut s = sig_w.peek().clone();
                        s.active_cell = Some(ActiveCell::new(to, ac.column_id));
                        s.range_selection =
                            vec![chorale_core::RangeSelection::single(to, ac.column_id)];
                        sig_w.set(s);
                        return;
                    }
                }
            }
            *anchor.borrow_mut() = here;
        });
    }

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
        let programmatic_effect = programmatic_scroll.clone();
        use_effect(move || {
            // Register the meas_trigger subscription (see comment above).
            let _ = meas_trigger.read();
            let cid = scroll_id_meas.clone();
            let mut sig2 = handle.signal();
            let programmatic_meas = programmatic_effect.clone();
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
                            programmatic_meas.set(Some(new_scroll));
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
    // Grouped-mode navigable columns: the same set the core transitions use
    // (the detail-expander column leads when a detail renderer is set).
    let grouped_nav_cols: Vec<ColumnId> = if has_detail {
        std::iter::once(chorale_core::DETAIL_EXPANDER_COLUMN)
            .chain(visible_col_ids_for_kb.iter().copied())
            .collect()
    } else {
        visible_col_ids_for_kb.clone()
    };
    // A header row holds focusable controls when it renders the chevron (host
    // renderer) or the group checkbox (selection on); Tab from a header moves
    // focus into them.
    let header_has_controls = group_header_renderer.is_some() || selection_enabled;
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
    // Expand-all / collapse-all header toggle (opt-in via `group_expand_toggle`).
    // Only meaningful when grouped. `all_groups_expanded` drives both the icon
    // and the action: when nothing is collapsed, the next click collapses all;
    // otherwise it expands all.
    let show_group_toggle = group_expand_toggle && is_grouped;
    // Rows of the grouped view the body renders: a scroll-driven window in
    // Virtualized grouped mode (with buffer rows), every row otherwise.
    let grouped_len = grouped_view_read.len();
    let (grouped_start, grouped_end) = if is_virtualized_grouped && grouped_len > 0 {
        let buf = state.buffer_rows;
        let raw_start = (state.scroll_top / state.row_height).floor() as usize;
        let visible = (state.viewport_height / state.row_height).ceil() as usize;
        let start = raw_start.saturating_sub(buf);
        let end = (raw_start + visible + buf).min(grouped_len);
        (start.min(end), end)
    } else {
        (0, grouped_len)
    };
    // Rows in the <thead>: the header row, plus the filter row when shown.
    let thead_rows = 1 + usize::from(filter_enabled);
    // aria-activedescendant target: the active header row, or the active data
    // cell, when it is rendered (virtualization can unmount it).
    let active_descendant: Option<String> = if is_grouped {
        active_cell
            .filter(|ac| ac.row_idx >= grouped_start && ac.row_idx < grouped_end)
            .and_then(|ac| match grouped_view_read.get(ac.row_idx) {
                Some(GroupedRow::Header { .. }) => Some(grouped_row_dom_id(&kb_id, ac.row_idx)),
                Some(GroupedRow::Data(..)) if grouped_nav_cols.contains(&ac.column_id) => {
                    Some(grouped_cell_dom_id(&kb_id, ac.row_idx, ac.column_id))
                }
                _ => None,
            })
    } else {
        None
    };
    let all_groups_expanded = state.collapsed_groups.is_empty();
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
            // Grouped tables (issue #51): focus stays on this container, which
            // names the cursor's header row or data cell for assistive
            // technology. `group` is a role that may own an active descendant.
            role: is_grouped.then_some("group"),
            "aria-activedescendant": active_descendant,
            style: "border: 1px solid var(--chorale-border, #ddd); border-radius: 4px; overflow: hidden; \
                    user-select: none; outline: none; \
                    background: var(--chorale-surface, #fff); color: var(--chorale-text, #333);",
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
                            // Grouped: fill data rows only, in the grouped index space.
                            let writes = if state.grouping.is_empty() {
                                fill_handle_targets(&*state, source_range, target_row, target_col)
                            } else {
                                grouped_fill_targets(&state, &grouped_view.peek(), source_range, target_row, target_col)
                            };
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
            // When this container gains focus with nothing selected — e.g. a
            // single Tab descending into a nested sub-table — light up the first
            // cell so keyboard nav is immediately visible (issue #17). `onfocus`
            // does not bubble, so focusing a descendant (editor input, filter
            // box) never triggers a spurious selection.
            onfocus: move |_| handle.ensure_active_cell(),
            onclick: {
                let kb_id = kb_id.clone();
                move |e: MouseEvent| {
                    // Stop the click from bubbling to an ANCESTOR table's keyboard
                    // container. A nested child table lives inside the parent's DOM,
                    // so without this the parent's focus-steal below would yank focus
                    // back to the parent the instant you click a child cell, and every
                    // arrow key would then drive the parent (issue #17 part 2).
                    e.stop_propagation();
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
                let kb_id_kb = kb_id.clone();
                let programmatic_kb = programmatic_scroll.clone();
                move |e: KeyboardEvent| {
                // A nested child table's keydown bubbles to this (parent)
                // container too; without stopping it, both tables would
                // navigate at once. Stop propagation so only the focused table
                // handles the key (issue #17 part 2).
                e.stop_propagation();
                let shift = e.modifiers().contains(Modifiers::SHIFT);
                let ctrl = e.modifiers().contains(Modifiers::CONTROL)
                    || e.modifiers().contains(Modifiers::META);
                // Grouped tables navigate in the grouped index space, with
                // header rows as treegrid stops (issue #51).
                if is_grouped {
                    let outcome = grouped_keydown(
                        handle,
                        &grouped_view.peek(),
                        &grouped_nav_cols,
                        &e.key(),
                        shift,
                        ctrl,
                        &scroll_id_kb,
                        header_has_controls,
                    );
                    match outcome {
                        GroupedKeyOutcome::NotHandled => {}
                        GroupedKeyOutcome::BrowserDefault => return,
                        GroupedKeyOutcome::Handled => {
                            e.prevent_default();
                            return;
                        }
                        GroupedKeyOutcome::Moved => {
                            e.prevent_default();
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
                            );
                            return;
                        }
                        GroupedKeyOutcome::TabMoved(ac) => {
                            e.prevent_default();
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
                            );
                            if let Some(cb) = on_tab_to_editable {
                                let editable = handle.signal().peek().columns.iter()
                                    .any(|c| c.id == ac.column_id && c.editor.is_some());
                                if editable {
                                    cb.call(ac);
                                }
                            }
                            return;
                        }
                    }
                }
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
                            handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
                        );
                    } else {
                        let new_s = move_active_cell(&*sig_w.peek(), dir);
                        sig_w.set(new_s);
                        ensure_active_row_visible(
                            handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
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
                                handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
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
                                handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
                            );
                        }
                        Key::PageUp => {
                            e.prevent_default();
                            let mut sig_w = handle.signal();
                            let page_sz = sig_w.peek().page_size;
                            let new_s = move_active_cell_page(&*sig_w.peek(), NavDirection::Up, page_sz);
                            sig_w.set(new_s);
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
                            );
                        }
                        Key::PageDown => {
                            e.prevent_default();
                            let mut sig_w = handle.signal();
                            let page_sz = sig_w.peek().page_size;
                            let new_s = move_active_cell_page(&*sig_w.peek(), NavDirection::Down, page_sz);
                            sig_w.set(new_s);
                            ensure_active_row_visible(
                                handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
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
                            // If this table is nested in another chorale table,
                            // Escape returns focus to the parent (issue #17 part 2).
                            // No-op for a top-level table.
                            focus_parent_container(&kb_id_kb);
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
                                // Enter while the active cell is on the
                                // detail-expander (chevron) column toggles that
                                // row's panel (keyboard expand/collapse, issue
                                // #17). F2 always edits; Enter on a data cell
                                // still edits.
                                let is_enter = matches!(e.key(), Key::Enter);
                                let on_expander = s.active_cell.is_some_and(|ac| {
                                    ac.column_id == chorale_core::DETAIL_EXPANDER_COLUMN
                                });
                                if is_enter && on_expander {
                                    e.prevent_default();
                                    let new_s = chorale_core::toggle_active_row_expansion(&s);
                                    drop(s);
                                    sig_w.set(new_s);
                                } else if let Some(ac) = s.active_cell {
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
                            // Grouped: span every grouped row (core counts
                            // the ungrouped rows of one page).
                            let new_s = if is_grouped {
                                grouped_select_all(&sig_w.peek(), grouped_view.peek().len(), &grouped_nav_cols)
                            } else {
                                select_all_range(&*sig_w.peek())
                            };
                            sig_w.set(new_s);
                        }
                        Key::Character(ref ch) if ch.to_lowercase() == "c" && ctrl => {
                            e.prevent_default();
                            let sig_r = handle.signal();
                            let s = sig_r.peek();
                            // Grouped: copy the data rows under the range
                            // (header rows skipped), resolved in the grouped view.
                            let copied = if is_grouped {
                                grouped_clipboard_tsv(&s, &grouped_view.peek())
                            } else {
                                to_clipboard_tsv(&*s)
                            };
                            if let Ok(tsv) = copied {
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
                                            // Grouped: paste onto consecutive data
                                            // rows; a header is not a target.
                                            if is_grouped {
                                                grouped_paste_state(&s, &grouped_view.peek(), &tsv)
                                                    .ok_or(chorale_core::ClipboardError::NoRangeSelected)
                                            } else {
                                                paste_tsv_into_range(&*s, &tsv)
                                            }
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
                            // Tab on the chevron of an EXPANDED row descends into
                            // that row's sub-table (issue #17 part 2). The active
                            // cell otherwise tab-navigates cells as before.
                            let descend_row = if shift {
                                None
                            } else {
                                let sig = handle.signal();
                                let s = sig.peek();
                                s.active_cell.and_then(|ac| {
                                    if ac.column_id != chorale_core::DETAIL_EXPANDER_COLUMN {
                                        return None;
                                    }
                                    let view = visible_view(&s);
                                    match view.get(ac.row_idx) {
                                        Some(RenderRow::Data { id, .. })
                                            if s.expanded_rows.contains(id) =>
                                        {
                                            Some(ac.row_idx)
                                        }
                                        _ => None,
                                    }
                                })
                            };
                            if let Some(row_idx) = descend_row {
                                e.prevent_default();
                                focus_child_subtable(&scroll_id_kb, row_idx);
                            } else {
                                let tab_dir =
                                    if shift { NavDirection::Left } else { NavDirection::Right };
                                let mut sig_w = handle.signal();
                                let old_ac = sig_w.peek().active_cell;
                                let new_s = move_active_cell(&*sig_w.peek(), tab_dir);
                                let new_ac = new_s.active_cell;
                                // Last or first column: let the browser move
                                // focus to the next (previous) element.
                                if old_ac.is_some() && new_ac == old_ac {
                                    return;
                                }
                                e.prevent_default();
                                sig_w.set(new_s);
                                ensure_active_row_visible(
                                    handle, &scroll_id_kb, variable_row_height, inline, &programmatic_kb,
                                );
                                if let (Some(ac), Some(cb)) = (new_ac, on_tab_to_editable) {
                                    let s = sig_w.peek();
                                    if s.columns.iter().any(|c| c.id == ac.column_id && c.editor.is_some()) {
                                        cb.call(ac);
                                    }
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
                    // Structural only: the slot content supplies its own
                    // visual treatment (kept consistent with the leptos adapter).
                    class: "chorale-selection-toolbar",
                    onkeydown: move |e: KeyboardEvent| keep_native_control_keys(&e),
                    style: "width: 100%; box-sizing: border-box;",
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
                    // Grouped rows form a tree: expose the ARIA treegrid
                    // pattern (rows carry aria-level and, for headers,
                    // aria-expanded). Ungrouped tables keep no role.
                    role: is_grouped.then_some("treegrid"),
                    // Full treegrid row count: the <thead> rows plus every row
                    // of the grouped view, rendered or not (virtualization).
                    "aria-rowcount": is_grouped.then(|| (thead_rows + grouped_view_read.len()).to_string()),

                    thead {
                        onkeydown: move |e: KeyboardEvent| keep_native_control_keys(&e),
                        tr {
                            style: "background: var(--chorale-header-bg, #f8f9fa);",
                            "aria-rowindex": is_grouped.then_some("1"),
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
                                // The detail-expander header carries the same
                                // border-bottom as every other header cell so the
                                // line under the header is continuous across all
                                // columns. Omitting it left a visible gap in the
                                // header underline above the chevron column.
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
                            for (col_i, col) in visible_cols.iter().enumerate() {
                                {header_th(col, widths.get(&col.id).copied(), handle, sort_enabled, current_sort, resize_enabled, drag_state, column_reorder_enabled, drag_col_id, drag_over_col, on_column_order_change, sticky_header_css.get(&col.id).map_or("", String::as_str), sticky_header, show_group_toggle && col_i == 0, all_groups_expanded)}
                            }
                        }
                        if filter_enabled {
                            tr {
                                style: "background: var(--chorale-surface, #fff);",
                                "aria-rowindex": is_grouped.then_some("2"),
                                if selection_enabled {
                                    th {
                                        style: "padding: 0.25rem; border-bottom: 1px solid var(--chorale-divider, #eee); \
                                                background: var(--chorale-surface, #fff); width: 2.5rem;",
                                    }
                                }
                                if has_detail {
                                    th { style: "width: 24px; padding: 0; background: var(--chorale-surface, #fff);" }
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
                                let (start_idx, end_idx) = (grouped_start, grouped_end);
                                let top_pad_px =
                                    (start_idx as f64 * state.row_height).max(0.0);
                                let bottom_pad_px =
                                    ((grouped_len.saturating_sub(end_idx)) as f64
                                        * state.row_height)
                                        .max(0.0);
                                rsx! {
                                    if top_pad_px > 0.0 {
                                        tr {
                                            // Spacers are layout only, not treegrid rows.
                                            "aria-hidden": "true",
                                            td {
                                                colspan: "{effective_col_count}",
                                                style: "padding: 0; height: {top_pad_px}px;",
                                            }
                                        }
                                    }
                                    for (offset, grouped_row) in grouped_view_read[start_idx..end_idx].iter().cloned().enumerate() {
                                        {render_grouped_row(grouped_row, start_idx + offset, effective_col_count, selection_enabled, has_detail, handle, &group_header_class, &visible_cols, row_height, &widths, variable_row_height, &cell_renderers, &row_cell_renderers, &row_class, editing_target, editing_text, edit_error, &validate_edit, on_commit_edit, &sticky_body_css, &selection_set, active_cell, &range_cells, fill_focus_cell, fill_drag_active, fill_hover, on_row_click, &kb_id, group_header_renderer.as_ref(), is_virtualized_grouped, state.grouping.len() + 1, thead_rows)}
                                    }
                                    if bottom_pad_px > 0.0 {
                                        tr {
                                            "aria-hidden": "true",
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
                                            data_tr(row, row_id, win.start_index + i, variable_row_height, &visible_cols, row_height, &widths, selection_enabled, selection_set.contains(&row_id), handle, &cell_renderers, &row_cell_renderers, &row_class, editing_col, editing_text, edit_error, &validate_edit, on_commit_edit, &sticky_body_css, active_cell, &range_cells, fill_focus_cell, fill_drag_active, fill_hover, has_detail, is_expanded, on_row_click, &kb_id, None)
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
                    onkeydown: move |e: KeyboardEvent| keep_native_control_keys(&e),
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

/// Geometry of one body row inside the table's scroll container, in CSS px.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RowGeometry {
    /// Row top in scroll-content coordinates: the distance from the top of
    /// the scrolled content (the `<thead>` included) to the row's top edge.
    top: f64,
    /// Row height.
    height: f64,
    /// The container's current `scrollTop`.
    scroll_top: f64,
    /// The container's `clientHeight` (visible height, scrollbar excluded).
    client_height: f64,
    /// Height of the band at the container top that a sticky `<thead>`
    /// covers (0 without a sticky header).
    cover: f64,
}

/// Sub-pixel slack for [`reveal_scroll_top`]: browsers round `scrollTop` and
/// report fractional rects, so a row overlapping an edge by less than this is
/// treated as visible instead of triggering another scroll.
const REVEAL_SLACK_PX: f64 = 0.5;

/// The `scrollTop` that shows the row of `g` fully, or `None` when it already
/// is. The visible band is `[scroll_top + cover, scroll_top + client_height]`.
///
/// - Row top above the band: put the row top just below the sticky header.
/// - Row bottom below the band: put the row bottom at the container bottom,
///   unless the row is taller than the band, then show its top.
/// - Otherwise `None`.
fn reveal_scroll_top(g: RowGeometry) -> Option<f64> {
    let band_top = g.scroll_top + g.cover;
    let band_bottom = g.scroll_top + g.client_height;
    let align_top = (g.top - g.cover).max(0.0);
    if g.top < band_top - REVEAL_SLACK_PX {
        Some(align_top)
    } else if g.top + g.height > band_bottom + REVEAL_SLACK_PX {
        if g.height > g.client_height - g.cover {
            // Taller than the band: showing its top is the best fit. Already
            // there means no scroll.
            ((align_top - g.scroll_top).abs() > REVEAL_SLACK_PX).then_some(align_top)
        } else {
            Some((g.top + g.height - g.client_height).max(0.0))
        }
    } else {
        None
    }
}

/// Estimated `(top, height)` of body row `row_idx`, measured from the top of
/// the `<tbody>`, for a row that is not rendered: `row_idx * row_height` in
/// fixed mode, or the sum of the measured heights of rows `0..row_idx` in
/// variable mode, with `row_height` for rows not measured yet.
fn estimated_row_offset<S: std::hash::BuildHasher>(
    row_idx: usize,
    row_height: f64,
    row_heights: &HashMap<usize, f64, S>,
    variable_row_height: bool,
) -> (f64, f64) {
    if variable_row_height {
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
    }
}

/// Compute the `scroll_top` required to bring the row at `row_idx` fully
/// into the virtualized viewport, or `None` when the row is already fully
/// visible (no scroll needed).
///
/// Operates in the same coordinate space as the virtualization window math
/// (`visible_window` / `visible_window_variable`), see
/// [`estimated_row_offset`]. `header_h` is the sticky `<thead>` height: it
/// sits above row 0 in the scrolled content and covers the top `header_h`
/// band of the container. The decision rules are [`reveal_scroll_top`]'s.
#[must_use]
fn scroll_top_to_reveal_row<S: std::hash::BuildHasher>(
    row_idx: usize,
    scroll_top: f64,
    viewport_height: f64,
    row_height: f64,
    row_heights: &HashMap<usize, f64, S>,
    variable_row_height: bool,
    header_h: f64,
) -> Option<f64> {
    let (row_top, row_h) =
        estimated_row_offset(row_idx, row_height, row_heights, variable_row_height);
    reveal_scroll_top(RowGeometry {
        top: row_top + header_h,
        height: row_h,
        scroll_top,
        client_height: viewport_height,
        cover: header_h,
    })
}

/// What [`measure_row_js`] reports.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RowMeasurement {
    /// The row is rendered; its geometry.
    Found(RowGeometry),
    /// The row is not rendered (outside the virtualized window). The
    /// container's `scrollTop`, `clientHeight`, sticky-header cover and full
    /// `<thead>` height, for an estimate.
    Missing {
        scroll_top: f64,
        client_height: f64,
        cover: f64,
        thead: f64,
    },
}

impl RowMeasurement {
    /// Decode the `[found, top, height, scrollTop, clientHeight, cover, thead,
    /// outer]` array [`measure_row_js`] sends (`outer` may be missing); `None`
    /// for anything else (no container). `outer` is read with
    /// [`measured_outer_offset`].
    fn from_js(v: &[f64]) -> Option<Self> {
        let (&[found, top, height, scroll_top, client_height, cover, thead], 0..=1) = (
            v.get(..7).and_then(|a| <&[f64; 7]>::try_from(a).ok())?,
            v.len() - 7,
        ) else {
            return None;
        };
        if ![top, height, scroll_top, client_height, cover, thead]
            .iter()
            .all(|x| x.is_finite())
        {
            return None;
        }
        Some(if found > 0.5 {
            Self::Found(RowGeometry {
                top,
                height,
                scroll_top,
                client_height,
                cover,
            })
        } else {
            Self::Missing {
                scroll_top,
                client_height,
                cover,
                thead,
            }
        })
    }
}

/// The outer scroll offset (`window.scrollY` plus every ancestor's
/// `scrollTop`) in a [`measure_row_js`] reply, when present and finite.
fn measured_outer_offset(v: &[f64]) -> Option<f64> {
    v.get(7).copied().filter(|x| x.is_finite())
}

/// JS that measures body row `row_idx` of the table whose scroll container
/// is `scroll_id` and sends what [`RowMeasurement::from_js`] decodes. With
/// `wait_for_render` it first waits two animation frames, so a render queued
/// by an earlier scroll write has reached the DOM. The eighth value is the
/// outer scroll offset: `window.scrollY` plus the `scrollTop` of every
/// ancestor of the container.
///
/// The cover is how far sticky `<th>` cells reach below the container's top
/// edge, so a non-sticky filter row (which scrolls away) is not counted. The
/// `chorale:measure-row` marker lets tests recognize the script.
fn measure_row_js(scroll_id: &str, row_idx: usize, wait_for_render: bool) -> String {
    let sel = body_row_selector(row_idx);
    let wait = if wait_for_render {
        "await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));"
    } else {
        ""
    };
    format!(
        "/*chorale:measure-row*/ {wait}\
         const el = document.getElementById('{scroll_id}');\
         if (!el) {{ dioxus.send([]); }} else {{\
         const top0 = el.getBoundingClientRect().top + el.clientTop;\
         const head = el.querySelector(':scope > table > thead');\
         let cover = 0;\
         if (head) head.querySelectorAll('th').forEach(th => {{\
           if (getComputedStyle(th).position === 'sticky') \
             cover = Math.max(cover, th.getBoundingClientRect().bottom - top0);\
         }});\
         const thead = head ? head.getBoundingClientRect().height : 0;\
         let outer = window.scrollY || 0;\
         for (let p = el.parentElement; p && p !== document.documentElement; p = p.parentElement) outer += p.scrollTop;\
         const tr = el.querySelector('{sel}');\
         if (tr) {{ const r = tr.getBoundingClientRect();\
           dioxus.send([1, r.top - top0 + el.scrollTop, r.height, el.scrollTop, el.clientHeight, cover, thead, outer]); }}\
         else dioxus.send([0, 0, 0, el.scrollTop, el.clientHeight, cover, thead, outer]); }}"
    )
}

/// JS that sets the container's `scrollTop` (when `scroll_top` is `Some`) and
/// then reveals body row `row_idx` in every scrolling ancestor and in the
/// window, so the row is on screen even when the page, not the table, is
/// what scrolls (a table taller than the window in Paged mode).
///
/// Vertical only: `scrollIntoView` would also scroll horizontally and reset
/// the grid's horizontal scroll position. Sends back the outer scroll offset
/// it leaves (as [`measure_row_js`] computes it), so a later measurement can
/// tell a user scroll of the page from the reveal's own. The
/// `chorale:reveal-row` marker lets tests recognize the script.
fn reveal_row_js(scroll_id: &str, row_idx: usize, scroll_top: Option<f64>) -> String {
    let sel = body_row_selector(row_idx);
    let write = match scroll_top {
        Some(t) if t.is_finite() => format!("el.scrollTo({{ top: {t}, behavior: 'instant' }});"),
        _ => String::new(),
    };
    format!(
        "/*chorale:reveal-row*/ const el = document.getElementById('{scroll_id}');\
         if (el) {{ {write}\
         const tr = el.querySelector('{sel}');\
         if (tr) {{\
           for (let p = el.parentElement; p && p !== document.documentElement; p = p.parentElement) {{\
             const oy = getComputedStyle(p).overflowY;\
             if ((oy === 'auto' || oy === 'scroll' || oy === 'overlay') && p.scrollHeight > p.clientHeight) {{\
               const r = tr.getBoundingClientRect(); const pr = p.getBoundingClientRect();\
               const top = pr.top + p.clientTop; const bottom = top + p.clientHeight;\
               if (r.top < top) p.scrollBy({{ top: r.top - top, behavior: 'instant' }});\
               else if (r.bottom > bottom) p.scrollBy({{ top: Math.min(r.bottom - bottom, r.top - top), behavior: 'instant' }});\
             }}\
           }}\
           const r = tr.getBoundingClientRect();\
           const vh = window.innerHeight || document.documentElement.clientHeight;\
           if (r.top < 0) window.scrollBy({{ top: r.top, behavior: 'instant' }});\
           else if (r.bottom > vh) window.scrollBy({{ top: Math.min(r.bottom - vh, r.top), behavior: 'instant' }});\
         }}\
         let outer = window.scrollY || 0;\
         for (let p = el.parentElement; p && p !== document.documentElement; p = p.parentElement) outer += p.scrollTop;\
         dioxus.send(outer); }} else dioxus.send(0);"
    )
}

/// The last `scrollTop` a table wrote to its own scroll container (shared by
/// the page effect, the VIRT-2 anchor write and keyboard scroll-follow).
type ProgrammaticScroll = std::rc::Rc<std::cell::Cell<Option<f64>>>;

/// JS that sets the container's `scrollTop` to `scroll_top` instantly, even
/// under `scroll-behavior: smooth` (an animated write would read back as a
/// value in between, which scroll-follow would take for a user scroll). The
/// `chorale:scroll-to` marker lets tests recognize the script.
fn instant_scroll_js(scroll_id: &str, scroll_top: f64) -> String {
    let scroll_top = if scroll_top.is_finite() {
        scroll_top
    } else {
        0.0
    };
    format!(
        "/*chorale:scroll-to*/ const el = document.getElementById('{scroll_id}'); \
         if (el) el.scrollTo({{ top: {scroll_top}, behavior: 'instant' }});"
    )
}

/// How far the container's `scrollTop` may differ from the value
/// [`ensure_active_row_visible`] last wrote before it counts as a user
/// scroll (browsers round written values to device pixels).
const USER_SCROLL_SLACK_PX: f64 = 1.0;

/// How many times [`ensure_active_row_visible`] measures the active row at
/// most: a far jump in Virtualized mode needs one measurement to find the row
/// missing, one after the estimate scroll and one or two to verify a
/// correction. Every measurement after the first waits for a render.
const REVEAL_ATTEMPTS: usize = 4;

/// After a keyboard-navigation transition lands a new `active_cell`, scroll
/// so the active row is fully visible: inside the table's scroll container
/// (below the sticky `<thead>`), and on screen when the page or another
/// ancestor is the scroller (issue #51).
///
/// The row is measured in the DOM rather than assumed to sit at
/// `row_idx * row_height`: group header rows and host header content differ
/// in height from data rows, so the fixed-height math let the active row
/// leave the view for a few presses. web-sys is unavailable in this adapter,
/// so measurement is an async `eval` round-trip, which also guarantees a
/// post-layout read.
///
/// 1. Measure the row ([`measure_row_js`]).
/// 2. Rendered: compute the target with [`reveal_scroll_top`]; write it to
///    `TableState.scroll_top` (so the next render computes the window from the
///    post-scroll position) and to the DOM, then reveal the row in outer
///    scrollers ([`reveal_row_js`]). After a write, measure again: a
///    virtualized window re-slices on scroll, and rows whose rendered height
///    differs from `row_height` (the default group header) shift the row.
/// 3. Not rendered (virtualized, far jump): scroll to the estimate from
///    [`estimated_row_offset`], wait for the render and measure again.
///
/// Steps 2 and 3 repeat until no write is needed, at most
/// [`REVEAL_ATTEMPTS`] measurements. They also stop when the cursor has moved
/// to another row (a newer key press owns the scroll) or when the container's
/// `scrollTop` is no longer the value this task wrote (the user scrolled).
///
/// Keyboard-driven only: the grid keydown handler is the sole caller, after
/// an arrow, Page, Home/End or Tab move. Mouse, trackpad and scrollbar
/// scrolling, focus and effects never reveal the active row, so the view
/// never snaps back to it mid-scroll.
/// 4. No container (the eval failed): fall back to the state-only estimate.
///
/// Inline mode is a deliberate no-op: there is no internal scroll container
/// (`overflow: visible; height: auto`), the parent owns scrolling and this
/// component has no authority over the parent's `scrollTop`.
fn ensure_active_row_visible<TRow: Clone + PartialEq + 'static>(
    handle: UseTableHandle<TRow>,
    scroll_container_id: &str,
    variable_row_height: bool,
    inline: bool,
    programmatic: &ProgrammaticScroll,
) {
    if inline {
        return;
    }
    // Snapshot the estimate inputs synchronously. A grouped body is sliced
    // with the fixed row height whatever `variable_row_height` says.
    let (row_idx, scroll_top, viewport_height, row_height, row_heights, variable) = {
        let sig = handle.signal();
        let s = sig.peek();
        let Some(ac) = s.active_cell else {
            return;
        };
        (
            ac.row_idx,
            s.scroll_top,
            s.viewport_height,
            s.row_height,
            s.row_heights.clone(),
            variable_row_height && s.grouping.is_empty(),
        )
    };
    let cid = scroll_container_id.to_owned();
    let programmatic = programmatic.clone();
    spawn(async move {
        // The scrollTop this task last wrote. A later measurement that reads a
        // different value means the user scrolled (wheel, trackpad, scrollbar)
        // since: the reveal ends instead of snapping the view back.
        let mut last_written: Option<f64> = None;
        // The outer scroll offset (window and ancestors) the last reveal left.
        let mut last_outer: Option<f64> = None;
        for attempt in 0..REVEAL_ATTEMPTS {
            let mut js = dioxus::document::eval(&measure_row_js(&cid, row_idx, attempt > 0));
            let raw = js.recv::<Vec<f64>>().await.ok();
            let measured = raw.as_deref().and_then(RowMeasurement::from_js);
            let outer_now = raw.as_deref().and_then(measured_outer_offset);
            // The window or an ancestor scrolled since the reveal: the user
            // did that, so stop rather than scroll the page back.
            if let (Some(before), Some(now)) = (last_outer, outer_now) {
                if (now - before).abs() > USER_SCROLL_SLACK_PX {
                    return;
                }
            }
            // A newer key press moved the cursor: its own task follows it,
            // so this one stops instead of fighting it over scrollTop.
            let still_active = handle
                .signal()
                .peek()
                .active_cell
                .is_some_and(|ac| ac.row_idx == row_idx);
            if !still_active {
                return;
            }
            let measured_scroll = match measured {
                Some(RowMeasurement::Found(g)) => Some(g.scroll_top),
                Some(RowMeasurement::Missing { scroll_top, .. }) => Some(scroll_top),
                None => None,
            };
            if let (Some(w), Some(now)) = (last_written, measured_scroll) {
                if (now - w).abs() > USER_SCROLL_SLACK_PX {
                    // Another write by this table (the VIRT-2 anchor after a
                    // row-height measurement) is not a user scroll: carry on
                    // from where it left the view.
                    let ours = programmatic
                        .get()
                        .is_some_and(|p| (now - p).abs() <= USER_SCROLL_SLACK_PX);
                    if !ours {
                        return;
                    }
                    last_written = Some(now);
                }
            }
            match measured {
                Some(RowMeasurement::Found(g)) => {
                    let target = reveal_scroll_top(g);
                    if let Some(t) = target {
                        handle.set_scroll(t);
                        last_written = Some(t);
                        programmatic.set(Some(t));
                    }
                    let mut reveal = dioxus::document::eval(&reveal_row_js(&cid, row_idx, target));
                    if target.is_none() {
                        return;
                    }
                    last_outer = reveal.recv::<f64>().await.ok().filter(|x| x.is_finite());
                    // A scroll write re-slices a virtualized window; rows
                    // rendered at heights other than row_height (the default
                    // group header) then shift. Measure again to verify.
                }
                Some(RowMeasurement::Missing {
                    scroll_top,
                    client_height,
                    cover,
                    thead,
                }) => {
                    let (row_top, height) =
                        estimated_row_offset(row_idx, row_height, &row_heights, variable);
                    let target = reveal_scroll_top(RowGeometry {
                        top: thead + row_top,
                        height,
                        scroll_top,
                        client_height,
                        cover,
                    });
                    let Some(t) = target else {
                        // The estimate says the row is in view, yet it is
                        // not rendered: nothing to scroll to.
                        return;
                    };
                    handle.set_scroll(t);
                    last_written = Some(t);
                    programmatic.set(Some(t));
                    dioxus::document::eval(&instant_scroll_js(&cid, t));
                }
                None => {
                    if let Some(t) = scroll_top_to_reveal_row(
                        row_idx,
                        scroll_top,
                        viewport_height,
                        row_height,
                        &row_heights,
                        variable,
                        0.0,
                    ) {
                        handle.set_scroll(t);
                        programmatic.set(Some(t));
                        dioxus::document::eval(&instant_scroll_js(&cid, t));
                    }
                    return;
                }
            }
        }
    });
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
    // When true (first column of a grouped table with the toggle enabled), prepend
    // an expand-all / collapse-all control to this header. `all_groups_expanded`
    // selects the icon + action.
    show_group_toggle: bool,
    all_groups_expanded: bool,
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
            // Key includes column_reorder_enabled AND is_sortable so the <th> is
            // RECREATED whenever either toggles — both change the cursor in
            // `extra` and thus the style string, and Dioxus 0.7's inline-style
            // diff unreliably drops a declaration on an in-place change (the
            // background on reorder, the border-bottom on sort — the header
            // underline vanished from every data column when Sort was enabled).
            // Keying by col_id also lets reorder MOVE the <th> by identity
            // rather than diffing in place.
            key: "{col_id}-{column_reorder_enabled}-{is_sortable}",
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
            if show_group_toggle {
                span {
                    class: "chorale-group-expand-all",
                    role: "button",
                    tabindex: "0",
                    title: if all_groups_expanded { "Collapse all groups" } else { "Expand all groups" },
                    "aria-label": if all_groups_expanded { "Collapse all groups" } else { "Expand all groups" },
                    // Absolutely positioned at the cell's left edge so it sits directly
                    // above the per-row group chevrons (which render at the group cell's
                    // left, 1rem left of this header's padded text) without shifting the
                    // column header label. A touch larger than the row chevrons so the
                    // "all" control reads as distinct from them.
                    style: "position: absolute; left: 0; top: 0; bottom: 0; \
                            display: flex; align-items: center; \
                            cursor: pointer; user-select: none; font-weight: 700; \
                            font-size: 1.1rem; line-height: 1; \
                            color: var(--chorale-accent, #4a90e2);",
                    onclick: move |e| {
                        // Don't let the click also trigger this column's sort.
                        e.stop_propagation();
                        if all_groups_expanded {
                            handle.collapse_all_groups();
                        } else {
                            handle.expand_all_groups();
                        }
                    },
                    if all_groups_expanded { "\u{25bc}" } else { "\u{25b6}" }
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
            onkeydown: move |e: KeyboardEvent| keep_native_control_keys(&e),
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
        FilterKind::None => rsx! { th { key: "{col_id}", style: "{empty_th_style}" } },
        FilterKind::Text => {
            let text = match &current {
                Some(FilterValue::Text(s)) => s.clone(),
                _ => String::new(),
            };
            let has_filter = current.is_some();
            rsx! {
                th { key: "{col_id}", style: "{th_style}",
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
                th { key: "{col_id}", style: "{th_style}",
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
        FilterKind::MultiSelectContains { options, separator } => {
            // Same checkbox picker as MultiSelect; the separator makes it emit a
            // MultiSelectContains filter (OR-match over the cell's tokenized list).
            let has_filter = current.is_some();
            rsx! {
                th { key: "{col_id}", style: "{th_style}",
                    div {
                        style: "display: flex; align-items: center; gap: 2px;",
                        div { style: "flex: 1; min-width: 0;",
                            MultiSelectFilter::<TRow> {
                                col_id,
                                options: options.clone(),
                                current: current.clone(),
                                handle,
                                all_label: all_label.clone(),
                                separator: separator.clone(),
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
                th { key: "{col_id}", style: "{th_style}",
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
                th { key: "{col_id}", style: "{th_style}",
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
                th { key: "{col_id}", style: "{th_style}",
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
        _ => rsx! { th { key: "{col_id}", style: "{empty_th_style}" } },
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
    // When `Some`, this picker drives a `MultiSelectContains` filter over list-valued cells
    // (tokens split on this separator); when `None`, a plain exact-match `MultiSelect`.
    #[props(default)] separator: Option<String>,
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
        Some(FilterValue::MultiSelectContains { selected, .. }) => selected.clone(),
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
                    {multi_select_option(col_id, opt.clone(), selected.clone(), handle, separator.clone())}
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
    separator: Option<String>,
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
                    } else if let Some(sep) = &separator {
                        handle.set_filter(
                            col_id,
                            Some(FilterValue::MultiSelectContains {
                                selected: next,
                                separator: sep.clone(),
                            }),
                        );
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
    row_class: &RowClass<TRow>,
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
    // In a grouped table, the row's treegrid `(aria-level, aria-rowindex)`;
    // `None` (no treegrid attributes, no ids) otherwise.
    grouped: Option<(usize, usize)>,
) -> Element {
    let aria_level = grouped.map(|(level, _)| level);
    let aria_rowindex = grouped.map(|(_, rowindex)| rowindex);
    // Ids for aria-activedescendant (issue #51): every grouped row, and the
    // cell that holds the keyboard cursor.
    let row_dom_id = grouped.map(|_| grouped_row_dom_id(kb_id, row_index));
    let cell_dom_id = |col: ColumnId| {
        let active = active_cell.is_some_and(|ac| ac.row_idx == row_index && ac.column_id == col);
        (grouped.is_some() && active).then(|| grouped_cell_dom_id(kb_id, row_index, col))
    };
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
    // Selected-row background is applied via a STYLESHEET rule keyed on the
    // `data-chorale-row-selected` attribute (see `theme_stylesheet`), not an
    // inline `background` declaration. Dioxus 0.7's inline-style diff on a <tr>
    // reliably updates CSS custom properties but drops standard `background`
    // changes — confirmed by reading the live DOM: a freshly-selected row kept
    // `--chorale-separator-color` updated to the selected divider but its
    // `background` declaration was stale/absent, so the row never repainted
    // (issue #20). Data attributes diff reliably (`data-chorale-index` proves
    // it), so driving the highlight through the attribute + CSS sidesteps the
    // bug. The divider color still rides the custom property, which diffs fine.
    let row_selected = is_selected && selection_enabled;
    let separator_color = if row_selected {
        "var(--chorale-row-selected-divider, #dbeafe)"
    } else {
        "var(--chorale-button-disabled-bg, #f0f0f0)"
    };
    let row_style = format!("--chorale-separator-color: {separator_color};");
    // Consumer-provided per-row class (#32). Applied to the row's <tr>; the consumer styles
    // it via their own CSS. Composes with selection (the selection background is driven by
    // the data-chorale-row-selected attribute, not this class).
    let row_class_attr = row_class.class_for(row).unwrap_or_default();
    // Focus ring on the chevron when the active cell is on the detail-expander
    // column of this row (keyboard navigation, issue #17).
    let chevron_focused = active_cell.is_some_and(|ac| {
        ac.row_idx == row_index && ac.column_id == chorale_core::DETAIL_EXPANDER_COLUMN
    });
    let chevron_outline = if chevron_focused {
        "outline: 2px solid var(--chorale-active-cell-outline, #0078d4); outline-offset: -2px;"
    } else {
        ""
    };
    rsx! {
        tr {
            // Stable key so Dioxus diffs grouped rows by IDENTITY, not position. Without it,
            // collapsing a group removes a contiguous run of rows and the positional diff
            // patches a data `tr` (N cells) and a group-header `tr` (one colspan cell) into
            // each other, corrupting the render (a parent header showing a child's indent /
            // jumbled counts). Data rows key on their RowId.
            key: "row-{row_id:?}",
            class: "{row_class_attr}",
            style: "{row_style}",
            "data-chorale-index": "{row_index}",
            "data-chorale-row-selected": "{row_selected}",
            "aria-level": aria_level.map(|l| l.to_string()),
            "aria-rowindex": aria_rowindex.map(|i| i.to_string()),
            id: row_dom_id,
            if selection_enabled {
                td {
                    style: "padding: 0.25rem 0.5rem; width: 2.5rem; text-align: center; \
                            box-shadow: inset 0 -1px 0 {separator_color};",
                    // Space ticks the row checkbox and Tab moves on natively.
                    onkeydown: move |e: KeyboardEvent| keep_native_control_keys(&e),
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
                    id: cell_dom_id(chorale_core::DETAIL_EXPANDER_COLUMN),
                    style: "width: 24px; cursor: pointer; user-select: none; text-align: center; \
                            position: relative; box-shadow: inset 0 -1px 0 {separator_color};{chevron_outline}",
                    "aria-label": if is_expanded { "Collapse row" } else { "Expand row" },
                    "aria-expanded": if is_expanded { "true" } else { "false" },
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
                        data_td(row, col, row_height, variable_row_height, widths.get(&col.id).copied(), cell_renderer.as_ref(), row_renderer.as_ref(), separator_color, sticky_css_map.get(&col.id).map_or("", String::as_str), is_active, is_in_range, row_index, row_id, handle, is_focus_cell, fill_drag_active, fill_hover, on_row_click, grouped.is_some(), cell_dom_id(col.id))
                    }
                }
            }
        }
    }
}

/// JS that focuses the element with DOM id `id` (the chevron focuses itself on
/// click, since Safari and macOS Firefox do not focus a button on click).
fn focus_element_js(id: &str) -> String {
    format!("var b=document.getElementById('{id}');if(b)b.focus();")
}

/// What the page effect writes to the DOM `scrollTop`: the saved offset on the
/// first run (mount), a literal 0 on a later page change, nothing when the page
/// did not change.
fn page_scroll_target(prev_page: Option<usize>, page: usize, saved: f64) -> Option<f64> {
    match prev_page {
        None => Some(saved),
        Some(p) if p == page => None,
        Some(_) => Some(0.0),
    }
}

/// JS that snaps the scroll container's native `scrollTop` to `scroll_top`.
/// A remounted table passes its saved `state.scroll_top`; a page change passes a
/// literal 0.
fn sync_scroll_js(scroll_id: &str, scroll_top: f64) -> String {
    // `inf` / `NaN` would format into invalid JS.
    let scroll_top = if scroll_top.is_finite() {
        scroll_top
    } else {
        0.0
    };
    format!(
        "const el = document.getElementById('{scroll_id}'); if (el) el.scrollTop = {scroll_top};"
    )
}

/// Keydown filter for focusable controls the grid contains but does not own:
/// the `<thead>` (filter inputs, the select-all checkbox, header buttons), the
/// row selection checkboxes, the toolbars and the footer (pagination, page
/// input, export buttons). `Space`, `Enter` and `Tab` keep their native
/// meaning there (type a space, tick a checkbox, press a button, move focus
/// on), so they never reach the grid's key handler, which would prevent them
/// and act on the cursor instead (issue #51 review round 2).
fn keep_native_control_keys(e: &KeyboardEvent) {
    let key = e.key();
    if matches!(key, Key::Enter | Key::Tab) || matches!(&key, Key::Character(c) if c == " ") {
        e.stop_propagation();
    }
}

/// Whether the group chevron button swallows this key. Only the keys that
/// activate it or move focus away; the rest reach the grid so arrows and
/// Ctrl+C / Ctrl+A keep working after a chevron click leaves focus on it.
fn chevron_keydown_stops(key: &Key) -> bool {
    match key {
        Key::Enter | Key::Tab => true,
        Key::Character(c) => c == " ",
        _ => false,
    }
}

/// Whether a key the chevron lets through is one the grid acts on, so focus
/// must move to the keyboard container first. Modifier-only presses (Shift,
/// Control, Alt, Meta) are excluded: refocusing on a bare Shift would break
/// Shift+Tab out of the chevron.
fn chevron_keydown_refocuses(key: &Key, ctrl: bool) -> bool {
    match key {
        Key::ArrowDown
        | Key::ArrowUp
        | Key::ArrowLeft
        | Key::ArrowRight
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown => true,
        Key::Character(_) => ctrl,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Grouped-mode keyboard navigation (issue #51)
// ---------------------------------------------------------------------------
//
// In a grouped table the body renders `visible_grouped_view`, so a row index
// (the one clicks write into `active_cell.row_idx` and the one the cursor
// highlight compares against) counts group header rows too. The core
// `move_active_cell*` transitions count `visible_view` rows instead (data rows
// of one page), which let the cursor land on a header with nothing drawn and
// clamped it at the page size in Virtualized mode. Grouped tables therefore
// navigate here, in the grouped index space, with header rows as stops (the
// ARIA treegrid pattern).
//
// One row model: the `grouped_view` memo (`visible_grouped_view` of the
// state) is what the body renders, what clicks index, what the cursor moves
// over and what Enter/F2 look rows up in. Code that needs "the row at the
// cursor" in a grouped table reads that list; see the "Row model" section of
// `visible_grouped_view`'s docs.

/// One cursor move in grouped mode.
#[derive(Clone, Copy, Debug)]
enum GroupedMove {
    /// Arrow key: one row or one column.
    Step(NavDirection),
    /// Ctrl+Arrow: to the first or last row or column.
    Edge(NavDirection),
    /// Page Up or Page Down by this many rows (horizontal directions do nothing).
    Page(NavDirection, usize),
    /// Home: first column of the current row.
    RowStart,
    /// End: last column of the current row.
    RowEnd,
    /// Ctrl+Home: first column of the first row.
    First,
    /// Ctrl+End: last column of the last row.
    Last,
}

/// Where `mv` takes the cursor in a grouped view of `rows` rows and the
/// navigable columns `cols`, or `None` when there is nothing to move over
/// (or a Page move is horizontal).
///
/// Mirrors the core transitions: clamps at the edges without wrapping, and
/// with no active cell a Step Up or Left starts at the last cell while every
/// other move starts from the first. The column is kept when moving onto and
/// off a header row, so arrowing through a header returns to the same column.
fn grouped_move_target(
    current: Option<ActiveCell>,
    rows: usize,
    cols: &[ColumnId],
    mv: GroupedMove,
) -> Option<ActiveCell> {
    if rows == 0 || cols.is_empty() {
        return None;
    }
    let last_row = rows - 1;
    let last_col = cols.len() - 1;
    let Some(ac) = current else {
        let (r, c) = match mv {
            // Like move_active_cell_page: start at row 0, then page.
            GroupedMove::Page(NavDirection::Down, n) => (n.min(last_row), 0),
            GroupedMove::Page(NavDirection::Up, _) => (0, 0),
            GroupedMove::Page(..) => return None,
            GroupedMove::Step(NavDirection::Up | NavDirection::Left) | GroupedMove::Last => {
                (last_row, last_col)
            }
            GroupedMove::Edge(NavDirection::Down) => (last_row, 0),
            GroupedMove::Edge(NavDirection::Right) | GroupedMove::RowEnd => (0, last_col),
            _ => (0, 0),
        };
        return Some(ActiveCell::new(r, cols[c]));
    };
    let row = ac.row_idx.min(last_row);
    let col = cols
        .iter()
        .position(|c| *c == ac.column_id)
        .unwrap_or(0)
        .min(last_col);
    let (r, c) = match mv {
        GroupedMove::Step(NavDirection::Up) => (row.saturating_sub(1), col),
        GroupedMove::Step(NavDirection::Down) => ((row + 1).min(last_row), col),
        GroupedMove::Step(NavDirection::Left) => (row, col.saturating_sub(1)),
        GroupedMove::Step(NavDirection::Right) => (row, (col + 1).min(last_col)),
        GroupedMove::Edge(NavDirection::Up) => (0, col),
        GroupedMove::Edge(NavDirection::Down) => (last_row, col),
        GroupedMove::Edge(NavDirection::Left) | GroupedMove::RowStart => (row, 0),
        GroupedMove::Edge(NavDirection::Right) | GroupedMove::RowEnd => (row, last_col),
        GroupedMove::Page(NavDirection::Up, n) => (row.saturating_sub(n), col),
        GroupedMove::Page(NavDirection::Down, n) => ((row + n).min(last_row), col),
        // `NavDirection` is non-exhaustive; any future direction is a no-op.
        GroupedMove::Step(_) | GroupedMove::Edge(_) | GroupedMove::Page(..) => return None,
        GroupedMove::First => (0, 0),
        GroupedMove::Last => (last_row, last_col),
    };
    Some(ActiveCell::new(r, cols[c]))
}

/// What the grid keydown handler must still do after [`grouped_keydown`].
#[derive(Debug, PartialEq)]
enum GroupedKeyOutcome {
    /// Not a grouped-mode key; the ungrouped handling runs.
    NotHandled,
    /// Handled by letting the browser default run (Tab leaving the grid).
    BrowserDefault,
    /// Handled; prevent the browser default and stop.
    Handled,
    /// The cursor moved; prevent the default and scroll it into view.
    Moved,
    /// Tab moved the cursor to this cell; prevent the default and fire
    /// `on_tab_to_editable` when the column has an editor.
    TabMoved(ActiveCell),
}

/// Grouped-mode handling of one grid keydown. `view` is the rendered grouped
/// view (the memo the body iterates), `cols` the navigable columns and
/// `header_has_controls` whether a header row holds focusable controls (the
/// chevron with a host renderer, or the group checkbox with selection on).
///
/// Header rows are treegrid rows: Left collapses, Right expands, Enter and
/// Space toggle, Tab moves focus into the header's own controls. Where Tab or
/// Shift+Tab cannot move (Shift+Tab on a header, a header without controls,
/// the last or first column) the browser default moves focus to the next or
/// previous focusable element. Every move, including Shift+Arrow range extension,
/// stays in the grouped index space. Space on a data row is prevented (no
/// page scroll). Escape and Ctrl+A keep their ungrouped handling; Ctrl+C and
/// Ctrl+V resolve rows through the grouped view in the keydown handler.
#[allow(clippy::too_many_arguments)]
fn grouped_keydown<TRow: Clone + PartialEq + 'static>(
    handle: UseTableHandle<TRow>,
    view: &[GroupedRow<TRow>],
    cols: &[ColumnId],
    key: &Key,
    shift: bool,
    ctrl: bool,
    scroll_id: &str,
    header_has_controls: bool,
) -> GroupedKeyOutcome {
    let mut sig = handle.signal();
    let (active, editing, page_size) = {
        let s = sig.peek();
        (s.active_cell, s.editing.is_some(), s.page_size)
    };
    // Open editors stop their own keydowns; anything that still bubbles while
    // an edit is open keeps the ungrouped behavior.
    if editing {
        return GroupedKeyOutcome::NotHandled;
    }
    let row = active.and_then(|ac| view.get(ac.row_idx).map(|r| (ac, r)));
    let header = match row {
        Some((
            ac,
            GroupedRow::Header {
                key, is_collapsed, ..
            },
        )) => Some((ac, key.clone(), *is_collapsed)),
        _ => None,
    };
    let data_row_id = match row {
        Some((_, GroupedRow::Data(id, _))) => Some(*id),
        _ => None,
    };
    let is_space = matches!(key, Key::Character(c) if c == " ");

    if let Some((ac, group_key, is_collapsed)) = header {
        match key {
            Key::ArrowLeft if !shift && !ctrl => {
                if !is_collapsed {
                    handle.toggle_group(group_key);
                }
                return GroupedKeyOutcome::Handled;
            }
            Key::ArrowRight if !shift && !ctrl => {
                if is_collapsed {
                    handle.toggle_group(group_key);
                }
                return GroupedKeyOutcome::Handled;
            }
            Key::Enter => {
                handle.toggle_group(group_key);
                return GroupedKeyOutcome::Handled;
            }
            _ if is_space && !ctrl => {
                handle.toggle_group(group_key);
                return GroupedKeyOutcome::Handled;
            }
            Key::F2 => return GroupedKeyOutcome::Handled,
            Key::Tab => {
                // With no header controls to enter, and for Shift+Tab, the
                // browser moves focus to the next (previous) focusable element.
                if shift || !header_has_controls {
                    return GroupedKeyOutcome::BrowserDefault;
                }
                focus_header_controls(scroll_id, ac.row_idx);
                return GroupedKeyOutcome::Handled;
            }
            _ => {}
        }
    }

    // Space on a data row: no browser page scroll; keep the row revealed.
    if is_space && !ctrl && data_row_id.is_some() {
        return GroupedKeyOutcome::Moved;
    }

    // Shift+Arrow extends the range in the grouped index space.
    if shift {
        if let Some(dir) = match key {
            Key::ArrowUp => Some(NavDirection::Up),
            Key::ArrowDown => Some(NavDirection::Down),
            Key::ArrowLeft => Some(NavDirection::Left),
            Key::ArrowRight => Some(NavDirection::Right),
            _ => None,
        } {
            let s = sig.peek().clone();
            let focus = s
                .range_selection
                .last()
                .map(|r| ActiveCell::new(r.focus.0, r.focus.1))
                .or(s.active_cell);
            let mv = if ctrl {
                GroupedMove::Edge(dir)
            } else {
                GroupedMove::Step(dir)
            };
            let Some(target) = grouped_move_target(focus, view.len(), cols, mv) else {
                return GroupedKeyOutcome::Handled;
            };
            let mut new_s = s;
            if let Some(last) = new_s.range_selection.last_mut() {
                last.focus = (target.row_idx, target.column_id);
            } else {
                new_s.range_selection = vec![chorale_core::RangeSelection::single(
                    target.row_idx,
                    target.column_id,
                )];
            }
            new_s.active_cell = Some(target);
            sig.set(new_s);
            return GroupedKeyOutcome::Moved;
        }
    }

    let mv = match key {
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight if !shift => {
            let dir = match key {
                Key::ArrowUp => NavDirection::Up,
                Key::ArrowDown => NavDirection::Down,
                Key::ArrowLeft => NavDirection::Left,
                _ => NavDirection::Right,
            };
            if ctrl {
                GroupedMove::Edge(dir)
            } else {
                GroupedMove::Step(dir)
            }
        }
        Key::Home if ctrl => GroupedMove::First,
        Key::Home => GroupedMove::RowStart,
        Key::End if ctrl => GroupedMove::Last,
        Key::End => GroupedMove::RowEnd,
        Key::PageUp => GroupedMove::Page(NavDirection::Up, page_size),
        Key::PageDown => GroupedMove::Page(NavDirection::Down, page_size),
        Key::Tab => GroupedMove::Step(if shift {
            NavDirection::Left
        } else {
            NavDirection::Right
        }),
        Key::Enter | Key::F2 => {
            // Resolve the data row under the cursor in the grouped view (the
            // ungrouped lookup would pick a different row). Enter on the
            // detail-expander column toggles that row's panel; otherwise
            // Enter and F2 edit.
            if let (Some(ac), Some(row_id)) = (active, data_row_id) {
                let on_expander = ac.column_id == chorale_core::DETAIL_EXPANDER_COLUMN;
                if on_expander && matches!(key, Key::Enter) {
                    handle.toggle_row_expansion(row_id);
                } else {
                    let started = chorale_core::start_edit(&*sig.peek(), row_id, ac.column_id);
                    if let Ok(new_s) = started {
                        sig.set(new_s);
                    }
                }
            }
            return GroupedKeyOutcome::Handled;
        }
        _ => return GroupedKeyOutcome::NotHandled,
    };
    let Some(target) = grouped_move_target(active, view.len(), cols, mv) else {
        return GroupedKeyOutcome::Handled;
    };
    // Tab or Shift+Tab that cannot move (last or first column): the grid is
    // one Tab stop, so the browser moves focus on to the next element.
    if matches!(key, Key::Tab) && active == Some(target) {
        return GroupedKeyOutcome::BrowserDefault;
    }
    // Like the core moves, a plain move collapses any range to the new cell.
    let mut new_s = sig.peek().clone();
    new_s.active_cell = Some(target);
    new_s.range_selection = vec![chorale_core::RangeSelection::single(
        target.row_idx,
        target.column_id,
    )];
    sig.set(new_s);
    if matches!(key, Key::Tab) {
        GroupedKeyOutcome::TabMoved(target)
    } else {
        GroupedKeyOutcome::Moved
    }
}

/// CSS selector (for a `querySelector` on the table's scroll container) of
/// body row `row_idx`: a data row or detail panel carries
/// `data-chorale-index`, a group header row `data-chorale-group-index`. The
/// `:scope > table > tbody > tr` chain keeps the lookup inside this table's
/// own rows, not a nested child table's. It contains double quotes only, so
/// it can sit inside a single-quoted JS string.
fn body_row_selector(row_idx: usize) -> String {
    format!(
        ":scope > table > tbody > tr[data-chorale-index=\"{row_idx}\"], \
         :scope > table > tbody > tr[data-chorale-group-index=\"{row_idx}\"]"
    )
}

/// The grouped view's data rows as an ungrouped, single-page table, so the
/// core clipboard and fill functions (which index `visible_view` rows) work
/// on exactly the rows a grouped table shows. Returns the projected state and
/// the grouped index of each projected row (`data_rows[k]` is the grouped
/// index of projected row `k`). Header rows have no projected row.
///
/// Copies every data row of the view; used only on explicit user actions
/// (copy, paste, fill), never per render.
fn grouped_data_projection<TRow: Clone>(
    state: &TableState<TRow>,
    view: &[GroupedRow<TRow>],
) -> (TableState<TRow>, Vec<usize>) {
    let mut rows = Vec::new();
    let mut data_rows = Vec::new();
    for (i, r) in view.iter().enumerate() {
        if let GroupedRow::Data(id, row) = r {
            rows.push((*id, row.clone()));
            data_rows.push(i);
        }
    }
    let mut p = state.clone();
    p.page_size = rows.len().max(1);
    p.rows = Arc::new(rows);
    p.grouping = Vec::new();
    p.collapsed_groups = HashSet::new();
    p.sort = Vec::new();
    p.filters = HashMap::new();
    p.page = 0;
    p.pagination_mode = PaginationMode::Pages;
    p.expanded_rows = HashSet::new();
    p.range_selection = state
        .range_selection
        .iter()
        .filter_map(|r| project_range(r, &data_rows))
        .collect();
    p.active_cell = state.active_cell.and_then(|ac| {
        data_rows
            .iter()
            .position(|g| *g == ac.row_idx)
            .map(|k| ActiveCell::new(k, ac.column_id))
    });
    (p, data_rows)
}

/// A grouped range in projected (data-row) coordinates: its top edge moves
/// down to the first data row at or below it, its bottom edge up to the last
/// data row at or above it. `None` when the range covers header rows only.
fn project_range(
    r: &chorale_core::RangeSelection,
    data_rows: &[usize],
) -> Option<chorale_core::RangeSelection> {
    let (lo, hi) = (r.anchor.0.min(r.focus.0), r.anchor.0.max(r.focus.0));
    let first = data_rows.iter().position(|g| *g >= lo)?;
    let last = data_rows.iter().rposition(|g| *g <= hi)?;
    if first > last {
        return None;
    }
    let (a, f) = if r.anchor.0 <= r.focus.0 {
        (first, last)
    } else {
        (last, first)
    };
    Some(chorale_core::RangeSelection::new(
        (a, r.anchor.1),
        (f, r.focus.1),
    ))
}

/// Ctrl+C text for a grouped table: the data rows under the range, header
/// rows skipped, formatted by core `to_clipboard_tsv`.
fn grouped_clipboard_tsv<TRow: Clone>(
    state: &TableState<TRow>,
    view: &[GroupedRow<TRow>],
) -> Result<String, chorale_core::ClipboardError> {
    let (p, _) = grouped_data_projection(state, view);
    if p.range_selection.is_empty() {
        return Ok(String::new());
    }
    to_clipboard_tsv(&p)
}

/// Ctrl+V range for a grouped table: core `paste_tsv_into_range` over the
/// data rows, mapped back to grouped indexes, so the pasted lines land on
/// consecutive data rows and header rows are skipped. `None` (no change)
/// when the range starts on a header row.
fn grouped_paste_state<TRow: Clone>(
    state: &TableState<TRow>,
    view: &[GroupedRow<TRow>],
    tsv: &str,
) -> Option<TableState<TRow>> {
    let first = state.range_selection.first()?;
    let top = first.anchor.0.min(first.focus.0);
    if !matches!(view.get(top), Some(GroupedRow::Data(..))) {
        return None;
    }
    let (p, data_rows) = grouped_data_projection(state, view);
    let pasted = paste_tsv_into_range(&p, tsv).ok()?;
    let to_grouped = |k: usize| data_rows.get(k).copied();
    let ranges: Option<Vec<chorale_core::RangeSelection>> = pasted
        .range_selection
        .iter()
        .map(|r| {
            Some(chorale_core::RangeSelection::new(
                (to_grouped(r.anchor.0)?, r.anchor.1),
                (to_grouped(r.focus.0)?, r.focus.1),
            ))
        })
        .collect();
    let mut out = state.clone();
    out.range_selection = ranges?;
    Some(out)
}

/// Fill-handle writes in a grouped table: like core `fill_handle_targets`,
/// but over the grouped view's data rows (header rows are never a target),
/// with rows reported as grouped indexes.
fn grouped_fill_targets<TRow: Clone>(
    state: &TableState<TRow>,
    view: &[GroupedRow<TRow>],
    source: &chorale_core::RangeSelection,
    target_row: usize,
    target_col: ColumnId,
) -> Vec<(usize, ColumnId, CellValue)> {
    let (p, data_rows) = grouped_data_projection(state, view);
    let Some(src) = project_range(source, &data_rows) else {
        return vec![];
    };
    let src_lo = source.anchor.0.min(source.focus.0);
    // The hovered row, moved onto a data row toward the source.
    let target = if target_row >= src_lo {
        data_rows.iter().rposition(|g| *g <= target_row)
    } else {
        data_rows.iter().position(|g| *g >= target_row)
    };
    let Some(target) = target else {
        return vec![];
    };
    fill_handle_targets(&p, &src, target, target_col)
        .into_iter()
        .filter_map(|(k, c, v)| data_rows.get(k).map(|g| (*g, c, v)))
        .collect()
}

/// Ctrl+A in a grouped table: like core `select_all`, but over all `rows`
/// rows of the grouped view (header rows included in the span; copy and
/// paste skip them).
fn grouped_select_all<TRow: Clone>(
    state: &TableState<TRow>,
    rows: usize,
    cols: &[ColumnId],
) -> TableState<TRow> {
    let (Some(first), Some(last)) = (cols.first(), cols.last()) else {
        return state.clone();
    };
    if rows == 0 {
        return state.clone();
    }
    let mut s = state.clone();
    s.range_selection = vec![chorale_core::RangeSelection::new(
        (0, *first),
        (rows - 1, *last),
    )];
    s
}

/// A click on a grouped data cell: like core `start_range_selection` /
/// `extend_range_to` / `add_disjoint_range`, but without clamping the row to
/// the ungrouped row count (a grouped row index can exceed it).
fn grouped_click_selection<TRow: Clone>(
    state: &TableState<TRow>,
    row: usize,
    col: ColumnId,
    ctrl: bool,
    shift: bool,
) -> TableState<TRow> {
    let mut s = state.clone();
    let here = chorale_core::RangeSelection::single(row, col);
    if ctrl {
        s.range_selection.push(here);
    } else if shift && !s.range_selection.is_empty() {
        if let Some(last) = s.range_selection.last_mut() {
            last.focus = (row, col);
        }
    } else {
        s.range_selection = vec![here];
    }
    s.active_cell = Some(ActiveCell::new(row, col));
    s
}

/// Which row a grouped cursor sits on, so it can follow that row when the
/// grouped view changes under it (a group collapsed by mouse or by the host).
#[derive(Clone, Debug, PartialEq)]
enum CursorRow {
    Header(GroupKey),
    Data(RowId),
}

/// The cursor's row and the keys of its enclosing group headers, innermost
/// first.
#[derive(Clone, Debug, PartialEq)]
struct CursorAnchor {
    row_idx: usize,
    row: CursorRow,
    ancestors: Vec<GroupKey>,
}

fn cursor_anchor_at<TRow>(view: &[GroupedRow<TRow>], row_idx: usize) -> Option<CursorAnchor> {
    let (row, mut depth) = match view.get(row_idx)? {
        GroupedRow::Header { key, depth, .. } => (CursorRow::Header(key.clone()), *depth),
        GroupedRow::Data(id, _) => (CursorRow::Data(*id), usize::MAX),
        _ => return None,
    };
    let mut ancestors = Vec::new();
    for r in view[..row_idx].iter().rev() {
        if depth == 0 {
            break;
        }
        if let GroupedRow::Header { key, depth: d, .. } = r {
            if *d < depth {
                ancestors.push(key.clone());
                depth = *d;
            }
        }
    }
    Some(CursorAnchor {
        row_idx,
        row,
        ancestors,
    })
}

/// Where a cursor anchored at `anchor` belongs in the new grouped `view`:
/// its own row when present; else its innermost enclosing header when that
/// header is now collapsed (APG: collapsing moves focus to the parent row);
/// else `None` (leave the index, as on a page change).
fn relocate_cursor<TRow>(view: &[GroupedRow<TRow>], anchor: &CursorAnchor) -> Option<usize> {
    let find_header = |k: &GroupKey| {
        view.iter()
            .position(|r| matches!(r, GroupedRow::Header { key, .. } if key == k))
    };
    let own = match &anchor.row {
        CursorRow::Header(k) => find_header(k),
        CursorRow::Data(id) => view
            .iter()
            .position(|r| matches!(r, GroupedRow::Data(rid, _) if rid == id)),
    };
    if own.is_some() {
        return own;
    }
    let idx = anchor.ancestors.iter().find_map(find_header)?;
    matches!(
        view.get(idx),
        Some(GroupedRow::Header {
            is_collapsed: true,
            ..
        })
    )
    .then_some(idx)
}

/// DOM id of grouped body row `row_idx` (`aria-activedescendant` target for a
/// header row).
fn grouped_row_dom_id(kb_id: &str, row_idx: usize) -> String {
    format!("{kb_id}-row-{row_idx}")
}

/// DOM id of the active data cell (`aria-activedescendant` target), unique
/// per table, row and column. ASCII alphanumeric, `-` and `_` only.
fn grouped_cell_dom_id(kb_id: &str, row_idx: usize, col: ColumnId) -> String {
    let col: String = col
        .0
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{kb_id}-cell-{row_idx}-{col}")
}

/// Keydown on a group header control (the group checkbox, either header
/// kind, or the chevron). `Space`, `Enter` and `Tab` stay with the control.
/// Grid keys first return focus to the keyboard container; an arrow key also
/// puts the cursor on this header row, so the grid then acts on this header
/// (`←` collapses, `→` expands, `↑` / `↓` move from it).
fn header_control_keydown<TRow: Clone + PartialEq + 'static>(
    e: &KeyboardEvent,
    handle: UseTableHandle<TRow>,
    kb_id: &str,
    row_index: usize,
) {
    let key = e.key();
    if chevron_keydown_stops(&key) {
        e.stop_propagation();
        return;
    }
    let m = e.modifiers();
    let ctrl = m.contains(Modifiers::CONTROL) || m.contains(Modifiers::META);
    if !chevron_keydown_refocuses(&key, ctrl) {
        return;
    }
    let arrow = matches!(
        key,
        Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
    );
    if arrow && !ctrl && !m.contains(Modifiers::SHIFT) {
        let mut sig = handle.signal();
        let mut s = chorale_core::ensure_active_cell(&sig.peek());
        if let Some(ac) = s.active_cell {
            s.active_cell = Some(ActiveCell::new(row_index, ac.column_id));
            s.range_selection = vec![chorale_core::RangeSelection::single(
                row_index,
                ac.column_id,
            )];
            sig.set(s);
        }
    }
    refocus_keyboard_container(kb_id);
}

/// Move focus to the first focusable control inside grouped row `row_idx` of
/// this table (the group checkbox, the chevron or host content).
fn focus_header_controls(scroll_id: &str, row_idx: usize) {
    dioxus::document::eval(&focus_header_controls_js(scroll_id, row_idx));
}

fn focus_header_controls_js(scroll_id: &str, row_idx: usize) -> String {
    let sel = body_row_selector(row_idx);
    format!(
        "var sc=document.getElementById('{scroll_id}');\
         var row=sc?sc.querySelector('{sel}'):null;\
         var c=row?row.querySelector('input:not([disabled]),button:not([disabled]),select:not([disabled]),\
         textarea:not([disabled]),a[href],[tabindex]:not([tabindex=\"-1\"])'):null;\
         if(c)c.focus();"
    )
}

/// Read-only description of one group header, passed to the host's
/// `group_header_renderer` callback on [`Table`].
///
/// `#[non_exhaustive]`: fields may be added in a minor release, so hosts read
/// it and never construct it.
#[derive(Clone, PartialEq, Debug)]
#[non_exhaustive]
pub struct GroupHeaderContext {
    /// Opaque key of the group; pass to `UseTableHandle::toggle_group`.
    pub key: GroupKey,
    /// Display label of the group (the grouped value, formatted).
    pub label: String,
    /// Nesting depth, 0 for the outermost group.
    pub depth: usize,
    /// Number of rows in the group.
    pub row_count: usize,
    /// Whether the group is currently collapsed.
    pub is_collapsed: bool,
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
    row_class: &RowClass<TRow>,
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
    // Host renderer for group-header content and whether the grouped view is
    // virtualized (fixed header row height).
    group_header_renderer: Option<&Callback<GroupHeaderContext, Element>>,
    is_virtualized_grouped: bool,
    // `aria-level` of a data row: one below the deepest group header.
    leaf_level: usize,
    // Rows in the `<thead>` (1, or 2 with the filter row): body row `i` has
    // `aria-rowindex` `thead_rows + i + 1`.
    thead_rows: usize,
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
            row_index,
            active_cell.is_some_and(|ac| ac.row_idx == row_index),
            kb_id,
            thead_rows + row_index + 1,
            visible_cols,
            cell_renderers,
            effective_col_count,
            selection_enabled,
            handle,
            group_header_class,
            group_header_renderer.map(|r| (r, is_virtualized_grouped, row_height, kb_id)),
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
                row_class,
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
                Some((leaf_level, thead_rows + row_index + 1)),
            )
        }
        _ => rsx! {},
    }
}

/// Short prefix shown before a column's aggregate value in the group header,
/// hinting at which aggregator produced it. `AggregatorKind` is
/// `#[non_exhaustive]`, so the wildcard covers `Custom` and future variants.
fn aggregator_prefix<TRow>(kind: Option<&AggregatorKind<TRow>>) -> &'static str {
    match kind {
        Some(AggregatorKind::Sum) => "\u{3a3}", // Σ
        Some(AggregatorKind::Average) => "avg",
        Some(AggregatorKind::Count) => "count",
        Some(AggregatorKind::Min) => "min",
        Some(AggregatorKind::Max) => "max",
        _ => "",
    }
}

/// Render a single group-header `<tr>`.
///
/// Clicking the row calls `toggle_group` on the handle. With a host renderer
/// (`host`), only the chevron button toggles and the row has no click handler.
/// Depth is expressed as left-padding on the first cell (8px per level).
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
fn group_header_tr<TRow: Clone + PartialEq + 'static>(
    key: GroupKey,
    label: String,
    depth: usize,
    row_count: usize,
    is_collapsed: bool,
    aggregates: Vec<Option<CellValue>>,
    // Index of this row in the grouped view, and whether the keyboard cursor
    // is on it.
    row_index: usize,
    is_active: bool,
    // DOM id of the table's keyboard container.
    kb_id: &str,
    // One-based `aria-rowindex` of this row in the whole treegrid.
    aria_rowindex: usize,
    visible_cols: &[ColumnDef<TRow>],
    cell_renderers: &CellRenderers,
    col_count: usize,
    selection_enabled: bool,
    handle: UseTableHandle<TRow>,
    extra_class: &str,
    // `(renderer, virtualized, row_height, kb_id)`; `None` = default 0.2.3 markup.
    // `kb_id` is the DOM id of the table's keyboard container.
    host: Option<(&Callback<GroupHeaderContext, Element>, bool, f64, &str)>,
) -> Element {
    let indent = depth * 16;
    let toggle_icon = if is_collapsed { "\u{25b6}" } else { "\u{25bc}" };
    // Treegrid row state (issue #51), on both the default and the host header.
    let aria_level = depth + 1;
    let expanded = if is_collapsed { "false" } else { "true" };
    let extra_class = extra_class.to_owned();
    // Per-group select-all (#31): tri-state checkbox in the header's selection cell. The key
    // is moved into the collapse onclick below, so clone the pieces the checkbox needs first.
    let group_sel = if selection_enabled {
        handle.group_selection_state(&key)
    } else {
        chorale_core::GroupSelectionState::None
    };
    let group_all = matches!(group_sel, chorale_core::GroupSelectionState::All);
    let group_partial = matches!(group_sel, chorale_core::GroupSelectionState::Partial);
    let key_for_select = key.clone();
    // A DOM id to set the native `indeterminate` property (not an HTML attribute) via eval;
    // sanitized so the opaque GroupKey is a safe id/selector.
    let cb_id = format!(
        "chorale-grp-cb-{}",
        key.to_string()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    let cb_id_mount = cb_id.clone();
    let kb_id_cb = kb_id.to_owned();
    let row_dom_id = grouped_row_dom_id(kb_id, row_index);
    // Per-column aggregate summary. Core computes each group's aggregates (one
    // per column, in effective column order); render them here for any column
    // that declares an aggregator. The value is formatted through the SAME
    // renderer / RenderKind the data cells use, so a summed Salary shows as
    // "$148,234,567" exactly like the cells below.
    let agg_summary: Vec<Element> = visible_cols
        .iter()
        .zip(aggregates.iter())
        .filter_map(|(col, agg)| {
            let value = agg.as_ref()?;
            let prefix = aggregator_prefix(col.aggregator.as_ref());
            let header = col.header.clone();
            let value_el = if let Some(renderer) = cell_renderers.get(col.id) {
                renderer(value)
            } else {
                let text = cell_text(value, &col.render_kind);
                rsx! { "{text}" }
            };
            Some(rsx! {
                span {
                    style: "margin-left: 1.25rem; font-weight: 400; font-size: 0.8125rem; \
                            color: var(--chorale-text-muted, #6b7280);",
                    "{prefix} {header}: "
                }
                span { style: "font-weight: 600; font-size: 0.8125rem;", {value_el} }
            })
        })
        .collect();
    if let Some((renderer, virtualized, row_height, kb_id)) = host {
        let host_el = renderer.call(GroupHeaderContext {
            key: key.clone(),
            label: label.clone(),
            depth,
            row_count,
            is_collapsed,
        });
        let tr_style = if virtualized {
            format!(
                "background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; \
                 height: {row_height}px;"
            )
        } else {
            "background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600;".to_owned()
        };
        // Virtualized: the content span clips so host content cannot grow the row
        // (the scroll math assumes one fixed row height).
        let content_style = if virtualized {
            format!(
                "display: inline-block; vertical-align: middle; max-width: calc(100% - 1.5rem); \
                 max-height: {row_height}px; overflow: hidden; white-space: nowrap;"
            )
        } else {
            "display: inline-block; vertical-align: middle;".to_owned()
        };
        // `height` on a <tr> is only a minimum: pin the cell too, as data cells do.
        let fixed_td = if virtualized {
            format!(
                "height: {row_height}px; box-sizing: border-box; white-space: nowrap; \
                 padding-block: 0;"
            )
        } else {
            String::new()
        };
        let toggle_key = key.clone();
        // Accessible names: the chevron falls back to "Group" for an empty label;
        // the group checkbox is named after the group.
        let chevron_label = if label.is_empty() {
            "Group"
        } else {
            label.as_str()
        }
        .to_owned();
        let checkbox_label = format!("Select group {label}");
        // DOM id so the chevron can focus itself on click (Safari and macOS
        // Firefox do not focus a button on click).
        let tg_id = group_toggle_dom_id(kb_id, &key.to_string());
        let tg_id_click = tg_id.clone();
        let kb_id_key = kb_id.to_owned();
        return rsx! {
            tr {
                key: "grp-{key}",
                class: "{extra_class}",
                style: "{tr_style}",
                "aria-level": "{aria_level}",
                "aria-expanded": "{expanded}",
                "data-chorale-group-index": "{row_index}",
                "data-chorale-active-row": "{is_active}",
                "aria-rowindex": "{aria_rowindex}",
                id: "{row_dom_id}",
                if selection_enabled {
                    td {
                        style: "padding: 0.25rem 0.5rem; width: 2.5rem; text-align: center;",
                        input {
                            id: "{cb_id}",
                            r#type: "checkbox",
                            checked: group_all,
                            aria_label: "{checkbox_label}",
                            // Like the chevron (issue #51): Space, Enter and Tab stay
                            // here; arrows return to the grid and act on this header.
                            onkeydown: move |e: KeyboardEvent| {
                                header_control_keydown(&e, handle, &kb_id_cb, row_index);
                            },
                            onmounted: move |_| {
                                if group_partial {
                                    let _ = dioxus::document::eval(&format!(
                                        "var e=document.getElementById('{cb_id_mount}');if(e)e.indeterminate=true;"
                                    ));
                                }
                            },
                            onchange: move |_| { handle.toggle_select_group(key_for_select.clone()); },
                        }
                    }
                }
                td {
                    colspan: "{col_count - usize::from(selection_enabled)}",
                    style: "padding: 0.4rem 1rem 0.4rem {indent}px; \
                            border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem; {fixed_td}",
                    button {
                        id: "{tg_id}",
                        class: "chorale-group-toggle",
                        r#type: "button",
                        aria_expanded: "{expanded}",
                        aria_label: "{chevron_label}",
                        style: "margin-right: 0.5rem; padding: 0; border: 0; background: transparent; \
                                cursor: pointer; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2); \
                                vertical-align: middle; min-width: 24px; min-height: 24px;",
                        // Enter/Space must reach the button's own click and Tab must move
                        // focus natively, not hit the grid's handler. Other keys (arrows,
                        // Ctrl+C / Ctrl+A) still reach the grid, so first move focus to the
                        // keyboard container: in Virtualized mode the header can scroll out
                        // and unmount, which would drop focus to <body> and kill the keys.
                        // An arrow also puts the cursor on this header row first, so the
                        // grid acts on this header (issue #51).
                        onkeydown: move |e: KeyboardEvent| {
                            header_control_keydown(&e, handle, &kb_id_key, row_index);
                        },
                        onclick: move |_| {
                            // Focus the button explicitly: Safari and macOS Firefox do not
                            // focus it on click, and the grid's click handler would then
                            // move focus to the container.
                            dioxus::document::eval(&focus_element_js(&tg_id_click));
                            handle.toggle_group(toggle_key.clone());
                        },
                        "{toggle_icon}"
                    }
                    span {
                        class: "chorale-group-header-content",
                        style: "{content_style}",
                        onkeydown: move |e: KeyboardEvent| { e.stop_propagation(); },
                        {host_el}
                    }
                }
            }
        };
    }
    rsx! {
        tr {
            // Stable identity key (see data_tr): group headers key on their GroupKey so a
            // collapse/expand moves DOM nodes by identity instead of patching a header `tr`
            // against a data `tr` at a shifted position.
            key: "grp-{key}",
            class: "{extra_class}",
            style: "background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;",
            // Treegrid state and the keyboard cursor (issue #51). The cursor
            // outline is a stylesheet rule keyed on `data-chorale-active-row`:
            // Dioxus 0.7 drops some inline-style changes on a <tr> (see data_tr).
            // The row index is `data-chorale-group-index`, not
            // `data-chorale-index`: the VIRT-2 height loop measures every
            // `tr[data-chorale-index]` and would shift scrollTop for a header
            // shorter than row_height.
            "aria-level": "{aria_level}",
            "aria-expanded": "{expanded}",
            "data-chorale-group-index": "{row_index}",
            "data-chorale-active-row": "{is_active}",
            "aria-rowindex": "{aria_rowindex}",
            id: "{row_dom_id}",
            onclick: move |_| { handle.toggle_group(key.clone()); },
            if selection_enabled {
                td {
                    style: "padding: 0.25rem 0.5rem; width: 2.5rem; text-align: center;",
                    input {
                        id: "{cb_id}",
                        r#type: "checkbox",
                        checked: group_all,
                        // `indeterminate` is a JS property, not an HTML attribute, so set it
                        // after mount when the group is partially selected.
                        onmounted: move |_| {
                            if group_partial {
                                let _ = dioxus::document::eval(&format!(
                                    "var e=document.getElementById('{cb_id_mount}');if(e)e.indeterminate=true;"
                                ));
                            }
                        },
                        // Don't let the checkbox click bubble to the row's collapse toggle.
                        onclick: move |e| { e.stop_propagation(); },
                        // Tab from the header row can focus this checkbox (issue #51).
                        // Like the chevron: Space, Enter and Tab stay with it; arrows
                        // return to the grid and act on this header.
                        onkeydown: move |e: KeyboardEvent| {
                            header_control_keydown(&e, handle, &kb_id_cb, row_index);
                        },
                        onchange: move |_| { handle.toggle_select_group(key_for_select.clone()); },
                    }
                }
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
                {agg_summary.into_iter()}
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
/// DOM id of a group header's chevron button. Scoped by the table-unique `kb_id`
/// so two tables (or a nested sub-table) with the same group keys never share an
/// id, and suffixed with an FNV-1a hash of the raw key so keys that sanitize alike
/// (`"a b"`, `"a_b"`) stay distinct. ASCII alphanumeric, `-` and `_` only.
fn group_toggle_dom_id(kb_id: &str, key: &str) -> String {
    let sanitized: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{kb_id}-tg-{sanitized}-{hash:x}")
}

fn refocus_keyboard_container(kb_id: &str) {
    dioxus::document::eval(&format!(
        "var el=document.getElementById('{kb_id}');if(el)el.focus();"
    ));
}

/// Move keyboard focus INTO the nested sub-table of an expanded row (issue #17
/// part 2). Finds the parent's data row by visible index, takes its detail
/// panel sibling, and focuses the first `tabindex=0` container inside it (the
/// child table's keyboard container). The `:scope > table > tbody > tr` chain
/// keeps the lookup scoped to this table's own rows, not a nested child's.
fn focus_child_subtable(scroll_id: &str, row_idx: usize) {
    dioxus::document::eval(&format!(
        "var sc=document.getElementById('{scroll_id}');\
         if(sc){{var row=sc.querySelector(':scope > table > tbody > tr[data-chorale-index=\"{row_idx}\"]');\
         if(row){{var p=row.nextElementSibling;\
         var kb=p?p.querySelector('[tabindex=\"0\"]'):null;\
         if(kb)kb.focus();}}}}"
    ));
}

/// Move keyboard focus OUT of a nested sub-table back to its parent table's
/// keyboard container (issue #17 part 2). Walks up the DOM from this table's
/// own container to the nearest ancestor chorale keyboard container (id ending
/// `-kb`) and focuses it. No-op for a top-level table (no such ancestor).
fn focus_parent_container(kb_id: &str) {
    dioxus::document::eval(&format!(
        "var self=document.getElementById('{kb_id}');\
         if(self){{var p=self.parentElement;\
         while(p){{if(p.id&&p.id.slice(-3)==='-kb'&&p.getAttribute('tabindex')==='0'){{p.focus();break;}}\
         p=p.parentElement;}}}}"
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
            key: "{col.id}",
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
    // A grouped table's cell: clicks keep the grouped row index unclamped.
    grouped: bool,
    // DOM id when this cell holds the cursor in a grouped table.
    cell_dom_id: Option<String>,
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
            // Stable per-column key so reorder moves cells by identity rather
            // than diffing styles in place (keeps frozen-cell backgrounds).
            key: "{col.id}",
            style: "{style}",
            id: cell_dom_id,
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
                // Grouped: the row index counts header rows and can exceed the
                // ungrouped row count the core transitions clamp to.
                let new_s = if grouped {
                    grouped_click_selection(&sig_w.peek(), row_index, col_id, ctrl, shift)
                } else if ctrl {
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

/// Inline style for a badge of the given color key. Built-in keys: `green`, `yellow`, `red`,
/// `gray`, `blue`, `purple`, `orange`. Each resolves to a CSS-variable pair the consumer can
/// override: `--chorale-badge-<color>-bg` / `--chorale-badge-<color>-text`. ANY other
/// (non-empty) key is a custom-color escape hatch: it resolves to
/// `--chorale-badge-<key>-bg/-text` and falls back to the neutral default when those are
/// undefined — so consumers can extend the palette via CSS without forking.
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
        "blue" => (
            "var(--chorale-badge-blue-bg, #dbeafe)",
            "var(--chorale-badge-blue-text, #1e40af)",
        ),
        "purple" => (
            "var(--chorale-badge-purple-bg, #ede9fe)",
            "var(--chorale-badge-purple-text, #5b21b6)",
        ),
        "orange" => (
            "var(--chorale-badge-orange-bg, #ffedd5)",
            "var(--chorale-badge-orange-text, #9a3412)",
        ),
        // Any other key resolves to consumer-defined `--chorale-badge-<key>-bg/-text`
        // variables, falling back to the neutral default when those are undefined. This is
        // the custom-color escape hatch: a consumer can add palette entries via CSS without
        // forking, and an unknown/unstyled key degrades to gray rather than breaking.
        other if !other.is_empty() => {
            return format!(
                "display:inline-block;padding:0.125rem 0.5rem;border-radius:9999px;\
                 background:var(--chorale-badge-{other}-bg, var(--chorale-badge-default-bg, #e5e7eb));\
                 color:var(--chorale-badge-{other}-text, var(--chorale-badge-default-text, #1f2937));\
                 font-size:0.75rem;font-weight:500;"
            );
        }
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
    use super::GroupHeaderContext;
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

    // ---- grouped_fill_targets (EM review A.8) -------------------------------

    #[test]
    fn grouped_fill_skips_headers_and_reports_grouped_rows() {
        // Group "a": rows 0 (header), 1, 2; group "b": rows 3 (header), 4.
        let rows: Vec<(RowId, R)> = [("a", 1), ("a", 2), ("b", 3)]
            .into_iter()
            .map(|(n, v)| {
                (
                    RowId::new(),
                    R {
                        name: n.into(),
                        score: v,
                    },
                )
            })
            .collect();
        let cols = vec![
            ColumnDef::new(ColumnId("name"), "Name", |r: &R| {
                CellValue::Text(r.name.clone())
            }),
            ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
                CellValue::Integer(r.score)
            }),
        ];
        let mut s = TableState::new(rows, cols);
        s.grouping = vec![ColumnId("name")];
        let view = chorale_core::visible_grouped_view(&s);
        assert_eq!(view.len(), 5);
        // Fill the score of row 1 down to row 4: targets rows 2 and 4, not the
        // header at 3.
        let src = chorale_core::RangeSelection::single(1, ColumnId("score"));
        let writes = super::grouped_fill_targets(&s, &view, &src, 4, ColumnId("score"));
        let rows_written: Vec<usize> = writes.iter().map(|(r, _, _)| *r).collect();
        assert_eq!(rows_written, vec![2, 4], "{writes:?}");
        assert!(writes
            .iter()
            .all(|(_, c, v)| *c == ColumnId("score") && *v == CellValue::Integer(1)));
        // A header as the source fills nothing.
        let src = chorale_core::RangeSelection::single(3, ColumnId("score"));
        assert!(super::grouped_fill_targets(&s, &view, &src, 4, ColumnId("score")).is_empty());
    }

    // ---- grouped_move_target --------------------------------------------------

    #[test]
    fn grouped_page_move_without_active_cell_pages_from_row_zero() {
        // Same as core move_active_cell_page: start at row 0, then page.
        let cols = [ColumnId("a"), ColumnId("b")];
        let down = super::grouped_move_target(
            None,
            100,
            &cols,
            super::GroupedMove::Page(chorale_core::NavDirection::Down, 50),
        );
        assert_eq!(down, Some(chorale_core::ActiveCell::new(50, ColumnId("a"))));
        let up = super::grouped_move_target(
            None,
            100,
            &cols,
            super::GroupedMove::Page(chorale_core::NavDirection::Up, 50),
        );
        assert_eq!(up, Some(chorale_core::ActiveCell::new(0, ColumnId("a"))));
        let short = super::grouped_move_target(
            None,
            10,
            &cols,
            super::GroupedMove::Page(chorale_core::NavDirection::Down, 50),
        );
        assert_eq!(short, Some(chorale_core::ActiveCell::new(9, ColumnId("a"))));
    }

    // ---- reveal_scroll_top (measured geometry, issue #51) --------------------

    fn geom(top: f64, height: f64, scroll_top: f64) -> super::RowGeometry {
        // A 500px container under a 41px sticky header.
        super::RowGeometry {
            top,
            height,
            scroll_top,
            client_height: 500.0,
            cover: 41.0,
        }
    }

    #[test]
    fn reveal_measured_none_when_fully_visible() {
        // Band [scroll_top + cover, scroll_top + client_height] = [141, 600].
        assert_eq!(super::reveal_scroll_top(geom(141.0, 40.0, 100.0)), None);
        assert_eq!(super::reveal_scroll_top(geom(560.0, 40.0, 100.0)), None);
    }

    #[test]
    fn reveal_measured_scrolls_down_to_row_bottom() {
        // Row 561..601 straddles the bottom edge at 600 by 1px.
        assert_eq!(
            super::reveal_scroll_top(geom(561.0, 40.0, 100.0)),
            Some(101.0)
        );
        // Header rows shorter than row_height: a 37px row at 1000.
        assert_eq!(
            super::reveal_scroll_top(geom(1000.0, 37.0, 0.0)),
            Some(537.0)
        );
    }

    #[test]
    fn reveal_measured_scrolls_up_below_sticky_header() {
        // Row top at 120 is under the sticky header (band starts at 141).
        assert_eq!(
            super::reveal_scroll_top(geom(120.0, 40.0, 100.0)),
            Some(79.0)
        );
        // Never negative.
        assert_eq!(super::reveal_scroll_top(geom(20.0, 40.0, 10.0)), Some(0.0));
    }

    #[test]
    fn reveal_measured_without_sticky_header_uses_full_height() {
        let g = super::RowGeometry {
            cover: 0.0,
            ..geom(95.0, 40.0, 100.0)
        };
        assert_eq!(super::reveal_scroll_top(g), Some(95.0));
        let g = super::RowGeometry {
            cover: 0.0,
            ..geom(100.0, 40.0, 100.0)
        };
        assert_eq!(super::reveal_scroll_top(g), None);
    }

    #[test]
    fn reveal_measured_row_taller_than_band_aligns_top() {
        // A 600px row cannot fit in the 459px band: show its top.
        assert_eq!(
            super::reveal_scroll_top(geom(900.0, 600.0, 0.0)),
            Some(859.0)
        );
        // Already showing its top: stay.
        assert_eq!(super::reveal_scroll_top(geom(900.0, 600.0, 859.0)), None);
    }

    #[test]
    fn reveal_measured_ignores_subpixel_overlap() {
        // Fractional rects after a scroll write must not cause a re-scroll.
        assert_eq!(super::reveal_scroll_top(geom(560.3, 40.0, 100.0)), None);
        assert_eq!(super::reveal_scroll_top(geom(140.7, 40.0, 100.0)), None);
    }

    // ---- scroll_top_to_reveal_row -------------------------------------------

    /// Row fully inside the viewport → no scroll adjustment.
    #[test]
    fn reveal_row_returns_none_when_row_already_visible() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [100, 400): row 5 spans [200, 240) — fully visible.
        let r = super::scroll_top_to_reveal_row(5, 100.0, 300.0, 40.0, &heights, false, 0.0);
        assert_eq!(r, None);
    }

    /// Row top above `scroll_top` → scroll up so the row top hits the viewport top.
    #[test]
    fn reveal_row_scrolls_up_to_row_top() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [400, 700): row 3 spans [120, 160) — above.
        let r = super::scroll_top_to_reveal_row(3, 400.0, 300.0, 40.0, &heights, false, 0.0);
        assert_eq!(r, Some(120.0));
    }

    /// Row bottom below the viewport → scroll down so the row bottom hits
    /// the viewport bottom.
    #[test]
    fn reveal_row_scrolls_down_to_row_bottom_minus_viewport() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [0, 300): row 10 spans [400, 440) — below.
        // Expected: 440 - 300 = 140.
        let r = super::scroll_top_to_reveal_row(10, 0.0, 300.0, 40.0, &heights, false, 0.0);
        assert_eq!(r, Some(140.0));
    }

    /// A sticky header shrinks the usable row viewport by `header_h`, so the
    /// down-scroll must fire one header EARLIER (when the row is the last
    /// fully-visible one) and target a scroll offset that clears the header.
    #[test]
    fn reveal_row_accounts_for_sticky_header_height() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [0, 300): row 6 spans [240, 280). With no header it is
        // fully visible (280 <= 300) → None.
        assert_eq!(
            super::scroll_top_to_reveal_row(6, 0.0, 300.0, 40.0, &heights, false, 0.0),
            None
        );
        // With a 40px sticky header the usable bottom is 260, so row 6's
        // bottom (280) is clipped → scroll to 280 + 40 - 300 = 20.
        assert_eq!(
            super::scroll_top_to_reveal_row(6, 0.0, 300.0, 40.0, &heights, false, 40.0),
            Some(20.0)
        );
    }

    /// Row partially clipped at the bottom edge also triggers a scroll.
    #[test]
    fn reveal_row_scrolls_when_row_straddles_bottom_edge() {
        let heights: HashMap<usize, f64> = HashMap::new();
        // viewport [0, 300): row 7 spans [280, 320) — straddles the bottom.
        let r = super::scroll_top_to_reveal_row(7, 0.0, 300.0, 40.0, &heights, false, 0.0);
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
        let r = super::scroll_top_to_reveal_row(3, 400.0, 300.0, 40.0, &heights, true, 0.0);
        assert_eq!(r, Some(200.0));
    }

    /// Row 0 above the viewport scrolls to exactly 0 (never negative).
    #[test]
    fn reveal_row_zero_clamps_at_top() {
        let heights: HashMap<usize, f64> = HashMap::new();
        let r = super::scroll_top_to_reveal_row(0, 250.0, 300.0, 40.0, &heights, false, 0.0);
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

    #[test]
    fn badge_style_supports_blue_purple_orange() {
        for color in ["blue", "purple", "orange"] {
            let s = super::badge_style(color);
            assert!(
                s.contains(&format!("var(--chorale-badge-{color}-bg")),
                "{color} should resolve to its own bg var"
            );
            assert!(
                s.contains(&format!("var(--chorale-badge-{color}-text")),
                "{color} should resolve to its own text var"
            );
        }
    }

    #[test]
    fn badge_style_custom_key_uses_escape_hatch_var_then_default() {
        // A consumer-defined key resolves to its own var, with the neutral default as the
        // nested fallback when the consumer hasn't defined it.
        let s = super::badge_style("brand");
        assert!(s.contains("var(--chorale-badge-brand-bg, var(--chorale-badge-default-bg"));
        assert!(s.contains("var(--chorale-badge-brand-text, var(--chorale-badge-default-text"));
    }

    // ---- row_class (#32) ---------------------------------------------------

    #[test]
    fn row_class_applies_predicate_per_row() {
        let rc: super::RowClass<i64> =
            super::RowClass::new(|n: &i64| (*n > 5).then(|| "big".to_string()));
        assert_eq!(rc.class_for(&10), Some("big".to_string()));
        assert_eq!(rc.class_for(&3), None);
        // Default (no hook) never adds a class.
        let none: super::RowClass<i64> = super::RowClass::default();
        assert_eq!(none.class_for(&10), None);
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

    // ---- group_header_renderer (SSR) --------------------------------------

    use dioxus::prelude::*;

    /// Byte-for-byte `<tbody>` rendered by 0.2.3 for the SSR fixture below
    /// (two collapsed groups of two rows, no selection, no host renderer).
    /// Captured from the unmodified v0.2.3 source before the hook existed.
    const BASELINE_TBODY_0_2_3: &str = "<tbody><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25b6}</span>g0<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25b6}</span>g1<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr></tbody>";

    /// 0.2.3 `<tbody>` for the "selection" fixture variant (see `SsrCfg`).
    const BASELINE_SELECTION_0_2_3: &str = "<tbody><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td style=\"padding: 0.25rem 0.5rem; width: 2.5rem; text-align: center;\"><input id=\"chorale-grp-cb-g0\" type=\"checkbox\"/></td><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25b6}</span>g0<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td style=\"padding: 0.25rem 0.5rem; width: 2.5rem; text-align: center;\"><input id=\"chorale-grp-cb-g1\" type=\"checkbox\"/></td><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25b6}</span>g1<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr></tbody>";

    /// 0.2.3 `<tbody>` for the "nested" fixture variant (see `SsrCfg`).
    const BASELINE_NESTED_0_2_3: &str = "<tbody><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>g0<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 16px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>0<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(1)</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"2\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g0</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>0</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 16px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>2<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(1)</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"4\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g0</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>2</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>g1<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 16px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>1<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(1)</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"7\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g1</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>1</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 16px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>3<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(1)</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"9\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g1</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>3</span></td></tr></tbody>";

    /// 0.2.3 `<tbody>` for the "aggregates" fixture variant (see `SsrCfg`).
    const BASELINE_AGGREGATES_0_2_3: &str = "<tbody><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25b6}</span>g0<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span><span style=\"margin-left: 1.25rem; font-weight: 400; font-size: 0.8125rem; color: var(--chorale-text-muted, #6b7280);\">\u{3a3} Score: </span><span style=\"font-weight: 600; font-size: 0.8125rem;\">2</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25b6}</span>g1<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span><span style=\"margin-left: 1.25rem; font-weight: 400; font-size: 0.8125rem; color: var(--chorale-text-muted, #6b7280);\">\u{3a3} Score: </span><span style=\"font-weight: 600; font-size: 0.8125rem;\">4</span></td></tr></tbody>";

    /// 0.2.3 `<tbody>` for the "expanded" fixture variant (see `SsrCfg`).
    const BASELINE_EXPANDED_0_2_3: &str = "<tbody><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>g0<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"1\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g0</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>0</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"2\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g0</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>2</span></td></tr><tr class=\"chorale-group-header\" style=\"background: var(--chorale-group-header-bg, #f0f4ff); font-weight: 600; cursor: pointer;\"><td colspan=\"2\" style=\"padding: 0.4rem 1rem 0.4rem 0px; border-bottom: 1px solid var(--chorale-group-header-border, #dce4ff); font-size: 0.875rem;\"><span style=\"margin-right: 0.5rem; font-size: 0.75rem; color: var(--chorale-accent, #4a90e2);\">\u{25bc}</span>g1<span style=\"margin-left: 0.5rem; font-size: 0.75rem; font-weight: 400; color: var(--chorale-text-subtle, #888);\">(2)</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"4\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g1</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>1</span></td></tr><tr class=\"\" style=\"--chorale-separator-color: var(--chorale-button-disabled-bg, #f0f0f0);\" data-chorale-index=\"5\" data-chorale-row-selected=\"false\"><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>g1</span></td><td style=\"position: relative; padding: 0.5rem 1rem; height: 40px; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-sizing: border-box; box-shadow: inset 0 -1px 0 var(--chorale-button-disabled-bg, #f0f0f0); cursor: default;  \"><span>3</span></td></tr></tbody>";

    #[derive(Clone, PartialEq, Default)]
    #[allow(clippy::struct_excessive_bools)]
    struct SsrCfg {
        collapse_first: bool,
        virtualized: bool,
        host: bool,
        selection: bool,
        /// Group by name, then score (depth-1 headers).
        nested: bool,
        /// Sum aggregator on the score column.
        aggregates: bool,
        /// First row has an empty group label.
        empty_label: bool,
    }

    #[allow(clippy::needless_pass_by_value)]
    fn ssr_app(cfg: SsrCfg) -> Element {
        let table = crate::use_table(move || {
            let rows: Vec<(RowId, R)> = (0..4)
                .map(|i| {
                    (
                        RowId::new(),
                        R {
                            name: if cfg.empty_label {
                                String::new()
                            } else {
                                format!("g{}", i % 2)
                            },
                            score: i,
                        },
                    )
                })
                .collect();
            let score = ColumnDef::new(ColumnId("score"), "Score", |r: &R| {
                CellValue::Integer(r.score)
            });
            let score = if cfg.aggregates {
                score.aggregator(chorale_core::AggregatorKind::Sum)
            } else {
                score
            };
            let columns = vec![
                ColumnDef::new(ColumnId("name"), "Name", |r: &R| {
                    CellValue::Text(r.name.clone())
                }),
                score,
            ];
            let mut s = TableState::new(rows, columns);
            s.grouping = if cfg.nested {
                vec![ColumnId("name"), ColumnId("score")]
            } else {
                vec![ColumnId("name")]
            };
            if cfg.virtualized {
                s.grouped_pagination = chorale_core::GroupedPaginationMode::Virtualized;
                s.viewport_height = 400.0;
            }
            if cfg.collapse_first {
                for row in chorale_core::visible_grouped_view(&s) {
                    if let chorale_core::GroupedRow::Header { key, .. } = row {
                        s.collapsed_groups.insert(key);
                    }
                }
            }
            s
        });
        let renderer = cfg.host.then(|| {
            Callback::new(|ctx: GroupHeaderContext| {
                rsx! {
                    em {
                        class: "host-content",
                        "{ctx.label}|{ctx.depth}|{ctx.row_count}|{ctx.is_collapsed}"
                    }
                }
            })
        });
        rsx! { super::Table::<R> { handle: table, selection_enabled: cfg.selection, group_header_renderer: renderer } }
    }

    /// Render the fixture; returns the markup and the names of every event
    /// listener the first render attached.
    fn render_ssr(collapse_first: bool, virtualized: bool, host: bool) -> (String, Vec<String>) {
        render_ssr_cfg(SsrCfg {
            collapse_first,
            virtualized,
            host,
            ..SsrCfg::default()
        })
    }

    fn render_ssr_cfg(cfg: SsrCfg) -> (String, Vec<String>) {
        let mut dom = VirtualDom::new_with_props(ssr_app, cfg);
        let muts = dom.rebuild_to_vec();
        let listeners = muts
            .edits
            .iter()
            .filter_map(|m| match m {
                dioxus::dioxus_core::Mutation::NewEventListener { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        (dioxus_ssr::render(&dom), listeners)
    }

    fn tbody(html: &str) -> &str {
        let Some(start) = html.find("<tbody>") else {
            return "";
        };
        let Some(end) = html.find("</tbody>").map(|i| i + "</tbody>".len()) else {
            return "";
        };
        &html[start..end]
    }

    /// Remove from every `<tr>` opening tag exactly the attributes 0.2.4 adds
    /// for treegrid keyboard navigation (issue #51): `aria-level`,
    /// `aria-expanded`, `data-chorale-active-row`, `data-chorale-group-index`,
    /// `aria-rowindex` and `id` (no 0.2.3 `<tr>` had an id). What remains must
    /// equal the 0.2.3 markup byte for byte.
    fn strip_treegrid_attrs(html: &str) -> String {
        fn drop_attr(tag: &mut String, name: &str) {
            let needle = format!(" {name}=\"");
            if let Some(at) = tag.find(&needle) {
                let value_start = at + needle.len();
                if let Some(len) = tag[value_start..].find('"') {
                    tag.replace_range(at..=value_start + len, "");
                }
            }
        }
        let mut out = String::with_capacity(html.len());
        let mut rest = html;
        while let Some(at) = rest.find("<tr ") {
            out.push_str(&rest[..at]);
            let end = rest[at..].find('>').map_or(rest.len(), |e| at + e);
            let mut tag = rest[at..end].to_owned();
            drop_attr(&mut tag, "aria-level");
            drop_attr(&mut tag, "aria-expanded");
            drop_attr(&mut tag, "data-chorale-active-row");
            drop_attr(&mut tag, "data-chorale-group-index");
            drop_attr(&mut tag, "aria-rowindex");
            drop_attr(&mut tag, "id");
            out.push_str(&tag);
            rest = &rest[end..];
        }
        out.push_str(rest);
        out
    }

    /// The 0.2.4 additions on the default header, in render order, up to the
    /// row id (whose value starts with the table-unique keyboard-container id).
    /// One `<thead>` row, so body row `index` has `aria-rowindex` `index + 2`.
    fn header_tree_attrs(level: usize, expanded: bool, index: usize) -> String {
        format!(
            " aria-level=\"{level}\" aria-expanded=\"{expanded}\" \
             data-chorale-group-index=\"{index}\" data-chorale-active-row=\"false\" \
             aria-rowindex=\"{}\" id=\"chorale-scroll-",
            index + 2
        )
    }

    /// Every body `<tr>` carries exactly one row index: `data-chorale-group-index`
    /// on a header (`aria-expanded` present), `data-chorale-index` on a data row,
    /// neither on a spacer (which is `aria-hidden`). Returns (headers, data rows).
    fn assert_row_index_attrs(body: &str) -> (usize, usize) {
        let (mut headers, mut data) = (0, 0);
        for tag in body
            .split("<tr")
            .skip(1)
            .map(|t| t.split('>').next().unwrap_or(""))
        {
            let group = tag.contains(" data-chorale-group-index=");
            let index = tag.contains(" data-chorale-index=");
            match (group, index) {
                (true, false) => {
                    assert!(tag.contains(" aria-expanded="), "{tag}");
                    headers += 1;
                }
                (false, true) => {
                    assert!(!tag.contains(" aria-expanded="), "{tag}");
                    data += 1;
                }
                (false, false) => assert!(tag.contains(" aria-hidden=\"true\""), "{tag}"),
                (true, true) => panic!("both row indexes: {tag}"),
            }
        }
        (headers, data)
    }

    // Changed on purpose in 0.2.4 (issue #51): header rows gained treegrid
    // attributes and a row index so the keyboard cursor can stop on them.
    // Everything else is still the 0.2.3 markup, byte for byte.
    #[test]
    fn group_header_default_markup_matches_0_2_3() {
        for virtualized in [false, true] {
            let (html, _) = render_ssr(true, virtualized, false);
            let body = tbody(&html);
            assert_eq!(
                strip_treegrid_attrs(body),
                BASELINE_TBODY_0_2_3,
                "virtualized={virtualized}"
            );
            // Both collapsed headers carry exactly the new attributes.
            assert_eq!(
                assert_row_index_attrs(body),
                (2, 0),
                "virtualized={virtualized}"
            );
            assert!(
                body.contains(&header_tree_attrs(1, false, 0)),
                "virtualized={virtualized}: {body}"
            );
            assert!(
                body.contains(&header_tree_attrs(1, false, 1)),
                "virtualized={virtualized}: {body}"
            );
        }
    }

    #[test]
    fn strip_treegrid_attrs_only_removes_the_new_attributes() {
        let header = "<tr class=\"chorale-group-header\" style=\"x\" aria-level=\"1\" \
                      aria-expanded=\"true\" data-chorale-group-index=\"0\" \
                      data-chorale-active-row=\"false\" aria-rowindex=\"2\" \
                      id=\"t-kb-row-0\"><td>g</td></tr>";
        assert_eq!(
            strip_treegrid_attrs(header),
            "<tr class=\"chorale-group-header\" style=\"x\"><td>g</td></tr>"
        );
        // A data row keeps its 0.2.3 index; the treegrid attributes go, and an
        // id inside a cell is not touched.
        let data = "<tr class=\"\" data-chorale-index=\"2\" data-chorale-row-selected=\"false\" \
                    aria-level=\"2\" aria-rowindex=\"4\" id=\"t-kb-row-2\"><td>d</td></tr>";
        assert_eq!(
            strip_treegrid_attrs(data),
            "<tr class=\"\" data-chorale-index=\"2\" data-chorale-row-selected=\"false\"><td>d</td></tr>"
        );
    }

    #[test]
    fn group_header_default_markup_matches_0_2_3_variants() {
        let cases = [
            (
                "selection cell",
                SsrCfg {
                    collapse_first: true,
                    selection: true,
                    ..SsrCfg::default()
                },
                BASELINE_SELECTION_0_2_3,
            ),
            (
                "nested groups",
                SsrCfg {
                    nested: true,
                    ..SsrCfg::default()
                },
                BASELINE_NESTED_0_2_3,
            ),
            (
                "aggregates",
                SsrCfg {
                    collapse_first: true,
                    aggregates: true,
                    ..SsrCfg::default()
                },
                BASELINE_AGGREGATES_0_2_3,
            ),
            (
                "expanded with data rows",
                SsrCfg::default(),
                BASELINE_EXPANDED_0_2_3,
            ),
        ];
        // Changed on purpose in 0.2.4: see `group_header_default_markup_matches_0_2_3`.
        let mismatched: Vec<&str> = cases
            .into_iter()
            .filter(|(_, cfg, baseline)| {
                strip_treegrid_attrs(tbody(&render_ssr_cfg(cfg.clone()).0)) != *baseline
            })
            .map(|(name, _, _)| name)
            .collect();
        assert!(mismatched.is_empty(), "differs from 0.2.3: {mismatched:?}");
        // Nested: depth-1 headers are level 2 and leaf data rows level 3.
        let body = render_ssr_cfg(SsrCfg {
            nested: true,
            ..SsrCfg::default()
        })
        .0;
        let body = tbody(&body);
        assert!(body.contains(&header_tree_attrs(1, true, 0)), "{body}");
        assert!(body.contains(&header_tree_attrs(2, true, 1)), "{body}");
        assert!(
            body.contains(
                "data-chorale-index=\"2\" data-chorale-row-selected=\"false\" aria-level=\"3\" \
                 aria-rowindex=\"4\" id=\"chorale-scroll-"
            ),
            "{body}"
        );
        // Six headers (g0 with 0 and 2, g1 with 1 and 3) and four data rows,
        // one row index each.
        assert_eq!(assert_row_index_attrs(body), (6, 4));
    }

    #[test]
    fn group_header_host_accessible_names_and_target_size() {
        let (html, _) = render_ssr_cfg(SsrCfg {
            collapse_first: true,
            host: true,
            selection: true,
            ..SsrCfg::default()
        });
        let body = tbody(&html);
        assert!(body.contains("aria-label=\"g0\""), "{body}");
        assert!(body.contains("aria-label=\"Select group g0\""), "{body}");
        assert!(body.contains("aria-label=\"Select group g1\""), "{body}");
        assert_eq!(
            body.matches("min-width: 24px; min-height: 24px;").count(),
            2,
            "{body}"
        );
        // Empty label: the chevron falls back to "Group".
        let (html, _) = render_ssr_cfg(SsrCfg {
            collapse_first: true,
            host: true,
            empty_label: true,
            ..SsrCfg::default()
        });
        let body = tbody(&html);
        assert!(body.contains("aria-label=\"Group\""), "{body}");
        assert!(!body.contains("aria-label=\"\""), "{body}");
        // The default (None) markup carries none of these.
        let (html, _) = render_ssr_cfg(SsrCfg {
            collapse_first: true,
            selection: true,
            ..SsrCfg::default()
        });
        let body = tbody(&html);
        assert!(!body.contains("aria-label"), "{body}");
        assert!(!body.contains("min-width"), "{body}");
    }

    #[test]
    fn group_header_chevron_has_focus_id() {
        let (html, _) = render_ssr(true, false, true);
        let body = tbody(&html);
        // The id carries the table-unique keyboard-container id as its prefix.
        let Some(at) = body.find("-kb-tg-g0-") else {
            panic!("{body}");
        };
        let from = body[..at].rfind('"').unwrap() + 1;
        let tg_id = body[from..].split('"').next().unwrap().to_owned();
        assert!(tg_id.starts_with("chorale-"), "{tg_id}");
        let js = super::focus_element_js(&tg_id);
        assert!(js.contains(&format!("getElementById('{tg_id}')")), "{js}");
        assert!(js.contains(".focus()"), "{js}");
    }

    #[test]
    fn group_toggle_ids_are_table_scoped_and_collision_free() {
        let a = super::group_toggle_dom_id("t-1-kb", "a b");
        let b = super::group_toggle_dom_id("t-1-kb", "a_b");
        assert_ne!(a, b);
        assert_ne!(
            super::group_toggle_dom_id("t-1-kb", "g0"),
            super::group_toggle_dom_id("t-2-kb", "g0")
        );
        assert!(a.starts_with("t-1-kb-tg-a_b-"), "{a}");
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn group_header_host_content_gets_context_values() {
        let (html, _) = render_ssr(true, false, true);
        let body = tbody(&html);
        assert!(
            body.contains("class=\"chorale-group-header-content\""),
            "{body}"
        );
        assert!(body.contains("g0|0|2|true"), "{body}");
        assert!(body.contains("g1|0|2|true"), "{body}");
        // Replaced: no built-in label markup or count span.
        assert!(!body.contains("(2)"), "{body}");
        // Expanded groups report is_collapsed=false.
        let (html, _) = render_ssr(false, false, true);
        assert!(tbody(&html).contains("g0|0|2|false"));
    }

    #[test]
    fn group_header_host_has_toggle_button_and_no_row_toggle() {
        let (html, listeners) = render_ssr(true, false, true);
        let body = tbody(&html);
        assert_eq!(
            body.matches("class=\"chorale-group-toggle\"").count(),
            2,
            "{body}"
        );
        // Each chevron reports the state, and since 0.2.4 (issue #51) so does
        // each treegrid header row.
        assert_eq!(
            body.matches("class=\"chorale-group-toggle\" type=\"button\" aria-expanded=\"false\"")
                .count(),
            2,
            "{body}"
        );
        assert_eq!(body.matches("aria-expanded=\"false\"").count(), 4, "{body}");
        assert!(body.contains("class=\"chorale-group-header\""), "{body}");
        // The row toggle's pointer cursor is gone from the <tr>.
        assert!(!body.contains("cursor: pointer; font-weight"), "{body}");
        let tr_style_has_pointer = body.split("<tr ").skip(1).any(|tr| {
            tr.split('>')
                .next()
                .unwrap_or("")
                .contains("cursor: pointer")
        });
        assert!(!tr_style_has_pointer, "{body}");
        // One click listener per header (the button), same count as the
        // default's per-row listener: the row onclick was replaced, not added.
        let (_, default_listeners) = render_ssr(true, false, false);
        let clicks = |l: &[String]| l.iter().filter(|n| *n == "click").count();
        assert_eq!(clicks(&listeners), clicks(&default_listeners));
        let (html_exp, _) = render_ssr(false, false, true);
        assert!(tbody(&html_exp).contains("aria-expanded=\"true\""));
    }

    #[test]
    fn group_header_host_keydown_is_isolated() {
        let keydowns = |l: &[String]| l.iter().filter(|n| *n == "keydown").count();
        let (_, default_listeners) = render_ssr(true, false, false);
        let (_, host_listeners) = render_ssr(true, false, true);
        // Two headers, each with a keydown stopper on the wrapper span and on
        // the toggle button.
        assert_eq!(keydowns(&host_listeners), keydowns(&default_listeners) + 4);
    }

    #[test]
    fn group_header_host_checkbox_keydown_is_isolated() {
        let keydowns = |l: &[String]| l.iter().filter(|n| *n == "keydown").count();
        let cfg = |host| SsrCfg {
            collapse_first: true,
            host,
            selection: true,
            ..SsrCfg::default()
        };
        let (_, default_listeners) = render_ssr_cfg(cfg(false));
        let (_, host_listeners) = render_ssr_cfg(cfg(true));
        // Since 0.2.4 (issue #51) the built-in header's checkbox has a keydown
        // listener too (Tab from the header row can focus it): one per header.
        let (_, default_no_selection) = render_ssr(true, false, false);
        assert_eq!(
            keydowns(&default_listeners),
            keydowns(&default_no_selection) + 2
        );
        // Two headers, each: wrapper span + chevron on top of the checkbox.
        assert_eq!(keydowns(&host_listeners), keydowns(&default_listeners) + 4);
    }

    #[test]
    fn chevron_stops_only_enter_space_tab() {
        assert!(super::chevron_keydown_stops(&Key::Enter));
        assert!(super::chevron_keydown_stops(&Key::Tab));
        assert!(super::chevron_keydown_stops(&Key::Character(" ".into())));
        assert!(!super::chevron_keydown_stops(&Key::ArrowDown));
        assert!(!super::chevron_keydown_stops(&Key::Character("c".into())));
        assert!(!super::chevron_keydown_stops(&Key::Character("a".into())));
    }

    #[test]
    fn chevron_refocuses_grid_only_for_grid_keys() {
        assert!(super::chevron_keydown_refocuses(&Key::ArrowDown, false));
        assert!(super::chevron_keydown_refocuses(&Key::PageUp, false));
        assert!(super::chevron_keydown_refocuses(
            &Key::Character("a".into()),
            true
        ));
        // A bare modifier or plain letter must not steal focus (Shift+Tab).
        assert!(!super::chevron_keydown_refocuses(&Key::Shift, false));
        assert!(!super::chevron_keydown_refocuses(&Key::Control, true));
        assert!(!super::chevron_keydown_refocuses(
            &Key::Character("a".into()),
            false
        ));
        assert!(!super::chevron_keydown_refocuses(&Key::Tab, false));
    }

    #[test]
    fn sync_scroll_js_non_finite_writes_zero() {
        for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(super::sync_scroll_js("t-1", bad).contains("el.scrollTop = 0;"));
        }
    }

    #[test]
    fn group_header_height_style_only_when_virtualized() {
        let row_height = TableState::<R>::new(vec![], vec![]).row_height;
        let (virt, _) = render_ssr(true, true, true);
        assert!(
            tbody(&virt).contains(&format!("height: {row_height}px;")),
            "{}",
            tbody(&virt)
        );
        let body = tbody(&virt);
        assert!(body.contains(&format!(
            "height: {row_height}px; box-sizing: border-box; white-space: nowrap; padding-block: 0;"
        )));
        assert!(body.contains(&format!("max-height: {row_height}px; overflow: hidden;")));
        assert!(body.contains("max-width: calc(100% - 1.5rem);"));
        assert!(!body.contains("max-width: 90%"));
        let (flat, _) = render_ssr(true, false, true);
        // The chevron's 24px target size (`min-height`) is not a row height.
        let flat_body = tbody(&flat).replace("min-height:", "");
        assert!(!flat_body.contains("height:"), "{flat_body}");
        assert!(!tbody(&flat).contains("overflow: hidden"));
    }

    #[test]
    fn page_scroll_target_saved_on_mount_zero_on_page_change() {
        // Mount: restore the saved offset.
        assert_eq!(super::page_scroll_target(None, 0, 240.0), Some(240.0));
        // Later page change: literal 0 even when state still holds an old offset.
        assert_eq!(super::page_scroll_target(Some(0), 1, 240.0), Some(0.0));
        assert_eq!(super::page_scroll_target(Some(3), 2, 99.0), Some(0.0));
        // Same page: no write.
        assert_eq!(super::page_scroll_target(Some(1), 1, 240.0), None);
    }

    #[test]
    fn sync_scroll_js_writes_saved_offset() {
        let js = super::sync_scroll_js("t-1", 240.0);
        assert!(js.contains("getElementById('t-1')"), "{js}");
        assert!(js.contains("el.scrollTop = 240;"), "{js}");
        assert!(super::sync_scroll_js("t-1", 0.0).contains("el.scrollTop = 0;"));
    }
}
