"""Private producer facts using canonical byte checks; never release authority.

Workflow context is asserted input, not authenticated provenance. The planned
tag is a private label. No tag lookup, publisher or network operation occurs.
"""
import argparse
import hashlib
from pathlib import Path
import re
import sys
import subprocess
from tempfile import TemporaryDirectory

import release_private_candidate as private


def source_context(directory, expected_digest, repository, artifact_run_id, artifact_name):
    """Admit captured unqualified context; this never authenticates its authority."""
    private.admission._hex(expected_digest, 64, "source context digest")
    if type(artifact_run_id) is not int or artifact_run_id <= 0 or not artifact_name:
        raise private.admission.AdmissionError("source context artifact identity missing")
    with TemporaryDirectory(prefix="perl-16762-context-") as temporary:
        captured = Path(temporary) / "context"
        private._snapshot(directory, captured)
        if sorted(p.name for p in captured.iterdir()) != ["context.json", "frozen-topology.json"]:
            raise private.admission.AdmissionError("source context artifact membership differs")
        raw = (captured / "context.json").read_bytes()
        if hashlib.sha256(raw).hexdigest() != expected_digest:
            raise private.admission.AdmissionError("source context digest mismatch")
        context = private.admission.parse_object(raw)
        private.admission._object(context, ("schema_version", "repository", "artifact_run_id",
            "artifact_name", "frozen_sha", "prepared_sha", "frozen_topology_sha256", "vsix_mapping", "targets"), "source context")
        if raw != private.terminal.canonical(context):
            raise private.admission.AdmissionError("source context JSON is not canonical")
        if context["schema_version"] != "private_source_context.v1":
            raise private.admission.AdmissionError("unsupported source context schema")
        if (context["repository"] != repository or type(context["artifact_run_id"]) is not int
                or context["artifact_run_id"] != artifact_run_id or context["artifact_name"] != artifact_name):
            raise private.admission.AdmissionError("source context repository/run/artifact mismatch")
        for field in ("frozen_sha", "prepared_sha"):
            private.admission._hex(context[field], 40, field)
        if context["frozen_sha"] == context["prepared_sha"]:
            raise private.admission.AdmissionError("source context requires distinct frozen/prepared commits")
        frozen = (captured / "frozen-topology.json").read_bytes()
        private.admission._hex(context["frozen_topology_sha256"], 64, "frozen topology digest")
        if hashlib.sha256(frozen).hexdigest() != context["frozen_topology_sha256"]:
            raise private.admission.AdmissionError("frozen topology digest mismatch")
        frozen_manifest = private.load_topology_json(frozen, supported_versions=(3, 4))
        private.topology.schema_validate(frozen_manifest)
        if frozen_manifest.get("frozen_product_sha") != context["frozen_sha"] or frozen_manifest.get("prepared_swarm_sha") is not None:
            raise private.admission.AdmissionError("frozen topology source role mismatch")
        mapping = context["vsix_mapping"]
        private.admission._object(mapping, ("extension", "candidate", "preRelease"), "VSIX mapping")
        private.admission._object(mapping["extension"], ("id", "version", "sourceSha"), "extension mapping")
        private.admission._object(mapping["candidate"], ("id", "release", "sourceSha"), "candidate mapping")
        for role in ("extension", "candidate"):
            for field, value in mapping[role].items():
                private.admission._text(value, role + " " + field)
        if mapping["preRelease"] is not True or any(mapping[role]["sourceSha"] != context["prepared_sha"] for role in ("extension", "candidate")):
            raise private.admission.AdmissionError("mapping prepared source mismatch")
        targets = context["targets"]
        if type(targets) is not list or not targets or any(type(target) is not str or not re.fullmatch(r"[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+)+", target) for target in targets) or targets != sorted(set(targets)):
            raise private.admission.AdmissionError("independent target mapping malformed")
        return context, frozen


