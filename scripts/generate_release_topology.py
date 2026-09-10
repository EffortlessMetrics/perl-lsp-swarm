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
from copy import deepcopy
from pathlib import Path
from typing import Any


SCHEMA = 1
SCHEMA_RELATIVE_PATH = "schemas/release_topology.v1.schema.json"
SCHEMA_PATH = Path(__file__).resolve().parents[1] / "schemas/release_topology.v1.schema.json"
PRIMARY_CHANNELS = ["github_release", "crates_io", "vscode_marketplace", "open_vsx"]
VERSION_DERIVED_SOURCE_PATHS = {
    "Cargo.toml",
    "Cargo.lock",
    "vscode-extension/package.json",
}
SOURCE_PATHS = [
    "Cargo.toml",
    "Cargo.lock",
    SCHEMA_RELATIVE_PATH,
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


def topology_schema_version(value: Any) -> int:
    # JSON Schema accepts integral numbers such as 1.0. Preserve that v1
    # behavior, while refusing Python's bool/int equality and unknown versions.
    if type(value) not in (int, float) or value not in (1, 2):
        raise TopologyError("release topology schema must be 1 or 2")
    return int(value)


def schema_relative_path(version: Any) -> str:
    return f"schemas/release_topology.v{topology_schema_version(version)}.schema.json"


def schema_validate(manifest: dict[str, Any], root: Path | None = None) -> None:
    """Validate the complete manifest against the checked-in JSON schema.

    The release and rolling-observation workflows install the pinned validator
    requirements before invoking this script.  Keeping the dependency
    explicit avoids silently falling back to a partial hand-written validator.
    """
    relative = schema_relative_path(manifest.get("schema"))
    try:
        import jsonschema
    except ImportError as error:
        raise TopologyError(
            "JSON-schema validation requires the pinned release requirements; "
            "install scripts/requirements-release.txt"
        ) from error
    try:
        schema_path = (root or SCHEMA_PATH.parents[1]) / relative
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        validator_type = jsonschema.validators.validator_for(schema)
        validator_type.check_schema(schema)
        validator = validator_type(schema)
    except (OSError, json.JSONDecodeError, jsonschema.SchemaError) as error:
        raise TopologyError(f"release topology schema is invalid: {error}") from error
    errors = sorted(
        validator.iter_errors(manifest),
        key=lambda error: (tuple(str(part) for part in error.absolute_path), error.message),
    )
    if errors:
        details = "; ".join(
            f"{'.'.join(str(part) for part in error.absolute_path) or '<root>'}: {error.message}"
            for error in errors
        )
        raise TopologyError(f"manifest schema validation failed: {details}")
    if root is not None:
        sources = manifest.get("sources")
        source = (
            sources.get(relative, {})
            if isinstance(sources, dict)
            else {}
        )
        if not isinstance(source, dict) or source.get("sha256") != sha256(
            root / relative
        ):
            raise TopologyError("schema source hash is stale")


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


def checksum_candidate_steps(release_text: str) -> list[str]:
    """Bind the recognized producer to its supported GitHub job environment.

    This intentionally recognizes the checked-in mapping layout, not general
    YAML, inherited run configuration, or conditional workflow execution.
    """
    lines = release_text.splitlines()
    top_keys = [line.split(":", 1)[0] for line in lines if line and not line[0].isspace() and not line.startswith("#")]
    if (
        any(key not in {"name", "on", "concurrency", "permissions", "env", "jobs"} for key in top_keys)
        or len(top_keys) != len(set(top_keys))
        or lines.count("jobs:") != 1
    ):
        raise TopologyError("checksum producer has unsupported workflow execution context")
    if "env" in top_keys:
        if "env:" not in lines:
            raise TopologyError("checksum producer has unsupported workflow environment")
        env_start = lines.index("env:") + 1
        env_end = next((index for index in range(env_start, len(lines)) if lines[index] and not lines[index][0].isspace() and not lines[index].startswith("#")), len(lines))
        environment = [line for line in lines[env_start:env_end] if line.strip() and not line.lstrip().startswith("#")]
        if sorted(environment) != ["  CARGO_TERM_COLOR: always", "  RUST_BACKTRACE: 1"]:
            raise TopologyError("checksum producer has unsupported workflow environment")
    jobs_start = lines.index("jobs:")
    jobs_end = next((index for index in range(jobs_start + 1, len(lines)) if lines[index] and not lines[index][0].isspace() and not lines[index].startswith("#")), len(lines))
    jobs = lines[jobs_start + 1:jobs_end]
    starts = [index for index, line in enumerate(jobs) if line == "  candidate:"]
    if len(starts) != 1:
        raise TopologyError("checksum producer requires exactly one candidate job")
    start = starts[0]
    end = next((index for index in range(start + 1, len(jobs)) if jobs[index].strip() and len(jobs[index]) - len(jobs[index].lstrip()) <= 2), len(jobs))
    candidate = jobs[start + 1:end]
    fields = [line for line in candidate if line.strip() and not line.lstrip().startswith("#") and len(line) - len(line.lstrip()) == 4]
    keys = [line.strip().split(":", 1)[0] for line in fields]
    allowed = {"name", "needs", "runs-on", "timeout-minutes", "permissions", "steps"}
    if len(keys) != len(set(keys)) or any(key not in allowed for key in keys):
        raise TopologyError("checksum producer has unsupported candidate execution context")
    if "    needs: [build, release-metadata]" not in fields or "    runs-on: ubuntu-24.04" not in fields or "    steps:" not in fields:
        raise TopologyError("checksum producer requires candidate build dependencies, Linux runner and steps")
    step_start = candidate.index("    steps:") + 1
    step_end = next((index for index in range(step_start, len(candidate)) if candidate[index].strip() and len(candidate[index]) - len(candidate[index].lstrip()) <= 4), len(candidate))
    steps = candidate[step_start:step_end]
    producer_starts = [index for index, line in enumerate(steps) if line == "      - name: Generate consolidated SHA256SUMS"]
    download_starts = [index for index, line in enumerate(steps) if line == "      - name: Download release archive artifacts"]
    if len(producer_starts) != 1 or len(download_starts) != 1 or download_starts[0] >= producer_starts[0]:
        raise TopologyError("checksum producer requires one earlier archive download in candidate steps")
    download_start = download_starts[0]
    download_end = next((index for index in range(download_start + 1, len(steps)) if steps[index].strip() and len(steps[index]) - len(steps[index].lstrip()) <= 6), len(steps))
    download = [line for line in steps[download_start + 1:download_end] if line.strip()]
    if (
        len(download) != 4
        or re.fullmatch(r"        uses: actions/download-artifact@[0-9a-f]{40}(?: +#.*)?", download[0]) is None
        or download[1:] != ["        with:", "          pattern: perllsp-*", "          path: artifacts"]
    ):
        raise TopologyError("checksum producer archive download inputs or execution shape are not recognized")
    return steps


def consolidated_checksum_producer(release_text: str) -> str:
    """Recognize the bounded archive-copy/checksum producer, without executing it.

    This is a closed source-shape contract, not a Bash interpreter. A changed
    producer must be reviewed with its execution oracle before v2 can project it.
    The duplicate-name guard, destination, coverage and hash pipeline all matter.
    """
    if sum(line.strip() == "- name: Generate consolidated SHA256SUMS" for line in release_text.splitlines()) != 1:
        raise TopologyError("release workflow must contain exactly one consolidated checksum producer")
    lines = checksum_candidate_steps(release_text)
    starts = [
        index for index, line in enumerate(lines)
        if line == "      - name: Generate consolidated SHA256SUMS"
    ]
    if len(starts) != 1:
        raise TopologyError("release workflow must contain exactly one consolidated checksum producer")
    start = starts[0]
    indent = len(lines[start]) - len(lines[start].lstrip())
    step = []
    for line in lines[start + 1:]:
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        if line.strip():
            step.append(line)
    if (
        not step or step[0] != " " * (indent + 2) + "run: |"
        or any(not line.startswith(" " * (indent + 4)) for line in step[1:])
    ):
        raise TopologyError("consolidated checksum producer has an unsupported step shape")
    body = "\n".join(line[indent + 4:] for line in step[1:]) + "\n"
    expected = r'''set -euo pipefail
mkdir -p candidate/dist
declare -A ARCHIVE_NAMES=()
while IFS= read -r -d '' archive; do
  name=$(basename "$archive")
  if [ -n "${ARCHIVE_NAMES[$name]:-}" ]; then
    printf '::error::Duplicate archive filename: %s\n' "$name"
    exit 1
  fi
  ARCHIVE_NAMES[$name]=1
  cp "$archive" candidate/dist/
done < <(find artifacts -type f \( -name '*.tar.gz' -o -name '*.zip' \) -print0)
cd candidate/dist
find . -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.zip' \) -print0 | \
  sort -z | \
  xargs -0 sha256sum | sed 's|  \./|  |' > SHA256SUMS
cat SHA256SUMS
'''
    # Shell indentation carries no meaning here; compare every command,
    # including loop bodies, rather than checking isolated token presence.
    if [line.strip() for line in body.splitlines()] != [
        line.strip() for line in expected.splitlines()
    ]:
        raise TopologyError("consolidated checksum producer command shape is not recognized")
    return body


def derive_checksum_assets(
    release_text: str, targets: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    consolidated_checksum_producer(release_text)
    return [{
        "asset_name": "SHA256SUMS",
        "algorithm": "sha256",
        "channel": "github_release",
        "archive_targets": sorted(target["target"] for target in targets),
    }]


def workspace_member_manifest_paths(
    metadata: dict[str, Any], root: Path
) -> list[str]:
    packages = {
        package["id"]: package
        for package in metadata.get("packages", [])
        if isinstance(package, dict) and isinstance(package.get("id"), str)
    }
    paths: list[str] = []
    for member_id in metadata.get("workspace_members", []):
        package = packages.get(member_id)
        if package is None or not isinstance(package.get("manifest_path"), str):
            raise TopologyError(f"workspace member metadata is incomplete: {member_id}")
        try:
            relative = (
                Path(package["manifest_path"])
                .resolve()
                .relative_to(root.resolve())
                .as_posix()
            )
        except ValueError as error:
            raise TopologyError(
                f"workspace member manifest escapes checkout: {package['manifest_path']}"
            ) from error
        if relative not in paths:
            paths.append(relative)
    return paths


def source_paths(
    crates: list[dict[str, Any]], workspace_manifests: list[str] | None = None,
    schema_version: int = SCHEMA,
) -> list[str]:
    paths = [
        schema_relative_path(schema_version) if path == SCHEMA_RELATIVE_PATH else path
        for path in SOURCE_PATHS
    ]
    manifests = workspace_manifests
    if manifests is None:
        manifests = []
        for crate in crates:
            package_path = crate.get("package_path")
            if not isinstance(package_path, str):
                raise TopologyError("published crate package_path must be a string")
            package = Path(package_path)
            if package.is_absolute() or ".." in package.parts:
                raise TopologyError(
                    f"published crate package_path escapes checkout: {package_path}"
                )
            manifests.append((package / "Cargo.toml").as_posix())
    for manifest in manifests:
        if manifest not in paths:
            paths.append(manifest)
    return paths


def workspace_inherited_package_names(
    root: Path, workspace_manifests: list[str]
) -> dict[str, str]:
    inherited: dict[str, str] = {}
    for relative in workspace_manifests:
        try:
            value = tomllib.loads((root / relative).read_text(encoding="utf-8"))
        except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
            raise TopologyError(f"cannot read workspace manifest {relative}: {error}") from error
        package = value.get("package", {})
        name = package.get("name") if isinstance(package, dict) else None
        version = package.get("version") if isinstance(package, dict) else None
        if isinstance(name, str) and isinstance(version, dict) and version == {"workspace": True}:
            inherited[name] = relative
    return inherited


def source_hashes_for_paths(root: Path, paths: list[str]) -> dict[str, str]:
    result: dict[str, str] = {}
    for relative in paths:
        path = root / relative
        if not path.is_file():
            raise TopologyError(f"topology source is missing: {relative}")
        result[relative] = sha256(path)
    return result


def ensure_committed_topology_inputs(root: Path, paths: list[str]) -> None:
    """Require every topology input to be tracked and clean in its checkout.

    Release output manifests are intentionally outside this set and may remain
    untracked.  Git performs the comparison through its configured filters, so
    a checkout with a harmless working-tree newline conversion is handled the
    same way as every other committed file.
    """
    tracked = subprocess.run(
        ["git", "ls-files", "--error-unmatch", "--", *paths],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if tracked.returncode not in (0, 1):
        detail = tracked.stderr.strip() or "git ls-files failed"
        raise TopologyError(f"cannot inspect tracked topology inputs: {detail}")
    tracked_paths = set(tracked.stdout.splitlines())
    missing = sorted(set(paths) - tracked_paths)
    if missing:
        raise TopologyError(
            "topology inputs are not tracked: " + ", ".join(missing)
        )
    for label, extra in (("unstaged", []), ("staged", ["--cached"])):
        result = subprocess.run(
            ["git", "diff", "--quiet", *extra, "--", *paths],
            cwd=root,
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode == 1:
            raise TopologyError(f"topology input has {label} changes")
        if result.returncode != 0:
            detail = result.stderr.strip() or "git diff failed"
            raise TopologyError(f"cannot inspect {label} topology inputs: {detail}")


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise TopologyError(f"cannot read frozen topology {path}: {error}") from error
    if not isinstance(value, dict):
        raise TopologyError("frozen topology must be a JSON object")
    return value


def load_frozen_authority(
    path: Path, expected_digest: str | None
) -> tuple[dict[str, Any], str]:
    if expected_digest is None or not re.fullmatch(r"[0-9a-f]{64}", expected_digest):
        raise TopologyError(
            "prepared validation requires a lowercase 64-hex frozen topology digest"
        )
    try:
        raw = path.read_bytes()
    except OSError as error:
        raise TopologyError(f"cannot read frozen topology {path}: {error}") from error
    actual_digest = hashlib.sha256(raw).hexdigest()
    if actual_digest != expected_digest:
        raise TopologyError(
            "frozen topology digest differs from --frozen-topology-sha256"
        )
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise TopologyError(f"cannot parse frozen topology {path}: {error}") from error
    if not isinstance(value, dict):
        raise TopologyError("frozen topology must be a JSON object")
    return value, actual_digest


def immutable_projection(manifest: dict[str, Any]) -> dict[str, Any]:
    """Return the product-subject portion that preparation must preserve.

    Version-bearing labels are deliberately removed because preparation may
    derive new package, archive, and VSIX identities. Source hashes are
    checked separately against normalized source bytes.
    All other membership, ordering, target, install, and channel fields remain
    load-bearing.
    """
    projected = deepcopy(manifest)
    projected.pop("release", None)
    projected.pop("workspace_version", None)
    projected.pop("prepared_swarm_sha", None)
    projected["frozen_product_sha"] = "<frozen-product>"
    for crate in projected.get("published_crates", []):
        if isinstance(crate, dict):
            crate.pop("version", None)
    for target in projected.get("binary_targets", []):
        if isinstance(target, dict):
            target.pop("archive_name", None)
    vsix = projected.get("vsix")
    if isinstance(vsix, dict):
        vsix.pop("version", None)
        vsix.pop("asset_name", None)
    return projected


def version_derived_source_paths(published_paths: set[str]) -> set[str]:
    return VERSION_DERIVED_SOURCE_PATHS | published_paths


def normalized_source_value(
    root: Path,
    relative: str,
    release: str,
    published_names: set[str],
    published_paths: set[str],
    workspace_inherited_names: set[str] | None = None,
) -> Any:
    path = root / relative
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise TopologyError(f"cannot read source {relative}: {error}") from error
    if (
        relative == "Cargo.toml"
        or relative in published_paths
        or Path(relative).name == "Cargo.toml"
    ):
        try:
            value = tomllib.loads(text)
        except tomllib.TOMLDecodeError as error:
            raise TopologyError(f"cannot parse source {relative}: {error}") from error
        version_names = published_names | (workspace_inherited_names or set())
        if relative == "Cargo.toml":
            workspace = value.get("workspace", {})
            if not isinstance(workspace, dict):
                raise TopologyError("Cargo.toml [workspace] must be a table")
            package = workspace.get("package", {})
            if isinstance(package, dict) and package.get("version") == release:
                package["version"] = "<version>"
            dependencies = workspace.get("dependencies", {})
            if not isinstance(dependencies, dict):
                raise TopologyError("workspace.dependencies must be a table")
        else:
            package = value.get("package", {})
            if (
                isinstance(package, dict)
                and package.get("name") in published_names
                and package.get("version") == release
            ):
                package["version"] = "<version>"
            for section in ("dependencies", "dev-dependencies", "build-dependencies"):
                table = value.get(section, {})
                if not isinstance(table, dict):
                    raise TopologyError(f"{relative} [{section}] must be a table")
                for name, dependency in table.items():
                    if (
                        name in version_names
                        and isinstance(dependency, dict)
                        and (
                            dependency.get("path") is not None
                            or dependency.get("workspace") is True
                        )
                        and dependency.get("version") == release
                    ):
                        dependency["version"] = "<version>"
        if relative == "Cargo.toml":
            for name, dependency in dependencies.items():
                if (
                    name in version_names
                    and isinstance(dependency, dict)
                    and (
                        dependency.get("path") is not None
                        or dependency.get("workspace") is True
                    )
                    and dependency.get("version") == release
                ):
                    dependency["version"] = "<version>"
        return value
    if relative == "Cargo.lock":
        try:
            value = tomllib.loads(text)
        except tomllib.TOMLDecodeError as error:
            raise TopologyError(f"cannot parse source {relative}: {error}") from error
        lock_version_names = published_names | (workspace_inherited_names or set())
        for package in value.get("package", []):
            if (
                isinstance(package, dict)
                and package.get("name") in lock_version_names
                and package.get("source") is None
                and package.get("version") == release
            ):
                package["version"] = "<version>"
        return value
    if relative == "vscode-extension/package.json":
        value = json.loads(text)
        if isinstance(value, dict) and value.get("version") == release:
            value["version"] = "<version>"
        return value
    return text


def validate_source_transition(
    frozen: dict[str, Any],
    prepared: dict[str, Any],
    frozen_root: Path,
    prepared_root: Path,
) -> None:
    frozen_sources = frozen.get("sources")
    prepared_sources = prepared.get("sources")
    if not isinstance(frozen_sources, dict) or not isinstance(prepared_sources, dict):
        raise TopologyError("frozen/prepared source inventories must be objects")
    if set(frozen_sources) != set(prepared_sources):
        raise TopologyError("prepared topology changes the source path set")
    if git_head(frozen_root) != frozen.get("frozen_product_sha"):
        raise TopologyError("frozen topology does not identify the supplied frozen checkout")
    published_names = {
        entry.get("name")
        for entry in frozen.get("published_crates", [])
        if isinstance(entry, dict) and isinstance(entry.get("name"), str)
    }
    published_paths = {
        (Path(entry["package_path"]) / "Cargo.toml").as_posix()
        for entry in frozen.get("published_crates", [])
        if isinstance(entry, dict) and isinstance(entry.get("package_path"), str)
    }
    ensure_committed_topology_inputs(frozen_root, sorted(frozen_sources))
    ensure_committed_topology_inputs(prepared_root, sorted(prepared_sources))
    workspace_manifests = sorted(
        relative
        for relative in frozen_sources
        if relative != "Cargo.toml" and Path(relative).name == "Cargo.toml"
    )
    frozen_inherited = workspace_inherited_package_names(
        frozen_root, workspace_manifests
    )
    prepared_inherited = workspace_inherited_package_names(
        prepared_root, workspace_manifests
    )
    if frozen_inherited != prepared_inherited:
        raise TopologyError(
            "prepared source changed workspace-inherited package membership"
        )
    version_sources = version_derived_source_paths(
        published_paths | set(workspace_manifests)
    )
    frozen_release = frozen.get("release")
    prepared_release = prepared.get("release")
    if not isinstance(frozen_release, str) or not isinstance(prepared_release, str):
        raise TopologyError("frozen/prepared release identities must be strings")
    for relative in sorted(frozen_sources):
        frozen_source = frozen_sources[relative]
        prepared_source = prepared_sources[relative]
        if not isinstance(frozen_source, dict) or not isinstance(prepared_source, dict):
            raise TopologyError(f"source inventory entry is not an object: {relative}")
        frozen_path = frozen_root / relative
        prepared_path = prepared_root / relative
        if not frozen_path.is_file() or not prepared_path.is_file():
            raise TopologyError(f"prepared source is missing: {relative}")
        if sha256(frozen_path) != frozen_source.get("sha256"):
            raise TopologyError(f"frozen source hash is stale: {relative}")
        if sha256(prepared_path) != prepared_source.get("sha256"):
            raise TopologyError(f"prepared source hash is stale: {relative}")
        if relative not in version_sources:
            if frozen_path.read_bytes() != prepared_path.read_bytes():
                raise TopologyError(
                    f"prepared source changed outside version metadata: {relative}"
                )
            continue
        if normalized_source_value(
            frozen_root,
            relative,
            frozen_release,
            published_names,
            published_paths,
            set(frozen_inherited),
        ) != normalized_source_value(
            prepared_root,
            relative,
            prepared_release,
            published_names,
            published_paths,
            set(frozen_inherited),
        ):
            raise TopologyError(
                f"prepared source changed outside version metadata: {relative}"
            )


def validate_prepared_projection(
    frozen: dict[str, Any],
    prepared: dict[str, Any],
    frozen_digest: str,
    frozen_topology_path: Path,
    frozen_root: Path,
    prepared_root: Path,
) -> None:
    authoritative, actual_digest = load_frozen_authority(frozen_topology_path, frozen_digest)
    if authoritative != frozen or actual_digest != frozen_digest:
        raise TopologyError("frozen topology authority bytes do not match supplied baseline")
    schema_validate(frozen, frozen_root)
    schema_validate(prepared, prepared_root)
    if topology_schema_version(frozen.get("schema")) != topology_schema_version(
        prepared.get("schema")
    ):
        raise TopologyError("frozen/prepared topology schema versions must match")
    if frozen.get("prepared_swarm_sha") is not None:
        raise TopologyError("frozen topology must not already bind prepared_swarm_sha")
    if frozen.get("frozen_product_sha") != prepared.get("frozen_product_sha"):
        raise TopologyError("prepared topology does not retain frozen_product_sha")
    validate_source_transition(frozen, prepared, frozen_root, prepared_root)
    frozen_subjects = deepcopy(frozen)
    prepared_subjects = deepcopy(prepared)
    frozen_subjects.pop("sources", None)
    prepared_subjects.pop("sources", None)
    if immutable_projection(frozen_subjects) != immutable_projection(prepared_subjects):
        raise TopologyError(
            "prepared topology changes an immutable frozen product subject "
            f"(frozen digest {frozen_digest})"
        )


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
    frozen_topology: dict[str, Any] | None = None,
    frozen_topology_digest: str | None = None,
    frozen_topology_path: Path | None = None,
    frozen_root: Path | None = None,
    *,
    schema_version: int = SCHEMA,
) -> dict[str, Any]:
    schema_version = topology_schema_version(schema_version)
    if frozen_topology is not None:
        schema_validate(frozen_topology, frozen_root)
    if frozen_topology is not None and prepared_swarm_sha is None:
        raise TopologyError(
            "frozen topology authority is only valid for prepared validation"
        )
    if not re.fullmatch(r"[0-9a-f]{40}", frozen_product_sha):
        raise TopologyError("frozen_product_sha must be a full lowercase commit SHA")
    current_sha = git_head(root)
    if prepared_swarm_sha is None and current_sha != frozen_product_sha:
        raise TopologyError(
            "frozen_product_sha must identify the exact checkout being inventoried"
        )
    if prepared_swarm_sha is not None:
        if not re.fullmatch(r"[0-9a-f]{40}", prepared_swarm_sha):
            raise TopologyError(
                "prepared_swarm_sha must be a full lowercase commit SHA"
            )
        if prepared_swarm_sha == frozen_product_sha:
            raise TopologyError(
                "prepared_swarm_sha must differ from frozen_product_sha for a prepared topology"
            )
        if prepared_swarm_sha != current_sha:
            raise TopologyError(
                "prepared_swarm_sha must identify the exact prepared checkout"
            )
        if (
            frozen_topology is None
            or frozen_topology_digest is None
            or frozen_topology_path is None
            or frozen_root is None
        ):
            raise TopologyError(
                "prepared topology requires explicit frozen topology bytes, digest, and checkout"
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
    workspace_manifests = workspace_member_manifest_paths(metadata, root)
    for entry in crates:
        entry["package_path"] = (
            Path(entry["package_path"]).resolve().relative_to(root.resolve()).as_posix()
        )
    ensure_committed_topology_inputs(
        root, source_paths(crates, workspace_manifests, schema_version)
    )
    manifest = {
        "schema": schema_version,
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
            for relative, digest in source_hashes_for_paths(
                root, source_paths(crates, workspace_manifests, schema_version)
            ).items()
        },
    }
    if schema_version == 2:
        manifest["checksum_assets"] = derive_checksum_assets(workflow, targets)
    if frozen_topology is not None:
        if frozen_topology_digest is None:
            raise TopologyError("frozen topology digest is missing")
        validate_prepared_projection(
            frozen_topology,
            manifest,
            frozen_topology_digest,
            frozen_topology_path,
            frozen_root,
            root,
        )
    return manifest


def validate_manifest(
    manifest: dict[str, Any],
    root: Path,
    expected_sha: str | None = None,
    expected_prepared_sha: str | None = None,
    frozen_topology: dict[str, Any] | None = None,
    frozen_topology_digest: str | None = None,
    frozen_topology_path: Path | None = None,
    frozen_root: Path | None = None,
) -> None:
    schema_validate(manifest, root)
    if frozen_topology is not None:
        schema_validate(frozen_topology, frozen_root)
    if frozen_topology is not None and expected_prepared_sha is None:
        raise TopologyError(
            "frozen topology authority is only valid for prepared validation"
        )
    schema_version = topology_schema_version(manifest.get("schema"))
    if expected_sha is not None and manifest.get("frozen_product_sha") != expected_sha:
        raise TopologyError(
            "manifest frozen_product_sha differs from the reviewed candidate SHA"
        )
    current_sha = git_head(root)
    if expected_prepared_sha is None and expected_sha is not None and current_sha != expected_sha:
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
        if prepared_swarm_sha == manifest.get("frozen_product_sha"):
            raise TopologyError(
                "prepared_swarm_sha must differ from frozen_product_sha"
            )
        if expected_prepared_sha is None:
            raise TopologyError(
                "prepared_swarm_sha requires an explicit prepared validation authority"
            )
        if prepared_swarm_sha != current_sha:
            raise TopologyError(
                "prepared_swarm_sha must identify the exact checkout being validated"
            )
    elif current_sha != manifest.get("frozen_product_sha"):
        raise TopologyError(
            "frozen_product_sha must identify the exact checkout being validated"
        )
    if expected_prepared_sha is not None:
        if prepared_swarm_sha != expected_prepared_sha:
            raise TopologyError(
                "manifest prepared_swarm_sha differs from the reviewed prepared SHA"
            )
        if prepared_swarm_sha == expected_sha:
            raise TopologyError(
                "prepared_swarm_sha must differ from frozen_product_sha"
            )
        if (
            frozen_topology is None
            or frozen_topology_digest is None
            or frozen_topology_path is None
            or frozen_root is None
        ):
            raise TopologyError(
                "prepared validation requires explicit frozen topology bytes, digest, and checkout"
            )
        validate_prepared_projection(
            frozen_topology,
            manifest,
            frozen_topology_digest,
            frozen_topology_path,
            frozen_root,
            root,
        )
    release = manifest.get("release")
    if not isinstance(release, str):
        raise TopologyError("manifest release is missing")
    metadata = cargo_metadata(root)
    expected_crates = derive_crates(metadata)
    workspace_manifests = workspace_member_manifest_paths(metadata, root)
    for entry in expected_crates:
        entry["package_path"] = (
            Path(entry["package_path"]).resolve().relative_to(root.resolve()).as_posix()
        )
    crates = manifest.get("published_crates")
    if not isinstance(crates, list) or manifest.get("crate_count") != len(crates):
        raise TopologyError("crate_count must be derived from published_crates")
    ensure_committed_topology_inputs(
        root, source_paths(crates, workspace_manifests, schema_version)
    )
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
    if schema_version == 2 and manifest.get("checksum_assets") != derive_checksum_assets(
        workflow, expected_targets
    ):
        raise TopologyError("checksum_assets does not match the public archive checksum inventory")
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
    expected_source_paths = set(
        source_paths(expected_crates, workspace_manifests, schema_version)
    )
    manifest_source_paths = set(sources)
    if manifest_source_paths != expected_source_paths:
        raise TopologyError(
            "source hash set does not match the topology source set: "
            f"missing={sorted(expected_source_paths - manifest_source_paths)}, "
            f"extra={sorted(manifest_source_paths - expected_source_paths)}"
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
        "--schema-version", type=int, choices=(1, 2), default=SCHEMA,
        help="topology contract to generate or check (default: 1; checksums require 2)",
    )
    parser.add_argument(
        "--release", required=True, help="candidate version, e.g. 0.18.0"
    )
    parser.add_argument("--frozen-product-sha", required=True)
    parser.add_argument("--prepared-swarm-sha")
    parser.add_argument(
        "--frozen-topology",
        type=Path,
        help="exact reviewed frozen topology JSON (required for prepared validation)",
    )
    parser.add_argument(
        "--frozen-topology-sha256",
        help="SHA-256 digest of --frozen-topology (required for prepared validation)",
    )
    parser.add_argument(
        "--frozen-root",
        type=Path,
        help="exact frozen checkout F (required for prepared validation)",
    )
    parser.add_argument(
        "--root",
        type=Path,
        help="checkout to inventory (defaults to the script's repository)",
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--check", action="store_true", help="validate an existing output"
    )
    args = parser.parse_args()
    root = (args.root or Path(__file__).resolve().parents[1]).resolve()
    try:
        if args.frozen_topology is None and (
            args.frozen_topology_sha256 is not None or args.frozen_root is not None
        ):
            raise TopologyError(
                "frozen topology auxiliary arguments require --frozen-topology"
            )
        if args.check:
            manifest = load_manifest(args.output)
            schema_validate(manifest, root)
            if topology_schema_version(manifest.get("schema")) != args.schema_version:
                raise TopologyError("manifest schema differs from --schema-version")
            frozen_topology = None
            frozen_digest = None
            if args.frozen_topology is not None:
                frozen_topology, frozen_digest = load_frozen_authority(
                    args.frozen_topology, args.frozen_topology_sha256
                )
                schema_validate(frozen_topology, args.frozen_root or root)
            if manifest.get("release") != args.release:
                raise TopologyError("manifest release differs from --release")
            validate_manifest(
                manifest,
                root,
                args.frozen_product_sha,
                args.prepared_swarm_sha,
                frozen_topology,
                frozen_digest,
                args.frozen_topology,
                args.frozen_root,
            )
        else:
            frozen_topology = None
            frozen_digest = None
            if args.frozen_topology is not None:
                frozen_topology, frozen_digest = load_frozen_authority(
                    args.frozen_topology, args.frozen_topology_sha256
                )
                schema_validate(frozen_topology, args.frozen_root or root)
            manifest = build_manifest(
                root,
                args.release,
                args.frozen_product_sha,
                args.prepared_swarm_sha,
                frozen_topology,
                frozen_digest,
                args.frozen_topology,
                args.frozen_root,
                schema_version=args.schema_version,
            )
            validate_manifest(
                manifest,
                root,
                args.frozen_product_sha,
                args.prepared_swarm_sha,
                frozen_topology,
                frozen_digest,
                args.frozen_topology,
                args.frozen_root,
            )
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
