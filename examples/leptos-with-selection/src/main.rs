//! Row selection example: per-row checkboxes + header select-all + a live
//! count of the selection. Demonstrates the `selected_ids()` and
//! `selection_count()` convenience methods.
//!
//! Run with: `trunk serve --open --package leptos-with-selection`

use chorale_core::{CellValue, ColumnDef, ColumnId, FilterKind};
use chorale_leptos::{use_chorale_table, Table};
use leptos::prelude::*;

#[derive(Clone, PartialEq)]
struct Task {
    title: String,
    assignee: String,
}

fn tasks() -> Vec<Task> {
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
    .map(|(t, a)| Task {
        title: t.into(),
        assignee: a.into(),
    })
    .collect()
}

fn columns() -> Vec<ColumnDef<Task>> {
    vec![
        ColumnDef::new(ColumnId("title"), "Task", |t: &Task| {
            CellValue::Text(t.title.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(280.0),
        ColumnDef::new(ColumnId("assignee"), "Assignee", |t: &Task| {
            CellValue::Text(t.assignee.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(140.0),
    ]
}

#[component]
fn App() -> impl IntoView {
    let table = use_chorale_table(tasks(), columns());

    view! {
        <div style="font-family: sans-serif; padding: 1rem; max-width: 800px; margin: 0 auto;">
            <h1>"Selection example"</h1>
            <p>"Click row checkboxes to select. The header checkbox toggles select-all for the visible page."</p>
            <p>
                <strong>"Selected: "</strong>
                {move || table.selection_count()}
                " row(s)"
            </p>
            {move || {
                let ids = table.selected_ids();
                if ids.is_empty() {
                    None
                } else {
                    Some(view! {
                        <p style="font-size: 0.8rem; color: #666;">
                            "IDs: "
                            {ids.iter().map(|id| view! {
                                <code style="margin-right: 6px; font-size: 0.75rem;">
                                    {id.as_uuid().to_string()}
                                </code>
                            }).collect::<Vec<_>>()}
                        </p>
                    })
                }
            }}
            <Table handle=table sort_enabled=true selection_enabled=true on_commit_edit=None />
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
