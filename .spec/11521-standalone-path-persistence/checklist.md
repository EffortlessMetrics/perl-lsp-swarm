# Checklist: #11521 — standalone PATH persistence and fresh-process resolution

## Gate

Step 0, blocking and permanent: this lane mutates no shell profile, no registry,
no environment, no install root, and no current process, and launches no hosted
fresh-process proof. `this_validator_performs_no_platform_mutation` enforces it
against the validator's own source; any successor that needs to write platform
state belongs to the #7832 adapters, not here.

Evidence base: `origin/main@75016cda820a`.

## Landed in this bundle

1. **Three published contracts.** `standalone_path_plan.v1` (policy and mutation
   intent), `standalone_path_persistence.v1` (what durable state changed), and
   `standalone_fresh_process.v1` (what a genuinely new process resolved), each
   with a `schema_version` constant, closed enums, `additionalProperties: false`,
   and the coherence laws in `acceptance.md §Contracts`.
2. **Checked validator.** `xtask/examples/standalone_path_persistence.rs`, with
   standalone validation per document, cross-binding of each result to its plan
   by id and by digest of the plan's exact bytes, and cross-binding of an
   observation to the persistence receipt it followed.
3. **Deterministic fixtures.** 26 documents under
   `fixtures/experience/install_path_persistence/`: five plans spanning POSIX,
   Windows, WSL, package-manager, and manual-instruction policies; seven
   persistence receipts; six fresh-process observations; and nine committed
   invalid documents, each violating exactly one named law.
4. **Schema-only parity harness.**
   `scripts/ci/test_standalone_path_contract_schemas.py` probes every published
   law without the Rust validator, because the POSIX and Windows adapters that
   will implement these mechanics are not Rust.
5. **Hosted gating.** `Standalone contract tests` gains a validator-battery step
   and a schema-harness step, triggered by the new paths.
6. **Policy ledger.** Explicit `policy/non-rust-allowlist.toml` rows for the
   three schemas, the harness, and the fixture set, each naming the commands
   that cover it.

## Deterministic checking of this bundle

```bash
for f in context.md acceptance.md checklist.md; do
  [ -f ".spec/11521-standalone-path-persistence/$f" ] || exit 1
done
rg -c "SPP-C(0[1-9]|1[0-9]|2[0-6])" .spec/11521-standalone-path-persistence/acceptance.md   # expect >= 26 contract rows
rg -c "^\| [0-9]+ \| " .spec/11521-standalone-path-persistence/acceptance.md                # expect exactly 10 falsifier rows
rg -n "origin/main@75016cda820a" .spec/11521-standalone-path-persistence/checklist.md        # pinned evidence base

cargo fmt -p xtask -- --check
cargo clippy -p xtask --all-targets --locked -- -D warnings
cargo test -p xtask --example standalone_path_persistence --locked                          # 46 focused tests
python3 -m venv .venv-schema-harness && .venv-schema-harness/bin/pip install --quiet jsonschema referencing
.venv-schema-harness/bin/python -m unittest scripts.ci.test_standalone_path_contract_schemas  # 39 schema-law checks
cargo xtask check-file-policy
cargo xtask changelog check
```

Validating a document set by hand:

```bash
cargo run -p xtask --example standalone_path_persistence -- \
  --plan fixtures/experience/install_path_persistence/plan_posix_installer_user_scope.json \
  --persistence fixtures/experience/install_path_persistence/persistence_installer_persisted.json \
  --fresh-process fixtures/experience/install_path_persistence/fresh_visible_after_documented_new_session.json
```

## Successor change order

- **S1 — POSIX adapter (#7832).** Implement profile and `path.d` mutation
  against `standalone_path_plan.v1`, emitting `standalone_path_persistence.v1`.
  Semantics are fixed here; the adapter chooses mechanics only.
- **S2 — Windows adapter (#7832).** The same for User-PATH registry mutation,
  keeping native Windows and WSL distinct.
- **S3 — Hosted fresh-process proof (#10746).** Emit
  `standalone_fresh_process.v1` from a genuinely new session. The harness may not
  prepend the install directory, invoke the absolute path, inherit the
  installer's mutated environment, or remove a competing ambient binary; the
  contract refuses every one of those spellings.
- **S4 — Public route classification (#10336).** Consume these results. The
  route word is not published here.

## Callers and consumers

None on `origin/main` yet: this packet is additive and is the input contract the
successors above consume. `standalone_install_transition.v1` and #11493's
diagnostics registry are unchanged; their `path_persistence` outcome dimension
remains a reported transition word, and this packet defines what may legitimately
produce it.

## Scope boundary

No POSIX profile or Windows registry mutation. No universal shell or profile
manager. No installed/public route classification. No release or publication
action. No change to landed vocabularies.
