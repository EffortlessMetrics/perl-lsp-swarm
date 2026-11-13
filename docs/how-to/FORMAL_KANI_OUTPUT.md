# Inspecting Kani 0.68 output

Owner: #19317. Domain consumer: #19300 / #19342. This implements the pure
structured-output inspection slice, not a new verifier runner or proof authority.

## Available command

```bash
cargo run -p xtask --bin formal-kani-output --locked -- expected.json kani-export.json
cargo test -p xtask --bin formal-kani-output --locked -- --test-threads=1 --color never
```

Run compiled work through the repository's admitted execution route. The CLI reads
two bounded regular files and writes an inspection to stdout. It does not install
or execute Kani, discover a trustworthy tool from PATH, modify source, publish
receipts, or accept a proof. Its unit tests need no Kani installation.

Exit 0 means the supplied positive export is internally consistent with the
supplied expectations. Exit 1 means a reported counterexample or an incomplete
observation; exit 2 means malformed, contradictory, unsupported, or unreadable
input. `--help` also returns 0. **None of these exits authenticates execution.**
Every successful JSON inspection includes:

```json
{
  "schema_version": "kani_export_inspection.v1",
  "evidence_class": "untrusted_artifact_inspection",
  "proof_accepted": false
}
```

The raw export and canonical typed expectation have separate SHA-256 identities.
Retain the original files: the normalized output deliberately does not replace raw
native evidence. Do not feed `positive_report` directly into a protected gate or
convert it to a `formal_proof.v1` PASS without the other admission conditions.

## Expectations belong to the run owner

Construct expectations from the reviewed claim/profile and verified tool manifest,
not by copying the report's claimed identities. A minimal shape is:

```json
{
  "target": "x86_64-unknown-linux-gnu",
  "tools": {
    "kani": "0.68.0",
    "rustc": "<exact independently verified kani-compiler --version output>",
    "cbmc": "<exact independently verified cbmc --version output>",
    "goto_cc": "<exact independently verified goto-cc --version output>",
    "goto_instrument": "<exact independently verified goto-instrument --version output>"
  },
  "cbmc_version": "<exact expected CBMC metadata version>",
  "solver": "cadical",
  "object_bits": 16,
  "property_categories": ["assertion", "cover", "overflow", "unwind"],
  "harnesses": {
    "<exact native harness ID>": {
      "goal": {"mode": "positive"},
      "source": "<exact emitted source filename>",
      "assertions": ["<required custom assertion description>"],
      "covers": ["<required custom cover description>"]
    }
  }
}
```

This is a **template**, not a qualified position profile. Replace all placeholders
and review the complete property-category set using genuine pinned-tool captures.
Version display strings do not replace executable/compiler/sysroot/solver digests.
The expected source filename does not replace the semantic source closure.

Required assertions and covers use exact descriptions and must each occur once.
Harness arrays use exact native IDs; source filenames are not matched by basename.
The required sets must be nonempty for a positive harness. Broader safety properties
remain checked, but ordinary unreachable checks are not confused with unreachable
required assertions or covers.

A deliberately failing control instead declares:

```json
"goal": {"mode": "counterexample", "assertion": "FV_POSITION_FALSE_BYTE_SCALAR_BOUNDARY"}
```

The named assertion must also be in `assertions`. A negative harness may have an
empty `covers` list. A listed cover is still required. The specified assertion must
actually fail: an unrelated failure cannot satisfy the control. The inspection may
record `expected_failure_observed: true`, but a settled native run, raw witness,
replay and independent wrong-production control are still required for admission.

## Checked interpretation

The parser is bound to Kani 0.68's export schema `metadata.version = "1.0"`.
It joins metadata, result, property-detail, error-detail and CBMC arrays by harness
ID; their native ordering is not required to match. The expected/observed harness
set must be exact and nonzero. Duplicate JSON keys are rejected before a JSON map
can overwrite a failure with a success.

