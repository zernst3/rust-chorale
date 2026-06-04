# Branch protection setup (`main`)

The chorale repo enforces an enforced-PR workflow on `main` via GitHub branch
protection rules. This document captures what to enable so a fresh
contributor — or a future-Zach setting up another clone — can re-apply the
same rules without guessing.

The rules below combine with the workflows in `.github/workflows/` to
implement ORCH-CONFORMANCE-1, ORCH-PREREVIEW-1, ORCH-REVIEWER-SPLIT-1,
ORCH-ENV-GATED-QUALITY-1, ORCH-NEW-PATH-TESTS-1, and CC-1.

**Reviewer note (2026-06-03):** `claude-review.yml` ships as
`workflow_dispatch`-only. The Anthropic API is metered usage on a separate
account from Zach's Claude Code subscription, and an automatic PR-trigger
is deferred until that spend is warranted. Zach manually runs a Claude
review locally on each PR via his existing subscription, which satisfies
ORCH-PREREVIEW-1's intent ("AI-reviewed before merge"). The workflow file
documents the re-enable recipe in its header for the day that flips.

## One-time setup steps

### 1. (Optional) Add the Anthropic API key as a repo secret

Only needed if you re-enable `claude-review.yml` as an automatic PR gate.
See the workflow file's header for the re-enable recipe.

1. Go to https://github.com/zernst3/rust-chorale/settings/secrets/actions
2. Click **New repository secret**.
3. Name: `ANTHROPIC_API_KEY`
4. Value: a Claude API key from https://console.anthropic.com/settings/keys.

### 2. Enable branch protection on `main`

1. Go to https://github.com/zernst3/rust-chorale/settings/branches
2. Under **Branch protection rules**, click **Add rule** (or edit existing).
3. **Branch name pattern:** `main`
4. Check the following:
   - **Require a pull request before merging**
     - Required approvals: `1` (Zach reviews his own AI-orchestrated PRs).
     - **Dismiss stale pull request approvals when new commits are pushed**
       ✓
     - **Require review from Code Owners** (only if `CODEOWNERS` is added
       later; not required for solo work).
   - **Require status checks to pass before merging** ✓
     - **Require branches to be up to date before merging** ✓
     - Required status checks (search and add each):
       - `fmt / clippy / test / doc` (from `ci.yml`)
       - `Convention citation check` (from `commit-lint.yml`)
       - (If `claude-review.yml` is later re-enabled as auto-trigger:
         also add `AI architectural review`.)
   - **Require linear history** ✓ (no merge commits; squash or rebase only).
   - **Require conversation resolution before merging** ✓
   - **Do not allow bypassing the above settings** ✓
     - Includes admins ✓ (Zach binds to the same rules; emergencies still
       go through PR).
   - **Restrict who can push to matching branches** ✓
     - Leave the list empty so nobody can push directly; everyone PRs.

### 3. (Optional) Force-push protection

Under the same branch protection rule:
- **Allow force pushes**: leave unchecked.
- **Allow deletions**: leave unchecked.

These two combine with the require-PR rule above to make `main` immutable
except via PR merge.

## Result

Once enabled:

- No human or bot can `git push origin main` directly. The push is rejected
  by GitHub.
- Every change to `main` arrives as a merged PR.
- Every PR is blocked from merge until: CI is green, commit-lint is green,
  Zach's manual Claude review is done, and Zach approves.
- `cargo publish` remains a Zach-only manual step (no workflow performs it).

## Day-to-day flow

For the overnight chorale routine (current target: v0.2.0):
1. The wrapper checks out `draft-release/v0.2.0`.
2. The bot commits to that branch and pushes it (no `--force`).
3. The first time the branch has commits, Zach opens a PR from
   `draft-release/v0.2.0` to `main` and leaves it open (draft or ready).
4. Subsequent bot pushes auto-update the PR.
5. When v0.2.0 is ready, Zach marks the PR ready-for-review (if draft),
   runs a manual Claude review locally, CI / commit-lint run green, Zach
   merges, and he tags + `cargo publish`s.

Patch releases (v0.1.1, v0.1.2, …) follow the same flow on
`draft-release/v0.1.x` branches; they are reserved for non-breaking bug
fixes against the v0.1 line and only get cut when an actual fix needs to
ship.

For Zach's interactive work:
1. Create a branch (any name).
2. Commit + push.
3. Open a PR.
4. Same gates apply.
5. Self-merge after the gates pass.
