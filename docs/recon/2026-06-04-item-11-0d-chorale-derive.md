# Item 11.0d: `chorale-derive` Proc-Macro Crate

## Problem

chorale v0.1.0 requires every caller to hand-write a `Vec<ColumnDef<TRow>>` array. For
a struct with 10 fields, this is 10 `ColumnDef::new(...)` calls with manually specified
IDs, headers, and accessor closures. The boilerplate is mechanical and error-prone:
mismatched IDs, typos in headers, forgetting `.sortable()` on a column that should sort.

`leptos-struct-table` ships `#[derive(TableRow)]` as its primary API surface — the derive
macro generates the column definitions automatically. This is a major ergonomics advantage
for users who own their row struct and want sensible defaults.

The explicit `Vec<ColumnDef<TRow>>` API remains canonical and is NOT replaced. `chorale-derive`
is additive opt-in sugar. Users who need custom types, computed accessors, or columns that
don't map 1:1 to struct fields will continue using the explicit API.

## Proposed Public API

### `chorale-derive` crate

New workspace crate: `chorale-derive`. This is a `proc-macro` crate (Cargo category
`proc-macro = true`). It depends on `chorale-core` (to reference the core types it
generates code for) and on `syn`, `quote`, and `proc-macro2`.

```rust
/// Derive macro for automatic ColumnDef generation.
/// Applied to a struct that is used as the TRow type in TableState<TRow>.
#[proc_macro_derive(TableRow, attributes(chorale))]
pub fn table_row_derive(input: TokenStream) -> TokenStream;
```

Generated output for a simple struct:

```rust
#[derive(TableRow, Clone, PartialEq)]
pub struct Invoice {
    pub id: u64,
    pub customer: String,
    pub amount: f64,
    pub due_date: chrono::NaiveDate,
    #[chorale(skip)]
    pub internal_notes: String,
    #[chorale(header = "Due", filter = "Date")]
    pub due_date: chrono::NaiveDate,
}
```

Generates:

```rust
impl Invoice {
    pub fn chorale_columns() -> Vec<ColumnDef<Invoice>> {
        vec![
            ColumnDef::new("id", "Id", |r| CellValue::Number(r.id as f64))
                .sortable(),
            ColumnDef::new("customer", "Customer", |r| CellValue::Text(r.customer.clone()))
                .sortable()
                .filter(FilterKind::Text),
            ColumnDef::new("amount", "Amount", |r| CellValue::Number(r.amount))
                .sortable()
                .filter(FilterKind::NumericRange),
            ColumnDef::new("due_date", "Due", |r| CellValue::Text(r.due_date.to_string()))
                .sortable()
                .filter(FilterKind::Date),
            // internal_notes skipped
        ]
    }
}
```

### Field-level attribute grammar

All attributes are under the `#[chorale(...)]` namespace:

| Attribute | Values | Notes |
|---|---|---|
| `skip` | (flag) | Exclude this field from generated columns |
| `header = "..."` | string literal | Override the column header (default: field name in Title Case) |
| `initial_width = N.0` | f64 literal | Set `ColumnDef::initial_width` |
| `sortable = false` | bool | Default: `true` for comparable types |
| `filter = "Text"` | `"Text"`, `"NumericRange"`, `"Date"`, `"Boolean"`, `"MultiSelect"` | Override inferred filter kind |
| `options = ["a", "b"]` | string array | Required for `filter = "MultiSelect"` (static list) |
| `align = "Left"` | `"Left"`, `"Center"`, `"Right"` | Override `Alignment` |
| `render = "Badge"` | `"Badge"`, `"Link"`, `"Custom"` | Override `RenderKind` |

### Type inference rules

The derive macro infers `FilterKind` and `CellValue` type from the Rust field type:

| Rust type | `CellValue` | Inferred `FilterKind` |
|---|---|---|
| `String`, `&str` | `Text` | `Text` |
| `u8..u128`, `i8..i128`, `f32`, `f64` | `Number` | `NumericRange` |
| `bool` | `Boolean(v)` | `Boolean` |
| `chrono::NaiveDate` | `Text(date.to_string())` | `Date` |
| `rust_decimal::Decimal` | `Number(v.to_f64())` | `NumericRange` |
| Any `Option<T>` | unwrapped or `CellValue::Text("")` | inherits from `T` |
| Any other type | `Text(v.to_string())` | `Text` (requires `Display` bound) |

## Internal Design

**Crate topology:** `chorale-derive` is a `proc-macro = true` workspace member at
`chorale-derive/`. It has no runtime dependency on `chorale-dioxus`; it generates code
that calls `chorale_core::ColumnDef::new(...)`. The generated `chorale_columns()` method
lives in the domain crate (the crate that defines the struct), not in chorale-derive.

**`MultiSelect` options:** for `filter = "MultiSelect"`, the options must be a static
`&[&str]` array in the attribute. At runtime the derive-generated code calls
`FilterKind::MultiSelect(options.iter().map(|s| s.to_string()).collect())`. Dynamic
options (from a database query) require the explicit `ColumnDef` API and cannot be
expressed in a derive attribute; document this clearly.

