# Worktree forensic evidence v2

## Scope and authority

This is the JSON contract for `cargo xtask worktree-recovery plan --json`,
introduced by [PR #13515](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/13515).
It supersedes only the v1 wire representation. The read-only observer, bounded
process and filesystem probes, sampled-interval limits, classification rules,
and no-mutation contracts in [v1](WORKTREE_FORENSIC_EVIDENCE_V1.md) remain unchanged.
No recovery execution, persisted-plan loader, or replay authority is added.

## Observation invariant

`schema_version` is `worktree_forensic_evidence.v2`. Every observation in the
plan (`repository`, `candidate`) and evidence (`repository_identity`,
`candidate_identity`, `pointer`, `administrative_gitdir`,
`administrative_commondir`) admits exactly:

- `OBSERVED` with a non-null `value` of the field's existing type.
- `NOT_PROVEN` without a value. A missing or null value decodes as absent.

`NOT_APPLICABLE` is not admitted. An uninspected fact is not proof that the fact
is inapplicable. `OBSERVED` without a value and any non-observed state retaining
a value are invalid. Optional `detail` does not establish proof or override
state; absent optional fields are omitted when serialized.

Examples:

```json
{"state":"OBSERVED","value":{"requested_path":"candidate","canonical_path":"candidate","path_key":"candidate"}}
{"state":"NOT_PROVEN","detail":"candidate identity not observed"}
```

Forensic field deserialization rejects invalid combinations with field context.
Because the shared Rust observation fields remain publicly constructible,
classification also checks all evidence observations: invalid combinations return
`NOT_PROVEN` before they can establish clean evidence. Plan construction and the
public human/JSON `render` route reject invalid observations; `exit_code` returns
non-success for them. Valid-source classification precedence is unchanged.
These are observation-invariant checks, not general validation of arbitrary
caller-supplied plan classifications, reasons, or digest strings. Raw shared-type
serialization is not a forensic output-admission API.

## Decoder and digest migration

V1 used `detail: "observed"` on observed values and `value: null` on unavailable
observations; v2 uses typed states. Consumers must choose a decoder using
`schema_version`. Preserve saved v1 evidence under its original schema or rerun
inspection to generate v2; do not relabel old JSON or infer a filesystem change
from a changed digest. No backward decoder is added to the shared cleanup type.

`plan_digest` hashes the serialized plan with `observed_at` and `plan_digest`
cleared. All other serialized fields, including schema version and observation
state/value/detail, remain digest inputs. Timestamp changes do not change identity;
material evidence changes do. Cross-version digests are different identities.
Human classification and unavailable-detail wording remain unchanged, while the
human report's displayed digest changes with the schema.

## Shared representation and proof

Cleanup and forensic evidence share `Observation`; they do not share permission
to change each other's wire contract. Any shared serialization change must review
its forensic output and digest impact and advance the forensic schema when the
wire contract changes. Exact observed/unavailable JSON fixtures detect drift;
the invariant matrix covers all forensic observation fields, direct construction,
field decoding, human/JSON output, and non-success exit status. Digest tests cover
timestamp/self exclusion and evidence/schema sensitivity.

Focused proof:

```sh
cargo test -p xtask --lib --locked worktree_forensic_recovery
cargo clippy -p xtask --lib --locked -- -D warnings
cargo fmt -p xtask -- --check
```

Fixture CLI proof remains `cargo test -p xtask --test worktree_forensic_recovery
--locked`. Platform execution remains bounded by the actual host exercised.
