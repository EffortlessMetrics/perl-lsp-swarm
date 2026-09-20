from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

PACKAGE_ROOT = Path(__file__).resolve().parent
RECORD_PATH = PACKAGE_ROOT / "compatibility.v1.json"
SERVER_MANIFEST_PATH = PACKAGE_ROOT / "server-manifest.json"

COMPATIBILITY_RESULTS = {"compatible", "incompatible", "not_proven"}
CURRENTNESS_RESULTS = {
    "current",
    "update_available",
    "stale_supported",
    "stale_unsupported",
    "withdrawn",
    "not_proven",
}
PUBLIC_READY_CURRENTNESS = {"current", "stale_supported"}
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")


class CompatibilityError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CompatibilityError(message)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise CompatibilityError(f"could not load {label}: {error}") from error
    require(isinstance(value, dict), f"{label} must be an object")
    return value


def load_record(path: Path = RECORD_PATH) -> dict[str, Any]:
    return _load_json(path, "Sublime compatibility record")


def load_server_manifest(path: Path = SERVER_MANIFEST_PATH) -> dict[str, Any]:
    return _load_json(path, "managed server manifest")


def _require_mapping(parent: dict[str, Any], field: str) -> dict[str, Any]:
    value = parent.get(field)
    require(isinstance(value, dict), f"{field} must be an object")
    return value


def _validate_package_subject(package: dict[str, Any], compatibility: str) -> None:
    require(package.get("name") == "LSP-perllsp", "unexpected package name")
    require(
        package.get("source_repository")
        in {"EffortlessMetrics/perl-lsp-swarm", "EffortlessMetrics/LSP-perllsp"},
        "unexpected package source repository",
    )
    require(
        package.get("source_path") in {"clients/sublime/LSP-perllsp", "."},
        "unexpected package source path",
    )
    version = package.get("version")
    tree_sha = package.get("tree_sha256")
    if compatibility == "not_proven":
        require(version is None or isinstance(version, str), "package.version must be null or a string")
        require(tree_sha is None or bool(SHA256.fullmatch(str(tree_sha))), "invalid package tree digest")
    else:
        require(isinstance(version, str) and version, "exact compatibility requires package.version")
        require(bool(SHA256.fullmatch(str(tree_sha))), "exact compatibility requires package.tree_sha256")


def _validate_evidence(record: dict[str, Any], compatibility: str) -> None:
    evidence = record.get("evidence")
    require(isinstance(evidence, list) and evidence, "evidence must be a non-empty array")
    exact_host_evidence = False
    exact_failure_evidence = False
    for index, entry in enumerate(evidence):
        require(isinstance(entry, dict), f"evidence[{index}] must be an object")
        require(isinstance(entry.get("kind"), str) and entry["kind"], f"evidence[{index}].kind is required")
        require(
            isinstance(entry.get("reference"), str) and entry["reference"],
            f"evidence[{index}].reference is required",
        )
        actual_host = entry.get("actual_host")
        exact_pair = entry.get("exact_pair")
        require(isinstance(actual_host, bool), f"evidence[{index}].actual_host must be boolean")
        require(isinstance(exact_pair, bool), f"evidence[{index}].exact_pair must be boolean")
        receipt_sha = entry.get("receipt_sha256")
        if receipt_sha is not None:
            require(bool(SHA256.fullmatch(str(receipt_sha))), f"evidence[{index}] has invalid receipt digest")
        if actual_host and exact_pair and receipt_sha is not None:
            exact_host_evidence = True
        if exact_pair and entry.get("result") == "failed" and receipt_sha is not None:
            exact_failure_evidence = True
    if compatibility == "compatible":
        require(exact_host_evidence, "compatible requires exact actual-host receipt evidence")
    if compatibility == "incompatible":
        require(exact_failure_evidence, "incompatible requires exact failure receipt evidence")


