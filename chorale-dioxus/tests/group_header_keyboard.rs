//! Keyboard navigation over grouped rows, driven through the real `Table`
//! component with real keydown events (issue #51).
//!
//! The harness mounts `Table` in a `VirtualDom`, reads the element ids of its
//! event listeners from the mutation stream, and sends `keydown` events through
//! `Runtime::handle_event` the way a renderer does. A fake `Document` records
//! every `eval` script the table runs and answers measurement scripts with
//! geometry chosen by the test, so the scroll-follow logic runs without a
//! browser. What only a browser can show (real layout, real scrolling) is
//! covered by the hand-test steps in `docs/QA.md` section 16a.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::task::{Context, Poll};

use chorale_core::{
    visible_grouped_view, ActiveCell, CellValue, ColumnDef, ColumnId, EditorKind,
    GroupedPaginationMode, GroupedRow, RowId, TableState,
};
use chorale_dioxus::{use_table, GroupHeaderContext, Table, UseTableHandle};
use dioxus::dioxus_core::{AttributeValue, ElementId, Event, Mutation, Mutations};
use dioxus::document::{Document, Eval, EvalError, Evaluator};
use dioxus::prelude::*;
use dioxus::signals::Owner;
use dioxus_html::{
    Code, Key, Location, Modifiers, PlatformEventData, SerializedHtmlEventConverter,
    SerializedKeyboardData,
};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

const GROUPS: usize = 8;
const ROWS_PER_GROUP: usize = 10;

#[derive(Clone, PartialEq, Debug)]
struct Item {
    group: String,
    name: String,
}

#[derive(Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
struct Cfg {
    hook: bool,
    selection: bool,
    virtualized: bool,
    /// Give the name column a text editor (for Enter-to-edit checks).
    editable: bool,
}

/// Props for the fixture app. `slot` hands the table handle back to the test.
#[derive(Clone)]
struct Props {
    cfg: Cfg,
    slot: Rc<RefCell<Option<UseTableHandle<Item>>>>,
}

