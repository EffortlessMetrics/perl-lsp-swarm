# Acceptance Criteria: #11766 — shared editor-host reliability and adoption contract

This is a checked, declarative contract. It does not implement a host runner,
editor adapter, receipt, workflow, support claim, or product behavior.

## §Behavior

| Input / condition | Required result | Evidence boundary |
|---|---|---|
| Run is evaluated | Exact repository, host, candidate, and run identity is retained | A mutable branch, filename, or client event is not identity |
| Receipt is consumed | Subject/run/current-generation freshness is checked | Pre-existing or stale output is rejected |
| Parent deadline expires | Last completed barrier and failure artifacts remain; graceful then forced settlement is distinguished | Timeout does not erase evidence |
| Host, candidate, or descendant processes exist | Run-owned identities are retained separately in a complete ledger | Direct PID alone is insufficient |
| Cleanup is evaluated | `pass` only after independent bounded observation of every exact run-owned identity in the denominator | Status 0 and client shutdown events do not prove cleanup |
| Capability or instrumentation is missing | Result is `not_proven` | Never fabricate pass or incompatibility |
| Source is bounded/redacted | Original/retained counts, truncation, digests, class, and finalization are retained | Prefix digest is not full-source identity |
| Product/instrument/reporting/cleanup differ | Four terminal planes remain independently reportable | No boolean flattening or result rewriting |
| Consumer adopts the contract | New drivers require substrate; modified active drivers migrate or carry reviewed exception; untouched legacy is inventoried debt | Consumers retain client semantics |

## §Hazards

| Class | Invariant | Surface | Required adversarial check |
|---|---|---|---|
| Identity/currentness | Every result names the exact current subject and run | `context.md` identity/freshness law | stale pre-existing output cannot satisfy current run |
| Deadline/barrier | Parent-owned deadline preserves the last completed barrier and artifacts | `context.md` deadline law | timeout with completed barrier and failure artifact |
| Process ownership | Required denominator includes host, candidate, and descendants | `context.md` process-domain law | surviving known descendant fails cleanup |
| Cleanup observation | OS-level independent observation is required | cleanup law | status 0/client event alone is rejected |
| Capability semantics | Missing ownership/instrumentation is `not_proven` | four-plane and stop rules | unproven platform mechanism cannot pass |
| Artifact integrity | Counts, truncation, full-source/retained digests, and redaction are distinguishable | artifact law | truncated prefix hash cannot claim full source |
| Outcome separation | Product, instrument, reporting, and cleanup retain independent status | four-plane law | reporting failure cannot erase product/cleanup disposition |
| Ownership leakage | Consumers cannot redefine shared semantics | authority split | adapter-local generic policy is rejected |
| Scope leakage | #10894 does not define named client semantics | consumers/non-goals | Eglot/Coc/LSP4IJ/Lite XL/Vim/DAP semantics remain consumer-owned |
| Receipt duplication | #7777/#10527 remain generic receipt authority | links and scope boundary | copied generic receipt rules are rejected |
| Adoption drift | Legacy and modified drivers have explicit disposition | adoption contract | untouched legacy is not silently supported |
| Determinism | Same tree produces same ordered check output twice | checklist proof | second run is byte-clean |

## §Contracts

