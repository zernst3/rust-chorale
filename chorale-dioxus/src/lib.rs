//! `chorale-dioxus`: Dioxus adapter for the `chorale-core` headless table.
//!
//! Renders a [`TableState<TRow>`] as a Dioxus component tree, including a
//! virtualized row container. The adapter owns rendering only; all state
//! mutation routes through `chorale-core` (CHORALE-DIOXUS-1).
//!
//! ## Relationship to `chorale-core`
//!
//! `chorale-core` is the framework-agnostic state layer (CHORALE-CORE-1).
//! `chorale-dioxus` wraps that state in a Dioxus [`Signal`] and exposes a
//! typed [`UseTableHandle`] whose methods dispatch pure state transitions from
//! `chorale-core`. The rendered `<Table>` reads the signal reactively.
//!
//! Other adapters (`chorale-leptos`, etc.) follow the same pattern against
//! the same `chorale-core` state; switching frameworks does not require
//! rewriting business logic.
//!
//! ## Quick start
//!
//! See the [repository README] for a full working example. The minimal pattern:
//!
//! ```rust,ignore
//! use chorale_core::{TableState, ColumnDef};
//! use chorale_dioxus::{use_table, Table};
//! use dioxus::prelude::*;
//!
//! #[component]
//! fn MyTable(rows: Vec<MyRow>) -> Element {
//!     let handle = use_table(|| TableState::new(
//!         rows.iter().map(|r| (RowId::new(), r.clone())).collect(),
//!         my_columns(),
//!     ));
//!     rsx! { Table { handle, sort_enabled: true, filter_enabled: true } }
//! }
//! ```
//!
//! [`TableState<TRow>`]: chorale_core::TableState
//! [`Signal`]: dioxus::prelude::Signal
//! [repository README]: https://github.com/zernst3/rust-chorale#quick-start

#![warn(missing_docs)]

mod components;
mod hooks;

/// The primary Dioxus table component.
///
/// Renders sort headers, an optional filter row, pagination controls,
/// a virtualized row container, optional selection checkboxes, column
/// visibility toolbar, CSV export button, and column resize handles.
///
/// All props except `handle` have sensible defaults so you can start minimal
/// and enable features incrementally.
///
/// ```rust,ignore
/// rsx! {
///     Table {
///         handle,
///         sort_enabled: true,
///         filter_enabled: true,
///         selection_enabled: true,
///         column_toolbar: true,
///         csv_export: true,
///         resize_enabled: true,
///     }
/// }
/// ```
///
/// See each prop's doc-comment in the `TableProps` struct for defaults and
/// visual effect.
pub use components::Table;

/// Button component that exports the current filtered+sorted view as an XLSX
/// file. Requires the `xlsx` feature on both `chorale-dioxus` and
/// `chorale-core`. See [`components::ExportXlsxButton`] for prop details.
#[cfg(feature = "xlsx")]
pub use components::ExportXlsxButton;

/// Type-erased cell renderer: maps a [`CellValue`] to a Dioxus [`Element`].
///
/// `Arc<dyn Fn(&CellValue) -> Element + Send + Sync + 'static>`
///
/// Pass one or more of these in a [`CellRenderers`] map to override the
/// built-in rendering for specific columns.
///
/// [`CellValue`]: chorale_core::CellValue
/// [`Element`]: dioxus::prelude::Element
pub use components::CellRenderer;

/// Per-column map of custom cell renderers.
///
/// Build with `CellRenderers::new(map)` where `map: HashMap<ColumnId, CellRenderer>`.
/// Pass to the `cell_renderers` prop of [`Table`] to override the built-in
/// rendering for specific columns.
///
/// ```rust,ignore
/// use std::{collections::HashMap, sync::Arc};
/// use chorale_dioxus::{CellRenderer, CellRenderers};
/// use chorale_core::ColumnId;
/// use dioxus::prelude::*;
///
/// let renderers = CellRenderers::new(HashMap::from([
///     (ColumnId("status"), Arc::new(|val| rsx! {
///         span { class: "badge", "{val}" }
///     }) as CellRenderer),
/// ]));
/// ```
pub use components::CellRenderers;

/// Create a reactive chorale table handle backed by a Dioxus signal.
///
/// Call inside a component to obtain a [`UseTableHandle<TRow>`]. Pass the
/// handle to [`Table`] and use its typed methods to dispatch transitions from
/// parent components or event handlers.
///
/// `init` is called once on component mount to produce the initial
/// [`TableState`]. The returned handle is [`Copy`] (thin signal reference)
/// so closures can capture it without `.clone()`.
///
/// ```rust,ignore
/// let handle = use_table(|| TableState::new(rows, columns));
/// // Read the current selection count reactively:
/// let count = handle.signal().read().selection.len();
/// ```
///
/// [`TableState`]: chorale_core::TableState
pub use hooks::use_table;

/// Reactive handle returned by [`use_table`].
///
/// Wraps a `Signal<TableState<TRow>>` and exposes typed methods for every
/// state transition: `toggle_sort`, `set_filter`, `set_page`, `set_page_size`,
/// `set_selection`, `toggle_select_all`, `set_column_visibility`,
/// `set_column_width`, `set_scroll`, `update_row`.
///
/// `UseTableHandle<TRow>` is [`Copy`] (Signal is a thin reference into a
/// generational arena) so it can be captured by value in Dioxus closures
/// without requiring `.clone()`.
///
/// ```rust,ignore
/// // In an event handler:
/// onclick: move |_| { handle.toggle_sort(ColumnId("name")); }
/// ```
pub use hooks::UseTableHandle;
