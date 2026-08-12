#!/usr/bin/env python3
"""Build release-shaped perl-lsp binaries with exact embedded identity.

This adapter owns one narrow transition:

    validated release-build identity
    -> perllsp + perl-dap build
    -> post-build canonical packet verification

It does not claim that an executable can attest its own final digest. The
receipt records externally measured build-output bytes; archive/install and
publication consumers bind the later packaged bytes independently.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence

INPUT_SCHEMA = "perl_lsp.release_build_identity.v1"
RECEIPT_SCHEMA = "perl_lsp.release_build_identity_receipt.v1"
PACKET_SCHEMA = "perl_lsp.binary_identity.v1"
ALLOWED_REPOSITORIES = {
    "EffortlessMetrics/perl-lsp",
    "EffortlessMetrics/perl-lsp-swarm",
}
ALLOWED_ARTIFACT_ROLES = {"archive", "managed", "package_install"}
ALLOWED_RUNNERS = {"cargo", "cross"}
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
VERSION = re.compile(
    r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
TOKEN = re.compile(r"^[0-9A-Za-z][0-9A-Za-z_.:@+-]{0,127}$")
TARGET = re.compile(r"^[0-9A-Za-z][0-9A-Za-z_.-]{2,127}$")
EXPECTED_KEYS = {
    "schema_version",
    "repository",
    "release_version",
    "source_revision",
    "source_tree_digest",
    "target",
    "profile",
    "candidate_identity",
    "artifact_role",
    "product_identity_contract_digest",
    "release_topology_digest",
    "toolchain_digest",
}


class BuildIdentityError(ValueError):
    """A release-build identity or verification invariant was not proven."""


@dataclass(frozen=True)
class ReleaseBuildIdentity:
    schema_version: str
    repository: str
    release_version: str
    source_revision: str
    source_tree_digest: str
    target: str
    profile: str
    candidate_identity: str
    artifact_role: str
    product_identity_contract_digest: str
    release_topology_digest: str
    toolchain_digest: str

    @classmethod
    def from_mapping(cls, value: Mapping[str, Any]) -> "ReleaseBuildIdentity":
        unknown = sorted(set(value) - EXPECTED_KEYS)
        missing = sorted(EXPECTED_KEYS - set(value))
        if unknown:
            raise BuildIdentityError(
                f"unknown release-build identity fields: {unknown}"
            )
        if missing:
            raise BuildIdentityError(
                f"missing release-build identity fields: {missing}"
            )
        if not all(isinstance(value[key], str) for key in EXPECTED_KEYS):
            raise BuildIdentityError(
                "every release-build identity field must be a string"
            )

        identity = cls(**{key: value[key] for key in EXPECTED_KEYS})
        identity.validate()
        return identity

    def validate(self) -> None:
        if self.schema_version != INPUT_SCHEMA:
            raise BuildIdentityError(
                f"unsupported release-build identity schema: {self.schema_version!r}"
            )
        if self.repository not in ALLOWED_REPOSITORIES:
            raise BuildIdentityError(
                f"unsupported repository identity: {self.repository!r}"
            )
        if not VERSION.fullmatch(self.release_version):
            raise BuildIdentityError(
                f"invalid release version: {self.release_version!r}"
            )
        if not HEX40.fullmatch(self.source_revision):
            raise BuildIdentityError(
                "source_revision must be a lowercase 40-hex commit"
            )
        if not HEX64.fullmatch(self.source_tree_digest):
            raise BuildIdentityError(
                "source_tree_digest must be a lowercase SHA-256"
            )
        if not TARGET.fullmatch(self.target) or "-" not in self.target:
            raise BuildIdentityError(f"invalid target triple: {self.target!r}")
        if self.profile != "release":
            raise BuildIdentityError(
                "release-shaped builds require profile='release'"
            )
        if not TOKEN.fullmatch(self.candidate_identity):
            raise BuildIdentityError(
                "invalid or oversized candidate identity: "
                f"{self.candidate_identity!r}"
            )
        if self.artifact_role not in ALLOWED_ARTIFACT_ROLES:
            raise BuildIdentityError(
                f"unsupported release artifact role: {self.artifact_role!r}"
            )
        for field_name in (
            "product_identity_contract_digest",
            "release_topology_digest",
            "toolchain_digest",
        ):
            if not HEX64.fullmatch(getattr(self, field_name)):
                raise BuildIdentityError(
                    f"{field_name} must be a lowercase SHA-256"
                )

    def as_dict(self) -> dict[str, str]:
        return {
            "schema_version": self.schema_version,
            "repository": self.repository,
            "release_version": self.release_version,
            "source_revision": self.source_revision,
            "source_tree_digest": self.source_tree_digest,
            "target": self.target,
            "profile": self.profile,
            "candidate_identity": self.candidate_identity,
            "artifact_role": self.artifact_role,
            "product_identity_contract_digest": (
                self.product_identity_contract_digest
            ),
            "release_topology_digest": self.release_topology_digest,
            "toolchain_digest": self.toolchain_digest,
        }


def canonical_json_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n"
    ).encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(
    args: Sequence[str],
    *,
    cwd: Path,
    env: Mapping[str, str] | None = None,
    capture: bool = True,
    timeout: int = 120,
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        list(args),
        cwd=cwd,
        env=None if env is None else dict(env),
        check=True,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        timeout=timeout,
    )


def command_text(result: subprocess.CompletedProcess[bytes], label: str) -> str:
    try:
        text = result.stdout.decode("utf-8", errors="strict")
    except UnicodeDecodeError as error:
        raise BuildIdentityError(f"{label} emitted non-UTF-8 output") from error
    return text.strip()


def require_regular_file(path: Path, label: str) -> Path:
    try:
        resolved = path.resolve(strict=True)
    except OSError as error:
        raise BuildIdentityError(f"{label} is unavailable: {path}") from error
    if not resolved.is_file():
        raise BuildIdentityError(f"{label} is not a regular file: {path}")
    return resolved


def require_within(root: Path, path: Path, label: str) -> Path:
    root_resolved = root.resolve(strict=True)
    resolved = require_regular_file(path, label)
    try:
        resolved.relative_to(root_resolved)
    except ValueError as error:
        raise BuildIdentityError(
            f"{label} escapes the workspace: {path}"
        ) from error
    return resolved


def current_revision(root: Path) -> str:
    value = command_text(
        run(["git", "rev-parse", "HEAD"], cwd=root), "git rev-parse"
    )
    if not HEX40.fullmatch(value):
        raise BuildIdentityError(
            f"git HEAD is not an exact lowercase commit: {value!r}"
        )
    return value


def tracked_tree_is_clean(root: Path) -> None:
    result = run(
        ["git", "status", "--porcelain=v1", "--untracked-files=no"],
        cwd=root,
    )
    if result.stdout:
        raise BuildIdentityError(
            "tracked checkout is dirty; release identity is not exact"
        )


def canonical_tree_digest(root: Path, revision: str) -> str:
    result = run(
        ["git", "ls-tree", "-r", "-z", "--full-tree", revision],
        cwd=root,
    )
    if not result.stdout:
        raise BuildIdentityError("git tree inventory is empty")
    return sha256_bytes(result.stdout)


def toolchain_digest(root: Path, runner: str) -> str:
    rustc = run(["rustc", "--version", "--verbose"], cwd=root).stdout
    runner_version = run([runner, "--version"], cwd=root).stdout
    return sha256_bytes(
        b"rustc\0" + rustc + b"\0runner\0" + runner_version
    )


def load_json_object(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise BuildIdentityError(
            f"{label} is not valid UTF-8 JSON: {path}"
        ) from error
    if not isinstance(value, dict):
        raise BuildIdentityError(f"{label} must be a JSON object")
    return value


def validate_product_contract(path: Path, repository: str) -> None:
    try:
        value = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise BuildIdentityError(
            f"product identity contract is invalid: {path}"
        ) from error
    if value.get("schema_version") != 1:
        raise BuildIdentityError(
            "product identity contract schema_version must be 1"
        )
    product = value.get("product")
    if not isinstance(product, dict):
        raise BuildIdentityError(
            "product identity contract has no [product] table"
        )
    if product.get("name") != "perl-lsp":
        raise BuildIdentityError(
            "product identity contract names another product"
        )
    repositories = {
        product.get("public_repository"),
        product.get("development_repository"),
    }
    if repository not in repositories:
        raise BuildIdentityError(
            "release-build repository is not authorized by product identity "
            "contract"
        )


def validate_topology(
    topology: Mapping[str, Any],
    *,
    release_version: str,
    source_revision: str,
    target: str,
) -> None:
    if topology.get("schema") != 1:
        raise BuildIdentityError("release topology schema must be 1")
    if topology.get("release") != release_version:
        raise BuildIdentityError(
            "release topology version differs from build identity"
        )
    frozen = topology.get("frozen_product_sha")
    prepared = topology.get("prepared_swarm_sha")
    applicable = prepared if prepared is not None else frozen
    if applicable != source_revision:
        raise BuildIdentityError(
            "release topology does not identify the exact build source revision"
        )
    targets = topology.get("binary_targets")
    if not isinstance(targets, list):
        raise BuildIdentityError(
            "release topology has no binary target inventory"
        )
    rows = [
        row
        for row in targets
        if isinstance(row, dict) and row.get("target") == target
    ]
    if len(rows) != 1:
        raise BuildIdentityError(
            f"release target must occur exactly once in topology: {target}"
        )
    required = rows[0].get("required_members")
    if not isinstance(required, list):
        raise BuildIdentityError(
            "release topology target has no required member list"
        )
    suffix = ".exe" if "windows" in target else ""
    for member in (f"perllsp{suffix}", f"perl-dap{suffix}"):
        if member not in required:
            raise BuildIdentityError(
                "release topology target does not ship required binary: "
                f"{member}"
            )


def prepare_identity(args: argparse.Namespace) -> ReleaseBuildIdentity:
    root = args.workspace_root.resolve(strict=True)
    product_path = require_within(
        root, args.product_identity, "product identity contract"
    )
    topology_path = require_within(
        root, args.release_topology, "release topology"
    )
    tracked_tree_is_clean(root)

    observed_revision = current_revision(root)
    if observed_revision != args.source_revision:
        raise BuildIdentityError(
            "declared source revision differs from the exact checkout"
        )
    tree_digest = canonical_tree_digest(root, observed_revision)
    topology = load_json_object(topology_path, "release topology")
    validate_topology(
        topology,
        release_version=args.release_version,
        source_revision=observed_revision,
        target=args.target,
    )
    validate_product_contract(product_path, args.repository)
    runner = args.runner
    if runner not in ALLOWED_RUNNERS:
        raise BuildIdentityError(f"unsupported build runner: {runner!r}")

    identity = ReleaseBuildIdentity(
        schema_version=INPUT_SCHEMA,
        repository=args.repository,
        release_version=args.release_version,
        source_revision=observed_revision,
        source_tree_digest=tree_digest,
        target=args.target,
        profile="release",
        candidate_identity=args.candidate_identity,
        artifact_role=args.artifact_role,
        product_identity_contract_digest=sha256_file(product_path),
        release_topology_digest=sha256_file(topology_path),
        toolchain_digest=toolchain_digest(root, runner),
    )
    identity.validate()
    write_atomic(args.output, canonical_json_bytes(identity.as_dict()))
    if args.github_env is not None:
        append_github_env(args.github_env, identity, runner=runner)
    return identity


def load_identity(path: Path) -> ReleaseBuildIdentity:
    return ReleaseBuildIdentity.from_mapping(
        load_json_object(path, "release-build identity")
    )


def verify_checkout(root: Path, identity: ReleaseBuildIdentity) -> None:
    tracked_tree_is_clean(root)
    if current_revision(root) != identity.source_revision:
        raise BuildIdentityError(
            "release-build identity is stale for this checkout"
        )
    if (
        canonical_tree_digest(root, identity.source_revision)
        != identity.source_tree_digest
    ):
        raise BuildIdentityError(
            "release-build source-tree digest is stale"
        )


def identity_environment(identity: ReleaseBuildIdentity) -> dict[str, str]:
    return {
        "PERL_LSP_BUILD_REVISION": identity.source_revision,
        "PERL_LSP_SOURCE_TREE_DIGEST": identity.source_tree_digest,
        "PERL_LSP_TARGET_TRIPLE": identity.target,
        "PERL_LSP_BUILD_PROFILE": identity.profile,
        "PERL_LSP_CANDIDATE_ID": identity.candidate_identity,
        "PERL_LSP_ARTIFACT_ROLE": identity.artifact_role,
    }


def cross_container_opts(identity: ReleaseBuildIdentity) -> str:
    """Return the closed Cross container environment passthrough contract."""
    arguments: list[str] = []
    for key, value in identity_environment(identity).items():
        arguments.extend(("-e", f"{key}={value}"))
    return shlex.join(arguments)


def build_environment(identity: ReleaseBuildIdentity) -> dict[str, str]:
    env = dict(os.environ)
    env.update(identity_environment(identity))
    return env


def append_github_env(
    path: Path,
    identity: ReleaseBuildIdentity,
    *,
    runner: str | None = None,
) -> None:
    """Append validated single-line build inputs for later workflow steps."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8", newline="\n") as handle:
        for key, value in identity_environment(identity).items():
            handle.write(f"{key}={value}\n")
        if runner == "cross":
            handle.write(f"CROSS_CONTAINER_OPTS={cross_container_opts(identity)}\n")


