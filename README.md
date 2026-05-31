# chorale

A headless, type-safe table library for Rust. Inspired by
[TanStack Table](https://tanstack.com/table). State and logic are
framework-agnostic; rendering ships today as a Dioxus adapter, with the same
core ready for Leptos, Yew, egui, and others to publish against without
forking.

## Status

Source-complete for v0.1, in a pre-publish verification window. All six
`chorale-*` crate names are reserved on crates.io as `0.0.0` placeholders
([chorale](https://crates.io/crates/chorale),
[chorale-core](https://crates.io/crates/chorale-core),
[chorale-dioxus](https://crates.io/crates/chorale-dioxus),
[chorale-leptos](https://crates.io/crates/chorale-leptos),
[chorale-yew](https://crates.io/crates/chorale-yew),
[chorale-sycamore](https://crates.io/crates/chorale-sycamore)). The real
`0.1.0` publish of `chorale-core` and `chorale-dioxus` is held pending
feedback from developers reading this source. Remaining work before
`0.1.0`: doc-comments on the public surface, a `CHANGELOG`, and the
publish pass itself.

## Quickstart

A working sortable, text-filterable table is a row type, a
`Vec<ColumnDef<Row>>`, and `use_table` plus `<Table>`:

```rust
use chorale_core::{
    Alignment, CellValue, ColumnDef, ColumnId, FilterKind, RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, Table};
use dioxus::prelude::*;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct Book { title: String, author: String, year: i64 }

fn columns() -> Vec<ColumnDef<Book>> {
    vec![
        ColumnDef {
            id: ColumnId("title"),
            header: "Title".into(),
            accessor: Arc::new(|b: &Book| CellValue::Text(b.title.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(280.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        // ... author, year
    ]
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(rows(), columns()));
    rsx! { Table { handle: table, sort_enabled: true, filter_enabled: true } }
}
```

The full version is in [`examples/basic`](examples/basic/src/main.rs).

## What you get in v0.1

Each item is implemented in `chorale-dioxus` and demonstrated by at least one
example:

- **Sort.** Single-column, ASC / DESC / none cycle. Comparison flows through
  the column's `CellValue` accessor, so the sort order matches the displayed
  value, not the underlying field.
- **Filter.** Per-column filter shape declared on `ColumnDef`. Five kinds:
  text substring, multi-select, dual-handle numeric range, date range, and
  tri-state boolean. The adapter renders the matching UI; the matcher lives
  in `chorale-core` and matches against the column's `CellValue`.
- **Pagination.** Configurable page size, prev / next / windowed page
  buttons with ellipses, plus a "Go to" number input (commits on Enter or
  blur, clamps out-of-range values) for jumping across hundreds of pages.
  Page change resets `scroll_top` and the DOM scroll position so the new
  page lands at the top.
- **Selection.** Per-row checkbox plus a header select-all that toggles only
  the visible page. Selection state is a `Vec<RowId>` on `TableState`, which
  the host app can read directly.
- **Custom cells.** Two paths: declarative `RenderKind::Badge` driven by a
  `BadgeVariantMap`, or `CellRenderers` (a per-column
  `Arc<dyn Fn(&CellValue) -> Element>`) for arbitrary Dioxus markup.
- **Column visibility toolbar.** Optional checklist that toggles columns
  on and off without losing their state.
- **Column resize.** Drag the right edge of any header. Widths persist in
  `TableState::column_widths`.
- **CSV export.** Optional toolbar button. Exports the full post-filter,
  post-sort dataset (not just the current page) with RFC 4180 quoting.
- **Sticky headers and fixed-row-height virtualization.** Always-on. Only
  rows in the viewport plus a small overscan buffer are mounted as `<tr>`
  elements, validated against a 10 000-row dataset in the
  `virtualized-10k-rows` and `qa-harness` examples.

Deferred to v0.2: variable-row-height virtualization, in-cell editing,
grouping and aggregation, column reorder, frozen columns.

## Architecture

Two crates, with a hard, lint-enforced boundary between them.

### `chorale-core`

Framework-agnostic state plus pure functions over it. The crate has zero UI
dependencies (CHORALE-CORE-1) and never holds a framework's element or
event type in its public surface.

| Module | Surface |
|---|---|
| `state` | `TableState<TRow>`, `VirtualWindow` |
| `column` | `ColumnDef<TRow>`, `RenderKind`, `FilterKind`, `BadgeVariantMap` |
| `types` | `CellValue`, `FilterValue`, `SortState`, `RowId`, `ColumnId`, `Alignment`, `CurrencyCode` |
| `transitions` | `toggle_sort`, `set_filter`, `set_page`, `set_page_size`, `set_scroll`, `set_selection`, `toggle_select_all`, `set_column_visibility`, `set_column_width`, `update_row` |
| `views` | `visible_view`, `visible_rows`, `visible_row_ids`, `visible_window`, `filtered_sorted_rows`, `to_csv` |
| `theme` | `Theme`, `CellClassFn`, `RowClassFn` |
| `error` | `StateError` |

Transitions are immutable: every transition takes `&TableState<TRow>` and
returns a fresh state, never mutates in place (CHORALE-CORE-2). This is what
makes time-travel debugging, undo stacks, and reactive integration
straightforward.

### `chorale-dioxus`

The Dioxus adapter. Wraps `TableState<TRow>` in a `Signal`, exposes a typed
`UseTableHandle<TRow>` with one method per core transition, and renders the
`<Table>` component:

```rust
Table {
    handle: table,
    sort_enabled: true,            // default true
    filter_enabled: false,         // default false
    selection_enabled: false,
    column_toolbar: false,
    csv_export: false,
    resize_enabled: false,
    cell_renderers: ...,           // default empty
}
```

Each prop above the default opts you into one of the v0.1 capabilities.

## Filtering

Declare a column's filter shape on its `ColumnDef`. For example:

```rust
filter: FilterKind::MultiSelect {
    options: vec!["Active".into(), "Suspended".into(), "Inactive".into()],
},
```

The five kinds map onto five `FilterValue` variants and five UIs in the
adapter:

| `FilterKind` | Adapter UI | Matches against |
|---|---|---|
| `None` | empty cell (column not filterable) | nothing |
| `Text` | text input, case-insensitive substring | `CellValue::Text` |
| `MultiSelect { options }` | `<details>` dropdown with checkbox list plus outside-click watcher | `CellValue::Text` against the selected set |
| `NumericRange { min, max, step }` | dual-handle range slider with compact min / max labels | `CellValue::Integer` or `Float` |
| `DateRange` | two native `<input type="date">` fields | `CellValue::Date` or `DateTime` |
| `Boolean` | tri-state All / Yes / No select | `CellValue::Boolean` |

When a column has an active filter, a `×` button appears at the right of its
filter cell with a `title="Clear Filter"` tooltip, wired to
`handle.set_filter(col_id, None)`.

## Virtualization

Fixed-row-height only in v0.1. The window math is O(1) per scroll event: two
integer divisions to derive `start_index` and `end_index`, no binary search.

The window math is `visible_window(scroll_top, viewport_height, row_height,
total_rows, buffer_rows)` and lives in `chorale-core`. The adapter renders a
`top_pad` spacer TR, the windowed data rows, and a `bottom_pad` spacer TR.
Total tbody height equals `total_rows * row_height` independent of which
window is rendered, so the scrollbar always reflects the full dataset.

**Known residual, v0.2.** The window math is O(1), but the filter / sort /
paginate pipeline currently recomputes once per render (memoized within a
render, not across renders). Scroll-only state changes therefore still
re-run the pipeline. The bottleneck this implies is bounded (the harness
hits 10 000 rows comfortably), but the principled fix is fine-grained
reactivity over the `TableState` fields (via `dioxus-stores` or a manual
keyed-memo pattern). Tracked for v0.2.

Three non-obvious requirements the Dioxus adapter handles for you, all of
which would also apply to any future adapter:

1. **`overflow-anchor: none` on the scroll container.** Chrome and Firefox
   default to scroll anchoring, which adjusts `scrollTop` in response to DOM
   mutations near the viewport. Virtualization is the pathological case:
   every scroll triggers a render that swaps rendered TRs, the browser
   compensates with another `scrollTop` adjustment, which fires another
   scroll event. The result is a runaway scroll until the user hits the
   top or bottom. Opting out is one CSS property.
2. **Synchronous `scrollTop` reads.** Reading the scroll position via
   `dioxus::document::eval` is async, and the lag lets the rendered window
   fall behind the DOM during fast scrolling. The visible result is
   bottom-padding empty space appearing under the rendered rows.
   `ScrollData::scroll_top()` reads synchronously from the event.
3. **Row separator via `box-shadow`, not TR `border-bottom`.** With
   `border-collapse: collapse`, a TR's bottom border adds 1px of layout per
   data row, but the top / bottom spacer TRs have no border. The result is
   that tbody height equals `total_rows * row_height + N_rendered`, and
   `N_rendered` shifts as the user scrolls, drifting the scroll extents
   render to render. `box-shadow: inset 0 -1px 0 <color>` paints the
   separator without participating in layout.

If you write a chorale-* adapter for another framework, all three of the
above apply to your scroll container too.

## Examples

Every example is a Dioxus-web crate. Install the CLI once
(`cargo install dioxus-cli`), then run any one of them:

```bash
dx serve --package <example-name>   # opens http://localhost:8080
```

| Example | Demonstrates |
|---|---|
| `basic` | The smallest working table: sort plus text filter on a hand-coded dataset. |
| `with-selection` | Per-row and select-all checkboxes plus a live selection count read from the signal. |
| `with-custom-cells` | `RenderKind::Badge` (declarative) versus `CellRenderers` (arbitrary Dioxus markup, demonstrated with a health-bar progress indicator). |
| `with-column-resize` | Drag the right edge of any column header to resize. |
| `virtualized-10k-rows` | 10 010-row dataset (intentionally not a round multiple of page size, so the last page is partial) rendered through the fixed-row-height virtualization. |
| `virtualized-1m-rows` | 1 000 000-row stress test. Sort and filter intentionally disabled (the v0.2 residual) so the example isolates virtualization scroll performance at scale. |
| `qa-harness` | All v0.1 features behind one set of toggles. The exhaustive smoke test. |

Stop the running `dx serve` (Ctrl+C) before starting another, or pass
`--port 8081` to run two side by side.

## Writing an adapter for another framework

To publish `chorale-<framework>`:

1. Depend on `chorale-core` and your framework's reactive primitive.
2. Wrap `TableState<TRow>` in the framework's signal or store equivalent
   (Dioxus `Signal`, Leptos `RwSignal`, Yew `Reducible`, etc.).
3. Expose a typed handle (analogous to `UseTableHandle`) with one method per
   core transition. Each method calls the transition, then writes the
   returned state back into the signal.
4. Render the table body from `visible_view(&state)` plus
   `visible_window(...)`. The window result carries `top_pad_px` and
   `bottom_pad_px` you place around the rendered rows.
5. Honor the three virtualization requirements above. They are not
   Dioxus-specific.

`chorale-core` deliberately does not know about your framework's element
type. The boundary CHORALE-CORE-1 enforces is what makes adapter authoring
tractable: you write the bridge to your framework's reactive model, never a
fork of the table logic.

## MSRV

Rust 1.78. Pinned in `workspace.package.rust-version`.

## Development guardrails

This library was built using
[Camerata](https://github.com/zernst3/camerata-ai), an AI-orchestration
guardrail tool I authored. The installed rule set is committed at the repo
root for transparency:

- `AGENTS.md` is the prose-tier principles digest the authoring agent reads
  at the start of every change (orchestration discipline, architectural
  commitments).
- `CONVENTIONS.md` is the structured / mechanical-tier conventions the agent
  applies during code generation (Rust and Dioxus patterns, error layering,
  async discipline, signal usage).
- `camerata.lock` records installed principle ids and content hashes so
  `camerata outdated` can detect upstream drift.

These are framework-level guardrails, not chorale-specific. They are
committed so any reviewer can see which rules the implementation was written
against, and re-run the same checks on a contribution.

Chorale-specific code conventions live in [`docs/CONVENTIONS.md`](docs/CONVENTIONS.md).

## Contributing

Open an issue before opening a non-trivial PR. The CI gates are
`cargo check --workspace --all-targets`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`. All three need to pass.

## License

Dual-licensed under MIT or Apache-2.0 at your option. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
