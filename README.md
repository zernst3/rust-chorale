# chorale

A framework-agnostic, type-safe data-table library for Rust. Ships with
a Dioxus adapter and a Leptos adapter; adapters for Yew and Sycamore are
planned for future releases. Inspired by
[TanStack Table](https://tanstack.com/table).

> **A note on "headless":** chorale uses the term in the
> [TanStack](https://tanstack.com/table) /
> [Radix UI](https://www.radix-ui.com/) sense — the logic (sort, filter,
> paginate, virtualize, select) lives in a separate crate from any
> rendering, so the same logic can power adapters for different UI
> frameworks. This differs from the Rust infra / web-scraping world where
> "headless" means "no display server."

## Status

`chorale-core` v0.2.0, `chorale-dioxus` v0.2.0, `chorale-leptos` v0.2.0,
and `chorale-derive` v0.2.0 are published. All four share a single
`draft-release/v0.2.0` branch; see `CHANGELOG.md` for the full feature list.

All six `chorale-*` crate names are reserved on crates.io:
[chorale](https://crates.io/crates/chorale),
[chorale-core](https://crates.io/crates/chorale-core),
[chorale-dioxus](https://crates.io/crates/chorale-dioxus),
[chorale-leptos](https://crates.io/crates/chorale-leptos),
[chorale-yew](https://crates.io/crates/chorale-yew),
[chorale-sycamore](https://crates.io/crates/chorale-sycamore).
The `chorale-yew` and `chorale-sycamore` placeholders remain at `0.0.0`
until those adapters are built.

## Quickstart — Dioxus

```rust
use chorale_core::{CellValue, ColumnDef, ColumnId, FilterKind, RowId, TableState};
use chorale_dioxus::{use_table, Table};
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
struct Book { title: String, author: String, year: i64 }

fn columns() -> Vec<ColumnDef<Book>> {
    vec![
        ColumnDef::new(ColumnId("title"), "Title", |b: &Book| {
            CellValue::Text(b.title.clone())
        }).sortable().filter(FilterKind::Text).initial_width(280.0),
        // … author, year
    ]
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(rows_with_ids(), columns()));
    rsx! { Table { handle: table, sort_enabled: true, filter_enabled: true } }
}
```

Full version: [`examples/basic`](examples/basic/src/main.rs).
Build with: `cargo install dioxus-cli && dx serve --package basic`

## Quickstart — Leptos

```rust
use chorale_core::{CellValue, ColumnDef, ColumnId, FilterKind};
use chorale_leptos::{use_chorale_table, Table};
use leptos::prelude::*;

#[derive(Clone, PartialEq)]
struct Book { title: String, author: String, year: i64 }

fn columns() -> Vec<ColumnDef<Book>> {
    vec![
        ColumnDef::new(ColumnId("title"), "Title", |b: &Book| {
            CellValue::Text(b.title.clone())
        }).sortable().filter(FilterKind::Text).initial_width(280.0),
        // … author, year
    ]
}

#[component]
fn App() -> impl IntoView {
    // Note: Vec<Book> — no RowIds needed; use_chorale_table assigns them.
    let table = use_chorale_table(books(), columns());
    view! { <Table handle=table sort_enabled=true filter_enabled=true /> }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
```

Full version: [`examples/leptos-basic`](examples/leptos-basic/src/main.rs).
Build with: `cargo install trunk && cd examples/leptos-basic && trunk serve --open`

## Using `#[derive(TableRow)]`

```rust
use chorale_derive::TableRow;
use chorale_core::{CellValue, ColumnId, FilterKind};

#[derive(Clone, PartialEq, TableRow)]
struct Employee {
    #[chorale(header = "Full Name", filter = "text", sortable)]
    name: String,
    #[chorale(header = "Salary", sortable)]
    salary: i64,
    #[chorale(skip)]
    internal_id: u64,
}

// Generates:
// impl Employee {
//     pub fn chorale_columns() -> Vec<ColumnDef<Employee>> { … }
// }
```

## What you get in v0.2.0

### Core table features (both adapters)

- **Sort.** Single-column or multi-column with `SortAction::Replace` (default)
  or `SortAction::Append` (Shift+click). Sort priority badges show which
  column is primary, secondary, etc.
- **Filter.** Per-column filter shape declared on `ColumnDef`. Five kinds:
  text substring, multi-select, dual-bound numeric range, date range, and
  tri-state boolean.
- **Pagination.** Configurable page size, prev / next / windowed page buttons
  with ellipsis, plus a "Go to" number input for jumping across hundreds of
  pages.
- **Infinite scroll.** `PaginationMode::InfiniteScroll` loads rows in batches
  as the user scrolls near the bottom. Switch back to Pages mode at any time.
- **Selection.** Per-row checkboxes plus a header select-all. Readable via
  `handle.selected_ids()` / `handle.selection_count()`.
- **Grouping and aggregation.** Group by one or more columns; collapse and
  expand groups. Per-column aggregators (sum, average, min, max, count)
  appear in group header rows.
- **Custom cells.** `RenderKind::Badge` (declarative pill rendering) or
  `CellRenderers` (per-column arbitrary framework markup).
- **Column visibility toolbar.** Toggle any column on or off at runtime.
- **Column resize.** Drag the right edge of any header. Widths persist in
  `TableState::column_widths`.
- **Column reorder.** Drag-and-drop column headers to rearrange column order.
- **Frozen columns.** Mark columns `FrozenSide::Left` or `FrozenSide::Right`
  to stick them in place while scrolling.
- **Variable-row-height virtualization.** Set per-row heights in
  `TableState::row_heights`; the window math automatically handles mixed heights.
- **Fixed-row-height virtualization.** Always-on when all rows share the same
  height. O(1) per scroll event regardless of dataset size.
- **In-cell editing.** `EditorKind::Text` on a column makes it double-click
  editable. Validation via `validate_edit` callback. Commit/cancel via
  Enter/Escape/Tab.
- **CSV export.** Exports the full post-filter, post-sort dataset with RFC 4180
  quoting.
- **User-overridable labels.** Pass a custom `Labels` struct to override every
  user-visible string — filter placeholder, pagination labels, export button
  text — for i18n without patching the library.
- **`selection_toolbar` slot.** Pass a child component shown only when one or
  more rows are selected (bulk-action toolbar pattern).

### `chorale-derive`

`#[derive(TableRow)]` generates `fn chorale_columns() -> Vec<ColumnDef<Self>>`
from struct fields. Attributes: `header`, `sortable`, `filter`, `initial_width`,
`alignment`, `render_kind`, `skip`.

## Architecture

Two persistent layers separated by a hard, lint-enforced boundary.

### `chorale-core`

Framework-agnostic state plus pure functions over it. Zero UI dependencies
(CHORALE-CORE-1). Transitions are immutable: every transition takes
`&TableState<TRow>` and returns a fresh state (CHORALE-CORE-2).

| Module | Surface |
|---|---|
| `state` | `TableState<TRow>`, `VirtualWindow` |
| `column` | `ColumnDef<TRow>`, `RenderKind`, `FilterKind`, `BadgeVariantMap`, `EditorKind`, `FrozenSide` |
| `types` | `CellValue`, `FilterValue`, `SortState`, `RowId`, `ColumnId`, `Alignment`, `CurrencyCode`, `GroupKey`, `PaginationMode` |
| `transitions` | `toggle_sort`, `set_filter`, `set_page`, `set_page_size`, `set_scroll`, `set_selection`, `toggle_select_all`, `set_column_visibility`, `set_column_width`, `update_row`, `move_column`, `set_grouping`, `toggle_group`, `expand_all_groups`, `collapse_all_groups`, `set_pagination_mode`, `load_more_rows`, `start_edit`, `commit_edit`, `cancel_edit` |
| `views` | `visible_view`, `visible_grouped_view`, `visible_rows`, `visible_row_ids`, `visible_window`, `filtered_sorted_rows`, `to_csv`, `frozen_left_columns`, `frozen_right_columns`, `scrollable_columns` |
| `labels` | `Labels` |
| `error` | `StateError` |

### `chorale-dioxus`

Wraps `TableState<TRow>` in a Dioxus `Signal<T>`, exposes `UseTableHandle<TRow>`
(a `Copy` typed handle), and renders the `<Table>` component. Uses a two-level
memo (PERF-1) so scroll events never retrigger the filter/sort/paginate
pipeline at scale.

```rust
Table {
    handle: table,
    sort_enabled: true,
    filter_enabled: true,
    selection_enabled: true,
    column_toolbar: true,
    csv_export: true,
    resize_enabled: true,
    column_reorder_enabled: true,
    // … labels, cell_renderers, validate_edit, on_commit_edit, selection_toolbar
}
```

### `chorale-leptos`

Same pattern as `chorale-dioxus`, but wraps `TableState<TRow>` in a Leptos
`RwSignal<T>`. The `UseTableHandle<TRow>` is `Copy` via a manual `impl Copy`
(not `#[derive]`, which would add an unwanted `TRow: Copy` bound). The
`Table` component accepts identical props to the Dioxus version.

```rust
view! {
    <Table
        handle=table
        sort_enabled=true
        filter_enabled=true
        selection_enabled=true
    />
}
```

## Filtering

```rust
ColumnDef::new(ColumnId("status"), "Status", |e: &Employee| {
    CellValue::Text(e.status.clone())
})
.filter(FilterKind::MultiSelect {
    options: vec!["Active".into(), "Inactive".into(), "Pending".into()],
})
```

| `FilterKind` | Adapter UI | Matches against |
|---|---|---|
| `None` | no filter cell | nothing |
| `Text` | text input, case-insensitive substring | `CellValue::Text` |
| `MultiSelect { options }` | `<details>` dropdown with checkboxes | `CellValue::Text` |
| `NumericRange { min, max, step }` | dual-bound range inputs | `CellValue::Integer` or `Float` |
| `DateRange` | two `<input type="date">` fields | `CellValue::Date` or `DateTime` |
| `Boolean` | tri-state All / Yes / No | `CellValue::Boolean` |

## Virtualization

The window math is O(1) per scroll event: two integer divisions from
`scroll_top`, `viewport_height`, and `row_height`. The adapter renders a
top-pad spacer, the windowed data rows, and a bottom-pad spacer; total
tbody height always equals `total_rows × row_height`, so the scrollbar
reflects the full dataset.

**PERF-1 (two-level memo):** a cheap `view_key` memo tracks only the fields
that affect `visible_view` output. The expensive pipeline subscribes to
`view_key`, not to the full signal — scroll and selection never retrigger
filter/sort/paginate. At 1M rows this eliminates ~30 MB of allocation per
scroll tick.

Three non-obvious requirements both adapters handle for you:

1. **`overflow-anchor: none`.** Without it, browser scroll anchoring fights
   DOM mutations during virtualization, producing a runaway scroll loop.
2. **Synchronous `scrollTop` reads.** Async reads let the rendered window fall
   behind the DOM during fast scrolling.
3. **Row separators via `box-shadow`.** `border-bottom` on `<tr>` adds layout
   pixels that shift scroll extents. `box-shadow: inset 0 -1px 0` paints
   the separator without participating in layout.

## Examples

### Dioxus examples

Install once: `cargo install dioxus-cli`. Run: `dx serve --package <name>`

| Example | Demonstrates |
|---|---|
| `basic` | Minimal table: sort + text filter. |
| `with-selection` | Per-row and select-all checkboxes, live count via `selection_count()`. |
| `with-custom-cells` | `RenderKind::Badge` vs `CellRenderers` (arbitrary Dioxus markup). |
| `with-column-resize` | Drag-to-resize column borders. |
| `virtualized-10k-rows` | 10 010-row dataset through fixed-row-height virtualization. |
| `virtualized-1m-rows` | 1 000 000-row stress test isolating scroll performance. |
| `qa-harness` | All v0.2.0 features behind runtime toggles; the exhaustive smoke test. |

### Leptos examples

Install once: `cargo install trunk`. Run from inside the example directory:
`trunk serve --open`

| Example | Demonstrates |
|---|---|
| `leptos-basic` | Same as `basic`, using the Leptos adapter. |
| `leptos-with-selection` | Selection with reactive `selection_count()` in Leptos signals. |
| `leptos-with-custom-cells` | `CellRenderers` returning Leptos `AnyView`. |
| `leptos-with-column-resize` | Column resize in Leptos. |
| `leptos-virtualized-10k-rows` | 10 010-row virtualization in Leptos. |
| `leptos-virtualized-1m-rows` | 1M-row stress test with two-stage mount (shows "Initializing…"). |
| `leptos-qa-harness` | Full v0.2.0 QA harness for the Leptos adapter. |

## Feature and architecture comparison

Both adapters provide identical feature coverage. The differences are in the
reactive primitive and build toolchain:

| | `chorale-dioxus` | `chorale-leptos` |
|---|---|---|
| Reactive primitive | `Signal<T>` | `RwSignal<T>` |
| Handle type | `UseTableHandle<TRow>: Copy` | `UseTableHandle<TRow>: Copy` |
| Initial rows | `Vec<(RowId, TRow)>` | `Vec<TRow>` (RowIds assigned internally) |
| Entry hook | `use_table(|| TableState::new(…))` | `use_chorale_table(rows, cols)` |
| Component syntax | `rsx! { Table { handle: table, … } }` | `view! { <Table handle=table … /> }` |
| Build tool | `dx serve` (Dioxus CLI) | `trunk serve` |
| Custom cell type | `Element` | `AnyView` |

The table logic (sort, filter, paginate, virtualize, group, edit) is shared via
`chorale-core` and behaves identically in both adapters.

## Writing an adapter for another framework

1. Depend on `chorale-core` and your framework's reactive primitive.
2. Wrap `TableState<TRow>` in the framework's signal or store.
3. Expose a typed `UseTableHandle<TRow>` (make it `Copy` via a manual `impl Copy`
   rather than `#[derive(Copy)]` to avoid a spurious `TRow: Copy` bound).
4. Each handle method calls a core transition and writes the returned state
   back into the signal.
5. Render from `visible_view(&state)` + `visible_window(...)`. Honor the three
   virtualization requirements (overflow-anchor, synchronous scroll reads,
   box-shadow row separators).

The `chorale-dioxus` and `chorale-leptos` crates are both under 1 400 lines and
serve as reference implementations.

## MSRV

Rust 1.78. Pinned in `workspace.package.rust-version`.

## Development guardrails

Built with [Camerata](https://github.com/zernst3/camerata-ai). Guardrail files
committed for transparency:

- `AGENTS.md` — prose-tier principles (orchestration, architectural commitments).
- `CONVENTIONS.md` — mechanical conventions (Rust patterns, signal usage).
- `docs/CONVENTIONS.md` — chorale-specific code conventions.

## Contributing

Open an issue before a non-trivial PR. CI gates:
`cargo check --workspace --all-targets`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace`. All three must pass.

## License

Dual-licensed under MIT or Apache-2.0 at your option.
[LICENSE-MIT](LICENSE-MIT) · [LICENSE-APACHE](LICENSE-APACHE)