impl PartialEq for Props {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

fn dataset() -> Vec<(RowId, Item)> {
    (0..GROUPS * ROWS_PER_GROUP)
        .map(|i| {
            (
                RowId::new(),
                Item {
                    group: format!("Group {}", i / ROWS_PER_GROUP),
                    name: format!("Item {i}"),
                },
            )
        })
        .collect()
}

#[allow(clippy::needless_pass_by_value)]
fn app(props: Props) -> Element {
    let cfg = props.cfg.clone();
    let table = use_table(move || {
        let name = ColumnDef::new(ColumnId("name"), "Name", |r: &Item| {
            CellValue::Text(r.name.clone())
        });
        let name = if cfg.editable {
            name.editor(EditorKind::Text)
        } else {
            name
        };
        let columns = vec![
            ColumnDef::new(ColumnId("group"), "Group", |r: &Item| {
                CellValue::Text(r.group.clone())
            }),
            name,
        ];
        let mut s = TableState::new(dataset(), columns);
        s.grouping = vec![ColumnId("group")];
        if cfg.virtualized {
            s.grouped_pagination = GroupedPaginationMode::Virtualized;
        }
        s
    });
    *props.slot.borrow_mut() = Some(table);
    let header_cb = Callback::new(|ctx: GroupHeaderContext| {
        rsx! {
            strong { "{ctx.label}" }
            select { aria_label: "Group action", option { "Keep" } }
        }
    });
    rsx! {
        Table::<Item> {
            handle: table,
            selection_enabled: props.cfg.selection,
            group_header_renderer: if props.cfg.hook { Some(header_cb) } else { None },
        }
    }
}

// ---------------------------------------------------------------------------
// Fake document: records eval scripts, answers measurement scripts
// ---------------------------------------------------------------------------

/// What the fake document answers for one script.
type Reply = Rc<dyn Fn(&str) -> serde_json::Value>;

struct FakeEval {
    value: serde_json::Value,
}

impl Evaluator for FakeEval {
    fn send(&self, _data: serde_json::Value) -> Result<(), EvalError> {
        Ok(())
    }
    fn poll_recv(&mut self, _: &mut Context<'_>) -> Poll<Result<serde_json::Value, EvalError>> {
        Poll::Ready(Ok(self.value.clone()))
    }
    fn poll_join(&mut self, _: &mut Context<'_>) -> Poll<Result<serde_json::Value, EvalError>> {
        Poll::Ready(Ok(self.value.clone()))
    }
}

struct FakeDocument {
    log: Rc<RefCell<Vec<String>>>,
    reply: Reply,
    // Keeps every eval's storage alive; dropping an owner ends its eval.
    owners: RefCell<Vec<Owner>>,
}

impl Document for FakeDocument {
    fn eval(&self, js: String) -> Eval {
        let value = (self.reply)(&js);
        self.log.borrow_mut().push(js);
        let owner = Owner::default();
        let boxed: Box<dyn Evaluator> = Box::new(FakeEval { value });
        let eval = Eval::new(owner.insert(boxed));
        self.owners.borrow_mut().push(owner);
        eval
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct Harness {
    dom: VirtualDom,
    slot: Rc<RefCell<Option<UseTableHandle<Item>>>>,
    evals: Rc<RefCell<Vec<String>>>,
    /// Element id -> value of its dynamic `id` attribute.
    dom_ids: HashMap<ElementId, String>,
    /// Element id -> value of its dynamic `style` attribute.
    styles: HashMap<ElementId, String>,
    /// (event name, element id) of every live listener.
    listeners: Vec<(String, ElementId)>,
}

fn null_reply() -> Reply {
    Rc::new(|_| serde_json::Value::Null)
}

impl Harness {
    fn new(cfg: Cfg) -> Self {
        Self::with_reply(cfg, null_reply())
    }

    fn with_reply(cfg: Cfg, reply: Reply) -> Self {
        dioxus_html::set_event_converter(Box::new(SerializedHtmlEventConverter));
        let slot = Rc::new(RefCell::new(None));
        let evals = Rc::new(RefCell::new(Vec::new()));
        let dom = VirtualDom::new_with_props(
            app,
            Props {
                cfg,
                slot: slot.clone(),
            },
        );
        let doc: Rc<dyn Document> = Rc::new(FakeDocument {
            log: evals.clone(),
            reply,
            owners: RefCell::new(Vec::new()),
        });
        dom.provide_root_context(doc);
        let mut h = Self {
            dom,
            slot,
            evals,
            dom_ids: HashMap::new(),
            styles: HashMap::new(),
            listeners: Vec::new(),
        };
        let muts = h.dom.rebuild_to_vec();
        h.record(&muts);
        h.settle();
        h
    }

    fn record(&mut self, muts: &Mutations) {
        for m in &muts.edits {
            match m {
                Mutation::SetAttribute {
                    name,
                    value: AttributeValue::Text(v),
                    id,
                    ..
                } => {
                    if *name == "id" {
                        self.dom_ids.insert(*id, v.clone());
                    } else if *name == "style" {
                        self.styles.insert(*id, v.clone());
                    }
                }
                Mutation::NewEventListener { name, id } => {
                    self.listeners.push((name.clone(), *id));
                }
                Mutation::RemoveEventListener { name, id } => {
                    self.listeners.retain(|(n, i)| !(n == name && i == id));
                }
                _ => {}
            }
        }
    }

    /// Run spawned tasks and re-render until nothing is left to do.
    fn settle(&mut self) {
        for _ in 0..20 {
            self.dom.process_events();
            let muts = self.dom.render_immediate_to_vec();
            self.record(&muts);
        }
    }

    fn handle(&self) -> UseTableHandle<Item> {
        self.slot.borrow().expect("table mounted")
    }

    fn state(&self) -> TableState<Item> {
        let h = self.handle();
        self.dom.in_runtime(|| h.signal().peek().clone())
    }

    fn active(&self) -> Option<ActiveCell> {
        self.state().active_cell
    }

    fn set_active(&mut self, row_idx: usize, col: &'static str) {
        let h = self.handle();
        self.dom.in_runtime(|| {
            let mut sig = h.signal();
            let mut s = sig.peek().clone();
            s.active_cell = Some(ActiveCell::new(row_idx, ColumnId(col)));
            s.range_selection = vec![chorale_core::RangeSelection::single(row_idx, ColumnId(col))];
            sig.set(s);
        });
        self.settle();
    }

    /// The table's keyboard container: the element whose dynamic id ends in `-kb`.
    fn kb_element(&self) -> ElementId {
        let (_, id) = self
            .listeners
            .iter()
            .find(|(n, id)| {
                n == "keydown" && self.dom_ids.get(id).is_some_and(|v| v.ends_with("-kb"))
            })
            .expect("keyboard container listener");
        *id
    }

    fn press_on(&mut self, element: ElementId, key: Key, modifiers: Modifiers) {
        let data = SerializedKeyboardData::new(
            key,
            Code::Unidentified,
            Location::Standard,
            false,
            modifiers,
            false,
        );
        let payload: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(data)));
        self.dom
            .runtime()
            .handle_event("keydown", Event::new(payload, true), element);
        self.settle();
    }

