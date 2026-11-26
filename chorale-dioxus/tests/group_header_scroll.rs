//! Scroll-follow keeps the active row fully visible, and only keyboard moves drive it (issue #51).
//!
//! See `tests/common/mod.rs` for the harness.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::wildcard_imports)]

mod common;

use common::*;

// ---------------------------------------------------------------------------
// Fix 2: scroll-follow keeps the active row fully visible
// ---------------------------------------------------------------------------

#[test]
fn arrow_keys_keep_active_row_fully_visible_in_paged_mode() {
    for hook in ON_OFF {
        let cfg = Cfg {
            hook,
            ..Cfg::default()
        };
        let layout = PagedLayout::new(&cfg);
        let mut h = Harness::with_reply(cfg, layout.reply());
        h.set_active(0, "name");
        let rows = view_of(&h.state()).len();
        for _ in 1..rows {
            h.press(Key::ArrowDown);
            let i = h.active().unwrap().row_idx;
            assert!(
                layout.fully_visible(i),
                "hook={hook}: Down to row {i}: top {} height {} scroll_top {}",
                layout.top(i),
                layout.height(i),
                layout.scroll_top.borrow()
            );
            assert!(
                (h.state().scroll_top - *layout.scroll_top.borrow()).abs() < 0.01,
                "hook={hook}: state scroll_top follows the DOM write"
            );
        }
        assert_eq!(h.active().unwrap().row_idx, rows - 1);
        for _ in 1..rows {
            h.press(Key::ArrowUp);
            let i = h.active().unwrap().row_idx;
            assert!(layout.fully_visible(i), "hook={hook}: Up to row {i}");
        }
        for key in [Key::End, Key::PageDown, Key::PageUp, Key::Home] {
            let ctrl = matches!(key, Key::End | Key::Home);
            let mods = if ctrl {
                Modifiers::CONTROL
            } else {
                Modifiers::empty()
            };
            h.press_mod(key.clone(), mods);
            let i = h.active().unwrap().row_idx;
            assert!(layout.fully_visible(i), "hook={hook}: {key:?} to row {i}");
        }
    }
}

#[test]
fn every_move_reveals_row_in_outer_scrollers() {
    let cfg = Cfg::default();
    let layout = PagedLayout::new(&cfg);
    let mut h = Harness::with_reply(cfg, layout.reply());
    h.set_active(1, "name");
    // Row 2 needs no table scroll, but the page around the table may.
    let before = h.eval_count();
    h.press(Key::ArrowDown);
    let scripts = h.evals_since(before);
    let reveal = scripts
        .iter()
        .find(|js| js.contains("chorale:reveal-row"))
        .unwrap_or_else(|| panic!("no reveal script: {scripts:?}"));
    assert_eq!(script_row_index(reveal), Some(2), "{reveal}");
    assert!(reveal.contains("window.scrollBy"), "{reveal}");
    assert!(reveal.contains("parentElement"), "{reveal}");
}

