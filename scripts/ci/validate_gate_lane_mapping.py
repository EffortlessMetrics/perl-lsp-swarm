#!/usr/bin/env python3
"""Validate gate policy mappings and required-gate workflow reachability.

Every gate in .ci/gate-policy.yaml must map to a lane in policy/ci-lanes.toml
(or be explicitly declared as not-mapped). When workflow files are supplied,
every ``required: true`` merge-gate or release gate must also have a statically
visible execution path through a ``gates --tier`` invocation or a workflow gate
matrix. Commit-tier hooks and nightly jobs are intentionally outside this
workflow reachability check.

This is the cross-reference validator: it does not enforce, it reports.
PR 09 of the CI economics rollout.

Usage:
  scripts/ci/validate_gate_lane_mapping.py [--strict] [--workflow PATH ...]

Exit codes:
  0 - all gates map to a lane (or are explicitly not-mapped)
  1 - a strict mapping/reachability invariant failed
"""
from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

# Repository defaults. Named so resolve_docs_target can tell "the caller left
# this alone" from "the caller pointed us at a fixture".
DEFAULT_DOCS = Path("docs/ci/gate-policy-economics.md")
DEFAULT_GATE_POLICY = Path(".ci/gate-policy.yaml")
DEFAULT_LANES = Path("policy/ci-lanes.toml")

