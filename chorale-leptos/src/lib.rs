//! `chorale-leptos`: Leptos adapter for the `chorale-core` headless table.
//!
//! Renders a [`TableState<TRow>`] as a Leptos component tree, including a
//! virtualized row container. The adapter owns rendering only; all state
//! mutation routes through `chorale-core` (CHORALE-CORE-1).
//!
//! ## Relationship to `chorale-core`
//!
//! `chorale-core` is the framework-agnostic state layer (CHORALE-CORE-1).
//! `chorale-leptos` wraps that state in a Leptos [`RwSignal`] and exposes a
//! typed [`UseTableHandle`] whose methods dispatch pure state transitions from
//! `chorale-core`. The rendered `<Table>` reads the signal reactively.
//!
//! `chorale-dioxus` follows the same pattern; switching framework adapters
//! does not require rewriting business logic.
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use chorale_core::{ColumnDef, CellValue};
//! use chorale_leptos::{use_chorale_table, Table};
//! use leptos::prelude::*;
//!
//! #[component]
//! fn MyTable(rows: Vec<MyRow>) -> impl IntoView {
//!     let handle = use_chorale_table(rows, my_columns());
//!     view! { <Table handle=handle sort_enabled=true filter_enabled=true /> }
//! }
//! ```
//!
//! [`TableState<TRow>`]: chorale_core::TableState
//! [`RwSignal`]: leptos::prelude::RwSignal

#![warn(missing_docs)]

mod components;
mod hooks;

/// The primary Leptos table component.
///
/// Renders sort headers, an optional filter row, pagination controls,
/// a virtualized row container, optional selection checkboxes, column
/// visibility toolbar, CSV export button, and column resize handles.
///
/// All props except `handle` have sensible defaults so you can start minimal
/// and enable features incrementally.
///
/// ```rust,ignore
/// view! {
///     <Table
///         handle=handle
///         sort_enabled=true
///         filter_enabled=true
///         selection_enabled=true
///         column_toolbar=true
///         csv_export=true
///         resize_enabled=true
///     />
/// }
/// ```
pub use components::Table;

/// Button that exports the current filtered+sorted view as an XLSX file.
///
/// Requires the `xlsx` feature plus a `wasm32` target (the click handler
/// uses browser download APIs). Mirrors the `chorale-dioxus` re-export of
/// the same component; without this re-export the component was
/// unreachable by consumers (the `components` module is private).
#[cfg(all(feature = "xlsx", target_arch = "wasm32"))]
pub use components::ExportXlsxButton;

/// Type-erased cell renderer: maps a [`CellValue`] to a Leptos [`AnyView`].
///
/// Build with `Arc::new(|val: &CellValue| view! { ... }.into_any())` and pass
/// via [`CellRenderers::new`].
///
/// [`CellValue`]: chorale_core::CellValue
/// [`AnyView`]: leptos::prelude::AnyView
pub use components::CellRenderer;

/// Per-column map of custom cell renderers.
///
/// Build with `CellRenderers::new(HashMap::from([...]))` where values are
/// [`CellRenderer`] closures. Pass to the `cell_renderers` prop of [`Table`]
/// to override the built-in rendering for specific columns.
pub use components::CellRenderers;

/// Per-row detail renderer: takes ownership of a row and returns a Leptos
/// [`AnyView`].
///
/// Build with `Arc::new(|row: MyRow| view! { ... }.into_any())` and pass via
/// the `detail_renderer` prop of [`Table`]. Without this re-export the type
/// was unreachable by consumers (the `components` module is private), making
/// the `detail_renderer` prop impossible to construct by name.
///
/// [`AnyView`]: leptos::prelude::AnyView
pub use components::DetailRenderer;

/// Type-erased row-aware cell renderer: maps the full row plus the cell's
/// [`CellValue`] to a Leptos [`AnyView`].
///
/// Build with `Arc::new(|row: &MyRow, val: &CellValue| view! { ... }.into_any())`
/// and register via [`RowCellRenderers::new`]. Use this when the cell needs
/// data from other fields on the row: composite cells, action columns, link cells.
///
/// [`CellValue`]: chorale_core::CellValue
/// [`AnyView`]: leptos::prelude::AnyView
pub use components::RowCellRenderer;

/// Per-column map of row-aware cell renderers.
///
/// Build with `RowCellRenderers::new(HashMap::from([...]))` where values are
/// [`RowCellRenderer`] closures. Pass to the `row_cell_renderers` prop of
/// [`Table`]. Entries take precedence over `cell_renderers` and `RenderKind`.
pub use components::RowCellRenderers;
/// Per-row conditional styling hook for the [`Table`]'s `row_class` prop, plus its closure
/// type. Build with `RowClass::new(|row| ...)`; the class is appended to that row's `<tr>`.
pub use components::{RowClass, RowClassFn};

/// Optional synchronous validation function for in-cell editing.
///
/// Build with `ValidateEditFn::new(|v| { ... })`. Default is no-op (all
/// commits are allowed).
pub use components::ValidateEditFn;

/// Input passed to the `validate_edit` callback before a cell edit is committed.
pub use components::EditValidation;

/// Create a reactive Chorale table handle backed by a Leptos `RwSignal`.
///
/// Call inside a Leptos component to obtain a [`UseTableHandle<TRow>`]. Pass
/// the handle to [`Table`] and use its typed methods to dispatch transitions
/// from parent components or event handlers.
///
/// `rows` and `columns` define the initial data; each row is assigned a new
/// random [`RowId`]. The returned handle is [`Copy`] (thin `RwSignal` wrapper)
/// so closures can capture it without `.clone()`.
///
/// ```rust,ignore
/// let handle = use_chorale_table(rows, my_columns());
/// let count = handle.signal().with_untracked(|s| s.rows.len());
/// ```
///
/// [`RowId`]: chorale_core::RowId
pub use hooks::use_chorale_table;

/// Reactive handle returned by [`use_chorale_table`].
///
/// Wraps a `RwSignal<TableState<TRow>>` and exposes typed methods for every
/// state transition: `toggle_sort`, `set_filter`, `set_page`, `set_page_size`,
/// `set_selection`, `toggle_select_all`, `set_column_visibility`,
/// `set_column_width`, `set_scroll`, `update_row`, and more.
///
/// `UseTableHandle<TRow>` is [`Copy`] (thin `RwSignal` reference) so it can
/// be captured by value in Leptos closures without requiring `.clone()`.
///
/// ```rust,ignore
/// // In an event handler:
/// on:click=move |_| { handle.toggle_sort(ColumnId("name"), SortAction::Replace); }
/// ```
pub use hooks::UseTableHandle;
