"""Actual canonical candidate-byte controls; no hosted/authentication claim."""
from copy import deepcopy
import json
import shutil
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch, Mock
import unittest

import release_private_producer as producer
import test_release_private_candidate as fixture


class PrivateProducerTests(unittest.TestCase):
    def test_actual_bytes_and_exact_context_remain_nonqualifying(self):
        with fixture.fixture(schema_version=3) as (candidate, expected, policy, roots), TemporaryDirectory() as directory:
            context_root = Path(directory)
            source = dict(schema_version="private_source_context.v1",
                repository="EffortlessMetrics/perl-lsp-swarm", artifact_run_id=17, artifact_name="context",
                frozen_sha="a" * 40, prepared_sha="b" * 40,
                frozen_topology_sha256=fixture.digest(roots.frozen_topology_bytes),
                vsix_mapping={"extension": {"id": "fixture.perl", "version": "0.18.0", "sourceSha": "b" * 40},
                              "candidate": {"id": "rc", "release": "0.18.0-rc.1", "sourceSha": "b" * 40}, "preRelease": True},
                targets=[fixture.terminal_fixture.TARGET])
            raw = producer.private.terminal.canonical(source)
            (context_root / "context.json").write_bytes(raw)
            (context_root / "frozen-topology.json").write_bytes(roots.frozen_topology_bytes)
            context = dict(candidate=candidate, source_sha=expected['subject']['prepared_sha'],
                           expected_sha=expected['subject']['prepared_sha'],
                           planned_tag=expected['subject']['tag'], transaction_id='a' * 64,
                           repository='EffortlessMetrics/perl-lsp-swarm',
                           workflow_ref='EffortlessMetrics/perl-lsp-swarm/.github/workflows/release.yml@refs/heads/main',
                           run_id=11, run_attempt=2, policy_bytes=policy,
                           expected_policy_sha256=fixture.digest(policy),
                           expected_topology_sha256=expected['subject']['topology_sha256'],
                           expected_run_attempt=2, prerelease=True,
                           context_directory=context_root, context_digest=fixture.digest(raw),
                           context_run_id=17, context_artifact_name="context",
                           frozen_root=roots.frozen_root, prepared_root=roots.prepared_root)
            # Source execution is forbidden without admitted worker/source authority.
            with patch.object(producer.subprocess, "run", return_value=Mock(returncode=0)), patch.object(producer, "validate_v4_sources", side_effect=RuntimeError("unadmitted helper executed")) as helper:
                self.check(context, expected, candidate, context_root, source, raw)
                fixture.require(helper.call_count == 0, "default path executed source helper")

    def test_admitted_owned_v4_callable_checks_transition_and_exact_targets(self):
        module = fixture.topology_fixture.MODULE
        with fixture.topology_fixture.ReleaseTopologyTests().valid_manifest_fixture(schema_version=3) as (root, frozen, frozen_sha):
            schema = module.schema_relative_path(4)
            (root / schema).write_bytes((fixture.topology_fixture.MODULE_PATH.parents[1] / schema).read_bytes())
            package_path = root / "vscode-extension/package.json"
            package = json.loads(package_path.read_text())
            package["publisher"] = "fixture-publisher"
            package_path.write_text(json.dumps(package), encoding="utf-8")
            frozen["sources"]["vscode-extension/package.json"]["sha256"] = module.sha256(package_path)
            helper = "scripts/release_vsix_mapping.py"
            (root / helper).write_bytes((fixture.topology_fixture.MODULE_PATH.parents[1] / helper).read_bytes())
            frozen_root = root.parent / "v4-frozen"
            shutil.copytree(root, frozen_root)
            frozen_bytes = producer.private.terminal.canonical(frozen)
            authority = root.parent / "frozen-authority.json"
            authority.write_bytes(frozen_bytes)
            release, prepared_sha = "0.18.0-rc.7", "b" * 40
            for relative in ("Cargo.toml", "Cargo.lock", "fixture/Cargo.toml"):
                path = root / relative
                path.write_text(path.read_text().replace("0.18.0", release), encoding="utf-8")
            package["version"] = "0.19.7"
            package_path.write_text(json.dumps(package), encoding="utf-8")
            mapping = {"extension": {"id": "fixture-publisher.perl-lsp-rs", "version": "0.19.7", "sourceSha": prepared_sha},
                       "candidate": {"id": "synthetic-rc-seven", "release": release, "sourceSha": prepared_sha}, "preRelease": True}
            metadata = deepcopy(module.cargo_metadata(root))
            metadata["packages"][0]["version"] = release
            head = lambda path: frozen_sha if Path(path).resolve() == frozen_root.resolve() else prepared_sha
            # Owned fixture roots only; acquisition mocked, canonical laws execute.
            with patch.object(producer.private, "topology", module), patch.object(module, "cargo_metadata", return_value=metadata), patch.object(module, "git_head", side_effect=head):
                prepared = module.build_manifest(root, release, frozen_sha, prepared_sha, frozen,
                    fixture.digest(frozen_bytes), authority, frozen_root, schema_version=4, vsix_mapping=mapping)
                captured = root.parent / "captured"
                evidence = captured / "evidence" / "target"
                evidence.mkdir(parents=True)
                topology_path = evidence / "release-topology.json"
                topology_path.write_bytes(producer.private.terminal.canonical(prepared))
                targets = sorted(row["target"] for row in prepared["binary_targets"])
                context = dict(frozen_sha=frozen_sha, prepared_sha=prepared_sha,
                    frozen_topology_sha256=fixture.digest(frozen_bytes), vsix_mapping=mapping, targets=targets)
                manifest = dict(version=release, archives=[{"target": t} for t in targets], build_evidence={"targets": targets})
                call = lambda: producer.validate_v4_sources(captured, manifest, context, frozen_bytes, frozen_root, root)
                call()
                context["targets"] = ["wrong-target"]
                fixture.refuses(call)
                context["targets"] = targets
                manifest["archives"] = []
                fixture.refuses(call)
                manifest["archives"] = [{"target": t} for t in targets]
                wrong = deepcopy(prepared)
                wrong["frozen_product_sha"] = "c" * 40
                topology_path.write_bytes(producer.private.terminal.canonical(wrong))
                fixture.refuses(call)

    def check(self, context, expected, candidate, context_root, source, raw):
        result = producer.observe(**context)
        fixture.require(result['qualification'] == 'not_proven', 'unsigned bytes qualified')
        fixture.require(result['source_validation'] == 'not_proven', 'unadmitted source reported validated')
        noop = producer.observe(**dict(context, admitted_source_executor=lambda *args: None))
        fixture.require(noop['source_validation'] == 'not_proven' and noop['qualification'] == 'not_proven',
                        'no-op executor upgraded a receipt')
        fixture.require(result['terminal_manifest_sha256'] == expected['terminal_manifest_sha256'],
                        'canonical byte identity changed')
        fixture.require(result == producer.observe(**context), 'nondeterministic receipt')
        for field, value in [('expected_sha', 'c' * 40), ('transaction_id', 'bad'),
                             ('run_attempt', 0), ('run_id', True),
                             ('workflow_ref', 'other/.github/workflows/release.yml@main'),
                             ('expected_run_attempt', 1), ('expected_policy_sha256', 'b' * 64),
                             ('expected_topology_sha256', 'b' * 64),
                             ('prerelease', False), ('context_directory', None), ('context_digest', 'e' * 64), ('context_run_id', 18)]:
            changed = dict(context, **{field: value})
            fixture.refuses(lambda: producer.observe(**changed))
        for field, value in [("prepared_sha", "c" * 40), ("targets", ["wrong-target"]), ("artifact_run_id", 18), ("repository", "other/repository"), ("schema_version", "unknown"), ("source_execution_admitted", True)]:
            changed = dict(source, **{field: value})
            changed_raw = producer.private.terminal.canonical(changed)
            (context_root / "context.json").write_bytes(changed_raw)
            fixture.refuses(lambda: producer.observe(**dict(context, context_digest=fixture.digest(changed_raw))))
        (context_root / "context.json").write_bytes(b'{"schema_version":')
        fixture.refuses(lambda: producer.observe(**dict(context, context_digest=fixture.digest(b'{"schema_version":'))))
        (context_root / "context.json").write_bytes(raw)
        with patch.object(producer.subprocess, "run", return_value=Mock(returncode=1)):
            fixture.refuses(lambda: producer.observe(**context))
        with patch.object(producer.private.topology, "git_head", return_value="c" * 40):
            fixture.refuses(lambda: producer.observe(**context))
        archive = next((candidate / 'dist').glob('*.tar.gz'))
        archive.write_bytes(archive.read_bytes() + b'corruption')
        fixture.refuses(lambda: producer.observe(**context))

if __name__ == '__main__':
    unittest.main()
