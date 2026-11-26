//! Shared harness for the grouped-table keyboard and scroll tests (issue #51).
//!
//! It mounts the real `Table` in a `VirtualDom`, reads the element ids of its
//! event listeners from the mutation stream, and sends events (keydown,
//! scroll, focus, click) through `Runtime::handle_event` the way a renderer
//! does. A fake `Document` records every `eval` script the table runs and
//! answers measurement scripts with geometry chosen by the test, so the
//! scroll-follow logic runs without a browser. What only a browser can show
//! (real layout, real scrolling) is covered by the hand-test steps in
//! `docs/QA.md` section 16a.

#![allow(dead_code, unused_imports, clippy::unwrap_used, clippy::expect_used)]

pub use std::any::Any;
pub use std::cell::RefCell;
pub use std::collections::HashMap;
pub use std::rc::Rc;
use std::task::{Context, Poll};

pub use chorale_core::{
    visible_grouped_view, ActiveCell, CellValue, ColumnDef, ColumnId, EditorKind,
    GroupedPaginationMode, GroupedRow, RowId, TableState,
};
pub use chorale_dioxus::{use_table, GroupHeaderContext, Table, UseTableHandle};
pub use dioxus::dioxus_core::{AttributeValue, ElementId, Event, Mutation, Mutations};
use dioxus::document::{Document, Eval, EvalError, Evaluator};
pub use dioxus::prelude::*;
use dioxus::signals::Owner;
pub use dioxus_html::{
    Code, Key, Location, Modifiers, PlatformEventData, SerializedFocusData,
    SerializedHtmlEventConverter, SerializedKeyboardData, SerializedMouseData,
    SerializedScrollData,
};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

pub const GROUPS: usize = 8;
pub const ROWS_PER_GROUP: usize = 10;

#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub group: String,
    pub name: String,
}

#[derive(Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cfg {
    pub hook: bool,
    pub selection: bool,
    pub virtualized: bool,
    /// Give the name column a text editor (for Enter-to-edit checks).
    pub editable: bool,
    /// Set a detail renderer (adds the detail-expander column).
    pub detail: bool,
    /// No grouping at all.
    pub ungrouped: bool,
    /// Override `page_size` (default 50).
    pub page_size: Option<usize>,
    /// Initial `scroll_top` (so a virtualized window starts away from the top).
    pub initial_scroll: f64,
}

/// Props for the fixture app. `slot` hands the table handle back to the test.
#[derive(Clone)]
pub struct Props {
    pub cfg: Cfg,
    pub slot: Rc<RefCell<Option<UseTableHandle<Item>>>>,
}

impl PartialEq for Props {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

pub fn dataset() -> Vec<(RowId, Item)> {
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
pub fn app(props: Props) -> Element {
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
        if !cfg.ungrouped {
            s.grouping = vec![ColumnId("group")];
        }
        if cfg.virtualized {
            s.grouped_pagination = GroupedPaginationMode::Virtualized;
        }
        if let Some(n) = cfg.page_size {
            s.page_size = n;
        }
        s.scroll_top = cfg.initial_scroll;
        s
    });
    *props.slot.borrow_mut() = Some(table);
    let header_cb = Callback::new(|ctx: GroupHeaderContext| {
        rsx! {
            strong { "{ctx.label}" }
            select { aria_label: "Group action", option { "Keep" } }
        }
    });
    let detail_cb = Callback::new(|item: Item| rsx! { p { "detail {item.name}" } });
    rsx! {
        Table::<Item> {
            handle: table,
            selection_enabled: props.cfg.selection,
            group_header_renderer: if props.cfg.hook { Some(header_cb) } else { None },
            detail_renderer: if props.cfg.detail { Some(detail_cb) } else { None },
        }
    }
}

// ---------------------------------------------------------------------------
// Fake document: records eval scripts, answers measurement scripts
// ---------------------------------------------------------------------------

/// What the fake document answers for one script.
pub type Reply = Rc<dyn Fn(&str) -> serde_json::Value>;

pub struct FakeEval {
    pub value: serde_json::Value,
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

pub struct FakeDocument {
    pub log: Rc<RefCell<Vec<String>>>,
    pub reply: Reply,
    // Keeps every eval's storage alive; dropping an owner ends its eval.
    pub owners: RefCell<Vec<Owner>>,
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

pub struct Harness {
    pub dom: VirtualDom,
    pub slot: Rc<RefCell<Option<UseTableHandle<Item>>>>,
    pub evals: Rc<RefCell<Vec<String>>>,
    /// Element id -> value of its dynamic `id` attribute.
    pub dom_ids: HashMap<ElementId, String>,
    /// Element id -> value of its dynamic `style` attribute.
    pub styles: HashMap<ElementId, String>,
    /// (event name, element id) of every live listener.
    pub listeners: Vec<(String, ElementId)>,
}

pub fn null_reply() -> Reply {
    Rc::new(|_| serde_json::Value::Null)
}

impl Harness {
    pub fn new(cfg: Cfg) -> Self {
        Self::with_reply(cfg, null_reply())
    }

