"""Prepare one isolated exact-source Zed host subject."""

from __future__ import annotations

import re
import shutil
from argparse import Namespace
from pathlib import Path
from typing import Any

import tomllib

from .common import (
    HostReceiptError,
    canonical_dir,
    canonical_file,
    load_json,
    platform_identity,
    require_clean_git_checkout,
    run_checked,
    sha256_file,
    sha256_tree,
    write_json,
)

_ZED_CHANNELS = {
    "Zed": "stable",
    "Zed Dev": "dev",
    "Zed Nightly": "nightly",
    "Zed Preview": "preview",
}
_COMMIT = re.compile(r"^[0-9a-f]{7,40}$")
_FULL_COMMIT = re.compile(r"^[0-9a-f]{40}$")


def _load_extension_manifest(path: Path) -> dict[str, Any]:
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise HostReceiptError(
            f"could not read extension manifest {path}: {error}"
        ) from error
    try:
        manifest = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise HostReceiptError(
            f"extension manifest is not valid TOML: {path}: {error}"
        ) from error
    if not isinstance(manifest, dict):
        raise HostReceiptError(f"extension manifest must be a TOML table: {path}")
    return manifest


def _bound_wasm(extension_dir: Path, wasm: Path) -> tuple[Path, str]:
    try:
        relative = wasm.relative_to(extension_dir)
    except ValueError as error:
        raise HostReceiptError(
            "extension WebAssembly must resolve inside the extension checkout"
        ) from error
    if not relative.parts:
        raise HostReceiptError(
            "extension WebAssembly must be a file below the extension checkout"
        )
    return relative, sha256_file(wasm)


def _parse_zed_identity(
    cli_output: str,
    system_specs: str,
    zed_app: Path,
    expected_version: str,
    expected_channel: str,
    expected_build: str,
) -> dict[str, str]:
    cli_line = next(
        (line.strip() for line in cli_output.splitlines() if line.strip()), ""
    )
    cli_match = re.fullmatch(r"Zed (?P<version>[^ ]+) [–-] (?P<path>.+)", cli_line)
    if cli_match is None:
        raise HostReceiptError("Zed CLI version output has no canonical identity line")
    reported_path = Path(cli_match.group("path")).expanduser().resolve()
    if reported_path != zed_app:
        raise HostReceiptError(
            f"Zed CLI selected {reported_path}, not the prepared application {zed_app}"
        )

    specs_line = next(
        (
            line.strip()
            for line in system_specs.splitlines()
            if line.startswith("Zed: ")
        ),
        "",
    )
    specs_match = re.match(
        r"Zed: v(?P<version>[^ ]+) \((?P<channel>Zed(?: Dev| Nightly| Preview)?)\)",
        specs_line,
    )
    if specs_match is None:
        raise HostReceiptError(
            "Zed application system specs have no canonical identity line"
        )

    version = specs_match.group("version")
    channel_display = specs_match.group("channel")
    channel = _ZED_CHANNELS[channel_display]
    base_version, separator, metadata = version.partition("+")
    if not separator or not metadata:
        raise HostReceiptError("Zed system specs do not contain release build metadata")
    metadata_parts = metadata.split(".")
    if len(metadata_parts) < 2 or metadata_parts[0] != channel or not metadata_parts[1]:
        raise HostReceiptError(
            "Zed system specs build metadata does not match its channel"
        )
    build = metadata_parts[1]
    commit = metadata_parts[2] if len(metadata_parts) == 3 else ""
    if commit and not _COMMIT.fullmatch(commit):
        raise HostReceiptError("Zed system specs contain an invalid commit identity")

    if cli_match.group("version") != base_version or base_version != expected_version:
        raise HostReceiptError(
            "Zed version output does not match the authoritative system specs"
        )
    if channel != expected_channel:
        raise HostReceiptError(
            "Zed channel does not match the authoritative system specs"
        )
    if expected_build not in {build, metadata}:
        raise HostReceiptError(
            "Zed build does not match the authoritative system specs"
        )
    return {
        "version": base_version,
        "channel": channel,
        "build": metadata,
        "build_id": build,
        "commit_sha": commit,
        "cli_output": cli_output,
        "system_specs": system_specs,
    }


def _parse_perllsp_identity(
    output: str, expected_version: str, expected_build: str
) -> str:
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    if not lines or lines[0] != f"perllsp {expected_version}":
        raise HostReceiptError(
            "perllsp version output does not match the prepared subject"
        )
    revision_lines = [line for line in lines[1:] if line.startswith("Git commit: ")]
    if len(revision_lines) != 1:
        raise HostReceiptError(
            "perllsp version output must contain one embedded Git commit"
        )
    embedded = revision_lines[0].removeprefix("Git commit: ").strip().lower()
    if not _COMMIT.fullmatch(embedded):
        raise HostReceiptError("perllsp embedded Git commit is not a valid revision")
    if not _FULL_COMMIT.fullmatch(expected_build.lower()):
        raise HostReceiptError("--perllsp-build must be a full Git commit")
    if not expected_build.lower().startswith(embedded):
        raise HostReceiptError("perllsp binary revision does not match --perllsp-build")
    return embedded


def _empty_run_dir(path: Path) -> Path:
    resolved = path.expanduser().resolve()
    if resolved.exists() and any(resolved.iterdir()):
        raise HostReceiptError(
            f"run directory must not contain prior state: {resolved}"
        )
    resolved.mkdir(parents=True, exist_ok=True)
    return resolved


def _settings(perl_settings: dict[str, Any], perllsp: Path) -> dict[str, Any]:
    return {
        "languages": {
            "Perl": {
                "language_servers": [
                    "perllsp",
                    "!perlnavigator-server",
                    "!perl-lsp",
                    "...",
                ]
            }
        },
        "lsp": {
            "perllsp": {
                "binary": {"path": str(perllsp), "arguments": []},
                "settings": {"perl": perl_settings},
            }
        },
    }


