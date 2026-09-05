"""Fail-closed validation for the Lite XL public-artifact replay receipt (#9012).

This is the #9012 consumer surface for the landed #11178 journey/evidence
ledger (train head `61b689077`). It binds the committed upstream-acceptance
manifest as the only authority for released/public subjects, the committed
exact-source receipts directory as the only authority for a #9008-style
exact-source pass, and re-derives the journey inventory live from the landed
ledger bytes, so every identity-collapse, overclaim, substitution,
stale-subject, and orphan mutation fails closed with the exact defect named.

Stage separation mirrors the Zed ladder:

- an exact-source dev-extension receipt (#9008 family) can never satisfy this
  stage;
- a staged managed-package candidate (#9010) can never satisfy this stage;
- a public pass requires released/public identities downloaded through the
  claimed route — no source checkout, Cargo target artifact, PATH injection,
  hand-populated package cache, or private checksum override;
- manual public-binary and managed-`lpm` routes stay separate cells that
  never infer each other;
- shutdown/orphan truth is load-bearing: an unknown cleanup never passes.
"""

from __future__ import annotations

import datetime as dt
import hashlib
import json
import tomllib
from pathlib import Path
from typing import Any

from . import ledger as ledger_mod

PUBLIC_RECEIPT_SCHEMA = "lite_xl_public_artifact_replay_receipt.v1"
PUBLIC_STAGE = "released_public_artifact"
EXACT_SOURCE_STAGE = "exact_source_dev_extension"
STAGED_MANAGED_STAGE = "staged_managed_package"

EXACT_SOURCE_RECEIPT_SCHEMA = "lite_xl_host_compat.v1"

ACCEPTANCE_MANIFEST_SCHEMA = "lite-xl-upstream-acceptance.v1"

RECEIPT_RELATIVE_PATH = ".ci/fixtures/lite-xl-perl-upstream/receipts/public-replay.v1.json"
MANIFEST_RELATIVE_PATH = ".ci/fixtures/lite-xl-perl-upstream/upstream-acceptance.toml"
LEDGER_RELATIVE_PATH = ".spec/11178-lite-xl-bdd-journeys/acceptance.md"

PUBLIC_RESULTS = {
    "not_run",
    "blocked_external",
    "pass",
    "fail",
    "not_proven",
    "instrument_failed",
    "contract_stale",
}

CELL_RESULTS = {"pass", "fail", "not_proven", "unsupported", "instrument_failed", "client_not_exposed"}

# The only install routes a public pass admits. A source checkout, a worktree
# candidate directory, a Cargo target artifact, or an explicit PATH override
# is a #9012 developer shortcut and always fails closed.
INSTALL_ROUTES = {"manual_public_install", "lpm_managed"}
FORBIDDEN_INSTALL_ROUTES = {
    "source_checkout",
    "worktree_candidate",
    "cargo_target",
    "path_override",
    "dev_extension",
}

# Entry-gate accounting retained with the receipt. Subject gates are
# live-bound to the committed upstream-acceptance manifest; the exact-source
# gate is live-bound to the committed receipts directory.
EXACT_SOURCE_GATE_CELL = "exact_source_lite_xl_receipt"
GATE_CELLS = [
    EXACT_SOURCE_GATE_CELL,
    "released_lite_xl_build",
    "public_lite_xl_lsp_package_release",
    "public_language_perl_package_release",
    "public_lsp_perl_package_release",
    "public_perllsp_release_asset",
]

# Gates whose truth is live-bound to the committed acceptance manifest,
# named explicitly so a GATE_CELLS reorder cannot change what binds where.
SUBJECT_GATE_CELLS = [cell for cell in GATE_CELLS if cell != EXACT_SOURCE_GATE_CELL]

GATE_STATES = {"absent", "stale", "current"}

PACKAGE_CELLS = [
    "public_lite_xl_lsp_package_release",
    "public_language_perl_package_release",
    "public_lsp_perl_package_release",
]

PROFILE_CELLS = [
    "candidate_source_checkout_absent",
    "manual_plugin_replacement_absent",
    "prior_public_package_state_absent",
    "hand_populated_package_cache_absent",
    "explicit_server_binary_override_absent",
    "path_candidate_satisfying_public_row_absent",
    "other_perl_server_selected_absent",
    "relabeled_receipt_absent",
]

DISCRIMINATOR_CELLS = [
    "wrong_root_same_basename_rejected",
    "second_perl_server_absent",
    "ambient_path_satisfies_no_row",
    "managed_row_not_satisfied_by_ambient_path",
]


class ReceiptError(RuntimeError):
    """A bounded, user-actionable replay-receipt failure."""


