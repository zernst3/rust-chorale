# Item 11.5: `chorale-leptos` Adapter

## Problem

chorale v0.1.0 ships only `chorale-dioxus`. The headless-core architecture (CHORALE-CORE-1,
CHORALE-CORE-2) was designed to support multiple framework adapters from day one, but the
Leptos adapter exists only as a `0.0.0` placeholder on crates.io. This is a gap the README
acknowledges explicitly.

`leptos-struct-table` is the dominant Leptos table library. It ships via a derive-macro
primary API, multi-column sort, virtualization, and a `PaginatedTableDataProvider` trait
for server-side data. Its gaps: no built-in filter UI, virtualization described as "not
perfectly smooth yet," no column resize, no CSV export, and no typed per-column filter
kinds. `chorale-leptos` can differentiate on all five in v0.2.0.

The headless portability claim in the chorale README ("write chorale-core once; plug into
any framework") becomes demonstrable, not aspirational, the moment `chorale-leptos` ships.
This item upgrades the `0.0.0` placeholder to a real `0.1.0` adapter.

## Proposed Public API

### `chorale-leptos` crate

New workspace crate: `chorale-leptos/`. Depends on `chorale-core` and `leptos`.

```rust
/// The typed handle returned by `use_chorale_table`. Wraps a Leptos
/// `RwSignal<TableState<TRow>>` and exposes the same convenience methods
/// as `UseTableHandle` in chorale-dioxus.
pub struct UseTableHandle<TRow: Clone + PartialEq + 'static> {
    signal: RwSignal<TableState<TRow>>,
}

impl<TRow: Clone + PartialEq + 'static> UseTableHandle<TRow> {
    pub fn signal(&self) -> RwSignal<TableState<TRow>>;
    pub fn toggle_sort(&self, column_id: &ColumnId, action: SortAction);
    pub fn set_filter(&self, column_id: &ColumnId, value: FilterValue);
    pub fn clear_filter(&self, column_id: &ColumnId);
    pub fn set_page(&self, page: usize);
    pub fn set_page_size(&self, size: Option<usize>);
    pub fn toggle_selection(&self, row_id: &RowId);
    pub fn set_column_width(&self, column_id: &ColumnId, width: f64);
    pub fn selection_count(&self) -> usize;
    pub fn selected_ids(&self) -> Vec<RowId>;
    pub fn update<F: Fn(&TableState<TRow>) -> TableState<TRow>>(&self, f: F);
}

/// Hook: creates and returns a UseTableHandle. Call inside a component body.
pub fn use_chorale_table<TRow>(
    rows: Vec<TRow>,
    columns: Vec<ColumnDef<TRow>>,
) -> UseTableHandle<TRow>
where
    TRow: Clone + PartialEq + 'static;

/// The table component.
#[component]
pub fn Table<TRow: Clone + PartialEq + 'static>(
    handle: UseTableHandle<TRow>,
    // Props mirror chorale-dioxus `Table` where applicable:
    #[prop(optional)] row_height: Option<f64>,
    #[prop(optional)] variable_row_height: Option<bool>,
    #[prop(optional)] selection_enabled: Option<bool>,
    #[prop(optional)] selection_toolbar: Option<ChildrenFn>,
    #[prop(optional)] labels: Option<Labels>,
    #[prop(optional)] column_reorder_enabled: Option<bool>,
    #[prop(optional)] on_commit_edit: Option<Callback<CommittedEdit>>,
    #[prop(optional)] on_validate_edit: Option<Callback<EditValidation, Result<(), String>>>,
    #[prop(optional)] group_header_class: Option<String>,
) -> impl IntoView;
```

Callsite shape (mirrors Dioxus closely):

```rust
#[component]
fn App() -> impl IntoView {
    let rows = vec![/* ... */];
    let columns = vec![
        ColumnDef::new("name", "Name", |r: &Invoice| CellValue::Text(r.name.clone()))
            .sortable()
            .filter(FilterKind::Text),
    ];
    let handle = use_chorale_table(rows, columns);

    view! {
        <Table handle=handle row_height=40.0 />
    }
}
```

## Internal Design

### Signal mapping

`TableState<TRow>` is stored in a `RwSignal<TableState<TRow>>`. This is the canonical
Leptos pattern for owned mutable state: `RwSignal` is both readable (`.get()` or
`.with(f)`) and writable (`.set()` or `.update(f)`).

Derived reactive values (the current page's rows) use `Memo`:

```rust
let view_key = Memo::new(move |_| {
    let state = handle.signal().get();
    (state.current_page, state.page_size, state.sort.clone(),
     state.filters.clone(), state.rows.len())
});

let visible = Memo::new(move |_| {
    let _ = view_key.get();  // subscribe to view_key, not full state
    visible_view(&handle.signal().get_untracked())
});
```

This mirrors the PERF-1 two-level memo pattern from chorale-dioxus: scroll events that
update DOM state (but not `view_key`) do not trigger the filter+sort pipeline.

`get_untracked()` reads the signal without subscribing, so `visible` re-fires only when
`view_key` changes, not on every `RwSignal` write. This is the Leptos equivalent of
Dioxus's `peek()` / `use_memo` keyed pattern.

### Reactive subscription model

Leptos's `Memo` automatically tracks which signals are read inside its closure. Reading
`view_key.get()` (which reads `handle.signal()`) makes `visible` react to `view_key`
changes. Reading `handle.signal().get_untracked()` in `visible_view(...)` reads the
state without a subscription, so the sort/filter pipeline does not re-fire on unrelated
state changes (e.g., `scroll_top` or column width changes that do not affect `view_key`).

### Virtualization in Leptos

Leptos's scroll event type is `leptos::ev::Event` (DOM `Event` cast from the scroll
event). The scroll handler reads `event.target().scroll_top()` via `wasm-bindgen`.

Three virtualization requirements from VIRT-1 apply unchanged:
1. `overflow-anchor: none` on the scroll container (prevents browser anchor-scroll
   fighting with programmatic scroll restoration).
2. Sync `scroll_top` from the scroll event into a `RwSignal<f64>` (equivalent of
   Dioxus's `onscroll` handler writing to a signal).
3. `box-shadow` row separator (CSS, framework-agnostic).

In Leptos:

```rust
let scroll_top = RwSignal::new(0.0f64);

view! {
    <div
        style="overflow-y: auto; overflow-anchor: none;"
        on:scroll=move |ev| {
            let target = ev.target().unwrap().unchecked_into::<web_sys::HtmlElement>();
            scroll_top.set(target.scroll_top() as f64);
        }
    >
        // top spacer
        <div style=move || format!("height: {}px;", window_memo.get().top_pad_px) />
        // rows
        <For
            each=move || visible.get()
            key=|row| row.id.clone()
            children=move |row| view! { <tr>/* cells */</tr> }
        />
        // bottom spacer
        <div style=move || format!("height: {}px;", window_memo.get().bottom_pad_px) />
    </div>
}
```

The `For` component with a stable `key` is Leptos's keyed iteration primitive (equivalent
to Dioxus's `for` loop with `key`).

### Component structure and rendering

```
Table
├── header row (sort icons, filter inputs)
├── scroll container
│   ├── top spacer div
│   ├── For (data rows)
│   │   └── tr > td (cell rendering pipeline)
│   └── bottom spacer div
├── pagination bar
└── selection_toolbar slot (when non-empty selection)
```

Leptos `#[component]` + `#[prop(...)]` derives are syntactically equivalent to Dioxus
`#[component]`. The primary difference is prop optionality: Leptos uses `#[prop(optional)]`
vs Dioxus's `Option<T>` field pattern.

### Custom cell renderer escape hatch

`leptos-struct-table` uses `...renderer` attribute pointing to a component name. chorale's
approach: `CellRenderers` is a `HashMap<ColumnId, Box<dyn Fn(CellInfo<TRow>) -> View>>`.
The adapter checks if a renderer is registered for the column before falling back to the
default `CellValue` rendering.

```rust
// In the Table component:
pub cell_renderers: Option<CellRenderers<TRow>>,

// Host app:
let renderers = CellRenderers::new()
    .add("status", |info: CellInfo<Invoice>| view! {
        <Badge variant=info.value.to_string() />
    });

view! { <Table handle=handle cell_renderers=renderers /> }
```

This matches the Dioxus adapter's `CellRenderers` API identically — the cell-renderer
contract is defined in chorale-core and is framework-agnostic.

### Filter component composition

The five `FilterKind` variants render as Leptos components. The `<details>`-based
`MultiSelect` outside-click watcher uses Leptos's `window_event_listener` (Leptos's
equivalent of adding a global event listener in Dioxus's `use_effect`).

## chorale-leptos vs leptos-struct-table: differentiation summary

| Feature | `leptos-struct-table` | `chorale-leptos` (v0.2.0) |
|---|---|---|
| Built-in filter UI | None | 5 `FilterKind` variants |
| Virtualization smoothness | "not perfectly smooth" | VIRT-1 proven approach |
| Column resize | No | Yes (chorale-core `column_widths`) |
| CSV export | No | Yes (chorale-core `to_csv`) |
| Multi-column sort | Yes | Yes (Item 11.0a) |
| Infinite scroll | Via `PaginatedTableDataProvider` | Yes (Item 11.0b) |
| Derive macro | Yes (primary API) | Yes (Item 11.0d, opt-in) |
| i18n labels | No | Yes (Item 11.0c) |
| Headless core | No (Leptos-coupled) | Yes (CHORALE-CORE-1) |

## Backwards Compatibility

New crate. No existing callers. The `0.0.0` placeholder on crates.io is overwritten with
the first real release (`0.1.0`). No compatibility concerns.

`chorale-core` is not modified by this item. The adapter consumes the existing public API.

## Test Plan

Per TESTS-1, ORCH-NEW-PATH-TESTS-1:

- `use_chorale_table` returns a `UseTableHandle` with a valid `RwSignal`.
- Calling `handle.toggle_sort(...)` updates the signal; `Memo`-derived `visible` updates.
- Filter + sort pipeline does not re-fire on `scroll_top` signal change (PERF-1).
- Virtualization: `window_memo` changes on scroll; rendered row slice matches window bounds.
- `selection_enabled: true` → checkboxes render; `handle.selection_count()` updates on
  click.
- `cell_renderers` registered for a column → custom view rendered for that column's cells.
- `labels: Some(...)` prop → custom strings appear in rendered output.
- At least one integration test using Leptos's `leptos::ssr::render_to_string` to verify
  the component renders without panic.

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **`RwSignal<TableState<TRow>>` vs `ArcRwSignal<TableState<TRow>>`.** Recommendation:
   `RwSignal` — Leptos 0.7's default signal type for component-scoped state. `ArcRwSignal`
   is for cross-thread scenarios; the table state is always owned by the component tree
   on the main WASM thread.

2. **`use_chorale_table` hook vs a `TableStore` struct pattern.** leptos-struct-table uses
   neither — its state is managed externally. Recommendation: hook pattern (mirrors
   Dioxus's `use_chorale_table`); makes the Leptos and Dioxus callsites nearly identical,
   reducing cognitive overhead for users who use both adapters.

3. **`selection_toolbar` prop type: `ChildrenFn` vs `Option<ChildrenFn>` vs `Option<View>`.**
   Recommendation: `Option<ChildrenFn>` — `ChildrenFn` is Leptos's equivalent of
   Dioxus's `children` prop type and is re-renderable (unlike `Children` which consumes
   itself on first render). `Option<_>` allows `None` as the default.

4. **Leptos version to target: 0.6 or 0.7?** Recommendation: Leptos 0.7 — it's the
   current stable release as of the v0.2.0 development window, and its `#[component]`
   and signal APIs are stable. Leptos 0.6 is the prior release; targeting it would require
   backporting if the community has moved on.

5. **How should `on_validate_edit` and `on_commit_edit` callbacks type in Leptos?**
   Dioxus uses `EventHandler<T>`; Leptos uses `Callback<T>` or `Callback<T, R>` for
   fallible callbacks. Recommendation: `Callback<EditValidation, Result<(), String>>` for
   validation, `Callback<CommittedEdit>` for commit. This is idiomatic Leptos; document
   the `EventHandler` ↔ `Callback` mapping in the API migration guide for users coming
   from chorale-dioxus.

## Decisions (signed off 2026-06-04)

All 5 recommendations accepted as written.

1. ✅ `RwSignal<TableState<TRow>>`. Component-scoped state on the main WASM
   thread; `ArcRwSignal` reserved for cross-thread cases.
2. ✅ `use_chorale_table` hook pattern. Mirrors Dioxus callsites; cross-adapter
   muscle memory.
3. ✅ `selection_toolbar: Option<ChildrenFn>`. Re-renderable; `None` default.
4. ✅ Target Leptos 0.7 (current stable in the v0.2.0 window).
5. ✅ `Callback<EditValidation, Result<(), String>>` for validate;
   `Callback<CommittedEdit>` for commit. Document the `EventHandler` ↔
   `Callback` mapping in the API migration guide.
