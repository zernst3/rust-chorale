//! Custom cell rendering: two approaches.
//!
//! 1. **`RenderKind::Badge`** — declarative pill rendering driven by a
//!    `BadgeVariantMap`. No closures, lives in `chorale-core`.
//! 2. **`CellRenderers`** — an arbitrary `Fn(&CellValue) -> AnyView` keyed
//!    by `ColumnId`. Lives in `chorale-leptos` because it returns a Leptos
//!    `AnyView`. Use this when the cell needs framework-specific markup
//!    (gradients, animations, conditional layouts, links).
//!
//! Run with: `trunk serve --open --package leptos-with-custom-cells`

use chorale_core::{
    Alignment, BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId, FilterKind,
    RenderKind,
};
use chorale_leptos::{use_chorale_table, CellRenderer, CellRenderers, Table};
use leptos::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct Deploy {
    service: String,
    status: String,
    health: f64,
}

fn deploys() -> Vec<Deploy> {
    [
        ("api", "healthy", 99.9),
        ("worker", "degraded", 92.3),
        ("scheduler", "healthy", 99.7),
        ("ingest", "failing", 41.2),
        ("notifier", "healthy", 99.8),
    ]
    .into_iter()
    .map(|(s, st, h)| Deploy {
        service: s.into(),
        status: st.into(),
        health: h,
    })
    .collect()
}

fn columns() -> Vec<ColumnDef<Deploy>> {
    let badges = BadgeVariantMap::new()
        .with("healthy", BadgeVariant::new("Healthy", "green"))
        .with("degraded", BadgeVariant::new("Degraded", "yellow"))
        .with("failing", BadgeVariant::new("Failing", "red"));

    vec![
        ColumnDef::new(ColumnId("service"), "Service", |d: &Deploy| {
            CellValue::Text(d.service.clone())
        })
        .sortable()
        .filter(FilterKind::Text)
        .initial_width(140.0),
        ColumnDef::new(ColumnId("status"), "Status", |d: &Deploy| {
            CellValue::Text(d.status.clone())
        })
        .sortable()
        .initial_width(120.0)
        .alignment(Alignment::Center)
        .render_kind(RenderKind::Badge(badges)),
        ColumnDef::new(ColumnId("health"), "Health", |d: &Deploy| {
            CellValue::Float(d.health)
        })
        .sortable()
        .initial_width(200.0)
        .alignment(Alignment::Right)
        .render_kind(RenderKind::Number),
    ]
}

/// Custom renderer for the `health` column: a horizontal progress bar whose
/// fill color reflects the health percentage.
fn health_bar_renderer() -> CellRenderer {
    Arc::new(|val: &CellValue| {
        let pct = match val {
            CellValue::Float(f) => *f,
            _ => 0.0,
        };
        let color = if pct >= 99.0 {
            "#10b981"
        } else if pct >= 90.0 {
            "#f59e0b"
        } else {
            "#ef4444"
        };
        let width_pct = pct.clamp(0.0, 100.0);
        view! {
            <div style="display: flex; align-items: center; gap: 0.5rem; width: 100%;">
                <div style="flex: 1; height: 8px; background: #e5e7eb; border-radius: 4px; overflow: hidden;">
                    <div style=format!("height: 100%; width: {width_pct}%; background: {color};")></div>
                </div>
                <span style="font-variant-numeric: tabular-nums; min-width: 4ch;">
                    {format!("{pct:.1}")}
                </span>
            </div>
        }
        .into_any()
    })
}

#[component]
fn App() -> impl IntoView {
    let table = use_chorale_table(deploys(), columns());
    let renderers = {
        let mut m = HashMap::new();
        m.insert(ColumnId("health"), health_bar_renderer());
        CellRenderers::new(m)
    };

    view! {
        <div style="font-family: sans-serif; padding: 1rem; max-width: 800px; margin: 0 auto;">
            <h1>"Custom cells example"</h1>
            <p>
                "Status uses the declarative "
                <code>"RenderKind::Badge"</code>
                " variant. Health uses a "
                <code>"CellRenderers"</code>
                " entry — arbitrary Leptos markup keyed by column id."
            </p>
            <Table handle=table sort_enabled=true cell_renderers=renderers on_commit_edit=None />
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(App);
}
