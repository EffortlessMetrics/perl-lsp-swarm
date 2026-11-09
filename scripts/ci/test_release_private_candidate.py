"""Canonical-validator integration on owned synthetic private candidate bytes.

Only Git/Cargo metadata acquisition is mocked; archive/topology admission is real.
These fixtures do not establish worker isolation, installed binaries or provenance.
"""
from contextlib import contextmanager
import copy
import hashlib
import json
from pathlib import Path
import shutil
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import release_private_candidate as adapter
import test_release_publication_admission as leaf_fixture
import test_release_terminal_manifest as terminal_fixture
import test_generate_release_topology as topology_fixture

require = leaf_fixture.require


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def refuses(call):
    try:
        call()
    except (adapter.admission.AdmissionError, adapter.terminal.ManifestError,
            topology_fixture.MODULE.TopologyError, ValueError):
        return
    raise RuntimeError("corrupted private candidate was accepted")


@contextmanager
def fixture(extra_target=False, schema_version=1):
    with topology_fixture.ReleaseTopologyTests().valid_manifest_fixture(schema_version=schema_version) as (root, frozen, sha):
        if extra_target:
            workflow_path = root / ".github/workflows/release.yml"
            workflow = workflow_path.read_text().replace("        steps:",
                "            - target: x86_64-pc-windows-msvc\n              os: windows-latest\n        steps:")
            workflow_path.write_text(workflow)
            frozen["binary_targets"] = topology_fixture.MODULE.derive_targets(workflow, "0.18.0")
            frozen["archive_count"] = len(frozen["binary_targets"])
            downstream = root / "docs/reference/downstream-dap-integrations.json"
            downstream.write_text(json.dumps({"targets": [{"triple": row["target"]}
                                                          for row in frozen["binary_targets"]]}))
            frozen["sources"]["docs/reference/downstream-dap-integrations.json"]["sha256"] = digest(downstream.read_bytes())
            frozen["vsix"]["managed_targets"] = sorted(row["target"] for row in frozen["binary_targets"])
            frozen["sources"][".github/workflows/release.yml"]["sha256"] = digest(workflow_path.read_bytes())
        frozen_root = root.parent / "frozen"
        shutil.copytree(root, frozen_root)
        frozen_bytes = json.dumps(frozen).encode()
        prepared = copy.deepcopy(frozen)
        prepared["prepared_swarm_sha"] = "b" * 40
        prepared_bytes = json.dumps(prepared).encode()
        with TemporaryDirectory(prefix="perl-14406-proof-") as directory, patch.multiple(
            terminal_fixture, SOURCE="b" * 40, VERSION="0.18.0", TAG="v0.18.0"
        ), patch.object(adapter, "topology", topology_fixture.MODULE), patch.object(
            topology_fixture.MODULE, "git_head", side_effect=lambda path: ("b" if path == root else "a") * 40
        ):
            candidate = terminal_fixture.candidate(Path(directory))
            evidence = candidate / "evidence" / terminal_fixture.TARGET
            (evidence / "release-topology.json").write_bytes(prepared_bytes)
            identity_path = evidence / "release-build-identity.json"
            identity = json.loads(identity_path.read_bytes())
            identity["release_topology_digest"] = digest(prepared_bytes)
            terminal_fixture.write_json(identity_path, identity)
            receipt_path = evidence / "release-build-receipt.json"
            receipt = json.loads(receipt_path.read_bytes())
            receipt["input"] = identity
            receipt["input_sha256"] = digest(adapter.terminal.canonical(identity))
            terminal_fixture.write_json(receipt_path, receipt)
            output, _ = adapter.terminal.write_outputs(candidate, "b" * 40, "v0.18.0")
            expected = leaf_fixture.packet()
            expected["subject"].update(tag="v0.18.0", version="0.18.0",
                                        topology_sha256=digest(prepared_bytes))
            expected["terminal_manifest_sha256"] = digest(output.read_bytes())
            policy_bytes = json.dumps(leaf_fixture.policy()).encode()
            expected["policy"]["sha256"] = digest(policy_bytes)
            context = adapter.TopologyContext(frozen_root, root, frozen_bytes, digest(frozen_bytes))
            yield candidate, expected, policy_bytes, context


def check(args):
    candidate, expected, policy, context = args
    return adapter.validate_private_candidate(candidate, copy.deepcopy(expected), expected, policy, context)


