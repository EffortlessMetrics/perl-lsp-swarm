"""Validate retained private bytes with canonical validators before eligibility.

Execution precondition: the caller supplies independently reviewed pinned source
roots inside an externally isolated credential-free worker. This module cannot
prove that isolation or authenticate roots, workflow context, or artifacts. It
executes the canonical topology validator (including its source helper and metadata
acquisition); it is not a safe parser for arbitrary unreviewed source trees.
"""
from dataclasses import dataclass, replace
import hashlib
from pathlib import Path
import shutil
import stat
import sys
from tempfile import TemporaryDirectory

# Imports are from this reviewed validator closure, never from a candidate path.
SCRIPTS = Path(__file__).resolve().parents[1]
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))
import generate_release_topology as topology
import release_terminal_manifest as terminal
from release_topology_json import load_topology_json
import release_publication_admission as admission


@dataclass(frozen=True)
class TopologyContext:
    """Independent source roots plus the captured frozen authority input."""
    frozen_root: Path
    prepared_root: Path
    frozen_topology_bytes: bytes
    frozen_topology_sha256: str


@dataclass(frozen=True)
class VerifiedPrivateBytes:
    """Actual local byte checks; not authenticated provenance or eligibility."""
    terminal_manifest_sha256: str
    topology_sha256: str
    attestation_inventory_sha256: str
    subject_comparison: admission.SubjectComparison


def _digest(raw):
    return hashlib.sha256(raw).hexdigest()


def _snapshot(source: Path, destination: Path):
    """Capture regular candidate files once; canonical path APIs see only this copy."""
    if (not stat.S_ISDIR(source.lstat().st_mode) or source.is_symlink()
            or getattr(source, "is_junction", lambda: False)()):
        raise admission.AdmissionError("candidate root must be a regular directory")
    destination.mkdir()
    for path in sorted(source.rglob("*")):
        mode = path.lstat().st_mode
        target = destination / path.relative_to(source)
        if path.is_symlink() or getattr(path, "is_junction", lambda: False)():
            raise admission.AdmissionError("candidate links are not supported")
        if stat.S_ISDIR(mode):
            target.mkdir(exist_ok=True)
        elif stat.S_ISREG(mode):
            target.parent.mkdir(parents=True, exist_ok=True)
            with path.open("rb") as incoming, target.open("xb") as outgoing:
                shutil.copyfileobj(incoming, outgoing)
        else:
            raise admission.AdmissionError("candidate contains a nonregular file")


def validate_private_candidate(candidate_root: Path, packet: dict, expected: dict,
                               policy_bytes: bytes, context: TopologyContext):
    """Check the captured candidate and real topology laws; return nonqualifying facts.

    The terminal build/tag subject is the prepared commit. The frozen product
    commit is independently retained and checked by canonical topology transition
    validation. Open VSX and complete VSIX have no acceptance adapters here.
    """
    admission._packet(packet)
    admission._packet(expected)
    if _digest(policy_bytes) != expected["policy"]["sha256"]:
        raise admission.AdmissionError("trusted policy bytes differ from expected digest")
    policy = admission.parse_object(policy_bytes)
    selection = admission.derive_selection(policy, expected["policy"]["profile_id"],
                                          expected["subject"]["prerelease"], packet["channels"])
    comparison = admission.validate_subject(packet, expected, selection)
    if _digest(context.frozen_topology_bytes) != context.frozen_topology_sha256:
        raise admission.AdmissionError("frozen topology bytes differ from expected digest")
    frozen = load_topology_json(context.frozen_topology_bytes.decode("utf-8"),
                                supported_versions=(1, 2, 3))
    with TemporaryDirectory(prefix="perl-14406-candidate-") as temporary:
        workspace = Path(temporary)
        captured = workspace / "candidate"
        _snapshot(candidate_root, captured)
        frozen_path = workspace / "frozen-topology.json"
        frozen_path.write_bytes(context.frozen_topology_bytes)
        prepared_sha = expected["subject"]["prepared_sha"]
        tag = expected["subject"]["tag"]
        if expected["producer"]["head_sha"] != prepared_sha:
            raise admission.AdmissionError("producer head differs from prepared build commit")
        output, inventory = terminal.check_outputs(captured, prepared_sha, tag)
        manifest_bytes = output.read_bytes()
        manifest = admission.parse_object(manifest_bytes)
        if _digest(manifest_bytes) != expected["terminal_manifest_sha256"]:
            raise admission.AdmissionError("terminal manifest bytes differ from expected digest")
        if manifest["version"] != expected["subject"]["version"]:
            raise admission.AdmissionError("terminal version differs from expected version")
        topology_digest = expected["subject"]["topology_sha256"]
        if manifest["release_topology_sha256"] != topology_digest:
            raise admission.AdmissionError("terminal topology differs from expected digest")
        paths = sorted((captured / "evidence").rglob("release-topology.json"))
        if not paths:
            raise admission.AdmissionError("candidate topology absent")
        prepared_bytes = paths[0].read_bytes()
        if _digest(prepared_bytes) != topology_digest:
            raise admission.AdmissionError("captured topology differs from expected digest")
        prepared = load_topology_json(prepared_bytes.decode("utf-8"), supported_versions=(1, 2, 3))
        topology.validate_manifest(
            prepared, context.prepared_root,
            expected_sha=expected["subject"]["source_sha"],
            expected_prepared_sha=prepared_sha,
            frozen_topology=frozen,
            frozen_topology_digest=context.frozen_topology_sha256,
            frozen_topology_path=frozen_path,
            frozen_root=context.frozen_root,
        )
        if prepared["release"] != manifest["version"]:
            raise admission.AdmissionError("canonical topology and terminal version differ")
        selected_targets = {row["target"] for row in prepared["binary_targets"]}
        archived_targets = {row["target"] for row in manifest["archives"]}
        evidence_targets = set(manifest["build_evidence"]["targets"])
        if selected_targets != archived_targets or selected_targets != evidence_targets:
            raise admission.AdmissionError("candidate archives/evidence omit canonical topology targets")
        # Local canonical byte/shape checks replace only their two missing entries.
        # Filesystem/worker isolation and platform provenance remain external duties.
        missing = tuple(item for item in comparison.missing_adapters
                        if item not in ("canonical_topology", "terminal_candidate_bytes"))
        missing += ("isolated_worker_provenance", "artifact_authentication", "immutable_tag_authority")
        return VerifiedPrivateBytes(_digest(manifest_bytes), topology_digest,
                                    _digest(inventory.read_bytes()),
                                    replace(comparison, missing_adapters=missing))
