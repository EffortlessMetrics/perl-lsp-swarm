"""Deterministic generator for the Domain-6 release inventory fragment (#16418).

Reads the exact pinned history range with local git only (no network) and
renders docs/releases/domain6-windows-editor-distribution-first-mile.v1.json.

Fail-closed: any count, coverage, identity, or seed-drift violation exits
nonzero before writing. A second run from identical inputs must produce
byte-identical output (proven by scripts/tests/test_domain6_fragment.py).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = (
    REPO_ROOT
    / "docs"
    / "releases"
    / "domain6-windows-editor-distribution-first-mile.v1.json"
)

SCHEMA = "post_sync_release_reconciliation.v1"
FRAGMENT_ID = "domain6_windows_editor_distribution_first_mile"
START_SHA = "f6b7b2c6626fbefbf01c9c9934cac5789186f8b2"
OBSERVED_HEAD = "102974155487bc955e01d5d5222053c4c449136c"
GROUPING_METHOD_VERSION = (
    "subject-trailing-pair.v1 "
    "+ transplant-PR-key correction "
    "+ merge_commit_sha verification for reviewed rows"
)

PREFILTER = [
    "vscode-extension",
    "distribution",
    "clients",
    "crates/perl-install",
    "docs/install",
    "docs/tutorials",
    "book",
    "CONTRIBUTING.md",
    "scripts",
    "Formula",
    "man",
]

EXPECTED_NON_MERGE = 682
EXPECTED_UNITS = 671
EXPECTED_SEEDS = 5

PAREN_REF = re.compile(r"\(#(\d+)\)")
BODY_REF = re.compile(r"#(\d+)")
QUEUE_PREFIX = "queue: merge #"
SYNC_SUBJECT = re.compile(r"^(sync:|release: history-preserving|chore\(sync\))")

# Merge units kept from the full-history merge enumeration. Every other
# in-range merge touching the prefilter is excluded only with proof:
# queue merges (single queued commit already covered as a non-merge record)
# or sync-lineage merges (second parent predates START_SHA).
KEEP_MERGES = {
    "8a7ba49e96c2d1da6137a264815961d42588be23": {
        "work_unit_id": "PR#12670",
        "subject": "Merge pull request #12670 from "
        "EffortlessMetrics/fix/11198-lite-xl-symbols-path-collision",
        "expected_files": [
            "clients/lite-xl/candidate_manifest.lua",
            "clients/lite-xl/tests/init_document_symbol_identity_test.lua",
            "clients/lite-xl/upstream/init.lua",
        ],
        "primary_fragment": "editor",
    },
    "fb68dd94a33424bf5007e686306b07c6bb2096e6": {
        "work_unit_id": "MERGE#fb68dd94",
        "subject": "merge: reconcile release lineage into swarm main (#0000)",
        "expected_files": [
            "book/src/getting-started/configuration.md",
            "scripts/check_release_channel_actuals.py",
            "scripts/check_release_container_actuals.py",
            "scripts/check_release_history.sh",
            "scripts/check_release_tag_provenance.py",
            "scripts/tests/test_release_channel_actuals.py",
            "scripts/tests/test_release_container_actuals.py",
            "scripts/tests/test_release_tag_provenance.py",
            "vscode-extension/src/test/configuration.test.ts",
        ],
        "primary_fragment": "cross_domain_unassigned",
    },
}

# First-parent-visible re-merge of the fb68dd94 lineage. Proven (in #16418)
# to contribute zero uncovered prefilter content: it differs from fb68dd94
# in exactly one prefilter file whose content comes from in-range non-merge
# commit 96b2ad3ca0ff2008c781ebeb7f4bb90061f66fa8. Recorded as a related
# merge on MERGE#fb68dd94, never as a separate unit.
RELATED_MERGE_984 = "984ff2c897c930e9cbeea331b65487730bda96bd"

# Draft labeled these transplant landings ISS#N from the "[same-root
# transplant]" marker. merge_commit_sha equality proves each commit IS the
# merged PR product, so PR-keying is correct per the grouping method.
TRANSPLANT_CORRECTIONS = {
    "681af39d7177bb24b34f2294fb0485ead56aacde": "PR#15577",
    "f4e8a9614bfe8d8d186d207fe30599f93561f84e": "PR#15582",
    "decd96de6b588dc888767611a43d9d1580998497": "PR#15586",
}

EDITOR_PREFIXES = ("vscode-extension/", "clients/")
DISTRIBUTION_PREFIXES = ("distribution/", "crates/perl-install/", "Formula/", "man/")
FIRST_MILE_PREFIXES = (
    "docs/install/",
    "docs/tutorials/",
    "book/",
    "CONTRIBUTING.md",
)
SCRIPTS_DISTRIBUTION_KEYWORDS = (
    "install",
    "archive",
    "vsix",
    "topology",
    "package",
    "payload",
    "post-publish",
    "smoke",
    "bootstrap",
)


class FragmentError(Exception):
    """Fail-closed inventory violation."""


# Reviewed seed rows (#16418 acceptance). paths_or_components equals the
# exact effective file list of each commit, verified against current history
# (merge_commit_sha equality included). The generator re-verifies every run.
SEEDS = {
    "PR#16371": {
        "commits": ["c76a4436b1a0f996e7e4cd76aefac3034319a4b9"],
        "subject": "fix(release): generated installer bootstrap values "
        "and release-archive install smoke (#16368) (#16371)",
        "paths_or_components": [
            "docs/tutorials/GETTING_STARTED.md",
            "scripts/post-publish-smoke.sh",
        ],
        "release_domains": ["install", "docs"],
        "primary_disposition": "packaging_install_or_editor",
        "reachable_installed_effect": "bounded",
        "public_claim_refs": [],
        "release_note_disposition": "not_user_facing",
        "migration_or_upgrade_refs": [],
        "api_schema_package_effects": [],
        "proof_owner_refs": ["scripts/post-publish-smoke.sh"],
        "known_limitations": [
            "smoke runs post-publish; no isolated-install extraction "
            "proof on Windows in-unit"
        ],
        "open_pr_relationships": [],
        "controlling_issues": ["16368"],
        "invalidators": ["archive layout/member changes after snapshot"],
        "platforms_and_targets": ["portable"],
        "artifact_or_route_effects": [
            "bootstrap values feed installer docs; smoke covers "
            "release-archive install route"
        ],
        "editor_manifest_or_protocol_effects": [],
        "installed_evidence_stage": "mechanism",
        "primary_fragment": "distribution",
        "grouping_evidence": "trailing (#16371) of subject pair "
        "(#16368)(#16371); merge_commit_sha of PR #16371 equals the "
        "commit; issue #16368 is the tracked problem",
    },
    "PR#16207": {
        "commits": ["f6bf580309b5e0f8cb7f82e84f9e2f0d6a37d8d3"],
        "subject": "feat(release): bind RC and numeric VSIX identities "
        "offline (#14923) (#16207)",
        "paths_or_components": [
            ".github/workflows/release-history.yml",
            ".github/workflows/vscode-prebuilt-payload-adapter.yml",
            ".spec/14923-rc-vsix-binding/README.md",
            "fixtures/rc_vsix_binding/valid.topology.v4.json",
            "policy/non-rust-allowlist.toml",
            "schemas/release_topology.v4.schema.json",
            "schemas/vsix_candidate_payload.v2.schema.json",
            "scripts/generate_release_topology.py",
            "scripts/prepare_vsix_prebuilt_payload.py",
            "scripts/release_build_identity.py",
            "scripts/release_topology_json.py",
            "scripts/release_vsix_mapping.py",
            "scripts/test_generate_release_topology.py",
            "scripts/test_prepare_vsix_prebuilt_payload.py",
            "scripts/test_release_build_identity.py",
            "vscode-extension/scripts/build_vsix_candidate_manifest.js",
            "vscode-extension/scripts/check-vsix-inventory-transition.js",
            "vscode-extension/scripts/check-vsix-prebuilt-payload.js",
            "vscode-extension/scripts/check-vsix-prebuilt-payload.test.js",
            "vscode-extension/src/test/vsixPackageProjection.test.ts",
            "vscode-extension/src/vsixPackageProjection.ts",
        ],
        "release_domains": ["editor", "install", "release"],
        "primary_disposition": "packaging_install_or_editor",
        "reachable_installed_effect": "bounded",
        "public_claim_refs": [],
        "release_note_disposition": "not_user_facing",
        "migration_or_upgrade_refs": [],
        "api_schema_package_effects": [
            "release_topology.v4",
            "vsix_candidate_payload.v2",
        ],
        "proof_owner_refs": [
            ".spec/14923-rc-vsix-binding/README.md",
            "fixtures/rc_vsix_binding/valid.topology.v4.json",
        ],
        "known_limitations": [
            "packaged selection lands via carrier PR #16230, verified "
            "OPEN 2026-09-23"
        ],
        "open_pr_relationships": ["16230-OPEN"],
        "controlling_issues": ["14923-OPEN"],
        "invalidators": ["#16230 conflict; topology v4 evolution"],
        "platforms_and_targets": ["portable"],
        "artifact_or_route_effects": [
            "RC/numeric VSIX identity binding offline; candidate payload "
            "composition"
        ],
        "editor_manifest_or_protocol_effects": [],
        "installed_evidence_stage": "mechanism",
        "primary_fragment": "editor",
        "grouping_evidence": "trailing (#16207) of subject pair "
        "(#14923)(#16207); merge_commit_sha of PR #16207 equals the "
        "commit; issue #14923 is the tracked problem",
    },
    "PR#12086": {
        "commits": ["19ba2dd4c5c3d0c9e6363f4cb0a174be25636f7c"],
        "subject": "feat(vscode): retain active managed candidates and GC "
        "only proven stale generations (#10083) (#12086)",
        "paths_or_components": [
            "vscode-extension/CHANGELOG.md",
            "vscode-extension/scripts/vsix-inventory-baseline.json",
            "vscode-extension/scripts/vsix-inventory-transition.json",
            "vscode-extension/src/downloader.ts",
            "vscode-extension/src/extension.ts",
            "vscode-extension/src/managedCandidateRuntime.ts",
            "vscode-extension/src/test/__mocks__/vscode.ts",
            "vscode-extension/src/test/downloader.test.ts",
            "vscode-extension/src/test/managedCandidateRuntime.test.ts",
        ],
        "release_domains": ["editor", "install"],
        "primary_disposition": "packaging_install_or_editor",
        "reachable_installed_effect": "yes",
        "public_claim_refs": [],
        "release_note_disposition": "required",
        "migration_or_upgrade_refs": ["stale-generation GC behavior"],
        "api_schema_package_effects": [],
        "proof_owner_refs": [
            "vscode-extension/src/test/downloader.test.ts",
            "vscode-extension/src/test/managedCandidateRuntime.test.ts",
        ],
        "known_limitations": [
            "unit/source stage only; installed-acceptance proof belongs "
            "to #6056 lane"
        ],
        "open_pr_relationships": [],
        "controlling_issues": ["10083"],
        "invalidators": [],
        "platforms_and_targets": ["vscode-managed"],
        "artifact_or_route_effects": [
            "managed binary download/retain/GC lifecycle on user machines"
        ],
        "editor_manifest_or_protocol_effects": [],
        "installed_evidence_stage": "source",
        "primary_fragment": "editor",
        "grouping_evidence": "trailing (#12086) of subject pair "
        "(#10083)(#12086); merge_commit_sha of PR #12086 equals the "
        "commit; issue #10083 is the tracked problem",
    },
    "PR#10198": {
        "commits": ["bed531ec598d84662dc3f146ff74db1b0ffaf974"],
        "subject": "fix(vscode): namespace managed candidates by exact "
        "host compatibility target (#9847) (#10198)",
        "paths_or_components": [
            "vscode-extension/scripts/vsix-inventory-baseline.json",
            "vscode-extension/src/downloader.ts",
            "vscode-extension/src/managedStorageIdentity.ts",
            "vscode-extension/src/test/debugAdapter.test.ts",
            "vscode-extension/src/test/downloader.test.ts",
            "vscode-extension/src/test/managedNamespaceIsolation.test.ts",
            "vscode-extension/src/test/managedStorageIdentity.test.ts",
        ],
        "release_domains": ["editor", "install"],
        "primary_disposition": "packaging_install_or_editor",
        "reachable_installed_effect": "yes",
        "public_claim_refs": [],
        "release_note_disposition": "required",
        "migration_or_upgrade_refs": [
            "per-host candidate namespace isolation"
        ],
        "api_schema_package_effects": [],
        "proof_owner_refs": [
            "vscode-extension/src/test/managedNamespaceIsolation.test.ts",
            "vscode-extension/src/test/managedStorageIdentity.test.ts",
        ],
        "known_limitations": [
            "unit/source stage only; installed-acceptance proof belongs "
            "to #6056 lane"
        ],
        "open_pr_relationships": [],
        "controlling_issues": ["9847"],
        "invalidators": [],
        "platforms_and_targets": ["vscode-managed"],
        "artifact_or_route_effects": [
            "managed candidates namespaced by exact host compatibility target"
        ],
        "editor_manifest_or_protocol_effects": [],
        "installed_evidence_stage": "source",
        "primary_fragment": "editor",
        "grouping_evidence": "trailing (#10198) of subject pair "
        "(#9847)(#10198); merge_commit_sha of PR #10198 equals the "
        "commit; issue #9847 is the tracked problem",
    },
    "PR#14523": {
        "commits": ["f6a34fde7abe2f2662fae91b14453b02313e8e65"],
        "subject": "feat(dap): add typed launch-authority startup "
        "contract (#8656) (#14523)",
        "paths_or_components": [
            ".ci/dap/editor-transport-inventory.v1.json",
            ".ci/public-api-baselines/perl-dap.txt",
            ".github/workflows/dap-editor-transport.yml",
            ".github/workflows/dap-scorecard.yml",
            "clients/sublime/LSP-perllsp/dap_support.py",
            "clients/sublime/LSP-perllsp/debugger_adapter.py",
            "clients/sublime/LSP-perllsp/host_tests/test_sublime_debugger_adapter.py",
            "clients/sublime/LSP-perllsp/tests/test_dap_receipt.py",
            "clients/sublime/LSP-perllsp/tests/test_dap_support.py",
            "clients/sublime/validate_sublime_dap_receipt.py",
            "crates/perl-dap/Cargo.toml",
            "crates/perl-dap/src/debug_adapter/mod.rs",
            "crates/perl-dap/src/debug_adapter/process.rs",
            "crates/perl-dap/src/lib.rs",
            "crates/perl-dap/src/main.rs",
            "crates/perl-dap/src/security/launch_authority.rs",
            "crates/perl-dap/src/security/mod.rs",
            "crates/perl-dap/src/server/config.rs",
            "crates/perl-dap/src/server/lifecycle.rs",
            "crates/perl-dap/tests/common/mod.rs",
            "crates/perl-dap/tests/dap_adapter_tests.rs",
            "crates/perl-dap/tests/dap_attach_e2e.rs",
            "crates/perl-dap/tests/dap_comprehensive_test.rs",
            "crates/perl-dap/tests/dap_coverage_audit_tests.rs",
            "crates/perl-dap/tests/dap_golden_transcript_tests.rs",
            "crates/perl-dap/tests/dap_integration_test.rs",
            "crates/perl-dap/tests/dap_launch_error_remediation_tests.rs",
            "crates/perl-dap/tests/dap_launch_security_test.rs",
            "crates/perl-dap/tests/dap_module_resolution_smoke.rs",
            "crates/perl-dap/tests/dap_scorecard_harness.rs",
            "crates/perl-dap/tests/dap_server_and_adapter_tests.rs",
            "crates/perl-dap/tests/dap_session_cleanup_e2e.rs",
            "crates/perl-dap/tests/security_regression_tests.rs",
            "crates/perl-dap/tests/wave_h_external_red_tests.rs",
            "docs/tutorials/DAP_USER_GUIDE.md",
            "scripts/ci/dap_scorecard_probes.py",
            "scripts/ci/dap_scorecard_runtime.py",
            "scripts/ci/dap_scorecard_transport.py",
            "scripts/tests/test_dap_scorecard_runtime.py",
            "scripts/ux/neovim/perl_dap.lua",
            "vscode-extension/package-lock.json",
            "vscode-extension/src/debugAdapter.ts",
            "vscode-extension/src/test/debugAdapter.test.ts",
        ],
        "release_domains": ["editor", "dap"],
        "primary_disposition": "product_behavior",
        "reachable_installed_effect": "bounded",
        "public_claim_refs": [],
        "release_note_disposition": "covered",
        "migration_or_upgrade_refs": [],
        "api_schema_package_effects": [
            ".ci/public-api-baselines/perl-dap.txt"
        ],
        "proof_owner_refs": [
            "clients/sublime host_tests",
            "editor-transport inventory",
        ],
        "known_limitations": [
            "sublime client surface plus perl-dap launch-authority "
            "implementation; VS Code DAP startup covered by separate units"
        ],
        "open_pr_relationships": [],
        "controlling_issues": ["8656"],
        "invalidators": [],
        "platforms_and_targets": ["sublime"],
        "artifact_or_route_effects": [
            "typed DAP launch-authority startup contract plus perl-dap "
            "launch-authority implementation"
        ],
        "editor_manifest_or_protocol_effects": ["dap startup contract"],
        "installed_evidence_stage": "mechanism",
        "primary_fragment": "editor",
        "grouping_evidence": "trailing (#14523) of subject pair "
        "(#8656)(#14523); merge_commit_sha of PR #14523 equals the "
        "commit; issue #8656 is the tracked problem",
    },
}


def git(*args: str) -> str:
    proc = subprocess.run(
        ["git", *args],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise FragmentError(f"git {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout


def resolve_range() -> None:
    for sha in (START_SHA, OBSERVED_HEAD):
        out = git("cat-file", "-t", sha).strip()
        if out != "commit":
            raise FragmentError(f"{sha} is not a commit")


def list_non_merges() -> list[dict]:
    raw = git(
        "log",
        "--no-merges",
        "--format=%H%x1f%s%x1f%b%x1e",
        f"{START_SHA}..{OBSERVED_HEAD}",
        "--",
        *PREFILTER,
    )
    records = []
    for chunk in raw.split("\x1e"):
        chunk = chunk.strip("\n")
        if not chunk.strip():
            continue
        sha, subject, body = chunk.split("\x1f")
        records.append({"sha": sha, "subject": subject, "body": body.strip()})
    records.sort(key=lambda r: r["sha"])
    # Population must reproduce the reviewed draft exactly; order here is
    # canonical-by-sha for determinism (draft file order was history order).
    if len(records) != EXPECTED_NON_MERGE:
        raise FragmentError(
            f"non-merge record count {len(records)} != {EXPECTED_NON_MERGE}"
        )
    return records


def list_merges() -> list[dict]:
    raw = git(
        "log",
        "--merges",
        "--format=%H%x1f%s%x1e",
        f"{START_SHA}..{OBSERVED_HEAD}",
        "--",
        *PREFILTER,
    )
    merges = []
    for chunk in raw.split("\x1e"):
        chunk = chunk.strip("\n")
        if not chunk.strip():
            continue
        sha, subject = chunk.split("\x1f")
        merges.append({"sha": sha, "subject": subject})
    return merges


def commit_files(sha: str, *, merge: bool = False) -> list[str]:
    if merge:
        first_parent = git("rev-parse", f"{sha}^1").strip()
        raw = git("diff", "--name-only", first_parent, sha, "--", *PREFILTER)
    else:
        raw = git("diff-tree", "--no-commit-id", "--name-only", "-r", sha)
    return sorted({line for line in raw.splitlines() if line.strip()})


def group_unit_id(subject: str) -> str:
    refs = PAREN_REF.findall(subject)
    if len(refs) >= 2:
        return f"PR#{refs[-1]}"
    if len(refs) == 1:
        return f"ISS#{refs[0]}"
    return "NOREF"


def file_family(path: str) -> str | None:
    for prefix in EDITOR_PREFIXES:
        if path.startswith(prefix):
            return "editor"
    for prefix in DISTRIBUTION_PREFIXES:
        if path.startswith(prefix):
            return "distribution"
    for prefix in FIRST_MILE_PREFIXES:
        if path.startswith(prefix):
            return "first_mile"
    if path.startswith("scripts/"):
        base = path.rsplit("/", 1)[-1].lower()
        if any(key in base for key in SCRIPTS_DISTRIBUTION_KEYWORDS):
            return "distribution"
        return None
    return None


def route_fragment(files: list[str]) -> tuple[str, str]:
    families = {file_family(path) for path in files} - {None}
    if len(families) == 1:
        only = next(iter(families))
        return only, "single_family_paths"
    return "cross_domain_unassigned", "mixed_or_mechanics_paths"


def second_parent_predates_start(sha: str) -> bool:
    parents = git("rev-parse", f"{sha}^1", f"{sha}^2").split()
    if len(parents) != 2:
        raise FragmentError(f"merge {sha} does not have two parents")
    proc = subprocess.run(
        ["git", "merge-base", "--is-ancestor", parents[1], START_SHA],
        cwd=REPO_ROOT,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return proc.returncode == 0


def classify_merges(
    merges: list[dict], non_merge_shas: set[str]
) -> tuple[list[dict], dict]:
    kept = []
    stats = {"queue_excluded": 0, "lineage_excluded": 0}
    for merge in merges:
        sha, subject = merge["sha"], merge["subject"]
        if sha in KEEP_MERGES:
            kept.append(merge)
            continue
        if subject.startswith(QUEUE_PREFIX):
            only = git("log", "--format=%H", f"{sha}^2", "--not", f"{sha}^1")
            only_shas = [line for line in only.splitlines() if line.strip()]
            if len(only_shas) != 1 or only_shas[0] not in non_merge_shas:
                raise FragmentError(
                    f"queue merge {sha} content is not a single covered "
                    f"non-merge record: {only_shas}"
                )
            stats["queue_excluded"] += 1
            continue
        if SYNC_SUBJECT.match(subject):
            if not second_parent_predates_start(sha):
                raise FragmentError(
                    f"sync-lineage merge {sha} has in-range second-parent "
                    "content and needs a unit"
                )
            stats["lineage_excluded"] += 1
            continue
        raise FragmentError(
            f"merge {sha} ({subject}) is neither kept, queue-covered, "
            "nor pre-range lineage"
        )
    if {m["sha"] for m in kept} != set(KEEP_MERGES):
        raise FragmentError("kept merge set drifted from KEEP_MERGES")
    return kept, stats


def build_work_units(records: list[dict]) -> tuple[list[dict], list[dict]]:
    by_unit: dict[str, list[dict]] = {}
    for record in records:
        unit_id = group_unit_id(record["subject"])
        by_unit.setdefault(unit_id, []).append(record)
    if len(by_unit) != EXPECTED_UNITS:
        raise FragmentError(
            f"work-unit identity count {len(by_unit)} != {EXPECTED_UNITS}"
        )
    if set(by_unit) != set(group_unit_id(r["subject"]) for r in records):
        raise FragmentError("unit identity mismatch")
    # Draft labeled transplant landings ISS#N; merge_commit_sha equality
    # proves PR-keying. The grouping rule already yields PR#N; record why.
    for sha, correct in TRANSPLANT_CORRECTIONS.items():
        found = [u for u, rs in by_unit.items() if any(r["sha"] == sha for r in rs)]
        if found != [correct]:
            raise FragmentError(
                f"transplant correction for {sha} failed: {found}"
            )
    files_cache = {r["sha"]: commit_files(r["sha"]) for r in records}
    units = []
    noref_watchlist = []
    for unit_id in sorted(by_unit):
        members = sorted(by_unit[unit_id], key=lambda r: r["sha"])
        commits = [r["sha"] for r in members]
        files = sorted({f for r in members for f in files_cache[r["sha"]]})
        if unit_id in SEEDS:
            seed = SEEDS[unit_id]
            if commits != seed["commits"]:
                raise FragmentError(f"seed {unit_id} commit set drifted")
            if files != sorted(seed["paths_or_components"]):
                raise FragmentError(
                    f"seed {unit_id} effective file list drifted from the "
                    "reviewed row"
                )
            row = {
                "work_unit_id": unit_id,
                "commits": commits,
                "grouping_evidence": seed["grouping_evidence"],
                "primary_fragment": seed["primary_fragment"],
                "primary_fragment_basis": "seed_review",
                "disposition_state": "reviewed",
                "paths_or_components": files,
                "release_domains": seed["release_domains"],
                "primary_disposition": seed["primary_disposition"],
                "reachable_installed_effect": seed[
                    "reachable_installed_effect"
                ],
                "public_claim_refs": seed["public_claim_refs"],
                "release_note_disposition": seed["release_note_disposition"],
                "migration_or_upgrade_refs": seed["migration_or_upgrade_refs"],
                "api_schema_package_effects": seed[
                    "api_schema_package_effects"
                ],
                "proof_owner_refs": seed["proof_owner_refs"],
                "known_limitations": seed["known_limitations"],
                "open_pr_relationships": seed["open_pr_relationships"],
                "controlling_issues": seed["controlling_issues"],
                "invalidators": seed["invalidators"],
                "platforms_and_targets": seed["platforms_and_targets"],
                "artifact_or_route_effects": seed["artifact_or_route_effects"],
                "editor_manifest_or_protocol_effects": seed[
                    "editor_manifest_or_protocol_effects"
                ],
                "installed_evidence_stage": seed["installed_evidence_stage"],
            }
            units.append(row)
            continue
        fragment, basis = route_fragment(files)
        subjects = sorted({r["subject"] for r in members})
        if len(subjects) == 1:
            refs = PAREN_REF.findall(subjects[0])
            if len(refs) >= 2:
                evidence = (
                    f"trailing (#{refs[-1]}) of subject pair; provisional "
                    "PR-keyed unit, merge_commit_sha not verified, "
                    "disposition owns verification"
                )
            elif len(refs) == 1:
                evidence = (
                    f"lone (#{refs[0]}) in subject; provisional "
                    "issue-grouped direct push, relationship not verified, "
                    "disposition owns verification"
                )
            else:
                body_refs = sorted(
                    {m for m in BODY_REF.findall(members[0]["body"])}
                )
                evidence = (
                    "no (#[0-9]+) in subject; #10305 adjudication owns "
                    "identity"
                )
                if body_refs:
                    evidence += (
                        "; body mentions candidate "
                        + ", ".join(f"#{n}" for n in body_refs)
                        + " (unverified relationship, not a regroup)"
                    )
        else:
            evidence = (
                f"{len(members)} commits share provisional unit {unit_id}; "
                "per-commit subjects retained below; title similarity "
                "alone never groups, disposition verifies each member"
            )
        row = {
            "work_unit_id": unit_id,
            "commits": commits,
            "grouping_evidence": evidence,
            "primary_fragment": fragment,
            "primary_fragment_basis": basis,
            "disposition_state": "not_proven",
            "paths_or_components": files,
        }
        if len(subjects) > 1:
            row["member_subjects"] = [
                {"commit": r["sha"], "subject": r["subject"]} for r in members
            ]
        units.append(row)
        if unit_id == "NOREF":
            for r in members:
                if not BODY_REF.search(r["subject"] + "\n" + r["body"]):
                    noref_watchlist.append(
                        {
                            "commit": r["sha"],
                            "subject": r["subject"],
                            "reason": "no (#[0-9]+) in subject or body; "
                            "#10305 adjudication owns identity",
                        }
                    )
    noref_watchlist.sort(key=lambda w: w["commit"])
    return units, noref_watchlist


def build_merge_units(kept: list[dict]) -> list[dict]:
    rows = []
    for merge in sorted(kept, key=lambda m: m["sha"]):
        spec = KEEP_MERGES[merge["sha"]]
        files = commit_files(merge["sha"], merge=True)
        if files != sorted(spec["expected_files"]):
            raise FragmentError(
                f"merge {merge['sha']} effective prefilter file list drifted"
            )
        evidence = (
            f"in-range merge kept from full-history enumeration; {merge['subject']}"
        )
        if merge["sha"] == "fb68dd94a33424bf5007e686306b07c6bb2096e6":
            evidence += (
                "; first-parent-visible re-merge "
                f"{RELATED_MERGE_984} contributes zero uncovered prefilter "
                "content (single-file delta resolves to in-range non-merge "
                "96b2ad3ca0ff2008c781ebeb7f4bb90061f66fa8) and is recorded "
                "as related, never as a separate unit"
            )
        rows.append(
            {
                "work_unit_id": spec["work_unit_id"],
                "commits": [merge["sha"]],
                "grouping_evidence": evidence,
                "primary_fragment": spec["primary_fragment"],
                "primary_fragment_basis": "merge_unit_review",
                "disposition_state": "not_proven",
                "paths_or_components": files,
            }
        )
    return rows


def canonical_digest(payload: dict) -> str:
    canonical = json.dumps(
        payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    )
    return "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def build_document() -> dict:
    resolve_range()
    records = list_non_merges()
    non_merge_shas = {r["sha"] for r in records}
    merges = list_merges()
    kept, merge_stats = classify_merges(merges, non_merge_shas)
    units, noref_watchlist = build_work_units(records)
    merge_units = build_merge_units(kept)
    reviewed = [u for u in units if u["disposition_state"] == "reviewed"]
    if len(reviewed) != EXPECTED_SEEDS:
        raise FragmentError(f"reviewed seed count {len(reviewed)} != 5")
    if set(SEEDS) != {u["work_unit_id"] for u in reviewed}:
        raise FragmentError("reviewed seed identity set drifted")
    not_proven = [u for u in units if u["disposition_state"] == "not_proven"]
    if len(units) - len(reviewed) != EXPECTED_UNITS - EXPECTED_SEEDS:
        raise FragmentError("not_proven unit count drifted")
    covered = sorted(c for u in units for c in u["commits"])
    if len(covered) != len(set(covered)):
        raise FragmentError("a non-merge commit is covered twice")
    if set(covered) != non_merge_shas:
        raise FragmentError("non-merge coverage is not exactly-once")
    total_rows = len(records) + len(merge_units) + len(noref_watchlist)
    payload = {
        "schema": SCHEMA,
        "fragment_id": FRAGMENT_ID,
        "parent_denominator": {
            "start_sha": START_SHA,
            "observed_head": OBSERVED_HEAD,
        },
        "prefilter_paths": PREFILTER,
        "grouping_method_version": GROUPING_METHOD_VERSION,
        "record_counts": {
            "non_merge": len(records),
            "merge_units": len(merge_units),
            "noref_watchlist_rows": len(noref_watchlist),
            "total_rows": total_rows,
            "unique_commits": len(non_merge_shas) + len(merge_units),
        },
        "work_unit_count": len(units),
        "reviewed_seed_count": len(reviewed),
        "not_proven_unit_count": len(not_proven),
        "work_units": units,
        "merge_units": merge_units,
        "noref_watchlist": noref_watchlist,
        "corrections": [
            "transplant landings 681af39d/f4e8a961/decd96de are PR-keyed "
            "(PR#15577/PR#15582/PR#15586): each commit equals the PR "
            "merge_commit_sha, so the draft ISS#N labels were corrected",
            f"first-parent-visible re-merge {RELATED_MERGE_984} is "
            "related to MERGE#fb68dd94, not a separate unit (zero "
            "uncovered prefilter content proven in #16418)",
            "subject-NOREF commits are 5; body-aware true-NOREF rows are "
            "3 (6a4e0ea6 mentions #14678, ca11589a mentions #4346; both "
            "kept NOREF with candidate notes, never silently regrouped)",
            "total_rows 687 = 682 unit-mapping rows + 2 merge rows + 3 "
            "true-NOREF watchlist rows; the 3 watchlist rows intentionally "
            "duplicate commits already mapped once (unique commits 684)",
        ],
        "merge_enumeration": {
            "full_history_merges_touching_prefilter": len(merges),
            "queue_merges_excluded_covered": merge_stats["queue_excluded"],
            "sync_lineage_merges_excluded_pre_range": merge_stats[
                "lineage_excluded"
            ],
            "merge_units_kept": len(merge_units),
        },
        "consumers": {
            "parent_ledger": "#16399",
            "domain_synthesizer": "#16407",
            "disposition_children": ["#16414", "#16415", "#16416"],
            "history_identity": "#10305",
            "live_pr_disposition": ["#13215", "#5888"],
        },
    }
    payload["coverage"] = {
        "omitted_records": 0,
        "duplicate_records": 0,
        "omitted_work_units": 0,
        "duplicate_work_units": 0,
        "digest": canonical_digest(payload),
    }
    return payload


def render(payload: dict) -> str:
    return (
        json.dumps(payload, sort_keys=True, indent=2, ensure_ascii=False)
        + "\n"
    )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Generate the Domain-6 inventory fragment (#16418)."
    )
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument(
        "--check",
        action="store_true",
        help="regenerate to a temp file and fail when the checked-in "
        "artifact differs (deterministic-render proof)",
    )
    args = parser.parse_args(argv)
    try:
        payload = build_document()
    except FragmentError as exc:
        print(f"domain6 fragment failed: {exc}", file=sys.stderr)
        return 1
    text = render(payload)
    if args.check:
        with tempfile.NamedTemporaryFile(
            "w", suffix=".json", delete=False, encoding="utf-8"
        ) as handle:
            handle.write(text)
            tmp = handle.name
        try:
            expected = args.out.read_text(encoding="utf-8")
        except OSError as exc:
            print(f"domain6 fragment --check failed: {exc}", file=sys.stderr)
            return 1
        if text != expected:
            print(
                "domain6 fragment drifted: regeneration is not "
                "byte-identical to the checked-in artifact",
                file=sys.stderr,
            )
            return 2
        print(f"domain6 fragment deterministic render matches {args.out}")
        Path(tmp).unlink()
        return 0
    args.out.write_text(text, encoding="utf-8")
    print(f"domain6 fragment wrote {args.out}")
    print(f"digest: {payload['coverage']['digest']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))


