"""Finalize an exact-source Zed observation bundle into the shared host receipt."""

from __future__ import annotations

import os
import subprocess
from argparse import Namespace
from pathlib import Path
from typing import Any

from .common import (
    HostReceiptError,
    artifact_reference,
    copy_redacted_text,
    load_json,
    redact_text,
    redactions,
    sha256_file,
    sha256_tree,
    verify_artifact_reference,
    write_json,
)


def _require_unchanged(manifest: dict[str, Any]) -> None:
    checks = [
        (Path(manifest["zed"]["cli"]), manifest["zed"]["cli_sha256"], "Zed CLI"),
        (Path(manifest["zed"]["app"]), manifest["zed"]["app_sha256"], "Zed app"),
        (
            Path(manifest["extension"]["manifest"]),
            manifest["extension"]["manifest_sha256"],
            "extension manifest",
        ),
        (
            Path(manifest["extension"]["wasm"]),
            manifest["extension"]["wasm_sha256"],
            "extension WebAssembly",
        ),
        (
            Path(manifest["perllsp"]["command"]),
            manifest["perllsp"]["binary_sha256"],
            "perllsp",
        ),
        (
            Path(manifest["configuration"]["settings"]),
            manifest["configuration"]["settings_sha256"],
            "Zed settings",
        ),
    ]
    for path, expected, label in checks:
        if sha256_file(path) != expected:
            raise HostReceiptError(f"{label} changed after subject preparation")
    extension = Path(manifest["extension"]["directory"])
    if sha256_tree(extension) != manifest["extension"]["tree_sha256"]:
        raise HostReceiptError("extension checkout changed after subject preparation")
    workspace = Path(manifest["workspace"]["directory"])
    if sha256_tree(workspace, ignored=(".git",)) != manifest["workspace"][
        "fixture_sha256"
    ]:
        raise HostReceiptError("workspace fixture changed after subject preparation")


def _require_run_binding(
    prepared_manifest_sha256: str,
    observations: dict[str, Any],
    launch: dict[str, Any],
    inventory: dict[str, Any],
) -> None:
    bindings = [
        (observations.get("prepared_manifest_sha256"), "observations"),
        (launch.get("prepared_manifest_sha256"), "launch evidence"),
        (inventory.get("prepared_manifest_sha256"), "process inventory"),
    ]
    language_server_log = observations.get("language_server_log")
    if not isinstance(language_server_log, dict):
        raise HostReceiptError("observations.language_server_log must be an object")
    bindings.append(
        (
            language_server_log.get("prepared_manifest_sha256"),
            "language-server log",
        )
    )
    for actual, label in bindings:
        if actual != prepared_manifest_sha256:
            raise HostReceiptError(
                f"{label} is not bound to the current prepared manifest"
            )


def _language_server_source(
    observations: dict[str, Any], prepared_manifest_sha256: str
) -> Path:
    binding = observations.get("language_server_log")
    if not isinstance(binding, dict):
        raise HostReceiptError("observations.language_server_log must be an object")
    if binding.get("prepared_manifest_sha256") != prepared_manifest_sha256:
        raise HostReceiptError(
            "language-server log is not bound to the current prepared manifest"
        )
    source = binding.get("path")
    expected_sha256 = binding.get("sha256")
    if not isinstance(source, str) or not source.strip():
        raise HostReceiptError("observations.language_server_log.path is required")
    if not isinstance(expected_sha256, str) or not expected_sha256.startswith("sha256:"):
        raise HostReceiptError("observations.language_server_log.sha256 is required")
    source_path = Path(source).expanduser().resolve(strict=True)
    if sha256_file(source_path) != expected_sha256:
        raise HostReceiptError("language-server log digest does not match its bytes")
    return source_path


def _cells(
    observations: dict[str, Any],
    group: str,
    replacements: list[tuple[str, str]],
) -> dict[str, Any]:
    value = observations.get(group)
    if not isinstance(value, dict):
        raise HostReceiptError(f"observations.{group} must be an object")
    cells: dict[str, Any] = {}
    allowed = {
        "pass",
        "unsupported",
        "not_proven",
        "fail",
        "legitimate_empty",
        "instrument_failed",
    }
    for name, cell in value.items():
        if not isinstance(cell, dict):
            raise HostReceiptError(f"observations.{group}.{name} must be an object")
        result = cell.get("result")
        evidence = cell.get("evidence")
        if result not in allowed:
            raise HostReceiptError(f"observations.{group}.{name} has invalid result")
        if result == "pass" and (
            not isinstance(evidence, str) or not evidence.strip()
        ):
            raise HostReceiptError(f"passing {group}.{name} requires direct evidence")
        if isinstance(evidence, str):
            evidence = redact_text(evidence, replacements)
        cells[name] = {"result": result, "evidence": evidence}
    return cells


def _validate_with_rust(repo_root: Path, receipt: Path) -> None:
    completed = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "-p",
            "xtask",
            "--bin",
            "validate-zed-host-receipt",
            "--",
            str(receipt),
        ],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
        timeout=300,
    )
    if completed.returncode != 0:
        raise HostReceiptError(
            "shared Rust Zed receipt validator rejected the candidate:\n"
            + completed.stdout
            + completed.stderr
        )


