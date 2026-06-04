//! QA harness for chorale-dioxus.
//!
//! Run with: `dx serve --package qa-harness` (requires `cargo install dioxus-cli`)
//!
//! Generates a reproducible 10k-row Employee dataset and renders a page
//! scaffold with feature-toggle controls. Each v0.1-dioxus work-queue item
//! wires one toggle as the matching adapter feature lands.

use chorale_core::{
    Alignment, BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId, CurrencyCode,
    FilterKind, RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, CellRenderer, CellRenderers, Table};
use chrono::NaiveDate;
use dioxus::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::Arc;

// Fixed seed guarantees the same dataset on every run (work-queue item 0).
const SEED: u64 = 42;
// Intentionally not a round multiple of the default page_size (50) so the
// last page renders only 10 rows. Exercises the partial-last-page edge case.
const ROW_COUNT: usize = 10_010;

// Spread the date range over 10 years starting from 2015-01-01.
const DATE_RANGE_DAYS: i64 = 3_650;

static FIRST_NAMES: &[&str] = &[
    "Alice", "Bob", "Charlie", "Diana", "Ethan", "Fiona", "George", "Hannah", "Ivan", "Julia",
    "Kevin", "Laura", "Mike", "Nancy", "Oscar", "Pam", "Quinn", "Rose", "Sam", "Tina", "Uma",
    "Victor", "Wendy", "Xander", "Yara", "Zoe",
];

static LAST_NAMES: &[&str] = &[
    "Smith", "Jones", "Williams", "Brown", "Davis", "Miller", "Wilson", "Moore", "Taylor",
    "Anderson", "Thomas", "Jackson", "White", "Harris", "Martin", "Thompson", "Garcia", "Martinez",
    "Robinson", "Clark",
];

static ROLES: &[&str] = &[
    "Engineer",
    "Designer",
    "Manager",
    "Analyst",
    "Director",
    "Coordinator",
    "Developer",
    "Architect",
];

static STATUSES: &[&str] = &["Active", "Inactive", "Pending", "Suspended"];

static EMAIL_DOMAINS: &[&str] = &["example.com", "corp.io", "company.net", "org.dev"];

#[derive(Clone, PartialEq)]
struct Employee {
    name: String,
    email: String,
    joined_date: NaiveDate,
    role: String,
    status: String,
    salary: i64,
}

#[must_use]
fn generate_dataset() -> Vec<(RowId, Employee)> {
    let mut rng = StdRng::seed_from_u64(SEED);
    // 2015-01-01 is always a valid Gregorian date.
    let base = NaiveDate::from_ymd_opt(2015, 1, 1).unwrap_or(NaiveDate::MIN);

    (0..ROW_COUNT)
        .map(|_| {
            let first = FIRST_NAMES[rng.gen_range(0..FIRST_NAMES.len())];
            let last = LAST_NAMES[rng.gen_range(0..LAST_NAMES.len())];
            let name = format!("{first} {last}");
            let domain = EMAIL_DOMAINS[rng.gen_range(0..EMAIL_DOMAINS.len())];
            let email = format!("{}.{}@{domain}", first.to_lowercase(), last.to_lowercase());
            let days: i64 = rng.gen_range(0..DATE_RANGE_DAYS);
            let joined_date = base + chrono::Duration::days(days);
            let role = ROLES[rng.gen_range(0..ROLES.len())].to_string();
            let status = STATUSES[rng.gen_range(0..STATUSES.len())].to_string();
            let salary: i64 = rng.gen_range(40_000..200_000);
            (
                RowId::new(),
                Employee {
                    name,
                    email,
                    joined_date,
                    role,
                    status,
                    salary,
                },
            )
        })
        .collect()
}

