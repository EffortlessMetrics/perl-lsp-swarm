# Installer adoption test selection

Source inspection basis: `origin/main` at
`4cc7960e61b685ce3b7e4cb9fe43b929bf900972`, fetched 2026-10-15 UTC.
This is analysis and proposed contract fixtures; the local hook worker owns
implementation. No existing candidate or owner branch was modified.

## Source-backed explanation

`hooks/pre-push` takes a docs-only fast path or a single-crate path; otherwise
it calls `just pr-fast`. That recipe invokes `xtask gates --tier pr-fast`.
The hook's per-ref changed-path classification does not supply that path set
to the planner. The planner independently resolves its base through
`CI_SCOPE_BASE`, `GITHUB_BASE_REF`, or the shared automatic change-set resolver.
An actual incident therefore needs both the pushed ref range and the effective
planner base/path set before its exact cause can be established.

In `xtask/src/tasks/ci_scope.rs`, `classify_diff` labels a script alone
`ci_config`, but a script plus README `mixed`. Scripts outside crates normally
have no direct package owner, reverse dependency, or architecture widener.
In `xtask/src/tasks/gates.rs`, `is_unowned_non_rust_diff` recognizes only
`prose_only`, `docs_as_code`, and `ci_config`. Consequently, a mixed script/prose
diff with an empty package set activates `fallback_used`. The
`RustPackageScoped` arm selects package gates unconditionally on fallback,
including `.ci/gate-policy.yaml`'s required `inline_completion_registration`:

```text
cargo test -p perl-lsp-rs --locked --test lsp_inline_completion_registration_tests
```

This route selects the product build conservatively; it does not establish a
Cargo dependency from the installer to inline completion. A script-only,
unowned `ci_config` diff should already skip this gate on the inspected tree.
If that exact input selected it, investigate base mismatch, additional paths,
scope/metadata failure, or a different candidate revision instead of attributing
it to the mixed-class case.

The reported compilation exceeding 35 minutes was not reproduced. Policy sets
this gate's timeout to 240 seconds with one retry; overall hook time, Cargo
startup, prior gates, and repeated attempts need separate timing evidence.

## Minimal correction for the implementation owner

Determine Rust relevance from the constituent paths, rather than treating every
`mixed` diff as Rust-relevant. Skip unowned Rust lanes only if every path is in
an established non-Rust class AND the selected package set is empty. Reuse the
existing path classification; do not add a blanket `scripts/` or `mixed`
exemption. Preserve fallback for unknown code and scope/metadata failures.

Current main's `78aadd5615` explicitly retained Rust owner proof for non-Rust
inputs. Preserve it: policy files owned by xtask still need
`unit_routed_full`; an affected product package still needs required product
registration tests. Keep the explicit required merge-gate
`doctest_contract_proof` route with `--doc` and its package floor. A global
`--lib` replacement would silently lose integration and default doctest proof.
No policy, gate requirement, timeout, or protection change is proposed.

## Fixtures and evidence limits

`xtask/src/tasks/gates/installer_selection_contract.rs` is wired into the actual
planner's unit tests and loads the real gate policy. It supplies synthetic
package topology to the production classifier, then calls production planning:

- installer alone: no unrelated product compilation;
- installer plus README: no fallback (expected regression on the inspected source);
- installer plus owned policy: owner integration tests retained;
- installer plus LSP source: required registration retained;
- installer plus unknown code: conservative fallback retained;
- explicit doctest route: required merge-gate `cargo test --locked --doc` and
  the existing five-package floor retained.

These are proposed fixtures, **NOT_PROVEN** until executed. No observed red or
green Rust result is claimed. The unknown-code control prevents a blanket
mixed-class exemption from satisfying the desired contract. The owned-policy
control traverses the same skip predicate and prevents reversal of #17763.
The doctest assertion checks routing and the current package floor, not doctest
execution or discovery of new contracts; existing doctest enforcement remains
necessary. Independent read-only source review confirmed the mixed-class route
and requested stronger owner-command and doctest-package controls; those
controls were added. The review also remains bounded by unavailable execution.

Focused attempt (after selecting the installed toolchain using PATH,
CARGO_HOME, RUSTUP_HOME and DEVPLANE under `/workspace/shared`):

```text
bash scripts/cargo-admitted test -p xtask --locked --bin xtask tasks::gates::tests::pr_fast_ci_config_keeps_always_on_and_skips_rust_lanes
cargo-admitted: DENY: insufficient disk headroom
free_bytes=31787794432 required_free_bytes=42949672960.0
```

No compilation began. The admission floor was preserved; no storage allocation,
cleanup, raw-Cargo retry, or large build was attempted. An initial invocation
without the installed toolchain on PATH was refused with exit 78; the configured
invocation above returned 75. Both are environment/admission blockers, not test
failures. On an admitted host, run the focused module with:

```text
bash scripts/cargo-admitted test -p xtask --locked --bin xtask tasks::gates::installer_selection_contract::
```

Executed locally: `bash scripts/tests/test-install-githooks-wrapper.sh` passed
3/3 assertions with stub Cargo; this proves wrapper delegation and error
propagation only. Rustfmt parsed/formatted the new module, and
`git diff --check` passed. Neither substitutes for executing planner fixtures.

## Access and ownership boundary

Git fetch succeeded. Both `gh pr list` and `gh issue list` returned
`Post "https://api.github.com/graphql": Forbidden`; live candidate, PR/issue
ownership, reviews, holds, and checks could not be refreshed. No alternate API
route was used to evade that denial. Branch-name discovery alone cannot prove
owner state. The source explanation above is a reproducible candidate mechanism,
not a verified attribution of the original owner's run. Resume with authorized
live-state access, the incident's exact candidate/base/logs, and an admitted
executor. Preserve the original owner branch and all explicit holds.