def validate_record(
    record: dict[str, Any],
    *,
    server_manifest_path: Path = SERVER_MANIFEST_PATH,
) -> dict[str, Any]:
    require(record.get("schema_version") == 1, "compatibility schema_version must be 1")
    record_id = record.get("record_id")
    require(isinstance(record_id, str) and record_id, "record_id is required")

    compatibility = record.get("compatibility")
    currentness = record.get("currentness")
    require(compatibility in COMPATIBILITY_RESULTS, "invalid compatibility result")
    require(currentness in CURRENTNESS_RESULTS, "invalid currentness result")

    package = _require_mapping(record, "package")
    client = _require_mapping(record, "client")
    host = _require_mapping(record, "host")
    server = _require_mapping(record, "server")
    policy = _require_mapping(record, "managed_policy")

    _validate_package_subject(package, str(compatibility))

    require(client.get("name") == "Sublime Text LSP", "unexpected LSP client name")
    require(client.get("repository") == "sublimelsp/LSP", "unexpected LSP client repository")
    require(isinstance(client.get("version"), str) and client["version"], "client.version is required")
    require(bool(FULL_SHA.fullmatch(str(client.get("ref", "")))), "client.ref must be a full Git SHA")

    require(host.get("product") == "Sublime Text", "unexpected host product")
    require(
        isinstance(host.get("build_constraint"), str) and host["build_constraint"],
        "host.build_constraint is required",
    )
    platforms = host.get("platforms")
    require(isinstance(platforms, list) and platforms, "host.platforms must be a non-empty array")
    require(
        all(isinstance(item, str) and item for item in platforms),
        "host.platforms entries must be strings",
    )
    require(platforms == sorted(set(platforms)), "host.platforms must be sorted and unique")

    manifest = load_server_manifest(server_manifest_path)
    require(server.get("repository") == manifest.get("repository"), "server repository differs from manifest")
    require(server.get("version") == manifest.get("version"), "server version differs from manifest")
    require(server.get("release_tag") == manifest.get("release_tag"), "server tag differs from manifest")
    require(
        server.get("manifest_path") == "server-manifest.json",
        "server.manifest_path must name the package manifest",
    )
    require(
        server.get("manifest_sha256") == sha256_file(server_manifest_path),
        "server manifest digest differs from compatibility subject",
    )
    require(
        client.get("version") == manifest.get("tested_lsp_package"),
        "LSP client version differs from server manifest",
    )
    require(
        host.get("build_constraint") == manifest.get("tested_sublime_build"),
        "Sublime build constraint differs from server manifest",
    )

    require(policy.get("selection") == "pinned_manifest", "managed selection must remain pinned_manifest")
    require(policy.get("allow_unreviewed_latest") is False, "unreviewed latest selection is forbidden")
    require(
        policy.get("offline_cache") == "verified_exact_only",
        "offline cache policy must remain verified_exact_only",
    )
    require(
        policy.get("failed_update") == "preserve_last_verified_compatible",
        "failed update policy must preserve the last verified compatible binary",
    )
    require(
        policy.get("external_binary_disposition") == "not_proven",
        "external binaries must remain not_proven by default",
    )
    require(
        policy.get("development_not_proven_allowed") is True,
        "pre-public exact-source development must state its not_proven allowance explicitly",
    )

    _validate_evidence(record, str(compatibility))

    if compatibility == "not_proven":
        require(
            currentness == "not_proven",
            "not_proven compatibility cannot be rendered as a current or supported pair",
        )
    if currentness != "not_proven":
        require(
            isinstance(package.get("version"), str)
            and bool(SHA256.fullmatch(str(package.get("tree_sha256", "")))),
            "currentness requires an exact package subject",
        )
    if currentness == "withdrawn":
        require(compatibility != "compatible", "withdrawn cannot remain compatible")

    limitations = record.get("limitations")
    require(isinstance(limitations, list), "limitations must be an array")
    require(
        all(isinstance(item, str) and item for item in limitations),
        "limitations entries must be non-empty strings",
    )
    return record


def assert_managed_install_allowed(
    record: dict[str, Any] | None = None,
    *,
    require_public_ready: bool = False,
    server_manifest_path: Path = SERVER_MANIFEST_PATH,
) -> dict[str, Any]:
    record = record or load_record()
    validate_record(record, server_manifest_path=server_manifest_path)
    compatibility = record["compatibility"]
    currentness = record["currentness"]
    if compatibility == "incompatible":
        raise CompatibilityError("The pinned LSP-perllsp/perllsp pair is explicitly incompatible.")
    if currentness in {"withdrawn", "stale_unsupported"}:
        raise CompatibilityError(f"The pinned managed server subject is {currentness}.")
    if require_public_ready:
        require(compatibility == "compatible", "public package requires an exact compatible pair")
        require(
            currentness in PUBLIC_READY_CURRENTNESS,
            "public package requires current or stale_supported currentness",
        )
    return record


def summary(record: dict[str, Any] | None = None) -> dict[str, Any]:
    record = validate_record(record or load_record())
    return {
        "record_id": record["record_id"],
        "compatibility": record["compatibility"],
        "currentness": record["currentness"],
        "package": record["package"],
        "client": record["client"],
        "server": record["server"],
        "limitations": record["limitations"],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Validate LSP-perllsp compatibility/currentness")
    parser.add_argument("command", choices=("check", "status"))
    parser.add_argument("--require-public-ready", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    try:
        record = assert_managed_install_allowed(
            require_public_ready=args.require_public_ready,
        )
        if args.command == "status":
            payload = summary(record)
            if args.json:
                print(json.dumps(payload, indent=2, sort_keys=True))
            else:
                print(
                    f"{payload['record_id']}: "
                    f"compatibility={payload['compatibility']} "
                    f"currentness={payload['currentness']}"
                )
        elif args.json:
            print(json.dumps(summary(record), indent=2, sort_keys=True))
        else:
            print(f"validated {record['record_id']}")
    except CompatibilityError as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
