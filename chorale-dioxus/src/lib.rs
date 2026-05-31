//! chorale-dioxus: Dioxus adapter for `chorale-core`.
//!
//! Renders a `chorale_core::TableState<TRow>` as a Dioxus component
//! tree, including a virtualized row container. The adapter owns
//! rendering only — all state mutation routes through `chorale-core`.

mod components;
mod hooks;

pub use components::{CellRenderer, CellRenderers, Table};
pub use hooks::{use_table, UseTableHandle};
