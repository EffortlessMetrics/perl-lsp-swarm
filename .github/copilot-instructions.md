# GitHub Copilot instructions

This file is a concise route map for Copilot. It does not duplicate the repository
architecture, issue state, crate inventory, or review workflow.

## Current authority

Use the highest applicable current source:

1. current code, tests, generated contracts, and live GitHub state;
2. accepted current specifications, ADRs, and policies;
3. [`AGENTS.md`](../AGENTS.md) and [`CLAUDE.md`](../CLAUDE.md);
4. current shared method documents under [`docs/agents/`](../docs/agents/);
5. this file.

Historical articles, forensics, completed implementation specs, old label taxonomies,
and archived orchestration documents are evidence, not current operating instructions.
Do not reconstruct a fixed agent conveyor or lifecycle-label state machine from them.

## Product direction

Perl LSP is becoming a compiler-backed Perl toolchain whose parser, semantic facts,
workspace model, LSP, DAP, packaging, and editor behavior remain honest about source,
freshness, confidence, fallback, and dynamic boundaries.

Optimize for user-visible closure, semantic ownership, deterministic proof, and
maintainable current-`main` behavior. Do not optimize for local component completion or
workflow compliance at the expense of the product claim.

## Shape one coherent change

A pull request should normally contain:

```text
one coherent claim
one semantic owner
one current candidate
one branch or worktree
one mutation owner
one rollback boundary
```

Before editing, read the controlling issue, nearest package-local guidance, current
implementation, tests, and proof owner. Do not bundle adjacent work merely because the
files are nearby.

Different claims may proceed in parallel, including same-file work. Coordinate only an
equivalent candidate, explicit stack or prerequisite, shared branch writer, destructive
shared runtime state, real Git conflict, or demonstrated combined-tree interaction.

## Current key surfaces

Use repository metadata rather than a hand-maintained full crate count. Common entrypoints:

| Surface | Path |
| --- | --- |
| Native parser | `crates/perl-parser/` |
| Semantic analysis and facts | `crates/perl-semantic-analyzer/`, `crates/perl-semantic-facts/` |
| LSP core | `crates/perl-lsp-rs-core/` |
| LSP integration | `crates/perl-lsp-rs/` |
| LSP executable | `crates/perllsp/` |
| DAP | `crates/perl-dap/` |
| Corpus and compiler harness | `crates/perl-corpus/`, `crates/perl-core-harness/` |
| Repository tooling | `xtask/`, `scripts/`, `.ci/` |
| VS Code extension | `vscode-extension/` |

Do not invent or rely on crates and paths that are absent from current `Cargo.toml` and
the current tree.

## Development route

Choose the narrowest applicable public flow, then enter it at the earliest missing
useful judgment:

- durable multi-PR outcome or umbrella → `deliver-goal`;
- one issue, PR, branch, candidate, or coherent claim → `deliver-pr`;
- problem, owner, scope, or plan unsettled → `prepare-issue`;
- proof absent or weak → `prepare-proof`;
- implementation incomplete → `build-candidate`;
- published candidate needs review, repair, CI, integration, or closeout → `finish-pr`.

A claim-level flow owns publication, review, integration, and reconciliation. Running an
atomic stage inside it does not complete the claim.

The concrete provider procedures live in `.agents/skills/` and `.claude/skills/`.
Do not invent a parallel lifecycle or fixed model/agent roster.

## Proof

Run the cheapest proof that can falsify the claim, then expand only when the changed
surface or risk requires it.

```bash
cargo fmt -p <package> -- --check
cargo clippy -p <package> --all-targets --locked -- -D warnings
cargo test -p <package> --all-targets --locked
just pr-fast
```

Useful repository checks:

```bash
just devex
just doctor
just ci-gate
just status-check
just ci-docs-check
just release-check
```

Do not claim that a passing local command proves hosted CI, another platform, a
packaged artifact, or a different command. Missing, partial, cancelled, timed-out,
rate-limited, or instrument-failed evidence is `NOT_PROVEN`.

Never weaken a test, ratchet, support claim, or required proof merely to obtain green
status.

## Review and currentness

Review is semantic and cumulative. The current head SHA identifies code and machine
statuses; it is not the review verdict.

- material claim, implementation, production route, authority, risk, rollback, or
  tested-seam change → refresh affected proof and review;
- focused finding repair → verify the finding and changed seam;
- actual conflict or combined-tree repair → review the affected interaction;
- formatting, editorial cleanup, or unrelated generated refresh → no full review
  restart merely because the SHA changed.

A clean review is valid. Green CI, mergeability, zero threads, bot approval, or labels
cannot create substantive review.

## Integration and branch movement

Behind-only movement requires no action.

```text
candidate remains conflict-free
+ unrelated main work lands
→ leave the candidate unchanged
```

Rebase is ordinary integration work. Its main accepted use is resolving a real conflict;
it is also available for another named integration or active-work reason. Merge-main,
retarget, cherry-pick, or reconstruction may be better for a particular stack or
interaction.

There is no one-rebase quota. Do not repeatedly rebase, push empty commits, or update a
branch solely to chase `main`, manufacture exact-head review, or retrigger CI.

Required GitHub statuses remain attached to the commit they evaluated. If a required
run is pending on the current head, let it report or request a same-head rerun where
supported. At merge, use the live head as compare-and-swap protection.

## Coding standards

Production code must not introduce:

- `unwrap` or `expect` outside a documented narrow exception;
- `panic!`, `todo!`, `unimplemented!`, or `abort`;
- `dbg!`;
- silent error swallowing or fabricated success;
- broad allowlists or fallback behavior that weaken a claim.

Prefer `Result`, `Option`, explicit typed states, deterministic ordering, and actionable
errors. Use tests that fail for a realistic wrong implementation, not only tests that
execute the new line.

## Git and worktrees

- use one worktree per genuine concurrent write claim, not per lifecycle pass;
- never edit the coordination checkout directly;
- never use `git stash` in worktrees;
- stage intended paths explicitly;
- use `--force-with-lease=<branch>:<expected-sha>` for an authorized rewrite;
- preserve dirty, unpushed, or salvageable work;
- clean only lane-owned branches, worktrees, and scratch after reconciliation.

See [WORKTREE_PROTOCOL.md](../docs/reference/WORKTREE_PROTOCOL.md).

## Pull requests and durable updates

Use a conventional PR title with the controlling issue number:

```text
fix(parser): correct bounded behavior (#1234)
docs(control-plane): retire stale authority (#1234)
```

The body should state claim, root cause, changed seams, proof, risk, rollback,
non-goals, and `NOT_PROVEN` boundaries.

Publish GitHub updates only when claim, authority, proof obligation, finding,
prerequisite, risk, route, accepted state, merge, or closeout changed. Do not publish
worker liveness, repeated pending observations, exact-head review receipts, or runtime
frontier state.

Labels are navigation. Current issues, PRs, reviews, threads, checks, rulesets, and code
own the decision.
