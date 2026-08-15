#!/usr/bin/env python3
"""Generate and validate the exact-SHA v0.18 release topology inventory.

This is an inventory generator, not a release-readiness or publication command.
It deliberately refuses to write a manifest when the structured release
authorities disagree (for example, when the workflow builds a target that the
downstream archive contract does not enumerate).
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


SCHEMA = 1
PRIMARY_CHANNELS = ["github_release", "crates_io", "vscode_marketplace", "open_vsx"]
SOURCE_PATHS = [
    "Cargo.toml",
    "Cargo.lock",
    ".github/workflows/release.yml",
    "vscode-extension/package.json",
    "docs/reference/downstream-dap-integrations.json",
    "vscode-extension/src/downloader.ts",
    "scripts/inject-sha-assets.sh",
]
TARGET_RE = re.compile(
    r"(?ms)^\s*- target:\s*(?P<target>[A-Za-z0-9_-]+)\s*$"
    r"(?P<body>.*?)(?=^\s*- target:|^\s*steps:|\Z)"
)


class TopologyError(ValueError):
    """A release topology input is missing, stale, or inconsistent."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git_head(root: Path) -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    value = result.stdout.strip()
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        raise TopologyError(f"git HEAD is not a full commit SHA: {value!r}")
    return value


def cargo_metadata(root: Path) -> dict[str, Any]:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise TopologyError(f"cargo metadata returned invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise TopologyError("cargo metadata did not return an object")
    return value


def derive_crates(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    packages = {package["id"]: package for package in metadata.get("packages", [])}
    member_ids = metadata.get("workspace_members", [])
    members = [packages[member_id] for member_id in member_ids if member_id in packages]
    publish_metadata = metadata.get("metadata", {}).get("publish", {})
    allowlist = publish_metadata.get("allow")
    if not isinstance(allowlist, list) or not all(
        isinstance(name, str) for name in allowlist
    ):
        raise TopologyError(
            "workspace metadata publish.allow must be an array of crate names"
        )
    allowed = set(allowlist)
    by_name = {package["name"]: package for package in members}
    missing = sorted(allowed - set(by_name))
    if missing:
        raise TopologyError(
            f"publish allowlist names missing from workspace: {missing}"
        )
    publishable = {
        package["name"]
        for package in members
        if package.get("publish") is None or package.get("publish")
    }
    if publishable != allowed:
        raise TopologyError(
            "publish allowlist drift: "
            f"missing={sorted(publishable - allowed)}, extra={sorted(allowed - publishable)}"
        )

    dependencies: dict[str, set[str]] = {}
    for name in allowed:
        package = by_name[name]
        dependencies[name] = {
            dependency["name"]
            for dependency in package.get("dependencies", [])
            if dependency.get("name") in allowed and dependency.get("source") is None
        }
    ready = sorted(name for name, deps in dependencies.items() if not deps)
    order: list[str] = []
    while ready:
        name = ready.pop(0)
        order.append(name)
        for dependent in sorted(dependencies):
            if name in dependencies[dependent]:
                dependencies[dependent].remove(name)
                if not dependencies[dependent]:
                    ready.append(dependent)
        ready.sort()
    if len(order) != len(allowed):
        remaining = sorted(name for name, deps in dependencies.items() if deps)
        raise TopologyError(
            f"publish dependency cycle or unresolved dependency: {remaining}"
        )
    publish_order = {name: index for index, name in enumerate(order, start=1)}

    entries: list[dict[str, Any]] = []
    for name in order:
        package = by_name[name]
        manifest_path = Path(package["manifest_path"])
        package_path = manifest_path.parent.as_posix()
        entries.append(
            {
                "name": name,
                "package_path": package_path,
                "version": package["version"],
                "publish_order": publish_order[name],
                "internal_dependencies": sorted(
                    dependency
                    for dependency in (
                        item["name"]
                        for item in package.get("dependencies", [])
                        if item.get("name") in allowed and item.get("source") is None
                    )
                ),
            }
        )
    return entries


def target_identity(target: str, runner: str) -> dict[str, Any]:
    if target.startswith("aarch64"):
        architecture = "aarch64"
    elif target.startswith("x86_64"):
        architecture = "x86_64"
    else:
        raise TopologyError(f"unsupported release target architecture: {target}")
    if "windows" in target:
        os_name = "windows"
    elif "apple-darwin" in target:
        os_name = "macos"
    elif "linux" in target:
        os_name = "linux"
    else:
        raise TopologyError(f"unsupported release target OS: {target}")
    libc = "gnu" if "-gnu" in target else "musl" if "-musl" in target else None
    extension = ".zip" if os_name == "windows" else ".tar.gz"
    binary_suffix = ".exe" if os_name == "windows" else ""
    return {
        "target": target,
        "os": os_name,
        "architecture": architecture,
        "libc": libc,
        "runner": runner,
        "archive_name": extension,
        "binary_members": [f"perllsp{binary_suffix}", f"perl-dap{binary_suffix}"],
    }


def derive_targets(release_text: str, release: str) -> list[dict[str, Any]]:
    targets: list[dict[str, Any]] = []
    seen: set[str] = set()
    for match in TARGET_RE.finditer(release_text):
        target = match.group("target")
        if target in seen:
            raise TopologyError(f"duplicate release target: {target}")
        seen.add(target)
        runner_match = re.search(
            r"^\s*os:\s*(\S+)\s*$", match.group("body"), re.MULTILINE
        )
        if not runner_match:
            raise TopologyError(f"release target has no runner: {target}")
        identity = target_identity(target, runner_match.group(1))
        identity["archive_name"] = (
            f"perllsp-{release}-{target}{identity['archive_name']}"
        )
        suffix = ".exe" if identity["os"] == "windows" else ""
        identity["required_members"] = [
            f"perllsp{suffix}",
            f"perl-dap{suffix}",
            "README.md",
            "LICENSE-APACHE",
            "LICENSE-MIT",
            "SHA256SUMS.txt",
        ]
        del identity["binary_members"]
        targets.append(identity)
    if not targets:
        raise TopologyError("release workflow has no target matrix")
    return targets


def source_hashes(root: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for relative in SOURCE_PATHS:
        path = root / relative
        if not path.is_file():
            raise TopologyError(f"topology source is missing: {relative}")
        result[relative] = sha256(path)
    return result


def derive_downloader_targets(source: str, workflow_targets: set[str]) -> set[str]:
    """Derive the release targets reachable through the managed downloader.

    The downloader deliberately constructs the Linux target from architecture and
    libc, so this is a small structural check of that production authority rather
    than a second hand-written release matrix.  Every workflow target must be
    represented by an explicit downloader branch or by the corresponding
    architecture/libc construction.
    """
    managed: set[str] = set()

    if "aarch64-apple-darwin" in source:
        managed.add("aarch64-apple-darwin")
    if "x86_64-apple-darwin" in source:
        managed.add("x86_64-apple-darwin")
    if "return 'x86_64-pc-windows-msvc'" in source:
        managed.add("x86_64-pc-windows-msvc")
    if "return 'aarch64-pc-windows-msvc'" in source:
        managed.add("aarch64-pc-windows-msvc")

    constructs_linux_targets = (
        "return `${archPrefix}-unknown-linux-${libc}`" in source
        and "archPrefix = arch === 'arm64' ? 'aarch64' : 'x86_64'" in source
        and "value === 'gnu'" in source
        and "value === 'musl'" in source
    )
    if constructs_linux_targets:
        managed.update(
            target
            for target in workflow_targets
            if target.endswith("-unknown-linux-gnu")
            or target.endswith("-unknown-linux-musl")
        )

    return managed & workflow_targets


def build_manifest(
    root: Path,
    release: str,
    frozen_product_sha: str,
    prepared_swarm_sha: str | None = None,
) -> dict[str, Any]:
    if not re.fullmatch(r"[0-9a-f]{40}", frozen_product_sha):
        raise TopologyError("frozen_product_sha must be a full lowercase commit SHA")
    current_sha = git_head(root)
    if current_sha != frozen_product_sha:
        raise TopologyError(
            "frozen_product_sha must identify the exact checkout being inventoried"
        )
    if prepared_swarm_sha is not None:
        if not re.fullmatch(r"[0-9a-f]{40}", prepared_swarm_sha):
            raise TopologyError(
                "prepared_swarm_sha must be a full lowercase commit SHA"
            )
        if prepared_swarm_sha != current_sha:
            raise TopologyError(
                "prepared_swarm_sha must identify the exact prepared checkout"
            )
    metadata = cargo_metadata(root)
    cargo_manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    workspace_version = (
        cargo_manifest.get("workspace", {}).get("package", {}).get("version")
    )
    if not isinstance(workspace_version, str):
        raise TopologyError("workspace.package.version is missing from Cargo.toml")
    if workspace_version != release:
        raise TopologyError(
            f"workspace version {workspace_version} does not match requested release {release}"
        )
    workflow = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
    targets = derive_targets(workflow, release)
    downstream = json.loads(
        (root / "docs/reference/downstream-dap-integrations.json").read_text()
    )
    downstream_targets = {entry["triple"] for entry in downstream.get("targets", [])}
    workflow_targets = {entry["target"] for entry in targets}
    if downstream_targets != workflow_targets:
        raise TopologyError(
            "downstream archive contract disagrees with release workflow: "
            f"missing={sorted(workflow_targets - downstream_targets)}, "
            f"extra={sorted(downstream_targets - workflow_targets)}"
        )
    downloader_targets = derive_downloader_targets(
        (root / "vscode-extension/src/downloader.ts").read_text(encoding="utf-8"),
        workflow_targets,
    )
    if downloader_targets != workflow_targets:
        raise TopologyError(
            "managed downloader target contract disagrees with release workflow: "
            f"missing={sorted(workflow_targets - downloader_targets)}, "
            f"extra={sorted(downloader_targets - workflow_targets)}"
        )
    package = json.loads(
        (root / "vscode-extension/package.json").read_text(encoding="utf-8")
    )
    if package.get("version") != release:
        raise TopologyError(
            f"VSIX version {package.get('version')} does not match {release}"
        )
    crates = derive_crates(metadata)
    for entry in crates:
        entry["package_path"] = (
            Path(entry["package_path"]).resolve().relative_to(root.resolve()).as_posix()
        )
    manifest = {
        "schema": SCHEMA,
        "release": release,
        "track": "public-beta",
        "frozen_product_sha": frozen_product_sha,
        "prepared_swarm_sha": prepared_swarm_sha,
        "workspace_version": workspace_version,
        "published_crates": crates,
        "crate_count": len(crates),
        "binary_targets": targets,
        "archive_count": len(targets),
        "vsix": {
            "version": package["version"],
            "asset_name": f"{package['name']}-{package['version']}.vsix",
            "package_path": "vscode-extension",
            "managed_targets": sorted(downloader_targets),
            "bundled_targets": [],
        },
        "primary_channels": PRIMARY_CHANNELS,
        "secondary_channels": {"docker": "required", "homebrew": "deferred"},
        "sources": {
            relative: {"path": relative, "sha256": digest}
            for relative, digest in source_hashes(root).items()
        },
    }
    return manifest


def validate_manifest(
    manifest: dict[str, Any], root: Path, expected_sha: str | None = None
) -> None:
    if manifest.get("schema") != SCHEMA:
        raise TopologyError("manifest schema must be 1")
    if expected_sha is not None and manifest.get("frozen_product_sha") != expected_sha:
        raise TopologyError(
            "manifest frozen_product_sha differs from the reviewed candidate SHA"
        )
    if expected_sha is not None and git_head(root) != expected_sha:
        raise TopologyError(
            "reviewed candidate SHA is not the exact checkout being validated"
        )
    prepared_swarm_sha = manifest.get("prepared_swarm_sha")
    if prepared_swarm_sha is not None:
        if not isinstance(prepared_swarm_sha, str) or not re.fullmatch(
            r"[0-9a-f]{40}", prepared_swarm_sha
        ):
            raise TopologyError(
                "prepared_swarm_sha must be a full lowercase commit SHA"
            )
        if prepared_swarm_sha != git_head(root):
            raise TopologyError(
                "prepared_swarm_sha must identify the exact checkout being validated"
            )
    release = manifest.get("release")
    if not isinstance(release, str):
        raise TopologyError("manifest release is missing")
    metadata = cargo_metadata(root)
    expected_crates = derive_crates(metadata)
    for entry in expected_crates:
        entry["package_path"] = (
            Path(entry["package_path"]).resolve().relative_to(root.resolve()).as_posix()
        )
    crates = manifest.get("published_crates")
    if not isinstance(crates, list) or manifest.get("crate_count") != len(crates):
        raise TopologyError("crate_count must be derived from published_crates")
    if crates != expected_crates:
        raise TopologyError("published_crates does not match current Cargo metadata")
    orders = [entry.get("publish_order") for entry in crates]
    if orders != list(range(1, len(crates) + 1)):
        raise TopologyError("publish_order must be a contiguous topological sequence")
    names = {entry.get("name") for entry in crates}
    for entry in crates:
        if not set(entry.get("internal_dependencies", [])).issubset(names):
            raise TopologyError(
                f"crate has an unlisted internal dependency: {entry.get('name')}"
            )
        order = entry["publish_order"]
        dependency_orders = {other["name"]: other["publish_order"] for other in crates}
        if any(
            dependency_orders[name] >= order for name in entry["internal_dependencies"]
        ):
            raise TopologyError(
                f"crate dependencies are not publishable before {entry['name']}"
            )
    targets = manifest.get("binary_targets")
    if not isinstance(targets, list) or manifest.get("archive_count") != len(targets):
        raise TopologyError("archive_count must be derived from binary_targets")
    target_names = [entry.get("target") for entry in targets]
    if len(target_names) != len(set(target_names)):
        raise TopologyError("binary target entries must be unique")
    archive_names = [entry.get("archive_name") for entry in targets]
    if len(archive_names) != len(set(archive_names)):
        raise TopologyError("archive names must be unique")
    workflow = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
    expected_targets = derive_targets(workflow, release)
    if targets != expected_targets:
        raise TopologyError("binary_targets does not match the release workflow")
    downstream = json.loads(
        (root / "docs/reference/downstream-dap-integrations.json").read_text()
    )
    if {entry["triple"] for entry in downstream.get("targets", [])} != set(
        target_names
    ):
        raise TopologyError(
            "binary_targets does not match the downstream archive contract"
        )
    downloader_targets = derive_downloader_targets(
        (root / "vscode-extension/src/downloader.ts").read_text(encoding="utf-8"),
        set(target_names),
    )
    if downloader_targets != set(target_names):
        raise TopologyError(
            "binary_targets does not match the managed downloader target contract"
        )
    package = json.loads(
        (root / "vscode-extension/package.json").read_text(encoding="utf-8")
    )
    if package.get("version") != release or manifest.get("vsix", {}).get(
        "version"
    ) != package.get("version"):
        raise TopologyError(
            "VSIX version does not match the release or current extension manifest"
        )
    expected_vsix_asset = f"{package.get('name')}-{package.get('version')}.vsix"
    if manifest.get("vsix", {}).get("asset_name") != expected_vsix_asset:
        raise TopologyError("VSIX asset name does not match the extension manifest")
    if sorted(manifest.get("vsix", {}).get("managed_targets", [])) != sorted(
        downloader_targets
    ):
        raise TopologyError(
            "VSIX managed targets do not match the release target matrix"
        )
    if manifest.get("primary_channels") != PRIMARY_CHANNELS:
        raise TopologyError("primary channel set is not the accepted v0.18 set")
    if manifest.get("vsix", {}).get("version") != manifest.get("release"):
        raise TopologyError("VSIX version must equal release version")
    sources = manifest.get("sources")
    if not isinstance(sources, dict):
        raise TopologyError("sources must be an object")
    expected_source_paths = set(SOURCE_PATHS)
    source_paths = set(sources)
    if source_paths != expected_source_paths:
        raise TopologyError(
            "source hash set does not match the topology source set: "
            f"missing={sorted(expected_source_paths - source_paths)}, "
            f"extra={sorted(source_paths - expected_source_paths)}"
        )
    for relative, source in sources.items():
        if not isinstance(source, dict):
            raise TopologyError(f"source hash entry is not an object: {relative}")
        if source.get("path") != relative:
            raise TopologyError(
                f"source path key disagrees with its path field: {relative}"
            )
        path = root / relative
        if not path.is_file() or sha256(path) != source.get("sha256"):
            raise TopologyError(f"source hash is stale: {relative}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--release", required=True, help="candidate version, e.g. 0.18.0"
    )
    parser.add_argument("--frozen-product-sha", required=True)
    parser.add_argument("--prepared-swarm-sha")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--check", action="store_true", help="validate an existing output"
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        if args.check:
            manifest = json.loads(args.output.read_text(encoding="utf-8"))
            if manifest.get("release") != args.release:
                raise TopologyError("manifest release differs from --release")
            validate_manifest(manifest, root, args.frozen_product_sha)
        else:
            manifest = build_manifest(
                root, args.release, args.frozen_product_sha, args.prepared_swarm_sha
            )
            validate_manifest(manifest, root, args.frozen_product_sha)
            args.output.write_text(
                json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
            )
    except (
        OSError,
        json.JSONDecodeError,
        subprocess.CalledProcessError,
        TopologyError,
    ) as error:
        print(f"release-topology: NOT_PROVEN: {error}", file=sys.stderr)
        return 2
    print(f"release-topology: PASS: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