/// Virtualized: the target row is not rendered yet, so the table scrolls to
/// an estimate, waits for the render and measures again.
#[test]
fn virtualized_far_jump_estimates_then_corrects_from_measurement() {
    for (real_top, expected) in [
        // The re-measure finds the row inside the band: keep the estimate.
        (3400.0, 3061.0),
        // The re-measure finds it below the band: correct to it.
        (3600.0, 3140.0),
    ] {
        let measures = Rc::new(RefCell::new(0_usize));
        let scroll = Rc::new(RefCell::new(0.0_f64));
        let reply: Reply = {
            let measures = measures.clone();
            let scroll = scroll.clone();
            Rc::new(move |js: &str| {
                if let Some(v) = script_scroll_write(js) {
                    *scroll.borrow_mut() = v;
                }
                if !js.contains("chorale:measure-row") {
                    return serde_json::Value::Null;
                }
                let n = {
                    let mut m = measures.borrow_mut();
                    *m += 1;
                    *m
                };
                let st = *scroll.borrow();
                if n == 1 {
                    // Not rendered: the window is still at the top.
                    serde_json::json!([0, 0, 0, st, CLIENT_H, THEAD_H, THEAD_H])
                } else {
                    // Rendered now; later measurements wait for a frame first.
                    assert!(js.contains("requestAnimationFrame"), "{js}");
                    serde_json::json!([1, real_top, DATA_H, st, CLIENT_H, THEAD_H, THEAD_H])
                }
            })
        };
        let mut h = Harness::with_reply(
            Cfg {
                virtualized: true,
                ..Cfg::default()
            },
            reply,
        );
        h.set_active(0, "name");
        let before = h.eval_count();
        h.press_mod(Key::End, Modifiers::CONTROL);
        let last = view_of(&h.state()).len() - 1;
        assert_eq!(h.active().unwrap().row_idx, last);
        // Estimate: thead 41 + 87 rows * 40 = 3521; bottom-align in 500px.
        let writes: Vec<f64> = h
            .evals_since(before)
            .iter()
            .filter_map(|js| script_scroll_write(js))
            .collect();
        assert_eq!(writes.first().copied(), Some(3061.0), "{writes:?}");
        // Missing, then found; a write is verified by one more measurement.
        let expected_measures = if real_top > 3500.0 { 3 } else { 2 };
        assert_eq!(*measures.borrow(), expected_measures);
        assert!(
            (h.state().scroll_top - expected).abs() < 0.01,
            "real top {real_top}: scroll_top {} expected {expected}",
            h.state().scroll_top
        );
        assert!(
            (*scroll.borrow() - expected).abs() < 0.01,
            "DOM scrollTop {} expected {expected}",
            scroll.borrow()
        );
        let reveal = h
            .evals_since(before)
            .into_iter()
            .rfind(|js| js.contains("chorale:reveal-row"));
        assert!(
            reveal.is_some(),
            "the measured row is revealed in outer scrollers"
        );
    }
}

/// Virtualized with the default header: rows render at heights other than
/// `row_height`, so after a scroll write the window re-slices and the rows
/// shift. The table measures again after every write and corrects.
#[test]
fn scroll_write_is_verified_and_corrected_after_render() {
    let measures = Rc::new(RefCell::new(0_usize));
    let scroll = Rc::new(RefCell::new(0.0_f64));
    let reply: Reply = {
        let measures = measures.clone();
        let scroll = scroll.clone();
        Rc::new(move |js: &str| {
            if let Some(v) = script_scroll_write(js) {
                *scroll.borrow_mut() = v;
            }
            if !js.contains("chorale:measure-row") {
                return serde_json::Value::Null;
            }
            let n = {
                let mut m = measures.borrow_mut();
                *m += 1;
                *m
            };
            let st = *scroll.borrow();
            // Row 12 sits at 520 before the write; once the window has
            // re-sliced it sits 10px lower (a 30px header now counted as 40).
            let top = if n == 1 { 520.0 } else { 530.0 };
            serde_json::json!([1, top, DATA_H, st, CLIENT_H, THEAD_H, THEAD_H])
        })
    };
    let mut h = Harness::with_reply(
        Cfg {
            virtualized: true,
            ..Cfg::default()
        },
        reply,
    );
    h.set_active(11, "name");
    let before = h.eval_count();
    h.press(Key::ArrowDown);
    let writes: Vec<f64> = h
        .evals_since(before)
        .iter()
        .filter_map(|js| script_scroll_write(js))
        .collect();
    // First write from the first measurement, then the correction.
    assert_eq!(writes, vec![60.0, 70.0], "{writes:?}");
    assert_eq!(*measures.borrow(), 3, "measure, verify, verify again");
    assert!((h.state().scroll_top - 70.0).abs() < 0.01);
}

/// Review finding: a scroll-follow task whose row is no longer the active
/// row (a newer key press moved the cursor) must stop writing `scrollTop`.
#[test]
fn stale_scroll_follow_task_stops_writing() {
    let slot: Rc<RefCell<Option<UseTableHandle<Item>>>> = Rc::new(RefCell::new(None));
    let scroll = Rc::new(RefCell::new(0.0_f64));
    let reply: Reply = {
        let slot = slot.clone();
        let scroll = scroll.clone();
        Rc::new(move |js: &str| {
            if let Some(v) = script_scroll_write(js) {
                *scroll.borrow_mut() = v;
            }
            if !js.contains("chorale:measure-row") {
                return serde_json::Value::Null;
            }
            // While the task for row 87 measures, the cursor moves on (as a
            // second Page Down would), and the row is not rendered yet.
            if script_row_index(js) == Some(87) {
                if let Some(h) = *slot.borrow() {
                    let mut sig = h.signal();
                    let mut s = sig.peek().clone();
                    s.active_cell = Some(ActiveCell::new(3, ColumnId("name")));
                    sig.set(s);
                }
            }
            let st = *scroll.borrow();
            serde_json::json!([0, 0, 0, st, CLIENT_H, THEAD_H, THEAD_H])
        })
    };
    let mut h = Harness::with_reply(
        Cfg {
            virtualized: true,
            ..Cfg::default()
        },
        reply,
    );
    *slot.borrow_mut() = Some(h.handle());
    h.set_active(0, "name");
    let before = h.eval_count();
    h.press_mod(Key::End, Modifiers::CONTROL);
    let writes: Vec<f64> = h
        .evals_since(before)
        .iter()
        .filter_map(|js| script_scroll_write(js))
        .collect();
    assert!(
        writes.is_empty(),
        "the stale task for row 87 wrote scrollTop: {writes:?}"
    );
}

