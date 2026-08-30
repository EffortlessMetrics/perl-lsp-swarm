# Install-Route Classification Implementation Plan

Status: durable tracked plan, refreshed 2026-08-30 — a provisional design that
is non-operative for selection while H1–H7 are human-pending and is not an
executable input contract until the validated route schema from issue #10333
and the catalog composition from issue #10334 exist (#10333 explicitly
excludes route population)
Owner: perl-lsp maintainers
Tracker: [#11549](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11549)
References: [#10333](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10333) (route schema/validation boundary), [#10334](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10334) (catalog composition and sequencing/fan-in), [#11434](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11434) (canonical denominator), [#11432](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11432) (evidence producers), [#11575](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11575) (landed inventory), [#11164](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11164) (current #10333 blocker); superseded [#12858](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/12858) is historical context only

Subject: conjunctive install-route classification and preferred-route
selection. Issue #11549 is the authority for human product rulings; this file
is repository-carried planning authority for scope, prerequisites, and proof
obligations.

## 0. Claim and entry

**Claim:** perl-lsp's install surfaces assert route facts across seven
independent hard dimensions: identity/topology, platform/target,
product-unit/lifecycle, integrity/provenance, freshness/channel/publication,
PATH/session/execution, and receipt binding. Today those dimensions are
asserted inconsistently across 13 prose surfaces (70 claim rows, 12 findings),
and no mechanism selects which install route a user should be told to take.
  Issue #11549 delivers (1) a conjunctive route classifier derived
deterministically
from the validated route schema and validation boundary owned by #10333, with
catalog composition and sequencing/fan-in owned by Issue #10334, the canonical
route denominator supplied by #11434, and evidence
producers such as #11432, and (2) a preferred-route selection whose *ordering*
is explicit, human-owned product data — never derived.

**Entry flow:** `$prepare-issue` → this plan → `$prepare-proof`/`$build-candidate`
in a later lane. Sibling state at authoring time:

**Human-ruling gate:** H1–H7 are explicit maintainer decisions, not defaults that
the implementer may adopt. Until each row is ruled in the authoritative issue, this
plan is a provisional design only: no preferred-route ordering, recommendation,
selection policy, or implementation may be treated as operative. A later lane may
prepare classification proof and fixtures only against the validated catalog; it
must stop at `provisional(human-pending)` for the selection surface.

- #11575 inventory **landed** on `origin/main`:
  `docs/distribution/INSTALL_CLAIM_SURFACES.md` — 13 surfaces (S01–S13), 70
  claim rows (C101–C1309), 12 findings (FND-1–FND-12). Its "Family handoff
  notes → For #11549" section is the direct requirement source.
- Dependency map: **#10333 owns the route schema/validation boundary and
  explicitly excludes route population** and is currently open and blocked by
  #11164; **#10334 owns catalog composition and sequencing/fan-in** under that
  schema contract; **#11548 is the current-v2 catalog assembly train under
  #10334**, not an independent schema authority; **#11443 owns the E07
  cross-cutting PATH/transition/publication closure fan-in under #10334**;
  **#11434 owns the canonical route denominator**; and
  **#11432 plus the other named producers own evidence inputs**. #11549 starts
  only after those authorities publish a validated contract. The closed,
  superseded #12858 attempt is historical context only and cannot supply route
  rows, projection contexts, producer joins, publication/channel bindings, or
  fail-closed route states.
- **Dependency state: OPEN-UNRESOLVED.** #12858 — the only PR ever opened for
  #11548's v2 route catalog — was closed unmerged as superseded on
  2026-08-27 with an explicit do-not-pin/do-not-fixture disposition, and
  a scoped successor is now in flight: PR #13362
  (`feat/11548-inventory-derivative`, open as of 2026-08-30 as observed then;
  GitHub remains authoritative for its current state) rebuilds the
  #12858 candidate as an explicitly **non-authoritative** #11575 inventory
  derivative — its rows carry no route IDs and no projection contexts, it claims
  no #10333/#10334 route-contract authority, and it does not close #11548 or
  #11549. It therefore does **not** satisfy the authoritative v2-catalog input
  this plan consumes, and the wake event below is unchanged: the authoritative
  #11548 v2-catalog route rows, or #10334 publishing the composed catalog under
  #10333's schema/validation boundary, re-bases this plan — §2.1's
  route families, the §2.1.1 closure denominators, and the §4 fixtures are
  then re-derived and rebound to exact catalog rows before any implementation
  lane starts. Until that event, every route join, projection context, and
  the 70-row denominator below are **provisional prose-audit derivations**
  from the landed #11575 inventory — planning fixtures, not accepted
  contract inputs.

**Non-goals:** judging/rewriting prose wording strength (#10342), canonical
fragment generation (#10339), release receipts themselves (#7831 family),
registry producer-side dispositions (#9104), literal-pin linting (#10342 CI
cutover, which owns FND-10's allowlist), and any doc rewrite (FND-11 belongs to
`distribution-docs-sync`).

---

## 1. What the inventory says the classifier must honor

The generated inventory delta is part of this plan only because adding the
tracked plan file `plans/install-route-classification/implementation-plan.md`
requires regenerating its report. The two `generate-badges` rows also
added by that regeneration describe `scripts/generate-badges.py` and
`scripts/tests/test-generate-badges.py`, which already exist on `origin/main` and
were absent from the base report; this PR does not add or modify those sources.
The inventory's +2 Rust-family entries likewise describe the pre-existing
`xtask/src/tasks/ci_route.rs` and `xtask/tests/ci_route_cli.rs` files, not files
introduced by this PR. The count changes therefore reconcile a stale generated
report rather than expand the implementation scope.

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
The exact route denominator, route IDs, projection contexts, and producer joins
must come from the validated catalog composed by #10334 under #10333's schema and
validation boundary (which excludes route population), then closed over the
canonical denominator from #11434. The following are
planning families only — provisional prose-audit derivations from the landed
issue #11575 inventory while the catalog dependency of §0 is OPEN-UNRESOLVED;
they are not an accepted catalog or a claim-to-route
join and must not be implemented as a hard-coded substitute:

| route_id | Backing claims | Notes |
| --- | --- | --- |
| VS Code Marketplace; Open VSX | Separate extension and managed-binary acquisition channels | Exact route rows, registry identity, and gallery contexts come from the catalog; C1202 and C1208 cannot share a receipt or route projection |
| Identity-bound archive / POSIX bootstrap | Release archive and bootstrap variants | Checksum and publication bindings come from the catalog |
| Homebrew / Cargo registry / unpinned Cargo git | Separate channel subjects | No cross-channel inference |
| Setup Action release/source modes | CI-oriented route families | Explicit ref and product-unit context |
| Windows zip / source-local builds; generic-client PATH | Platform, product-unit, and generic-client variants | Receipt-bound support remains separate; C107 is projected when the catalog supplies this route |
| Unproven channels | Scoop, Chocolatey, winget, Docker, or other deferred channels | Only catalog-provided channels may enter here |

Out-of-scope rows (must be *explicitly* excluded, not silently unjoined): C108
(`non_install_dependency`, external formatting/critic tools), C201
(`channel_rule`, channel-independence frame), C703 (`channel_rule`, channel-
independence frame), C801 (`diagnostic_surface`, diagnostic advice), C1001
(`diagnostic_surface`, PATH/health advice), C1002 (`non_install_dependency`,
runtime-dependency posture), C1008 (`verification_metadata`, post-install
probes), C1102 (`non_install_dependency`, virtual-workspace limitation), C1201
(`volatile_metadata`, install-count badge), C1205 (`non_install_dependency`,
internal deployment guidance), C1309 (`adjacent_product`, `lsp-mcp` tool).
C106 and C216 are not route-selection rows: they must be listed as explicit
`verification_metadata` exclusions in the future ledger while their post-install
probe metadata remains available to the classifier.

### 2.1.1 Inventory projection/exclusion oracle

The landed #11575 inventory is the complete audit input for this plan, not the
future route denominator. A validator must parse every literal claim ID in its
claim-row table and require an exact-one disposition record for every claim in the
future join data (a record may project to a route already supported by other
claims):

1. a claim is projected to one or more exact catalog route rows and projection
   contexts, or
2. it is listed in an explicit exclusion record containing the claim ID and a
   reason (`diagnostic_surface`, `verification_metadata`, `channel_rule`,
   `volatile_metadata`, `adjacent_product`, or `non_install_dependency`).

The validator rejects an unknown ID, duplicate disposition, missing ID, range
shorthand, or exclusion reason not in that closed vocabulary. Claim kind is also
validated: `channel_rule` is valid only for channel-independence claims,
`diagnostic_surface` only for diagnostic advice, `verification_metadata` only for
verification-only rows, `volatile_metadata` only for volatile metadata,
`adjacent_product` only for adjacent products, and `non_install_dependency` only
for non-install dependencies. A channel-specific acquisition claim must project
to that channel's exact catalog route and compatible projection context. Therefore
C1208 (Open VSX) must project to an Open VSX route in an Open VSX-compatible
marketplace context; it cannot be excluded as a `channel_rule`, or by any other
reason incompatible with its claim kind. This prevents arbitrary exclusions from
hiding route-relevant claims. The accepted join must explicitly dispose of
currently unlisted rows such as C107, C108, C1205, and C1208: generic-client and
Open VSX rows must be projected when route-relevant, while non-install dependency
or metadata rows may be excluded only with a compatible explicit reason. `C202` is one
inventory claim with four required route/context projections, not one preferred route:
(1) the VS Code extension route in the `VS_Code_compatible_managed_client` context,
(2) the manual archive route in the `macOS_or_Linux_or_Windows_manual_archive`
context, (3) the other-editor download route in the `generic_LSP_client` context,
and (4) the local Cargo route in the `local_testing_or_prerelease_validation`
context. Each projection must bind to its own exact catalog route ID and compatible
context; the four rows may not be collapsed, inferred across editor families, or
ordered from the prose claim. The catalog contract owns the eventual opaque IDs, so
these semantic route roles and contexts are requirements only until #10334 publishes
catalog rows conforming to #10333's contract.

C202's `VS_Code_extension` projection is the registry-unspecified family row;
C1202's marketplace and Open VSX projections and C1208's Open VSX projection are
registry-specific rows and must bind to catalog route IDs distinct from it,
distinguished by registry and projection context. Registry-specific children may
share a registry when their projection contexts differ. When a request supplies
`target_registry`, only the matching registry-specific projection is eligible and
the family row is not returned; when
`target_registry` is unspecified, the family row is eligible and the
registry-specific rows are returned only as diagnostics. No single request may
receive both the family row and one of its registry-specific rows. The
family/child relation is a required property of the future #10334 catalog under
#10333's contract, declared in the ledger rather than inferred from route names.

The compatibility contract is per ID, not merely a count or a closed vocabulary:
every route-relevant claim is required to be a `project` disposition with its own
expected semantic route role and projection context. The fixture binds those roles
to distinct synthetic route/context pairs so swapping C101 and C102 is rejected even
when the pair denominator is unchanged. The same rule applies to each finding:
FND-1, FND-2, FND-3, FND-4, FND-6, FND-7, FND-9, FND-11, and FND-12 are route joins;
FND-5 is the route-independent constraint for the mutable deployment links; FND-8
is the `volatile_metadata` exclusion for the install-count badge; and FND-10 is a
route-independent constraint owned by the literal-pin policy. These are expected
finding-specific dispositions, not replaceable examples. A route-relevant claim or
finding changed to an allowed exclusion, or a finding changed to the wrong kind,
must fail before catalog closure.

The same exact-once rule applies to FND-1 through FND-12: each finding must be joined to a
route, recorded as a route-independent constraint, or explicitly excluded with a
compatible reason. This oracle is the acceptance condition for inventory
traceability; the 70-row prose count and the planning-family table are not
substitutes for it.

For the six rows raised by review, the provisional ledger disposition is literal:
C107 → `project(generic-client PATH, generic-client editor context)`;
C108 → `exclude(non_install_dependency, external formatting/critic tools)`;
C201 → `exclude(channel_rule, channel-independence frame)`;
C202 → four distinct projection records:
`project(VS_Code_extension, VS_Code_compatible_managed_client)`,
`project(manual_archive, macOS_or_Linux_or_Windows_manual_archive)`,
`project(other_editor_download, generic_LSP_client)`, and
`project(local_cargo, local_testing_or_prerelease_validation)`. Each record must
  carry its own exact catalog route ID after #10334 publishes the catalog under
  the #10333 contract; labels
on a reused route/context tuple do not satisfy this requirement. C1205 →
`exclude(non_install_dependency, internal deployment guidance)`; and C1208 →
`project(open-vsx, Open VSX-compatible marketplace context)`. These are required
dispositions for the future validated catalog ledger; they are not permission to
invent route IDs before #10334 publishes that catalog under #10333's contract.

#### Literal closure manifest

The following is the complete literal audit manifest for the landed #11575
inventory. The future join/exclusion ledger must contain exactly one disposition
record for every ID below; C202's one project disposition expands to four
distinct projection records. These lines are the auditable denominator, not
shorthand or examples. A disposition is either `project(route_id, projection_context)` or
`exclude(reason, rationale)`, and no ID may appear in both forms.

| Inventory surface | Literal claim IDs |
| --- | --- |
| S01 | C101, C102, C103, C104, C105, C106, C107, C108 |
| S02 | C201, C202, C203, C204, C205, C206, C207, C208, C209, C210, C211, C212, C213, C214, C215, C216 |
| S03 | C301, C302, C303 |
| S04 | C401, C402, C403, C404, C405, C406 |
| S05 | C501, C502, C503 |
| S06 | C601 |
| S07 | C701, C702, C703 |
| S08 | C801 |
| S09 | C901, C902 |
| S10 | C1001, C1002, C1003, C1004, C1005, C1006, C1007, C1008 |
| S11 | C1101, C1102 |
| S12 | C1201, C1202, C1203, C1204, C1205, C1206, C1207, C1208 |
| S13 | C1301, C1302, C1303, C1304, C1305, C1306, C1307, C1308, C1309 |

The finding denominator is likewise literal and exact-once: FND-1, FND-2,
FND-3, FND-4, FND-5, FND-6, FND-7, FND-8, FND-9, FND-10, FND-11, and FND-12.
Each finding must have one and only one ledger disposition: a route join, a
route-independent constraint, or an explicit exclusion with an allowed reason.
In particular, FND-5 is a `route-independent constraint` for the mutable
`INTERNAL_DEPLOYMENT.md` links represented by C1205; it is not silently omitted.

The closure contract is a future executable acceptance fixture, not proof that a
production ledger exists in this planning PR. The fixture below is deliberately
self-contained and list-based: repeated IDs are observable, and multi-route
claims (including C202) cannot overwrite one another in a dictionary. Its synthetic route
IDs are test data only; the implementation must replace them with distinct opaque
IDs from #10334 under #10333's contract and validate the catalog digest before
accepting the ledger. The
fixture catalog below uses one deliberately opaque ID so C1208 resolution tests
the catalog binding, not a route-name convention.

```python
from collections import Counter
from pathlib import Path
import re

EXPECTED_CLAIMS = tuple(
    f"C{n}"
    for n in (list(range(101, 109)) + list(range(201, 217)) +
              list(range(301, 304)) + list(range(401, 407)) +
              list(range(501, 504)) + [601] + list(range(701, 704)) + [801] +
              list(range(901, 903)) + list(range(1001, 1009)) + [1101, 1102] +
              list(range(1201, 1209)) + list(range(1301, 1310))))
EXPECTED_FINDINGS = tuple(f"FND-{n}" for n in range(1, 13))

INVENTORY_PATH = Path("docs/distribution/INSTALL_CLAIM_SURFACES.md")

def inventory_rows_from_markdown(markdown):
    """Read claim IDs from the checked-in inventory, preserving table order."""
    claim_section = markdown.split("## Claim rows", 1)
    if len(claim_section) != 2:
        raise ValueError("inventory is missing its claim-row section")
    rows = []
    for line in claim_section[1].splitlines():
        match = re.match(r"^\|\s*(C\d+)\s*\|", line)
        if match:
            rows.append({"id": match.group(1)})
    if not rows:
        raise ValueError("inventory contains no claim rows")
    return rows

def checked_in_inventory_rows():
    return inventory_rows_from_markdown(INVENTORY_PATH.read_text(encoding="utf-8"))
C202_PROJECTIONS = [
    ("VS_Code_extension", "VS_Code_compatible_managed_client"),
    ("manual_archive", "macOS_or_Linux_or_Windows_manual_archive"),
    ("other_editor_download", "generic_LSP_client"),
    ("local_cargo", "local_testing_or_prerelease_validation"),
]
ALLOWED_EXCLUSIONS = {
    "diagnostic_surface", "verification_metadata", "channel_rule",
    "volatile_metadata", "adjacent_product", "non_install_dependency",
}
CLAIM_DISPOSITION_RULES = {
    "C108": {
        "kind": "exclude",
        "reason": "non_install_dependency",
        "rationale": "external formatting/critic tools",
    },
    "C201": {
        "kind": "exclude",
        "reason": "channel_rule",
        "rationale": "channel-independence frame",
    },
    "C703": {
        "kind": "exclude",
        "reason": "channel_rule",
        "rationale": "channel-independence frame",
    },
    "C801": {
        "kind": "exclude",
        "reason": "diagnostic_surface",
        "rationale": "diagnostic advice",
    },
    "C1001": {
        "kind": "exclude",
        "reason": "diagnostic_surface",
        "rationale": "PATH/health advice",
    },
    "C1002": {
        "kind": "exclude",
        "reason": "non_install_dependency",
        "rationale": "runtime-dependency posture",
    },
    "C1008": {
        "kind": "exclude",
        "reason": "verification_metadata",
        "rationale": "post-install probes",
    },
    "C1102": {
        "kind": "exclude",
        "reason": "non_install_dependency",
        "rationale": "virtual-workspace limitation",
    },
    "C1201": {
        "kind": "exclude",
        "reason": "volatile_metadata",
        "rationale": "install-count badge",
    },
    "C1205": {
        "kind": "exclude",
        "reason": "non_install_dependency",
        "rationale": "internal deployment guidance",
    },
    "C1309": {
        "kind": "exclude",
        "reason": "adjacent_product",
        "rationale": "lsp-mcp tool",
    },
    "C106": {
        "kind": "exclude",
        "reason": "verification_metadata",
        "rationale": "post-install probe metadata",
    },
    "C216": {
        "kind": "exclude",
        "reason": "verification_metadata",
        "rationale": "post-install probe metadata",
    },
}
REQUIRED_EXCLUSION_IDS = (
    "C106", "C108", "C201", "C216", "C703", "C801", "C1001", "C1002",
    "C1008", "C1102", "C1201", "C1205", "C1309",
)
ROUTE_FINDING_IDS = (
    "FND-1", "FND-2", "FND-3", "FND-4", "FND-6", "FND-7", "FND-9",
    "FND-11", "FND-12",
)
CLAIM_ROUTE_BINDINGS = {
    item_id: {
        "exact_catalog_route_id": f"fixture-route-{item_id}",
        "exact_projection_context": f"fixture-context-{item_id}",
    }
    for item_id in EXPECTED_CLAIMS
    if item_id not in REQUIRED_EXCLUSION_IDS and item_id != "C202"
}
CLAIM_ROUTE_BINDINGS["C202"] = tuple(
    {
        "exact_catalog_route_id": f"fixture-route-{route}",
        "exact_projection_context": context,
    }
    for route, context in C202_PROJECTIONS
)
CLAIM_ROUTE_BINDINGS["C1208"] = {
    "exact_catalog_route_id": "r_4f8c2a",
    "exact_projection_context": "Open_VSX_compatible_marketplace_context",
}
MULTI_ROUTE_PROJECTIONS = {
    # A claim may support several independently selectable channel/context
    # projections.  These fixture IDs are opaque test data, not catalog IDs.
    "C1202": ("marketplace", "open-vsx"),
    "C1302": ("registry", "local-cargo"),
    "C1304": ("registry", "homebrew", "local-cargo"),
    "C1305": ("registry", "homebrew", "local-cargo"),
    "C1308": ("registry", "homebrew"),
}
for item_id, route_names in MULTI_ROUTE_PROJECTIONS.items():
    CLAIM_ROUTE_BINDINGS[item_id] = tuple({
        "exact_catalog_route_id": f"fixture-route-{item_id}-{route_name}",
        "exact_projection_context": f"fixture-context-{item_id}-{route_name}",
    } for route_name in route_names)
ROUTE_FAMILY_CHILDREN = {
    "fixture-route-VS_Code_extension": (
        "fixture-route-C1202-marketplace",
        "fixture-route-C1202-open-vsx",
        "r_4f8c2a",
    ),
}
ROUTE_FAMILY_REGISTRY_ROLES = {
    "fixture-route-VS_Code_extension": None,
    "fixture-route-C1202-marketplace": "fixture_marketplace",
    "fixture-route-C1202-open-vsx": "open_vsx",
    "r_4f8c2a": "open_vsx",
}
FINDING_DISPOSITION_RULES = {
    finding_id: {
        "kind": "project",
        "exact_catalog_route_id": f"fixture-finding-route-{finding_id}",
        "exact_projection_context": f"fixture-finding-context-{finding_id}",
    }
    for finding_id in ROUTE_FINDING_IDS
}
FINDING_DISPOSITION_RULES.update({
    "FND-5": {
        "kind": "constrain",
        "route_independent": True,
        "exact_reason": "mutable INTERNAL_DEPLOYMENT links",
    },
    "FND-8": {
        "kind": "exclude",
        "reason": "volatile_metadata",
        "rationale": "install-count badge",
    },
    "FND-10": {
        "kind": "constrain",
        "route_independent": True,
        "exact_reason": "literal-pin policy owned by #10342",
    },
})
FINDING_DISPOSITION_RULES["FND-3"] = tuple({
    "kind": "project",
    "exact_catalog_route_id": f"fixture-finding-route-FND-3-{route_name}",
    "exact_projection_context": f"fixture-finding-context-FND-3-{route_name}",
} for route_name in ("marketplace", "homebrew"))

def projection_records(subject_id):
    expected = (CLAIM_ROUTE_BINDINGS if subject_id.startswith("C")
                else FINDING_DISPOSITION_RULES)[subject_id]
    return list(expected) if isinstance(expected, tuple) else [expected]

def fixture_catalog():
    rows = []
    for item_id in EXPECTED_CLAIMS:
        if item_id in REQUIRED_EXCLUSION_IDS:
            continue
        for projection in projection_records(item_id):
            route_id = projection["exact_catalog_route_id"]
            context = projection["exact_projection_context"]
            rows.append({
                "route_id": route_id,
                "target_registry": ROUTE_FAMILY_REGISTRY_ROLES.get(
                    route_id,
                    "open_vsx" if item_id == "C1208" else "fixture_registry",
                ),
                "projection_contexts": (context,),
            })
    rows.extend({
        "route_id": projection["exact_catalog_route_id"],
        "target_registry": "fixture_registry",
        "projection_contexts": (projection["exact_projection_context"],),
    } for finding_id in ROUTE_FINDING_IDS
      for projection in projection_records(finding_id))
    return tuple(rows)

FIXTURE_CATALOG = fixture_catalog()
WRONG_REGISTRY_CATALOG = tuple(
    {
        **row,
        "target_registry": "vs_marketplace",
    } if row["route_id"] == "r_4f8c2a" else row
    for row in FIXTURE_CATALOG
)
DUPLICATE_ID_CATALOG = FIXTURE_CATALOG + tuple(
    row for row in FIXTURE_CATALOG if row["route_id"] == "r_4f8c2a"
)

def resolve_catalog_route(catalog_rows, route_id):
    matches = [row for row in catalog_rows if row["route_id"] == route_id]
    if len(matches) != 1:
        raise ValueError(f"route ID must resolve exactly once: {route_id}")
    return matches[0]

def require_catalog_projection(catalog_rows, route_id, projection_context):
    route = resolve_catalog_route(catalog_rows, route_id)
    if projection_context not in route["projection_contexts"]:
        raise ValueError("route is incompatible with its catalog context")
    return route

def require_route_family_bindings(
    catalog_rows,
    route_family_children=ROUTE_FAMILY_CHILDREN,
):
    catalog_route_ids = {row["route_id"] for row in catalog_rows}
    for parent_route_id, child_route_ids in route_family_children.items():
        family_route_ids = (parent_route_id, *child_route_ids)
        if len(set(family_route_ids)) != len(family_route_ids):
            raise ValueError("route family parent and children must be distinct")
        missing_route_ids = [
            route_id for route_id in family_route_ids
            if route_id not in catalog_route_ids
        ]
        if missing_route_ids:
            raise ValueError(
                "route family references unknown catalog route ID(s): "
                + ", ".join(missing_route_ids)
            )
        require_route_family_registries(
            catalog_rows,
            parent_route_id,
            child_route_ids,
        )

def require_route_family_registries(
    catalog_rows,
    parent_route_id,
    child_route_ids,
):
    parent_route = resolve_catalog_route(catalog_rows, parent_route_id)
    if (
        "target_registry" not in parent_route
        or parent_route["target_registry"] is not None
    ):
        raise ValueError("route family parent must be registry-unspecified")
    child_pairs = set()
    for child_route_id in child_route_ids:
        child_route = resolve_catalog_route(catalog_rows, child_route_id)
        child_registry = child_route.get("target_registry")
        if not isinstance(child_registry, str) or not child_registry:
            raise ValueError("route family child must have a concrete registry")
        projection_contexts = child_route.get("projection_contexts")
        if not isinstance(projection_contexts, (tuple, list)) or not projection_contexts:
            raise ValueError("route family child must have projection context(s)")
        for projection_context in projection_contexts:
            child_pair = (child_registry, projection_context)
            if child_pair in child_pairs:
                raise ValueError(
                    "route family children must use distinct registry/context pairs"
                )
            child_pairs.add(child_pair)

def require_open_vsx_route(catalog_rows, route_id, projection_context):
    route = require_catalog_projection(catalog_rows, route_id, projection_context)
    if route["target_registry"] != "open_vsx":
        raise ValueError("route is not compatible with the Open VSX registry")
    return route

def require_claim_disposition(row):
    expected = CLAIM_DISPOSITION_RULES.get(row["id"])
    if expected is not None:
        if any(row.get(key) != value for key, value in expected.items()):
            raise ValueError(f"{row['id']}: incompatible claim disposition")
    elif row["id"] in EXPECTED_CLAIMS:
        if row.get("kind") != "project":
            raise ValueError(f"{row['id']}: route claim must be projected")
    elif row["id"] in EXPECTED_FINDINGS:
        expected = FINDING_DISPOSITION_RULES[row["id"]]
        if isinstance(expected, tuple):
            if row.get("kind") != "project":
                raise ValueError(f"{row['id']}: route finding must be projected")
        elif any(row.get(key) != value for key, value in expected.items()):
            raise ValueError(f"{row['id']}: incompatible finding disposition")
    else:
        raise ValueError(f"{row['id']}: unknown disposition subject")

def require_claim_route_binding(row):
    expected = CLAIM_ROUTE_BINDINGS.get(row["id"])
    if expected is None or row["id"] == "C202":
        return
    actual = {
        "exact_catalog_route_id": row.get("exact_catalog_route_id"),
        "exact_projection_context": row.get("exact_projection_context"),
    }
    if actual not in (expected if isinstance(expected, tuple) else (expected,)):
        raise ValueError(f"{row['id']}: claim-to-route/context binding mismatch")

def require_finding_route_binding(row):
    expected = FINDING_DISPOSITION_RULES.get(row["id"])
    if expected is None or (not isinstance(expected, tuple)
                           and expected["kind"] != "project"):
        return
    actual = {
        "exact_catalog_route_id": row.get("exact_catalog_route_id"),
        "exact_projection_context": row.get("exact_projection_context"),
    }
    expected_bindings = expected if isinstance(expected, tuple) else (expected,)
    if actual not in ({
        "exact_catalog_route_id": binding["exact_catalog_route_id"],
        "exact_projection_context": binding["exact_projection_context"],
    } for binding in expected_bindings):
        raise ValueError(f"{row['id']}: finding-to-route/context binding mismatch")

def require_true(condition, message):
    if not condition:
        raise AssertionError(message)

def assert_rejected(catalog_rows, route_id, projection_context):
    try:
        require_open_vsx_route(catalog_rows, route_id, projection_context)
    except ValueError:
        return True
    raise AssertionError("incompatible catalog route was accepted")

def assert_disposition_rejected(row):
    try:
        require_claim_disposition(row)
    except ValueError:
        return True
    raise AssertionError(f"{row['id']}: incompatible claim disposition was accepted")

def fixture_ledger():
    rows = []
    for item_id in EXPECTED_CLAIMS:
        if item_id in REQUIRED_EXCLUSION_IDS:
            rows.append({"id": item_id, **CLAIM_DISPOSITION_RULES[item_id]})
        else:
            rows.extend({"id": item_id, "kind": "project", **projection}
                        for projection in projection_records(item_id))
    for number in range(1, 13):
        finding_id = f"FND-{number}"
        rows.extend({"id": finding_id, **projection}
                    for projection in projection_records(finding_id))
    return rows

def validate_closure(inventory_rows, ledger_rows, catalog_rows):
    expected = set(EXPECTED_CLAIMS + EXPECTED_FINDINGS)
    inventory_ids = [row["id"] for row in inventory_rows]
    ledger_ids = [row["id"] for row in ledger_rows]
    require_route_family_bindings(catalog_rows)
    if len(EXPECTED_CLAIMS) != 70 or len(set(EXPECTED_CLAIMS)) != 70:
        raise ValueError("invalid 70-claim manifest")
    if len(EXPECTED_FINDINGS) != 12 or len(set(EXPECTED_FINDINGS)) != 12:
        raise ValueError("invalid 12-finding manifest")
    if Counter(inventory_ids) != Counter(EXPECTED_CLAIMS):
        raise ValueError("missing, duplicate, or unknown inventory claim ID")
    expected_counts = Counter({item_id: 1 for item_id in expected})
    expected_counts["C202"] = 4
    for item_id, route_names in MULTI_ROUTE_PROJECTIONS.items():
        expected_counts[item_id] = len(route_names)
    expected_counts["FND-3"] = len(FINDING_DISPOSITION_RULES["FND-3"])
    if Counter(ledger_ids) != expected_counts:
        raise ValueError("missing, duplicate, or unknown ledger ID")
    for row in ledger_rows:
        require_claim_disposition(row)
        if row["kind"] == "project":
            if not row.get("exact_catalog_route_id") or not row.get("exact_projection_context"):
                raise ValueError(f"{row['id']}: incomplete projection")
            require_catalog_projection(
                catalog_rows,
                row["exact_catalog_route_id"],
                row["exact_projection_context"],
            )
            require_claim_route_binding(row)
            require_finding_route_binding(row)
        elif row["kind"] == "constrain":
            if not row.get("route_independent") or not row.get("exact_reason"):
                raise ValueError(f"{row['id']}: incomplete constraint")
        elif row["kind"] == "exclude":
            if row.get("reason") not in ALLOWED_EXCLUSIONS or not row.get("rationale"):
                raise ValueError(f"{row['id']}: incompatible exclusion")
        else:
            raise ValueError(f"{row['id']}: invalid disposition")
    c202_rows = [row for row in ledger_rows if row["id"] == "C202"]
    if [(row["exact_catalog_route_id"].removeprefix("fixture-route-"),
         row["exact_projection_context"]) for row in c202_rows] != C202_PROJECTIONS:
        raise ValueError("C202 requires four distinct ordered route/context tuples")
    if len({row["exact_catalog_route_id"] for row in c202_rows}) != 4:
        raise ValueError("C202 route IDs must not be reused")
    if [
        (row["exact_catalog_route_id"], row["exact_projection_context"])
        for row in c202_rows
    ] != [
        (binding["exact_catalog_route_id"], binding["exact_projection_context"])
        for binding in CLAIM_ROUTE_BINDINGS["C202"]
    ]:
        raise ValueError("C202 claim-to-route/context bindings changed")
    c1208_rows = [row for row in ledger_rows if row["id"] == "C1208"]
    if len(c1208_rows) != 1:
        raise ValueError("C1208 must remain a single Open VSX projection")
    require_open_vsx_route(
        catalog_rows,
        c1208_rows[0]["exact_catalog_route_id"],
        c1208_rows[0]["exact_projection_context"],
    )
    catalog_projections = [
        (row["route_id"], projection_context)
        for row in catalog_rows
        for projection_context in row["projection_contexts"]
    ]
    referenced_projections = [
        (row["exact_catalog_route_id"], row["exact_projection_context"])
        for row in ledger_rows
        if row["kind"] == "project"
    ]
    if set(referenced_projections) != set(catalog_projections):
        raise ValueError("catalog contains an unreferenced route/context")
    return True

def assert_closure_rejected(catalog_rows):
    try:
        validate_closure(
            [{"id": item_id} for item_id in EXPECTED_CLAIMS],
            fixture_ledger(),
            catalog_rows,
        )
    except ValueError:
        return True
    raise AssertionError("unreferenced catalog row was accepted")

def assert_route_family_rejected(catalog_rows, route_family_children):
    try:
        require_route_family_bindings(catalog_rows, route_family_children)
    except ValueError:
        return True
    raise AssertionError("invalid route family declaration was accepted")

def assert_route_family_accepted(catalog_rows, route_family_children):
    try:
        require_route_family_bindings(catalog_rows, route_family_children)
    except ValueError:
        raise AssertionError("valid route family declaration was rejected")
    return True

def assert_inventory_rejected(inventory_rows):
    try:
        validate_closure(inventory_rows, fixture_ledger(), FIXTURE_CATALOG)
    except ValueError:
        return True
    raise AssertionError("changed inventory was accepted")

require_true(validate_closure(
    checked_in_inventory_rows(),
    fixture_ledger(),
    FIXTURE_CATALOG,
), "fixture closure was not validated")
INVENTORY_TEXT = INVENTORY_PATH.read_text(encoding="utf-8")
CHANGED_INVENTORY_TEXT = INVENTORY_TEXT.replace("| C101 |", "| C999 |", 1)
require_true(
    assert_inventory_rejected(inventory_rows_from_markdown(CHANGED_INVENTORY_TEXT)),
    "changed inventory claim ID was accepted",
)
DUPLICATED_INVENTORY_TEXT = INVENTORY_TEXT.replace("| C101 |", "| C102 |", 1)
require_true(
    assert_inventory_rejected(inventory_rows_from_markdown(DUPLICATED_INVENTORY_TEXT)),
    "duplicated inventory claim ID was accepted",
)
UNREFERENCED_CONTEXT_CATALOG = tuple(
    {
        **row,
        "projection_contexts": row["projection_contexts"] + ("unused-context",),
    } if row["route_id"] == "r_4f8c2a" else row
    for row in FIXTURE_CATALOG
)
require_true(
    assert_closure_rejected(UNREFERENCED_CONTEXT_CATALOG),
    "unreferenced projection context was accepted",
)
UNREFERENCED_CATALOG = FIXTURE_CATALOG + ({
    "route_id": "r_unreferenced",
    "target_registry": "fixture_registry",
    "projection_contexts": ("fixture-context-unreferenced",),
},)
require_true(assert_closure_rejected(UNREFERENCED_CATALOG), "unreferenced catalog row was accepted")
MISSING_FAMILY_PARENT_CATALOG = tuple(
    row for row in FIXTURE_CATALOG
    if row["route_id"] != "fixture-route-VS_Code_extension"
)
require_true(
    assert_closure_rejected(MISSING_FAMILY_PARENT_CATALOG),
    "route family with a missing parent was accepted",
)
MISSING_FAMILY_CHILD_DECLARATION = {
    "fixture-route-VS_Code_extension": (
        "fixture-route-C1202-absent",
        "fixture-route-C1202-open-vsx",
        "r_4f8c2a",
    ),
}
require_true(
    assert_route_family_rejected(
        FIXTURE_CATALOG,
        MISSING_FAMILY_CHILD_DECLARATION,
    ),
    "missing route family child declaration was accepted",
)
NON_DISTINCT_FAMILY_DECLARATION = {
    "fixture-route-VS_Code_extension": (
        "fixture-route-VS_Code_extension",
        "fixture-route-C1202-open-vsx",
        "r_4f8c2a",
    ),
}
require_true(
    assert_route_family_rejected(
        FIXTURE_CATALOG,
        NON_DISTINCT_FAMILY_DECLARATION,
    ),
    "non-distinct route family declaration was accepted",
)
REGISTRY_SPECIFIC_PARENT_CATALOG = tuple(
    {
        **row,
        "target_registry": "fixture_parent_registry",
    } if row["route_id"] == "fixture-route-VS_Code_extension" else row
    for row in FIXTURE_CATALOG
)
require_true(
    assert_route_family_rejected(
        REGISTRY_SPECIFIC_PARENT_CATALOG,
        ROUTE_FAMILY_CHILDREN,
    ),
    "registry-specific family parent was accepted",
)
REGISTRY_UNSPECIFIED_CHILD_CATALOG = tuple(
    {
        **row,
        "target_registry": None,
    } if row["route_id"] == "fixture-route-C1202-marketplace" else row
    for row in FIXTURE_CATALOG
)
require_true(
    assert_route_family_rejected(
        REGISTRY_UNSPECIFIED_CHILD_CATALOG,
        ROUTE_FAMILY_CHILDREN,
    ),
    "registry-unspecified family child was accepted",
)
COLLIDING_FAMILY_CHILD_CATALOG = tuple(
    {
        **row,
        "target_registry": "open_vsx",
        "projection_contexts": ("fixture-context-C1202-open-vsx",),
    } if row["route_id"] == "fixture-route-C1202-marketplace" else row
    for row in FIXTURE_CATALOG
)
require_true(
    assert_route_family_rejected(
        COLLIDING_FAMILY_CHILD_CATALOG,
        ROUTE_FAMILY_CHILDREN,
    ),
    "family children sharing a registry and context were accepted",
)
require_true(
    assert_route_family_accepted(FIXTURE_CATALOG, ROUTE_FAMILY_CHILDREN),
    "family children sharing open_vsx with distinct contexts were rejected",
)
require_true(assert_rejected(
    WRONG_REGISTRY_CATALOG,
    "r_4f8c2a",
    "Open_VSX_compatible_marketplace_context",
), "wrong registry was accepted")
require_true(assert_rejected(FIXTURE_CATALOG, "r_unknown", "Open_VSX_compatible_marketplace_context"), "unknown route was accepted")
require_true(assert_rejected(DUPLICATE_ID_CATALOG, "r_4f8c2a", "Open_VSX_compatible_marketplace_context"), "duplicate route was accepted")
require_true(assert_rejected(FIXTURE_CATALOG, "r_4f8c2a", "VS_Marketplace_compatible_marketplace_context"), "wrong context was accepted")
for item_id in REQUIRED_EXCLUSION_IDS:
    incompatible = {"id": item_id, "kind": "project"}
    require_true(assert_disposition_rejected(incompatible), "incompatible disposition was accepted")

def assert_ledger_rejected(ledger_rows):
    try:
        validate_closure(
            [{"id": item_id} for item_id in EXPECTED_CLAIMS],
            ledger_rows,
            FIXTURE_CATALOG,
        )
    except ValueError:
        return True
    raise AssertionError("tampered ledger was accepted")

SWAPPED_CLAIM_BINDINGS = [
    {
        **row,
        **(
            {
                "exact_catalog_route_id": "fixture-route-C102",
                "exact_projection_context": "fixture-context-C102",
            }
            if row["id"] == "C101" else {
                "exact_catalog_route_id": "fixture-route-C101",
                "exact_projection_context": "fixture-context-C101",
            }
            if row["id"] == "C102" else {}
        ),
    }
    for row in fixture_ledger()
]
require_true(
    assert_ledger_rejected(SWAPPED_CLAIM_BINDINGS),
    "swapped claim route/context bindings were accepted",
)

COLLAPSED_ROUTE_FAMILY = [
    {
        **row,
        "exact_catalog_route_id": "fixture-route-VS_Code_extension",
    }
    if row["id"] == "C1202"
    and row["exact_catalog_route_id"] == "fixture-route-C1202-marketplace"
    else row
    for row in fixture_ledger()
]
require_true(
    assert_ledger_rejected(COLLAPSED_ROUTE_FAMILY),
    "registry-specific child route was collapsed onto the family route ID",
)

ROUTE_CLAIM_EXCLUDED = [
    {
        **row,
        "kind": "exclude",
        "reason": "non_install_dependency",
        "rationale": "tamper control",
    }
    if row["id"] == "C102" else row
    for row in fixture_ledger()
]
require_true(
    assert_ledger_rejected(ROUTE_CLAIM_EXCLUDED),
    "route-relevant claim exclusion was accepted",
)

FINDING_DISPOSITION_TAMPER = [
    {
        **row,
        "kind": "exclude",
        "reason": "non_install_dependency",
        "rationale": "tamper control",
    }
    if row["id"] == "FND-4" else row
    for row in fixture_ledger()
]
require_true(
    assert_ledger_rejected(FINDING_DISPOSITION_TAMPER),
    "finding-specific disposition tamper was accepted",
)
```

For family validation, each child row contributes one
`(target_registry, projection_context)` pair per existing projection context;
multiple contexts are valid when none of those pairs overlap another child.

This closure fixture is executed manually against this document and is not wired
to a repository check in this planning PR. Binding it to an owning package's
test is an implementation-lane obligation; until then, fixture drift is caught
only by re-running it.

The fixture reads `docs/distribution/INSTALL_CLAIM_SURFACES.md` and rejects missing,
duplicate, unknown, and incompatible IDs before
acceptance (the production harness should collect those errors rather than stop at the
first one). It validates every literal claim and finding ID individually, including
all 93 ledger records and the multi-route claims (including C202's four records), rather than trusting a count or range
shorthand. Thus the complete 70-claim / 12-finding closure check is executable
before route classification exists; only exact catalog route IDs and projection
contexts remain gated on #10334's catalog and #10333's contract. A prose manifest or count without this
set-equality and compatibility check does not satisfy closure. Multiple ledger
facts may support one catalog projection, but every catalog projection must be
referenced at least once. Every project row
also resolves its exact route ID and projection context through the fixture catalog.
The claim-to-disposition rules independently require every explicit out-of-scope
row (C106, C108, C201, C216, C703, C801, C1001, C1002, C1008, C1102, C1201,
C1205, and C1309) to remain its declared exclusion; a project or other incompatible
exclusion disposition is rejected for each. The reverse catalog check compares exact
`(route_id, projection_context)` pairs as sets and rejects any unreferenced
route/context pair, including an extra context on an otherwise known route;
many-to-one references are valid because several claims or findings can support
the same catalog projection.
The per-ID route-binding check also rejects a complete C101/C102 binding swap, and
the expected finding disposition map rejects moving route-relevant FND-4 to an
allowed exclusion. These are deliberate tamper controls: they fail under both
normal Python execution and `python -O` because validation uses explicit exceptions,
not assertion statements.
The C1208 assertion
also proves that its opaque ID resolves to exactly one catalog row whose registry is
Open VSX and whose projection context is compatible; an ID that resolves to the
Marketplace row, an unknown ID, a duplicate ID, or an incompatible context fails
the fixture. These controls exercise the future catalog contract shape only: the
validated production catalog is not present in this planning PR, so binding this
fixture to #10334's eventual catalog under #10333's contract remains `NOT_PROVEN`
until those authorities land.

### 2.2 Classification = conjunction of independent per-dimension verdicts

For each exact catalog route row and projection context, classification is the
**AND-join over all seven required hard dimensions**. Never reduce to a scalar
or infer a route from prose claim count. The seven dimensions below are the
authoritative aggregation; a missing or unjoined required dimension yields
`unproven`, and no dimension may be silently absorbed into another:

1. **Identity and topology** — product units yielded (`perllsp` / `perl-dap` /
   `extension`) + identity names bound, honoring the collision map
   (`perl-lsp` → rejected-foreign). Source: `product_units` dimensions (C208,
   C209, C210) + C203/C1101.
2. **Platform and target** — per-OS/arch coverage with the **three-way**
   `windows_arm64` record (`user_prose`, `tracked_source`,
   `published_receipt`) kept separate exactly as the validated route catalog
   must model it. Effective support = **receipt-bound**: `tracked_source=built` does NOT
   yield `supported` while `published_receipt=absent` (FND-4 disposition owned
   here).
3. **Product-unit and lifecycle completeness** — server/adapter membership,
   installation, first-use, repair, upgrade, rollback, and removal cells do
   not compensate for one another.
4. **Integrity and provenance** — `sha256sums_enforcement` mode and independent
   provenance/checksum identity. A contradiction inside
   the conjunction (C207 fail-closed vs C1005 fail-open on the same
   `scripts/install.sh`) resolves **pessimistically to `contradicted`**, never
   to the optimistic value, until `distribution-docs-sync` lands FND-7.
5. **Freshness, channel, and publication** — drift, currentness, public
   publication, and public verification remain separate. Map every inventory drift
   status deterministically to the closed dimension verdict set:

   | Drift status | Dimension result |
   | --- | --- |
   | `current` | `proven_current` only if independent publication and verification also pass |
   | `pending` | `pending_gate(issue)` |
   | `stale_example`, `future_example`, `mutable_pin`, `cross_surface_drift`, `source_drift` | `unproven` |
   | `volatile_number` | no downgrade; retain as inert metadata and report FND-8 separately |

   For multiple joined statuses, remove the inert status and apply the fixed
   order `pending_gate` > `unproven` > `proven_current`; catalog or input order
   must not affect the result. A `current` row never upgrades a route whose
   publication or verification predicate fails.
6. **PATH, session, and execution** — fresh-process resolution, exact host
   lookup, transport, cleanup, and process settlement are explicit dimensions.
7. **Receipt axis** — channel-independence: each route inherits exactly the
   receipt channel(s) its catalog row cites; no cross-channel inference (C201/C703).

The result retains a fixed-order vector of seven dimension verdicts, followed by
an overall summary. The vector is authoritative when failures overlap; an overall
summary must never erase a failing dimension:

```text
route_verdict {
  identity_topology,
  platform_target,
  product_unit_lifecycle,
  integrity_provenance,
  freshness_channel_publication,
  path_session_execution,
  receipt,
  overall,
}
```

Each dimension uses `proven_current`, `receipt_bound_partial`, `pending_gate`,
`contradicted`, or `unproven`. `not_recommended` is a selection annotation, not
a replacement for a dimension result. For deterministic summaries, compute
`overall` from the complete vector using this fixed severity order:
`contradicted` > `pending_gate` > `unproven` > `receipt_bound_partial` >
`proven_current`; ties retain all dimension names, claim IDs, findings, and
values in numeric dimension order. Thus a route can expose both
`integrity=contradicted{C207,C1005,FND-7}` and
`product_unit_lifecycle=unproven`; the summary is `contradicted` but the
unproven lifecycle failure remains machine-visible. No first/last claim or
single scalar may select the winning failure.

### 2.3 Where it lives (artifact + generator shape)

Follow the validated route-catalog generator pattern — deterministic, generated,
schema-closed:

- Extend the repository's eventual classification generator (the command,
  artifact filenames, schema path, and API are deliberately deferred to the
  validated contract) so it **consumes the exact validated route schema/catalog
  composed by #10334 under the validated contract published by #10333**
  (input digest and producer revisions recorded) plus a
  small curated **route-join table** (`policy/install-route-join.toml` or a
  static map in the generator). The join representation must be selected only
  after #10333 publishes its accepted schema contract and #10334 publishes the
  catalog; this plan does not name or
  imply a v2 filename, module, schema, or generator API.
- Emit the classification artifact and closed schema required by that accepted
  contract, with a regenerate-and-compare byte-identity check wired beside the
  catalog's own validation gate.
- Classification (§2.2) is **pure derivation** — it must be mechanical and
  testable. The only curated inputs are: route→claim join, the anti-claim
  identity map, and the pessimistic-contradiction rule. **Preference ordering
  is NOT derived here** (§3).
- Composition note: #10334 composes and fans in the catalog under #10333's
  schema/validation authority; #10333 excludes route population. #11434
  supplies denominator closure, #11432 supplies its named evidence producer,
  #11548 is the catalog assembly outcome, and #11443 owns cross-cutting E07
  closure. Neither issue supplies route-selection authority.
  Do not build against the closed #12858
  branch or any artifact/API from that attempt. Re-derive exact route joins, projection contexts, and falsifier
  fixtures if the validated input contract changes.

---

## 3. Preferred-route selection: algorithm options and tradeoffs

Selection input is an exact context tuple:
`(editor_family, editor_identity, target_registry, os, os_version, arch,
target_triple, libc, platform_capabilities, desired_product_units, context,
risk_posture)`, where
`editor_family` distinguishes VS Code-compatible managed clients from generic
LSP clients (Emacs, Neovim, Helix, Sublime, and other clients),
`target_registry` distinguishes VS Marketplace from Open VSX (C1202 and C1208
are never one receipt or one selectable route),
`target_triple`/`libc` distinguishes GNU from musl projections,
`platform_capabilities` includes Windows ARM emulation capability/version, and
`risk_posture` is either `strict` or `permissive`.

`strict` has `selected_count=1` only when exactly one `proven_current` route
survives the hard filter. Zero candidates returns `no_route` with sorted reasons,
and more than one exact eligible row is an ambiguity refusal; strict never chooses
by input order. `permissive` first uses the same hard filter and partitions the
eligible routes into `P` (all dimensions `proven_current`) and `Q` (all dimensions
either `proven_current` or `receipt_bound_partial`, with at least one
`receipt_bound_partial` dimension and the required explicit integrity/provenance,
product-unit/lifecycle, and freshness/channel/publication receipts). `P` and `Q`
are disjoint by construction; equivalently, `Q` is the eligible-route complement
of `P`.

The permissive result has closed cardinality. If `|P| > 0`, the candidate set is
exactly all `|P|` proven-current routes, each returned once; every `Q` route is
returned exactly once as a partial diagnostic but is not selectable. If `|P| = 0` and
`|Q| > 0`, the candidate set is exactly all `|Q|` partial routes, each returned
once and labeled `partial`. If both are empty, the result is `no_route`. In either
non-empty case, `selected_count=1` exactly when the candidate set has cardinality
one; when its cardinality is greater than one, `selected_count=0` and the result
uses `selection=deferred_human_order`. A partial route is never selected while a
proven-current route exists, and no route with `pending_gate`, `unproven`, or
`contradicted` is in either candidate set.

Every returned candidate uses this ascending total-order key:
`(candidate_class, integrity, lifecycle, publication, identity_topology,
platform_target, path_session_execution, receipt, exact_catalog_route_id,
exact_projection_context)`. The seven dimension fields use the closed ranks
`proven_current=0` and `receipt_bound_partial=1` for this eligible set;
`candidate_class` is `proven_current=0`, `partial=1`. The final two fields compare
the UTF-8 bytes of the opaque route ID and exact projection context. The catalog
rejects duplicate route-ID/context pairs, so this key is total and cannot depend
on catalog or input order. This is mechanical candidate ordering, not preferred
product authority; while H1–H7 remain pending, even a sole candidate is
`provisional(human-pending)` and the ordered set is not a recommendation. Once
H1–H7 are ruled, the applicable human-authored policy may choose from this set.
Duplicate exact route/context rows are a catalog error and return `NOT_PROVEN`
with the duplicate IDs. Unknown context or registry fields return `no_route`
with a sorted reason; they never fall back to another registry, `latest`, or an
unpinned command. Pending diagnostics are sorted by issue ID and all remaining
routes are omitted with reasons. These outputs are mechanical and non-operative
while H1–H7 remain human-pending; H7(c) can add only a verify-first,
not-selectable-until-verified diagnostic for deferred channels.

This section defines the selection contract but does not claim executable
route-classification proof in this planning PR. The future classifier harness must
bind the cardinality cases (`|P|`/`|Q|` equal to zero, one, and multiple), shuffle
candidate input, include Unicode route IDs and projection contexts, and include
duplicate route-ID/context controls before claiming the UTF-8 order or cardinality
rules are proven. Until that harness runs against the validated #10334 catalog
under #10333's contract,
those runtime claims remain `NOT_PROVEN`; the rules above are acceptance criteria,
not observed production behavior.

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
Survivors are ordered only by a **small explicit context policy table** whose
entries are authored after the applicable H1-H7 ruling. C202 supplies four
projection contexts (VS Code-compatible managed client, published manual archive,
generic/other-editor client, and local Cargo/source build), but supplies no order;
its prose enumeration is not a convenience-first default. CI context likewise
requires an explicit ruling and policy entry; S03's pinned-action wording cannot
seed a recommendation by itself. Before the rulings, Option C may emit only the
mechanical verdict diagnostics defined above and no preferred route.

- **Pros:** the filter is provable and conjunctive (matches the issue title);
  curated data stays small (one order per context, not per platform×units);
  consistent with existing doc behavior — the classifier *formalizes* what
  INSTALLATION.md already teaches rather than inventing policy.
- **Cons:** still requires EXPLICIT-HUMAN rows (§5) for orderings prose does
  not settle; two moving parts (filter + table).

**Design recommendation: Option C, pending H1–H7.** Option B is rejected because
preference ordering is product authority and must remain reviewable data, not
emergent constants. This recommendation does not authorize implementation of a
preferred-route policy or emission of recommendations before all seven rulings.

---

## 4. Discriminating falsifiers

Each falsifier distinguishes a correct classifier/selector from a plausible
wrong one. All are cheap (pure functions over catalog + table; no network,
no installs). Claim IDs and route names below are examples carried forward
from the #11575 inventory; once #10333's schema is validated, #10334's catalog
composition/fan-in is complete, and #11434 closes the denominator, each fixture must be
rebound to the exact catalog row, route ID, and projection context rather than
assuming the former prose-row denominator.

1. **Receipt-binding (FND-4, routed from the landed inventory to this
   classifier).**
   Query `(windows, aarch64, {perllsp}, editor)`. Wrong implementations join
   `tracked_source=built` (C405/C501) or `user_prose=x64_fallback_build_from_source`
   (C1204) into "archive download supported". Correct output: **no
   receipt-backed native route** while `published_receipt_v0_17_0=absent`;
   either a refusal citing FND-4 or an explicit x64-fallback/build-from-source
   recommendation labeled receipt-bound. The fixture must include Windows version
   and emulation capability; Windows 10 ARM cannot use the x64 fallback, while
   Windows 11 ARM may do so when the capability is observed. A classifier that prints "supported"
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
   may list `manual-archive` first. Under H2(a), `powershell-installer` must
   appear as `pending_gate(#5461/#4348)`/`not_recommended`; under H2(b), its
   intentional omission must be recorded as the ruled policy. An implementation
   that treats either ruling as the other fails.
7. **Drift-status fixture semantics.** The fixture harness must construct one
   otherwise-identical exact route/context row for each status and assert the
   following result without relying on prose parsing or a scalar quality score:

   | Input `drift_status` | Required freshness result | Required additional assertion |
   | --- | --- | --- |
   | `current` | `proven_current` candidate | publication and independent verification are still checked separately |
   | `pending` | `pending_gate(issue)` | route is diagnostic-only and never eligible |
   | `stale_example` | `unproven` | current competitors are not downgraded by input order |
   | `future_example` | `unproven` | future prose cannot satisfy currentness |
   | `mutable_pin` | `unproven` | selection may annotate/demote but cannot hide the failure |
   | `cross_surface_drift` | `unproven` | another surface/channel cannot repair the row |
   | `source_drift` | `unproven` | tracked-source disagreement cannot be treated as current |
   | `volatile_number` | `proven_current` candidate with inert metadata | volatility is retained and FND-8 is reported; it is neither a downgrade nor an upgrade |

   The harness must run two-row permutations for every status, including
   `volatile_number`, and assert that input order does not change the fixed
   result (`pending_gate` > `unproven` > `proven_current`). Removing the inert
   status must leave the same result. A fixture that only lists the eight names,
   or treats `volatile_number` as `unproven`, is incomplete.
8. **Multi-route projection preservation.** The closure fixture must emit every
   declared projection for multi-route claims and findings. C202 must emit exactly four
   ordered rows for C202:
   `(VS_Code_extension, VS_Code_compatible_managed_client)`,
   `(manual_archive, macOS_or_Linux_or_Windows_manual_archive)`,
   `(other_editor_download, generic_LSP_client)`, and
   `(local_cargo, local_testing_or_prerelease_validation)`. A fixture that emits
   only the VS Code row, merges contexts, substitutes an opaque catalog ID before
   #10334 publishes it under #10333's contract, or changes this source order fails. This checks preservation
   of applicable projections; this order is deterministic serialization/source order
   only, and does not define route-selection precedence or a preferred route while
   H1–H7 remain human-pending.
9. **Determinism.** The eventual catalog-owner regeneration check must produce
   byte-identical classification output across repeated runs and supported
   environments. The output contains no timestamps or other ambient state, and
   catalog ordering is normalized rather than observed; any run-to-run diff
   fails. The concrete command and owning package remain deferred until the
   validated catalog contract selects them.
10. **Denominator and inventory closure.** Every exact route row and projection
   context in the validated catalog is classified exactly once, and every one of
   the literal claim IDs C101, C102, C103, C104, C105, C106, C107, C108, C201,
   C202, C203, C204, C205, C206, C207, C208, C209, C210, C211, C212, C213,
   C214, C215, C216, C301, C302, C303, C401, C402, C403, C404, C405, C406,
   C501, C502, C503, C601, C701, C702, C703, C801, C901, C902, C1001, C1002,
   C1003, C1004, C1005, C1006, C1007, C1008, C1101, C1102, C1201, C1202,
   C1203, C1204, C1205, C1206, C1207, C1208, C1301, C1302, C1303, C1304,
   C1305, C1306, C1307, C1308, and C1309, plus literal findings FND-1,
   FND-2, FND-3, FND-4, FND-5, FND-6, FND-7, FND-8, FND-9, FND-10, FND-11,
   and FND-12, has exactly one ledger disposition. A join referencing an
   unknown ID, duplicate disposition, missing ID, range shorthand, or an
   unjoined row fails the check. The 70 prose claim rows from #11575 are the
   closed audit denominator; they do not define the classifier's route
   denominator by themselves. That manifest is a provisional prose-audit
   derivation under §0's OPEN-UNRESOLVED dependency state, not an accepted
   v2 input contract; it is re-derived at the §0 wake event and rebound to
   the validated catalog rows before any implementation lane starts.
11. **Cross-channel inference block (C201/C703).** A claim's receipt on channel
   X must never satisfy another route's receipt requirement (e.g., GitHub
   Releases v0.17.0 receipt must not make `homebrew-tap` `proven_current`).
   An implementation with a global "release exists" fact fails.
12. **Independent checksum/provenance binding.** A route with matching
    `SHA256SUMS` text but no independently bound artifact and release identity
    must remain `unproven`; a checksum string copied from a different channel
    must not satisfy the integrity/provenance axis.
13. **Candidate versus installed state.** A candidate artifact that has been
    built or uploaded but has no installed, verified product-unit observation
    must not satisfy installation or first-use cells. Conversely, an installed
    local build must not be emitted as a public publication receipt.
14. **PATH/session/execution isolation.** A route that resolves only through
    the current shell's PATH, an inherited session, or an ambient working
    directory must remain unproven for a fresh-process route. A fresh lookup,
    transport, cleanup, and settled process must each be present; one cannot
    stand in for the others.
15. **Lifecycle closure.** A route with install and first-use evidence but no
    repair, upgrade, rollback, or removal cell remains incomplete. A lifecycle
    cell from another product unit or channel must not close this route.
16. **Publication and verification separation.** A private/candidate upload or
    an unverified public listing must not become `proven_current`; publication,
    checksum/provenance verification, and currentness are separate predicates.
17. **No-route and ambiguity closure.** If every route fails a hard dimension,
    output must be an explicit no-route result with reasons. If two exact rows
    or contexts are ambiguous, selection must refuse rather than choose by
    input order, prose frequency, or a fallback command.
18. **Context and fallback isolation.** An editor route must not satisfy a CI,
    server-only, or manual context without an explicit catalog projection.
    Missing preferred policy or a failed hard filter must not silently fall back
    to `latest`, an unpinned command, or another context's route.
19. **Composite simultaneous failures.** A fixture with integrity contradiction
    and incomplete lifecycle must retain both dimension verdicts in the fixed
    vector and produce the same `overall=contradicted` summary regardless of
    catalog or claim input order. A scalar-only result, or a result that drops
    the lifecycle failure, fails.
20. **Selection-context and risk isolation.** Identical requests differing only
    in editor family, target registry, target/libc, observed Windows emulation
    capability, or `strict` versus `permissive` must produce the corresponding
    observable result: strict selects only one proven-current projection (or
    `no_route` with reasons); permissive returns exactly `|P|` proven-current
    candidates when `|P| > 0`, otherwise exactly `|Q|` explicitly annotated
    receipt-bound partial candidates when `|Q| > 0`, otherwise `no_route`, using
    the closed total order in §3. It selects exactly one only when the applicable
    candidate set has cardinality one; multiple candidates return
    `selection=deferred_human_order` with `selected_count=0`. A route that ignores
    any supplied field fails; permissive must not turn a contradiction, unproven
    route, or H7 verify-first diagnostic into a selection.
21. **Managed-client registry-family isolation.** A VS Code managed-client
   request that supplies `target_registry` must return exactly the matching
   registry-specific route and never the registry-unspecified family row. The
   same request with `target_registry` unspecified must return the family row,
   with the registry-specific rows present only as diagnostics. A result that
   returns both a family row and one of its registry-specific rows, or that
   treats the family row as registry-specific, fails.

---

## 5. EXPLICIT-HUMAN decision rows

Preference ordering is **product authority, not derivable** from source or
inventory. These rows cannot be resolved by the implementer; each needs a
maintainer ruling in issue #11549 (an issue comment suffices). The listed options
are proposals for that ruling, not defaults. If a row is unanswered, its state is
`human-pending`: no ordering, recommendation, or operative selection policy may
be emitted.

| # | Decision | Options | Unanswered state | Consequence of getting it wrong |
| --- | --- | --- | --- | --- |
| H1 | Global ordering principle: convenience-first (C202 teaches extension → archive → …) vs integrity-first (identity-bound verify > ease) | (a) convenience-first per C202; (b) integrity-first reorder | `human-pending`; no ordering | Classifier formalizes guidance that contradicts maintainer intent across every context |
| H2 | Windows x86_64 policy while published PowerShell installer is broken (#5461/#4348) | (a) manual-archive first, powershell shown as `pending_gate`; (b) omit powershell entirely; (c) surface scoop/choco/winget as verify-first diagnostics, never selectable | `human-pending`; no route recommendation | Users routed to a 404 installer, or gate visibility lost (FND-9's four-site spread re-grows) |
| H3 | Windows ARM64 until an ARM64 release ships (FND-4 — routed to #11549 by name) | (a) recommend x64-fallback + build-from-source labeled receipt-bound; (b) refuse with "no receipt-backed route"; (c) follow prose (contradicts receipts) | `human-pending`; no route recommendation | Either false "supported" claim or an unnecessarily barren answer; FND-4 disposition must be defensible |
| H4 | macOS server-only ranking: homebrew-tap vs cargo-registry (both `current`; tap freshness unproven per C1304/C1305 caveats) | (a) tap first (native UX); (b) cargo-registry first (version receipts); (c) context-split (editor=tap, headless=cargo) | `human-pending`; no ordering | A preferred route whose freshness caveat the docs themselves flag |
| H5 | Unpinned mutability policy: `cargo-git` (FND-6) and `latest` endorsements (FND-3) | (a) classify `not_recommended`, never selected; (b) retain as a diagnostic with warning, never selectable; (c) keep the mechanical §2.2 freshness verdict only and add no policy label | `human-pending`; no selection | Classifier endorses what FND-3 calls a moving target the receipt does not cover |
| H6 | perl-dap (adapter) acquisition policy for non-VS Code users | (a) archive pair route; (b) separate `cargo install --locked perl-dap` (C702); (c) defer — server-only answer, adapter on request | `human-pending`; no ordering | Users get a server without a debugger, or build-from-source surprises (C208) |
| H7 | Unproven channels (`unproven-channels`: scoop/choco/winget C212, Docker) in selection output | (a) never selected, visible in a "unproven" appendix; (b) fully omitted; (c) visible with a verify-first instruction but never selectable | `human-pending`; no selection | Either dead output weight or an implied endorsement the receipts don't back |

H2 and H7 are not independent for unproven Windows channels: H2(c) is
reachable only under an H7(c) ruling, and no H2 ruling may place an unproven
channel in a selectable slot.

*(Count: 7 EXPLICIT-HUMAN rows.)*

No H5 ruling may make a route without a literal pin selectable: §2.2 maps
`mutable_pin` to `unproven`, and §3 excludes every unproven route from both
candidate sets; H5 rules only the diagnostic and labeling treatment.

When issue #11549 explicitly rules H7(c), the approved registry action is exactly
`verify-first`: the registry may list the deferred channel with its opaque catalog
route ID, verification command or receipt type, verification status, and
`not-selectable-until-verified` marker. The action must be evaluated against the
current artifact and channel before selection; a registry listing, a verify-first
instruction, or a successful check on another channel is not publication or
support evidence. H7(c) therefore permits an actionable verification path, never
an implicit route recommendation, and remains unavailable until the ruling is
recorded in the authoritative issue.

### Deferred catalog route-ID compatibility

All route IDs supplied by #10334's catalog are opaque contract values governed by
the #10333 schema. Until those contracts exist, this plan uses semantic projection
names only and must not mint IDs. When the catalog lands, the join and ledger
validator must require every referenced ID
to exist in that catalog, reject unknown IDs and reused IDs, and validate the
catalog version/input digest recorded with the ledger. A catalog migration may
provide an explicit old-to-new compatibility map with one-to-one entries and a
removal date; it may not silently reinterpret an ID, fall back by route name, or
carry a deferred ID forward. Changed IDs or projection contexts invalidate the
affected joins and fixtures and require re-derivation, ending in `NOT_PROVEN` until
the ledger is rebound.

---

## 6. Implementation sizing, title, and next actions

**Suggested conventional title:**
`feat(distribution): add conjunctive install-route classification and preferred-route selection (#11549)`

**Sizing: M after the human-ruling gate** — one candidate writer, one coherent
claim. Roughly: route-join
table + verdict derivation (~250–400 lines incl. static tables, following the
existing catalog-generator idiom), artifact + closed schema +
regen-check wiring (~150–250), selection filter + context policy table
(~100–200), and falsifier tests (§4) (~300). No production-crate changes; no
doc rewrites. **Optional split if review prefers:** slice 1 = classification
artifact (S/M), slice 2 = selection + policy table (S), sharing §2.1's join
table. Do NOT pull FND-7/FND-11 doc syncs or #10342 linting into this claim.

**Hard prerequisite check at build time:** require explicit rulings for H1–H7 in
addition to the validated route
schema/validation contract from #10333 and catalog composition/fan-in from #10334
(including #11548's catalog assembly and #11443's E07 closure), with #11434's canonical
denominator, and the relevant #11432/evidence-producer revisions. Require exact
route IDs, projection contexts, producer joins, publication/channel bindings,
and fail-closed route states. If any authority is absent or structurally
changes, stop at `NOT_PROVEN` and re-derive §2 and its fixtures; do not
substitute the closed #12858 attempt.

**Proof strategy:** all §4 falsifiers as focused cases in the catalog-owner
proof harness, plus the byte-identity regeneration check selected by the
validated catalog contract. The owning package's formatting, lint, and test
 checks, together with `just doctor`, are run once that contract selects the
 concrete surfaces. No CI-cycle dependency beyond the standard gates.

**Residual risks:**

- The #10333 route schema/validation contract or #10334 catalog may change before
  implementation, or #11434 denominator closure may be incomplete → re-derive §2.1,
  hard-dimension bindings, and affected fixtures; the human preference rulings
  remain separate.
- H1–H7 contain proposed options only, not defaults or maintainer rulings. Until
  each is explicitly ruled in the authoritative issue, all seven rows are
  **non-operative**: the plan and any resulting artifact remain
  `provisional(human-pending)` and **non-preferred**. The implementer must not
  turn a proposed option into a recommendation, and the authority gap must
  remain machine-visible rather than silently choosing it.
- Prose drift (new surfaces) after inventory audit commit `20174d50c` →
  denominator closure falsifier catches unmodeled rows only after the inventory
  itself is re-audited (#11575 cadence), so the check is bounded by that
  document's freshness, by design.