# Many-to-one and one-to-many mappings between gate-policy.yaml gate names and
# policy/ci-lanes.toml lane keys. Right-hand side is a list of lane keys (a
# single gate may map to multiple lanes, e.g. an aggregate gate that runs in
# both pr_smoke and merge_gate_shards).
#
# Gates explicitly not yet mapped to a lane carry an empty list and a reason.
GATE_TO_LANE_MAP: dict[str, dict[str, Any]] = {
    # pr_fast gates roll up under pr_smoke
    "fmt": {"lanes": ["pr_smoke"]},
    "check_conflict_markers": {"lanes": ["conflict_markers"]},
    "release_history": {"lanes": ["pr_smoke"]},
    "readme_heading_check": {"lanes": ["pr_smoke"]},
    "publish_closure": {"lanes": ["pr_smoke"]},
    "publish_manifest_check": {"lanes": ["pr_smoke"]},
    "layer_check": {"lanes": ["pr_smoke"]},
    "published_crate_count_pr_fast": {"lanes": ["pr_smoke"]},
    "release_history_check": {"lanes": ["pr_smoke"]},
    "source_commit_api_check": {"lanes": ["pr_smoke", "merge_gate_shards"]},
    # Arrived from the release lineage in the reconciliation merge (#4976). The
    # gate was defined in .ci/gate-policy.yaml there but never mapped here, so
    # this validator failed the moment both files met. tier: pr_fast, and it is
    # listed alongside fmt in the ci-gate group, so it rolls up under pr_smoke.
    "ignored_tests_check_refs": {"lanes": ["pr_smoke"]},
    "clippy_scoped": {"lanes": ["pr_smoke"]},
    "unit_scoped": {"lanes": ["pr_smoke"]},
    "check_tests_scoped": {"lanes": ["pr_smoke"]},
    "unit_routed_full": {"lanes": ["pr_smoke"]},
    # The gate runs inside the existing pr-fast invocation in advisory
    # `pr-smoke`; it is not a separate workflow or receipt-producing lane.
    "clippy_tests_kernel": {"lanes": ["pr_smoke"]},
    # Former `inline_completion_contract` (&&-composite, issue #6845) split
    # into four independent gates.  All four remain in the pr_smoke tier lane.
    "inline_completion_registration": {"lanes": ["pr_smoke"]},
    "lsp_registration_contract": {"lanes": ["pr_smoke"]},
    "lsp_capability_snapshots": {"lanes": ["pr_smoke"]},
    "inline_completion_core": {"lanes": ["pr_smoke"]},
    "inline_completion_quality_receipt": {"lanes": ["pr_smoke"]},

    # core / foundation gates roll up under merge_gate_shards
    "clippy_core": {"lanes": ["merge_gate_shards"]},
    "unit_core": {"lanes": ["merge_gate_shards"]},
    "perl_token_leaf_contract": {"lanes": ["merge_gate_shards"]},
    "clippy_full": {"lanes": ["merge_gate_shards"]},
    "unit_foundation_full": {"lanes": ["merge_gate_shards"]},
    "unit_parser_stack_full": {"lanes": ["merge_gate_shards"]},
    "parser_integration": {"lanes": ["merge_gate_shards"]},
    "parser_behavior_proof": {"lanes": ["merge_gate_shards"]},
    "unit_analysis_full": {"lanes": ["merge_gate_shards"]},
    "unit_lsp_core_full": {"lanes": ["merge_gate_shards"]},
    "unit_lsp_full": {"lanes": ["merge_gate_shards"]},
    "unit_dap_support_full": {"lanes": ["merge_gate_shards"]},
    "compile_all_targets": {"lanes": ["check_all_targets"]},
    "lsp_smoke": {"lanes": ["ux_tests"]},

    # corpus / parser maintenance gates
    "common_corpus_clean": {"lanes": ["merge_gate_shards"]},
    "parser_corpus_ratchet": {"lanes": ["merge_gate_shards"]},
    "cpan_corpus_ratchet": {"lanes": ["merge_gate_shards"]},
    "parser_audit_closeout": {"lanes": ["merge_gate_shards"]},
    "v2_parity": {"lanes": ["merge_gate_shards"]},
    "v2_bundle_sync": {"lanes": ["merge_gate_shards"]},

    # security / policy gates
    "security_audit": {"lanes": ["security_audit"]},
    "policy_checks": {"lanes": ["pr_smoke"]},
    "workflow_audit": {"lanes": ["pr_smoke"]},
    "nested_lock_check": {"lanes": ["pr_smoke"]},
    "agent_context_coverage": {"lanes": ["merge_gate_shards"]},
    "non_rust_inventory_check": {"lanes": ["merge_gate_shards"]},
    "msrv_authority_sync": {"lanes": ["merge_gate_shards"]},
    "compiler_concept_ledger": {"lanes": ["merge_gate_shards"]},
    "compiler_proof_policy": {"lanes": ["merge_gate_shards"]},
    "compiler_concept_proof": {"lanes": ["merge_gate_shards"]},

    # commit-tier staged-tree hygiene (local pre-commit; not CI)
    "staged_tree_identity": {"lanes": ["commit_checks"]},
    "whitespace_check": {"lanes": ["commit_checks"]},
    "conflict_markers_staged": {"lanes": ["commit_checks"]},
    "staged_exec_mode_policy": {"lanes": ["commit_checks"]},
    "staged_config_syntax": {"lanes": ["commit_checks"]},
    "forbidden_machine_paths": {"lanes": ["commit_checks"]},
    "staged_oversized_or_binary": {"lanes": ["commit_checks"]},
    "changie_fragment_staged": {"lanes": ["commit_checks"]},
    "rustfmt_staged": {"lanes": ["commit_checks"]},
    "from_raw_staged": {"lanes": ["commit_checks"]},

    # release-adjacent gates
    "adr_link_check": {"lanes": ["docs_gate"]},
    "docs_build": {"lanes": ["docs_gate"]},
    "release_build": {"lanes": ["release_check"]},
    "inline_completion_binary_smoke": {"lanes": ["release_check"]},
    "version_sync": {"lanes": ["release_check"]},
    "sbom_verify": {"lanes": ["release_check"]},
    "determinism_check": {"lanes": ["release_check"]},

    # nightly deep gates
    "mutation": {"lanes": ["mutation"]},
    "fuzz": {"lanes": ["fuzz"]},
    "benchmarks": {"lanes": ["real_repo_latency"]},
    "full_matrix": {"lanes": ["perl_version_matrix"]},
    "coverage": {"lanes": ["coverage"]},
    "corpus_validation": {"lanes": ["mutation"]},
    "corpus_sweep": {"lanes": ["mutation"]},

    # LSP tier gates
    "lsp_tier_a": {"lanes": ["ux_tests", "real_repo_latency"]},
    "lsp_tier_b": {"lanes": ["real_repo_latency"]},
}


