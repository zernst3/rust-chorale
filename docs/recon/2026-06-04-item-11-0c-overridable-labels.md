# Item 11.0c: User-Overridable Labels (i18n Compatibility)

## Problem

chorale v0.1.0 emits hardcoded English strings in its rendered UI: "Filter…",
"Go to", "of {n} pages", "Clear Filter", "All", "Select all", the pagination arrow glyphs.
A user who wants to ship chorale in a non-English app — or who wants to customize
these strings to match their design system's terminology — has no supported path today.

Adding a dedicated i18n library dependency (fluent-rs, i18next-rs, etc.) would violate
DEPS-1 and CHORALE-CORE-1. The correct approach: expose a `Labels` struct whose fields
cover every user-visible string the adapter emits. The host app provides translations by
constructing a `Labels` with values from whichever i18n library they use; chorale renders
whatever is in the struct.

`leptos-struct-table` ships an i18n prop (`locale: Signal<Locale>`). `table-rs` does not.
v0.2.0 chorale ships the same capability via a dependency-free `Labels` struct rather than
coupling to a specific framework's i18n system.

## Proposed Public API

### `chorale-core`

```rust
/// All user-visible strings the table renders. Override any field to customize.
/// `#[non_exhaustive]` so fields can be added in future minor releases without
/// breaking callers that construct `Labels { ..Labels::default() }`.
#[non_exhaustive]
pub struct Labels {
    // Filter bar
    pub filter_placeholder: String,     // "Filter…"
    pub clear_filter_label: String,     // "Clear Filter"

    // Pagination bar
    pub previous_page_label: String,    // "‹" (or "Previous")
    pub next_page_label: String,        // "›" (or "Next")
    pub go_to_page_label: String,       // "Go to"
    pub page_size_all_label: String,    // "All"
    pub page_count_of: String,          // "of" (rendered as "{page} of {total}")

    // Selection
    pub select_all_label: String,       // "Select all"
    pub deselect_all_label: String,     // "Deselect all"

    // Column visibility toolbar
    pub column_visibility_label: String, // "Columns"
    pub show_all_columns_label: String,  // "Show all"

    // CSV export
    pub export_csv_label: String,       // "Export CSV"

    // Sort (screen-reader text, not rendered visually)
    pub sort_ascending_label: String,   // "Sort ascending"
    pub sort_descending_label: String,  // "Sort descending"
    pub sort_none_label: String,        // "Unsorted"

    // Empty state
    pub no_rows_label: String,          // "No rows match the current filter."
}

impl Default for Labels {
    fn default() -> Self {
        Labels {
            filter_placeholder: "Filter…".into(),
            clear_filter_label: "Clear Filter".into(),
            previous_page_label: "‹".into(),
            next_page_label: "›".into(),
            go_to_page_label: "Go to".into(),
            page_size_all_label: "All".into(),
            page_count_of: "of".into(),
            select_all_label: "Select all".into(),
            deselect_all_label: "Deselect all".into(),
            column_visibility_label: "Columns".into(),
            show_all_columns_label: "Show all".into(),
            export_csv_label: "Export CSV".into(),
            sort_ascending_label: "Sort ascending".into(),
            sort_descending_label: "Sort descending".into(),
            sort_none_label: "Unsorted".into(),
            no_rows_label: "No rows match the current filter.".into(),
        }
    }
}
```

### `chorale-dioxus`

```rust
/// Optional labels override. Defaults to `Labels::default()` (English).
pub labels: Option<Labels>,
```

Adapter rendering replaces every hardcoded string with `labels.field_name`. The adapter
holds `let labels = props.labels.clone().unwrap_or_default();` at the top of the render.

Callsite shape for a French-language app:

```rust
rsx! {
    Table {
        handle: handle,
        labels: Some(Labels {
            filter_placeholder: t!("table.filter_placeholder"),  // leptos-i18n / any t! macro
            clear_filter_label: t!("table.clear_filter"),
            previous_page_label: "‹".into(),
            next_page_label: "›".into(),
            go_to_page_label: t!("table.go_to_page"),
            page_count_of: t!("table.of"),
            no_rows_label: t!("table.no_rows"),
            ..Labels::default()   // ok because Labels is #[non_exhaustive] within-crate;
                                  // cross-crate must use Labels::default() + field overrides
        }),
    }
}
```

Wait — `#[non_exhaustive]` blocks the struct-update syntax (`..Default`) cross-crate.
The host must call `Labels::default()` and then override individual fields:

```rust
let mut labels = Labels::default();
labels.filter_placeholder = t!("table.filter_placeholder");
labels.no_rows_label = t!("table.no_rows");
// ...
rsx! { Table { handle: handle, labels: Some(labels), ... } }
```

This is slightly more verbose but consistent: `#[non_exhaustive]` ensures future fields
added to `Labels` get their English defaults from `Labels::default()` rather than causing
a compile error at every cross-crate callsite. See open question #1 on whether to
provide a builder for `Labels`.

## Internal Design

`Labels` lives in `chorale-core` (not per-adapter) because the string set is identical
across adapters. Both `chorale-dioxus` and `chorale-leptos` render the same user-visible
strings; they both consume `Labels` from core.

The adapter's render receives `props.labels.clone().unwrap_or_default()` at the start of
each component render. No new signals. No new `TableState` fields — labels are a rendering
concern, not state.

Future: when a host app uses a reactive i18n library, the labels will be a derived value
of a locale signal. The host wraps the `Labels` construction in a `use_memo` and passes
the memoized `Labels` as the prop. This works naturally with Dioxus's prop system; no
special support from chorale needed.

## Backwards Compatibility

`Labels` is a new type in chorale-core, marked `#[non_exhaustive]` from the start.
Adding a new type is non-breaking.

