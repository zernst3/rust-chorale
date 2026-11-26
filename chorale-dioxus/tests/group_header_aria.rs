//! Treegrid markup and the one visible-row model of grouped tables (issue #51).
//!
//! See `tests/common/mod.rs` for the harness.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::wildcard_imports)]

mod common;

use common::*;

#[test]
fn grouped_rows_carry_treegrid_aria() {
    for hook in ON_OFF {
        let h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        let html = h.html();
        assert!(html.contains("role=\"treegrid\""), "hook={hook}");
        let header = h.tr_tag(0).unwrap();
        assert!(header.contains("aria-level=\"1\""), "hook={hook}: {header}");
        assert!(
            header.contains("aria-expanded=\"true\""),
            "hook={hook}: {header}"
        );
        let data = h.tr_tag(1).unwrap();
        assert!(data.contains("aria-level=\"2\""), "hook={hook}: {data}");
        assert!(!data.contains("aria-expanded"), "hook={hook}: {data}");
    }
}

/// The VIRT-2 row-height loop measures `tr[data-chorale-index]` and shifts
/// `scrollTop` when a measured height differs from `row_height`. Header rows
/// (shorter than `row_height` with the default header) must stay out of it,
/// or it undoes scroll-follow; they carry their own index attribute.
#[test]
fn header_rows_stay_out_of_row_height_measurement() {
    for hook in ON_OFF {
        for virtualized in [false, true] {
            let h = Harness::new(Cfg {
                hook,
                virtualized,
                ..Cfg::default()
            });
            let html = h.html();
            let headers: Vec<&str> = html
                .split("<tr ")
                .skip(1)
                .map(|t| t.split('>').next().unwrap_or(""))
                .filter(|t| t.contains("aria-expanded"))
                .collect();
            assert!(!headers.is_empty());
            for tag in headers {
                assert!(
                    !tag.contains("data-chorale-index="),
                    "hook={hook} virtualized={virtualized}: {tag}"
                );
                assert!(
                    tag.contains("data-chorale-group-index="),
                    "hook={hook} virtualized={virtualized}: {tag}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// One row model: visible_grouped_view
// ---------------------------------------------------------------------------

/// Body row `i` is `visible_grouped_view(&state)[i]`: the same list the
/// cursor indexes. A header carries its label, a data row its item name.
#[test]
fn rendered_rows_follow_visible_grouped_view_one_to_one() {
    for hook in ON_OFF {
        for virtualized in [false, true] {
            let mut h = Harness::new(Cfg {
                hook,
                virtualized,
                ..Cfg::default()
            });
            // Collapse one group so indexes shift after it.
            h.set_active(11, "name");
            h.press(Key::ArrowLeft);
            let view = view_of(&h.state());
            let html = h.html();
            let mut checked = 0;
            for (i, row) in view.iter().enumerate() {
                let Some(tag) = h.tr_tag(i) else {
                    continue; // outside the virtualized window
                };
                let tr_start = html.find(&format!("<tr {tag}>")).unwrap();
                let tr = &html[tr_start..];
                let tr = &tr[..tr.find("</tr>").unwrap()];
                match row {
                    GroupedRow::Header { label, .. } => {
                        assert!(
                            tag.contains("data-chorale-group-index"),
                            "hook={hook} virtualized={virtualized} row {i}: {tag}"
                        );
                        assert!(tr.contains(label.as_str()), "row {i}: {tr}");
                    }
                    GroupedRow::Data(_, item) => {
                        assert!(tag.contains("data-chorale-index"), "row {i}: {tag}");
                        assert!(tr.contains(&format!(">{}<", item.name)), "row {i}: {tr}");
                    }
                    _ => panic!("unexpected row kind"),
                }
                checked += 1;
            }
            assert!(
                checked >= 12,
                "hook={hook} virtualized={virtualized}: {checked}"
            );
            // The cursor's row is the same entry.
            let ac = h.active().unwrap();
            assert!(matches!(view[ac.row_idx], GroupedRow::Header { .. }));
        }
    }
}

// ---------------------------------------------------------------------------
// EM review of #52 (A.1, A.5)
// ---------------------------------------------------------------------------

/// A.1: focus stays on the grid's keyboard container, which points
/// `aria-activedescendant` at the active header row or the active data cell,
/// so assistive technology follows the cursor. Cleared when virtualization
/// unmounts the active row.
#[test]
fn keyboard_container_points_aria_activedescendant_at_the_cursor() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        h.set_active(0, "name");
        let kb = h.kb_tag();
        let target = Harness::attr(&kb, "aria-activedescendant")
            .unwrap_or_else(|| panic!("hook={hook}: no aria-activedescendant on {kb}"));
        let header = h.tr_tag(0).unwrap();
        assert_eq!(
            Harness::attr(&header, "id").as_deref(),
            Some(target.as_str()),
            "hook={hook}: it names header row 0: {header}"
        );
        h.press(Key::ArrowDown);
        let target = Harness::attr(&h.kb_tag(), "aria-activedescendant").expect("cell target");
        let html = h.html();
        let at = html
            .find(&format!("id=\"{target}\""))
            .unwrap_or_else(|| panic!("hook={hook}: no element with id {target}"));
        let open = html[..at].rfind('<').unwrap();
        assert!(
            html[open..].starts_with("<td"),
            "hook={hook}: the active data cell"
        );
        assert!(html[at..].contains("Item 0"), "hook={hook}");
        // Every rendered row carries a stable id.
        for i in h.rendered_rows() {
            let tag = h.tr_tag(i).unwrap();
            assert!(
                Harness::attr(&tag, "id").is_some(),
                "hook={hook}: row {i}: {tag}"
            );
        }
        // The kb container is a composite that may own an active descendant.
        assert_eq!(Harness::attr(&h.kb_tag(), "role").as_deref(), Some("group"));
    }
    // Virtualized: an active row outside the rendered window is not referenced.
    let mut h = Harness::new(Cfg {
        virtualized: true,
        ..Cfg::default()
    });
    h.set_active(80, "name");
    assert!(!h.rendered_rows().contains(&80));
    assert_eq!(Harness::attr(&h.kb_tag(), "aria-activedescendant"), None);
    // Ungrouped tables are unchanged: no role, no active descendant.
    let mut h = Harness::new(Cfg {
        ungrouped: true,
        ..Cfg::default()
    });
    h.set_active(2, "name");
    assert_eq!(Harness::attr(&h.kb_tag(), "aria-activedescendant"), None);
    assert_eq!(Harness::attr(&h.kb_tag(), "role"), None);
}

/// A.5: the treegrid reports its full row count and each row its one-based
/// position (the `<thead>` row is row 1); spacer rows are hidden.
#[test]
fn treegrid_exposes_row_count_and_row_index() {
    for virtualized in [false, true] {
        let h = Harness::new(Cfg {
            virtualized,
            initial_scroll: if virtualized { 2400.0 } else { 0.0 },
            ..Cfg::default()
        });
        let total = view_of(&h.state()).len();
        let html = h.html();
        let table = html
            .split("<table ")
            .nth(1)
            .and_then(|t| t.split('>').next())
            .unwrap()
            .to_owned();
        assert_eq!(
            Harness::attr(&table, "aria-rowcount"),
            Some((total + 1).to_string()),
            "virtualized={virtualized}: {table}"
        );
        for i in h.rendered_rows() {
            let tag = h.tr_tag(i).unwrap();
            assert_eq!(
                Harness::attr(&tag, "aria-rowindex"),
                Some((i + 2).to_string()),
                "virtualized={virtualized}: row {i}: {tag}"
            );
        }
        let thead_tr = html.split("<thead>").nth(1).unwrap();
        let thead_tr = thead_tr.split('>').next().unwrap();
        assert_eq!(
            Harness::attr(thead_tr, "aria-rowindex").as_deref(),
            Some("1")
        );
        // Spacer rows (no row index) are hidden from assistive technology.
        let spacers: Vec<&str> = html
            .split("<tr")
            .skip(1)
            .map(|t| t.split('>').next().unwrap_or(""))
            .filter(|t| !t.contains("data-chorale-") && !html.is_empty())
            .collect();
        if virtualized {
            assert!(!spacers.is_empty());
        }
        for tag in spacers {
            if tag.contains("aria-rowindex=\"1\"") {
                continue; // the <thead> row
            }
            assert!(
                tag.contains("aria-hidden=\"true\""),
                "virtualized={virtualized}: {tag}"
            );
        }
    }
}