def read_yaml_gate_specs(gate_policy_path: Path) -> dict[str, dict[str, Any]]:
    """Lightweight gate-name extraction without a full YAML dependency.

    Looks for `^  - name: <ident>$` patterns under the `gates:` block. Avoids
    importing pyyaml, which keeps this validator runnable on a clean Python.
    """
    text = gate_policy_path.read_text(encoding="utf-8")
    in_gates = False
    specs: dict[str, dict[str, Any]] = {}
    current: str | None = None
    for line in text.splitlines():
        stripped = line.rstrip()
        if stripped == "gates:":
            in_gates = True
            continue
        if in_gates and stripped and not line.startswith(" "):
            # Hit the next top-level key — end of gates block.
            break
        if in_gates and line.startswith("  - name:"):
            name = line.split("name:", 1)[1].strip()
            if name:
                current = name
                specs[name] = {"tier": None, "required": False}
            continue
        if current is None or not line.startswith("    "):
            continue
        field, separator, value = line.strip().partition(":")
        if not separator:
            continue
        value = value.strip()
        if field == "tier":
            specs[current]["tier"] = value
        elif field == "required":
            specs[current]["required"] = value.lower() == "true"
    return specs


def read_yaml_gate_names(gate_policy_path: Path) -> list[str]:
    """Return gate names, retaining the legacy helper's public behavior."""
    return list(read_yaml_gate_specs(gate_policy_path))


def _workflow_line_indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _workflow_line_is_metadata(line: str) -> bool:
    """Return True when a workflow line cannot execute gate commands.

    Only matrix ``gates:`` declarations and shell ``run`` steps are trusted
    execution contexts. Gate-like substrings in ``name:``, ``env:``, or other
    YAML metadata are not executable paths.
    """
    stripped = line.lstrip()
    if re.match(r"^(?:-\s*)?gates:\s*", stripped):
        return False
    if "run:" in line or stripped.startswith("run "):
        return False
    return True


def _record_workflow_gate_matches(
    line: str,
    *,
    known: set[str],
    gate_specs: dict[str, dict[str, Any]],
    reachable: set[str],
) -> None:
    tier_match = re.search(r"\bgates\s+--tier\s+([a-z0-9_-]+)\b", line)
    if tier_match:
        tier = tier_match.group(1).replace("-", "_")
        reachable.update(
            name for name, spec in gate_specs.items() if spec.get("tier") == tier
        )

    matrix_match = re.match(r"^\s*(?:-\s*)?gates:\s*(.*)$", line)
    if matrix_match:
        reachable.update(
            token
            for token in re.findall(r"[a-z][a-z0-9_]*", matrix_match.group(1))
            if token in known
        )

    direct_match = re.search(r"\bgates\s+--gate\s+([a-z][a-z0-9_]*)\b", line)
    if direct_match and direct_match.group(1) in known:
        reachable.add(direct_match.group(1))


def read_workflow_reachable_gates(
    workflow_paths: list[Path], gate_specs: dict[str, dict[str, Any]]
) -> set[str]:
    """Extract statically visible gate execution paths from workflow YAML.

    The CI workflow has two relevant shapes: a tier invocation for the PR-fast
    tier and explicit ``gates:`` matrix values consumed by ``--gate "$gate"``.
    Dynamic shell variables are intentionally not treated as gate names; the
    matrix values that feed them are the authoritative source in the workflow.
    """
    known = set(gate_specs)
    reachable: set[str] = set()
    for workflow_path in workflow_paths:
        text = workflow_path.read_text(encoding="utf-8")
        in_run_block = False
        run_block_indent = -1
        for raw_line in text.splitlines():
            line = raw_line.split("#", 1)[0]
            if not line.strip():
                continue

            indent = _workflow_line_indent(line)
            if in_run_block and indent <= run_block_indent:
                in_run_block = False

            stripped = line.lstrip()
            block_run = re.match(r"run:\s*[|>][-+]?\s*$", stripped)
            inline_run = (
                re.match(r"run:\s*(.+)$", stripped) if block_run is None else None
            )

            if block_run is not None:
                in_run_block = True
                run_block_indent = indent
                continue

            if inline_run is not None:
                _record_workflow_gate_matches(
                    inline_run.group(1),
                    known=known,
                    gate_specs=gate_specs,
                    reachable=reachable,
                )
                continue

            if in_run_block and indent > run_block_indent:
                _record_workflow_gate_matches(
                    stripped,
                    known=known,
                    gate_specs=gate_specs,
                    reachable=reachable,
                )
                continue

            if _workflow_line_is_metadata(line):
                continue

            _record_workflow_gate_matches(
                stripped,
                known=known,
                gate_specs=gate_specs,
                reachable=reachable,
            )
    return reachable


def read_lane_ids(lanes_path: Path) -> set[str]:
    with lanes_path.open("rb") as f:
        doc = tomllib.load(f)
    return set(doc.get("lane", {}).keys())


