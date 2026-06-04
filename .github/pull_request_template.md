<!-- This template is named at `.github/pull_request_template.md` so GitHub
     picks it up for every PR by default. -->

## What this PR changes

<!-- One or two sentences describing the change in user-visible terms.
     Not "refactored types.rs" but "added MultiSelect filter to ColumnDef so
     status columns can be filtered against a set of values." -->

## Convention citations (CC-1 / PROC-CITE-CONVENTION-ID-1)

<!-- List the rule IDs from `docs/CONVENTIONS.md` and `CONVENTIONS.md`
     (root) that this PR applies. If the PR touches `chorale-core/src/` or
     `chorale-dioxus/src/` and you can't cite any rule, this is a sign the
     PR should either cite a rule, add a new rule, or scope down. -->

- [ ] Applied: ___________
- [ ] Applied: ___________
- [ ] No conventions apply (docs-only / examples-only / infra-only — explain below)

## Public API impact (API-1)

<!-- Any change to `pub` re-exports in `chorale-core/src/lib.rs` or
     `chorale-dioxus/src/lib.rs`, or to a type re-exported from there,
     routes to Zach. State the impact explicitly. -->

- [ ] No `pub` re-exports changed.
- [ ] `pub` re-exports added (NEW item, backwards-compatible).
- [ ] `pub` re-exports renamed or removed (BREAKING — requires major bump and Zach sign-off).

## Tests (TESTS-1 / ORCH-NEW-PATH-TESTS-1)

- [ ] All new pure state transitions have unit tests.
- [ ] All new code paths are exercised by at least one test.
- [ ] `cargo test --workspace` runs green locally.
- [ ] No tests added (no new code paths added either — explain below).

## Verification

- [ ] `cargo fmt --all -- --check` clean locally.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean locally.
- [ ] `cargo doc --workspace --no-deps` builds with no broken intra-doc links.

## Notes for the reviewer

<!-- Anything the AI reviewer or Zach needs to know that isn't obvious
     from the diff: a non-obvious design choice, a tradeoff considered, a
     follow-up tracked elsewhere. -->