def validate_v4_sources(captured, manifest, context, frozen_bytes, frozen_root, prepared_root):
    """Reach the canonical prepared transition and exact archive target denominator."""
    if private.topology.git_head(frozen_root) != context["frozen_sha"] or private.topology.git_head(prepared_root) != context["prepared_sha"]:
        raise private.admission.AdmissionError("declared source checkout mismatch")
    paths = sorted((captured / "evidence").rglob("release-topology.json"))
    if not paths:
        raise private.admission.AdmissionError("candidate topology missing")
    raw = paths[0].read_bytes()
    if any(path.read_bytes() != raw for path in paths):
        raise private.admission.AdmissionError("candidate topology bytes differ between targets")
    prepared = private.load_topology_json(raw, supported_versions=(4,))
    frozen = private.load_topology_json(frozen_bytes, supported_versions=(3, 4))
    with TemporaryDirectory(prefix="perl-16762-frozen-") as temporary:
        authority = Path(temporary) / "frozen-topology.json"
        authority.write_bytes(frozen_bytes)
        private.topology.validate_manifest(prepared, prepared_root,
            expected_sha=context["frozen_sha"], expected_prepared_sha=context["prepared_sha"],
            frozen_topology=frozen, frozen_topology_digest=context["frozen_topology_sha256"],
            frozen_topology_path=authority, frozen_root=frozen_root, vsix_mapping=context["vsix_mapping"])
    selected = {row["target"] for row in prepared["binary_targets"]}
    if selected != set(context["targets"]):
        raise private.admission.AdmissionError("independent target mapping differs from canonical topology")
    if selected != {row["target"] for row in manifest["archives"]} or selected != set(manifest["build_evidence"]["targets"]):
        raise private.admission.AdmissionError("candidate target membership differs from canonical topology")
    if prepared["release"] != manifest["version"]:
        raise private.admission.AdmissionError("candidate version differs from canonical topology")