def check_docs_drift(docs_path: Path, lane_ids: set[str]) -> list[str]:
    """Check the cross-reference doc's lane enumeration against this mapping.

    The doc names this script as its authority, so the two must agree. It drifted
    twice during #5425 alone -- once by omitting a gate from a lane row, once by
    listing lanes that do not exist -- and a hand-maintained mirror of a machine
    fact will keep drifting. Returns a list of problems; empty means consistent.
    """
    if not docs_path.exists():
        # Fail closed. A guard whose input has vanished must not report success:
        # deleting the doc would otherwise silently satisfy strict mode.
        return [f"{docs_path}: expected cross-reference doc is missing"]

    text = docs_path.read_text(encoding="utf-8")
    problems: list[str] = []

    # Lane -> gates table rows.
    table: dict[str, set[str]] = {}
    for row in re.finditer(r"^\|\s*`([a-z0-9_]+)`\s*\|\s*(.+?)\s*\|\s*$", text, re.M):
        gates = set(re.findall(r"`([a-z0-9_]+)`", row.group(2)))
        if gates:
            table[row.group(1)] = gates

    # Expected lane -> gates, inverted from the mapping table above.
    expected: dict[str, set[str]] = {}
    for gate, spec in GATE_TO_LANE_MAP.items():
        for lane in spec["lanes"]:
            expected.setdefault(lane, set()).add(gate)

    for lane in sorted(set(expected) | set(table)):
        want, have = expected.get(lane, set()), table.get(lane, set())
        for gate in sorted(want - have):
            problems.append(f"{docs_path}: lane `{lane}` row is missing gate `{gate}`")
        for gate in sorted(have - want):
            problems.append(
                f"{docs_path}: lane `{lane}` row lists `{gate}`, which this mapping "
                f"does not assign to it"
            )

    # "Lanes without any gate mapping today" paragraph.
    unmapped_block = re.search(
        r"Lanes without any gate mapping today:(.*?)(?:These either|\n\n)", text, re.S
    )
    if unmapped_block is None:
        # Fail closed: removing the section must not silently disable the
        # phantom-lane and lane-coverage checks it feeds. Treat the list as
        # empty so the checks below still run and report the resulting gaps.
        problems.append(
            f"{docs_path}: the 'Lanes without any gate mapping today:' section is "
            f"missing, so its lane list cannot be checked"
        )
        listed: set[str] = set()
    else:
        listed = set(re.findall(r"`([a-z0-9_]+)`", unmapped_block.group(1)))
        for lane in sorted(listed - lane_ids):
            problems.append(
                f"{docs_path}: `{lane}` is listed as an unmapped lane but is not a "
                f"lane in ci-lanes.toml"
            )

    for lane in sorted(lane_ids - listed - set(table)):
        problems.append(
            f"{docs_path}: lane `{lane}` appears in neither the lane table nor "
            f"the unmapped-lane list"
        )
    total = len(table) + len(listed)
    if total != len(lane_ids):
        problems.append(
            f"{docs_path}: lane enumeration sums to {total} "
            f"({len(table)} mapped + {len(listed)} unmapped) but ci-lanes.toml "
            f"defines {len(lane_ids)}"
        )

    return problems