// ---------------------------------------------------------------------------
// Scroll-follow is keyboard-driven only
// ---------------------------------------------------------------------------

/// A mouse or trackpad scroll (the container's native scroll event) and a
/// focus event never run the reveal, so the view does not snap back to the
/// active row.
#[test]
fn scroll_that_is_not_keyboard_driven_keeps_scroll_top() {
    for virtualized in [false, true] {
        let cfg = Cfg {
            virtualized,
            ..Cfg::default()
        };
        let layout = PagedLayout::new(&cfg);
        let mut h = Harness::with_reply(cfg, layout.reply());
        h.set_active(1, "name");
        h.press(Key::ArrowDown);
        let before = h.eval_count();
        let sc = h.scroll_element();
        h.fire("scroll", wheel_scroll(900.0), sc);
        let kb = h.kb_element();
        h.fire("focus", Box::new(SerializedFocusData::default()), kb);
        h.fire("scroll", wheel_scroll(1300.0), sc);
        let scripts = h.evals_since(before);
        assert!(
            !scripts.iter().any(|js| js.contains("chorale:measure-row")
                || js.contains("chorale:reveal-row")
                || script_scroll_write(js).is_some()),
            "virtualized={virtualized}: a non-keyboard scroll ran scroll-follow: {scripts:?}"
        );
        assert!(
            (h.state().scroll_top - 1300.0).abs() < 0.01,
            "virtualized={virtualized}: scroll_top {}",
            h.state().scroll_top
        );
        assert_eq!(h.active().map(|a| a.row_idx), Some(2));
    }
}

/// A wheel scroll that lands while a keyboard reveal is still verifying its
/// write (the next measurement sees a scrollTop it did not write) ends the
/// reveal instead of scrolling back.
#[test]
fn wheel_scroll_during_reveal_verification_is_not_undone() {
    let measures = Rc::new(RefCell::new(0_usize));
    let scroll = Rc::new(RefCell::new(0.0_f64));
    let reply: Reply = {
        let measures = measures.clone();
        let scroll = scroll.clone();
        Rc::new(move |js: &str| {
            if let Some(v) = script_scroll_write(js) {
                *scroll.borrow_mut() = v;
            }
            if !js.contains("chorale:measure-row") {
                return serde_json::Value::Null;
            }
            let n = {
                let mut m = measures.borrow_mut();
                *m += 1;
                *m
            };
            if n == 2 {
                // The user wheel-scrolled down between the write and this
                // verification; row 12 (530..570) is now above the band
                // [641, 1100], so a reveal would scroll back up to 489.
                *scroll.borrow_mut() = 600.0;
            }
            let st = *scroll.borrow();
            let top = if n == 1 { 520.0 } else { 530.0 };
            serde_json::json!([1, top, DATA_H, st, CLIENT_H, THEAD_H, THEAD_H])
        })
    };
    let mut h = Harness::with_reply(
        Cfg {
            virtualized: true,
            ..Cfg::default()
        },
        reply,
    );
    h.set_active(11, "name");
    let before = h.eval_count();
    h.press(Key::ArrowDown);
    let writes: Vec<f64> = h
        .evals_since(before)
        .iter()
        .filter_map(|js| script_scroll_write(js))
        .collect();
    assert_eq!(
        writes,
        vec![60.0],
        "the reveal fought the wheel: {writes:?}"
    );
    assert!((*scroll.borrow() - 600.0).abs() < 0.01);
}
