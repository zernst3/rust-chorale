# Overnight bot session — 2026-06-05 (session 2)
## Branch: draft-release/v0.2.0

### What was completed

This session resumed from a context-limit handoff. The `chorale-leptos` adapter
was blocked by a Leptos macro parse error; all remaining v0.2.0 items were then
completed sequentially.

| Item | Status |
|---|---|
| **11.5** — chorale-leptos adapter (blocked) | ✅ Complete |
| **11.7** — Leptos examples | ✅ Complete |
| **13** — Release readiness gate | ✅ Complete |

---

### Item 11.5 fix detail

**Root cause of `#[component]` parse error:**
`#[prop(default)]` without `= expr` is not valid syntax in Leptos 0.8.
The macro expected `default = <expression>` but found `)]`. Fix: replace
`#[prop(default)]` with `#[prop(default = CellRenderers::default())]` and
`#[prop(default = ValidateEditFn::default())]`.

**Secondary issues resolved:**
- `Labels` (`Arc<Labels>`) captured by multiple `move` closures in the same
  `view!` block. Fix: wrap each of the three `{move || { … }}` blocks in
  `{{ let labels = labels.clone(); move || { … } }}` to clone the Arc cheaply
  before each capture.
- `chrono::NaiveDate` used directly. Fix: use `chorale_core::NaiveDate` (the
  chorale-core re-export) so no direct `chrono` dep needed.
- 15 clippy pedantic violations fixed: `i32 as f64` → `f64::from()`, bare
  match arms with identical bodies, `#[prop(default)]` bare form, too-many-lines
  / too-many-args / too-many-bools — addressed with targeted `#[allow]`.

---

### Item 11.7 detail

7 Leptos example crates created, each mirroring a Dioxus example:

| Leptos crate | Mirrors | Build |
|---|---|---|
| `leptos-basic` | `basic` | `trunk serve --open` |
| `leptos-with-selection` | `with-selection` | `trunk serve --open` |
| `leptos-with-custom-cells` | `with-custom-cells` | `trunk serve --open` |
| `leptos-with-column-resize` | `with-column-resize` | `trunk serve --open` |
| `leptos-virtualized-10k-rows` | `virtualized-10k-rows` | `trunk serve --open` |
| `leptos-virtualized-1m-rows` | `virtualized-1m-rows` | `trunk serve --open` |
| `leptos-qa-harness` | `qa-harness` | `trunk serve --open` |

Key differences from Dioxus:
- `use_chorale_table(rows: Vec<TRow>, cols)` vs `use_table(|| TableState::new(rows_with_ids, cols))`
- `view! { ... }` vs `rsx! { ... }`
- `CellRenderer = Arc<dyn Fn(&CellValue) -> AnyView>` vs Dioxus `Element`
- 1M-row example uses `RwSignal<Option<UseTableHandle<Event>>>` + `Effect::new` for two-stage mount

---

### Item 13 detail

Release gate checklist completed:

- [x] All 14 example crates `cargo check` clean (7 Dioxus + 7 Leptos)
- [x] `cargo test --workspace` — 263 tests, 0 failures
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] `cargo fmt --all -- --check` clean
- [x] `cargo doc --workspace --no-deps` clean (fixed 2 broken intra-doc links)
- [x] `docs/QA.md` extended with v0.2.0 verification recipes + Leptos parity checklist
- [x] README updated: v0.2.0 feature list, Leptos quickstart, framework comparison table
- [x] CHANGELOG `[Unreleased]` promoted to `[0.2.0] — 2026-06-05`
- [x] Workspace version bumped to `0.2.0` across all `Cargo.toml` files

---

### Push status

**`git push` was not run.** The branch is 27 commits ahead of
`origin/draft-release/v0.2.0`. Run:

```bash
git push origin draft-release/v0.2.0
```

---

### What remains for Zach

1. **Manual browser QA** per `docs/QA.md` — walk through the Leptos and Dioxus
   examples interactively. The bot cannot test browser rendering.
2. **Merge `draft-release/v0.2.0` into `main`** and tag `v0.2.0`.
3. **`cargo publish`** for `chorale-core`, `chorale-dioxus`, `chorale-leptos`,
   `chorale-derive` (in dependency order: core first, then the others).

The `chorale`, `chorale-yew`, `chorale-sycamore` placeholders stay at `0.0.0`
until those adapters are built.
