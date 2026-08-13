"""Prepare one isolated exact-source Zed host subject."""

from __future__ import annotations

import shutil
import tomllib
from argparse import Namespace
from pathlib import Path
from typing import Any

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


def _empty_run_dir(path: Path) -> Path:
    resolved = path.expanduser().resolve()
    if resolved.exists() and any(resolved.iterdir()):
        raise HostReceiptError(f"run directory must not contain prior state: {resolved}")
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
    require_clean_git_checkout(extension_dir, args.extension_candidate, args.extension_base)

    manifest_path = extension_dir / "extension.toml"
    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("id") != "perl":
        raise HostReceiptError("extension manifest id must remain `perl`")
    if manifest.get("version") != args.extension_version:
        raise HostReceiptError("extension manifest version does not match the prepared subject")

    zed_version_output = run_checked(
        [str(zed_cli), "--zed", str(zed_app), "--version"]
    )
    if args.zed_version not in zed_version_output:
        raise HostReceiptError("Zed version output does not match --zed-version")
    perllsp_version_output = run_checked([str(perllsp), "--version"])
    if "perllsp" not in perllsp_version_output.lower() or args.perllsp_version not in perllsp_version_output:
        raise HostReceiptError("perllsp version output does not match the prepared subject")

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

    observations = repo_root / ".ci/fixtures/zed-perl-upstream/receipts/exact-source-observations-template.json"
    shutil.copyfile(observations, run_dir / "observations.json")

    subject = {
        "schema_version": "zed_exact_source_run.v1",
        "zed": {
            "cli": str(zed_cli),
            "cli_sha256": sha256_file(zed_cli),
            "app": str(zed_app),
            "app_sha256": sha256_file(zed_app),
            "version": args.zed_version,
            "channel": args.zed_channel,
            "build": args.zed_build,
            "version_output": zed_version_output,
        },
        "extension": {
            "directory": str(extension_dir),
            "tree_sha256": sha256_tree(extension_dir),
            "repository": "tree-sitter-perl/zed-perl",
            "base_commit": args.extension_base,
            "candidate_commit": args.extension_candidate,
            "manifest_version": args.extension_version,
            "wasm": str(wasm),
            "wasm_sha256": sha256_file(wasm),
            "install_action": "zed::InstallDevExtension",
        },
        "perllsp": {
            "server_id": "perllsp",
            "command": str(perllsp),
            "arguments": ["--stdio"],
            "version": args.perllsp_version,
            "build_commit": args.perllsp_build,
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