    fn press(&mut self, key: Key) {
        let kb = self.kb_element();
        self.press_on(kb, key, Modifiers::empty());
    }

    fn press_mod(&mut self, key: Key, modifiers: Modifiers) {
        let kb = self.kb_element();
        self.press_on(kb, key, modifiers);
    }

    fn html(&self) -> String {
        dioxus_ssr::render(&self.dom)
    }

    /// Opening tag of the `<tr>` that renders grouped row `idx` (a data row
    /// carries `data-chorale-index`, a header `data-chorale-group-index`).
    fn tr_tag(&self, idx: usize) -> Option<String> {
        let html = self.html();
        let data = format!(" data-chorale-index=\"{idx}\"");
        let header = format!(" data-chorale-group-index=\"{idx}\"");
        html.split("<tr ")
            .skip(1)
            .map(|t| t.split('>').next().unwrap_or("").to_owned())
            .find(|t| t.contains(&data) || t.contains(&header))
    }

    fn evals_since(&self, from: usize) -> Vec<String> {
        self.evals.borrow()[from..].to_vec()
    }

    fn eval_count(&self) -> usize {
        self.evals.borrow().len()
    }
}

fn view_of(s: &TableState<Item>) -> Vec<GroupedRow<Item>> {
    visible_grouped_view(s)
}

fn is_header(s: &TableState<Item>, idx: usize) -> bool {
    matches!(view_of(s).get(idx), Some(GroupedRow::Header { .. }))
}

fn header_collapsed(s: &TableState<Item>, idx: usize) -> bool {
    match view_of(s).get(idx) {
        Some(GroupedRow::Header { is_collapsed, .. }) => *is_collapsed,
        _ => panic!("row {idx} is not a header"),
    }
}

const ON_OFF: [bool; 2] = [true, false];

// ---------------------------------------------------------------------------
// Fix 1: header rows are real arrow-key stops
// ---------------------------------------------------------------------------

#[test]
fn up_from_first_data_row_lands_on_header_with_visible_cursor() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        // Grouped row 1 is Item 0, the first data row of Group 0.
        h.set_active(1, "name");
        h.press(Key::ArrowUp);
        assert_eq!(h.active().map(|a| a.row_idx), Some(0), "hook={hook}");
        let tag = h.tr_tag(0);
        assert!(
            tag.is_some(),
            "hook={hook}: header row 0 carries no row index"
        );
        let tag = tag.unwrap();
        assert!(tag.contains("chorale-group-header"), "hook={hook}: {tag}");
        assert!(
            tag.contains("data-chorale-active-row=\"true\""),
            "hook={hook}: header shows no cursor: {tag}"
        );
        // Only the active row carries the cursor.
        let other = h.tr_tag(11).unwrap();
        assert!(
            other.contains("data-chorale-active-row=\"false\""),
            "hook={hook}: {other}"
        );
    }
}

