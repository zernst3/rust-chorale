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
