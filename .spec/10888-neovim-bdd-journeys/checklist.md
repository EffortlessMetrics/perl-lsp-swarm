# Implementation Checklist: #10888 — native Neovim LSP user journeys and evidence boundaries

## Change order

This is a documentation/specification-only change. Each step is reviewable
without building or executing any Neovim/host process.

### Step 1: Create the journey/evidence-boundary context contract

- **File:** `.spec/10888-neovim-bdd-journeys/context.md`
- **Change:** record the problem, ledger-format evolution record, the absent
  native-Neovim subject artifact and its open owner, the documented native
  `vim.lsp.config`/`vim.lsp.enable` shape, the stable scenario-ID namespace and
  three-subject law, the journey inventory (41 baseline + 6 optional), claim
  profiles and laws, the evidence chain, the security boundary, the authority
  table, the stable-versus-mutable rule, alternatives rejected, prior art,
  links, and scope.
- **Verify:** checker below enforces contract terms, profile names, chain
  owners, and boundary vocabulary; `git diff --check`.

### Step 2: Create the normative behavior ledger and falsifiers

- **File:** `.spec/10888-neovim-bdd-journeys/acceptance.md`
- **Change:** include all canonical `SPEC_TEMPLATE.md` sections; §Behavior
  carries the bounded core ledger, the conditional #8129 sync branches, the
  deep lifecycle rows, the support stage laws, the optional-input table, and
  profile membership/laws; §Test-Grid carries all twenty-five falsifiers in
  fixed order.
- **Depends on:** Step 1.
- **Verify:** structural heading, scenario-ID-set, profile-vocabulary, and
  falsifier-table checks below; `git diff --check`.

### Step 3: Create the builder/proof contract (this file)

- **File:** `.spec/10888-neovim-bdd-journeys/checklist.md`
- **Change:** bounded change order, deterministic structural checking,
  second-run proof, acceptance gates, handoff.
- **Depends on:** Steps 1-2.
- **Verify:** read-only checker runs twice with byte-identical output and no
  tree diff.

### Step 4: Refresh the tracked non-Rust inventory

- **File:** `docs/policy/NON_RUST_INVENTORY.md`
- **Command:** `cargo xtask non-rust inventory --write`
- **Depends on:** Steps 1-3, and on the packet files being **committed** — the
  generator walks `git ls-files`, so it emits no rows for untracked files.
- **Verify:** the `non_rust_inventory_check` gate in the `policy` CI shard.

The committed inventory enumerates every tracked non-Rust file, including
`.spec/**` documents, so three new tracked documents require three new rows.

Two facts about this regeneration are worth recording, because a reviewer will
otherwise read the diff as scope creep:

