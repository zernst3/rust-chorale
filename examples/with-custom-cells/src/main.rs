//! Custom cell rendering: two approaches.
//!
//! 1. **`RenderKind::Badge`** — declarative pill rendering driven by a
//!    `BadgeVariantMap`. No closures, lives in `chorale-core`.
//! 2. **`CellRenderers`** — an arbitrary `Fn(&CellValue) -> Element` keyed
//!    by `ColumnId`. Lives in `chorale-dioxus` because it returns a Dioxus
//!    `Element`. Use this when the cell needs framework-specific markup
//!    (gradients, animations, conditional layouts, links).
//!
//! Run with: `dx serve --package with-custom-cells`

use chorale_core::{
    Alignment, BadgeVariant, BadgeVariantMap, CellValue, ColumnDef, ColumnId, FilterKind,
    RenderKind, RowId, TableState,
};
use chorale_dioxus::{use_table, CellRenderer, CellRenderers, Table};
use dioxus::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct Deploy {
    service: String,
    status: String,
    health: f64,
}

fn deploys() -> Vec<(RowId, Deploy)> {
    [
        ("api", "healthy", 99.9),
        ("worker", "degraded", 92.3),
        ("scheduler", "healthy", 99.7),
        ("ingest", "failing", 41.2),
        ("notifier", "healthy", 99.8),
    ]
    .into_iter()
    .map(|(s, st, h)| {
        (
            RowId::new(),
            Deploy {
                service: s.into(),
                status: st.into(),
                health: h,
            },
        )
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
/// fill color reflects the health percentage. Returns a Dioxus `Element` —
/// arbitrary markup is allowed.
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
        rsx! {
            div {
                style: "display: flex; align-items: center; gap: 0.5rem; width: 100%;",
                div {
                    style: "flex: 1; height: 8px; background: #e5e7eb; \
                            border-radius: 4px; overflow: hidden;",
                    div {
                        style: "height: 100%; width: {width_pct}%; background: {color};",
                    }
                }
                span { style: "font-variant-numeric: tabular-nums; min-width: 4ch;",
                    "{pct:.1}"
                }
            }
        }
    })
}

#[component]
fn App() -> Element {
    let table = use_table(|| TableState::new(deploys(), columns()));
    let renderers = use_memo(|| {
        let mut m = HashMap::new();
        m.insert(ColumnId("health"), health_bar_renderer());
        CellRenderers::new(m)
    });

    rsx! {
        div { style: "font-family: sans-serif; padding: 1rem; max-width: 800px; margin: 0 auto;",
            h1 { "Custom cells example" }
            p {
                "Status uses the declarative "
                code { "RenderKind::Badge" }
                " variant. Health uses a "
                code { "CellRenderers" }
                " entry — arbitrary Dioxus markup keyed by column id."
            }
            Table {
                handle: table,
                sort_enabled: true,
                cell_renderers: renderers.read().clone(),
            }
        }
    }
}

fn main() {
    dioxus::launch(App);
}