#[test]
fn down_crosses_group_boundary_through_the_header() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        // Row 10 is Item 9, the last data row of Group 0; row 11 is Group 1's header.
        h.set_active(10, "name");
        h.press(Key::ArrowDown);
        assert_eq!(h.active().map(|a| a.row_idx), Some(11), "hook={hook}");
        assert!(is_header(&h.state(), 11));
        h.press(Key::ArrowDown);
        let ac = h.active().unwrap();
        assert_eq!(
            ac.row_idx, 12,
            "hook={hook}: arrows continue into data rows"
        );
        // The column is remembered across the header.
        assert_eq!(ac.column_id, ColumnId("name"), "hook={hook}");
        h.press(Key::ArrowUp);
        h.press(Key::ArrowUp);
        assert_eq!(h.active().map(|a| a.row_idx), Some(10), "hook={hook}");
    }
}

#[test]
fn left_collapses_right_expands_header() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        h.set_active(11, "name");
        h.press(Key::ArrowLeft);
        assert!(
            header_collapsed(&h.state(), 11),
            "hook={hook}: Left collapses"
        );
        assert_eq!(h.active().map(|a| a.row_idx), Some(11), "hook={hook}");
        // Left again on a collapsed header changes nothing.
        h.press(Key::ArrowLeft);
        assert!(header_collapsed(&h.state(), 11), "hook={hook}");
        h.press(Key::ArrowRight);
        assert!(
            !header_collapsed(&h.state(), 11),
            "hook={hook}: Right expands"
        );
        assert_eq!(h.active().map(|a| a.row_idx), Some(11), "hook={hook}");
        h.press(Key::ArrowRight);
        assert!(!header_collapsed(&h.state(), 11), "hook={hook}");
        // The header row reports its state through aria-expanded.
        let tag = h.tr_tag(11).unwrap();
        assert!(tag.contains("aria-expanded=\"true\""), "hook={hook}: {tag}");
    }
}

#[test]
fn enter_and_space_toggle_header() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            ..Cfg::default()
        });
        h.set_active(0, "name");
        h.press(Key::Enter);
        assert!(
            header_collapsed(&h.state(), 0),
            "hook={hook}: Enter collapses"
        );
        h.press(Key::Enter);
        assert!(
            !header_collapsed(&h.state(), 0),
            "hook={hook}: Enter expands"
        );
        h.press(Key::Character(" ".into()));
        assert!(
            header_collapsed(&h.state(), 0),
            "hook={hook}: Space collapses"
        );
        h.press(Key::Character(" ".into()));
        assert!(
            !header_collapsed(&h.state(), 0),
            "hook={hook}: Space expands"
        );
        assert_eq!(h.active().map(|a| a.row_idx), Some(0), "hook={hook}");
    }
}

#[test]
fn down_from_collapsed_header_goes_to_next_header() {
    let mut h = Harness::new(Cfg {
        hook: true,
        ..Cfg::default()
    });
    h.set_active(0, "name");
    h.press(Key::ArrowLeft);
    h.press(Key::ArrowDown);
    // Group 0 is collapsed, so row 1 is now Group 1's header.
    assert_eq!(h.active().map(|a| a.row_idx), Some(1));
    assert!(is_header(&h.state(), 1));
}

#[test]
fn every_grouped_row_is_reachable_in_virtualized_mode() {
    for hook in ON_OFF {
        let mut h = Harness::new(Cfg {
            hook,
            virtualized: true,
            ..Cfg::default()
        });
        let total = view_of(&h.state()).len();
        assert_eq!(total, GROUPS * (ROWS_PER_GROUP + 1));
        // Past the data-row page size the cursor used to stop moving.
        h.set_active(49, "name");
        h.press(Key::ArrowDown);
        assert_eq!(h.active().map(|a| a.row_idx), Some(50), "hook={hook}");
        h.press_mod(Key::ArrowDown, Modifiers::CONTROL);
        assert_eq!(
            h.active().map(|a| a.row_idx),
            Some(total - 1),
            "hook={hook}: Ctrl+Down reaches the last grouped row"
        );
        h.press_mod(Key::End, Modifiers::CONTROL);
        assert_eq!(h.active().map(|a| a.row_idx), Some(total - 1));
        h.press_mod(Key::Home, Modifiers::CONTROL);
        assert_eq!(h.active().map(|a| a.row_idx), Some(0), "hook={hook}");
    }
}

