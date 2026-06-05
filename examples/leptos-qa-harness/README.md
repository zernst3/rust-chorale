# leptos-qa-harness

Leptos QA harness for `chorale-leptos`. Mirrors the Dioxus `qa-harness` with
toggle controls for every v0.1 and v0.2.0 feature.

## Running

```sh
# From this directory:
trunk serve --open

# Or from the workspace root:
trunk serve --open --package leptos-qa-harness
```

Requires [Trunk](https://trunkrs.dev): `cargo install trunk`

> **Note:** This is a Leptos/WASM example. Do **not** use `dx serve` (that is for
> Dioxus examples). Use `trunk serve` as shown above.

## Feature toggles

### v0.1 features
- **Sort** — multi-column sort via Shift+click
- **Filter** — all 5 filter kinds (text, numeric range, date range, multi-select, boolean)
- **Selection** — per-row checkboxes + select-all
- **Column Visibility** — show/hide columns at runtime
- **CSV Export** — downloads all post-filter rows
- **Column Resize** — drag the column header edge to resize

### v0.2.0 features
- **Infinite Scroll** — switches pagination to `PaginationMode::InfiniteScroll`
- **French Labels** — overrides all user-visible strings with French equivalents
- **Variable Row Height** — not yet implemented in the Leptos adapter (stub note shown)
- **In-cell Editing (Name)** — double-click the Name column to edit in place
- **Group by Role** — groups rows by the Role field with `AggregatorKind::Sum` on Salary
- **Grouped pagination** select — `DataRowsOnly` vs `Virtualized`
- **Column Reorder** — drag handles on column headers
- **Frozen Columns** — Name pinned left, Salary pinned right
- **Selection Toolbar** — shows a count bar above the table when rows are selected
- **Use #[derive(TableRow)] columns** — switches to macro-generated column definitions
