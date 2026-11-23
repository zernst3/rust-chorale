//! Hand-test page for the `group_header_renderer` hook.
//!
//! Run with: `dx serve --package group-header-hook`. See README.md for the
//! numbered "what to try" list.

use chorale_core::{CellValue, ColumnDef, ColumnId, GroupedPaginationMode, RowId, TableState};
use chorale_dioxus::{use_table, GroupHeaderContext, Table};
use dioxus::prelude::*;

const GROUPS: usize = 300;
const ROWS_PER_GROUP: usize = 10;

#[derive(Clone, PartialEq)]
struct Item {
    group: String,
    name: String,
    qty: i64,
}

fn dataset() -> Vec<(RowId, Item)> {
    (0..GROUPS * ROWS_PER_GROUP)
        .map(|i| {
            (
                RowId::new(),
                Item {
                    group: format!("Group {:03}", i / ROWS_PER_GROUP),
                    name: format!("Item {i}"),
                    qty: i64::try_from(i % 97).unwrap_or(0),
                },
            )
        })
        .collect()
}

fn columns() -> Vec<ColumnDef<Item>> {
    vec![
        ColumnDef::new(ColumnId("group"), "Group", |r: &Item| {
            CellValue::Text(r.group.clone())
        }),
        ColumnDef::new(ColumnId("name"), "Name", |r: &Item| {
            CellValue::Text(r.name.clone())
        }),
        ColumnDef::new(ColumnId("qty"), "Qty", |r: &Item| CellValue::Integer(r.qty)),
    ]
}

/// Delay without a timer crate: the browser's `setTimeout` through eval.
async fn sleep_ms(ms: u32) {
    let _ = dioxus::document::eval(&format!(
        "await new Promise(r => setTimeout(r, {ms})); return 0;"
    ))
    .await;
}

#[component]
fn App() -> Element {
    let table = use_table(|| {
        let mut s = TableState::new(dataset(), columns());
        s.grouping = vec![ColumnId("group")];
        s.page_size = 50;
        s
    });
    let mut hook_on = use_signal(|| true);
    let mut selection_on = use_signal(|| false);
    let mut mounted = use_signal(|| true);
    let mut focus_desc = use_signal(|| "(nothing focused)".to_string());
    let mut remount_log = use_signal(String::new);

    // Show the focused element so a tester can see where Tab lands.
    use_future(move || async move {
        let mut ev = dioxus::document::eval(
            r"
            const d = e => {
              const el = document.activeElement;
              if (!el || el === document.body) return '(nothing focused)';
              let s = el.tagName.toLowerCase();
              if (el.className && typeof el.className === 'string') s += '.' + el.className.split(' ')[0];
              if (el.getAttribute('aria-label')) s += ' [' + el.getAttribute('aria-label') + ']';
              if (el.id) s += ' #' + el.id;
              return s;
            };
            document.addEventListener('focusin', e => dioxus.send(d(e)));
            document.addEventListener('focusout', e => setTimeout(() => dioxus.send(d(e)), 0));
            ",
        );
        while let Ok(v) = ev.recv::<String>().await {
            focus_desc.set(v);
        }
    });

    // Host renderer: text, a select, a button and a count. `use_callback` keeps
    // its identity stable across renders.
    let header_cb = use_callback(move |ctx: GroupHeaderContext| {
        let key = ctx.key.clone();
        rsx! {
            strong { "{ctx.label} " }
            span { style: "color: #6b7280;", "({ctx.row_count} rows) " }
            select {
                aria_label: "Group action",
                option { "Keep" }
                option { "Review" }
                option { "Drop" }
            }
            button {
                r#type: "button",
                aria_label: "Host toggle",
                onclick: move |_| table.toggle_group(key.clone()),
                if ctx.is_collapsed { "Open" } else { "Close" }
            }
        }
    });

    let state = table.signal();
    let scroll_top = state.read().scroll_top;
    let active = state.read().active_cell.map(|a| a.row_idx);
    let virtualized = state.read().grouped_pagination == GroupedPaginationMode::Virtualized;

    rsx! {
        div { style: "font-family: sans-serif; padding: 1rem; max-width: 1000px; margin: 0 auto;",
            h1 { "Group header hook" }
            p { "{GROUPS} groups, {GROUPS * ROWS_PER_GROUP} rows. See the README for what to try." }

            fieldset {
                legend { "Hook and pagination" }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *hook_on.read(),
                        onchange: move |_| { let v = *hook_on.read(); hook_on.set(!v); },
                    }
                    " group_header_renderer on (off renders the 0.2.3 default header) "
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *selection_on.read(),
                        onchange: move |_| { let v = *selection_on.read(); selection_on.set(!v); },
                    }
                    " Selection "
                }
                label {
                    " Grouped pagination: "
                    select {
                        aria_label: "Grouped pagination",
                        value: if virtualized { "virtualized" } else { "paged" },
                        onchange: move |e| table.set_grouped_pagination(if e.value() == "virtualized" {
                            GroupedPaginationMode::Virtualized
                        } else {
                            GroupedPaginationMode::DataRowsOnly
                        }),
                        option { value: "paged", "Paged (DataRowsOnly)" }
                        option { value: "virtualized", "Virtualized" }
                    }
                }
            }

            fieldset {
                legend { "Expand and collapse" }
                button { r#type: "button", onclick: move |_| table.expand_all_groups(), "Expand all" }
                " "
                button { r#type: "button", onclick: move |_| table.collapse_all_groups(), "Collapse all" }
            }

            fieldset {
                legend { "Scroll restore on remount" }
                button {
                    r#type: "button",
                    onclick: move |_| {
                        spawn(async move {
                            let before = table.signal().read().scroll_top;
                            mounted.set(false);
                            sleep_ms(300).await;
                            mounted.set(true);
                            sleep_ms(500).await;
                            let after = table.signal().read().scroll_top;
                            remount_log.set(format!("scroll_top before: {before}, after remount: {after}"));
                        });
                    },
                    "Unmount and remount table"
                }
                " {remount_log}"
            }

            div {
                style: "margin: 0.5rem 0; padding: 0.5rem; background: #eef2ff; font-family: monospace; font-size: 0.85rem;",
                div { "Focused element: {focus_desc}" }
                div { "Active cell row index: {active:?}" }
                div { "scroll_top: {scroll_top}" }
            }

            if *mounted.read() {
                Table {
                    handle: table,
                    selection_enabled: *selection_on.read(),
                    group_header_renderer: if *hook_on.read() { Some(header_cb) } else { None },
                }
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}