def finalize(args: Namespace, repo_root: Path) -> int:
    run_dir = args.run_dir.expanduser().resolve(strict=True)
    prepared_manifest = run_dir / "manifest.json"
    prepared_manifest_sha256 = sha256_file(prepared_manifest)
    manifest = load_json(prepared_manifest)
    observations = load_json(args.observations or run_dir / "observations.json")
    launch = load_json(run_dir / "launch.json")
    process_inventory = run_dir / "artifacts/process-inventory.json"
    inventory = load_json(process_inventory)

    if launch.get("schema_version") != "zed_exact_source_launch.v1":
        raise HostReceiptError("unexpected exact-source launch schema")
    if inventory.get("schema_version") != "zed_exact_source_process_inventory.v1":
        raise HostReceiptError("unexpected exact-source process inventory schema")
    if launch.get("result") != "pass":
        raise HostReceiptError("launch/process isolation did not pass")
    if observations.get("schema_version") != "zed_exact_source_observations.v1":
        raise HostReceiptError("unexpected exact-source observation schema")

    _require_run_binding(
        prepared_manifest_sha256,
        observations,
        launch,
        inventory,
    )
    verify_artifact_reference(
        process_inventory,
        run_dir,
        launch.get("process_inventory"),
        "process inventory",
    )
    _require_unchanged(manifest)

    replacements = redactions(manifest, run_dir)
    template = load_json(
        repo_root / ".ci/fixtures/zed-perl-upstream/receipts/exact-source-template.json"
    )
    result = observations.get("result")
    if result not in {"pass", "fail", "instrument_failed"}:
        raise HostReceiptError(
            "observations.result must be pass, fail, or instrument_failed"
        )
    observed_at = observations.get("observed_at")
    if not isinstance(observed_at, str) or not observed_at.strip():
        raise HostReceiptError("observations.observed_at is required")

    config_observation = observations.get("configuration")
    if not isinstance(config_observation, dict):
        raise HostReceiptError("observations.configuration must be an object")

    raw_stderr = run_dir / "artifacts/zed-foreground.stderr.log"
    verify_artifact_reference(
        raw_stderr,
        run_dir,
        launch.get("stderr"),
        "Zed foreground stderr",
    )
    redacted_stderr = run_dir / "artifacts/zed-foreground.stderr.redacted.log"
    copy_redacted_text(raw_stderr, redacted_stderr, replacements)

    language_server_source_path = _language_server_source(
        observations, prepared_manifest_sha256
    )
    language_server_log = run_dir / "artifacts/language-server.redacted.log"
    copy_redacted_text(
        language_server_source_path,
        language_server_log,
        replacements,
    )

    receipt = template
    receipt["result"] = result
    receipt["observed_at"] = observed_at
    receipt["zed"] = {
        "product": "Zed",
        "version": manifest["zed"]["version"],
        "channel": manifest["zed"]["channel"],
        "build": manifest["zed"]["build"],
    }
    receipt["extension"] = {
        "repository": manifest["extension"]["repository"],
        "base_commit": manifest["extension"]["base_commit"],
        "candidate_commit": manifest["extension"]["candidate_commit"],
        "manifest_version": manifest["extension"]["manifest_version"],
        "wasm_sha256": manifest["extension"]["wasm_sha256"],
        "install_route": "dev_extension",
    }
    receipt["perllsp"] = {
        "server_id": "perllsp",
        "command": "<perllsp>",
        "arguments": ["--stdio"],
        "version": manifest["perllsp"]["version"],
        "build_commit": manifest["perllsp"]["build_commit"],
        "binary_sha256": manifest["perllsp"]["binary_sha256"],
        "resolution_route": manifest["perllsp"]["resolution_route"],
    }
    receipt["platform"] = manifest["platform"]
    receipt["profile"] = {
        "clean_profile": True,
        "prior_extension_absent": True,
        "prior_managed_cache_absent": True,
        "other_perl_servers_disabled": True,
    }
    receipt["workspace"] = {
        "fixture_id": manifest["workspace"]["fixture_id"],
        "fixture_sha256": manifest["workspace"]["fixture_sha256"],
        "root_identity": manifest["workspace"]["root_identity"],
    }
    receipt["configuration"] = {
        "settings_sha256": manifest["configuration"]["settings_sha256"],
        "server_order": manifest["configuration"]["server_order"],
        "workspace_configuration_observed": config_observation.get(
            "workspace_configuration_observed"
        ),
        "precedence_observed": config_observation.get("precedence_observed"),
        "live_update_observed": config_observation.get("live_update_observed"),
    }
    receipt["activation"] = _cells(observations, "activation", replacements)
    receipt["journey"] = _cells(observations, "journey", replacements)
    receipt["artifacts"] = {
        "zed_log": artifact_reference(redacted_stderr, run_dir),
        "language_server_log": artifact_reference(language_server_log, run_dir),
        "process_inventory": artifact_reference(process_inventory, run_dir),
        "redacted": True,
    }
    limitations = observations.get("limitations")
    if not isinstance(limitations, list) or not all(
        isinstance(item, str) for item in limitations
    ):
        raise HostReceiptError("observations.limitations must be a string array")
    receipt["limitations"] = [
        redact_text(item, replacements) for item in limitations
    ]
    receipt["claim_boundary"] = (
        "Exact-source development-extension evidence only. No official-registry, managed-download, cross-platform, or broad Zed support claim follows."
    )

    output = args.output or run_dir / "receipt.json"
    candidate = output.with_suffix(output.suffix + ".candidate")
    write_json(candidate, receipt)
    if result == "pass":
        _validate_with_rust(repo_root, candidate)
    output.parent.mkdir(parents=True, exist_ok=True)
    os.replace(candidate, output)
    print(f"Wrote exact-source Zed receipt: {output}")
    return 0 if result == "pass" else 1