#[must_use]
fn columns() -> Vec<ColumnDef<Employee>> {
    let status_badges = BadgeVariantMap::new()
        .with("Active", BadgeVariant::new("Active", "green"))
        .with("Inactive", BadgeVariant::new("Inactive", "gray"))
        .with("Pending", BadgeVariant::new("Pending", "yellow"))
        .with("Suspended", BadgeVariant::new("Suspended", "red"));

    vec![
        ColumnDef::new(ColumnId("name"), "Name", |r: &Employee| {
            CellValue::Text(r.name.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(160.0),
        ColumnDef::new(ColumnId("email"), "Email", |r: &Employee| {
            CellValue::Text(r.email.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(220.0),
        ColumnDef::new(ColumnId("joined_date"), "Joined", |r: &Employee| {
            CellValue::Date(r.joined_date)
        })
        .sortable()
        .filter(FilterKind::DateRange)
        .initial_width(180.0)
        .render_kind(RenderKind::Date),
        ColumnDef::new(ColumnId("role"), "Role", |r: &Employee| {
            CellValue::Text(r.role.clone())
        })
        .sortable()
        .filter(FilterKind::MultiSelect {
            options: ROLES.iter().map(|s| (*s).to_string()).collect(),
        })
        .initial_width(140.0),
        ColumnDef::new(ColumnId("status"), "Status", |r: &Employee| {
            CellValue::Text(r.status.clone())
        })
        .sortable()
        .filter(FilterKind::MultiSelect {
            options: STATUSES.iter().map(|s| (*s).to_string()).collect(),
        })
        .initial_width(120.0)
        .alignment(Alignment::Center)
        .render_kind(RenderKind::Badge(status_badges)),
        ColumnDef::new(ColumnId("salary"), "Salary", |r: &Employee| {
            CellValue::Integer(r.salary)
        })
        .sortable()
        .filter(FilterKind::NumericRange {
            min: 40_000.0,
            max: 200_000.0,
            step: 1_000.0,
        })
        .initial_width(160.0)
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Currency(CurrencyCode::USD)),
    ]
}

fn make_status_renderer() -> CellRenderer {
    Arc::new(|val: &CellValue| {
        let text = if let CellValue::Text(s) = val {
            s.as_str()
        } else {
            ""
        };
        let (prefix, color) = match text {
            "Active" => ("● ", "#065f46"),
            "Inactive" => ("○ ", "#374151"),
            "Pending" => ("◑ ", "#92400e"),
            "Suspended" => ("✕ ", "#991b1b"),
            _ => ("", "#333"),
        };
        rsx! {
            span {
                style: "color: {color}; font-weight: 500; font-size: 0.875rem;",
                "{prefix}{text}"
            }
        }
    })
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(generate_dataset(), columns()));
    let row_count = table.signal().read().rows.len();
    let mut sort_on = use_signal(|| false);
    let mut filter_on = use_signal(|| false);
    let mut selection_on = use_signal(|| false);
    let mut col_toolbar_on = use_signal(|| false);
    let mut csv_export_on = use_signal(|| false);
    let mut resize_on = use_signal(|| false);
    let status_renderers = use_memo(|| {
        let mut m = HashMap::new();
        m.insert(ColumnId("status"), make_status_renderer());
        CellRenderers::new(m)
    });

    rsx! {
        div {
            style: "font-family: sans-serif; padding: 1rem; max-width: 1200px; margin: 0 auto;",

            h1 { "chorale QA Harness" }
            p { "Dataset: {row_count} rows" }

            // Feature toggles — each item wires one up as the feature lands.
            div {
                style: "display:flex; gap:1rem; flex-wrap:wrap; margin-bottom:1rem; \
                        padding:0.75rem; background:#f5f5f5; border-radius:4px;",

                label {
                    input {
                        r#type: "checkbox",
                        checked: *sort_on.read(),
                        onchange: move |_| {
                            let v = *sort_on.read();
                            sort_on.set(!v);
                        },
                    }
                    " Sort"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *filter_on.read(),
                        onchange: move |_| {
                            let v = *filter_on.read();
                            filter_on.set(!v);
                        },
                    }
                    " Filter"
                }
                span {
                    style: "color: #555; font-size: 0.875rem; align-self: center;",
                    "Virtualization: always on"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *selection_on.read(),
                        onchange: move |_| {
                            let v = *selection_on.read();
                            selection_on.set(!v);
                        },
                    }
                    " Selection"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *col_toolbar_on.read(),
                        onchange: move |_| {
                            let v = *col_toolbar_on.read();
                            col_toolbar_on.set(!v);
                        },
                    }
                    " Column Visibility"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *csv_export_on.read(),
                        onchange: move |_| {
                            let v = *csv_export_on.read();
                            csv_export_on.set(!v);
                        },
                    }
                    " CSV Export"
                }
                label {
                    input {
                        r#type: "checkbox",
                        checked: *resize_on.read(),
                        onchange: move |_| {
                            let v = *resize_on.read();
                            resize_on.set(!v);
                        },
                    }
                    " Column Resize"
                }
                label {
                    style: "display: flex; align-items: center; gap: 0.25rem;",
                    " Page size: "
                    select {
                        onchange: move |e| {
                            if let Ok(n) = e.value().parse::<usize>() {
                                table.set_page_size(n).ok();
                            }
                        },
                        option { value: "10", "10" }
                        option { value: "25", "25" }
                        option { value: "50", selected: true, "50" }
                        option { value: "100", "100" }
                        option { value: "200", "200" }
                    }
                }
            }

            Table {
                handle: table,
                sort_enabled: *sort_on.read(),
                filter_enabled: *filter_on.read(),
                selection_enabled: *selection_on.read(),
                cell_renderers: status_renderers.read().clone(),
                column_toolbar: *col_toolbar_on.read(),
                csv_export: *csv_export_on.read(),
                resize_enabled: *resize_on.read(),
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_dataset_returns_correct_count() {
        let rows = generate_dataset();
        assert_eq!(rows.len(), ROW_COUNT);
    }

    #[test]
    fn generate_dataset_is_deterministic() {
        let rows1 = generate_dataset();
        let rows2 = generate_dataset();
        assert_eq!(rows1[0].1.name, rows2[0].1.name);
        assert_eq!(rows1[0].1.salary, rows2[0].1.salary);
        assert_eq!(rows1[ROW_COUNT - 1].1.email, rows2[ROW_COUNT - 1].1.email);
    }

    #[test]
    fn columns_definition_has_six_columns() {
        let cols = columns();
        assert_eq!(cols.len(), 6);
    }
}
