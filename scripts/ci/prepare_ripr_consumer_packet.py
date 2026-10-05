#!/usr/bin/env python3
"""Collect offline qualification inputs for the frozen RIPR 0.10.0 consumer.

This reads existing Git objects and bounded recursive Git-tree API snapshots.
It never fetches, runs RIPR/Cargo, qualifies a runner, or produces gate evidence.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys


MAX_TREE_BYTES = 8 * 1024 * 1024
MAX_OBJECT_BYTES = 1024 * 1024
KNOWN_BLOBS = {
    ".github/workflows/ripr.yml": "f39dbbda84d264af20a5025c9083d520fa2090de",
    "xtask/src/tasks/ripr_evidence.rs": "daaaefa06dc13e5340ce6a5ba31027fa5239caf3",
    "xtask/src/tasks/quality_gate.rs": "4319363165a78b80bb826299246c6c56da271b5d",
}
INACTIVE_ENTRY_BLOB = "077facaafb2f17c2739b068c09990872d0d1ae45"
MODES = {"040000": "tree", "100644": "blob", "100755": "blob",
         "120000": "blob", "160000": "commit"}


class PacketError(ValueError):
    """Invalid or unsupported collection input; no packet is published."""


def sha(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        raise PacketError("expected a full lowercase SHA-1 object identity")
    return value


def git_hash(kind, content):
    header = kind.encode("ascii") + b" " + str(len(content)).encode("ascii") + b"\0"
    return hashlib.sha1(header + content, usedforsecurity=False).hexdigest()


def unique_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise PacketError("duplicate JSON key")
        result[key] = value
    return result


def read_snapshot(path):
    with Path(path).open("rb") as stream:
        raw = stream.read(MAX_TREE_BYTES + 1)
    if len(raw) > MAX_TREE_BYTES:
        raise PacketError("tree snapshot exceeds the 8 MiB input bound")
    return json.loads(raw, object_pairs_hook=unique_keys), hashlib.sha256(raw).hexdigest()


def verify_tree(snapshot, expected):
    """Reconstruct every Git tree: the API completeness flag is insufficient."""
    if (not isinstance(snapshot, dict) or snapshot.get("sha") != sha(expected)
            or snapshot.get("truncated") is not False
            or not isinstance(snapshot.get("tree"), list)):
        raise PacketError("wrong, missing or truncated recursive tree")
    entries = {}
    for entry in snapshot["tree"]:
        if not isinstance(entry, dict):
            raise PacketError("invalid tree entry")
        path = entry.get("path")
        if (not isinstance(path, str) or "\0" in path or not path
                or any(part in {"", ".", ".."} for part in path.split("/"))):
            raise PacketError("invalid Git path")
        if path in entries:
            raise PacketError("duplicate Git path")
        mode = entry.get("mode")
        if not isinstance(mode, str) or mode not in MODES or entry.get("type") != MODES[mode]:
            raise PacketError("invalid Git mode/type")
        sha(entry.get("sha"))
        if mode != "040000" and entry["type"] == "blob":
            if type(entry.get("size")) is not int or entry["size"] < 0:
                raise PacketError("missing or invalid API-reported blob size")
        entries[path] = entry

    children = {"": []}
    for path, entry in entries.items():
        if entry["type"] == "tree":
            children[path] = []
    for path, entry in entries.items():
        parent, _, name = path.rpartition("/")
        if parent not in children:
            raise PacketError("missing parent subtree")
        children[parent].append((name, entry))
    for path, group in children.items():
        group.sort(key=lambda pair: pair[0].encode("utf-8")
                   + (b"/" if pair[1]["type"] == "tree" else b""))
        raw = b"".join(
            entry["mode"].lstrip("0").encode("ascii") + b" "
            + name.encode("utf-8") + b"\0" + bytes.fromhex(entry["sha"])
            for name, entry in group)
        tree_sha = expected if not path else entries[path]["sha"]
        if git_hash("tree", raw) != tree_sha:
            raise PacketError("Git tree reconstruction mismatch")
    return {path: entry for path, entry in entries.items() if entry["type"] != "tree"}


def git_read(repo, *args):
    # Scope trust to the explicitly selected checkout, never global Git settings.
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
               GIT_NO_LAZY_FETCH="1", GIT_OPTIONAL_LOCKS="0")
    result = subprocess.run(
        ["git", "-c", "safe.directory=" + Path(repo).resolve().as_posix(), "-C", str(repo), *args],
        env=env, capture_output=True, timeout=10, check=False)
    if result.returncode:
        raise PacketError("Git object unavailable; fetch the named input separately")
    return result.stdout


def read_object(repo, kind, identity):
    sha(identity)
    size = int(git_read(repo, "cat-file", "-s", identity).strip())
    if size > MAX_OBJECT_BYTES:
        raise PacketError("Git metadata object exceeds the 1 MiB input bound")
    raw = git_read(repo, "cat-file", kind, identity)
    if len(raw) != size or git_hash(kind, raw) != identity:
        raise PacketError("wrong Git object type or content")
    return raw


def commit(repo, identity):
    raw = read_object(repo, "commit", identity)
    headers = raw.split(b"\n\n", 1)[0].splitlines()
    trees = [line[5:].decode("ascii") for line in headers if line.startswith(b"tree ")]
    parents = [sha(line[7:].decode("ascii")) for line in headers if line.startswith(b"parent ")]
    if len(trees) != 1:
        raise PacketError("invalid commit tree header")
    return sha(trees[0]), parents


def is_config(path):
    return (PurePosixPath(path).name in {"Cargo.toml", "Cargo.lock"}
            or path.startswith((".cargo/", ".config/"))
            or path in {"rust-toolchain.toml", "ripr.toml", "policy/ripr-suppressions.toml"})


def identity(entry):
    return [entry["mode"], entry["type"], entry["sha"]]


def projection(entries):
    blobs = {path: entry for path, entry in entries.items() if entry["type"] == "blob"}
    rust = {path: entry for path, entry in blobs.items()
            if path.endswith(".rs") and entry["mode"] in {"100644", "100755"}}
    rows = [[path, *identity(entry)] for path, entry in sorted(entries.items())]
    encoded = json.dumps(rows, ensure_ascii=True, separators=(",", ":")).encode("ascii")
    return {
        "git_identity_manifest_sha256": hashlib.sha256(encoded).hexdigest(),
        "tracked_blobs": len(blobs),
        "api_reported_blob_bytes": sum(entry["size"] for entry in blobs.values()),
        "gitlinks": sum(entry["type"] == "commit" for entry in entries.values()),
        "symlink_blobs": sum(entry["mode"] == "120000" for entry in blobs.values()),
        "tracked_regular_rust_candidates": len(rust),
        "api_reported_rust_candidate_bytes": sum(entry["size"] for entry in rust.values()),
    }


def proof_sequence():
    # Static recipe bound to KNOWN_BLOBS, not observed phase/resource evidence.
    return [
        {"phase": "freshness_and_build", "requires": "full-history checkout; invocation-specific invalidation handoff; normalized base",
         "host_setup": ["record rustc version",
                        "restore-only Cargo dependencies and version/head-keyed RIPR fact cache",
                        "cargo install ripr --version 0.10.0 --locked",
                        "python3 scripts/tests/test_observe_xtask_build.py",
                        "observe-xtask-build.sh host-initial -- cargo build -p xtask --locked",
                        "ripr doctor"]},
        {"phase": "pr_evidence", "command": "cargo xtask ripr-pr --base B --head E --pr-head H",
         "profile": "existing Docker: 4 CPUs, 14g memory/swap, RIPR_MAX_DIFF_INDEX_FILES=2560",
         "cache_boundary": "host RIPR_CACHE_DIR is not passed or mounted into this container; actual reuse NOT_PROVEN",
         "image": "rust:1.95-slim-bookworm; runtime image ID/digest recorded",
         "host_setup": ["docker pull: TERM at 3m, kill after 10s",
                        "image ID and digest inspections: each TERM at 15s, kill after 5s"],
         "container_setup": ["apt update and build-essential/pkg-config/libssl-dev/git/curl/jq/time install",
                             "cargo install ripr --version 0.10.0 --locked",
                             "record rustc/cargo/ripr versions and binary SHA-256",
                             "observe-xtask-build.sh container -- cargo build -p xtask --locked",
                             "ripr doctor"],
         "analysis_envelope": "TERM at 25m, kill after 30s; /usr/bin/time -v wraps cargo xtask ripr-pr",
         "container_envelope": "TERM at 40m, kill after 30s; includes setup, build, analysis and immediate check",
         "settlement": "EXIT trap: container inspection/removal each TERM at 15s plus 5s kill; target ownership restore has no separate timeout; cleanup failure fails step",
         "immediate_validation": ["nonempty repo-exposure.json",
                                  "cargo xtask ripr-pr --base B --head E --pr-head H --check (no new RIPR analysis)",
                                  "clear-succeeded marker equals invocation freshness token"]},
        {"phase": "baseline", "command": "cargo xtask ripr-plus --receipt target/receipts/quality/ripr-plus.json",
         "condition": "always(); exact memo-hit adjudication precedes generation",
         "fresh_generation_repo_checks": ["repo-badge-json", "repo-seams-json"],
         "exact_memo_hit": "may skip generation only; subsequent byte-comparison check remains required"},
        {"phase": "badge_producer", "condition": "always()",
         "requires": "current evaluated head, root and RIPR_VERSION stamp"},
        {"phase": "guidance", "command": "cargo xtask ripr-review-comments --base B --head E --pr-head H --timeout-seconds 3600",
         "condition": "always()",
         "external_command": "ripr review-comments --root ROOT --base B --head E --out OUTPUT",
         "clean_fast_path": "only exact PR receipt with zero severe gaps",
         "limit_scope": "external child; Cargo, fallback and settlement are not a whole-phase bound"},
        {"phase": "derived_outputs", "commands": ["impacted-evidence", "ripr-pr-summary", "ripr-annotations"],
         "condition": "each always()",
         "advisory": "suppression lifecycle audit"},
        {"phase": "validation", "commands": ["ripr-pr --check", "ripr-plus --check",
         "ripr-review-comments --check", "impacted-evidence --check", "ripr-pr-summary --check", "ripr-annotations --check"],
         "pr_check": "freshness/committed diff/packet validation; no new RIPR analysis",
         "condition": "always(); successful invocation invalidation token required",
         "additional_repo_checks": ["repo-badge-json", "repo-seams-json"]},
        {"phase": "genuine_gap_gate", "commands": ["quality-gate --mode enforce-new-ripr", "quality-gate --mode enforce-new-ripr --check"],
         "condition": "always(); successful invocation invalidation token required",
         "requires": "fresh baseline, PR exposure and head-bound review receipt; existing gate interprets status, completion required for static-limitation credit"},
        {"phase": "summary_and_freshness", "condition": "always()",
         "requires": "current successful invalidation token; ready means invalidation succeeded, not producer completion"},
        {"phase": "proof_export", "artifact": "ripr-pr-evidence", "retention_days": 14,
         "condition": "always() && steps.ripr-freshness-result.outputs.ready == 'true'",
         "proof_boundary": "may export partial diagnostics; validated receipts and completed gate remain separate evidence"},
        {"phase": "diagnostic_export", "artifact": "ripr-xtask-build-observations", "retention_days": 7,
         "condition": "always(); continue-on-error; no proof authority"},
        {"phase": "memo_save", "condition": "success() && produced-fresh marker exists",
         "key": "exact RIPR version/evaluated head/suppression identity; no partial-success writer"},
    ]


def collect(repo, evaluated, base, head, expected_tree, evaluated_snapshot, base_snapshot):
    evaluated, base, head, expected_tree = map(sha, (evaluated, base, head, expected_tree))
    tree, parents = commit(repo, evaluated)
    base_tree, _ = commit(repo, base)
    commit(repo, head)
    if tree != expected_tree or parents != [base, head]:
        raise PacketError("evaluated tree or ordered B/H parents differ from the expected subject")
    e_raw, e_digest = read_snapshot(evaluated_snapshot)
    b_raw, b_digest = read_snapshot(base_snapshot)
    current = verify_tree(e_raw, tree)
    before = verify_tree(b_raw, base_tree)
    for path, blob in KNOWN_BLOBS.items():
        entry = current.get(path, {})
        if entry.get("mode") not in {"100644", "100755"} or entry.get("sha") != blob:
            raise PacketError("unsupported consumer source profile: " + path)
    # Verify exact workflow bytes as well as its roster identity. No lazy fetch.
    read_object(repo, "blob", KNOWN_BLOBS[".github/workflows/ripr.yml"])
    changed = []
    for path in sorted(before.keys() | current.keys()):
        old, new = before.get(path), current.get(path)
        if old is None or new is None or identity(old) != identity(new):
            changed.append({"path": path, "change": "added" if old is None else "deleted" if new is None else "modified",
                            "base": identity(old) if old else None, "evaluated": identity(new) if new else None})
    rust_manifest = [{"path": path, "blob": entry["sha"], "mode": entry["mode"],
                      "api_reported_bytes": entry["size"]}
                     for path, entry in sorted(current.items())
                     if path.endswith(".rs") and entry["mode"] in {"100644", "100755"}]
    configs = [{"path": path, "blob": entry["sha"], "mode": entry["mode"],
                "api_reported_bytes": entry["size"]}
               for path, entry in sorted(current.items()) if entry["type"] == "blob" and is_config(path)]
    return {
        "schema": "ripr-consumer-qualification-inputs/v1",
        "admission_effect": "none", "qualification": "NOT_PROVEN",
        "subject": {"evaluated_head": evaluated, "tree": tree, "base": base,
                    "pr_head": head, "ordered_parents": parents, "base_tree": base_tree},
        "source_profile": {"consumer_ripr_version": "0.10.0", "source_blobs": KNOWN_BLOBS,
                           "lane": "ripr-github (primary hosted); not selfhosted or disk-full fallback",
                           "inactive_entry": {"blob": INACTIVE_ENTRY_BLOB, "ripr_version": "0.10.1"}
                           if current.get(".ci/ripr-proof.sh", {}).get("sha") == INACTIVE_ENTRY_BLOB
                           else "NOT_PROVEN", "upstream_candidate": "NOT_PROVEN"},
        "snapshots": {"evaluated_sha256": e_digest, "base_sha256": b_digest,
                      "roster_identity": "all supplied Git trees reconstructed against commit-bound object IDs",
                      "byte_counts": "API-reported metadata; blob contents/sizes are not verified by tree hashes"},
        "base_inventory": projection(before), "evaluated_inventory": projection(current),
        "changed_paths": changed, "rename_detection": "disabled; complete leaf identity comparison",
        "tracked_regular_rust_manifest": rust_manifest,
        "named_configuration_group": configs,
        "configuration_scope": "named Cargo/.cargo/.config/toolchain/ripr/suppression group; not effective configuration",
        "hosted_job_timeout_minutes": 135,
        "proof_sequence": proof_sequence(),
        "required_runtime_evidence": {
            field: "NOT_PROVEN" for field in (
                "executed_binary_hash_features_toolchain", "effective_configuration_and_exclusions",
                "actual_selected_parsed_analyzed_indexed_files_bytes", "actual_guidance_closure_and_seam_counts",
                "cold_warm_cache_and_index_reuse", "per_phase_cpu_user_system_wall_time",
                "per_phase_direct_child_and_aggregate_peak_rss", "allocation_counts_bytes_and_retained_memory",
                "per_phase_completed_validated_receipts_and_export_digests", "cancellation_and_process_tree_settlement",
                "repeated_workspace_lifecycle", "runner_image_profile_capacity_lifetime_isolation",
                "complete_semantic_parity_and_genuine_gap_verdict", "lost_contact_initiating_cause")},
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, type=Path)
    for name in ("evaluated-head", "base", "pr-head", "expected-tree"):
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--evaluated-tree-json", required=True, type=Path)
    parser.add_argument("--base-tree-json", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        packet = collect(args.repo, args.evaluated_head, args.base, args.pr_head,
                         args.expected_tree, args.evaluated_tree_json, args.base_tree_json)
    except (PacketError, OSError, UnicodeError, json.JSONDecodeError,
            subprocess.TimeoutExpired) as error:
        print("qualification input collection refused: " + str(error), file=sys.stderr)
        return 2
    json.dump(packet, sys.stdout, ensure_ascii=True, sort_keys=True, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