def binary_path(root: Path, identity: ReleaseBuildIdentity, executable: str) -> Path:
    suffix = ".exe" if "windows" in identity.target else ""
    return root / "target" / identity.target / identity.profile / f"{executable}{suffix}"


def build_command(
    runner: str,
    identity: ReleaseBuildIdentity,
    package: str,
    binary: str,
) -> list[str]:
    return [
        runner,
        "build",
        "--locked",
        "--release",
        "--target",
        identity.target,
        "-p",
        package,
        "--bin",
        binary,
    ]


def identity_command(
    runner: str,
    identity: ReleaseBuildIdentity,
    package: str,
    binary: str,
    path: Path,
) -> list[str]:
    if runner == "cross":
        return [
            runner,
            "run",
            "--locked",
            "--release",
            "--target",
            identity.target,
            "-p",
            package,
            "--bin",
            binary,
            "--",
            "--identity-json",
        ]
    return [str(path), "--identity-json"]


def parse_packet(raw: bytes, label: str) -> dict[str, Any]:
    if len(raw) > 256 * 1024:
        raise BuildIdentityError(f"{label} identity output exceeds 256 KiB")
    try:
        value = json.loads(raw.decode("utf-8", errors="strict"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise BuildIdentityError(f"{label} emitted invalid identity JSON") from error
    if not isinstance(value, dict):
        raise BuildIdentityError(f"{label} identity packet must be an object")
    return value


def expect_object(
    parent: Mapping[str, Any], key: str, label: str
) -> Mapping[str, Any]:
    value = parent.get(key)
    if not isinstance(value, dict):
        raise BuildIdentityError(f"{label}.{key} must be an object")
    return value


def validate_packet(
    packet: Mapping[str, Any],
    *,
    identity: ReleaseBuildIdentity,
    executable: str,
    package: str,
    role: str,
) -> None:
    allowed = {
        "schema_version",
        "product",
        "binary",
        "build",
        "artifact",
        "compatibility",
        "limitations",
    }
    unknown = sorted(set(packet) - allowed)
    if unknown:
        raise BuildIdentityError(
            f"{executable} packet contains unknown v1 fields: {unknown}"
        )
    if packet.get("schema_version") != PACKET_SCHEMA:
        raise BuildIdentityError(
            f"{executable} emitted an incompatible packet schema"
        )

    product = expect_object(packet, "product", executable)
    if product != {
        "name": "perl-lsp",
        "public_repository": "EffortlessMetrics/perl-lsp",
        "development_repository": "EffortlessMetrics/perl-lsp-swarm",
    }:
        raise BuildIdentityError(
            f"{executable} product identity disagrees with authority"
        )

    binary = expect_object(packet, "binary", executable)
    expected_binary = {
        "executable": executable,
        "cargo_package": package,
        "role": role,
        "version": identity.release_version,
    }
    if binary != expected_binary:
        raise BuildIdentityError(
            f"{executable} binary identity mismatch: "
            f"expected={expected_binary}, observed={binary}"
        )

    build = expect_object(packet, "build", executable)
    expected_build = {
        "source_revision": identity.source_revision,
        "source_tree_digest": identity.source_tree_digest,
        "target": identity.target,
        "profile": identity.profile,
        "identity_state": "exact",
    }
    if build != expected_build:
        raise BuildIdentityError(
            f"{executable} build identity mismatch: "
            f"expected={expected_build}, observed={build}"
        )

    artifact = expect_object(packet, "artifact", executable)
    if artifact.get("role") != identity.artifact_role:
        raise BuildIdentityError(
            f"{executable} artifact role mismatch"
        )
    if artifact.get("candidate_identity") != identity.candidate_identity:
        raise BuildIdentityError(
            f"{executable} candidate identity mismatch"
        )
    if artifact.get("digest") is not None:
        raise BuildIdentityError(
            f"{executable} must not self-attest its final executable digest"
        )
    if set(artifact) - {"role", "digest", "candidate_identity"}:
        raise BuildIdentityError(
            f"{executable} artifact packet contains unknown fields"
        )

    compatibility = expect_object(packet, "compatibility", executable)
    if compatibility != {
        "expected_product_identity_version": 1,
        "dap_posture": "preview",
    }:
        raise BuildIdentityError(
            f"{executable} compatibility identity mismatch"
        )

    limitations = packet.get("limitations", [])
    if limitations != []:
        raise BuildIdentityError(
            f"{executable} exact release packet retained limitations: "
            f"{limitations}"
        )


def execute_identity(
    root: Path,
    *,
    runner: str,
    identity: ReleaseBuildIdentity,
    package: str,
    binary: str,
    role: str,
    env: Mapping[str, str],
) -> dict[str, Any]:
    path = binary_path(root, identity, binary)
    path = require_within(root, path, f"{binary} build output")
    digest_before = sha256_file(path)
    result = run(
        identity_command(runner, identity, package, binary, path),
        cwd=root,
        env=env,
        timeout=120,
    )
    if result.stderr and len(result.stderr) > 256 * 1024:
        raise BuildIdentityError(
            f"{binary} identity stderr exceeds 256 KiB"
        )
    packet = parse_packet(result.stdout, binary)
    digest_after = sha256_file(path)
    if digest_before != digest_after:
        raise BuildIdentityError(
            f"{binary} changed while its identity was observed"
        )
    validate_packet(
        packet,
        identity=identity,
        executable=binary,
        package=package,
        role=role,
    )
    return {
        "role": role,
        "executable": binary,
        "path_role": path.relative_to(root).as_posix(),
        "file_sha256": digest_after,
        "packet_sha256": sha256_bytes(canonical_json_bytes(packet)),
        "packet": packet,
    }


def verify_binaries(
    args: argparse.Namespace,
    *,
    build_execution: str,
) -> dict[str, Any]:
    root = args.workspace_root.resolve(strict=True)
    input_path = require_within(
        root, args.input, "release-build identity"
    )
    identity = load_identity(input_path)
    verify_checkout(root, identity)
    if args.runner not in ALLOWED_RUNNERS:
        raise BuildIdentityError(f"unsupported build runner: {args.runner!r}")
    if toolchain_digest(root, args.runner) != identity.toolchain_digest:
        raise BuildIdentityError(
            "toolchain identity changed after build input generation"
        )

    env = build_environment(identity)
    commands = [
        build_command(args.runner, identity, "perllsp", "perllsp"),
        build_command(args.runner, identity, "perl-dap", "perl-dap"),
    ]
    binaries = [
        execute_identity(
            root,
            runner=args.runner,
            identity=identity,
            package="perllsp",
            binary="perllsp",
            role="server",
            env=env,
        ),
        execute_identity(
            root,
            runner=args.runner,
            identity=identity,
            package="perl-dap",
            binary="perl-dap",
            role="dap",
            env=env,
        ),
    ]
    receipt = {
        "schema_version": RECEIPT_SCHEMA,
        "status": "pass",
        "input_sha256": sha256_bytes(
            canonical_json_bytes(identity.as_dict())
        ),
        "input": identity.as_dict(),
        "runner": args.runner,
        "build_execution": build_execution,
        "build_commands": commands,
        "binaries": binaries,
        "claim_boundary": (
            "Embedded source/candidate identity and observed build-output "
            "bytes; packaged artifact identity remains externally bound "
            "after packaging."
        ),
    }
    write_atomic(args.receipt, canonical_json_bytes(receipt))
    return receipt


def build_and_verify(args: argparse.Namespace) -> dict[str, Any]:
    root = args.workspace_root.resolve(strict=True)
    input_path = require_within(
        root, args.input, "release-build identity"
    )
    identity = load_identity(input_path)
    verify_checkout(root, identity)
    if args.runner not in ALLOWED_RUNNERS:
        raise BuildIdentityError(f"unsupported build runner: {args.runner!r}")
    if toolchain_digest(root, args.runner) != identity.toolchain_digest:
        raise BuildIdentityError(
            "toolchain identity changed after build input generation"
        )
    env = build_environment(identity)
    for package, binary in (
        ("perllsp", "perllsp"),
        ("perl-dap", "perl-dap"),
    ):
        run(
            build_command(args.runner, identity, package, binary),
            cwd=root,
            env=env,
            capture=False,
            timeout=1800,
        )
    return verify_binaries(args, build_execution="adapter")


def write_atomic(path: Path, content: bytes) -> None:
    parent = path.parent
    parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=parent,
        prefix=f".{path.name}.",
        delete=False,
    ) as handle:
        temporary = Path(handle.name)
        handle.write(content)
        handle.flush()
        os.fsync(handle.fileno())
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    subparsers = result.add_subparsers(dest="command", required=True)

    prepare = subparsers.add_parser(
        "prepare", help="create an exact release-build identity"
    )
    prepare.add_argument(
        "--workspace-root", type=Path, default=Path(".")
    )
    prepare.add_argument("--repository", required=True)
    prepare.add_argument("--release-version", required=True)
    prepare.add_argument("--source-revision", required=True)
    prepare.add_argument("--target", required=True)
    prepare.add_argument("--candidate-identity", required=True)
    prepare.add_argument(
        "--artifact-role",
        required=True,
        choices=sorted(ALLOWED_ARTIFACT_ROLES),
    )
    prepare.add_argument(
        "--release-topology", type=Path, required=True
    )
    prepare.add_argument(
        "--product-identity", type=Path, required=True
    )
    prepare.add_argument(
        "--runner", required=True, choices=sorted(ALLOWED_RUNNERS)
    )
    prepare.add_argument("--output", type=Path, required=True)
    prepare.add_argument(
        "--github-env",
        type=Path,
        help="append validated build inputs for later GitHub Actions steps",
    )

    build = subparsers.add_parser(
        "build", help="build and verify both product binaries"
    )
    build.add_argument(
        "--workspace-root", type=Path, default=Path(".")
    )
    build.add_argument("--input", type=Path, required=True)
    build.add_argument(
        "--runner", required=True, choices=sorted(ALLOWED_RUNNERS)
    )
    build.add_argument("--receipt", type=Path, required=True)

    verify = subparsers.add_parser(
        "verify", help="verify binaries built by the release workflow"
    )
    verify.add_argument(
        "--workspace-root", type=Path, default=Path(".")
    )
    verify.add_argument("--input", type=Path, required=True)
    verify.add_argument(
        "--runner", required=True, choices=sorted(ALLOWED_RUNNERS)
    )
    verify.add_argument("--receipt", type=Path, required=True)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "prepare":
            identity = prepare_identity(args)
            print(
                "release-build-identity: PASS: "
                f"{args.output} "
                f"({identity.source_revision}/{identity.target})"
            )
        elif args.command == "build":
            receipt = build_and_verify(args)
            print(
                "release-build-identity: PASS: "
                f"{args.receipt} ({len(receipt['binaries'])} binaries)"
            )
        else:
            receipt = verify_binaries(
                args, build_execution="external_release_workflow"
            )
            print(
                "release-build-identity: PASS: "
                f"{args.receipt} ({len(receipt['binaries'])} binaries)"
            )
    except (
        BuildIdentityError,
        OSError,
        subprocess.CalledProcessError,
        subprocess.TimeoutExpired,
    ) as error:
        print(
            f"release-build-identity: NOT_PROVEN: {error}",
            file=sys.stderr,
        )
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