class PrivateCandidateTests(unittest.TestCase):
    def test_actual_canonical_validators_accept_bounded_fixture(self):
        with fixture() as args:
            result = check(args)
            require(result.terminal_manifest_sha256 == args[1]["terminal_manifest_sha256"],
                    "manifest byte identity changed")
            require(result.subject_comparison.qualification == "not_proven", "false eligibility")
            require("complete_vsix" in result.subject_comparison.missing_adapters, "VSIX gap lost")
            require("isolated_worker_provenance" in result.subject_comparison.missing_adapters,
                    "worker authentication fabricated")

    def test_actual_archive_checksum_sbom_and_evidence_corruption_refuse(self):
        for selected in ("archive", "checksum", "sbom", "receipt", "manifest", "inventory", "topology"):
            with fixture() as args:
                candidate = args[0]
                paths = {
                    "archive": next((candidate / "dist").glob("*.tar.gz")),
                    "checksum": candidate / "dist/SHA256SUMS",
                    "sbom": candidate / "dist/sbom-spdx.json",
                    "receipt": candidate / "evidence" / terminal_fixture.TARGET / "release-build-receipt.json",
                    "manifest": candidate / "dist/release-terminal-manifest.json",
                    "inventory": candidate / "attestation-subjects.sha256",
                    "topology": candidate / "evidence" / terminal_fixture.TARGET / "release-topology.json",
                }
                path = paths[selected]
                raw = path.read_bytes()
                require(bool(raw), "fixture unexpectedly empty")
                path.write_bytes(b"!" + raw[1:])
                refuses(lambda: check(args))

    def test_source_corruption_reaches_canonical_topology_validator(self):
        with fixture() as args:
            context = args[3]
            path = context.prepared_root / "vscode-extension/src/downloader.ts"
            path.write_text(path.read_text() + "\n// source drift\n")
            refuses(lambda: check(args))

    def test_frozen_bytes_and_policy_bytes_are_independently_bound(self):
        with fixture() as args:
            candidate, expected, policy, context = args
            refuses(lambda: adapter.validate_private_candidate(candidate, expected, expected,
                                                                policy + b" ", context))
            changed = adapter.TopologyContext(context.frozen_root, context.prepared_root,
                                               context.frozen_topology_bytes + b" ",
                                               context.frozen_topology_sha256)
            refuses(lambda: adapter.validate_private_candidate(candidate, expected, expected,
                                                                policy, changed))

    def test_terminal_build_subject_is_prepared_not_frozen(self):
        with fixture() as args:
            candidate, expected, policy, context = args
            wrong = copy.deepcopy(expected)
            wrong["subject"]["prepared_sha"] = wrong["subject"]["source_sha"]
            wrong["producer"]["head_sha"] = wrong["subject"]["prepared_sha"]
            try:
                adapter.validate_private_candidate(candidate, wrong, wrong, policy, context)
            except adapter.terminal.ManifestError as error:
                require("build identity names another source" in str(error),
                        "wrong refusal masks terminal build subject")
            else:
                raise RuntimeError("terminal accepted frozen rather than prepared build subject")

    def test_snapshot_rejects_root_and_child_links(self):
        for method in ("is_symlink", "is_junction"):
            for root_link in (True, False):
                with TemporaryDirectory(prefix="perl-14406-links-") as directory:
                    root = Path(directory) / "source"
                    root.mkdir()
                    child = root / "ordinary-file"
                    child.write_bytes(b"owned fixture")
                    destination = Path(directory) / "captured"
                    selected = root if root_link else child
                    # Exercise the refusal branch deterministically on all hosts;
                    # this does not prove native link detection or worker isolation.
                    with patch.object(Path, method, autospec=True,
                                      side_effect=lambda path: path == selected):
                        try:
                            adapter._snapshot(root, destination)
                        except adapter.admission.AdmissionError as error:
                            wanted = "candidate root" if root_link else "candidate links"
                            require(wanted in str(error), "wrong link refusal")
                        else:
                            raise RuntimeError("snapshot accepted a declared link")

    def test_captured_candidate_is_the_only_terminal_input(self):
        with fixture() as args:
            candidate = args[0]
            original = adapter.terminal.check_outputs
            def observing(snapshot, source, tag):
                require(snapshot != candidate, "canonical validation rereads live candidate")
                (candidate / "dist/SHA256SUMS").write_bytes(b"changed after capture")
                return original(snapshot, source, tag)
            with patch.object(adapter.terminal, "check_outputs", side_effect=observing):
                check(args)

    def test_real_topology_selected_target_cannot_be_omitted_from_candidate(self):
        with fixture(extra_target=True) as args:
            try:
                check(args)
            except adapter.admission.AdmissionError as error:
                require("omit canonical topology targets" in str(error), "wrong refusal masks join")
            else:
                raise RuntimeError("canonical selected target disappeared from candidate")