The `labels: Option<Labels>` prop on `Table` defaults to `None`, which resolves to
`Labels::default()` (English). All existing `Table` callsites that do not supply `labels`
continue to render English strings — no behavior change.

Future additions to `Labels` (new fields for new UI affordances shipped in v0.3+) are
non-breaking because `Labels` is `#[non_exhaustive]`: callers that construct
`Labels::default()` and override specific fields will get the English default for any
new field they haven't overridden.

## Test Plan

Per TESTS-1 and ORCH-NEW-PATH-TESTS-1:

- `Labels::default()` contains the expected English strings for every field.
- Adapter renders `labels.filter_placeholder` in the filter input's placeholder, not
  a hardcoded string literal (static check / grep for string literals in component
  source after implementation).
- Adapter renders `labels.no_rows_label` when filtered row count is 0.
- Adapter renders `labels.previous_page_label` and `labels.next_page_label` in pagination.
- Providing a custom `Labels` with overridden `filter_placeholder` → the custom string
  appears in the rendered output (component test).
- `labels: None` prop → English defaults render (equivalence test with `labels:
  Some(Labels::default())`).

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **Should `Labels` provide a builder API (`Labels::new().filter_placeholder("...")
   .no_rows_label("...").build()`) instead of requiring `let mut labels = Labels::default();
   labels.foo = ...;`?** Recommendation: no builder — the field-mutation pattern is
   idiomatic Rust for `#[non_exhaustive]` structs, and a builder adds a non-trivial amount
   of boilerplate to generate. Document the field-mutation pattern in the rustdoc for
   `Labels`.

2. **Should parameterized strings (e.g., "of {n} pages") use format strings with
   placeholders or function fields?** The `page_count_of` field is currently just `"of"`;
   the adapter assembles `"{page} {labels.page_count_of} {total}"`. For full
   parameterization (e.g., German "Seite 3 von 10") the field approach can't reorder
   tokens. Recommendation: for v0.2.0, keep simple concatenation ("of") and document the
   limitation. A richer `Fn(usize, usize) -> String` field can be added in v0.3 for the
   parameterized case.

3. **`Labels` in `chorale-core` vs per-adapter struct.** Recommendation: `chorale-core`
   — the string set is identical for all adapters, and a single canonical type avoids
   duplication. The only argument for per-adapter structs is if adapters have genuinely
   different labels; in v0.2.0 they do not.

4. **Should the `labels` prop be `Labels` (non-optional, always required) rather than
   `Option<Labels>`?** Recommendation: `Option<Labels>` — forcing every callsite to supply
   a `Labels` would be a papercut for the 95% of users who just want English. Optional
   with a `Default` fallback is more ergonomic.

5. **Should `Labels` strings support Markdown or basic HTML for rich text (e.g., a `no_rows_label`
   with a link to "clear filters")?** Recommendation: plain `String` in v0.2.0. Rich text
   would require either a `VNode`-typed field (coupling core to Dioxus types) or a HTML
   escape layer. Deferred to v0.3.

## Decisions (signed off 2026-06-04)

4 of 5 recommendations accepted as written. Question #2 amended to the **layered
design**: simple `String` fields for non-parameterized labels, `Arc<dyn Fn>`
fields for parameterized labels. Ships in v0.2.0.

1. ✅ Field-mutation pattern (no builder).
2. ⚙️ **Layered design — ship in v0.2.0.** Non-parameterized fields stay
   `String` (`filter_placeholder`, `clear_filter_label`, `previous_page_label`,
   `next_page_label`, `go_to_page_label`, `page_size_all_label`,
   `select_all_label`, `deselect_all_label`, `column_visibility_label`,
   `show_all_columns_label`, `export_csv_label`, `sort_ascending_label`,
   `sort_descending_label`, `sort_none_label`, `no_rows_label`).

   The one parameterized field is upgraded:

   ```rust
   // BEFORE: pub page_count_of: String,  // "of"

   /// Renders the "page N of M" affordance. Receives (current_page, total_pages)
   /// and returns the full string. Default impl: `format!("{} of {}", page, total)`.
   /// Hosts override to support token-reordering languages
   /// (`|p, t| format!("{}ページ中{}ページ目", t, p)` for Japanese).
   pub page_count: Arc<dyn Fn(usize, usize) -> String + Send + Sync>,
   ```

   **`PartialEq` impl:** `Labels` cannot derive `PartialEq` because `dyn Fn`
   does not impl it. Hand-rolled impl compares all `String` fields by value
   equality and the `page_count` field by `Arc::ptr_eq`:

   ```rust
   impl PartialEq for Labels {
       fn eq(&self, other: &Self) -> bool {
           self.filter_placeholder == other.filter_placeholder
               && self.clear_filter_label == other.clear_filter_label
               // ... all other String fields ...
               && Arc::ptr_eq(&self.page_count, &other.page_count)
       }
   }
   ```

   `Arc::ptr_eq` compares the underlying pointer, not the closure body. Hosts
   memoize the `Labels` (e.g., `use_memo(move || Labels { ... })` in Dioxus)
   so the `Arc` is preserved across renders — the prop-equality check stays
   cheap and correct.

   **Default impl** uses `Arc::new(|page, total| format!("{} of {}", page, total))`.

   This is a v0.3-safe shape: adding more parameterized fields later is
   additive (`Labels` is `#[non_exhaustive]`).
3. ✅ `Labels` lives in `chorale-core`. Single canonical type across adapters.
4. ✅ `labels: Option<Labels>` prop. `None` falls back to `Labels::default()`
   (English).
5. ✅ Plain `String` for label content (no Markdown / HTML). Rich text
   deferred to v0.3.