#[test]
fn page_down_and_horizontal_moves_keep_grouped_row() {
    let mut h = Harness::new(Cfg {
        virtualized: true,
        ..Cfg::default()
    });
    let total = view_of(&h.state()).len();
    // Row 70 is past the data-row count of one page; Left/Right must not clamp it.
    h.set_active(70, "name");
    h.press(Key::ArrowLeft);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("group"))));
    h.press(Key::ArrowRight);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("name"))));
    h.press(Key::Home);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("group"))));
    h.press(Key::End);
    assert_eq!(h.active(), Some(ActiveCell::new(70, ColumnId("name"))));
    h.press(Key::PageDown);
    assert_eq!(h.active().map(|a| a.row_idx), Some(total - 1));
    h.press(Key::PageUp);
    assert_eq!(h.active().map(|a| a.row_idx), Some(total - 1 - 50));
}

#[test]
fn tab_from_header_focuses_its_first_control() {
    for (hook, selection) in [(true, false), (true, true), (false, true)] {
        let mut h = Harness::new(Cfg {
            hook,
            selection,
            ..Cfg::default()
        });
        h.set_active(11, "name");
        let before = h.eval_count();
        h.press(Key::Tab);
        let scripts = h.evals_since(before);
        let focus = scripts.iter().find(|js| {
            js.contains("tr[data-chorale-group-index=\"11\"]") && js.contains(".focus()")
        });
        assert!(
            focus.is_some(),
            "hook={hook} selection={selection}: no focus script for header 11: {scripts:?}"
        );
        // The cursor stays on the header; Tab does not move it to a cell.
        assert_eq!(
            h.active(),
            Some(ActiveCell::new(11, ColumnId("name"))),
            "hook={hook} selection={selection}"
        );
    }
}

#[test]
fn tab_from_header_without_controls_keeps_cursor() {
    let mut h = Harness::new(Cfg::default());
    h.set_active(11, "group");
    h.press(Key::Tab);
    assert_eq!(h.active(), Some(ActiveCell::new(11, ColumnId("group"))));
}

#[test]
fn keys_inside_host_header_content_stay_there() {
    let mut h = Harness::new(Cfg {
        hook: true,
        ..Cfg::default()
    });
    h.set_active(1, "name");
    // The host content wrapper span (its style pins `inline-block`).
    let span = h
        .listeners
        .iter()
        .find(|(n, id)| {
            n == "keydown"
                && h.styles
                    .get(id)
                    .is_some_and(|s| s.starts_with("display: inline-block"))
        })
        .map(|(_, id)| *id)
        .expect("host content span listener");
    for key in [Key::ArrowDown, Key::ArrowUp, Key::Enter, Key::Tab] {
        h.press_on(span, key, Modifiers::empty());
    }
    h.press_on(span, Key::Character(" ".into()), Modifiers::empty());
    assert_eq!(h.active(), Some(ActiveCell::new(1, ColumnId("name"))));
    assert!(!header_collapsed(&h.state(), 0));
}

#[test]
fn enter_on_grouped_data_row_edits_that_row() {
    let mut h = Harness::new(Cfg {
        editable: true,
        ..Cfg::default()
    });
    // Row 13 is Item 11 (Group 1's header is row 11).
    h.set_active(13, "name");
    h.press(Key::Enter);
    let s = h.state();
    let expected = match view_of(&s).get(13) {
        Some(GroupedRow::Data(id, row)) => {
            assert_eq!(row.name, "Item 11");
            *id
        }
        _ => panic!("row 13 is not a data row"),
    };
    let editing = s.editing.expect("Enter starts an edit");
    assert_eq!(
        editing.row_id, expected,
        "Enter edits the row under the cursor"
    );
    // Enter on a header never starts an edit.
    let mut h = Harness::new(Cfg {
        editable: true,
        ..Cfg::default()
    });
    h.set_active(11, "name");
    h.press(Key::Enter);
    assert!(h.state().editing.is_none());
}

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

// ---------------------------------------------------------------------------
// Fix 2: scroll-follow keeps the active row fully visible
// ---------------------------------------------------------------------------

