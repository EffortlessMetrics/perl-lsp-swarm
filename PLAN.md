# Plan: #11549 — Conjunctive install-route classification and preferred-route selection

> Status: LOCAL DRAFT (prepared 2026-08-27 on branch `codex/11549-planner` from
> `origin/main` `a9664af79`). Post to #11549 when gh budget restores. GitHub API
> was unavailable at authoring time; every fact below is sourced from local git
> objects (`origin/main` and branch `codex/11548-claims-v2`), cited inline.

## 0. Claim and entry

**Claim:** perl-lsp's install surfaces assert route facts along three
independent conjunctive dimensions (Windows ARM64, SHA256SUMS enforcement,
product-unit membership) plus a four-name subject-identity field. Today those
dimensions are asserted inconsistently across 13 prose surfaces (70 claim rows,
12 findings), and no mechanism selects which install route a user should be
told to take. #11549 delivers (1) a conjunctive route classifier derived
deterministically from the #11548 v2 catalog, and (2) a preferred-route
selection whose *ordering* is explicit, human-owned product data — never
derived.

**Entry flow:** `$prepare-issue` → this plan → `$prepare-proof`/`$build-candidate`
in a later lane. Sibling state at authoring time:

- #11575 inventory **landed** on `origin/main`:
  `docs/distribution/INSTALL_CLAIM_SURFACES.md` — 13 surfaces (S01–S13), 70
  claim rows (C101–C1309), 12 findings (FND-1–FND-12). Its "Family handoff
  notes → For #11549" section is the direct requirement source.
