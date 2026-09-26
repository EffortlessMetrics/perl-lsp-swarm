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
    "+ merge_commit_sha verification for reviewed rows "
    "+ placeholder-0000 per-commit split "
    "+ NOREF body-mention evidence"
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
EXPECTED_UNITS = 672
EXPECTED_SEEDS = 5
# Unsimplified range merge population (START_SHA..OBSERVED_HEAD,
# --full-history): 2 kept + 1 related-record + 62 content-free +
# 11 blob-covered + 18 created = 94. Pinned: the range is fixed, the
# traversal is unsimplified, so drift fails closed.
EXPECTED_MERGES = 94
EXPECTED_CONTENT_FREE = 62
EXPECTED_BLOB_COVERED = 11
EXPECTED_MERGE_CREATED = 18

PAREN_REF = re.compile(r"\(#(\d+)\)")
BODY_REF = re.compile(r"#(\d+)")
# Placeholder issue numbers carry no work-unit identity. (#0000) in a
# subject never groups: each such commit becomes its own per-commit unit.
PLACEHOLDER_ISSUE_NUMBERS = {"0000"}
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

# First-parent-visible re-merge of the fb68dd94 lineage. Mechanically verified
# (verify_related_remerge) to contribute zero uncovered prefilter content:
# it differs from fb68dd94 in exactly one prefilter file
# (scripts/ci/validate_gate_lane_mapping.py) whose blob sits in the mapped
# range records 3dc8ade1 (#4976) and b86ba02a (#5426). Recorded as a related
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
        "release_domains": ["install", "docs", "release"],
        "primary_disposition": "packaging_install_or_editor",
        "reachable_installed_effect": "bounded",
        "public_claim_refs": [],
        "release_note_disposition": "required",
        "migration_or_upgrade_refs": [],
        "api_schema_package_effects": [],
        "proof_owner_refs": [
            "scripts/post-publish-smoke.sh",
            ".github/workflows/post-publish-smoke.yml",
        ],
        "known_limitations": [
            "smoke Section 1b runs post-publish; its Linux/macOS happy "
            "path has not executed through current main (v0.17.0 "
            "predates it, no later release, zero post-publish-smoke.yml "
            "runs)",
            "installer supports Linux/macOS hosts only; Windows takes a "
            "labeled skip, so Windows archive install/extraction proof "
            "belongs to #16408, not this unit",
            "generated INSTALLER_REF/INSTALLER_SHA256 are "
            "convenience-level (derived from the same host that serves "
            "the installer), not independent review; the reviewed "
            "closeout digest remains unpublished",
            "the v0.17.0 root wrapper ignores the identity env vars and "
            "re-execs floating master; docs repaired after the "
            "denominator head by PR#16398 (#16396, still open for the "
            "other wrapper docs) with two routes and "
            "VERSION=${RELEASE_TAG:-latest} binding",
        ],
        "open_pr_relationships": [
            "16359-OPEN touches GETTING_STARTED.md (config-limits "
            "examples only; installer Option 2 section untouched)"
        ],
        "controlling_issues": ["16368"],
        "invalidators": [
            "archive layout/member changes after snapshot",
            "install.sh or root-wrapper verification semantics change "
            "before the next release-archive smoke run",
            "a future Section 1b run whose outcome contradicts this "
            "mechanism-stage row",
        ],
        "platforms_and_targets": ["linux", "macos"],
        "artifact_or_route_effects": [
            "docs Option 2 generated-values step derives INSTALLER_REF "
            "(tag-dereferenced publish commit) and INSTALLER_SHA256 "
            "(digest of scripts/install.sh at that ref) from a pinned "
            "RELEASE_TAG, replacing unusable placeholders",
            "smoke Section 1b executes scripts/install.sh against the "
            "published GitHub release archive into an isolated "
            "INSTALL_DIR (download, SHA256SUMS verify, install, "
            "installed perllsp --version must report the release "
            "version); SKIP_INSTALL=1 and non-Linux/macOS hosts take "
            "labeled skips",
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
            "release_topology.v4 (new; accepted topology versions extend "
            "1-3 with 4, opt-in only)",
            "vsix_candidate_payload.v2 (new; legacy v1 default admission "
            "unchanged)",
        ],
        "proof_owner_refs": [
            ".spec/14923-rc-vsix-binding/README.md",
            "fixtures/rc_vsix_binding/valid.topology.v4.json",
            "scripts/test_generate_release_topology.py",
            "scripts/test_release_build_identity.py",
            "scripts/test_prepare_vsix_prebuilt_payload.py",
            "vscode-extension/scripts/check-vsix-prebuilt-payload.test.js",
            "vscode-extension/src/test/vsixPackageProjection.test.ts",
        ],
        "known_limitations": [
            "offline binding only: production package-vsix.js still "
            "rejects payload v2 on main, so packaged selection lands via "
            "carrier PR #16230 (verified OPEN 2026-09-25); that carrier's "
            "disposition stays with #13215/#5888, not this row",
            "no production VSIX packaging, installed Windows/Linux "
            "acceptance, publication, or release qualification is proven "
            "in-unit; synthetic-ZIP verifier proof carries none",
            "frozen-v3 -> prepared-v4 is opt-in and requires both schema "
            "files byte-identical in frozen and prepared roots; legacy "
            "defaults and prerelease admission remain unchanged",
        ],
        "open_pr_relationships": [
            "16230-OPEN carrier: production mapped-RC packaging consumes "
            "this binding (stacked on #16207), verified OPEN 2026-09-25; "
            "disposition owned by #13215/#5888"
        ],
        "controlling_issues": ["14923-OPEN"],
        "invalidators": [
            "carrier #16230 rework or supersession changes packaged "
            "selection",
            "release_topology.v4 or vsix_candidate_payload.v2 schema byte "
            "changes after candidate composition",
            "mapping-law changes (canonical X.Y.Z-rc.N, preRelease=true, "
            "one prepared sourceSha, exact RC asset basename)",
        ],
        "platforms_and_targets": [
            "portable",
            "topology-v4 managed/bundled VSIX target matrix; the verifier "
            "requires native XML platform to match the selected target "
            "and universal packages to omit the attribute",
        ],
        "artifact_or_route_effects": [
            "opt-in offline RC/numeric VSIX identity binding: "
            "release_vsix_mapping validates the supplied "
            "extension/candidate identity against the prepared package "
            "(canonical X.Y.Z-rc.N, preRelease=true, one prepared source "
            "SHA) and derives the exact RC asset basename; never "
            "allocates a version",
            "release topology gains schema v4 with a frozen-v3 -> "
            "prepared-v4 mapped transition law (byte-identical schema "
            "files in both roots, stale selected-schema identity "
            "refused, no implicit v1/v2 upgrades)",
            "vsix-prebuilt-payload verifier gains a mapped route: "
            "expected archive SHA, package/XML identity, prerelease, "
            "payload and native members, semantic inventory",
            "release-history and vscode-prebuilt-payload-adapter "
            "workflows install the pinned release-schema requirements",
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
        "release_note_disposition": "covered",
        "migration_or_upgrade_refs": [
            "upgrade keeps legacy current-pointer resolution for "
            "pre-policy namespaces and rollback; policy namespaces "
            "activate as installs mint candidate manifests and "
            "managed_current_selection.v1 records; pre-policy namespaces "
            "without a selection record are skipped"
        ],
        "api_schema_package_effects": [],
        "proof_owner_refs": [
            "vscode-extension/src/test/downloader.test.ts",
            "vscode-extension/src/test/managedCandidateRuntime.test.ts",
        ],
        "known_limitations": [
            "unit/source stage only; installed-acceptance proof belongs "
            "to #6056 lane",
            "residual scope stays on open issues: #7859 cross-process "
            "GC-vs-launch race matrix, #11540 deletion-boundary "
            "revalidation lease seam, #11539 crashed-session "
            "live-reference recovery (a crashed window pins its "
            "candidate — fail-safe direction, not reclamation)",
        ],
        "open_pr_relationships": [],
        "controlling_issues": ["10083-OPEN"],
        "invalidators": [
            "retention-classification rework in "
            "managedCandidateSelection.ts (policy owner #11780)",
            "#11540 deletion-boundary revalidation changing GC "
            "guarantees",
            "any re-introduction of age/mtime-authorized deletion",
        ],
        "platforms_and_targets": ["vscode-managed"],
        "artifact_or_route_effects": [
            "installs mint an immutable candidate.json manifest from "
            "verified digests and commit a versioned "
            "managed_current_selection.v1 record next to the legacy "
            "current pointer",
            "each extension-host session persists a live host reference "
            "before its server process spawns and releases it at clean "
            "client teardown (a crashed session never releases)",
            "stale-generation cleanup enumerates catalog, selection, and "
            "host references and deletes only stale_unreferenced "
            "candidates per classifyManagedCandidateRetention; "
            "unreadable, malformed, or incomplete evidence blocks "
            "destructive cleanup",
            "the mtime-recency pruneOldVersionedInstalls heuristic is "
            "deleted",
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
            "upgrade: legacy unscoped perl-lsp.lastUpdateCheck seeds the "
            "per-target key once; legacy bin/<platform>-<arch> installs "
            "are revalidated and adopted by reference, never moved or "
            "deleted, so an incompatible host downloads its own candidate"
        ],
        "api_schema_package_effects": [],
        "proof_owner_refs": [
            "vscode-extension/src/test/managedNamespaceIsolation.test.ts",
            "vscode-extension/src/test/managedStorageIdentity.test.ts",
        ],
        "known_limitations": [
            "unit/source stage only; installed-acceptance proof belongs "
            "to #6056 lane",
            "static ELF (no PT_INTERP) yields unknown libc and stays "
            "unadopted — conservative, may cost an extra download",
            "whether a Windows ARM64 release shipped a native ARM64 "
            "asset is a property of the release, not the host; the "
            "emulated x64 namespace remains the fallback row",
            "residual: #10073 (OPEN) still gates managed downloader side "
            "effects on the workspace-host target decision",
        ],
        "open_pr_relationships": [],
        "controlling_issues": ["9847-CLOSED"],
        "invalidators": [
            "managedStorageIdentity key-projection or legacy-adoption "
            "revalidation rework",
            "#10073 landing changing downloader side-effect gating",
            "retention/GC rework that bypasses target-scoped namespaces",
        ],
        "platforms_and_targets": [
            "vscode-managed",
            "per-target namespaces: GNU vs musl Linux distinguished via "
            "PT_INTERP (Bionic separate), Windows ARM64 native vs "
            "x64-emulation as distinct rows",
        ],
        "artifact_or_route_effects": [
            "managed selection moves from bin/<platform>-<arch> to "
            "managed/<compatibility-key>/ with a per-install target.json; "
            "one spelling is both the logical key and the path segment",
            "resolution walks the keys a host may legitimately consume, "
            "most preferred first; an install whose target.json names "
            "another key is not selected",
            "legacy installs revalidate by ELF/PE/Mach-O headers (arch, "
            "libc via PT_INTERP on Linux) before adoption-by-reference; "
            "missing evidence stays unadopted",
            "perl-lsp.lastUpdateCheck becomes "
            "perl-lsp.lastUpdateCheck.<key>; rollback and GC become "
            "target-scoped by living inside the namespace",
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
        "release_domains": ["editor", "dap", "security", "docs"],
        "primary_disposition": "product_behavior",
        "reachable_installed_effect": "bounded",
        "public_claim_refs": [],
        "release_note_disposition": "required",
        "migration_or_upgrade_refs": [
            "upgrade: DAP launches with neither authority nor a "
            "configured boundary now refuse (previously failed open); "
            "editors pass the host workspace folder as trusted root; no "
            "settings migration"
        ],
        "api_schema_package_effects": [
            ".ci/public-api-baselines/perl-dap.txt (refreshed)",
            "crates/perl-dap: sha2 promoted dev-dependency -> dependency "
            "(workspace-pinned)",
        ],
        "proof_owner_refs": [
            "crates/perl-dap tests (dap_launch_security_test.rs, "
            "security_regression_tests.rs, wave_h_external_red_tests.rs)",
            ".ci/dap/editor-transport-inventory.v1.json",
            ".github/workflows/dap-editor-transport.yml",
            "clients/sublime/LSP-perllsp/host_tests",
            "scripts/ci/dap_scorecard_runtime.py",
        ],
        "known_limitations": [
            "parent program #8145 (OPEN) owns enforcement breadth beyond "
            "the launch path and full retirement of the legacy "
            "single-root workspace_root surface",
            "startup without authority inputs does not exit "
            "(boundary-free management flows keep working); only "
            "launches are refused fail-closed",
            "symlink-root rejection is unix-verified in-unit; Windows "
            "symlink behavior unproven",
            "no installed DAP-launch acceptance (real editors, real "
            "debuggees) in-unit; belongs to #6056/#4346 lanes",
        ],
        "open_pr_relationships": [],
        "controlling_issues": ["8656-CLOSED"],
        "invalidators": [
            "#8145 landing reworking admission breadth or retiring the "
            "legacy single-root surface",
            "editor authority pass-through no longer sourced from "
            "host-owned workspace state",
            "public-api baseline drift without regeneration",
        ],
        "platforms_and_targets": [
            "perl-dap server: cross-platform managed bundle",
            "vscode adapter: host workspace root canonicalized (realpath) "
            "before --trusted-root so native symlink rejection does not "
            "refuse symlinked workspaces",
            "sublime and neovim clients: editor-owned workspace folder "
            "becomes the trusted root",
        ],
        "artifact_or_route_effects": [
            "perl-dap gains a typed launch-authority startup contract "
            "(workspace_bound trusted roots | explicit_unbounded "
            "acknowledgement) validated by DapServer::new before any "
            "debuggee process spawns; the two historical fail-open launch "
            "paths are retired fail-closed",
            "launch-args workspaceRoot may only narrow a trusted root; "
            "trusted-root retargeting is rejected; authority identity is "
            "an immutable sha256 over mode, canonical roots, and "
            "acknowledgement",
            "vscode/sublime/neovim clients pass host-owned workspace "
            "authority; launch.json cwd cannot create or widen authority",
            "dap editor-transport inventory and scorecard workflows gate "
            "the client transport; DAP_USER_GUIDE documents Startup "
            "Authority",
        ],
        "editor_manifest_or_protocol_effects": [
            "dap startup contract: launch admitted only under installed "
            "authority; --trusted-root (repeatable) and --allow-unbounded "
            "CLI startup inputs, user/machine-owned sources only"
        ],
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
        check=False,
    )
    if proc.returncode != 0:
        try:
            err = proc.stderr.decode("utf-8", errors="strict").strip()
        except UnicodeDecodeError as exc:
            raise FragmentError(
                f"git {' '.join(args)} failed with undecodable stderr: {exc}"
            ) from exc
        raise FragmentError(f"git {' '.join(args)} failed: {err}")
    try:
        return proc.stdout.decode("utf-8", errors="strict")
    except UnicodeDecodeError as exc:
        raise FragmentError(
            f"git {' '.join(args)} produced undecodable output: {exc}"
        ) from exc


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
    # Complete range enumeration: every merge in START_SHA..OBSERVED_HEAD
    # touching the prefilter, WITHOUT history simplification. A simplified
    # `git log -- paths` hides 72 in-range merges (proven: simplified yields
    # 22, unsimplified yields 94), several carrying merge-unique prefilter
    # content. `--full-history` disables simplification; the range bounds
    # keep the pre-range history (262-94=168 older merges) out of scope by
    # the denominator's own definition. Deterministic: no traversal order
    # leaks into the artifact (rows sort by sha; only counts are stored).
    raw = git(
        "log",
        "--merges",
        "--full-history",
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
        if refs[-1] in PLACEHOLDER_ISSUE_NUMBERS:
            refs = [r for r in refs if r not in PLACEHOLDER_ISSUE_NUMBERS]
            if not refs:
                return "ISS#0000"
            if len(refs) >= 2:
                return f"PR#{refs[-1]}"
            return f"ISS#{refs[0]}"
        return f"PR#{refs[-1]}"
    if len(refs) == 1:
        return f"ISS#{refs[0]}"
    return "NOREF"


def split_unit_id(record: dict, provisional: str) -> str:
    """Replace the placeholder sentinel with a per-commit identity.

    (#0000) carries no issue identity, so two commits sharing that marker
    must never become one unit from title text. Each becomes its own
    COMMIT#<short-sha> unit.
    """
    if provisional == "ISS#0000":
        return f"COMMIT#{record['sha'][:8]}"
    return provisional


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


def merge_parents(sha: str) -> tuple[str, str]:
    parents = git("rev-parse", f"{sha}^1", f"{sha}^2").split()
    if len(parents) != 2:
        raise FragmentError(f"merge {sha} does not have two parents")
    return parents[0], parents[1]


def resolution_files(sha: str, first_parent: str) -> list[str]:
    """Prefilter paths where the merge tree differs from its first parent.

    Empty means the merge contributes zero prefilter content beyond its
    first-parent line (TREESAME): whatever that line carries is either
    mapped (in-range records) or out of scope (pre-range), never new.
    """
    raw = git("diff", "--name-only", first_parent, sha, "--", *PREFILTER)
    return sorted({line for line in raw.splitlines() if line.strip()})


def tree_blob(commit: str, path: str) -> str | None:
    # Absence is proven by ls-tree (empty output), never by a failed
    # rev-parse: any git error below raises instead of masquerading as a
    # deletion. Error direction stays fail-closed toward unit creation.
    listing = git("ls-tree", commit, "--", path)
    if not [line for line in listing.splitlines() if line.strip()]:
        return None
    proc = subprocess.run(
        ["git", "rev-parse", f"{commit}:{path}"],
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        try:
            err = proc.stderr.decode("utf-8", errors="strict").strip()
        except UnicodeDecodeError as exc:
            raise FragmentError(
                f"git rev-parse produced undecodable stderr: {exc}"
            ) from exc
        raise FragmentError(f"git rev-parse {commit}:{path} failed: {err}")
    try:
        return proc.stdout.decode("utf-8", errors="strict").strip()
    except UnicodeDecodeError as exc:
        raise FragmentError(
            f"undecodable blob id for {commit}:{path}: {exc}"
        ) from exc


def range_holders(blob_id: str, path: str) -> list[str]:
    """In-range non-merge commits whose tree holds this exact blob at path."""
    raw = git(
        "log", "--format=%H", f"--find-object={blob_id}",
        f"{START_SHA}..{OBSERVED_HEAD}", "--", path,
    )
    return sorted({line for line in raw.splitlines() if line.strip()})


def subject_shape(subject: str) -> str:
    if subject.startswith(QUEUE_PREFIX):
        return "queue-shaped"
    if SYNC_SUBJECT.match(subject):
        return "sync-shaped"
    return "other-shaped"


def verify_related_remerge(non_merge_shas: set[str]) -> dict:
    """Mechanically prove RELATED_MERGE_984 carries no uncovered content.

    Proven facts (not prose): against fb68dd94 it differs in exactly one
    prefilter file, scripts/ci/validate_gate_lane_mapping.py, whose blob
    sits in the mapped range records 3dc8ade1 (#4976) and b86ba02a
    (#5426) — the re-merge pulls the #4976 line content, which the range
    already maps. All other differences are outside the prefilter and out
    of Domain-6 scope.
    """
    base = "fb68dd94a33424bf5007e686306b07c6bb2096e6"
    target = "scripts/ci/validate_gate_lane_mapping.py"
    holders = {
        "3dc8ade1c8bd1bda6bec61331ccfd5c82cacc66e",
        "b86ba02a1840199d17d621ab36049ba8294e3a69",
    }
    raw = git("diff", "--name-only", base, RELATED_MERGE_984, "--", *PREFILTER)
    files = sorted({line for line in raw.splitlines() if line.strip()})
    if files != [target]:
        raise FragmentError(
            f"related re-merge delta drifted, expected [{target}]: {files}"
        )
    if not holders <= non_merge_shas:
        raise FragmentError(
            "related re-merge holders are not all mapped range records"
        )
    blob_id = tree_blob(RELATED_MERGE_984, target)
    actual = {
        h
        for h in range_holders(blob_id, target)
        if h in non_merge_shas
    }
    if actual != holders:
        raise FragmentError(
            f"related re-merge blob holders drifted: {sorted(actual)}"
        )
    return {
        "sha": RELATED_MERGE_984,
        "subject": "merge: reconcile release lineage into swarm main (#4976)",
        "reason": "related-record",
        "detail": (
            "first-parent-visible re-merge of fb68dd94; prefilter delta "
            "against fb68dd94 is exactly "
            "scripts/ci/validate_gate_lane_mapping.py whose blob sits in "
            "mapped range records 3dc8ade1 (#4976) and b86ba02a (#5426); "
            "recorded as related, never as a separate unit"
        ),
    }


def classify_merges(
    merges: list[dict], non_merge_shas: set[str]
) -> tuple[list[dict], list[dict], list[dict], list[dict]]:
    """Sort every enumerated merge into kept / related / excluded / created.

    - kept: explicit KEEP_MERGES units (file-verified later).
    - related: RELATED_MERGE_984 (mechanically verified, recorded only).
    - excluded content-free: merge tree equals first parent on prefilter
      paths (zero new content by construction).
    - excluded blob-covered: every resolution path's merge blob already
      sits in a mapped range record's tree (covering commits named).
    - created: resolution carries blobs no range record holds (pre-range
      versions or merge deletions) -> new merge units, never silent.
    Anything else fails closed.
    """
    kept = []
    related_rows: list[dict] = []
    excluded: list[dict] = []
    created: list[dict] = []
    stats = {"content_free": 0, "blob_covered": 0}
    seen = set()
    for merge in merges:
        sha, subject = merge["sha"], merge["subject"]
        if sha in seen:
            raise FragmentError(f"merge {sha} enumerated twice")
        seen.add(sha)
        if sha in KEEP_MERGES:
            kept.append(merge)
            continue
        if sha == RELATED_MERGE_984:
            related_rows.append(verify_related_remerge(non_merge_shas))
            continue
        first_parent, _second_parent = merge_parents(sha)
        files = resolution_files(sha, first_parent)
        shape = subject_shape(subject)
        if not files:
            excluded.append(
                {
                    "sha": sha,
                    "subject": subject,
                    "reason": "content-free",
                    "detail": (
                        f"{shape}; merge tree equals first parent on all "
                        "prefilter paths, zero new content"
                    ),
                }
            )
            stats["content_free"] += 1
            continue
        uncovered: list[str] = []
        covered_notes: list[str] = []
        for path in files:
            blob_id = tree_blob(sha, path)
            if blob_id is None:
                uncovered.append(f"{path} (deleted-in-merge)")
                continue
            holders = [
                h for h in range_holders(blob_id, path)
                if h in non_merge_shas
            ]
            if holders:
                covered_notes.append(f"{path}->{sorted(holders)[0][:8]}")
            else:
                uncovered.append(f"{path} (blob-predates-range)")
        if not uncovered:
            excluded.append(
                {
                    "sha": sha,
                    "subject": subject,
                    "reason": "blob-covered",
                    "detail": (
                        f"{shape}; every resolution blob sits in a mapped "
                        "range record: " + "; ".join(covered_notes)
                    ),
                }
            )
            stats["blob_covered"] += 1
            continue
        created.append(
            {
                "sha": sha,
                "subject": subject,
                "files": files,
                "uncovered": uncovered,
                "shape": shape,
            }
        )
    if {m["sha"] for m in kept} != set(KEEP_MERGES):
        raise FragmentError("kept merge set drifted from KEEP_MERGES")
    if len(related_rows) != 1:
        raise FragmentError("related re-merge record missing")
    return kept, related_rows, excluded, created


def build_work_units(records: list[dict]) -> tuple[list[dict], list[dict]]:
    by_unit: dict[str, list[dict]] = {}
    for record in records:
        unit_id = split_unit_id(record, group_unit_id(record["subject"]))
        by_unit.setdefault(unit_id, []).append(record)
    if len(by_unit) != EXPECTED_UNITS:
        raise FragmentError(
            f"work-unit identity count {len(by_unit)} != {EXPECTED_UNITS}"
        )
    if set(by_unit) != {
        split_unit_id(r, group_unit_id(r["subject"])) for r in records
    }:
        raise FragmentError("unit identity mismatch")
    if any(u == "ISS#0000" or u == "PR#0000" for u in by_unit):
        raise FragmentError("placeholder number acts as work-unit identity")
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
        if unit_id.startswith("COMMIT#"):
            evidence = (
                "subject carries placeholder (#0000) with no issue identity; "
                "per-commit unit split from the former ISS#0000 group, "
                "disposition owns verification"
            )
        elif len(subjects) == 1:
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
            if unit_id == "NOREF":
                body_notes = []
                for r in members:
                    refs_in_body = sorted(
                        set(BODY_REF.findall(r["body"]))
                    )
                    if refs_in_body:
                        body_notes.append(
                            f"{r['sha'][:8]}->"
                            + ",".join(f"#{n}" for n in refs_in_body)
                        )
                if body_notes:
                    evidence += (
                        "; body mentions candidate "
                        + "; ".join(body_notes)
                        + " (unverified relationship, not a regroup; "
                        "commits stay NOREF)"
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


def build_merge_units(
    kept: list[dict], created: list[dict], unit_ids: set[str]
) -> list[dict]:
    rows = []
    for merge in sorted(kept, key=lambda m: m["sha"]):
        spec = KEEP_MERGES[merge["sha"]]
        files = commit_files(merge["sha"], merge=True)
        if files != sorted(spec["expected_files"]):
            raise FragmentError(
                f"merge {merge['sha']} effective prefilter file list drifted"
            )
        evidence = (
            "merge kept from unsimplified range enumeration; "
            f"{merge['subject']}"
        )
        if merge["sha"] == "fb68dd94a33424bf5007e686306b07c6bb2096e6":
            evidence += (
                "; first-parent-visible re-merge "
                f"{RELATED_MERGE_984} contributes zero uncovered prefilter "
                "content (single-file delta "
                "scripts/ci/validate_gate_lane_mapping.py resolves to "
                "mapped range records 3dc8ade1 (#4976) and b86ba02a "
                "(#5426), mechanically verified) and is recorded as "
                "related, never as a separate unit"
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
    for item in sorted(created, key=lambda m: m["sha"]):
        unit_id = f"MERGE#{item['sha'][:8]}"
        if unit_id in unit_ids:
            raise FragmentError(f"created merge unit id collides: {unit_id}")
        unit_ids.add(unit_id)
        fragment, basis = route_fragment(item["files"])
        evidence = (
            f"unsimplified-range merge ({item['shape']}) whose resolution "
            f"carries prefilter state no range record holds: "
            + "; ".join(item["uncovered"])
            + "; subject: "
            + item["subject"]
            + "; disposition owns verification"
        )
        rows.append(
            {
                "work_unit_id": unit_id,
                "commits": [item["sha"]],
                "grouping_evidence": evidence,
                "primary_fragment": fragment,
                "primary_fragment_basis": basis,
                "disposition_state": "not_proven",
                "paths_or_components": item["files"],
            }
        )
    if len(created) != EXPECTED_MERGE_CREATED:
        raise FragmentError(
            f"created merge unit count {len(created)} != "
            f"{EXPECTED_MERGE_CREATED}"
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
    if len(merges) != EXPECTED_MERGES:
        raise FragmentError(
            f"enumerated merge count {len(merges)} != {EXPECTED_MERGES}"
        )
    kept, related_rows, excluded, created = classify_merges(
        merges, non_merge_shas
    )
    if len(excluded) != EXPECTED_CONTENT_FREE + EXPECTED_BLOB_COVERED:
        raise FragmentError(
            f"excluded merge count {len(excluded)} != "
            f"{EXPECTED_CONTENT_FREE + EXPECTED_BLOB_COVERED}"
        )
    if (
        len(kept)
        + len(related_rows)
        + len(excluded)
        + len(created)
        != len(merges)
    ):
        raise FragmentError(
            "merge terminal arithmetic broken: "
            f"{len(kept)} kept + {len(related_rows)} related + "
            f"{len(excluded)} excluded + {len(created)} created != "
            f"{len(merges)} enumerated"
        )
    units, noref_watchlist = build_work_units(records)
    merge_units = build_merge_units(
        kept, created, {u["work_unit_id"] for u in units}
    )
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
    merge_commits = sorted(m["commits"][0] for m in merge_units)
    if len(merge_commits) != len(set(merge_commits)):
        raise FragmentError("a merge commit is covered twice")
    if set(merge_commits) & non_merge_shas:
        raise FragmentError("a merge commit collides with a range record")
    if set(merge_commits) != {m["sha"] for m in kept} | {
        c["sha"] for c in created
    }:
        raise FragmentError("merge unit commit set drifted")
    excluded_rows = sorted(
        related_rows + excluded, key=lambda r: r["sha"]
    )
    if {r["sha"] for r in excluded_rows} & (
        set(merge_commits) | non_merge_shas
    ):
        raise FragmentError("an excluded merge collides with mapped commits")
    if (
        len(merge_commits)
        + len(excluded_rows)
        != len(merges)
    ):
        raise FragmentError("merge identity coverage is not exactly-once")
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
        "excluded_merges": excluded_rows,
        "noref_watchlist": noref_watchlist,
        "corrections": [
            "transplant landings 681af39d/f4e8a961/decd96de are PR-keyed "
            "(PR#15577/PR#15582/PR#15586): each commit equals the PR "
            "merge_commit_sha, so the draft ISS#N labels were corrected",
            f"first-parent-visible re-merge {RELATED_MERGE_984} is "
            "related to MERGE#fb68dd94, not a separate unit (prefilter "
            "delta against fb68dd94 is exactly "
            "scripts/ci/validate_gate_lane_mapping.py resolving to mapped "
            "range records 3dc8ade1 (#4976) and b86ba02a (#5426), "
            "mechanically verified)",
            "subject-NOREF commits are 5; body-aware true-NOREF rows are "
            "3; the NOREF unit grouping_evidence names the complete "
            "per-commit body mentions "
            "(6a4e0ea6->#14501,#14549,#14581,#14678,#14680,#14686,#7866; "
            "ca11589a->#4346) as unverified candidates, and both commits "
            "stay NOREF, never regrouped",
            "former ISS#0000 group split: (#0000) carries no identity, so "
            "e7c99232 becomes COMMIT#e7c99232 and fecc9de7 becomes "
            "COMMIT#fecc9de7, each with per-commit placeholder evidence",
            "merge enumeration corrected from the simplified 22 (13 "
            "queue + 7 lineage + 2 kept): default history simplification "
            "hides 72 in-range merges, so the field is renamed to "
            "range_merges_touching_prefilter and the unsimplified range "
            "population is 94 = 2 kept + 1 related-record + 62 "
            "content-free + 11 blob-covered + 18 created merge units; "
            "the 18 created units carry sync-cut resolutions whose blobs "
            "predate the range or whose deletions exist in no record",
            "total_rows 705 = 682 unit-mapping rows + 20 merge rows + 3 "
            "true-NOREF watchlist rows; the 3 watchlist rows intentionally "
            "duplicate commits already mapped once (unique commits 702)",
        ],
        "merge_enumeration": {
            "range_merges_touching_prefilter": len(merges),
            "merge_units_kept": len(kept),
            "related_remerges_recorded": len(related_rows),
            "content_free_merges_excluded": sum(
                1 for r in excluded_rows if r["reason"] == "content-free"
            ),
            "blob_covered_merges_excluded": sum(
                1 for r in excluded_rows if r["reason"] == "blob-covered"
            ),
            "merge_units_created": len(created),
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


def render_bytes(payload: dict) -> bytes:
    # One canonical byte sequence: LF newlines, UTF-8, trailing newline.
    # Callers must compare and write these exact bytes; decoded text
    # equality is not byte identity on Windows (CRLF translation).
    return (
        json.dumps(payload, sort_keys=True, indent=2, ensure_ascii=False)
        + "\n"
    ).encode("utf-8")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Generate the Domain-6 inventory fragment (#16418)."
    )
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument(
        "--check",
        action="store_true",
        help="regenerate and fail when the checked-in artifact bytes "
        "differ (deterministic-render proof)",
    )
    args = parser.parse_args(argv)
    try:
        payload = build_document()
    except FragmentError as exc:
        print(f"domain6 fragment failed: {exc}", file=sys.stderr)
        return 1
    data = render_bytes(payload)
    if b"\r" in data:
        print("domain6 fragment failed: rendered bytes contain CR", file=sys.stderr)
        return 1
    if args.check:
        try:
            expected = args.out.read_bytes()
        except OSError as exc:
            print(f"domain6 fragment --check failed: {exc}", file=sys.stderr)
            return 1
        if b"\r\n" in expected:
            print(
                "domain6 fragment --check failed: checked-in artifact "
                "contains CRLF; canonical bytes are LF-only",
                file=sys.stderr,
            )
            return 2
        if data != expected:
            print(
                "domain6 fragment drifted: regeneration is not "
                "byte-identical to the checked-in artifact",
                file=sys.stderr,
            )
            return 2
        print(f"domain6 fragment deterministic render matches {args.out}")
        return 0
    try:
        args.out.write_bytes(data)
    except OSError as exc:
        print(f"domain6 fragment write failed: {exc}", file=sys.stderr)
        return 1
    print(f"domain6 fragment wrote {args.out}")
    print(f"digest: {payload['coverage']['digest']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