**Generic `TRow` support:** the derive macro only supports concrete (non-generic) structs.
Generic structs (`pub struct Row<T>`) require the explicit `ColumnDef<Row<T>>` API. The
derive macro emits a compile error on generic input.

**Versioning:** `chorale-derive` is versioned independently of `chorale-core`. Its major
version tracks the minimum `chorale-core` version it targets. v0.2.0 scope: `chorale-derive`
publishes at `0.1.0` (not `0.2.0`), since this is its first release.

## Backwards Compatibility

New crate; no existing callers. No compatibility concerns.

The explicit `Vec<ColumnDef<TRow>>` API in `chorale-core` is unmodified. Users who do not
opt into `chorale-derive` see no change.

## Test Plan

Per TESTS-1 and ORCH-NEW-PATH-TESTS-1. Derive macro testing uses golden-file snapshots
(a common pattern for proc-macro crates: serialize the generated token stream and compare
against a stored `expected.rs` file).

- Basic struct with `String`, `f64`, `bool` fields → generated code matches golden file.
- `#[chorale(skip)]` on a field → that field absent from generated vec.
- `#[chorale(header = "Custom")]` → generated `ColumnDef::new(id, "Custom", ...)`.
- `#[chorale(filter = "Text")]` on a numeric field → overrides inferred `NumericRange`.
- `#[chorale(filter = "MultiSelect", options = ["A", "B"])]` → generated
  `FilterKind::MultiSelect(vec!["A".into(), "B".into()])`.
- Generic struct → compile error (not a panic; check error message text).
- `Option<String>` field → accessor unwraps or returns empty string.
- chrono `NaiveDate` field → inferred `FilterKind::Date`.
- Generated `chorale_columns()` return value is identical to the hand-written equivalent
  for the reference struct (round-trip test).

## Open Questions / Decisions Zach Must Sign Off (API-1)

1. **Should `chorale_columns()` be a free function or a trait method?** Recommendation:
   inherent method on the struct (`impl Invoice { pub fn chorale_columns() -> Vec<ColumnDef<Self>> }`).
   A trait (`trait TableRow { fn chorale_columns() -> Vec<ColumnDef<Self>>; }`) is
   cleaner for generic bounds but adds a public trait to chorale-core that may be
   over-engineered for v0.2.0. Implement as inherent method; a trait can be added in
   v0.3 if needed.

2. **The `chorale-derive` crate version: `0.1.0` or track `chorale-core` at `0.2.0`?**
   Recommendation: `0.1.0` for the first publish — it's a new crate at its own v0.1. Track
   `chorale-core` `^0.2` as a dependency. Semver-wise, `chorale-derive 0.1.0` requires
   `chorale-core >=0.2.0`.

3. **Should the derive macro accept `Display`-only types without a filter (currently
   inferred as `FilterKind::Text`)?** Recommendation: yes, with a note in the generated
   code's rustdoc that filter results for custom types compare against `Display` output.
   If the host app doesn't want filtering on a custom-type column, they add
   `#[chorale(filter = "none")]` (or a similar opt-out attribute; Zach decides the
   keyword).

4. **Should `chorale-derive` gate `chrono` support behind a Cargo feature flag?**
   Recommendation: yes — `features = ["chrono"]`. The feature enables the `NaiveDate`
   inference rules and adds `chrono` as an optional dependency of `chorale-derive`.
   Without the feature, `NaiveDate` fields fall back to the `Display`/`Text` path.

5. **Should `#[derive(TableRow)]` enforce that the struct also derives `Clone + PartialEq`
   (which `TableState<TRow>` requires via its bounds)?** Recommendation: yes — emit a
   compile error if the struct doesn't satisfy the `TRow: Clone + PartialEq` bounds.
   This gives a clear error at derive site rather than a confusing monomorphization error
   at the `TableState::new` call site.

## Decisions (signed off 2026-06-04)

All 5 recommendations accepted. Opt-out keyword resolved to `filter = "none"`.

1. ✅ Inherent method on the struct (`impl Invoice { pub fn chorale_columns() }`).
   `TableRow` trait deferred to v0.3.
2. ✅ `chorale-derive` publishes at `0.1.0`. Depends on `chorale-core ^0.2`.
3. ✅ `Display`-only types default to `FilterKind::Text` (filter compares against
   `Display` output). **Opt-out attribute: `#[chorale(filter = "none")]`.**
   Consistent with the existing `filter = "Text"` / `filter = "Date"` family —
   one key for all filter concerns, value selects the kind. The string `"none"`
   is a sentinel; not a real `FilterKind` variant.
4. ✅ `chrono` support gated behind Cargo feature `chrono`. Without the
   feature, `NaiveDate` fields fall back to `Display`/`Text`.
5. ✅ Enforce `Clone + PartialEq` at derive site with a compile error.
