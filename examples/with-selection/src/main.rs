//! Row selection example: per-row checkboxes + header select-all + a live
//! count of the selection. Demonstrates the `selected_ids()` and
//! `selection_count()` convenience methods added in v0.2.0 (Item 3).
//!
//! Run with: `dx serve --package with-selection`

use chorale_core::{
    Alignment, CellValue, ColumnDef, ColumnId, FilterKind, RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, Table};
use dioxus::prelude::*;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct Task {
    title: String,
    assignee: String,
}

fn tasks() -> Vec<(RowId, Task)> {
    [
        ("Wire up auth flow", "Alice"),
        ("Refactor checkout", "Bob"),
        ("Migrate to Postgres 16", "Charlie"),
        ("Add email digest", "Diana"),
        ("Fix N+1 in feed", "Ethan"),
        ("Roll out feature flag", "Fiona"),
        ("Patch CVE-2026-1234", "George"),
    ]
    .into_iter()
    .map(|(t, a)| {
        (
            RowId::new(),
            Task {
                title: t.into(),
                assignee: a.into(),
            },
        )
    })
    .collect()
}

fn columns() -> Vec<ColumnDef<Task>> {
    vec![
        ColumnDef {
            id: ColumnId("title"),
            header: "Task".into(),
            accessor: Arc::new(|t: &Task| CellValue::Text(t.title.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(280.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
        ColumnDef {
            id: ColumnId("assignee"),
            header: "Assignee".into(),
            accessor: Arc::new(|t: &Task| CellValue::Text(t.assignee.clone())),
            sortable: true,
            filter: FilterKind::Text,
            initial_width: Some(140.0),
            alignment: Alignment::Left,
            render_kind: RenderKind::Text,
            header_class: None,
            cell_class: None,
        },
    ]
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(tasks(), columns()));

    // Use the v0.2 convenience methods instead of reading the signal directly.
    let selected_count = table.selection_count();
    let selected_ids = table.selected_ids();

    rsx! {
        div { style: "font-family: sans-serif; padding: 1rem; max-width: 800px; margin: 0 auto;",
            h1 { "Selection example" }
            p { "Click row checkboxes to select. The header checkbox toggles select-all for the visible page." }
            p { strong { "Selected: " } "{selected_count} row(s)" }
            if !selected_ids.is_empty() {
                p { style: "font-size: 0.8rem; color: #666;",
                    "IDs: "
                    {selected_ids.iter().map(|id| rsx! {
                        code { style: "margin-right: 6px; font-size: 0.75rem;",
                            "{id.as_uuid()}"
                        }
                    })}
                }
            }
            Table {
                handle: table,
                sort_enabled: true,
                selection_enabled: true,
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}
