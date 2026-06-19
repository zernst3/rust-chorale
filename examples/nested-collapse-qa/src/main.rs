//! Combined QA harness for the **v0.2.3** changes.
//!
//! Run with: `dx serve --package nested-collapse-qa` (requires `cargo install dioxus-cli`).
//!
//! Originally built for the nested-grouping collapse bug (#36), now wired to
//! exercise every functional v0.2.3 change in one place:
//!
//!  - #36 stable keys for grouped rows (collapse no longer corrupts the parent)
//!  - #31 per-group "select all" (tri-state group-header checkbox, filter-aware)
//!  - #33 badge palette: blue / purple / orange + a custom-color escape hatch
//!  - #32 per-row conditional styling hook (`row_class`)
//!  - #35 set-filter with OR-contains for list-valued cells (`MultiSelectContains`)
//!  - #34 is docs-only (the `Send + Sync` interactive-cell pattern) — nothing to click.
//!
//! The data is two-level grouped (RULE -> FILE) with line-level leaf rows, plus a
//! per-row Severity (badge) and a list-valued Tags column (set-filter). See the
//! on-page notes for what to check per item.

use chorale_core::{
    BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId, FilterKind, PaginationMode,
    RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, RowClass, Table};
use dioxus::prelude::*;

/// Page CSS, passed to `dangerous_inner_html` as an expression (not an rsx
/// string literal, which would be parsed as a format string and choke on the
/// CSS braces). Defines the `teal` custom badge color (#33 escape hatch) and
/// the `danger-row` style applied by `row_class` (#32).
const HARNESS_CSS: &str = r"
:root {
  --chorale-badge-teal-bg: #ccfbf1;
  --chorale-badge-teal-text: #0f766e;
}
tr.danger-row td { background: #fff1f2; }
tr.danger-row td:first-child { box-shadow: inset 4px 0 0 #dc2626; }
";

#[derive(Clone, PartialEq)]
struct Finding {
    rule: String,
    file: String,
    line: i64,
    /// One of: blocker / high / medium / low / info — rendered as a Severity badge (#33).
    severity: String,
    /// Comma-joined list-valued cell, e.g. "api, security" — filtered via set-filter (#35).
    tags: String,
    detail: String,
}

/// Several rules, each spanning a few files, each file with a few lines — enough
/// breadth + depth to exercise collapse/expand at both grouping levels. Each rule
/// also carries a severity (badge color) and a list of tags (set-filter).
#[allow(clippy::type_complexity)]
fn findings() -> Vec<Finding> {
    // (rule, severity, tags, [(file, [lines])])
    let data: &[(&str, &str, &str, &[(&str, &[i64])])] = &[
        (
            "ARCH-STRICT-LAYERING-1",
            "high",
            "api, style",
            &[
                ("crates/api/src/handlers.rs", &[12, 41]),
                ("crates/api/src/main.rs", &[8]),
            ],
        ),
        (
            "ARCH-STRUCTURED-ERRORS-1",
            "medium",
            "api, security",
            &[("crates/api/src/handlers.rs", &[41, 58, 73, 90])],
        ),
        (
            "RUST-DIOXUS-9",
            "low",
            "ui, style",
            &[("crates/ui/src/page.rs", &[20, 34, 51, 67])],
        ),
        (
            "SQL-DB-INDEX-2",
            "blocker",
            "db, perf",
            &[("crates/infra/src/lib.rs", &[15])],
        ),
        (
            "RUST-SEAORM-RAW-SQL-ESCAPE-1",
            "info",
            "db, security",
            &[("crates/infra/src/repo.rs", &[22, 44, 61])],
        ),
    ];
    let mut out = Vec::new();
    for (rule, severity, tags, files) in data {
        for (file, lines) in *files {
            for line in *lines {
                out.push(Finding {
                    rule: (*rule).to_string(),
                    file: (*file).to_string(),
                    line: *line,
                    severity: (*severity).to_string(),
                    tags: (*tags).to_string(),
                    detail: format!("{rule} violated at {file}:{line}"),
                });
            }
        }
    }
    out
}

/// Severity -> badge variant map (#33). Uses the three new first-class colors
/// (orange/purple/blue), one built-in (red), and one CUSTOM escape-hatch color
/// (`teal`) whose CSS variables are defined in the page `<style>` below.
fn severity_badges() -> BadgeVariantMap {
    BadgeVariantMap::new()
        .with("blocker", BadgeVariant::new("Blocker", "red"))
        .with("high", BadgeVariant::new("High", "orange"))
        .with("medium", BadgeVariant::new("Medium", "purple"))
        .with("low", BadgeVariant::new("Low", "blue"))
        .with("info", BadgeVariant::new("Info", "teal"))
        .with_fallback(BadgeVariant::new("?", "gray"))
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
        // #33 — Severity rendered as a colored badge.
        ColumnDef::new(ColumnId("severity"), "Severity", |f: &Finding| {
            CellValue::Text(f.severity.clone())
        })
        .sortable()
        .render_kind(RenderKind::Badge(severity_badges())),
        // #35 — list-valued Tags column with a per-value (OR-contains) set-filter.
        ColumnDef::new(ColumnId("tags"), "Tags", |f: &Finding| {
            CellValue::Text(f.tags.clone())
        })
        .filter(FilterKind::MultiSelectContains {
            options: ["api", "db", "perf", "security", "style", "ui"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            separator: ", ".to_string(),
        }),
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

    // #32 — highlight every blocker-severity leaf row via a row class.
    let row_class =
        RowClass::new(|f: &Finding| (f.severity == "blocker").then(|| "danger-row".to_string()));

    rsx! {
        // #33 custom-color escape hatch: define the `teal` badge variables the
        // built-in palette doesn't ship. #32: style the `danger-row` class.
        style { dangerous_inner_html: HARNESS_CSS }
        div { style: "font-family: system-ui, sans-serif; padding: 1rem; max-width: 980px; margin: 0 auto;",
            h1 { "Chorale v0.2.3 QA" }
            p { style: "color: #444; line-height: 1.5;",
                "Grouped RULE → FILE → lines, all collapsed on load. Use the checklist below "
                "to exercise every v0.2.3 change in one place."
            }
            ul { style: "color: #444; line-height: 1.6; font-size: 0.92rem;",
                li { b { "#36 collapse: " } "expand a rule, then collapse one of its FILE sub-headers — the parent RULE header must stay at depth 0 with its count, siblings stay aligned. Repeat at both depths in any order; nothing should jumble or mis-indent." }
                li { b { "#31 group select-all: " } "each group header has a checkbox. Click it to select that group's rows only; partial selection shows the box indeterminate (dash); it never touches other groups or filtered-out rows." }
                li { b { "#33 badges: " } "expand to leaf rows — Severity shows colored pills: High=orange, Medium=purple, Low=blue (new colors), Blocker=red (built-in), Info=teal (custom escape-hatch color defined in this page's CSS)." }
                li { b { "#32 row_class: " } "Blocker rows (rule SQL-DB-INDEX-2) render with a red left bar + pink tint via the row_class hook." }
                li { b { "#35 set-filter: " } "open the Tags filter (filter row under the header) and pick e.g. \"security\" — every row whose tag list contains it matches (OR), regardless of the other tags. Group counts update; combine with #31 to confirm group-select is filter-aware." }
                li { b { "#34: " } "docs-only (the Send+Sync interactive-cell pattern) — nothing to click here." }
            }
            Table {
                handle,
                sort_enabled: true,
                filter_enabled: true,
                selection_enabled: true,
                group_expand_toggle: true,
                row_class,
            }
        }
    }
}