- the snapshot is **already stale on unmodified main** (#14203/#14161), so the
  regenerated output also carries one unrelated row and the aggregate counts
  that go with it. A whole-file generator cannot emit this packet's rows
  without them; that row no-ops once main carries its own repair.
- an earlier revision of this packet deliberately skipped the refresh, on the
  correct observation that a pre-commit run produced *only* other people's
  pending rows. That observation stops holding the moment these files are
  committed, which is when the gate actually evaluates them.

Do not hand-edit this file. It is generated output; the command above is its
only sanctioned writer, and the post-merge job runs the same command.

## Deterministic structural proof

The repository has no executable `.spec` graph validator and no Gherkin or
feature-status generator on current main (recorded as the ledger evolution in
`context.md`). Do not invent a generated receipt or claim a missing tool
passed.

**Deviation from the coc/vim precedents:** those packets documented a
PowerShell checker. `pwsh` is not present in this repository's Linux
toolchain, so that checker cannot actually be executed here. This packet
therefore specifies an equivalent Python 3 checker, which is portable, uses
only the standard library, and genuinely runs in this repository.

The script below is the canonical checker. The proof extracts it from this
file and runs it twice, so the executed bytes and the documented bytes cannot
drift. It enforces: the exact three files; required canonical headings, each as
a real heading line rather than a cross-reference; required contract terms
(owner chains, profile names, envelope vocabulary, documented-configuration
shape); the exact forty-seven scenario IDs bound to their §Behavior ledger rows
in fixed family order; **a digest of every complete normative row, so a
published ID cannot be silently rebound to a different behavior, profile tag,
or owner chain**; that the digest map covers **exactly** the stable ID set, so
an entry cannot be dropped to retire a row's binding while the checker still
reports every row as bound; that no foreign rail ID appears as a ledger row; all
twenty-five falsifiers with exact scenario/kind/verdict text in fixed order;
a digest of each named prose invariant (subject law, profile laws, claim
boundary, three-subject law, evidence vocabulary, and the claim-profile
membership table), so a boundary claim cannot be reversed while a
bare required token survives elsewhere in the file, together with the same
exact-coverage requirement over the named invariant set; `git diff --check` over the
candidate range, work tree, and index, with a nonzero status failing the check
rather than being discarded; and a fail-closed changed-path union restricted to
exactly this packet.

Pinning the ID alone would be insufficient: a mutation could keep
`neovim.bdd.core.04` while replacing the proposition it names, which is the
precise failure the immutability law exists to prevent. The row digests make
the ID and its meaning inseparable, so any legitimate rewording of a published
row must update its digest deliberately and visibly in the diff.

Stated limitation, inventory: this checker verifies that the committed
inventory actually contains this packet's rows, not that the whole file equals
the generator's projection. Re-deriving that projection means running the
generator, which would break the checker's read-only, no-build contract and
duplicate `non_rust_inventory_check`, whose job it already is. The boundary is
deliberate: the checker catches an omitted or hand-faked packet refresh; the
gate owns whole-file equivalence.

Stated limitation, owner chains: the checker digests each row's owner-chain cell,
so a chain cannot change silently — but it cannot judge whether the cited owner
actually produces that row's evidence. That is a semantic fit between a
proposition and a `#11392` node's subject, and nothing here reads the manifest.
Two review rounds found mis-routed owners for exactly this reason. An audit that
confirms each cited issue owns *some* node is not the same check and must not be
described as one: `opt.03` cited #10508, which owns a real node that pins
version/platform cells and never produces archive-replay evidence. Owner-subject
fit stays a review obligation on every ledger revision.

Stated limitation, enforcement: no repository gate runs this checker. It is
executed by extracting it from this file, which binds the documented and
executed bytes to each other but not to CI, so a later ledger change can be
made without it. Wiring a gate means new `xtask` and CI surface, which this
docs-only claim does not own; the Vim rail gained its catalog mirror only after
#11371 merged, under separate ownership, and the same sequencing applies here.
Until such an owner exists, this checker is a review instrument, not a ratchet.

Stated limitation, digests: a row and its digest can be changed together in one commit,
so the digests prove *consistency and visibility*, not *authority*. They make
an ID reassignment impossible to perform silently; they cannot establish that
the reassignment was governed. Governance remains a #10888 revision decision
enforced by review, not by this checker.

```python
#!/usr/bin/env python3
"""Deterministic structural checker for .spec/10888-neovim-bdd-journeys/.

Read-only. Re-reads and revalidates every table on each invocation; it never
writes to the work tree. Exits non-zero with a named reason on any violation.
"""
import hashlib
import pathlib
import re
import subprocess

SPEC = ".spec/10888-neovim-bdd-journeys"
FILES = [f"{SPEC}/context.md", f"{SPEC}/acceptance.md", f"{SPEC}/checklist.md"]
# Regenerated by `cargo xtask non-rust inventory --write` because this packet
# adds three tracked documents the committed inventory must list.
INVENTORY = "docs/policy/NON_RUST_INVENTORY.md"

HEADINGS = ["§Behavior", "§Hazards", "§Contracts", "§API-Shape",
            "§Test-Grid", "§Blast-Radius", "§Coverage-Map"]

# The six governed profile IDs from .spec/11392-native-neovim-train-graph/
# train.manifest.json. This packet consumes them verbatim; renaming one here
# would fork the programme vocabulary.
PROFILES = ["native_neovim_configuration", "native_neovim_core",
            "release_v0_18_bounded", "native_neovim_deep_lifecycle",
            "native_neovim_first_class",
            "native_neovim_programme_closeout"]

REQUIRED_TERMS = [
    "perllsp --stdio", "vim.lsp.config", "vim.lsp.enable",
    "editor_client_compat.v1", "consumes_if_available", "not_proven",
    "configuration_documented", "atomic_incremental", "full_document_utf16",
    "#7739", "#10501", "#8129", "#10502", "#10504", "#10505", "#10506",
    "#10507", "#10508", "#10511", "#10514", "#10516", "#10518", "#10520",
    "#10522", "#10523", "#10527", "#7777", "#10858", "#10894", "#7122",
    "#4998", "#6736", "#7762", "#7691", "#6739", "#3983", "#11392",
    "#7716", "nv_release_scope_decision", "0.11.3",
]

FAMILIES = ("attach", "core", "sync", "lifecycle", "support", "opt")
COUNTS = {"attach": 6, "core": 10, "sync": 10, "lifecycle": 7,
          "support": 8, "opt": 6}
EXPECTED_IDS = [f"neovim.bdd.{fam}.{n:02d}"
                for fam in FAMILIES for n in range(1, COUNTS[fam] + 1)]

# Foreign rails must never appear as a ledger row ID in this packet.
FOREIGN_ROW = re.compile(
    r"(?m)^\|\s*`(?:vim\.bdd\.|coc\.[a-z]+\.bdd\.|lite_xl\.bdd\.)")

# Digest of each COMPLETE normative ledger row, so a published ID cannot
# be silently rebound to a different proposition.
# Load-bearing invariants that live in prose rather than in a table. Their
# wording is pinned so a boundary claim cannot be reversed while the required
# tokens survive elsewhere in the file.
INVARIANT_BLOCKS = {
    "subject_law": ("acceptance", r"(?ms)^Subject law:.*?(?=\n\n)"),
    "profile_laws": ("acceptance", r"(?ms)^Laws: a stronger profile.*?(?=\n\n)"),
    "claim_boundary": ("acceptance", r"(?ms)^- \*\*Claim boundary:\*\*.*?(?=\n\n|\Z)"),
    "three_subject_law": ("context", r"(?ms)^Three-subject law:.*?(?=\n\n)"),
    # The evidence-stage vocabulary #10888 requires. Without this the whole
    # table could be deleted and the checker would still pass.
    "evidence_vocabulary": ("acceptance",
                            r"(?ms)^### Evidence-stage vocabulary.*?(?=^## §Test-Grid)"),
    # The claim-profile membership table. Its rows are not `neovim.bdd.*`
    # rows, so ROW_DIGESTS never covered them: a profile's membership could be
    # rewritten -- folding in a branch #11392 forbids, or making one profile a
    # prerequisite of another -- with every other check still passing. Three
    # separate review rounds found exactly that. Bind the table.
    # The conditional-sync preamble. It states which owner observes which
    # branch and that exactly one group is applicable -- a load-bearing law
    # that lived in prose no digest covered. A row correction left it stating
    # the atomic owner for both branches, and nothing failed.
    "branch_selection_law": ("acceptance",
                             r"(?ms)^Both branch groups are published.*?(?=\n\n)"),
    "profile_membership": ("acceptance",
                           r"(?ms)^## Claim profiles \(ledger membership\).*?(?=^Laws: a stronger profile)"),
}

# The set of invariants that must be bound, stated independently of the maps
# above. Iterating a map only visits the keys it still has, so deleting an
# entry from both maps would silently retire an invariant while the checker
# went on reporting success.
EXPECTED_INVARIANTS = {
    "subject_law",
    "profile_laws",
    "claim_boundary",
    "three_subject_law",
    "evidence_vocabulary",
    "profile_membership",
    "branch_selection_law",
}

INVARIANT_DIGESTS = {
    "subject_law": "a48b4d92cad52534",
    "profile_laws": "60e532ff4fcc0274",
    "claim_boundary": "3d6755e631a9c99f",
    "three_subject_law": "e53af321800e90fb",
    "branch_selection_law": "d553244690ec9bd4",
    "profile_membership": "f4de26e57133c176",
    "evidence_vocabulary": "d78c020224e6bc3d",
}

ROW_DIGESTS = {
    "neovim.bdd.attach.01": "3f5b7d942c7cb158",
    "neovim.bdd.attach.02": "e1dea4aedff52388",
    "neovim.bdd.attach.03": "7f9a2da986de1c22",
    "neovim.bdd.attach.04": "2ec42d3754a59bb5",
    "neovim.bdd.attach.05": "896f778bc55f4df5",
    "neovim.bdd.attach.06": "12e3818ea5ce3dfb",
    "neovim.bdd.core.01": "274214192adc000c",
    "neovim.bdd.core.02": "f0d4bc0c256f182e",
    "neovim.bdd.core.03": "5b6b8511c1daff92",
    "neovim.bdd.core.04": "d881fd24dda16ba4",
    "neovim.bdd.core.05": "536b8c2c4595235e",
    "neovim.bdd.core.06": "3554602a0e868157",
    "neovim.bdd.core.07": "4f7218ff0f57ffe3",
    "neovim.bdd.core.08": "017f59ff8c2ea204",
    "neovim.bdd.core.09": "16e3e514e05e4f72",
    "neovim.bdd.core.10": "3e7e2560e23ddd46",
    "neovim.bdd.sync.01": "37855bffcd9130f9",
    "neovim.bdd.sync.02": "8406cf4e14808f72",
    "neovim.bdd.sync.03": "087df18f02c9c85d",
    "neovim.bdd.sync.04": "516bc97b5338f5f2",
    "neovim.bdd.sync.05": "dfc3b58949b3feff",
    "neovim.bdd.sync.06": "06c9aac01c93d6ef",
    "neovim.bdd.sync.07": "79a7596f55c251eb",
    "neovim.bdd.sync.08": "e8c26b53507658ff",
    "neovim.bdd.sync.09": "0946b2febe0e2d75",
    "neovim.bdd.sync.10": "2608d744f1fd042d",
    "neovim.bdd.lifecycle.01": "eb7a2d0f3d283ac1",
    "neovim.bdd.lifecycle.02": "dedecd133898cce7",
    "neovim.bdd.lifecycle.03": "5b222faae72d315f",
    "neovim.bdd.lifecycle.04": "1d61b0cf358bf0b5",
    "neovim.bdd.lifecycle.05": "93a0c2c98fc6f1df",
    "neovim.bdd.lifecycle.06": "5236178f0faa8351",
    "neovim.bdd.lifecycle.07": "78da30c0d117d88c",
    "neovim.bdd.support.01": "b97c19e0b19b003a",
    "neovim.bdd.support.02": "d09ca51510597409",
    "neovim.bdd.support.03": "0c128b4e7684f657",
    "neovim.bdd.support.04": "9280e2dc24e5c1e2",
    "neovim.bdd.support.05": "854b07f5a63fa7c5",
    "neovim.bdd.support.06": "20a4e683cc554e48",
    "neovim.bdd.support.07": "871a2bd1e6d5bd3f",
    "neovim.bdd.support.08": "5411a6c538655928",
    "neovim.bdd.opt.01": "173e8e7968d16764",
    "neovim.bdd.opt.02": "5751ab966cce4258",
    "neovim.bdd.opt.03": "c59af250645ab76f",
    "neovim.bdd.opt.04": "e11adc269e551f73",
    "neovim.bdd.opt.05": "66c897131a0d5185",
    "neovim.bdd.opt.06": "800c91496af6650a",
}

EXPECTED_ROWS = [
    (1, 'Native filetype was pre-set before observation', 'negative', 'reject; activation must be observed, not arranged (attach.01)'),
    (2, 'Wrong parent/sibling root contains an equivalent symbol and passes as root-correct', 'negative', 'reject; root-sensitive answers require the governed root (attach.04–05)'),
    (3, 'Unrelated diagnostic exists while the required diagnostic is absent', 'negative', 'reject; the expected diagnostic itself must appear (core.01)'),
    (4, 'Diagnostic fingerprint changes but the defect remains current', 'negative', 'reject; clearing requires the defect to be gone (core.02)'),
    (5, 'Any completion item exists but the expected item or its application is absent', 'negative', 'reject; the intended item and applied text are the proposition (core.03)'),
    (6, 'Hover is non-null but semantically empty or about another symbol', 'negative', 'reject; identity of the answered entity is the proposition (core.04)'),
    (7, 'Definition request succeeds but opens the wrong target', 'negative', 'reject; intended target content under the governed root (core.05)'),
    (8, '`workspace/configuration` appears in a trace but the setting has no behavior effect', 'negative', 'reject; independent semantic change required (core.07)'),
    (9, 'Formatting returns but bytes remain wrong or a second run changes again', 'negative', 'reject; canonical bytes and idempotence (core.08)'),
    (10, 'Previous-generation fact satisfies a post-edit query', 'negative', 'reject; accepted-generation currentness (core.02/lifecycle.06)'),
    (11, 'Capability is advertised but actual request or application is unobserved', 'negative', 'reject; only actual built-in-client traffic satisfies actual-host rows'),
    (12, 'Forced process kill is recorded as graceful shutdown', 'negative', 'reject; normal quit leaves no bound process (core.09)'),
    (13, 'One version, platform, or channel receipt is substituted for another', 'negative', 'reject; stage and subject non-substitution (support.01–04)'),
    (14, 'Exact-source or local packet is promoted to public distribution', 'negative', 'reject; public stages need their own evidence (support.05–06)'),
    (15, 'A DAP result satisfies an LSP scenario', 'negative', 'reject; rail separation (support.08/opt.06)'),
    (16, 'Core and deep profiles are collapsed into one definition of done', 'negative', 'reject; profiles close independently (profile table)'),
    (17, 'Both #8129 branches are active, or neither branch owner is represented', 'negative', 'reject; exactly one branch applies once #8129 rules (sync.*)'),
    (18, 'An actual-host scenario carries no executable evidence owner', 'negative', 'reject; every row names a downstream owner chain'),
    (19, 'One version, platform, or channel is allowed to satisfy another', 'negative', 'reject; separation laws (support.01–04)'),
    (20, 'A scenario ID is absent from the fixture, receipt, or support mapping', 'negative', 'reject; the scenario→consumer map must be total'),
    (21, 'An optional or upstream-dependent feature is made baseline', 'negative', 'reject; optionals are `consumes_if_available` (opt.*)'),
    (22, 'DAP enters the LSP blocking profile', 'negative', 'reject; sidecar never blocks or promotes core (support.08)'),
    (23, 'Generated feature or status output is stale', 'negative', 'reject; no generator exists on main, so the two-run structural proof discharges this'),
    (24, 'An unsafe client setting is presented as ordinary positive behavior', 'negative', 'reject; governed/rejected per #4998 (core.10)'),
    (25, 'A semantic scenario is satisfied by a boolean-only observation', 'negative', 'reject; semantic identity is the proposition, not liveness'),
]


def fail(msg):
    raise SystemExit(f"SPEC_10888_STRUCTURAL_CHECK=FAIL: {msg}")


def git(*args):
    out = subprocess.run(["git", *args], capture_output=True, text=True)
    if out.returncode != 0:
        fail(f"git {' '.join(args)} failed")
    return out.stdout


def status_paths():
    raw = git("status", "--porcelain=v1", "-z", "--untracked-files=all")
    recs = [r for r in raw.split("\0") if r]
    found, i = [], 0
    while i < len(recs):
        rec = recs[i]
        if len(rec) < 4 or rec[2] != " " or not re.fullmatch(r"[ MADRCU?!]{2}", rec[:2]):
            fail("malformed porcelain record")
        found.append(rec[3:])
        if re.search(r"[RC]", rec[:2]):
            i += 1
            if i >= len(recs) or not recs[i]:
                fail("rename/copy record has no source path")
            found.append(recs[i])
        i += 1
    return found


def main():
    # "Exactly three files" must mean the directory, not three named lookups:
    # otherwise a fourth file could coexist while the checker still reports
    # exact shape.
    present = sorted(q.as_posix() for q in pathlib.Path(SPEC).iterdir())
    if present != sorted(FILES):
        fail(f"packet directory is not exactly the three spec files: {present}")

    texts = {}
    for path in FILES:
        try:
            texts[path] = open(path, encoding="utf-8").read()
        except OSError:
            fail(f"missing spec file: {path}")

    context = texts[FILES[0]]
    acceptance = texts[FILES[1]]
    contract = context + "\n" + acceptance

    for term in REQUIRED_TERMS:
        if term not in contract:
            fail(f"missing contract term: {term}")
    # A heading must be a real heading line; a cross-reference to its name
    # elsewhere in the document must not satisfy it.
    for heading in HEADINGS:
        if not re.search(rf"(?m)^## {re.escape(heading)}(?:\s|$)", acceptance):
            fail(f"missing acceptance heading: {heading}")
    # Profile membership is the acceptance ledger's own obligation; presence in
    # another bundle file cannot satisfy it.
    for term in PROFILES:
        if term not in acceptance:
            fail(f"missing acceptance profile term: {term}")
    for term in ("security-sensitive configuration",
                 "A stronger profile never erases",
                 "three-subject law"):
        if term not in contract and term.capitalize() not in contract:
            fail(f"missing boundary term: {term}")

    # Exactly 47 scenario IDs bound to ledger rows, fixed family order, unique.
    ids = re.findall(
        r"(?m)^\|\s*`(neovim\.bdd\.(?:attach|core|sync|lifecycle|support|opt)\.\d{2})`\s*\|",
        acceptance)
    if len(ids) != len(EXPECTED_IDS):
        fail(f"expected {len(EXPECTED_IDS)} scenario ledger rows, found {len(ids)}")
    if len(set(ids)) != len(ids):
        fail("scenario IDs are not unique")
    if ids != EXPECTED_IDS:
        fail("scenario ledger rows do not match the stable ID set in fixed order")
    if FOREIGN_ROW.search(acceptance):
        fail("a foreign rail ID appears as a ledger row in this packet")

    # A stable ID must name a stable proposition. Pinning the ID alone would
    # let a mutation keep `neovim.bdd.core.04` while replacing its behavior,
    # profile tag, or owner chain -- silent semantic reuse of a published ID,
    # which is exactly what the immutability law forbids. Bind each ID to a
    # digest of its COMPLETE normative row.
    rows_by_id = {}
    for line in acceptance.splitlines():
        m = re.match(
            r"^\|\s*`(neovim\.bdd\.(?:attach|core|sync|lifecycle|support|opt)\.\d{2})`\s*\|",
            line)
        if m:
            canonical = " ".join(line.split())
            rows_by_id[m.group(1)] = hashlib.sha256(
                canonical.encode("utf-8")).hexdigest()[:16]
    # Binding the values is not enough: the loop below visits only the keys
    # the map still has, so deleting an entry would unbind that row's meaning
    # while the ID check above still found all 47 rows present. Require the
    # digest map to cover exactly the stable ID set before trusting it.
    if set(ROW_DIGESTS) != set(EXPECTED_IDS):
        missing = sorted(set(EXPECTED_IDS) - set(ROW_DIGESTS))
        extra = sorted(set(ROW_DIGESTS) - set(EXPECTED_IDS))
        fail(f"row digest map does not cover the stable ID set: "
             f"unbound={missing} unknown={extra}")
    for scenario_id, digest in ROW_DIGESTS.items():
        actual = rows_by_id.get(scenario_id)
        if actual is None:
            fail(f"scenario row missing: {scenario_id}")
        if actual != digest:
            fail(f"scenario {scenario_id} kept its ID but its normative row changed")

    # Row digests bind the tables, but the load-bearing invariants of this
    # packet live in prose. A required-term check is context-blind: it only
    # asks whether a token appears somewhere, so a sentence could be inverted
    # while a bare token survived elsewhere and the checker still passed.
    # Digest each named invariant block so reversing one fails closed.
    sources = {"acceptance": acceptance, "context": context}
    if set(INVARIANT_BLOCKS) != EXPECTED_INVARIANTS or \
            set(INVARIANT_DIGESTS) != EXPECTED_INVARIANTS:
        fail("invariant maps do not cover exactly the named invariant set")
    for name, (which, pattern) in INVARIANT_BLOCKS.items():
        # Digesting the FIRST match would leave a second, contradictory copy
        # of the same block entirely unchecked: the governing paragraph could
        # be restated later with its ownership or applicability reversed, and
        # the digest of the untouched original would still match. An invariant
        # must occur exactly once to be an invariant.
        found = re.findall(pattern, sources[which])
        if not found:
            fail(f"invariant block missing or reshaped: {name}")
        if len(found) != 1:
            fail(f"invariant block occurs {len(found)} times, must be unique: {name}")
        m = re.search(pattern, sources[which])
        got = hashlib.sha256(" ".join(m.group(0).split()).encode("utf-8")).hexdigest()[:16]
        if got != INVARIANT_DIGESTS[name]:
            fail(f"invariant block changed: {name}")

    # Twenty-five falsifiers: fixed order, exact semantics.
    grid = re.search(r"(?ms)^## §Test-Grid\s*(.*?)(?=^## |\Z)", acceptance)
    if not grid:
        fail("§Test-Grid section not found")
    rows = re.findall(
        r"(?m)^\|\s*(\d+)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|",
        grid.group(1))
    if len(rows) != len(EXPECTED_ROWS):
        fail(f"expected {len(EXPECTED_ROWS)} falsifier rows, found {len(rows)}")
    for got, want in zip(rows, EXPECTED_ROWS):
        if (int(got[0]), got[1], got[2], got[3]) != want:
            fail(f"falsifier {want[0]} does not match its published text")

    # Bind the proof to the explicit candidate range.
    # Bind the proof to the candidate's merge-base range. Using origin/main
    # directly would make ordinary main movement invalidate an unchanged,
    # conflict-free packet, which the repository's currentness doctrine
    # explicitly rejects.
    base = git("merge-base", "origin/main", "HEAD").strip()
    head = git("rev-parse", "--verify", "HEAD^{commit}").strip()
    if not base or not head:
        fail("candidate base/HEAD refs are not resolvable")
    # Whitespace hygiene is part of the proof, so a nonzero status must fail
    # the checker rather than being discarded.
    for label, argv in (
            ("candidate range", ["diff", "--check", f"{base}..{head}"]),
            ("work tree", ["diff", "--check"]),
            ("index", ["diff", "--cached", "--check"])):
        probe = subprocess.run(["git", *argv], capture_output=True, text=True)
        if probe.returncode != 0:
            fail(f"git diff --check reported whitespace errors in the {label}")

    changed = set()
    changed.update(git("diff", "--name-only", f"{base}..{head}").split())
    changed.update(git("diff", "--name-only").split())
    changed.update(git("diff", "--cached", "--name-only", "HEAD").split())
    changed.update(status_paths())
    allowed = set(FILES) | {INVENTORY}
    stray = sorted(p for p in changed if p not in allowed)
    if stray:
        fail(f"unexpected changed paths: {stray}")
    missing = sorted(p for p in FILES if p not in changed)
    if missing:
        fail(f"spec files absent from the candidate change set: {missing}")

    # A newly tracked file needs an inventory row, so when this candidate ADDS
    # the packet files the regenerated inventory must travel with them.
    # Allowlisting it is not enough: omitting it would pass here and fail only
    # in the policy shard's non_rust_inventory_check gate. Conditioned on an
    # add rather than required unconditionally, so a later revision that only
    # edits packet prose is not forced to touch generated output.
    statuses = [line for line in
                git("diff", "--name-status", f"{base}..{head}").splitlines()
                if line.strip()]
    adds_packet_file = any(
        line.split("\t")[0].startswith("A") and line.split("\t")[-1] in set(FILES)
        for line in statuses)
    if adds_packet_file:
        if INVENTORY not in changed:
            fail(f"packet files are added but {INVENTORY} was not regenerated")
        # Presence in the changed set is not evidence of content: any edit at
        # all would satisfy it, including an inventory still missing these
        # rows. Validate the required generated state itself.
        try:
            inventory_text = open(INVENTORY, encoding="utf-8").read()
        except OSError:
            fail(f"missing {INVENTORY}")
        absent = [path for path in FILES if f"| `{path}` |" not in inventory_text]
        if absent:
            fail(f"{INVENTORY} does not list the packet's rows: {absent}")

    print("SPEC_10888_STRUCTURAL_CHECK=PASS")
    print(f"scenario_ids={len(ids)} falsifiers={len(rows)}")


if __name__ == "__main__":
    main()
```

Run the proof from the candidate worktree after the files are complete:

```bash
python3 - <<'EXTRACT' > /tmp/spec-10888-check.py
import re, pathlib
text = pathlib.Path('.spec/10888-neovim-bdd-journeys/checklist.md').read_text(encoding='utf-8')
block = re.search(r'(?ms)^```python\n(.*?)^```', text)
if not block:
    raise SystemExit('checker block not found')
print(block.group(1), end='')
EXTRACT

python3 /tmp/spec-10888-check.py > /tmp/spec-10888-run1.out
python3 /tmp/spec-10888-check.py > /tmp/spec-10888-run2.out
cmp /tmp/spec-10888-run1.out /tmp/spec-10888-run2.out && echo SPEC_10888_SECOND_RUN=PASS
sha256sum /tmp/spec-10888-run1.out /tmp/spec-10888-run2.out
git diff --check
git diff --cached --check
```

The checker is read-only: it opens the three files, runs `git` query commands,
and writes nothing into the work tree. A second run must therefore produce
byte-identical output and leave the tree fingerprint unchanged.

## Acceptance gates

- [ ] Exactly `context.md`, `acceptance.md`, and `checklist.md` are added,
      plus the generated `docs/policy/NON_RUST_INVENTORY.md` refresh.
- [ ] All 41 baseline scenarios carry stable IDs, user-visible wording,
      profile/evidence tags, and named downstream owner chains.
- [ ] Core (16 rows), deep lifecycle, and distribution profiles close
      independently; optional rows stay `consumes_if_available`.
- [ ] Both #8129 sync branches are published as conditional groups and neither
      is asserted current.
- [ ] All twenty-five falsifiers present, fixed order, exact verdict semantics.
- [ ] Every published scenario ID is digest-bound to its complete normative
      row, so an ID cannot be reused for a different proposition.
- [ ] `git diff --check` is enforced, not merely invoked.
- [ ] Named prose invariants are digest-bound, so a required term cannot be
      satisfied by an unrelated mention while its sentence is inverted.
- [ ] The evidence-stage vocabulary #10888 requires is consumed explicitly and
      no Neovim-only verdict scalar is minted.
- [ ] Security boundary keeps absolute/traversal include paths out of positive
      behavior (#4998).
- [ ] No native-Neovim subject digest is pre-stated; `attach.02` binds by
      reference to open owner #10502.
- [ ] No fixture bytes, host execution, Lua, receipt, support-tier change,
      train-manifest node, CI edit, or upstream action.
- [ ] Deterministic structural proof passes twice; second run byte-identical.

## Callers and consumers

- #10502 binds fixture/oracle, activation-root, and subject cells to these IDs.
- #10504/#10505/#10506/#10507 bind raw host observations to IDs.
- Generic `editor_client_compat.v1` producers, #10508 version/platform rows,
  #10516/#10518/#10520 channel rows, and #10522/#7122 support projection cite
  IDs downstream.
- #10511/#10514 own the external submission and acceptance stages; #10523 owns
  the DAP sidecar exclusion.

## Flags for builder

- Scenario IDs are immutable once published downstream; changes route through
  a #10888 revision, never silent reuse.
- Behavior wording stays user-visible; implementation trivia belongs to #10502
  and the host leaves.
- If a later leaf can pass only by widening a proposition here, stop and return
  to #10888 instead of editing boundaries locally.
- When #8129 rules, the losing sync branch becomes `not_applicable` by that
  ruling; it is not deleted from the ledger and its IDs are not reused.
- When #10502 lands the native-Neovim subject artifact, `attach.02` is
  re-checked against it; no digest is pre-stated here.
- Deviation note: the controlling issue sketched Gherkin feature files plus
  generated status commands; neither exists on current main, so the journeys
  project into the shipped `.spec` ledger per the evolution record in
  `context.md`.

## Scope boundary

Files IN scope:

- `.spec/10888-neovim-bdd-journeys/context.md`
- `.spec/10888-neovim-bdd-journeys/acceptance.md`
- `.spec/10888-neovim-bdd-journeys/checklist.md`
- `docs/policy/NON_RUST_INVENTORY.md` (generated refresh only)

Files OUT of scope: fixtures, host harnesses/runners, Lua, provisioning,
server/client behavior, receipts, support registry values,
`.spec/11392-native-neovim-train-graph/train.manifest.json`, docs prose, CI
workflows, external upstream surfaces, and any new BDD runner infrastructure.

## Handoff and follow-ups

The writer returns the exact commit SHA, changed-path list, structural-check
output, two-run comparison, and `git diff --check` result. Independent review
must challenge whether every behavioral statement traces to the documented
native configuration or a named authority, whether evidence boundaries name
real owning issues without duplication, whether the conditional sync branches
avoid asserting #8129, and whether any row smuggles implementation trivia into
specification. A clean review proves no Neovim behavior; executable truth
belongs to the downstream leaves, and every scenario remains `not_proven` as
behavior until its exact-host chain passes.