| Contract | Authority | How this bundle satisfies it |
|---|---|---|
| Checked spec directory shape | [`SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md) | Provides all three canonical files and acceptance sections |
| Shared host identity/freshness/deadline/process/cleanup/artifact contract | #10894 | Defines exact subjects, currentness, parent deadline, process domain, ledger, independent cleanup, bounded artifacts, and refusal semantics |
| Generic durable receipts | #7777 / #10527 | Explicitly preserved; this bundle does not create or copy a receipt schema |
| Parent controller and recurrence | #9800 / #10899 | Links the parent and leaves recurrence/adoption to their owners |
| Consumer semantics | Emacs/Eglot/lsp-mode, LSP4IJ, Coc, Lite XL, Vim/DAP leaves | Names representative consumers without absorbing their client/provider contracts |
| Currentness and exact-head evidence | repository review/currentness method | Requires current subject/run identity and exact-head evidence; treats stale/missing evidence as non-success |

## §API-Shape

No Rust or public API is introduced. The declarative names below are semantic
contract terms only; implementation may choose an internal representation after a
separate reviewed #10894 implementation plan.

| Item | Kind | Contract shape | Dup-risk / owner |
|---|---|---|---|
| `HostRunSubject` | semantic identity | exact host/candidate/run subject | #10894; consumers acquire their inputs |
| `FreshReceiptTarget` | semantic identity | current subject/run/generation target | #10894; #7777/#10527 remain receipt authority |
| Process ledger | evidence model | direct-host, candidate, ambient, replacement, descendant, surviving identities | #10894; no consumer copy |
| Four terminal planes | result model | product / instrument / reporting / cleanup independently retained | #10894; provider semantics remain consumer-owned |
| Cleanup denominator | ownership evidence | every declared direct-host, candidate, descendant, and run-owned replacement path is independently observed; ambient is excluded unless adopted | #10894; a representative subset is insufficient |

N/A — no public function, type, protocol field, crate, dependency, or support
surface changes in this spec-only PR.

## §Test-Grid

The rows are negative controls for candidate designs and are intentionally
discriminating rather than implementation tests.

| # | Scenario | Kind | Required verdict |
|---:|---|---|---|
| 1 | Exit status 0 or client `shutdown_completed` is used as OS cleanup proof | negative | reject; cleanup requires independent observation |
| 2 | Only direct editor PID is killed while known descendant survives | negative | reject; surviving run-owned identity fails cleanup |
| 3 | A stale receipt lacks the current run ID, start marker, nonce/subject digest, or write-after-start proof | negative | reject as stale/mismatched freshness |
| 4 | Missing host/capability is skipped or treated as pass in required lane | negative | reject; emit `not_proven` |
| 5 | Timeout loses last completed barrier or failure artifacts | negative | reject; preserve barrier/artifacts and distinguish forced settlement |
| 6 | Numeric PID sorting is compared as lexicographic strings | negative | reject; ledger comparison is typed/canonical, not accidental string order |
| 7 | One platform ownership mechanism is generalized without proof | negative | reject; unsupported capability is `not_proven` |
| 8 | Forced cleanup is reported as clean normal shutdown | negative | reject; forced settlement remains distinct |
| 9 | Product, instrument, reporting, and cleanup collapse into one boolean | negative | reject; four planes remain independent |
| 10 | Editor adapter reimplements shared freshness/process/cleanup policy | negative | reject; adapter references #10894 |
| 11 | #10894 defines Eglot, lsp-mode, Coc, LSP4IJ, Lite XL, Vim, or DAP semantics | negative | reject; named consumer owns client semantics |
| 12 | Consumer must copy generic receipt rules from #7777/#10527 | negative | reject; generic receipts remain one authority |
| 13 | Same unchanged spec tree is checked twice | determinism | identical ordered output and byte-clean second run |
| 14 | Receipt names a different host/candidate executable path, hash, or version | negative | reject as wrong-executable evidence |

## §Blast-Radius

| Consumer / surface | Impact | Required update |
|---|---|---|
| #10894 implementation | Shared substrate must implement this contract with representative proof | Separate implementation PR; no code here |
| Emacs/Eglot and lsp-mode (#8734 and successors) | May reference shared laws; retains editor/provider semantics | Consumer conformance issue when modified |
| LSP4IJ (#8644/#8658 and successors) | May reference shared laws; retains IntelliJ semantics | Consumer conformance issue when modified |
| Coc (#10685/#10704 and successors) | May reference shared laws; retains provider semantics | Consumer conformance issue when modified |
| Lite XL (#10673) | May reference shared laws; retains client semantics | Consumer conformance issue when modified |
| Vim/DAP host leaves | May reference shared laws; retains host/client semantics | Consumer conformance issue when modified |
| #7777/#10527 receipts | Unchanged generic authority | No copy or schema change |
| Host/editor/CI/support surfaces | No impact in this PR | Must-not-touch boundary |

Must-not-touch: `crates/`, editor/client adapters, host harnesses, receipt
implementations, `.github/workflows/`, CI routes, support/public claims,
generated status, policy ledgers, and external processes.

## Scope, rollback, and proof claims

- **In scope:** only the three files in `.spec/10894-editor-host-reliability/`.
- **Rollback:** remove this projection or revert its commit; preserve existing
  #10894 issue authority and generic receipt history, and do not restore copied
  consumer policy.
- **Transfer:** transfer only with exact current subject, evidence inventory, and
  named receiving owner; otherwise remain `not_proven`.
- **Stop:** stop on missing identity, currentness, ownership, independent cleanup,
  artifact integrity, plane separation, or consumer authority; do not weaken a law
  to make a proof green.
- **Claim boundary:** this PR proves a durable checked contract and deterministic
  structural inspection only. It does not prove host execution, OS cleanup,
  editor behavior, CI routing, support, or release readiness.
