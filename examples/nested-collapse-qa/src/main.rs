//! Dedicated QA harness for the nested-grouping collapse bug (#36).
//!
//! Run with: `dx serve --package nested-collapse-qa` (requires `cargo install dioxus-cli`)
//!
//! Reproduces the camerata findings shape: a flat list grouped TWO levels deep
//! (RULE -> FILE), with the individual lines as leaf rows. Before the fix,
//! collapsing a depth-1 (FILE) header corrupted the depth-0 (RULE) header — the
//! parent appeared collapsed/indented and counts jumbled — because grouped rows
//! had no stable Dioxus key, so the positional diff patched a header `<tr>` (one
//! colspan cell) against a data `<tr>` (N cells). After the fix every grouped
//! row keys on its identity, so collapse/expand at any depth moves nodes by
//! identity instead of mis-patching structures.
//!
//! WHAT TO CHECK:
//!  1. Opens with all RULE groups COLLAPSED — just rule headers, each with a (count).
//!  2. Expand a rule -> its FILE sub-headers appear indented one level, each with
//!     its own (count). Expand a file -> its lines appear.
//!  3. Collapse a FILE (the grandchild header): ONLY that file collapses. Its
//!     parent RULE header stays at depth 0 (no indent shift) with its count intact,
//!     and sibling rules/files stay aligned.
//!  4. Repeat at both depths in any order — nothing should jumble or mis-indent.

use chorale_core::{CellValue, ColumnDef, ColumnId, PaginationMode, RowId, TableState};
use chorale_dioxus::{use_table, Table};
use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
struct Finding {
    rule: String,
    file: String,
    line: i64,
    detail: String,
}

/// Several rules, each spanning a few files, each file with a few lines — enough
/// breadth + depth to exercise collapse/expand at both grouping levels.
#[allow(clippy::type_complexity)]
fn findings() -> Vec<Finding> {
    let data: &[(&str, &[(&str, &[i64])])] = &[
        (
            "ARCH-STRICT-LAYERING-1",
            &[
                ("crates/api/src/handlers.rs", &[12, 41]),
                ("crates/api/src/main.rs", &[8]),
            ],
        ),
        (
            "ARCH-STRUCTURED-ERRORS-1",
            &[("crates/api/src/handlers.rs", &[41, 58, 73, 90])],
        ),
        (
            "RUST-DIOXUS-9",
            &[("crates/ui/src/page.rs", &[20, 34, 51, 67])],
        ),
        ("SQL-DB-INDEX-2", &[("crates/infra/src/lib.rs", &[15])]),
        (
            "RUST-SEAORM-RAW-SQL-ESCAPE-1",
            &[("crates/infra/src/repo.rs", &[22, 44, 61])],
        ),
    ];
    let mut out = Vec::new();
    for (rule, files) in data {
        for (file, lines) in *files {
            for line in *lines {
                out.push(Finding {
                    rule: (*rule).to_string(),
                    file: (*file).to_string(),
                    line: *line,
                    detail: format!("{rule} violated at {file}:{line}"),
                });
            }
        }
    }
    out
}

fn columns() -> Vec<ColumnDef<Finding>> {
    vec![
        ColumnDef::new(ColumnId("rule"), "Rule", |f: &Finding| {
            CellValue::Text(f.rule.clone())
        })
        .sortable(),
        ColumnDef::new(ColumnId("file"), "File", |f: &Finding| {
            CellValue::Text(f.file.clone())
        })
        .sortable(),
        ColumnDef::new(ColumnId("line"), "Line", |f: &Finding| {
            CellValue::Integer(f.line)
        })
        .sortable(),
        ColumnDef::new(ColumnId("detail"), "Detail", |f: &Finding| {
            CellValue::Text(f.detail.clone())
        }),
    ]
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let rows: Vec<(RowId, Finding)> =
        use_hook(|| findings().into_iter().map(|f| (RowId::new(), f)).collect());
    let handle = use_table(move || TableState::new(rows.clone(), columns()));
    use_hook(move || {
        // Group RULE -> FILE; load all, then collapse every group by default.
        handle.set_grouping(vec![ColumnId("rule"), ColumnId("file")]);
        handle.set_pagination_mode(PaginationMode::InfiniteScroll);
        let _ = handle.set_page_size(5000);
        handle.collapse_all_groups();
    });
    rsx! {
        div { style: "font-family: system-ui, sans-serif; padding: 1rem; max-width: 920px; margin: 0 auto;",
            h1 { "Nested-collapse QA (#36)" }
            p { style: "color: #444; line-height: 1.5;",
                "Grouped RULE → FILE → lines. Expand a rule, then collapse one of its FILE "
                "sub-headers: the parent RULE header must stay at depth 0 (no indent shift) with "
                "its count, and siblings stay aligned. Repeat at both depths in any order — "
                "nothing should jumble or mis-indent."
            }
            Table {
                handle,
                sort_enabled: true,
                filter_enabled: true,
                selection_enabled: true,
            }
        }
    }
}