def prepare(args: Namespace, repo_root: Path) -> int:
    run_dir = _empty_run_dir(args.run_dir)
    zed_cli = canonical_file(args.zed_cli, "Zed CLI")
    zed_app = canonical_file(args.zed_app, "Zed application binary")
    extension_dir = canonical_dir(args.extension_dir, "extension checkout")
    perllsp = canonical_file(args.perllsp, "perllsp candidate")
    wasm = canonical_file(args.wasm, "extension WebAssembly")
    workspace = canonical_dir(args.workspace, "workspace fixture")
    require_clean_git_checkout(
        extension_dir, args.extension_candidate, args.extension_base
    )

    manifest_path = extension_dir / "extension.toml"
    manifest = _load_extension_manifest(manifest_path)
    if manifest.get("id") != "perl":
        raise HostReceiptError("extension manifest id must remain `perl`")
    if manifest.get("version") != args.extension_version:
        raise HostReceiptError(
            "extension manifest version does not match the prepared subject"
        )
    if manifest.get("repository") != "https://github.com/tree-sitter-perl/zed-perl":
        raise HostReceiptError(
            "extension manifest repository is not the reviewed upstream subject"
        )
    wasm_relative, wasm_sha256 = _bound_wasm(extension_dir, wasm)

    zed_version_output = run_checked([str(zed_cli), "--zed", str(zed_app), "--version"])
    zed_system_specs = run_checked([str(zed_app), "--system-specs"])
    zed_identity = _parse_zed_identity(
        zed_version_output,
        zed_system_specs,
        zed_app,
        args.zed_version,
        args.zed_channel,
        args.zed_build,
    )
    perllsp_version_output = run_checked([str(perllsp), "--version"])
    perllsp_revision = _parse_perllsp_identity(
        perllsp_version_output, args.perllsp_version, args.perllsp_build
    )

    perl_settings: dict[str, Any] = {}
    if args.perl_settings is not None:
        perl_settings = load_json(args.perl_settings)
        if "perl" in perl_settings:
            raise HostReceiptError(
                "--perl-settings must contain the object below the canonical `perl` root"
            )

    profile = run_dir / "profile"
    config = profile / "config"
    artifacts = run_dir / "artifacts"
    config.mkdir(parents=True)
    artifacts.mkdir()
    settings_path = config / "settings.json"
    write_json(settings_path, _settings(perl_settings, perllsp))

    observations = (
        repo_root
        / ".ci/fixtures/zed-perl-upstream/receipts/exact-source-observations-template.json"
    )
    shutil.copyfile(observations, run_dir / "observations.json")

    subject = {
        "schema_version": "zed_exact_source_run.v1",
        "zed": {
            "cli": str(zed_cli),
            "cli_sha256": sha256_file(zed_cli),
            "app": str(zed_app),
            "app_sha256": sha256_file(zed_app),
            "version": zed_identity["version"],
            "channel": zed_identity["channel"],
            "build": zed_identity["build"],
            "build_id": zed_identity["build_id"],
            "commit_sha": zed_identity["commit_sha"],
            "version_output": zed_identity["cli_output"],
            "system_specs": zed_identity["system_specs"],
        },
        "extension": {
            "directory": str(extension_dir),
            "tree_sha256": sha256_tree(extension_dir),
            "repository": "tree-sitter-perl/zed-perl",
            "base_commit": args.extension_base,
            "candidate_commit": args.extension_candidate,
            "manifest_version": args.extension_version,
            "manifest": str(manifest_path),
            "manifest_sha256": sha256_file(manifest_path),
            "wasm": str(wasm),
            "wasm_relative_path": wasm_relative.as_posix(),
            "wasm_sha256": wasm_sha256,
            "install_action": "zed::InstallDevExtension",
        },
        "perllsp": {
            "server_id": "perllsp",
            "command": str(perllsp),
            "arguments": ["--stdio"],
            "version": args.perllsp_version,
            "build_commit": args.perllsp_build,
            "embedded_revision": perllsp_revision,
            "binary_sha256": sha256_file(perllsp),
            "resolution_route": args.resolution_route,
            "version_output": perllsp_version_output,
        },
        "platform": platform_identity(),
        "profile": {
            "directory": str(profile),
            "clean_profile": True,
            "prior_extension_absent": True,
            "prior_managed_cache_absent": True,
            "other_perl_servers_disabled": True,
        },
        "workspace": {
            "directory": str(workspace),
            "fixture_id": args.fixture_id,
            "fixture_sha256": sha256_tree(workspace, ignored=(".git",)),
            "root_identity": args.root_identity or workspace.name,
        },
        "configuration": {
            "settings": str(settings_path),
            "settings_sha256": sha256_file(settings_path),
            "server_order": [
                "perllsp",
                "!perlnavigator-server",
                "!perl-lsp",
                "...",
            ],
        },
    }
    write_json(run_dir / "manifest.json", subject)
    instructions = f"""# Exact-source Zed session\n\n1. Run the launch command emitted by this driver.\n2. In Zed invoke `zed::InstallDevExtension`.\n3. Select `{extension_dir}`.\n4. Complete every directly observed cell in `observations.json`.\n5. Export or locate the language-server log and set `language_server_log`.\n6. Close the Zed window normally.\n7. Finalize and validate the receipt.\n\nDo not copy the extension into Zed's internal extension directories.\n"""
    (run_dir / "INSTRUCTIONS.md").write_text(instructions, encoding="utf-8")
    print(f"Prepared exact-source Zed run: {run_dir}")
    print(f"Observations: {run_dir / 'observations.json'}")
    return 0