def load_json(path: Path) -> dict[str, Any]:
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, ValueError) as error:
        raise ReceiptError(f"cannot read {path}: {error}") from error
    try:
        value = json.loads(text)
    except json.JSONDecodeError as error:
        raise ReceiptError(f"{path} is not valid JSON: {error}") from error
    if not isinstance(value, dict):
        raise ReceiptError(f"{path} must contain a JSON object")
    return value


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def parse_digest(value: Any, context: str) -> str:
    prefix = "sha256:"
    if not isinstance(value, str) or not value.startswith(prefix):
        raise ReceiptError(f"{context} must be a `sha256:` digest")
    hex_part = value[len(prefix) :]
    if len(hex_part) != 64 or any(c not in "0123456789abcdef" for c in hex_part):
        raise ReceiptError(f"{context} must be a lowercase 64-hex sha256 digest")
    return value


def parse_timestamp(value: Any, context: str) -> str:
    if not isinstance(value, str):
        raise ReceiptError(f"{context} must be an RFC 3339 timestamp")
    try:
        dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        raise ReceiptError(f"{context} must be an RFC 3339 timestamp") from None
    return value


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as handle:
            manifest = tomllib.load(handle)
    except OSError as error:
        raise ReceiptError(f"cannot read {path}: {error}") from error
    except tomllib.TOMLDecodeError as error:
        raise ReceiptError(f"{path} is not valid TOML: {error}") from error
    if not isinstance(manifest, dict):
        raise ReceiptError(f"{path} must contain a TOML table")
    if manifest.get("schema_version") != ACCEPTANCE_MANIFEST_SCHEMA:
        raise ReceiptError(f"{path} lacks the lite-xl acceptance manifest schema")
    return manifest


def upstream_subject(manifest: dict[str, Any]) -> dict[str, Any]:
    """Evaluate the merged-and-released upstream acceptance predicate.

    The predicate follows the DU01 shape the train contract names: each
    external subject counts independently, and submission/merge metadata alone
    never satisfies it. Journeys keep their route separation at the receipt
    level while gate accounting tracks each subject honestly.
    """
    hosts = manifest.get("hosts")
    packages = manifest.get("packages")
    server = manifest.get("server")
    validation = manifest.get("validation")
    for table in (hosts, packages, server, validation):
        if not isinstance(table, dict):
            raise ReceiptError("acceptance manifest lacks its hosts/packages/server tables")

    host = hosts.get("lite_xl") if isinstance(hosts.get("lite_xl"), dict) else {}
    lsp_pkg = packages.get("lite_xl_lsp") if isinstance(packages.get("lite_xl_lsp"), dict) else {}
    lang_pkg = (
        packages.get("language_perl") if isinstance(packages.get("language_perl"), dict) else {}
    )
    managed_pkg = packages.get("lsp_perl") if isinstance(packages.get("lsp_perl"), dict) else {}
    perllsp = server.get("perllsp") if isinstance(server.get("perllsp"), dict) else {}

    ready = manifest.get("ready") is True

    def _released(table: dict[str, Any], needs_ref: bool) -> bool:
        version = str(table.get("version") or "")
        digest = str(table.get("sha256") or "")
        ref_ok = str(table.get("ref") or "") != "" if needs_ref else True
        return bool(
            table.get("state") == "released"
            and version
            and ref_ok
            and digest.startswith("sha256:")
            and validation.get("package_versions_match_refs") is True
        )

    host_release = bool(
        host.get("state") == "released"
        and str(host.get("released_build") or "")
        and validation.get("host_release_contains_changes") is True
    )
    lite_xl_lsp_release = _released(lsp_pkg, needs_ref=True)
    language_perl_release = _released(lang_pkg, needs_ref=True)
    lsp_perl_release = _released(managed_pkg, needs_ref=True)
    perllsp_asset = bool(
        perllsp.get("state") == "released"
        and str(perllsp.get("release_tag") or "")
        and str(perllsp.get("asset_name") or "")
        and str(perllsp.get("asset_sha256") or "").startswith("sha256:")
        and validation.get("server_asset_digest_verified") is True
    )
    return {
        "ready": ready,
        "released_lite_xl_build": host_release and ready,
        "public_lite_xl_lsp_package_release": lite_xl_lsp_release and ready,
        "public_language_perl_package_release": language_perl_release and ready,
        "public_lsp_perl_package_release": lsp_perl_release and ready,
        "public_perllsp_release_asset": perllsp_asset and ready,
        "host": {
            "product": str(host.get("product") or ""),
            "version": str(host.get("version") or ""),
            "released_build": str(host.get("released_build") or ""),
        },
        "packages": {
            "lite_xl_lsp": lsp_pkg,
            "language_perl": lang_pkg,
            "lsp_perl": managed_pkg,
        },
        "perllsp": perllsp,
    }