def observe(candidate, source_sha, expected_sha, planned_tag, transaction_id,
            repository, workflow_ref, run_id, run_attempt, policy_bytes, expected_policy_sha256,
            expected_topology_sha256, expected_run_attempt, prerelease, *,
            context_directory=None, context_digest=None, context_run_id=None, context_artifact_name=None,
            frozen_root=None, prepared_root=None, admitted_source_executor=None):
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha) or source_sha != expected_sha:
        raise private.admission.AdmissionError("private producer source mismatch")
    if not re.fullmatch(r"[0-9a-f]{64}", transaction_id):
        raise private.admission.AdmissionError("private transaction identity is malformed")
    if not repository or not workflow_ref.startswith(repository + "/.github/workflows/release.yml@"):
        raise private.admission.AdmissionError("private producer workflow/repository mismatch")
    if type(run_id) is not int or type(run_attempt) is not int or min(run_id, run_attempt) <= 0:
        raise private.admission.AdmissionError("private producer run/attempt is malformed")
    if type(expected_run_attempt) is not int or run_attempt != expected_run_attempt:
        raise private.admission.AdmissionError("private producer attempt mismatch")
    private.admission._hex(expected_policy_sha256, 64, "expected policy digest")
    private.admission._hex(expected_topology_sha256, 64, "expected topology digest")
    if hashlib.sha256(policy_bytes).hexdigest() != expected_policy_sha256:
        raise private.admission.AdmissionError("private producer policy digest mismatch")
    policy = private.admission.parse_object(policy_bytes)
    channels = private.admission.derive_selection(policy, "public-beta-rc", prerelease, ["github_release"])
    if context_directory is None or frozen_root is None or prepared_root is None:
        raise private.admission.AdmissionError("source context acquisition NOT_PROVEN")
    context, frozen_bytes = source_context(context_directory, context_digest, repository,
                                           context_run_id, context_artifact_name)
    if context["prepared_sha"] != source_sha:
        raise private.admission.AdmissionError("producer source differs from declared prepared source")
    with TemporaryDirectory(prefix="perl-16762-private-") as temporary:
        captured = Path(temporary) / "candidate"
        private._snapshot(candidate, captured)
        manifest_path, inventory_path = private.terminal.check_outputs(captured, source_sha, planned_tag)
        manifest = private.terminal.build_manifest(captured, source_sha, planned_tag)
        if manifest['release_topology_sha256'] != expected_topology_sha256:
            raise private.admission.AdmissionError("private producer topology digest mismatch")
        if private.topology.git_head(frozen_root) != context["frozen_sha"] or private.topology.git_head(prepared_root) != context["prepared_sha"]:
            raise private.admission.AdmissionError("declared source checkout mismatch")
        ancestry = subprocess.run(["git", "-C", str(prepared_root), "merge-base", "--is-ancestor",
                                  context["frozen_sha"], context["prepared_sha"]], capture_output=True)
        if ancestry.returncode != 0:
            raise private.admission.AdmissionError("frozen source ancestry not established")
        if set(context["targets"]) != {row["target"] for row in manifest["archives"]}:
            raise private.admission.AdmissionError("independent target mapping differs from terminal candidate")
        # No CLI/workflow input can grant execution of helpers from declared roots.
        # Only a separately admitted reviewed-source/isolated-worker adapter may
        # call this canonical closure. The current hosted interface has none.
        if admitted_source_executor is not None:
            admitted_source_executor(captured, manifest, context, frozen_bytes, frozen_root, prepared_root)
        return {
            "schema_version": "private_producer_observation.v1",
            "phase": "private_candidate",
            "source_validation": "not_proven",
            "qualification": "not_proven",
            "source_sha": source_sha,
            "planned_tag": planned_tag,
            "transaction_id": transaction_id,
            "producer": {"repository": repository, "workflow_ref": workflow_ref,
                         "run_id": run_id, "run_attempt": run_attempt},
            "policy_sha256": hashlib.sha256(policy_bytes).hexdigest(),
            "topology_sha256": manifest["release_topology_sha256"],
            "terminal_manifest_sha256": private.terminal.digest(manifest_path),
            "inventory_sha256": private.terminal.digest(inventory_path),
            "archive_targets": [row["target"] for row in manifest["archives"]],
            "selected_channels": list(channels),
            "missing_adapters": ["reviewed_source_execution", "credential_free_worker", "trusted_policy_source", "producer_authentication",
                                 "artifact_authentication", "isolated_worker_provenance",
                                 "complete_vsix", "live_controls", "publication_authorization"],
        }


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "inspect-source-context":
        parser = argparse.ArgumentParser(description="Validate unqualified read-only acquisition context")
        parser.add_argument("--directory", type=Path, required=True)
        parser.add_argument("--digest", required=True)
        parser.add_argument("--repository", required=True)
        parser.add_argument("--run-id", type=int, required=True)
        parser.add_argument("--artifact-name", required=True)
        parser.add_argument("--output", type=Path, required=True)
        args = parser.parse_args(sys.argv[2:])
        context, _ = source_context(args.directory, args.digest, args.repository, args.run_id, args.artifact_name)
        args.output.write_text("frozen_sha=" + context["frozen_sha"] + "\nprepared_sha=" + context["prepared_sha"] + "\n", encoding="utf-8")
        return 0
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "policy", "output", "context-directory", "frozen-root", "prepared-root"):
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ("source-sha", "expected-sha", "planned-tag", "transaction-id", "repository", "workflow-ref",
                 "expected-policy-sha256", "expected-topology-sha256", "context-digest", "context-artifact-name"):
        parser.add_argument("--" + name, required=True)
    for name in ("run-id", "run-attempt", "expected-run-attempt", "context-run-id"):
        parser.add_argument("--" + name, type=int, required=True)
    parser.add_argument("--prerelease", choices=("true", "false"), required=True)
    args = parser.parse_args()
    receipt = observe(args.candidate, args.source_sha, args.expected_sha, args.planned_tag,
                      args.transaction_id, args.repository, args.workflow_ref,
                      args.run_id, args.run_attempt, args.policy.read_bytes(), args.expected_policy_sha256,
                      args.expected_topology_sha256, args.expected_run_attempt, args.prerelease == "true",
                      context_directory=args.context_directory, context_digest=args.context_digest,
                      context_run_id=args.context_run_id, context_artifact_name=args.context_artifact_name,
                      frozen_root=args.frozen_root, prepared_root=args.prepared_root)
    args.output.write_bytes(private.terminal.canonical(receipt))
    return 0


if __name__ == "__main__":
    sys.exit(main())