- #11548 v2 catalog **NOT landed**; flying on branch `codex/11548-claims-v2`
  (head `4501c89fc`, merge-base current with main). It adds
  `distribution/public_release_claims.v2.json` (generated, 70 claims / 13
  surfaces / 12 findings / 10 dimensioned rows),
  `schemas/public_release_claims.v2.schema.json`, generator
  `cargo xtask public-release-claims-v2 {build,check,list,explain}`, and a
  `scripts/validate_public_release_claims_v2.py` gate. The schema's own
  description says: *"this schema is the current catalog shape for install-route
  consumers (#11549 classifier)"* and FND-4's `owner_route` is
  `"#11549-classifier"`. **Dependency:** #11549 builds on the #11548 branch; if
  #11548 lands first, rebase point moves but nothing in this plan changes. If
  #11548's shape changes materially, only §2's join table needs re-derivation.

**Non-goals:** judging/rewriting prose wording strength (#10342), canonical
fragment generation (#10339), release receipts themselves (#7831 family),
registry producer-side dispositions (#9104), literal-pin linting (#10342 CI
cutover, which owns FND-10's allowlist), and any doc rewrite (FND-11 belongs to
`distribution-docs-sync`).

---

## 1. What the inventory says the classifier must honor

Derived from `docs/distribution/INSTALL_CLAIM_SURFACES.md`
(`git show origin/main:docs/distribution/INSTALL_CLAIM_SURFACES.md`):

- **13 surfaces, 70 claim rows.** Surfaces S01–S13 span root README, canonical
  install guide, CI guide, composite action + catalog, maintained example,
  upgrade/troubleshooting/editor guides, tutorial, extension manifest +
  marketplace copy, and 7 editor guides with live acquisition commands (S13).
  All rows are `curated` provenance; **zero generated fragments exist behind any
  row** (#10339 gap) — the catalog join is the only machine-readable backing
  until fragment generation lands.
- **8-value drift vocabulary** per row: `current`, `pending`, `stale_example`,
  `future_example`, `mutable_pin`, `cross_surface_drift`, `source_drift`,
  `volatile_number`. These are *independent results*, not a scalar quality —
  the classifier must keep them that way.
- **Three conjunctive dimensions named for #11549** (handoff notes, verbatim
  obligations):
  - **(a) Windows ARM64** — tracked source builds it (`release.yml:121-125`;
    C405, C501), user prose matches only published v0.17.0 assets (C210, C1204:
    "no native ARM64"). Treat **asset availability as receipt-bound per channel
    until an ARM64 Windows release ships**; neither prose direction may collapse
    into "supported" (FND-4, owned by `#11549-classifier`).
  - **(b) SHA256SUMS enforcement** — fail-closed canonical C207 vs fail-open
    residue C1005 describing the *same* `scripts/install.sh` (FND-7).
  - **(c) Product-unit membership** — `BUILD_FROM_SOURCE=1` installs perllsp
    only (C208) vs archives shipping the server+adapter pair (C209); FND-11
    shows tracked `install.ps1:221-229` already ships both executables, so
    tracked-installer behavior and published receipts diverge.
- **Subject-identity binding** — four-name collision field (C203, C1101):
  `perl-lsp` = foreign crates.io package (**anti-claim**: do-not-install),
  `perllsp` = server, `perl-dap` = adapter, `perl-lsp-rs` /
  `EffortlessMetrics.perl-lsp` = extension IDs.
- **Channel independence** (C201, C703, Distribution Matrix): GitHub Releases,
  VS Code Marketplace, Open VSX, crates.io, Homebrew, Docker, source builds are
  **separate receipts**; presence on one proves nothing about another.
- **Findings with route-relevant blast radius:** FND-1 (`@master` action pins ×4
  surfaces), FND-2 (stale example versions), FND-3 (`latest` defaults endorsed
  ×5 sites), FND-4 (ARM64 three-way split — **owned here**), FND-6 (unpinned
  `cargo install --git`), FND-7 (checksum-mode contradiction), FND-9
  (PowerShell-breakage annotation at 4 sites, split issue refs #4348/#5461),
  FND-11 (installer prose behind tracked source), FND-12 (unpinned
  no-checksum archive curl in VS_CODE_SETUP — first archive surface outside
  S02/S10/S01).

---

## 2. Conjunctive-classification design

### 2.1 Route subject model

Define a **route** as a named acquisition path a user can be told to take.
Ten routes are enumerable from the claim rows (every route below lists its
backing claim IDs — this is the join table):

| route_id | Backing claims | Notes |
| --- | --- | --- |
| `vscode-marketplace` | C102, C901, C902, C1003, C1101, C1202, C1203, C1204, C1301 | Extension; managed binary download (C1203 `channel: latest` default, FND-3) |
| `manual-archive` | C103, C206, C215, C1006, C1204, C1303 | GitHub release archive; C1303/FND-12 is a **no-checksum variant** — distinct sub-mode |
| `homebrew-tap` | C104, C213, C1206, C1304, C1305, C1308 | Owned tap; formula freshness unproven (C1304/C1305 `omitted_caveats`) |
| `cargo-registry` | C203 (anti-claim), C214, C701, C702, C1302, C1304, C1306, C1307, C1308 | `cargo install perllsp`; C203 rejects foreign `perl-lsp` |
| `cargo-git` | C1207 | Unpinned `--git` (FND-6) |
| `github-action` | C301, C302, C303, C401–C406, C501–C503, C601 | Composite action; FND-1/2/3 concentrated here |
| `posix-bootstrap` | C103, C204, C205, C207, C1004, C1005, C1206 | Identity-bound curl; fail-closed canonical C207 vs C1005 drift |
| `powershell-installer` | C105, C209, C210, C211 | Broken against published assets (#5461/#4348); FND-9/FND-10 |
| `source-build` | C208, C214, C1007, C1302 | Server-only under `BUILD_FROM_SOURCE=1` (C208); local path variants |
| `unproven-channels` | C212 | Scoop/Chocolatey/winget (`search`-verify only); Docker per Distribution Matrix |

Out-of-scope rows (must be *explicitly* excluded, not silently unjoined):
C801 (diagnostic advice), C901/C902 deferral framing keeps C902's currentness
claim joined to marketplace, C1001/C1002/C1008 (probes/posture), C1102
(virtual-workspace note), C1201/C101 (`volatile_number` badges), C1309
(`lsp-mcp` adjacent tool), C106/C216 (verification semantics — join as
post-install probes metadata, not route selection), C703 (channel
independence frame — a rule, not a route).

### 2.2 Classification = conjunction of independent per-dimension verdicts

For each route, classification is the **AND-join over all claim rows that
reference the route, evaluated per independent dimension**. Never reduce to a
scalar. Five verdict axes:

1. **Identity axis** — product units yielded (`perllsp` / `perl-dap` /
   `extension`) + identity names bound, honoring the collision map
   (`perl-lsp` → rejected-foreign). Source: `product_units` dimensions (C208,
   C209, C210) + C203/C1101.
2. **Platform axis** — per-OS/arch coverage with the **three-way**
   `windows_arm64` record (`user_prose`, `tracked_source`,
   `published_receipt`) kept separate exactly as #11548's schema already models
   it. Effective support = **receipt-bound**: `tracked_source=built` does NOT
   yield `supported` while `published_receipt=absent` (FND-4 disposition owned
   here).
3. **Integrity axis** — `sha256sums_enforcement` mode. A contradiction inside
   the conjunction (C207 fail-closed vs C1005 fail-open on the same
   `scripts/install.sh`) resolves **pessimistically to `contradicted`**, never
   to the optimistic value, until `distribution-docs-sync` lands FND-7.
4. **Freshness axis** — worst-of joined drift statuses, mapped:
   any joined `mutable_pin` | `cross_surface_drift` | `source_drift` |
   `stale_example` | `future_example` caps the route below `proven_current`;
   `pending` yields `pending_gate(issue)`; `volatile_number` is inert (does not
   gate routes; FND-8 is copy metadata).
5. **Receipt axis** — channel-independence: each route inherits exactly the
   receipt channel(s) its claims cite; no cross-channel inference (C201/C703).

Resulting **route verdict enum** (suggested):
`proven_current` · `receipt_bound_partial{dimension, values}` ·
`pending_gate{issue}` · `contradicted{claim_a, claim_b, finding}` ·
`unproven` · `not_recommended{reason, finding}`.

### 2.3 Where it lives (artifact + generator shape)

Follow the #11548 pattern exactly — deterministic, generated, schema-closed:

- Extend the xtask generator (new subcommand, e.g.
  `cargo xtask install-route-classification build --write`) that **consumes
  `distribution/public_release_claims.v2.json`** (input digest recorded) plus a
  small curated **route-join table** (`policy/install-route-join.toml` or a
  static map in the generator, mirroring how #11548 pinned
  `dimension_overrides(claim_id)` and `restatement_group(claim_id)` as static
  tables in `xtask/src/public_release_claims.rs`).
- Emit `distribution/install_route_classification.v1.json` + closed schema
  `schemas/install_route_classification.v1.schema.json`; regenerate-and-compare
  byte-identity check wired beside #11548's Python/xtask gate.
- Classification (§2.2) is **pure derivation** — it must be mechanical and
  testable. The only curated inputs are: route→claim join, the anti-claim
  identity map, and the pessimistic-contradiction rule. **Preference ordering
  is NOT derived here** (§3).
- Sequencing note: if #11548 hasn't landed when work starts, build against its
  branch and rebase; the join table (§2.1) is the only #11548-shape-coupled
  artifact.

---

## 3. Preferred-route selection: algorithm options and tradeoffs

Selection input: `(platform, arch, desired product units, context
{editor | ci | server-only | manual}, risk posture)`. Output: ordered route
recommendation with per-route verdicts and gate citations.

### Option A — Static precedence table (curated data only)

A `policy/install-route-preference.toml` maps context keys to **ordered route
lists**; selection validates each listed route's classification verdict before
emitting it (demote/annotate anything below `proven_current`).

- **Pros:** ordering is explicit auditable product authority; fully
  deterministic; trivially testable; no inference to explain; stale entries
  fail loudly against regenerated verdicts.
- **Cons:** combinatorial surface (platform × units × context); every new route
  needs manual placement; ordering drift is silent unless verdict-gating is
  strict.

### Option B — Scored/lexicographic derivation

Derive ordering from weighted dimensions (integrity mode > receipt freshness >
drift > unit completeness > platform match) with tie-breakers.

- **Pros:** less manual data; new routes slot in automatically.
- **Cons:** **weights are product authority smuggled in as constants** — the
  exact failure this plan must avoid; explanations become "the scoring said
  so"; contradictions need special-casing anyway; can silently contradict
  documented guidance (e.g., INSTALLATION.md C202's fastest-path enumeration is
  convenience-first, while the bootstrap identity contract is
  integrity-first — a scorer must pick one without authority to do so).
- **Verdict: reject** as the ordering mechanism; acceptable only *inside* a
  context bucket as a deterministic tie-break among equal-verdict routes, if
  ratified by an EXPLICIT-HUMAN row.

### Option C — Constraint filter + small curated order (recommended hybrid)

Mechanical **hard filter** eliminates routes whose verdict disqualifies them
for the request (platform unsupported per receipt; integrity mode below floor
(`fail_open_conditional`/`contradicted` fails; `verify_present_no_mode` is
context-tolerant); product-unit mismatch; identity-collision rejection).
Survivors are ordered by a **small explicit context policy table** seeded from
the de facto order already taught by S02's C202 enumeration (extension →
manual archive → other-editor download → local cargo), with CI context seeded
from S03 (pinned action first). Option A's table, but only where prose already
implies an order + a refusal rule that is pure derivation.

- **Pros:** the filter is provable and conjunctive (matches the issue title);
  curated data stays small (one order per context, not per platform×units);
  consistent with existing doc behavior — the classifier *formalizes* what
  INSTALLATION.md already teaches rather than inventing policy.
- **Cons:** still requires EXPLICIT-HUMAN rows (§5) for orderings prose does
  not settle; two moving parts (filter + table).

**Recommendation: Option C.** Option B is rejected because preference ordering
is product authority and must remain reviewable data, not emergent constants.

---

## 4. Discriminating falsifiers

Each falsifier distinguishes a correct classifier/selector from a plausible
wrong one. All are cheap (pure functions over catalog + table; no network,
no installs).

1. **Receipt-binding (FND-4, the falsifier #11548 explicitly deferred here).**
   Query `(windows, aarch64, {perllsp}, editor)`. Wrong implementations join
   `tracked_source=built` (C405/C501) or `user_prose=x64_fallback_build_from_source`
   (C1204) into "archive download supported". Correct output: **no
   receipt-backed native route** while `published_receipt_v0_17_0=absent`;
   either a refusal citing FND-4 or an explicit x64-fallback/build-from-source
   recommendation labeled receipt-bound. A classifier that prints "supported"
   for Windows ARM64 fails.
2. **Conjunction-contradiction (FND-7).** Route `posix-bootstrap` joins C207
   (`fail_closed_required`) and C1005 (`fail_open_conditional`). Integrity
   verdict must be `contradicted{C207,C1005,FND-7}` — any implementation
   emitting `fail_closed_required` (took canonical) or `fail_open_conditional`
   (took first/last row) fails.
3. **Identity collision (C203/C1101).** A query or join keyed on the string
   `perl-lsp` must not resolve to the `cargo-registry perllsp` route; the
   anti-claim map must reject it with the collision note. An implementation
   that substring-matches crate names fails.
4. **Product-unit mode separation (C208 vs C209 vs FND-11).**
   `(source-build, {perllsp, perl-dap})` → source-build classifies **server-only**
   (C208 is receipt-bound for published behavior; FND-11 says the tracked
   installer ships the adapter but that is not a published receipt).
   `(manual-archive, {perllsp, perl-dap})` → pair OK. An implementation that
   merges `build_from_source_units` with `archive_units_claimed`, or that lets
   `tracked_installer_ships_adapter=true` upgrade the source-build route to
   pair-complete, fails.
5. **Stale-preference demotion.** Preference table lists a route whose joined
   rows include `mutable_pin` (e.g., `github-action` under FND-1/2/3, or
   `cargo-git` under FND-6): selection must demote/annotate it below any
   `proven_current` competitor — never reorder the table to hide the verdict.
   `cargo-git` must never outrank `cargo-registry`.
6. **Pending-gate honesty (FND-9).** `(windows, x86_64, {perllsp})`: the table
   may list `manual-archive` first, but `powershell-installer` must appear (if
   at all) as `pending_gate(#5461/#4348)`/`not_recommended` — not silently
   omitted (silence hides the gate) and not recommended (a wrong implementation
   ranks by prose recency).
7. **Determinism.** `cargo xtask install-route-classification check` is
   byte-identical across runs and machines: classification output contains no
   timestamps, ambient state, or catalog-order dependence beyond stable claim
   IDs. Any run-to-run diff fails.
8. **Denominator closure (mirrors #11548's missing-producer-omits-route
   pattern).** Every one of the 70 claim rows is either joined to ≥1 route or
   listed in the explicit out-of-scope set (§2.1). A join referencing an
   unknown claim ID, or an unjoined and unexcluded row, fails the check.
9. **Cross-channel inference block (C201/C703).** A claim's receipt on channel
   X must never satisfy another route's receipt requirement (e.g., GitHub
   Releases v0.17.0 receipt must not make `homebrew-tap` `proven_current`).
   An implementation with a global "release exists" fact fails.

---

## 5. EXPLICIT-HUMAN decision rows

Preference ordering is **product authority, not derivable** from source or
inventory. These rows cannot be resolved by the implementer; each needs a
maintainer ruling (issue comment suffices). Defaults are proposed so a ruling
can be one word.

| # | Decision | Options | Proposed default if unanswered | Consequence of getting it wrong |
| --- | --- | --- | --- | --- |
| H1 | Global ordering principle: convenience-first (C202 teaches extension → archive → …) vs integrity-first (identity-bound verify > ease) | (a) convenience-first per C202; (b) integrity-first reorder | (a) convenience-first, matching live prose; integrity surfaces as annotation | Classifier formalizes guidance that contradicts maintainer intent across every context |
| H2 | Windows x86_64 default while published PowerShell installer is broken (#5461/#4348) | (a) manual-archive first, powershell shown as `pending_gate`; (b) omit powershell entirely; (c) lead with scoop/choco/winget verify-first | (a) | Users routed to a 404 installer, or gate visibility lost (FND-9's four-site spread re-grows) |
| H3 | Windows ARM64 until an ARM64 release ships (FND-4 — routed to #11549 by name) | (a) recommend x64-fallback + build-from-source labeled receipt-bound; (b) refuse with "no receipt-backed route"; (c) follow prose (contradicts receipts) | (a), citing FND-4 | Either false "supported" claim or an unnecessarily barren answer; FND-4 disposition must be defensible |
| H4 | macOS server-only ranking: homebrew-tap vs cargo-registry (both `current`; tap freshness unproven per C1304/C1305 caveats) | (a) tap first (native UX); (b) cargo-registry first (version receipts); (c) context-split (editor=tap, headless=cargo) | (c) context-split | A preferred route whose freshness caveat the docs themselves flag |
| H5 | Unpinned mutability policy: `cargo-git` (FND-6) and `latest` endorsements (FND-3) | (a) classify `not_recommended`, never selected; (b) selectable with warning; (c) leave unclassified | (a); CI context additionally: pinned `version:` only, contradicting C303's `latest` endorsement | Classifier endorses what FND-3 calls a moving target the receipt does not cover |
| H6 | perl-dap (adapter) acquisition default for non-VS Code users | (a) archive pair route; (b) separate `cargo install --locked perl-dap` (C702); (c) defer — server-only answer, adapter on request | (a) for editor contexts, (b) for headless, mirroring C702 | Users get a server without a debugger, or build-from-source surprises (C208) |
| H7 | Unproven channels (`unproven-channels`: scoop/choco/winget C212, Docker) in selection output | (a) never selected, visible in a "unproven" appendix; (b) fully omitted; (c) selectable with verify-first instruction (as C212 prose does) | (a) | Either dead output weight or an implied endorsement the receipts don't back |

*(Count: 7 EXPLICIT-HUMAN rows.)*

---

## 6. Implementation sizing, title, and next actions

**Suggested conventional title:**
`feat(distribution): add conjunctive install-route classification and preferred-route selection (#11549)`

**Sizing: M** — one candidate writer, one coherent claim. Roughly: route-join
table + verdict derivation (~250–400 lines incl. static tables, following
#11548's `xtask/src/public_release_claims.rs` idiom), artifact + closed schema +
regen-check wiring (~150–250), selection filter + context policy table
(~100–200), and falsifier tests (§4) (~300). No production-crate changes; no
doc rewrites. **Optional split if review prefers:** slice 1 = classification
artifact (S/M), slice 2 = selection + policy table (S), sharing §2.1's join
table. Do NOT pull FND-7/FND-11 doc syncs or #10342 linting into this claim.

**Hard prerequisite check at build time:** if `codex/11548-claims-v2` has landed,
consume landed `distribution/public_release_claims.v2.json`; otherwise branch
from it and state the stacking in the PR body.

**Proof strategy:** all §4 falsifiers as focused `cargo test -p xtask` cases
plus the byte-identity regen check; `just doctor`, `cargo fmt -p xtask --
--check`, `cargo clippy -p xtask --all-targets --locked -- -D warnings`,
`cargo test -p xtask --all-targets --locked`. No CI-cycle dependency beyond the
standard gates.

**Residual risks:**
- #11548 branch may still move (schema/claim-ID changes) → re-derive §2.1 join
  table only; plan logic unaffected.
- H1–H5 unanswered → implementer proceeds on proposed defaults but must mark
  the policy table rows `provisional(human-pending)` in the artifact so the
  authority gap is machine-visible, not silent.
- Prose drift (new surfaces) after inventory audit commit `20174d50c` →
  denominator closure falsifier catches unmodeled rows only after the inventory
  itself is re-audited (#11575 cadence), so the check is bounded by that
  document's freshness, by design.
