# Implementation Checklist: #11661 — expose the canonical Cargo executor command and durable receipts

## Gate

### Step 0: Wake verification (blocking)

- **Check:** #11642 accepted and consumed; #11647 (pure executor domain:
  `CargoOperationSubject/Request/Result`, result planes, composition laws)
  MERGED; #11660 (canonical programmatic transaction) MERGED with a callable API;
  #9550 command-identity authority landed or the current `Commands` enum confirmed
  by its owner as the binding convention for this row. State/capacity/process
  identity fields (#11650/#11653/#11659) either merged or explicitly typed as
  absent/not-proven in the receipt until their lanes land.
- **If unmet:** stop. The live cut stays `BLOCKED_BY_PREREQUISITE`; this packet
  remains the durable prep. Do not register the command against an invented
  vocabulary, do not implement the transaction here, do not freeze
  `cargo_operation_result.v1` type names ahead of #11647.
- **Re-verify live** (never trust this file's snapshot):

```bash
gh issue view 11642 --json state --template "{{.state}}"
gh issue view 11647 --json state --template "{{.state}}"
gh issue view 11660 --json state --template "{{.state}}"
gh pr list --state all --search "11660 OR 11647 OR cargo executor in:title,body" --limit 20
```

## Change order after wake (compiles at each step)

### Step 1: Reconcile this packet against landed prerequisites

- **File:** `.spec/11661-cargo-executor-command/*` (UPDATE)
- **Details:** replace deferred authority references with exact landed type/API
  names from #11647/#11660; confirm command spelling against #9550's registry;
  re-map CEXC-C01..C15 rows if plane names changed.
- **Verify:** every "post-wake binding authority" cell cites a real symbol.

### Step 2: Red falsifier fixtures first

- **File:** new `xtask/src/tasks/cargo_executor/` test module (name per landed
  contract) — write failing F04, F05, F09, F10, F14 from acceptance.md §Test-Grid.
- **Verify:** `cargo test -p xtask --all-targets --locked <fixture>` fails for the
  right reason.

### Step 3: Typed request parser + command registration

- **File:** `xtask/src/main.rs` (`Commands` enum / #9550 row),
  `xtask/src/tasks/cargo_executor/request.rs`
- **Details:** subcommands run/validate/explain/reproduce per #9550 spelling;
  typed flags + validated request packet (file/stdin); no raw-argv variant exists
  structurally (F01/F02); metadata-resolved package IDs only (F03); malformed
  input → instrument failure, never default (F04).
- **Depends on:** Steps 0–2.
- **Verify:** Step 2 fixtures green; `cargo xtask cargo-executor` help shows only
  typed surfaces.

### Step 4: Transaction invocation + atomic receipt publication

- **File:** `xtask/src/tasks/cargo_executor/{transaction, receipt}.rs`
- **Details:** invoke #11660's API programmatically; publish one
  `cargo_operation_result.v1` object via the publication_drift atomic shape
  (private temp under owned root → sync → content-bound rename → read-back
  validate); sibling-owned identities serialized as absent/not-proven until their
  lanes land; publication failure degrades class, never overclaims (F09/F10/F11).
- **Verify:** F09/F10/F11 green; read-back validation rejects tampered files.

### Step 5: One-object projections + exit mapping

- **File:** `xtask/src/tasks/cargo_executor/render.rs`
- **Details:** human compact summary, JSON projection, deterministic-order
  explain (offline capable), stable exit classes — all derived from the single
  validated object (F06/F07/F08); unknown variants fail closed visibly (F14).
- **Depends on:** Step 4.

### Step 6: Narrow rerun packet + privacy/retention checks

- **File:** `xtask/src/tasks/cargo_executor/rerun.rs`
- **Details:** bounded packet preserving exact subject/model/target/filter/
  profiles/toolchain; `--print` renders only; stale/cross-subject reuse rejected
  (F12); secrets/env/private-path scan redaction with visible downgrades (F13/F15);
  retention classification through existing policy owners (C14).

### Step 7: Proof-manufacture + result-overclaim reviews (issue-required)

- Lens A: find any raw-input/stale-receipt/atomic-publication route that can
  manufacture proof (F01/F02/F03/F05/F10/F11 evidence).
- Lens B: find any output/exit/redaction/rerun path that changes or overstates the
  typed result (F06–F09/F12–F15 evidence).
- **Verify:** both lenses return findings-or-clean WITH citations on the PR.

### Step 8: Final verification

```bash
cargo fmt -p xtask -- --check
cargo test -p xtask --all-targets --locked cargo_executor
cargo clippy -p xtask --all-targets --locked -- -D warnings
cargo xtask check-architecture
cargo xtask check-devex-docs
git diff --check
```

## Deterministic checking of this packet (valid NOW, before wake)

```bash
for f in context.md acceptance.md checklist.md; do [ -f ".spec/11661-cargo-executor-command/$f" ] || exit 1; done
rg -c "CEXC-C(0[1-9]|1[0-5])" .spec/11661-cargo-executor-command/acceptance.md   # expect >= 15 contract rows
rg -c "^\| [0-9]+ \| " .spec/11661-cargo-executor-command/acceptance.md        # expect exactly 17 falsifier rows
rg -n "main@f1e74db90" .spec/11661-cargo-executor-command/context.md             # pinned evidence base
rg -n "cargo-safe|publication_drift|session_receipt" .spec/11661-cargo-executor-command/context.md
git diff --check
```

Note: the falsifier-row count is 17 by construction; if the rg pattern above
returns a different count than the table's row count, treat as NOT_PROVEN and fix
the pattern before trusting it. Run the structural checks twice against the
unchanged tree; identical ordered output and a clean second run is the
determinism proof. A missing tool or row is `NOT_PROVEN`, never a green result.

## Callers and consumers

- Post-wake command consumers remain OUT of this claim: #9549/#9554/#11630/#9567
  migrate in their own lanes; #11662 owns `scripts/cargo-safe` adapter reduction;
  #9156 owns routed-gate adaptation of executor observations.
- Review lenses after wake: input-bypass, receipt-publication fault injection,
  projection-equivalence, privacy/redaction, boundary (per issue review plan).

## Scope boundary

Files IN scope: `.spec/11661-cargo-executor-command/context.md`,
`acceptance.md`, `checklist.md`.

Files OUT of scope until Step 0 passes: `xtask/src/main.rs`,
`xtask/src/tasks/cargo_executor/**`, `.ci/receipts/schemas/`,
`scripts/cargo-safe`, `justfile`, and every caller surface named above.

## Flags for builder

- #11647's plane names are latitude ("Exact names are latitude") — re-map CEXC
  rows at Step 1 instead of freezing this packet's wording.
- The receipt schema cross-check test pattern (session_receipt.rs:549-584 vs
  `.ci/receipts/schemas/session-start.schema.json`) is the required proof shape
  for `cargo_operation_result.v1`; keep schema file and struct in one PR.
- Windows is a first-class host: the atomic rename and process-identity capture
  must hold without POSIX-only primitives; absent lock/process primitives are
  typed not-proven/blocked states, never silent fallbacks (the cargo-safe
  unlocked-run mistake).
- Do not let `--print` grow execution semantics later; the render/exec boundary
  is structural (separate code paths), not a flag check.
