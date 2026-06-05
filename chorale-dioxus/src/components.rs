//! Dioxus components for chorale tables.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chorale_core::{
    batch_record_row_heights, cancel_edit, commit_edit, frozen_left_columns, frozen_right_columns,
    next_editable_cell, prev_editable_cell, scrollable_columns, to_csv, visible_view,
    visible_window, visible_window_variable, Alignment, BadgeVariantMap, CellValue, ColumnDef,
    ColumnId, CommittedEdit, CurrencyCode, EditorKind, FilterKind, FilterValue, Labels,
    PaginationMode, RenderKind, RowId, SortAction, SortDirection, SortState, TableState,
    VirtualWindow,
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

/// The primary chorale Dioxus table component.
///
/// Renders column headers, an optional filter row, virtualized data rows,
/// pagination controls, and optional selection checkboxes. All features are
/// opt-in via props; the minimal form shows a read-only sorted table.
///
/// ## Props
///
/// | Prop | Type | Default | Effect |
/// |---|---|---|---|
/// | `handle` | `UseTableHandle<TRow>` | — | Required. Reactive handle from `use_table`. |
/// | `sort_enabled` | `bool` | `true` | Show sort-direction arrows and make sortable headers clickable. Set `false` to render headers as plain text without clearing existing sort state. |
/// | `filter_enabled` | `bool` | `false` | Show a filter input row below the column headers. Each column renders its `FilterKind` UI: text input, numeric range, date range, multi-select dropdown, or boolean radio group. |
/// | `selection_enabled` | `bool` | `false` | Show a checkbox column on the left. The header checkbox toggles selection for all visible rows on the current page. Read the selection via `handle.signal().read().selection`. |
/// | `cell_renderers` | `CellRenderers` | empty | Per-column custom renderers that override `RenderKind`. Pass `CellRenderers::new(map)` with a `HashMap<ColumnId, CellRenderer>`. |
/// | `column_toolbar` | `bool` | `false` | Show a column visibility toolbar above the table. Each column gets a toggle checkbox. |
/// | `csv_export` | `bool` | `false` | Show a "Download CSV" button above the table. Exports the full post-filter/post-sort dataset (not just the current page). |
/// | `resize_enabled` | `bool` | `false` | Show drag handles on column header borders. Dragging adjusts `column_widths` in the table state. |
/// | `variable_row_height` | `bool` | `false` | Enable variable-row-height virtualization (VIRT-2). When `true`, the component measures each rendered row's height after mount via a DOM eval and caches the result in `state.row_heights`. The `row_height` prop (or `state.row_height`) is used as the fallback for unmeasured rows. Requires a web target. |
/// | `validate_edit` | `ValidateEditFn` | no-op | Optional synchronous validator called before a cell edit is committed. Return `Ok(())` to allow, `Err(msg)` to show an inline error. |
/// | `on_commit_edit` | `Option<EventHandler<CommittedEdit<TRow>>>` | `None` | Fired after a successful commit. Receives the new raw value and a `PriorEdit` snapshot for rollback. |
/// | `selection_toolbar` | `Option<Element>` | `None` | Optional slot rendered above the table when `state.selection` is non-empty. Use for bulk-action bars. Wrapped in `div.chorale-selection-toolbar`. |
/// | `labels` | `Option<Labels>` | `None` | All user-visible strings (filter placeholder, pagination labels, CSV button, etc.). `None` uses English defaults. Override for i18n. |
/// | `column_reorder_enabled` | `bool` | `false` | Show drag handles on column headers. Drop fires `move_column` and triggers `on_column_order_change`. |
/// | `on_column_order_change` | `Option<EventHandler<Vec<ColumnId>>>` | `None` | Called with the new `column_order` vec after a successful column drag-and-drop. |
#[component]
pub fn Table<TRow: Clone + PartialEq + 'static>(
    handle: UseTableHandle<TRow>,
    #[props(default = true)] sort_enabled: bool,
    #[props(default = false)] filter_enabled: bool,
    #[props(default = false)] selection_enabled: bool,
    #[props(default)] cell_renderers: CellRenderers,
    #[props(default = false)] column_toolbar: bool,
    #[props(default = false)] csv_export: bool,
    #[props(default = false)] resize_enabled: bool,
    #[props(default = false)] variable_row_height: bool,
    #[props(default)] validate_edit: ValidateEditFn,
    on_commit_edit: Option<EventHandler<CommittedEdit<TRow>>>,
    #[props(default)] selection_toolbar: Option<Element>,
    #[props(default)] labels: Option<Labels>,
    #[props(default = false)] column_reorder_enabled: bool,
    on_column_order_change: Option<EventHandler<Vec<ColumnId>>>,
    /// CSS `z-index` applied to frozen column cells (header, filter row, and body).
    /// Raise if custom cell renderers use `z-index` internally.
    /// Default is `2` (above scrollable columns, which use no explicit z-index).
    #[props(default = 2)]
    frozen_column_z_index: i32,
    /// Distance from the scroll container bottom (px) at which to fire
    /// `load_more_rows` in `PaginationMode::InfiniteScroll`. Default is `200`.
    #[props(default = 200.0_f64)]
    infinite_scroll_threshold_px: f64,
) -> Element {
    let labels = labels.clone().unwrap_or_default();

    // drag_state: Some((col_id, start_x_px, start_width_px)) while a resize is active.
    let mut drag_state: Signal<Option<(ColumnId, f64, f64)>> = use_signal(|| None);
    // drag_col_id: column being dragged for column-reorder (None when not reordering).
    let drag_col_id: Signal<Option<ColumnId>> = use_signal(|| None);
    // In-cell editing state: current editor text and optional validation error.
    let mut editing_text: Signal<String> = use_signal(String::new);
    let mut edit_error: Signal<Option<String>> = use_signal(|| None);

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

    // PERF-1: Two-level memo to decouple the expensive pipeline from scroll.
    //
    // view_key tracks only the fields that affect visible_view output:
    // page, page_size, sort, filters, and row count. When scroll_top (or
    // viewport_height, row_height, column_widths, selection) changes, this
    // key runs but returns the same tuple → Dioxus PartialEq short-circuits
    // → the view memo does NOT re-run the filter+sort+paginate pipeline.
    //
    // At 1M rows this eliminates ~30 MB of allocation per scroll tick.
    // See docs/perf-2026-06-04-fine-grained-reactivity.md for rationale.
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
        )
    });
    // sig.peek() reads without subscribing this memo to sig directly;
    // the dependency flows through view_key only.
    let view = use_memo(move || {
        let _key = view_key.read();
        visible_view(&*sig.peek())
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

    // In-cell editing: reset editor text and error when the active cell changes.
    // Unconditional use_effect (Dioxus hook ordering rules); no-op when no edit target.
    {
        let edit_target_memo = use_memo(move || sig.read().editing);
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
            }
        });
    }

    // VIRT-2: variable-row-height measurement loop.
    // Always called (hooks must be unconditional); no-op when variable_row_height=false.
    // After each render, queries rendered rows by data-chorale-index attribute,
    // measures their heights via getBoundingClientRect, and dispatches a batch
    // state update if any measurement differs from the cached value by > 0.5px.
    // The threshold prevents convergence loops caused by sub-pixel float rounding.
    {
        let scroll_id_meas = scroll_id.clone();
        use_effect(move || {
            if !variable_row_height {
                return;
            }
            let cid = scroll_id_meas.clone();
            let mut sig2 = handle.signal();
            spawn(async move {
                let mut js = dioxus::document::eval(&format!(
                    r"const rs=document.querySelectorAll('#{cid} [data-chorale-index]');
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
                    let any_changed = measurements
                        .iter()
                        .any(|(k, v)| cur.row_heights.get(k).map_or(true, |h| (h - v).abs() > 0.5));
                    if any_changed {
                        let new_state = batch_record_row_heights(&cur, &measurements);
                        drop(cur);
                        sig2.set(new_state);
                    }
                }
            });
        });
    }

    let view_read = view.read();
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
                format!("position: sticky; left: {left_offset}px; z-index: {header_z};{divider}"),
            );
            sticky_body_css.insert(
                col.id,
                format!(
                    "position: sticky; left: {left_offset}px; z-index: {body_z}; background: #fff;{divider}"
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
                format!("position: sticky; right: {right_offset}px; z-index: {header_z};{divider}"),
            );
            sticky_body_css.insert(
                col.id,
                format!(
                    "position: sticky; right: {right_offset}px; z-index: {body_z}; background: #fff;{divider}"
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

    let (win, id_slice, row_slice) = compute_window_slice(&state, &view_read, variable_row_height);
    let total_pages = state.total_pages();
    let page_idx = state.page; // zero-based
    let total_rows = state.filtered_row_count();
    let is_infinite_scroll = state.pagination_mode == PaginationMode::InfiniteScroll;
    let has_more_rows = is_infinite_scroll && view_read.len() < total_rows;
    let col_count = visible_cols.len();
    let effective_col_count = col_count + usize::from(selection_enabled);
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
    let all_page_selected =
        !view_read.is_empty() && view_read.iter().all(|(id, _)| selection_set.contains(id));

    let page_buttons = page_button_range(page_idx, total_pages);
    let prev_disabled = page_idx == 0;
    let next_disabled = page_idx + 1 >= total_pages;
    let nav_btn = "padding:0.25rem 0.6rem;border:1px solid #ddd;border-radius:3px;\
                   font-size:0.875rem;cursor:pointer;background:white;color:#333;";
    let nav_btn_dis = "padding:0.25rem 0.6rem;border:1px solid #ddd;border-radius:3px;\
                       font-size:0.875rem;cursor:not-allowed;background:#f0f0f0;color:#aaa;";

    rsx! {
        div {
            style: "border: 1px solid #ddd; border-radius: 4px; overflow: hidden; \
                    user-select: none;",
            onmousemove: move |e| {
                if let Some((col_id, start_x, start_w)) = *drag_state.read() {
                    let delta = e.client_coordinates().x - start_x;
                    handle.set_column_width(col_id, (start_w + delta).max(40.0)).ok();
                }
            },
            onmouseup: move |_| { drag_state.set(None); },
            onmouseleave: move |_| { drag_state.set(None); },

            if column_toolbar {
                {column_visibility_toolbar(&all_col_defs, &col_visibility, handle, &labels)}
            }

            if !state.selection.is_empty() {
                if let Some(toolbar) = selection_toolbar {
                    div {
                        class: "chorale-selection-toolbar",
                        style: "padding: 0.5rem 1rem; border-bottom: 1px solid #ddd; \
                                background: #fffbeb;",
                        {toolbar}
                    }
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
                style: "overflow-y: auto; overflow-x: auto; overflow-anchor: none; \
                        height: {viewport_height}px;",
                onscroll: move |e| {
                    let st = e.scroll_top();
                    handle.set_scroll(st);
                    // Infinite scroll: trigger load_more_rows when within threshold of the bottom.
                    let sig_for_scroll = handle.signal();
                    let s = sig_for_scroll.read();
                    if s.pagination_mode == PaginationMode::InfiniteScroll {
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
                            style: "background: #f8f9fa;",
                            if selection_enabled {
                                {select_all_th(handle, all_page_selected)}
                            }
                            for col in &visible_cols {
                                {header_th(col, widths.get(&col.id).copied(), handle, sort_enabled, current_sort, resize_enabled, drag_state, column_reorder_enabled, drag_col_id, on_column_order_change, sticky_header_css.get(&col.id).map_or("", String::as_str))}
                            }
                        }
                        if filter_enabled {
                            tr {
                                style: "background: #fff;",
                                if selection_enabled {
                                    th {
                                        style: "padding: 0.25rem; border-bottom: 1px solid #eee; \
                                                background: #fff; width: 2.5rem;",
                                    }
                                }
                                for col in &visible_cols {
                                    {filter_th(col, widths.get(&col.id).copied(), handle, &filters, &labels, sticky_header_css.get(&col.id).map_or("", String::as_str))}
                                }
                            }
                        }
                    }

                    tbody {
                        if win.top_pad_px > 0.0 {
                            tr {
                                td {
                                    colspan: "{effective_col_count}",
                                    style: "height: {win.top_pad_px}px; padding: 0; border: 0;",
                                }
                            }
                        }
                        for (i, (row_id, row)) in id_slice.iter().zip(row_slice.iter()).enumerate() {
                            {
                                let editing_col = editing_target
                                    .filter(|t| t.row_id == *row_id)
                                    .map(|t| t.column_id);
                                data_tr(row, *row_id, win.start_index + i, variable_row_height, &visible_cols, row_height, &widths, selection_enabled, selection_set.contains(row_id), handle, &cell_renderers, editing_col, editing_text, edit_error, &validate_edit, on_commit_edit, &sticky_body_css)
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
                                            color: #999; font-style: italic;",
                                    "{labels.no_rows_label}"
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
                                border-top: 1px solid #ddd; background: #fafafa; \
                                font-size: 0.875rem; color: #999;",
                        "{labels.load_more_label}"
                    }
                }
            } else {
                div {
                    style: "padding: 0.5rem 1rem; display: flex; align-items: center; \
                            flex-wrap: wrap; gap: 0.25rem; border-top: 1px solid #ddd; \
                            background: #fafafa; font-size: 0.875rem; color: #555;",
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
                    span { style: "margin-left: 0.5rem; color: #999;", "\u{00b7}" }
                    span { "{total_rows} rows" }
                    if total_pages > 1 {
                        span { style: "margin-left: 0.5rem; color: #999;", "\u{00b7}" }
                        GotoPageInput::<TRow> { handle, total_pages, labels: labels.clone() }
                    }
                    if csv_export {
                        span { style: "flex: 1;" }
                        button {
                            style: "padding:0.25rem 0.75rem;border:1px solid #4a90e2;\
                                    border-radius:3px;font-size:0.875rem;cursor:pointer;\
                                    background:white;color:#4a90e2;",
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
    view: &[(RowId, TRow)],
    variable: bool,
) -> (VirtualWindow, Vec<RowId>, Vec<TRow>) {
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
        return (win, vec![], vec![]);
    }
    let win_end = win.end_index.min(total.saturating_sub(1));
    let slice = &view[win.start_index..=win_end];
    let ids: Vec<RowId> = slice.iter().map(|(id, _)| *id).collect();
    let rows: Vec<TRow> = slice.iter().map(|(_, r)| r.clone()).collect();
    (win, ids, rows)
}

// ---------------------------------------------------------------------------
// Row and cell helpers (not components — plain functions returning Element)
// ---------------------------------------------------------------------------

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
    on_column_order_change: Option<EventHandler<Vec<ColumnId>>>,
    sticky_css: &str,
) -> Element {
    let w = col_width_style(override_width, col.initial_width);
    let align = alignment_css(col.alignment);
    let header = col.header.clone();
    let col_id = col.id;
    let is_sortable = sort_enabled && col.sortable;
    let initial_width = col.initial_width;

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
    let is_drag_over = column_reorder_enabled && drag_col_id.read().is_some_and(|id| id != col_id);

    let drag_over_style = if is_drag_over {
        "outline: 2px dashed #4a90e2; outline-offset: -2px; "
    } else {
        ""
    };

    rsx! {
        th {
            style: "{extra}padding: 0.5rem 1rem; border-bottom: 1px solid #ddd; \
                    text-align: {align}; white-space: nowrap; overflow: hidden; \
                    text-overflow: ellipsis; position: sticky; top: 0; \
                    background: #f8f9fa; z-index: 1; {w} {sticky_css} {drag_over_style}",
            draggable: column_reorder_enabled,
            onclick: move |e| {
                if is_sortable {
                    let action = if e.modifiers().shift() {
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
            ondragover: move |e| {
                if column_reorder_enabled {
                    e.prevent_default();
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
                }
            },
            ondragend: move |_| {
                drag_col_id.set(None);
            },
            "{header}{sort_arrow}"
            if let Some(badge) = sort_badge {
                sup {
                    class: "chorale-sort-badge",
                    style: "font-size: 0.65em; margin-left: 2px; color: #4a90e2; \
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
                        let current_w = override_width.or(initial_width).unwrap_or(100.0);
                        drag_state.set(Some((col_id, e.client_coordinates().x, current_w)));
                    },
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
            style: "padding: 0.5rem 1rem; background: #f0f4ff; border-bottom: 1px solid #ddd; \
                    display: flex; gap: 0.75rem; flex-wrap: wrap; align-items: center; \
                    font-size: 0.8rem; color: #444;",
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
        "padding: 0.25rem 0.5rem; border-bottom: 1px solid #eee; background: #fff; {w} {sticky_css}"
    );
    let empty_th_style = format!(
        "padding: 0.25rem; border-bottom: 1px solid #eee; background: #fff; {w} {sticky_css}"
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
                                    padding: 2px 4px; border: 1px solid #ccc; \
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
                    cursor: pointer; color: #888; font-size: 0.95rem; \
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
                style: "cursor: pointer; padding: 2px 4px; border: 1px solid #ccc; \
                        border-radius: 2px; background: #fff; list-style: none;",
                "{summary_label} \u{25be}"
            }
            div {
                style: "position: absolute; left: 0; top: 100%; z-index: 10; \
                        background: #fff; border: 1px solid #ccc; border-radius: 2px; \
                        padding: 4px 6px; min-width: 100%; white-space: nowrap; \
                        box-shadow: 0 2px 6px rgba(0,0,0,0.08); max-height: 240px; \
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
                style: "display: flex; justify-content: space-between; color: #555;",
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
                style: "flex: 1; min-width: 0; padding: 1px 2px; border: 1px solid #ccc; \
                        border-radius: 2px; font-size: 0.75rem;",
                oninput: move |e| {
                    let parsed = chorale_core::NaiveDate::parse_from_str(&e.value(), "%Y-%m-%d").ok();
                    commit_date_range(col_id, parsed, cur_max, &handle);
                },
            }
            input {
                r#type: "date",
                value: "{max_str}",
                style: "flex: 1; min-width: 0; padding: 1px 2px; border: 1px solid #ccc; \
                        border-radius: 2px; font-size: 0.75rem;",
                oninput: move |e| {
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
                    border: 1px solid #ccc; border-radius: 2px; font-size: 0.8rem; background: #fff;",
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
) -> Element {
    rsx! {
        th {
            style: "padding: 0.25rem 0.5rem; border-bottom: 1px solid #ddd; position: sticky; \
                    top: 0; background: #f8f9fa; z-index: 1; width: 2.5rem; text-align: center;",
            input {
                r#type: "checkbox",
                checked: all_page_selected,
                onchange: move |_| handle.toggle_select_all(),
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
    editing_col: Option<ColumnId>,
    editing_text: Signal<String>,
    edit_error: Signal<Option<String>>,
    validate_edit: &ValidateEditFn,
    on_commit_edit: Option<EventHandler<CommittedEdit<TRow>>>,
    sticky_css_map: &HashMap<ColumnId, String>,
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
    // tr keeps its old `background: #eff6ff` and the row stays blue after
    // the checkbox toggles off. Always emitting a concrete background
    // value forces the override.
    let (row_bg, separator_color) = if is_selected && selection_enabled {
        ("background: #eff6ff;", "#dbeafe")
    } else {
        ("background: transparent;", "#f0f0f0")
    };
    rsx! {
        tr {
            style: "{row_bg}",
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
                    )}
                } else {
                    {data_td(row, col, row_height, variable_row_height, widths.get(&col.id).copied(), cell_renderers.get(col.id), separator_color, sticky_css_map.get(&col.id).map_or("", String::as_str))}
                }
            }
        }
    }
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
) -> Element {
    let col_id = col.id;
    let editor_kind = col.editor.clone().unwrap_or(EditorKind::Text);
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

    rsx! {
        td {
            style: "{style}",
            input {
                r#type: "{input_type}",
                value: "{text_val}",
                min: if !num_min.is_empty() { "{num_min}" },
                max: if !num_max.is_empty() { "{num_max}" },
                step: if !num_step.is_empty() { "{num_step}" },
                style: "width: 100%; box-sizing: border-box; font: inherit; \
                        padding: 1px 4px; border: 1px solid #4a90e2; border-radius: 2px;",
                oninput: move |e| editing_text.set(e.value()),
                onkeydown: move |e: KeyboardEvent| {
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
                                }
                                Err(msg) => edit_error.set(Some(msg)),
                            }
                        }
                        Key::Escape => {
                            let mut sig = handle.signal();
                            let new_state = cancel_edit(&*sig.read());
                            sig.set(new_state);
                        }
                        Key::Tab => {
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
            if let Some(err) = err_val {
                div {
                    style: "color: #c0392b; font-size: 0.75rem; margin-top: 2px;",
                    "{err}"
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn data_td<TRow: Clone>(
    row: &TRow,
    col: &ColumnDef<TRow>,
    row_height: f64,
    variable_row_height: bool,
    override_width: Option<f64>,
    custom_renderer: Option<CellRenderer>,
    separator_color: &str,
    sticky_css: &str,
) -> Element {
    let val = (col.accessor)(row);
    let align = alignment_css(col.alignment);
    let w = col_width_style(override_width, col.initial_width);
    // In variable-height mode, omit the explicit height constraint so that the
    // TD sizes to its content and getBoundingClientRect() returns the true height.
    let style = if variable_row_height {
        format!(
            "padding: 0.5rem 1rem; text-align: {align}; \
             box-sizing: border-box; box-shadow: inset 0 -1px 0 {separator_color}; {w} {sticky_css}"
        )
    } else {
        format!(
            "padding: 0.5rem 1rem; height: {row_height}px; text-align: {align}; \
             white-space: nowrap; overflow: hidden; text-overflow: ellipsis; \
             box-sizing: border-box; box-shadow: inset 0 -1px 0 {separator_color}; {w} {sticky_css}"
        )
    };
    let content = if let Some(renderer) = custom_renderer {
        renderer(&val)
    } else {
        cell_element(&val, &col.render_kind)
    };
    rsx! {
        td {
            style: "{style}",
            {content}
        }
    }
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
                    color: #555; font-size: 0.875rem;",
            "{labels.go_to_page_label}"
            input {
                r#type: "number",
                min: "1",
                max: "{max_page}",
                value: "{draft.read()}",
                style: "width: 4.5em; padding: 2px 4px; border: 1px solid #ccc; \
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
                style: "padding: 0 0.25rem; color: #aaa; font-size: 0.875rem;",
                "\u{2026}"
            }
        };
    };
    let is_active = p == current_idx;
    let style = if is_active {
        "padding:0.25rem 0.5rem;border:1px solid #4a90e2;background:#4a90e2;\
         color:white;border-radius:3px;cursor:default;font-size:0.875rem;"
    } else {
        "padding:0.25rem 0.5rem;border:1px solid #ddd;background:white;\
         color:#333;border-radius:3px;cursor:pointer;font-size:0.875rem;"
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
    use chorale_core::{
        visible_row_ids, visible_view, visible_window_for_state, Alignment, CellValue, ColumnDef,
        ColumnId, RenderKind, RowId, SortDirection, SortState, TableState,
    };

    use super::compute_window_slice;

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

        let (helper_win, helper_ids, helper_rows) = compute_window_slice(&state, &view, false);

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
        state.rows.clear();
        let view = visible_view(&state);
        let (win, ids, rows) = compute_window_slice(&state, &view, false);
        assert_eq!(win.start_index, 0);
        assert_eq!(win.end_index, 0);
        assert!(ids.is_empty());
        assert!(rows.is_empty());
    }

    /// Asserts `compute_window_slice` is deterministic given the same view
    /// and state — a regression here would suggest hidden non-determinism
    /// (e.g. iteration-order-dependent logic in the slicing).
    #[test]
    fn compute_window_slice_is_deterministic() {
        let state = make_state(120.0, 30.0, 200.0);
        let view = visible_view(&state);
        let (w1, i1, r1) = compute_window_slice(&state, &view, false);
        let (w2, i2, r2) = compute_window_slice(&state, &view, false);
        assert_eq!(w1, w2);
        assert_eq!(i1, i2);
        assert_eq!(r1, r2);
    }

    /// Page count = 1 → single button rendered for page 0.
    #[test]
    fn compute_window_slice_clamps_scroll_past_content() {
        // A stale scroll_top can outrun the page content after a sort/filter
        // shrinks the view. The window math should not panic and should not
        // produce a negative-arithmetic out-of-bounds slice.
        let state = make_state(10_000.0, 40.0, 300.0);
        let view = visible_view(&state);
        let (win, ids, rows) = compute_window_slice(&state, &view, false);
        assert!(win.end_index < view.len());
        assert!(ids.len() == rows.len());
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
        let (_win, _ids, rows) = super::compute_window_slice(&s, &view, false);
        assert_eq!(rows.len(), view.len());
    }

    // ---- badge_style -------------------------------------------------------

    #[test]
    fn badge_style_unknown_color_falls_back_to_default() {
        let s = super::badge_style("hotpink");
        assert!(
            s.contains("#e5e7eb"),
            "unknown color should use fallback bg"
        );
        assert!(
            s.contains("#1f2937"),
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
}