The run must report `completed`, not a partial fail-fast result. Selected, executed,
successful, failed and duration totals must agree with the per-harness observations.
Every property ID must be unique and in the emitted 1-based range. All nine status
counts are recomputed, including `Satisfied`, `Unsatisfiable`, `Unreachable`,
`Undetermined`/`Unknown` and solver errors. Null/unmeasured counts are not zero.

All required covers must be `Satisfied`; overall `Success` does not override a
missing or unsatisfied cover. A required unreachable assertion is not a useful
positive result. `Unknown`, `Undetermined` and solver errors produce no positive
classification even when the native aggregate appears successful. A failed unwind,
recursion, missing-definition or unsupported-construct check is `not_proven`, not
an asserted production counterexample. A named assertion failure can be recorded
as `reported_counterexample`, never as an admitted proof receipt.

The initial profile rejects expected-panic, automatically generated, constructor,
loop-contract, function-contract, coverage and external-solver modes. New semantic
fields, status meanings and property categories must be reviewed explicitly.
Unknown descriptive metadata remains bound by the raw-input hash. Input is capped
at 16 MiB, 256 harnesses and 100,000 properties per harness; ordinary JSON nesting
limits remain enabled. A limit hit is refusal, not truncated successful evidence.

## Pinned source contract and upgrade review

Source inspected at Kani commit
`0d2328a93f0e0ff66132d6bfa1a7d884877cf862` (tag `kani-0.68.0`):

- `kani-driver/src/frontend/schema_utils.rs`, blob
  `00d9063213a7ad36ac12731284bfce5e926454cc`: emitted fields and count partition.
- `kani-driver/src/main.rs`, blob
  `e6ed618e1baf7597fbd0476b080166201b26e721`: export construction.
- `kani-driver/src/harness_runner.rs`, blob
  `0f0137025453abd72f4c3b9cf80db674c5918c36`: complete/fail-fast status.
- `kani-driver/src/cbmc_output_parser.rs`, blob
  `686ced189311caa4125be0104c18fc85cbd7ba49`: property classes and statuses.

The release's version string alone does not protect against changed builds using
the same version. #19315 owns immutable tool-byte verification. Do not silently
parse current upstream `main` using this old contract; its export format may differ.

Tests in `xtask/src/formal_kani/tests.rs` are explicitly **synthetic emitter-shaped
fixtures**, not native captures. They check the parser and its refusal behavior.
They cannot establish Kani compiler compatibility, complete tool output, process
settlement, mutation sensitivity, or a formal property of Perl LSP.

The tests execute as command 40 in the existing required merge-surface producer,
after #19305's inventory controls. The command requires all 34 named tests and no
ignored/filtered cases. Existing commands are unchanged; no new workflow, solver
installation, normal-PR matrix, or protected-context list is introduced.

## Remaining integration, without a second authority

#19313 supplies the typed formal receipt; #19314 supplies complete semantic closure,
freshness and correspondence. #19315 must launch the verified tool with owned fresh
outputs, resource limits, exit status and descendant settlement. #19317 joins this
parser to those native observations and retains genuine captures, including all
positive and deliberately failing outcomes. #19319–#19322 own conservative routing,
generated status and protected consumption. No independent shell runner is added.

For #19342, admission needs the exact three positive position harnesses, all 18
required covers, all required assertions and safety/unwind checks, the separate
named false-theorem failure and independently executed wrong-production controls.
Its actual domain is 585 texts, at most three scalars/12 UTF-8 bytes, not the earlier
eight-scalar planning proposal. Ordinary Rust test passes remain separate evidence.

When tools, profiles, bounds, assumptions, source or parser semantics change,
invalidate the applicable results. Qualify the real positive run and each refusal
class again before promoting an adapter or admitting a current proof. Preserve
counterexamples and raw evidence; do not convert missing, stale or expired evidence
into success.
