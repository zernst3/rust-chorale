//! QA harness for chorale-leptos.
//!
//! Run with: `trunk serve --open --package leptos-qa-harness`
//!
//! Generates a reproducible 10k-row Employee dataset and renders a page
//! scaffold with feature-toggle controls.

use chorale_core::{
    Alignment, BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId, CurrencyCode,
    FilterKind, NaiveDate, RenderKind,
};
use chorale_leptos::{use_chorale_table, CellRenderer, CellRenderers, Table};
use leptos::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::Arc;

// Fixed seed guarantees the same dataset on every run.
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

fn generate_dataset() -> Vec<Employee> {
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
            Employee {
                name,
                email,
                joined_date,
                role,
                status,
                salary,
            }
        })
        .collect()
}

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
            s.clone()
        } else {
            String::new()
        };
        let (prefix, color) = match text.as_str() {
            "Active" => ("● ", "#065f46"),
            "Inactive" => ("○ ", "#374151"),
            "Pending" => ("◑ ", "#92400e"),
            "Suspended" => ("✕ ", "#991b1b"),
            _ => ("", "#333"),
        };
        let label = format!("{prefix}{text}");
        view! {
            <span style=format!("color: {color}; font-weight: 500; font-size: 0.875rem;")>
                {label}
            </span>
        }
        .into_any()
    })
}

#[component]
fn App() -> impl IntoView {
    let table = use_chorale_table(generate_dataset(), columns());
    let row_count = table.signal().with_untracked(|s| s.rows.len());

    let sort_on = RwSignal::new(false);
    let filter_on = RwSignal::new(false);
    let selection_on = RwSignal::new(false);
    let col_toolbar_on = RwSignal::new(false);
    let csv_export_on = RwSignal::new(false);
    let resize_on = RwSignal::new(false);

    let status_renderers = {
        let mut m = HashMap::new();
        m.insert(ColumnId("status"), make_status_renderer());
        CellRenderers::new(m)
    };

    view! {
        <div style="font-family: sans-serif; padding: 1rem; max-width: 1200px; margin: 0 auto;">
            <h1>"chorale QA Harness"</h1>
            <p>"Dataset: "{row_count}" rows"</p>

            // Feature toggles
            <div style="display:flex; gap:1rem; flex-wrap:wrap; margin-bottom:1rem; padding:0.75rem; background:#f5f5f5; border-radius:4px;">
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || sort_on.get()
                        on:change=move |_| sort_on.update(|v| *v = !*v)
                    />
                    " Sort"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || filter_on.get()
                        on:change=move |_| filter_on.update(|v| *v = !*v)
                    />
                    " Filter"
                </label>
                <span style="color: #555; font-size: 0.875rem; align-self: center;">
                    "Virtualization: always on"
                </span>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || selection_on.get()
                        on:change=move |_| selection_on.update(|v| *v = !*v)
                    />
                    " Selection"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || col_toolbar_on.get()
                        on:change=move |_| col_toolbar_on.update(|v| *v = !*v)
                    />
                    " Column Visibility"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || csv_export_on.get()
                        on:change=move |_| csv_export_on.update(|v| *v = !*v)
                    />
                    " CSV Export"
                </label>
                <label>
                    <input
                        type="checkbox"
                        prop:checked=move || resize_on.get()
                        on:change=move |_| resize_on.update(|v| *v = !*v)
                    />
                    " Column Resize"
                </label>
                <label style="display: flex; align-items: center; gap: 0.25rem;">
                    " Page size: "
                    <select on:change=move |e| {
                        let val = event_target_value(&e);
                        if let Ok(n) = val.parse::<usize>() {
                            table.set_page_size(n).ok();
                        }
                    }>
                        <option value="10">"10"</option>
                        <option value="25">"25"</option>
                        <option value="50" selected=true>"50"</option>
                        <option value="100">"100"</option>
                        <option value="200">"200"</option>
                    </select>
                </label>
            </div>

            {move || {
                let sort = sort_on.get();
                let filter = filter_on.get();
                let selection = selection_on.get();
                let toolbar = col_toolbar_on.get();
                let csv = csv_export_on.get();
                let resize = resize_on.get();
                let renderers = status_renderers.clone();
                view! {
                    <Table
                        handle=table
                        sort_enabled=sort
                        filter_enabled=filter
                        selection_enabled=selection
                        cell_renderers=renderers
                        column_toolbar=toolbar
                        csv_export=csv
                        resize_enabled=resize
                        on_commit_edit=None
                    />
                }
            }}
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
