#!/usr/bin/env python3
"""Focused tests for scripts/release_build_identity.py."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).with_name("release_build_identity.py")
REPO_ROOT = MODULE_PATH.parent.parent
SPEC = importlib.util.spec_from_file_location(
    "release_build_identity", MODULE_PATH
)
assert SPEC is not None and SPEC.loader is not None
subject = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = subject
SPEC.loader.exec_module(subject)


CROSS_TARGET = "aarch64-unknown-linux-gnu"
OTHER_CROSS_TARGET = "x86_64-unknown-linux-musl"


def write_cross_config(
    root: Path, images: dict[str, str] | None = None
) -> Path:
    """Write a Cross config with an exact passthrough enrollment."""
    if images is None:
        images = {
            CROSS_TARGET: (
                f"{subject.CROSS_IMAGE_REGISTRY}/{CROSS_TARGET}"
                f"@sha256:{'a' * 64}"
            ),
            OTHER_CROSS_TARGET: (
                f"{subject.CROSS_IMAGE_REGISTRY}/{OTHER_CROSS_TARGET}"
                f"@sha256:{'c' * 64}"
            ),
        }
    path = root / subject.CROSS_CONFIG_RELATIVE
    path.parent.mkdir(parents=True, exist_ok=True)
    rendered = "".join(f'  "{key}",\n' for key in subject.IDENTITY_ENV_KEYS)
    targets = "".join(
        f'\n[target.{target}]\nimage = "{image}"\n'
        for target, image in images.items()
    )
    path.write_text(
        f"[build.env]\npassthrough = [\n{rendered}]\n{targets}",
        encoding="utf-8",
    )
    return path


def cross_runner_outputs(calls: int) -> list[mock.Mock]:
    """Host rustc/cross probes for `calls` toolchain_digest invocations."""
    outputs: list[mock.Mock] = []
    for _ in range(calls):
        outputs.append(mock.Mock(stdout=b"rustc 1\n"))
        outputs.append(mock.Mock(stdout=b"cross 1\n"))
    return outputs


def valid_mapping() -> dict[str, str]:
    return {
        "schema_version": subject.INPUT_SCHEMA,
        "repository": "EffortlessMetrics/perl-lsp-swarm",
        "release_version": "0.18.0",
        "source_revision": "a" * 40,
        "source_tree_digest": "b" * 64,
        "target": "x86_64-unknown-linux-gnu",
        "profile": "release",
        "candidate_identity": "v0.18.0-rc1",
        "artifact_role": "archive",
        "product_identity_contract_digest": "c" * 64,
        "release_topology_digest": "d" * 64,
        "toolchain_digest": "e" * 64,
    }


def valid_packet(
    executable: str, package: str, role: str
) -> dict[str, object]:
    identity = valid_mapping()
    return {
        "schema_version": subject.PACKET_SCHEMA,
        "product": {
            "name": "perl-lsp",
            "public_repository": "EffortlessMetrics/perl-lsp",
            "development_repository": "EffortlessMetrics/perl-lsp-swarm",
        },
        "binary": {
            "executable": executable,
            "cargo_package": package,
            "role": role,
            "version": identity["release_version"],
        },
        "build": {
            "source_revision": identity["source_revision"],
            "source_tree_digest": identity["source_tree_digest"],
            "target": identity["target"],
            "profile": "release",
            "identity_state": "exact",
        },
        "artifact": {
            "role": "archive",
            "candidate_identity": identity["candidate_identity"],
        },
        "compatibility": {
            "expected_product_identity_version": 1,
            "dap_posture": "preview",
        },
    }


class ReleaseBuildIdentityTests(unittest.TestCase):
    def test_closed_input_rejects_unknown_fields(self) -> None:
        value = valid_mapping()
        value["forged"] = "accepted"
        with self.assertRaisesRegex(subject.BuildIdentityError, "unknown"):
            subject.ReleaseBuildIdentity.from_mapping(value)

    def test_release_identity_rejects_workspace_or_moving_inputs(self) -> None:
        for field, value in [
            ("source_revision", "main"),
            ("source_tree_digest", "f" * 63),
            ("target", "/tmp/custom-target.json"),
            ("profile", "debug"),
            ("candidate_identity", "../../private"),
            ("artifact_role", "unknown"),
        ]:
            with self.subTest(field=field):
                candidate = valid_mapping()
                candidate[field] = value
                with self.assertRaises(subject.BuildIdentityError):
                    subject.ReleaseBuildIdentity.from_mapping(candidate)

    def test_topology_must_bind_exact_source_target_and_members(self) -> None:
        identity = valid_mapping()
        topology = {
            "schema": 1,
            "release": identity["release_version"],
            "frozen_product_sha": identity["source_revision"],
            "prepared_swarm_sha": None,
            "binary_targets": [
                {
                    "target": identity["target"],
                    "required_members": [
                        "perllsp",
                        "perl-dap",
                        "README.md",
                    ],
                }
            ],
        }
        subject.validate_topology(
            topology,
            release_version=identity["release_version"],
            source_revision=identity["source_revision"],
            target=identity["target"],
        )
        topology["binary_targets"][0]["required_members"].remove(
            "perl-dap"
        )
        with self.assertRaisesRegex(subject.BuildIdentityError, "perl-dap"):
            subject.validate_topology(
                topology,
                release_version=identity["release_version"],
                source_revision=identity["source_revision"],
                target=identity["target"],
            )

    def test_packet_validation_is_load_bearing_for_source_tree(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        packet = valid_packet("perllsp", "perllsp", "server")
        subject.validate_packet(
            packet,
            identity=identity,
            executable="perllsp",
            package="perllsp",
            role="server",
        )
        packet["build"]["source_tree_digest"] = "f" * 64
        with self.assertRaisesRegex(
            subject.BuildIdentityError, "build identity mismatch"
        ):
            subject.validate_packet(
                packet,
                identity=identity,
                executable="perllsp",
                package="perllsp",
                role="server",
            )

    def test_packet_cannot_self_attest_final_executable_digest(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        packet = valid_packet("perl-dap", "perl-dap", "dap")
        packet["artifact"]["digest"] = "f" * 64
        with self.assertRaisesRegex(subject.BuildIdentityError, "self-attest"):
            subject.validate_packet(
                packet,
                identity=identity,
                executable="perl-dap",
                package="perl-dap",
                role="dap",
            )

    def test_prepare_binds_authorities_and_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            product = root / "policy" / "product-identity.toml"
            product.parent.mkdir()
            product.write_text(
                """schema_version = 1
