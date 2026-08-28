# Acceptance Criteria: #11548 successor, shape (a) — non-authoritative #11575 inventory derivative

Authority: the scoping receipt on
[#11549 (comment 5458545004)](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11549#issuecomment-5458545004).
This shape is explicitly **NON-authoritative**: it is an #11575
(`docs/distribution/INSTALL_CLAIM_SURFACES.md`) claim-surface inventory
derivative. It is **not** the #10333/#10334 true `public_release_claims.v2`
route contract and must not pretend to be (see §Non-goals). Prior candidate
#12858 (head `4501c89`) was closed unmerged with six dispositioned defects;
this PR rebuilds the file set fresh from `origin/main` with each defect
cleared by a validation mechanic, not prose.

## §Behavior — the six defect-clearing mechanics (D1–D6)

| Row | Defect (from #12858 review) | Required mechanic | Proof |
|---|---|---|---|
| D1 | C210/C1204 `published_receipt_v0_17_0` = `present` though v0.17.0 shipped no Windows ARM64 asset | Receipt fields bind to a checked-in release-asset manifest (`distribution/release_receipts/v0.17.0.assets.json`, mirrored from the live `gh release view v0.17.0 --repo EffortlessMetrics/perl-lsp` asset list). The generator *derives* `dimensions.windows_arm64.published_receipt_v0_17_0` from that manifest (`absent` for v0.17.0 — its asset list has `perllsp-0.17.0-x86_64-pc-windows-msvc.zip` and no `aarch64-pc-windows-msvc` archive). The Python oracle cross-checks every `published_receipt_v0_17_0` value against the manifest — never against the inventory digest | `cargo xtask public-release-claims-v2 check` exit 0; oracle `--prove-tamper` receipt-field probe fails naming the row; manual audit command recorded in the manifest header |
| D2 | C1302/C1306/C1307/C1308 crates.io name collision demoted to `omitted_caveats` prose, reversing source truth | First-class closed-schema `identity_anti_claims` field on claim rows: `{kind: crates_io_name_collision, foreign_name: perl-lsp, owned_name: perllsp, disposition: do_not_install}` with `additionalProperties: false` and const-valued fields. `crates_io_name_collision` is **removed** from the `omitted_caveats` vocabulary. The field set is mechanically derived: every claim row whose raw inventory text (a) states the collision (names foreign `perl-lsp` in a crates.io/collision context: C203, C1101, C214) or (b) asserts a crates.io registry route `cargo install … perllsp` (non-`--path`, non-`--git` occurrence: C701, C702, C214, C1302, C1304–C1308) carries it — 10 rows including the four receipt-named rows. The validator fails any row whose raw inventory text satisfies (a) or (b) without the field | Oracle per-row re-derivation of the anti-claim set; tamper probe removing a row's anti-claim fails naming the row; unit test asserts the derived set contains C1302/C1306/C1307/C1308 |
| D3 | Summary trimming strips boundary backticks non-symmetrically, leaves interior Markdown delimiters → malformed rows | Cell extractor trims code-span delimiters **symmetrically**: drop exactly one leading and one trailing backtick only as a matched boundary pair, else keep the cell verbatim; no repeated/matched-set stripping. The validator rejects any derived `summary`/`notes` field with an **unbalanced** backtick delimiter count (odd count). Unit test covers every code-span-led claim row (all 70 summaries assert balanced delimiters) | Generator unit test over all rows; oracle rejects a tampered summary with an odd backtick count |
| D4 | FND-1 cites `action.yml:3` (=C401) but relation derivation only followed literal `FND-n` tokens → FND-1↔C401 missing | Finding↔claim relations derive from the inventory-cited **file:line → claim-location join** (same-cited-file mapping, basename granularity), not a token scan. Both sides cite files: findings cite `file:line` tokens in their prose; claim rows cite `file:line` in their Location cell. A finding relates to every claim row citing the same file basename. Regression assertion: FND-1 `related_claims` contains **C401**. Documented granularity: basename join is a deterministic superset (e.g. FND-10 citing `INSTALLATION.md:174` relates to all S02 rows); it is a derived relation surface, not a curated cross-reference | Generator regression test `fnd1_relates_c401`; oracle re-derives the full relation map and fails on any mismatch |
| D5 | Python oracle bound only the inventory digest; hand-edited artifact rows passed | The oracle re-derives **each generated row's content from its inventory source region** and compares row-by-row (whole-document hash is necessary, not sufficient): per-claim summary/location/drift/notes re-extraction, anti-claim set, relation map, receipt binding, digests. `--prove-tamper` runs a per-row tamper sweep: for every one of the 70 claim rows it mutates one derived field in a temp copy, re-validates, and requires the validator to **exit 1 naming that row** | `python scripts/validate_public_release_claims_v2.py distribution/public_release_claims.v2.json` exit 0; `--prove-tamper` prints a 70/70 caught table and exits 0; one manual negative run recorded in the PR body |
| D6 | Nested dimension objects accepted schema-forbidden keys (`dimensions.sha256sums_enforcement.rogue` passed) | `additionalProperties: false` on **every** object node of `schemas/public_release_claims.v2.schema.json` (verified by a structural walk of the schema file itself) and by closed-key checks in the oracle. Per-dimension-family rogue-key negative probes: injecting a `rogue` key into `windows_arm64`, `sha256sums_enforcement`, `product_units`, each `identity_anti_claims` item, and every other object family must fail both the schema closure walk and the oracle | `--prove-tamper` rogue-key section: each injected family fails naming the family; schema-walk check passes on this schema and fails on a probe-broken copy |

## §Artifact set

- `schemas/public_release_claims.v2.schema.json` — closed catalog schema (D6).
- `distribution/release_receipts/v0.17.0.assets.json` — release-asset
  manifest, receipt authority for D1 (closed schema: release, source,
  verified_date, sorted asset names).
- `distribution/public_release_claims.v2.json` — the generated 70-row catalog
  (13 surfaces, 12 findings), byte-canonical (sorted keys, 2-space indent,
  trailing LF), digest-bound to the inventory + schema + release manifest.
- `xtask/src/public_release_claims.rs` — generator/validation library;
  `xtask/src/tasks/public_release_claims.rs` — clap surface
  (`build [--write]` / `check` / `list` / `explain`), wired in
  `xtask/src/lib.rs`, `xtask/src/tasks/mod.rs`, `xtask/src/main.rs`.
- `scripts/validate_public_release_claims_v2.py` — the independent Python
  oracle (stdlib only, no external deps).

## §Proof seam (narrowest sufficient)

```bash
cargo xtask public-release-claims-v2 check                 # byte-stability + repository validation
python scripts/validate_public_release_claims_v2.py \
       distribution/public_release_claims.v2.json          # oracle green
python scripts/validate_public_release_claims_v2.py \
       distribution/public_release_claims.v2.json --prove-tamper   # per-row + rogue-key negatives
cargo test -p xtask --all-targets --locked public_release_claims   # focused module tests
cargo fmt -p xtask -- --check
cargo clippy -p xtask --lib --all-targets --locked -- -D warnings -A missing_docs
cargo clippy -p xtask --bins --all-targets --locked -- -D clippy::unwrap_used -D clippy::expect_used
just pr-fast                                               # if runtime allows
```

## §Non-goals (hard scope boundary)

- **No route IDs.** Rows carry no install-route identity; route IDs are
  reserved to the owner-gated true v2 contract (#10333 rail, #10334
  denominator, E00–E07).
- **No projection contexts.** No `development|candidate|release|public`
  projection-context fields; this catalog projects nothing beyond the
  inventory's own audit scope.
- **No v2 authority claims.** The PR must not `Closes #11548`, must not claim
  classifier-current or route-catalog authority, and must not present this as
  the #11549-consumable route contract. v1 remains historical/unmaintained
  per #10333; this derivative is non-authoritative inventory tooling.
- No producer packets, channel receipts, publication digests, policy/topology
  joins, support-posture binding, ranking, preference, or fragments.
- No mutation of `schemas/public_release_claims.v1.schema.json`,
  `scripts/validate_public_release_claims.py`, or any public surface.

## §Honesty ledger

- **Fixed in this PR:** D1–D6 as tabled above.
- **Deferred (recorded, not repaired):** all inventory findings FND-1…FND-12
  remain recorded-with-owner (`owner_route`); no prose is rewritten; the
  Windows ARM64 / SHA256SUMS / product-unit contradictions stay independent
  per-row dimensions awaiting the #11549 classifier and the distribution docs
  sync. The true route contract stays with the #10334 rail.
- **Receipt verification:** `v0.17.0.assets.json` mirrored from the live
  release asset list on 2026-08-28 via
  `gh release view v0.17.0 --repo EffortlessMetrics/perl-lsp --json assets`;
  the audit command is recorded in the manifest for re-verification.