/// Index `N` from the first `data-chorale-index="N"` in a script.
fn script_row_index(js: &str) -> Option<usize> {
    let at = js.find("data-chorale-index=\"")? + "data-chorale-index=\"".len();
    js[at..].split('"').next()?.parse().ok()
}

/// Value `X` of the last `scrollTop = X;` write in a script.
fn script_scroll_write(js: &str) -> Option<f64> {
    let at = js.rfind("scrollTop = ")? + "scrollTop = ".len();
    js[at..].split(';').next()?.trim().parse().ok()
}

const THEAD_H: f64 = 41.0;
const CLIENT_H: f64 = 500.0;
const DATA_H: f64 = 40.0;
const HEADER_H: f64 = 37.0;

/// A Paged-mode browser stand-in: every row of the page is rendered, header
/// rows are 37px and data rows 40px (`row_height` is 40), under a 41px sticky
/// `<thead>`. It answers row measurements from that layout and applies the
/// `scrollTop` writes the table sends.
#[derive(Clone)]
struct PagedLayout {
    headers: Rc<Vec<bool>>,
    scroll_top: Rc<RefCell<f64>>,
}

impl PagedLayout {
    fn new(cfg: &Cfg) -> Self {
        // The fixture's grouped view, built the same way the app builds it.
        let mut s = TableState::new(
            dataset(),
            vec![ColumnDef::new(ColumnId("group"), "Group", |r: &Item| {
                CellValue::Text(r.group.clone())
            })],
        );
        s.grouping = vec![ColumnId("group")];
        if cfg.virtualized {
            s.grouped_pagination = GroupedPaginationMode::Virtualized;
        }
        let headers = visible_grouped_view(&s)
            .iter()
            .map(|r| matches!(r, GroupedRow::Header { .. }))
            .collect();
        Self {
            headers: Rc::new(headers),
            scroll_top: Rc::new(RefCell::new(0.0)),
        }
    }

    fn height(&self, idx: usize) -> f64 {
        if self.headers[idx] {
            HEADER_H
        } else {
            DATA_H
        }
    }

    /// Row top in scroll-content coordinates.
    fn top(&self, idx: usize) -> f64 {
        THEAD_H + (0..idx).map(|i| self.height(i)).sum::<f64>()
    }

    fn reply(&self) -> Reply {
        let me = self.clone();
        Rc::new(move |js: &str| {
            if let Some(v) = script_scroll_write(js) {
                *me.scroll_top.borrow_mut() = v;
            }
            if !js.contains("chorale:measure-row") {
                return serde_json::Value::Null;
            }
            let st = *me.scroll_top.borrow();
            match script_row_index(js) {
                Some(i) if i < me.headers.len() => {
                    serde_json::json!([1, me.top(i), me.height(i), st, CLIENT_H, THEAD_H, THEAD_H])
                }
                _ => serde_json::json!([0, 0, 0, st, CLIENT_H, THEAD_H, THEAD_H]),
            }
        })
    }

    /// Whether row `idx` lies fully inside the band below the sticky header.
    fn fully_visible(&self, idx: usize) -> bool {
        let st = *self.scroll_top.borrow();
        let top = self.top(idx);
        top >= st + THEAD_H - 0.5 && top + self.height(idx) <= st + CLIENT_H + 0.5
    }
}

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

/// Switching to Virtualized at runtime (the test page's select) must
/// re-derive the grouped view: every group, not the first page's rows.
#[test]
fn runtime_switch_to_virtualized_renders_and_reaches_every_group() {
    let mut h = Harness::new(Cfg::default());
    let handle = h.handle();
    h.dom
        .in_runtime(|| handle.set_grouped_pagination(GroupedPaginationMode::Virtualized));
    h.settle();
    let total = GROUPS * (ROWS_PER_GROUP + 1);
    assert_eq!(view_of(&h.state()).len(), total);
    h.set_active(0, "name");
    h.press_mod(Key::End, Modifiers::CONTROL);
    assert_eq!(
        h.active().map(|a| a.row_idx),
        Some(total - 1),
        "Ctrl+End reaches the last group's last row"
    );
    // The last row is rendered once it is the target (bottom of the window).
    assert!(
        h.tr_tag(total - 1).is_some(),
        "row {} is not rendered",
        total - 1
    );
}