def exact_source_receipt_current(receipts_dir: Path) -> bool:
    """Whether a committed exact-source Lite XL receipt currently records a pass.

    Only a genuine #9008-family observation counts: the file must carry the
    exact-source receipt schema and evidence stage plus a `pass` result. An
    unrelated editor receipt or malformed pass-shaped file caught by the same
    glob never satisfies the gate.
    """
    if not receipts_dir.is_dir():
        return False
    for path in sorted(receipts_dir.glob("exact-source*.json")):
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if (
            isinstance(value, dict)
            and value.get("schema_version") == EXACT_SOURCE_RECEIPT_SCHEMA
            and value.get("evidence_stage") == EXACT_SOURCE_STAGE
            and value.get("result") == "pass"
        ):
            return True
    return False


def _object(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ReceiptError(f"{context} must be an object")
    return value


def _nonempty(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ReceiptError(f"{context} must be a non-empty string")
    return value


def validate_public_replay_receipt(
    receipt: dict[str, Any],
    ledger_path: Path,
    inventory: dict[str, Any],
    manifest_path: Path,
    manifest: dict[str, Any],
    receipts_dir: Path,
) -> None:
    """Validate one public-artifact replay receipt.

    Every retained receipt (passing or blocked) is bound to the exact landed
    ledger bytes, so a #11178 revision invalidates it offline. Passing receipts
    are additionally bound to an accepted upstream subject, clean
    public-install profile, an admitted install route, discriminator truth,
    and the complete ledger journey. Blocked receipts are valid only while the
    external subjects they name are actually absent.
    """
    if receipt.get("schema_version") != PUBLIC_RECEIPT_SCHEMA:
        raise ReceiptError("unexpected lite-xl public replay receipt schema")
    if receipt.get("stage") != PUBLIC_STAGE:
        raise ReceiptError(
            "lite-xl public replay receipt must name the released_public_artifact stage; "
            f"{EXACT_SOURCE_STAGE!r} and {STAGED_MANAGED_STAGE!r} receipts cannot satisfy "
            "this stage"
        )
    if receipt.get("evidence_stage") is not None:
        raise ReceiptError(
            "public replay receipt must not carry an exact-source evidence_stage field"
        )
    result = receipt.get("result")
    if result not in PUBLIC_RESULTS:
        raise ReceiptError(f"unknown public replay receipt result {result!r}")
    parse_timestamp(receipt.get("observed_at"), "observed_at")
    parse_digest(receipt.get("ledger_sha256"), "ledger_sha256")
    if receipt["ledger_sha256"] != sha256_file(ledger_path):
        raise ReceiptError(
            "ledger_sha256 does not match the landed #11178 ledger bytes; "
            "the binding is stale and the receipt must be regenerated"
        )

    recorded_ledger = str(receipt.get("ledger_relative_path") or "")
    provided = ledger_path.as_posix()
    if recorded_ledger != LEDGER_RELATIVE_PATH and recorded_ledger != provided:
        raise ReceiptError("ledger_relative_path must bind the canonical spec ledger")

    journey = _object(receipt.get("journey"), "journey")
    expected_cells = inventory["baseline_set"] | inventory["optional_set"]
    for cell_id in sorted(expected_cells):
        entry = journey.get(cell_id)
        if entry is None:
            raise ReceiptError(f"journey cell {cell_id!r} from the landed ledger is missing")
        entry_object = _object(entry, f"journey.{cell_id}")
        cell_result = entry_object.get("result")
        if cell_result not in CELL_RESULTS:
            raise ReceiptError(f"journey cell {cell_id!r} has an invalid result")
        if result != "pass" and cell_result == "pass":
            raise ReceiptError(
                f"non-passing receipt cannot claim a proven journey cell {cell_id!r}"
            )
    unexpected = sorted(set(journey.keys()) - expected_cells)
    if unexpected:
        raise ReceiptError(
            f"journey cell {unexpected[0]!r} is not in the landed #11178 ledger"
        )

    if result == "pass":
        _validate_pass(receipt, inventory, manifest, receipts_dir)
        return
    # Every non-passing result keeps the same live gate binding, so a
    # relabeled result cannot escape the stale-subject check after the
    # external subjects land.
    _validate_blocked_gates(receipt, manifest, receipts_dir)


def _validate_blocked_gates(
    receipt: dict[str, Any], manifest: dict[str, Any], receipts_dir: Path
) -> None:
    gates = _object(receipt.get("gates"), "gates")
    subject = upstream_subject(manifest)
    for cell in SUBJECT_GATE_CELLS:
        state = gates.get(cell)
        if state not in GATE_STATES:
            raise ReceiptError(f"gate {cell!r} has an invalid state")
        if state == "current" and not subject[cell]:
            raise ReceiptError(
                f"gate {cell!r} cannot claim a subject the acceptance manifest does not record"
            )
        if state in ("absent", "stale") and subject[cell]:
            raise ReceiptError(
                f"gate {cell!r} cannot deny it — the acceptance manifest currently records "
                "a merged-and-released subject"
            )

    exact_gate = gates.get(EXACT_SOURCE_GATE_CELL)
    if exact_gate not in GATE_STATES:
        raise ReceiptError(f"gate {EXACT_SOURCE_GATE_CELL!r} has an invalid state")
    current_exact = exact_source_receipt_current(receipts_dir)
    if exact_gate == "current" and not current_exact:
        raise ReceiptError(
            f"gate {EXACT_SOURCE_GATE_CELL!r} claims a pass but no committed fixture "
            "records one"
        )
    if exact_gate in ("absent", "stale") and current_exact:
        raise ReceiptError(
            f"gate {EXACT_SOURCE_GATE_CELL!r} cannot deny it — a committed fixture "
            "records a current exact-source pass"
        )

    blockers = gates.get("blockers")
    if not isinstance(blockers, list) or not all(isinstance(item, str) for item in blockers):
        raise ReceiptError("gates.blockers must be a list of strings")
    absent_subjects = [cell for cell in SUBJECT_GATE_CELLS if not subject[cell]]
    exact_absent = gates.get(EXACT_SOURCE_GATE_CELL) != "current"
    if (absent_subjects or exact_absent) and not blockers:
        raise ReceiptError("a blocked receipt must name its absent external subjects")


def _validate_pass(
    receipt: dict[str, Any],
    inventory: dict[str, Any],
    manifest: dict[str, Any],
    receipts_dir: Path,
) -> None:
    subject = upstream_subject(manifest)
    missing_subjects = [cell for cell in SUBJECT_GATE_CELLS if not subject[cell]]
    if missing_subjects:
        raise ReceiptError(
            "a public replay pass requires an accepted merged-and-released upstream "
            f"subject recorded by the acceptance manifest ({missing_subjects[0]!r} is absent)"
        )

    gates = _object(receipt.get("gates"), "gates")
    for cell in GATE_CELLS:
        state = gates.get(cell)
        if state not in GATE_STATES:
            raise ReceiptError(f"gate {cell!r} has an invalid state")
    for cell in GATE_CELLS:
        if cell == EXACT_SOURCE_GATE_CELL:
            # A public pass cannot outrun an absent or stale entry gate: the
            # #9008-family exact-source prerequisite must itself be current.
            if gates[cell] != "current":
                raise ReceiptError(f"gate {cell!r} cannot outrun an absent or stale entry gate")
            if not exact_source_receipt_current(receipts_dir):
                raise ReceiptError(
                    f"gate {EXACT_SOURCE_GATE_CELL!r} claims a pass but no committed "
                    "fixture records one"
                )
            continue
        if gates[cell] != "current":
            raise ReceiptError(
                f"a passing receipt cannot record gate {cell!r} as anything but current"
            )

    host = _object(receipt.get("host"), "host")
    if (
        host.get("product") != subject["host"]["product"]
        or host.get("version") != subject["host"]["version"]
        or host.get("build") != subject["host"]["released_build"]
    ):
        raise ReceiptError("host identity disagrees with the accepted upstream subject")

    platform = _object(receipt.get("platform"), "platform")
    _nonempty(platform.get("os"), "platform.os")
    _nonempty(platform.get("architecture"), "platform.architecture")

    packages = _object(receipt.get("packages"), "packages")
    for key, cell in (
        ("lite_xl_lsp", "public_lite_xl_lsp_package_release"),
        ("language_perl", "public_language_perl_package_release"),
        ("lsp_perl", "public_lsp_perl_package_release"),
    ):
        bound = _object(packages.get(key), f"packages.{key}")
        manifest_pkg = subject["packages"][key]
        if bound.get("version") != str(manifest_pkg.get("version") or ""):
            raise ReceiptError(f"packages.{key}.version disagrees with the accepted subject")
        parse_digest(bound.get("sha256"), f"packages.{key}.sha256")
        if bound["sha256"] != str(manifest_pkg.get("sha256") or ""):
            raise ReceiptError(f"packages.{key}.sha256 must equal the accepted subject identity")

    server = _object(receipt.get("server"), "server")
    route = server.get("install_route")
    if route in FORBIDDEN_INSTALL_ROUTES:
        raise ReceiptError(
            f"developer shortcut install_route={route!r}; the public replay admits only "
            f"{sorted(INSTALL_ROUTES)}"
        )
    if route not in INSTALL_ROUTES:
        raise ReceiptError(f"unknown install_route {route!r}")
    binary_identity = subject["perllsp"]
    parse_digest(server.get("binary_sha256"), "server.binary_sha256")
    if server["binary_sha256"] != str(binary_identity.get("member_sha256") or ""):
        raise ReceiptError(
            "server.binary_sha256 must equal the selected public asset member identity"
        )
    process_path = _nonempty(server.get("process_path"), "server.process_path")
    installed_path = _nonempty(server.get("installed_path"), "server.installed_path")
    normalized = process_path.replace("\\", "/")
    relative = installed_path.replace("\\", "/").lstrip("/")
    # The comparison must respect path-component boundaries: a decoy root
    # whose final component merely ends with installed_path's first component
    # is exactly the wrong-root substitution this receipt rejects.
    if normalized != relative and not normalized.endswith("/" + relative):
        raise ReceiptError(
            "server.process_path is not the managed public artifact resolved by the host"
        )
    argv = server.get("process_argv")
    if not isinstance(argv, list) or [str(item) for item in argv[:2]] != ["perllsp", "--stdio"]:
        raise ReceiptError("server.process_argv must launch the exact perllsp product")
    version_output = _nonempty(server.get("version_output"), "server.version_output")
    expected_version = str(binary_identity.get("release_version") or "")
    if not version_output.startswith("perllsp ") or expected_version not in version_output:
        raise ReceiptError(
            "server.version_output must name the exact canonical perllsp product and version"
        )

    workspace = _object(receipt.get("workspace"), "workspace")
    parse_digest(workspace.get("fixture_sha256"), "workspace.fixture_sha256")
    configuration = _object(receipt.get("configuration"), "configuration")
    for cell in ("config_sha256", "driver_sha256", "instrument_sha256"):
        parse_digest(configuration.get(cell), f"configuration.{cell}")

    profile = _object(receipt.get("profile"), "profile")
    for cell in PROFILE_CELLS:
        if profile.get(cell) is not True:
            raise ReceiptError(f"clean public-install profile precondition failed: {cell}")

    discriminators = _object(receipt.get("discriminators"), "discriminators")
    for cell in DISCRIMINATOR_CELLS:
        if discriminators.get(cell) is not True:
            raise ReceiptError(f"discriminator {cell!r} must hold on a public replay")

    cleanup = _object(receipt.get("cleanup"), "cleanup")
    for cell in ("adapter_orphans", "debuggee_orphans"):
        survivors = cleanup.get(cell)
        if not isinstance(survivors, list) or survivors:
            raise ReceiptError(f"cleanup.{cell} must be empty on shutdown")

    journey = _object(receipt.get("journey"), "journey")
    for cell_id in inventory["baseline_set"]:
        entry = _object(journey[cell_id], f"journey.{cell_id}")
        if entry.get("result") != "pass" or not _nonempty(entry.get("evidence"), "evidence"):
            raise ReceiptError(
                f"baseline journey cell {cell_id!r} is not proven by this receipt"
            )
    for cell_id in inventory["optional_set"]:
        entry = _object(journey[cell_id], f"journey.{cell_id}")
        if entry.get("result") == "pass" and _nonempty(entry.get("evidence"), "evidence"):
            continue
        limitation = entry.get("limitation")
        if entry.get("result") != "unsupported" or not isinstance(limitation, str) or not limitation:
            raise ReceiptError(
                f"optional journey cell {cell_id!r} must either pass or record an explicit "
                "unsupported limitation"
            )

    limitations = receipt.get("limitations")
    if not isinstance(limitations, list) or not limitations:
        raise ReceiptError("a public replay receipt must record its limitations")
    currentness = _object(receipt.get("currentness"), "currentness")
    invalidators = currentness.get("invalidators")
    if not isinstance(invalidators, list) or not invalidators:
        raise ReceiptError("a public replay receipt must record its currentness invalidators")
    claim_boundary = _object(receipt.get("claim_boundary"), "claim_boundary")
    if claim_boundary.get("lsp_support_rows") != "unchanged":
        raise ReceiptError("claim_boundary.lsp_support_rows must remain unchanged at this stage")