[product]
name = "perl-lsp"
public_repository = "EffortlessMetrics/perl-lsp"
development_repository = "EffortlessMetrics/perl-lsp-swarm"
""",
                encoding="utf-8",
            )
            topology = root / "target" / "release-topology.json"
            topology.parent.mkdir()
            topology.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "release": "0.18.0",
                        "frozen_product_sha": "a" * 40,
                        "prepared_swarm_sha": None,
                        "binary_targets": [
                            {
                                "target": "x86_64-unknown-linux-gnu",
                                "required_members": [
                                    "perllsp",
                                    "perl-dap",
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            output = root / "target" / "identity.json"
            args = argparse.Namespace(
                workspace_root=root,
                repository="EffortlessMetrics/perl-lsp-swarm",
                release_version="0.18.0",
                source_revision="a" * 40,
                target="x86_64-unknown-linux-gnu",
                candidate_identity="v0.18.0-rc1",
                artifact_role="archive",
                release_topology=topology,
                product_identity=product,
                runner="cargo",
                output=output,
                github_env=None,
            )
            with (
                mock.patch.object(subject, "tracked_tree_is_clean"),
                mock.patch.object(
                    subject, "current_revision", return_value="a" * 40
                ),
                mock.patch.object(
                    subject,
                    "canonical_tree_digest",
                    return_value="b" * 64,
                ),
                mock.patch.object(
                    subject, "toolchain_digest", return_value="c" * 64
                ),
            ):
                first = subject.prepare_identity(args)
                first_bytes = output.read_bytes()
                second = subject.prepare_identity(args)
                second_bytes = output.read_bytes()
            self.assertEqual(first, second)
            self.assertEqual(first_bytes, second_bytes)
            self.assertEqual(json.loads(first_bytes), first.as_dict())

    def test_build_commands_are_structured_and_share_one_environment(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        server = subject.build_command(
            "cargo", identity, "perllsp", "perllsp"
        )
        dap = subject.build_command(
            "cargo", identity, "perl-dap", "perl-dap"
        )
        self.assertEqual(server[:2], ["cargo", "build"])
        self.assertEqual(dap[:2], ["cargo", "build"])
        self.assertIn(identity.target, server)
        self.assertIn("--locked", server)
        env = subject.build_environment(identity)
        self.assertEqual(
            env["PERL_LSP_BUILD_REVISION"], identity.source_revision
        )
        self.assertEqual(
            env["PERL_LSP_SOURCE_TREE_DIGEST"],
            identity.source_tree_digest,
        )
        self.assertEqual(
            env["PERL_LSP_CANDIDATE_ID"], identity.candidate_identity
        )
        self.assertEqual(env["PERL_LSP_ARTIFACT_ROLE"], "archive")

    def test_cross_identity_command_executes_the_declared_target(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        command = subject.identity_command(
            "cross",
            identity,
            "perllsp",
            "perllsp",
            Path("/ignored"),
        )
        self.assertEqual(command[0:2], ["cross", "run"])
        self.assertIn(identity.target, command)
        self.assertEqual(command[-2:], ["--", "--identity-json"])

    def test_github_env_projection_is_closed_and_deterministic(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "github.env"
            subject.append_github_env(path, identity)
            first = path.read_text(encoding="utf-8")
            subject.append_github_env(path, identity)
            second = path.read_text(encoding="utf-8")
        expected_lines = [
            f"PERL_LSP_BUILD_REVISION={identity.source_revision}",
            f"PERL_LSP_SOURCE_TREE_DIGEST={identity.source_tree_digest}",
            f"PERL_LSP_TARGET_TRIPLE={identity.target}",
            "PERL_LSP_BUILD_PROFILE=release",
            f"PERL_LSP_CANDIDATE_ID={identity.candidate_identity}",
            "PERL_LSP_ARTIFACT_ROLE=archive",
        ]
        self.assertEqual(first.splitlines(), expected_lines)
        self.assertEqual(second.splitlines(), expected_lines + expected_lines)

    def test_cross_config_enrolls_all_six_identity_vars(self) -> None:
        config = REPO_ROOT / subject.CROSS_CONFIG_RELATIVE
        subject.validate_cross_config(config, CROSS_TARGET)
        passthrough = subject.load_cross_passthrough(config)
        self.assertEqual(passthrough, list(subject.IDENTITY_ENV_KEYS))
        self.assertEqual(len(passthrough), 6)

    def test_cross_config_rejects_missing_identity_var(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / subject.CROSS_CONFIG_RELATIVE
            path.parent.mkdir(parents=True)
            incomplete = list(subject.IDENTITY_ENV_KEYS)[:-1]
            rendered = "\n".join(f'  "{key}",' for key in incomplete)
            path.write_text(
                f"[build.env]\npassthrough = [\n{rendered}\n]\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                subject.BuildIdentityError, "missing="
            ):
                subject.validate_cross_config(path, CROSS_TARGET)

    def test_cross_runner_exports_cross_config_for_build_and_verify(
        self,
    ) -> None:
        mapping = valid_mapping()
        # A cross build must name a target the reviewed config actually pins.
        mapping["target"] = CROSS_TARGET
        identity = subject.ReleaseBuildIdentity.from_mapping(mapping)
        env = subject.build_environment(
            identity, root=REPO_ROOT, runner="cross"
        )
        expected = str(
            (REPO_ROOT / subject.CROSS_CONFIG_RELATIVE).resolve(strict=True)
        )
        self.assertEqual(env["CROSS_CONFIG"], expected)
        for key in subject.IDENTITY_ENV_KEYS:
            self.assertIn(key, env)

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "github.env"
            subject.append_github_env(
                path, identity, runner="cross", root=REPO_ROOT
            )
            lines = path.read_text(encoding="utf-8").splitlines()
        self.assertEqual(lines[-1], f"CROSS_CONFIG={expected}")
        self.assertTrue(
            all(
                any(line.startswith(f"{key}=") for line in lines)
                for key in subject.IDENTITY_ENV_KEYS
            )
        )

    def test_cross_toolchain_digest_binds_reviewed_config_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = write_cross_config(root)
            with mock.patch.object(
                subject, "run", side_effect=cross_runner_outputs(2)
            ):
                first = subject.toolchain_digest(root, "cross", CROSS_TARGET)
                config.write_text(
                    config.read_text(encoding="utf-8") + "# touch\n",
                    encoding="utf-8",
                )
                # Config no longer exact after comment? Order still exact;
                # bytes changed so digest must move. Re-validate still passes.
                second = subject.toolchain_digest(root, "cross", CROSS_TARGET)
            self.assertNotEqual(first, second)

    def test_cross_toolchain_digest_moves_with_pinned_image_digest(
        self,
    ) -> None:
        """#7534: the container that runs the compiler must bind the digest.

        Host `rustc --version --verbose` and the cross client version describe
        the host, not the container supplying the compiler, linker and sysroot.
        Selecting a different immutable image must move `toolchain_digest`.
        """
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = write_cross_config(root)
            with mock.patch.object(
                subject, "run", side_effect=cross_runner_outputs(2)
            ):
                first = subject.toolchain_digest(root, "cross", CROSS_TARGET)
                config.write_text(
                    config.read_text(encoding="utf-8").replace(
                        "a" * 64, "b" * 64
                    ),
                    encoding="utf-8",
                )
                second = subject.toolchain_digest(root, "cross", CROSS_TARGET)
            self.assertNotEqual(first, second)

    def test_cross_toolchain_digest_separates_targets_sharing_one_config(
        self,
    ) -> None:
        """Two cross targets read the same file but select different images."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_cross_config(root)
            with mock.patch.object(
                subject, "run", side_effect=cross_runner_outputs(2)
            ):
                first = subject.toolchain_digest(root, "cross", CROSS_TARGET)
                second = subject.toolchain_digest(
                    root, "cross", OTHER_CROSS_TARGET
                )
            self.assertNotEqual(first, second)

    def test_cargo_runner_digest_ignores_cross_image_pins(self) -> None:
        """A non-cross build must not depend on the container config."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = write_cross_config(root)
            with mock.patch.object(
                subject,
                "run",
                side_effect=[
                    mock.Mock(stdout=b"rustc 1\n"),
                    mock.Mock(stdout=b"cargo 1\n"),
                    mock.Mock(stdout=b"rustc 1\n"),
                    mock.Mock(stdout=b"cargo 1\n"),
                ],
            ):
                first = subject.toolchain_digest(
                    root, "cargo", "x86_64-unknown-linux-gnu"
                )
                config.write_text(
                    config.read_text(encoding="utf-8").replace(
                        "a" * 64, "b" * 64
                    ),
                    encoding="utf-8",
                )
                second = subject.toolchain_digest(
                    root, "cargo", "x86_64-unknown-linux-gnu"
                )
            self.assertEqual(first, second)

    def test_cross_image_override_names_follow_cross_naming(self) -> None:
        names = subject.cross_image_override_names(CROSS_TARGET)
        self.assertEqual(
            names,
            (
                "CROSS_TARGET_AARCH64_UNKNOWN_LINUX_GNU_IMAGE",
                "CROSS_TARGET_AARCH64_UNKNOWN_LINUX_GNU_DOCKERFILE",
                "CROSS_BUILD_DOCKERFILE",
            ),
        )

    def test_ambient_image_override_cannot_bypass_the_pin(self) -> None:
        """#7534: an env override selects the container, `Cross.toml` does not.

        Cross reads image selection from the environment before `Cross.toml`
        (0.2.5 `src/config.rs::string_from_config` returns the env value and
        never consults the TOML), and a dockerfile override replaces the
        pulled image entirely. Either would run a container the digest does
        not describe, so the digest must refuse to be computed at all.
        """
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_cross_config(root)
            for name in subject.cross_image_override_names(CROSS_TARGET):
                for value in (
                    "ghcr.io/attacker/evil@sha256:" + "b" * 64,
                    "",  # cross reads an empty value as a selection too
                ):
                    with self.subTest(variable=name, value=value):
                        with mock.patch.object(
                            subject, "run", side_effect=cross_runner_outputs(1)
                        ):
                            with self.assertRaisesRegex(
                                subject.BuildIdentityError,
                                "ambient cross image override",
                            ):
                                subject.toolchain_digest(
                                    root,
                                    "cross",
                                    CROSS_TARGET,
                                    env={name: value},
                                )

    def test_override_for_another_target_does_not_block_this_one(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_cross_config(root)
            slug = OTHER_CROSS_TARGET.upper().replace("-", "_")
            other = f"CROSS_TARGET_{slug}_IMAGE"
            with mock.patch.object(
                subject, "run", side_effect=cross_runner_outputs(1)
            ):
                digest = subject.toolchain_digest(
                    root, "cross", CROSS_TARGET, env={other: "x"}
                )
            self.assertEqual(len(digest), 64)

    def test_cargo_runner_ignores_cross_image_overrides(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_cross_config(root)
            with mock.patch.object(
                subject,
                "run",
                side_effect=[
                    mock.Mock(stdout=b"rustc 1\n"),
                    mock.Mock(stdout=b"cargo 1\n"),
                ],
            ):
                digest = subject.toolchain_digest(
                    root,
                    "cargo",
                    "x86_64-unknown-linux-gnu",
                    env={"CROSS_BUILD_DOCKERFILE": "Dockerfile"},
                )
            self.assertEqual(len(digest), 64)

    def test_build_environment_strips_overrides_it_controls(self) -> None:
        mapping = valid_mapping()
        mapping["target"] = CROSS_TARGET
        identity = subject.ReleaseBuildIdentity.from_mapping(mapping)
        polluted = dict(os.environ)
        polluted["CROSS_BUILD_DOCKERFILE"] = "Dockerfile.evil"
        with mock.patch.object(subject.os, "environ", polluted):
            with self.assertRaisesRegex(
                subject.BuildIdentityError, "ambient cross image override"
            ):
                subject.build_environment(
                    identity, root=REPO_ROOT, runner="cross"
                )

        clean = {k: v for k, v in os.environ.items()}
        clean.pop("CROSS_BUILD_DOCKERFILE", None)
        with mock.patch.object(subject.os, "environ", clean):
            env = subject.build_environment(
                identity, root=REPO_ROOT, runner="cross"
            )
        for name in subject.cross_image_override_names(CROSS_TARGET):
            self.assertNotIn(name, env)

    def test_cross_config_rejects_unpinned_target(self) -> None:
        """The pre-#7534 config shape must no longer be admitted."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = write_cross_config(root, images={})
            with self.assertRaisesRegex(
                subject.BuildIdentityError, "does not pin a container image"
            ):
                subject.validate_cross_config(config, CROSS_TARGET)

    def test_cross_config_rejects_mutable_tag_pin(self) -> None:
        for mutable in (
            f"ghcr.io/cross-rs/{CROSS_TARGET}:main",
            f"ghcr.io/cross-rs/{CROSS_TARGET}:0.2.5",
            f"ghcr.io/cross-rs/{CROSS_TARGET}",
        ):
            with self.subTest(image=mutable):
                with self.assertRaisesRegex(
                    subject.BuildIdentityError, "immutable"
                ):
                    subject.validate_cross_image(mutable, CROSS_TARGET)

    def test_cross_config_rejects_another_targets_image(self) -> None:
        with self.assertRaisesRegex(
            subject.BuildIdentityError, "another target's image repository"
        ):
            subject.validate_cross_image(
                f"ghcr.io/cross-rs/{OTHER_CROSS_TARGET}@sha256:{'a' * 64}",
                CROSS_TARGET,
            )

    def test_reviewed_cross_config_pins_every_release_cross_target(
        self,
    ) -> None:
        """Every `use_cross: true` row in the live matrix must be pinned."""
        workflow = (
            REPO_ROOT / ".github" / "workflows" / "release.yml"
        ).read_text(encoding="utf-8")
        lines = workflow.splitlines()
        cross_targets = [
            line.split("target:", 1)[1].strip()
            for index, line in enumerate(lines)
            if "- target:" in line
            and "use_cross: true" in "\n".join(lines[index : index + 4])
        ]
        self.assertEqual(len(cross_targets), 3)
        config = REPO_ROOT / subject.CROSS_CONFIG_RELATIVE
        for target in cross_targets:
            with self.subTest(target=target):
                # validate_cross_config returns the image it accepted, so
                # assert on its shape rather than re-reading the same value:
                # the pin must be an immutable digest for this exact target.
                image = subject.validate_cross_config(config, target)
                match = subject.CROSS_IMAGE_PIN.fullmatch(image)
                self.assertIsNotNone(match, image)
                self.assertEqual(
                    match.group("repository"),
                    f"{subject.CROSS_IMAGE_REGISTRY}/{target}",
                )
                self.assertEqual(len(match.group("digest")), 64)

    def test_reviewed_pins_are_not_the_0_2_5_tag_images(self) -> None:
        """Pinning must freeze the container in use, not swap the toolchain.

        `.github/workflows/release.yml` installs cross with
        `cargo install --git --rev`, and a cargo git checkout keeps a usable
        `.git`, so cross's `commit-info.txt` is non-empty and it resolves the
        rolling `:main` tag rather than `:0.2.5` (0.2.5
        `src/docker/shared.rs:668-687`). The `:0.2.5` images differ from
        `:main` for all three targets, so pinning them would silently change
        which container compiles the release binaries.

        These are the recorded `:0.2.5` digests. If a pin ever equals one, the
        pin moved the toolchain instead of freezing it.
        """
        tag_0_2_5 = {
            "aarch64-unknown-linux-gnu": (
                "7f8308a8734d9fcd2ebbe9a3e4bdea74af293f0799d80c3cc341e340cda49a4c"
            ),
            "x86_64-unknown-linux-musl": (
                "77db671d8356a64ae72a3e1415e63f547f26d374fbe3c4762c1cd36c7eac7b99"
            ),
            "aarch64-unknown-linux-musl": (
                "702154f52b2d8091671aa2c84d5582d849f949977228c735ff8462f93cc0e1e4"
            ),
        }
        config = REPO_ROOT / subject.CROSS_CONFIG_RELATIVE
        for target, superseded in tag_0_2_5.items():
            with self.subTest(target=target):
                image = subject.load_cross_image(config, target)
                self.assertNotIn(superseded, image)

    def test_release_workflow_declares_all_three_cross_rows(self) -> None:
        workflow = (REPO_ROOT / ".github" / "workflows" / "release.yml").read_text(
            encoding="utf-8"
        )
        self.assertEqual(workflow.count("use_cross: true"), 3)
        self.assertIn('--runner "$BUILD_CMD"', workflow)
        self.assertIn('--github-env "$GITHUB_ENV"', workflow)
        self.assertTrue(
            (REPO_ROOT / subject.CROSS_CONFIG_RELATIVE).is_file()
        )

    def test_partial_packet_is_not_proven_and_never_accepted(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        packet = valid_packet("perllsp", "perllsp", "server")
        packet["build"] = {
            **packet["build"],
            "source_revision": None,
            "source_tree_digest": None,
            "target": None,
            "identity_state": "not_proven",
        }
        packet["limitations"] = [
            "source_revision_not_embedded",
            "source_tree_digest_not_embedded",
            "target_triple_not_embedded",
        ]
        with self.assertRaisesRegex(subject.BuildIdentityError, "build identity mismatch"):
            subject.validate_packet(
                packet,
                identity=identity,
                executable="perllsp",
                package="perllsp",
                role="server",
            )

    def test_verify_receipt_names_external_build_execution(self) -> None:
        identity = subject.ReleaseBuildIdentity.from_mapping(valid_mapping())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            input_path = root / "identity.json"
            receipt_path = root / "receipt.json"
            input_path.write_bytes(
                subject.canonical_json_bytes(identity.as_dict())
            )
            args = argparse.Namespace(
                workspace_root=root,
                input=input_path,
                runner="cargo",
                receipt=receipt_path,
            )
            observed = [
                {"role": "server", "executable": "perllsp"},
                {"role": "dap", "executable": "perl-dap"},
            ]
            with (
                mock.patch.object(subject, "verify_checkout"),
                mock.patch.object(
                    subject,
                    "toolchain_digest",
                    return_value=identity.toolchain_digest,
                ),
                mock.patch.object(
                    subject, "execute_identity", side_effect=observed
                ),
            ):
                receipt = subject.verify_binaries(
                    args, build_execution="external_release_workflow"
                )
        self.assertEqual(
            receipt["build_execution"], "external_release_workflow"
        )
        self.assertEqual(len(receipt["build_commands"]), 2)
        self.assertEqual(receipt["binaries"], observed)

    def test_atomic_write_creates_missing_parent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "nested" / "receipt.json"
            subject.write_atomic(path, b'{"ok":true}\n')
            self.assertEqual(path.read_bytes(), b'{"ok":true}\n')


if __name__ == "__main__":
    unittest.main()
