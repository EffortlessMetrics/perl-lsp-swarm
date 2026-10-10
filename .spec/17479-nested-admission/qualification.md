# Two real resolver consumers: native qualification contract

This manual, unactivated experiment reuses the existing nested-plan owner. It
does not complete the canonical nine-package gate. The selected denominator is
exactly two named tests, serially executed in two Cargo processes. The adapter
requires a current named one-test pass and a successful terminal summary from
each; zero, ignored, incomplete, duplicate or wrong-name output refuses.

## Exact workload

The outer command bootstraps xtask, then the preparation adapter executes:

1. `build -p perllsp --locked --message-format=json`
2. `test -p perl-lsp-rs --locked --test cli_smoke health_prints_ok -- --exact --test-threads=1 --color never`
3. `test -p perl-lsp-rs --locked --test binary_version_test lsp_server_version_matches_crate_version -- --exact --test-threads=1 --color never`

The first test exercises support::product_binary_path and `perllsp --health`.
The second exercises common::binary_resolution and a real stdio initialize,
crate-version assertion, shutdown and exit. Existing initialization timeout and
single timeout retry remain unchanged. Eligibility version probes do not count
as behavioral tests. Both resolver families validate the current artifact and
live owner without invoking Cargo. There are four Cargo invocations including
the outer bootstrap, not four necessarily cold compilations.

## Linux operator command

Use a clean committed worktree and the installed native pinned toolchain. Keep
the ordinary debug profile. Do not set raw CARGO/RUSTC/RUSTDOC selectors or
compiler wrappers. Paths below must be native absolute paths.

```bash
export DEVPLANE=/workspace/.devplane-17479
export CARGO_HOME=/workspace/.cloud-tools/cargo
export RUSTUP_HOME=/workspace/.cloud-tools/rustup
export RUSTUP_AUTO_INSTALL=0
export CARGO_BUILD_JOBS=1
# On the observed native cloud host Cargo fills this system trust directory.
# Declare it before admission so later insertion cannot change frozen inputs.
# Verify the native host's actual trust directory before using this path elsewhere.
export SSL_CERT_DIR=/usr/lib/ssl/certs
export TMPDIR="$PWD/target/receipts"
export TEMP="$TMPDIR" TMP="$TMPDIR"
unset CARGO_STORAGE_POLICY MIN_FREE_GB MAX_USED_PCT
unset CARGO_EXPECTED_GROWTH_GB CARGO_STORAGE_BUDGET_EVIDENCE

python3 scripts/cargo_admitted.py \
  --nested-plan "$PWD/.spec/17479-nested-admission/qualification-plan.json" \
  --preflight \
  run -p xtask --bin xtask --locked -- gates \
  --tier merge-gate --gate perllsp_handoff_narrow \
  --gate-policy .spec/17479-nested-admission/qualification-gates.yaml \
  --receipt --receipt-path "$TMPDIR/perllsp-handoff-narrow.json"
```

The gate runner writes logs to worktree `target/receipts/logs` regardless of
CARGO_TARGET_DIR. Setting the admitted temp destination to worktree
`target/receipts` accounts for that volume, the receipt and handoff scratch.
The target and intermediate roots remain separate existing owner-controlled
roots. Explicit named gate selection avoids dynamic PR-fast planning.

For the approved scoped-budget route, copy the complete `scope` object emitted
by this exact preflight into a regular JSON file with exactly these fields:

```json
{
  "schema_version": 1,
  "scope": "REPLACE WITH THE COMPLETE PREFLIGHT OBJECT, NOT A STRING",
  "reserve_bytes": 8589934592,
  "expected_growth_bytes": 21474836480,
  "basis": "Operator sizing proposal for the exact cold debug xtask bootstrap, perllsp build and two LSP harnesses, including cache/intermediate/temp/log growth; not measured or execution-qualified."
}
```

These 8 GiB reserve and 20 GiB TOTAL-growth numbers are a proposal for arithmetic
evaluation, not a measured or approved compiler envelope. Repeat the same
command with `--budget-file /absolute/path/to/budget.json` immediately after
`--preflight`. After source/environment/argv/path changes regenerate the scope.
Only after operator qualification of growth, RAM, tool behavior and lifecycle
may that exact command omit `--preflight` to execute. A preflight PASS is a
read-only free-space snapshot, not a reservation; `basis_verified` stays false.

The budget-file API accepts any positive integer reserve and growth, with exact
request scope. The default percent policy retains its 40 GiB reserve and 85%
limit; the legacy environment byte-budget policy retains a 40 GiB minimum.
No global storage policy or profile changes are made here.

## Qualification limits

The existing nested plan rejects non-Linux hosts before execution. Its /proc
ancestry and Linux kernel child settlement are required by this handoff.
Windows launcher/Job Object and marker-repair evidence does not implement this
nested protocol. This experiment cannot currently qualify on native Windows.

Cloud cgroup memory.max is 16 GiB; cheap-control memory observations do not
establish compiler peak RAM. Current free space can pass the proposed scoped
budget arithmetic, but conservative cold growth and RAM remain unqualified.
The broader canonical population remains 995 integration declarations plus
43 eligible lib/bin declarations, not executed counts. Parser/xtask compiler
fixtures, complete dynamic shell closure, planner scope and aggregate capacity
remain outside this bounded experiment. No hosted activation is requested.
