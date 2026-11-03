# Local Security Scanning Commands

Authority for the local security command surface: `.justfiles/security.just`,
implemented by [`scripts/security_scan.sh`](../scripts/security_scan.sh)
(Issues #282 and #15366). The GitHub Actions workflow
`.github/workflows/ci-security.yml` remains the separate CI enforcement
surface; local commands reconcile with its result vocabulary but do not claim
identical enforcement.

## Why this exists

Earlier recipes chained scanners with `&&` (one failure suppressed the rest),
treated a missing scanner binary as success (`SKIP` + exit 0), and generated
reports behind `|| true` with a fresh timestamp regardless of scanner outcome.
Those surfaces could report a passing "comprehensive" scan while tools were
unexecuted, unavailable, or failed. The current contract removes those
truth-loss paths.

## Command contract

| Command | Denominator | Behavior |
|---|---|---|
| `just security-scan` | cargo-audit, cargo-deny, trivy | Every scanner runs independently even when an earlier one fails. |
| `just security-quick` | cargo-audit only | Narrower denominator is recorded; fails closed when unavailable. |
| `just security-report` | cargo-audit, cargo-deny, trivy | Full scan, then `report.md` rendered from this run's receipt. |
| `just security-verify <receipt>` | — | Re-verifies every referenced artifact (existence, size, sha256). |

Standalone recipes (`security-audit-strict`, `security-deny`,
`security-trivy`, `security-trivy-sarif`, `security-trivy-docker`) fail closed
with exit 3 when their tool is absent; they never substitute a skip for a run.

## Exit codes

```text
0  clean              every requested scanner ran with a valid artifact, none found issues
1  findings           at least one scanner ran with a valid artifact and reported issues
2  instrument_failed  a scanner ran but produced no valid artifact (or evidence changed)
3  incomplete         a scanner was unavailable/not_run, or a receipt digest mismatched
```

Precedence: digest/evidence-integrity problems and unavailable scanners make
the run `incomplete`; otherwise any instrument failure dominates; otherwise
findings dominate. All non-clean aggregates exit non-zero.

## Result vocabulary (per scanner)

```text
clean              ran, schema-valid artifact, exit 0
findings           ran, schema-valid artifact, non-zero exit
instrument_failed  ran without producing a valid artifact
unavailable        scanner binary not found; not executed
not_run            skipped by explicit policy (none currently defined)
```

Findings and instrument failure are distinct non-clean outcomes. Neither is
rendered as success.

## Receipts and reports

- Every run writes to a fresh private directory
  `target/security-reports/run-<utc-timestamp>-<pid>/` containing
  `receipt.json` (`perl-lsp.security-receipt/v1`) and, for report mode,
  `report.md`.
- `receipt.json` records, per scanner: name, version, result, exit status,
  artifact path/bytes/sha256/schema status/digest verification, start/end
  times, and limitations. The subject is the full commit SHA plus a dirty-tree
  flag; a short SHA alone never identifies a report subject.
- Reports are rendered only from the receipt of the same run; artifact links
  are run-local. Old `audit.json`/`trivy.json` files can never satisfy a
  current run. The aggregate is complete only when every requested scanner has
  an explicit acceptable result (`clean` or `findings`) with verified digests.
- Report headlines (`CLEAN | FINDINGS | INSTRUMENT FAILED | INCOMPLETE`) and
  the process exit code always agree with the receipt.
- A report may be generated from a non-clean run; its headline and exit then
  say findings/instrument-failed/incomplete as applicable.

## Scanner notes

- `cargo audit --json` writes `cargo-audit.json` (single JSON document).
- `cargo deny --locked --format json check advisories licenses bans sources`
  writes `cargo-deny.json` (JSON lines).
- `trivy fs --format json --exit-code 1` writes `trivy.json`; `--exit-code 1`
  makes findings non-zero so they classify as findings, matching the CI
  blocking scan's `exit-code: '1'` posture for CRITICAL/HIGH.
- The engine no longer auto-installs scanners: silent installation would
  change the instrument mid-run and mask unavailability. Install tools
  explicitly (`cargo install cargo-audit --locked`,
  `cargo install cargo-deny --locked`, Trivy's installer).
- Scanner runs are preceded by the repo cargo toolchain guard
  (`scripts/lib/cargo-toolchain-guard.sh`, issue #12593); a refusal exits 78
  with remediation guidance and is fail-closed. `verify` does not require a
  toolchain.
- Artifact schema validation uses `python3`/`python` when available; if no
  JSON validator exists the artifact is not schema-verified and the scanner is
  recorded `instrument_failed` (fail-closed).

## False positives

Trivy-ignored findings are listed in `.trivyignore` with rationale. Do not add
an ignore entry without recording the justification there.

## Proof

`bash scripts/security_scan_test.sh` exercises the discriminating matrix with
fake scanner binaries: independent execution under failure, typed outcomes,
missing-tool fail-closed behavior, findings vs instrument failure, report
freshness, headline/exit agreement, and receipt digest verification.