    pub fn with_reply(cfg: Cfg, reply: Reply) -> Self {
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

    pub fn record(&mut self, muts: &Mutations) {
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
    pub fn settle(&mut self) {
        for _ in 0..20 {
            self.dom.process_events();
            let muts = self.dom.render_immediate_to_vec();
            self.record(&muts);
        }
    }

    pub fn handle(&self) -> UseTableHandle<Item> {
        self.slot.borrow().expect("table mounted")
    }

    pub fn state(&self) -> TableState<Item> {
        let h = self.handle();
        self.dom.in_runtime(|| h.signal().peek().clone())
    }

    pub fn active(&self) -> Option<ActiveCell> {
        self.state().active_cell
    }

    pub fn set_active(&mut self, row_idx: usize, col: &'static str) {
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
    pub fn kb_element(&self) -> ElementId {
        let (_, id) = self
            .listeners
            .iter()
            .find(|(n, id)| {
                n == "keydown" && self.dom_ids.get(id).is_some_and(|v| v.ends_with("-kb"))
            })
            .expect("keyboard container listener");
        *id
    }

    pub fn press_on(&mut self, element: ElementId, key: Key, modifiers: Modifiers) {
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

    /// Send any event with `data` as its platform payload to `element`.
    pub fn fire(&mut self, name: &str, data: Box<dyn Any>, element: ElementId) {
        let payload: Rc<dyn Any> = Rc::new(PlatformEventData::new(data));
        self.dom
            .runtime()
            .handle_event(name, Event::new(payload, true), element);
        self.settle();
    }

    /// The table's scroll container (dynamic id `chorale-scroll-N`).
    pub fn scroll_element(&self) -> ElementId {
        let (_, id) =
            self.listeners
                .iter()
                .find(|(n, id)| {
                    n == "scroll"
                        && self.dom_ids.get(id).is_some_and(|v| {
                            v.starts_with("chorale-scroll-") && !v.ends_with("-kb")
                        })
                })
                .expect("scroll container listener");
        *id
    }

    /// Like `press_on`, and report whether the browser default stays enabled
    /// (`true` means the handler did not call `prevent_default`).
    pub fn press_default(&mut self, element: ElementId, key: Key, modifiers: Modifiers) -> bool {
        let data = SerializedKeyboardData::new(
            key,
            Code::Unidentified,
            Location::Standard,
            false,
            modifiers,
            false,
        );
        let payload: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(data)));
        let event = Event::new(payload, true);
        let probe = event.clone();
        self.dom.runtime().handle_event("keydown", event, element);
        self.settle();
        probe.default_action_enabled()
    }

    /// Element ids of the rendered data cells (`<td>`s with a click
    /// listener and the fixed data-cell style), in document order. Valid
    /// right after `Harness::new` (first render); `cells_per_row` cells each.
    pub fn data_cells(&self) -> Vec<ElementId> {
        self.listeners
            .iter()
            .filter(|(n, id)| {
                n == "click"
                    && self
                        .styles
                        .get(id)
                        .is_some_and(|s| s.starts_with("position: relative; padding: 0.5rem 1rem;"))
            })
            .map(|(_, id)| *id)
            .collect()
    }

    /// Plain left click on `element`.
    pub fn click(&mut self, element: ElementId, modifiers: Modifiers) {
        use dioxus_html::geometry::{ClientPoint, ElementPoint, PagePoint, ScreenPoint};
        let data = SerializedMouseData::new(
            Some(dioxus_html::input_data::MouseButton::Primary),
            dioxus_html::input_data::MouseButtonSet::empty(),
            dioxus_html::geometry::Coordinates::new(
                ScreenPoint::origin(),
                ClientPoint::origin(),
                ElementPoint::origin(),
                PagePoint::origin(),
            ),
            modifiers,
        );
        self.fire("click", Box::new(data), element);
    }

    /// Opening tag of the keyboard container `<div>`.
    pub fn kb_tag(&self) -> String {
        let html = self.html();
        html.split("<div ")
            .skip(1)
            .map(|t| t.split('>').next().unwrap_or("").to_owned())
            .find(|t| t.contains("-kb\""))
            .unwrap_or_default()
    }

    /// Value of attribute `name` in an opening tag.
    pub fn attr(tag: &str, name: &str) -> Option<String> {
        let needle = format!(" {name}=\"");
        let at = tag.find(&needle)? + needle.len();
        Some(tag[at..].split('"').next()?.to_owned())
    }

    /// Indexes of the grouped rows rendered right now, in document order.
    pub fn rendered_rows(&self) -> Vec<usize> {
        self.html()
            .split("<tr ")
            .skip(1)
            .filter_map(|t| {
                let tag = t.split('>').next().unwrap_or("");
                Self::attr(tag, "data-chorale-index")
                    .or_else(|| Self::attr(tag, "data-chorale-group-index"))
                    .and_then(|v| v.parse().ok())
            })
            .collect()
    }

    pub fn press(&mut self, key: Key) {
        let kb = self.kb_element();
        self.press_on(kb, key, Modifiers::empty());
    }

    pub fn press_mod(&mut self, key: Key, modifiers: Modifiers) {
        let kb = self.kb_element();
        self.press_on(kb, key, modifiers);
    }

    pub fn html(&self) -> String {
        dioxus_ssr::render(&self.dom)
    }

    /// Opening tag of the `<tr>` that renders grouped row `idx` (a data row
    /// carries `data-chorale-index`, a header `data-chorale-group-index`).
    pub fn tr_tag(&self, idx: usize) -> Option<String> {
        let html = self.html();
        let data = format!(" data-chorale-index=\"{idx}\"");
        let header = format!(" data-chorale-group-index=\"{idx}\"");
        html.split("<tr ")
            .skip(1)
            .map(|t| t.split('>').next().unwrap_or("").to_owned())
            .find(|t| t.contains(&data) || t.contains(&header))
    }

    pub fn evals_since(&self, from: usize) -> Vec<String> {
        self.evals.borrow()[from..].to_vec()
    }

    pub fn eval_count(&self) -> usize {
        self.evals.borrow().len()
    }
}

pub fn view_of(s: &TableState<Item>) -> Vec<GroupedRow<Item>> {
    visible_grouped_view(s)
}

pub fn is_header(s: &TableState<Item>, idx: usize) -> bool {
    matches!(view_of(s).get(idx), Some(GroupedRow::Header { .. }))
}

pub fn header_collapsed(s: &TableState<Item>, idx: usize) -> bool {
    match view_of(s).get(idx) {
        Some(GroupedRow::Header { is_collapsed, .. }) => *is_collapsed,
        _ => panic!("row {idx} is not a header"),
    }
}

pub const ON_OFF: [bool; 2] = [true, false];

// ---------------------------------------------------------------------------
// Fix 2: scroll-follow keeps the active row fully visible
// ---------------------------------------------------------------------------

/// Index `N` from the first `data-chorale-index="N"` in a script.
pub fn script_row_index(js: &str) -> Option<usize> {
    let at = js.find("data-chorale-index=\"")? + "data-chorale-index=\"".len();
    js[at..].split('"').next()?.parse().ok()
}

/// Value `X` of the last `scrollTop = X;` write in a script.
pub fn script_scroll_write(js: &str) -> Option<f64> {
    let at = js.rfind("scrollTop = ")? + "scrollTop = ".len();
    js[at..].split(';').next()?.trim().parse().ok()
}

pub const THEAD_H: f64 = 41.0;
pub const CLIENT_H: f64 = 500.0;
pub const DATA_H: f64 = 40.0;
pub const HEADER_H: f64 = 37.0;

/// A Paged-mode browser stand-in: every row of the page is rendered, header
/// rows are 37px and data rows 40px (`row_height` is 40), under a 41px sticky
/// `<thead>`. It answers row measurements from that layout and applies the
/// `scrollTop` writes the table sends.
#[derive(Clone)]
pub struct PagedLayout {
    pub headers: Rc<Vec<bool>>,
    pub scroll_top: Rc<RefCell<f64>>,
}

impl PagedLayout {
    pub fn new(cfg: &Cfg) -> Self {
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

    pub fn height(&self, idx: usize) -> f64 {
        if self.headers[idx] {
            HEADER_H
        } else {
            DATA_H
        }
    }

    /// Row top in scroll-content coordinates.
    pub fn top(&self, idx: usize) -> f64 {
        THEAD_H + (0..idx).map(|i| self.height(i)).sum::<f64>()
    }

    pub fn reply(&self) -> Reply {
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
    pub fn fully_visible(&self, idx: usize) -> bool {
        let st = *self.scroll_top.borrow();
        let top = self.top(idx);
        top >= st + THEAD_H - 0.5 && top + self.height(idx) <= st + CLIENT_H + 0.5
    }
}

// ---------------------------------------------------------------------------
// Scroll-follow is keyboard-driven only
// ---------------------------------------------------------------------------

pub fn wheel_scroll(scroll_top: f64) -> Box<dyn Any> {
    Box::new(SerializedScrollData {
        scroll_top,
        scroll_left: 0.0,
        scroll_width: 1000,
        scroll_height: 4000,
        client_width: 1000,
        client_height: 500,
    })
}