def resolve_docs_target(
    docs: Path | None, gate_policy: Path, lanes: Path
) -> Path | None:
    """Decide which doc, if any, the lane enumeration should be checked against.

    The doc mirrors the *repository's* gate policy and lane set. Checking it
    against caller-supplied fixtures compares two unrelated things and reports
    drift that does not exist -- which is exactly what happened to
    test_validate_gate_lane_mapping.py, whose temporary gate-policy/lanes files
    made strict mode fail on the real doc.

    So: an explicit --docs is always honoured (the caller asked for it); the
    repository default is used only when both other inputs are also defaults;
    otherwise the caller is working with fixtures and docs validation is out of
    scope. Returning None means "not applicable", which is distinct from "the
    file is missing" -- that is a discrepancy, handled in check_docs_drift.
    """
    if docs is not None:
        return docs
    if gate_policy == DEFAULT_GATE_POLICY and lanes == DEFAULT_LANES:
        return DEFAULT_DOCS
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--docs",
        type=Path,
        default=None,
        help=(
            "Cross-reference doc whose lane enumeration must match this mapping. "
            f"Defaults to {DEFAULT_DOCS} when --gate-policy and --lanes are also "
            "left at their repository defaults; see resolve_docs_target."
        ),
    )
    parser.add_argument("--gate-policy", type=Path, default=DEFAULT_GATE_POLICY)
    parser.add_argument("--lanes", type=Path, default=DEFAULT_LANES)
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit 1 on mapping, documentation, or required-gate reachability failures.",
    )
    parser.add_argument(
        "--workflow",
        dest="workflows",
        action="append",
        type=Path,
        default=[],
        help=(
            "Workflow YAML to inspect for gate execution paths. Repeat for multiple "
            "workflows; required-gate reachability is skipped when omitted."
        ),
    )
    parser.add_argument(
        "--json-out",
        type=Path,
        default=None,
        help="Write the mapping report as JSON to this path.",
    )
    args = parser.parse_args()

    gate_specs = read_yaml_gate_specs(args.gate_policy)
    gate_names = list(gate_specs)
    lane_ids = read_lane_ids(args.lanes)

    unmapped_gates: list[str] = []
    missing_lanes: list[tuple[str, str]] = []
    mapped: list[tuple[str, list[str]]] = []

    for gate in gate_names:
        entry = GATE_TO_LANE_MAP.get(gate)
        if entry is None:
            unmapped_gates.append(gate)
            continue
        lanes_for_gate = entry.get("lanes") or []
        for lane in lanes_for_gate:
            if lane not in lane_ids:
                missing_lanes.append((gate, lane))
        mapped.append((gate, lanes_for_gate))

    print(f"Gates in {args.gate_policy}: {len(gate_names)}")
    print(f"Lanes in {args.lanes}: {len(lane_ids)}")
    print(f"Mapped: {len(mapped)}")
    print(f"Unmapped gates: {len(unmapped_gates)}")
    if unmapped_gates:
        for g in unmapped_gates[:30]:
            print(f"  - {g}")
        if len(unmapped_gates) > 30:
            print(f"  ... and {len(unmapped_gates) - 30} more")
    print(f"Mapped to non-existent lanes: {len(missing_lanes)}")
    for g, l in missing_lanes:
        print(f"  - {g} -> {l}  (lane not in ci-lanes.toml)")

    docs_target = resolve_docs_target(args.docs, args.gate_policy, args.lanes)
    if docs_target is None:
        docs_problems: list[str] = []
        print("Docs lane-enumeration drift: skipped (custom --gate-policy/--lanes)")
    else:
        docs_problems = check_docs_drift(docs_target, lane_ids)
        print(f"Docs lane-enumeration drift: {len(docs_problems)}")
    for problem in docs_problems:
        print(f"  - {problem}")

    required_unreachable: list[str] = []
    if args.workflows:
        reachable = read_workflow_reachable_gates(args.workflows, gate_specs)
        workflow_scoped = {
            name
            for name, spec in gate_specs.items()
            if spec.get("tier") in {"pr_fast", "merge_gate", "release"}
        }
        unreachable = sorted(workflow_scoped - reachable)
        required_unreachable = sorted(
            name
            for name, spec in gate_specs.items()
            if spec.get("required")
            and spec.get("tier") in {"pr_fast", "merge_gate", "release"}
            and name not in reachable
        )
        print(f"Workflow files checked: {len(args.workflows)}")
        print(f"Reachable gates: {len(reachable)}")
        print(f"Unreachable gates: {len(unreachable)}")
        for name in unreachable[:30]:
            print(f"  - {name}")
        if len(unreachable) > 30:
            print(f"  ... and {len(unreachable) - 30} more")
        print(f"Required unreachable gates: {len(required_unreachable)}")
        for name in required_unreachable:
            print(f"  - {name}")
    else:
        print("Workflow reachability: skipped (no --workflow supplied)")

    if args.json_out:
        report = {
            "gate_policy": str(args.gate_policy),
            "lanes": str(args.lanes),
            "gate_count": len(gate_names),
            "lane_count": len(lane_ids),
            "mapped": [{"gate": g, "lanes": l} for g, l in mapped],
            "unmapped_gates": unmapped_gates,
            "missing_lanes": [{"gate": g, "lane": l} for g, l in missing_lanes],
            "workflows": [str(path) for path in args.workflows],
            "required_unreachable_gates": required_unreachable,
        }
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    if args.strict and (
        unmapped_gates or missing_lanes or docs_problems or required_unreachable
    ):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
