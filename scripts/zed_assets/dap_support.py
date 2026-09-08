"""Fail-closed projection of the Zed perl-dap debugger support surface.

This is the D06 (#9489) consumer slice of the Zed DAP journey chain. It
consumes the D05 (#9487) official-registry journey validator exactly as it
landed: the committed public receipt is first validated through
`validate_dap_public_receipt` against the checked #9516 managed-download
contract, the committed aggregate asset receipt, and the DU01 acceptance
manifest, so any gate staleness fails the projection closed before a single
support cell is emitted.

The projection never re-derives public asset selection, digests, or the
recorded Windows member divergence, and it never creates a second inventory:
every public cell, gate, blocker, and limitation is quoted from the committed
receipt, and the exact-source stage boolean reuses the validator's own
`exact_source_receipt_current` predicate. Zed LSP support stays a separate
surface owned by `policy/lsp-client-support.toml`; this projection records
that row as unchanged and never writes it.
"""

from __future__ import annotations

import json
import tomllib
from pathlib import Path
from typing import Any

from .common import ReceiptError, load_json, sha256_file
from .dap_contract import DAP_ADAPTER_ID
from .dap_public import load_registry_manifest, validate_dap_public_receipt

SUPPORT_SCHEMA = "zed-dap-support.v1"
PROJECTION_ISSUE = 9489

# The D02 (#9486) DAP receipt must bind the perl-dap adapter identity under
# one of these keys. The shared `zed_host_compat.v1` + `exact_source_dev_extension`
# family is also used by Zed LSP exact-source receipts, so schema+stage alone
# cannot distinguish an LSP journey from a debugger journey; only a receipt
# naming the exact adapter identity may promote the DAP exact-source stage.
DAP_IDENTITY_KEYS = ("debug_adapter", "adapter", "dap")

PUBLIC_RECEIPT_RELATIVE_PATH = ".ci/fixtures/zed-perl-upstream/receipts/dap-public-registry.v1.json"
EXTENSION_MANIFEST_RELATIVE_PATH = ".ci/fixtures/zed-perl-upstream/zed-perl/extension.toml"
ADAPTER_SCHEMA_RELATIVE_PATH = (
    ".ci/fixtures/zed-perl-upstream/zed-perl/debug_adapter_schemas/perl-dap.json"
)
LSP_SUPPORT_POLICY_RELATIVE_PATH = "policy/lsp-client-support.toml"
POLICY_OUTPUT_RELATIVE_PATH = "policy/zed-dap-support.toml"
DOCS_OUTPUT_RELATIVE_PATH = "docs/EDITORS/ZED_DAP_SUPPORT.md"

REGENERATE_COMMAND = (
    "python scripts/zed_dap_asset_receipts.py project-dap-support "
    f"--policy-output {POLICY_OUTPUT_RELATIVE_PATH} "
    f"--docs-output {DOCS_OUTPUT_RELATIVE_PATH}"
)

# Zed LSP server identities stay outside the debugger-adapter identity.
LANGUAGE_SERVER_IDS = ("perlnavigator-server", "perl-lsp", "perllsp")

STAGE_STATIC = "configuration_static_adapter_authority"
STAGE_EXACT_SOURCE = "exact_source_dev_extension"
STAGE_PUBLIC = "public_registry_install"
STAGES = (STAGE_STATIC, STAGE_EXACT_SOURCE, STAGE_PUBLIC)

# Every D05 journey cell is consumed by exactly one D06 projection cell, so
# the projection can neither drop earned evidence nor invent a cell no
# receipt observes.
PROJECTION_CELLS: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("host_and_adapter_selection", ("zed_host_identity", "fixture_open", "adapter_selection")),
    ("managed_resolution", ("managed_resolution", "adapter_process_identity")),
    ("session_initialize_launch", ("initialize_launch",)),
    ("breakpoint_stop", ("breakpoint_verified", "stopped_event")),
    ("stack_source_identity", ("frame_source_identity",)),
    ("scopes_variables", ("scopes_variables",)),
    ("continue_step", ("continue_step",)),
    ("termination_cleanup", ("terminate_disconnect", "cleanup_bounded")),
    ("restart_reuse", ("restart_reuse",)),
)

SESSION_KIND_BY_CELL = {"session_initialize_launch": "launch"}

NOT_PROVEN = "not_proven"


def _string_list(value: Any, context: str) -> list[str]:
    if not isinstance(value, list) or not all(
        isinstance(item, str) and item.strip() for item in value
    ):
        raise ReceiptError(f"{context} must be a list of non-empty strings")
    return [str(item) for item in value]


def load_extension_manifest(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        manifest = tomllib.load(handle)
    if not isinstance(manifest, dict):
        raise ReceiptError(f"{path} must contain a TOML table")
    return manifest


def exact_source_dap_receipt_present(receipts_dir: Path) -> bool:
    """Whether a committed exact-source receipt binds a genuine DAP journey.

    Unlike the D05 entry-gate predicate (which counts any passing
    `zed_host_compat.v1` exact-source receipt), the projected stage must not
    promote from an LSP journey receipt that merely shares the schema family:
    the receipt must additionally record the exact `perl-dap` adapter
    identity under a debug-adapter block.
    """
    if not receipts_dir.is_dir():
        return False
    for path in sorted(receipts_dir.glob("exact-source*.json")):
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if not isinstance(value, dict):
            continue
        if (
            value.get("schema_version") != "zed_host_compat.v1"
            or value.get("evidence_stage") != "exact_source_dev_extension"
            or value.get("result") != "pass"
        ):
            continue
        for key in DAP_IDENTITY_KEYS:
            block = value.get(key)
            if isinstance(block, dict) and block.get("adapter_id") == DAP_ADAPTER_ID:
                return True
    return False


def static_adapter_authority(
    extension_manifest: dict[str, Any],
    adapter_schema: dict[str, Any],
    adapter_schema_path: Path,
) -> dict[str, Any]:
    """Evaluate the static adapter authority stage over the staged extension.

    Static authority documents configuration only: it can never promote an
    actual debugger behavior cell, and the adapter identity may never alias a
    Zed language-server identity.
    """
    debug_adapters = extension_manifest.get("debug_adapters")
    if not isinstance(debug_adapters, dict) or not debug_adapters:
        raise ReceiptError("the staged extension declares no debug adapter")
    if set(debug_adapters) != {DAP_ADAPTER_ID}:
        raise ReceiptError(
            "the staged extension must declare exactly the "
            f"{DAP_ADAPTER_ID!r} debug adapter; an aliased or additional "
            "adapter identity cannot anchor the static stage"
        )
    language_servers = extension_manifest.get("language_servers")
    if not isinstance(language_servers, dict) or not language_servers:
        raise ReceiptError("the staged extension declares no language servers")
    aliased = sorted(set(language_servers) & set(debug_adapters))
    if aliased:
        raise ReceiptError(
            "debug adapter identity aliases language-server identity "
            f"{aliased[0]!r}; the debugger surface may not reuse a "
            "language-server ID"
        )
    if DAP_ADAPTER_ID in LANGUAGE_SERVER_IDS:
        raise ReceiptError("the adapter identity must stay outside the LSP identity family")

    entry = debug_adapters[DAP_ADAPTER_ID]
    schema_path = str(entry.get("schema_path") or "")
    if not schema_path:
        raise ReceiptError("the staged debug adapter must bind its configuration schema")
    # The validated schema must be the file the manifest names, so a retargeted
    # or mistyped schema_path cannot hide a different file behind the static
    # authority claim.
    normalized_schema = adapter_schema_path.as_posix()
    if not normalized_schema.endswith(schema_path.replace("\\", "/")):
        raise ReceiptError(
            "the staged manifest names a different adapter schema "
            f"({schema_path!r}) than the one validated "
            f"({normalized_schema!r}); the static authority must describe the "
            "file Zed will actually load"
        )

    properties = adapter_schema.get("properties")
    request = properties.get("request") if isinstance(properties, dict) else None
    session_kinds = request.get("enum") if isinstance(request, dict) else None
    required = adapter_schema.get("required")
    if _string_list(session_kinds, "adapter schema request enum") != ["launch"]:
        raise ReceiptError(
            "the staged adapter schema must support exactly the launch session "
            "kind; an unobserved attach claim cannot enter the projection"
        )
    if _string_list(required, "adapter schema required keys") != ["request", "program"]:
        raise ReceiptError(
            "the staged adapter schema must require exactly request and program "
            "for the documented launch configuration shape"
        )
    return {
        "adapter_id": DAP_ADAPTER_ID,
        "executable": DAP_ADAPTER_ID,
        "schema_path": schema_path,
        "session_kinds_supported": ["launch"],
        "separate_language_server_ids": list(LANGUAGE_SERVER_IDS),
        "launch_required_keys": ["request", "program"],
    }


def _public_cell_results(receipt: dict[str, Any]) -> dict[str, str]:
    journey = receipt.get("journey")
    if not isinstance(journey, dict):
        raise ReceiptError("the public receipt lacks its journey table")
    results: dict[str, str] = {}
    for cell, members in PROJECTION_CELLS:
        observed: list[str] = []
        for member in members:
            entry = journey.get(member)
            if not isinstance(entry, dict) or entry.get("result") is None:
                raise ReceiptError(
                    f"the public receipt lacks journey cell {member!r}; the "
                    "projection cannot quote a cell the receipt does not record"
                )
            observed.append(str(entry["result"]))
        # A projection cell is proven only when every member receipt cell is
        # proven; anything less stays not_proven at the public stage.
        results[cell] = "pass" if all(item == "pass" for item in observed) else NOT_PROVEN
    return results


def project_dap_support(
    receipt_path: Path,
    contract_path: Path,
    asset_receipt_path: Path,
    manifest_path: Path,
    receipts_dir: Path,
    extension_manifest_path: Path,
    adapter_schema_path: Path,
) -> dict[str, Any]:
    """Build the D06 support model from the landed D05 authority surfaces.

    The D05 validator runs first and fail-closed: a stale or lying receipt,
    a drifted #9516 binding, an accepted registry subject, or a committed
    exact-source pass that the receipt denies all abort the projection with
    the validator's typed error before any cell is emitted.
    """
    receipt = load_json(receipt_path)
    contract = load_json(contract_path)
    asset_receipt = load_json(asset_receipt_path)
    manifest = load_registry_manifest(manifest_path)
    validate_dap_public_receipt(
        receipt,
        contract_path,
        contract,
        asset_receipt_path,
        asset_receipt,
        manifest_path,
        manifest,
        receipts_dir=receipts_dir,
    )

    extension_manifest = load_extension_manifest(extension_manifest_path)
    adapter_schema = load_json(adapter_schema_path)
    authority = static_adapter_authority(extension_manifest, adapter_schema, adapter_schema_path)

    exact_source_current = exact_source_dap_receipt_present(receipts_dir)
    public_result = str(receipt.get("result"))
    public_pass = public_result == "pass"
    public_cells = _public_cell_results(receipt)

    gates = receipt.get("gates")
    if not isinstance(gates, dict):
        raise ReceiptError("the public receipt lacks its gates table")
    blockers = _string_list(gates.get("blockers"), "gates.blockers") if public_result == "blocked_external" else []
    limitations = _string_list(receipt.get("limitations"), "limitations")
    invalidators = _string_list(
        receipt.get("currentness", {}).get("invalidators")
        if isinstance(receipt.get("currentness"), dict)
        else None,
        "currentness.invalidators",
    )

    selected = receipt.get("asset_evidence", {}).get("selected_target")
    if not isinstance(selected, dict):
        raise ReceiptError("the public receipt lacks its selected asset target")

    stage_states = {
        STAGE_STATIC: "registered_static_authority",
        STAGE_EXACT_SOURCE: "pass_receipt_present" if exact_source_current else NOT_PROVEN,
        STAGE_PUBLIC: public_result,
    }

    cells = []
    for cell, members in PROJECTION_CELLS:
        cells.append(
            {
                "id": cell,
                "session_kind": SESSION_KIND_BY_CELL.get(cell, ""),
                "receipts_consumed": list(members),
                "static_authority": "configuration_only",
                STAGE_EXACT_SOURCE: NOT_PROVEN,
                STAGE_PUBLIC: public_cells[cell],
            }
        )

    return {
        "schema": SUPPORT_SCHEMA,
        "projection_issue": PROJECTION_ISSUE,
        "product": "Zed",
        "integration_mode": "native_editor_extension_debugger_adapter_surface",
        "source_receipt": receipt_path.as_posix(),
        "source_receipt_sha256": sha256_file(receipt_path),
        "source_receipt_result": public_result,
        "source_receipt_observed_at": str(receipt.get("observed_at") or ""),
        "adapter": authority,
        "binary_routes": [
            {
                "id": "managed_download",
                "state": public_cells["managed_resolution"],
                "boundary": "requires the official-registry public journey (#9487) to pass",
            },
            {
                "id": "path",
                "state": NOT_PROVEN,
                "boundary": "distinct route; managed-public evidence can never promote it",
            },
        ],
        "lsp_surface": {
            "policy": LSP_SUPPORT_POLICY_RELATIVE_PATH,
            "row_id": "zed",
            "support_rows": "unchanged_by_this_projection",
        },
        "stages": [{"id": stage, "state": stage_states[stage]} for stage in STAGES],
        "static_stage_source": EXTENSION_MANIFEST_RELATIVE_PATH,
        "exact_source_stage_source": receipts_dir.as_posix(),
        "public_stage_source": receipt_path.as_posix(),
        "cells": cells,
        "gates": {str(cell): str(gates.get(cell)) for cell in sorted(gates) if cell != "blockers"},
        "blockers": blockers,
        "platform": {
            "os": str(selected.get("os")),
            "architecture": str(selected.get("architecture")),
            "target": str(selected.get("target")),
            "asset_binding": "quoted-from-receipt",
            "public_journey": public_cells["host_and_adapter_selection"],
        },
        "cross_platform_promotion": "denied",
        "claim_boundary": {
            # The tier follows the validated receipt, never a hard-coded
            # verdict, so a future public pass projects a consistent surface.
            "support_tier": "public_registry_proven" if public_pass else NOT_PROVEN,
            "public_support_requires_issue": 9487,
            "exact_source_alone_cannot_promote": True,
            "all_platforms": NOT_PROVEN,
            "lsp_support_rows": "unchanged",
        },
        "invalidators": invalidators,
        "limitations": limitations
        + (
            []
            if exact_source_current
            else [
                "The exact-source stage promotes individual cells only from a "
                "genuine committed #9486 receipt binding the perl-dap adapter "
                "identity; no such receipt is committed, so every exact-source "
                "cell stays not_proven.",
            ]
        )
        + [
            "The exact-source stage boolean counts only receipts that bind the "
            "exact perl-dap adapter identity; a Zed LSP exact-source receipt "
            "sharing the zed_host_compat.v1 family never promotes the DAP stage.",
        ],
    }


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def render_support_toml(model: dict[str, Any]) -> str:
    """Render the support registry deterministically (pure function)."""
    lines: list[str] = []
    lines.append("# Generated by `python scripts/zed_dap_asset_receipts.py project-dap-support`.")
    lines.append("# Do not edit: regenerate and commit. Owner: #9489 (Zed DAP support projection).")
    lines.append("# Every public cell is quoted from the committed #9487 journey receipt;")
    lines.append("# the D05 validator runs before this file is rendered, so gate staleness")
    lines.append("# fails the projection closed instead of drifting this surface.")
    lines.append("")
    lines.append(f'schema = {_toml_string(model["schema"])}')
    lines.append(f"projection_issue = {model['projection_issue']}")
    lines.append(f'product = {_toml_string(model["product"])}')
    lines.append(f'integration_mode = {_toml_string(model["integration_mode"])}')
    lines.append(f'source_receipt = {_toml_string(model["source_receipt"])}')
    lines.append(f'source_receipt_sha256 = {_toml_string(model["source_receipt_sha256"])}')
    lines.append(f'source_receipt_result = {_toml_string(model["source_receipt_result"])}')
    lines.append(f'source_receipt_observed_at = {_toml_string(model["source_receipt_observed_at"])}')
    lines.append("")
    adapter = model["adapter"]
    lines.append("[adapter]")
    lines.append(f'adapter_id = {_toml_string(adapter["adapter_id"])}')
    lines.append(f'executable = {_toml_string(adapter["executable"])}')
    lines.append(f'dap_config_schema = {_toml_string(adapter["schema_path"])}')
    lines.append(
        "session_kinds_supported = ["
        + ", ".join(_toml_string(item) for item in adapter["session_kinds_supported"])
        + "]"
    )
    lines.append(
        "separate_language_server_ids = ["
        + ", ".join(_toml_string(item) for item in adapter["separate_language_server_ids"])
        + "]"
    )
    lines.append(
        "launch_required_keys = ["
        + ", ".join(_toml_string(item) for item in adapter["launch_required_keys"])
        + "]"
    )
    lines.append("")
    for route in model["binary_routes"]:
        lines.append("[[binary_route]]")
        lines.append(f'id = {_toml_string(route["id"])}')
        lines.append(f'state = {_toml_string(route["state"])}')
        lines.append(f'boundary = {_toml_string(route["boundary"])}')
        lines.append("")
    lsp = model["lsp_surface"]
    lines.append("[lsp_surface]")
    lines.append(f'policy = {_toml_string(lsp["policy"])}')
    lines.append(f'row_id = {_toml_string(lsp["row_id"])}')
    lines.append(f'support_rows = {_toml_string(lsp["support_rows"])}')
    lines.append("")
    for stage in model["stages"]:
        lines.append("[[stage]]")
        lines.append(f'id = {_toml_string(stage["id"])}')
        lines.append(f'state = {_toml_string(stage["state"])}')
        source = (
            model["static_stage_source"]
            if stage["id"] == STAGE_STATIC
            else model["exact_source_stage_source"]
            if stage["id"] == STAGE_EXACT_SOURCE
            else model["public_stage_source"]
        )
        lines.append(f'source = {_toml_string(source)}')
        lines.append("")
    for cell in model["cells"]:
        lines.append("[[cell]]")
        lines.append(f'id = {_toml_string(cell["id"])}')
        if cell["session_kind"]:
            lines.append(f'session_kind = {_toml_string(cell["session_kind"])}')
        lines.append(
            "receipts_consumed = ["
            + ", ".join(_toml_string(item) for item in cell["receipts_consumed"])
            + "]"
        )
        lines.append(f'static_authority = {_toml_string(cell["static_authority"])}')
        lines.append(f'{STAGE_EXACT_SOURCE} = {_toml_string(cell[STAGE_EXACT_SOURCE])}')
        lines.append(f'{STAGE_PUBLIC} = {_toml_string(cell[STAGE_PUBLIC])}')
        lines.append("")
    platform = model["platform"]
    lines.append("[platform]")
    lines.append(f'os = {_toml_string(platform["os"])}')
    lines.append(f'architecture = {_toml_string(platform["architecture"])}')
    lines.append(f'target = {_toml_string(platform["target"])}')
    lines.append(f'asset_binding = {_toml_string(platform["asset_binding"])}')
    lines.append(f'public_journey = {_toml_string(platform["public_journey"])}')
    lines.append(f'cross_platform_promotion = {_toml_string(model["cross_platform_promotion"])}')
    lines.append("")
    lines.append("[platform.other_os_architecture_rows]")
    lines.append(f'state = {_toml_string("not_observed")}')
    lines.append(
        'boundary = "one OS/architecture cannot promote another; linux/macOS asset rows stay managed_extracted_not_executed per the receipt limitations"'
    )
    lines.append("")
    boundary = model["claim_boundary"]
    lines.append("[claim_boundary]")
    lines.append(f'support_tier = {_toml_string(boundary["support_tier"])}')
    lines.append(f"public_support_requires_issue = {boundary['public_support_requires_issue']}")
    lines.append(f"exact_source_alone_cannot_promote = {str(boundary['exact_source_alone_cannot_promote']).lower()}")
    lines.append(f'all_platforms = {_toml_string(boundary["all_platforms"])}')
    lines.append(f'lsp_support_rows = {_toml_string(boundary["lsp_support_rows"])}')
    lines.append("")
    lines.append("[currentness]")
    lines.append(
        "invalidators = [\n"
        + "".join(f"  {_toml_string(item)},\n" for item in model["invalidators"])
        + "]"
    )
    lines.append(
        "blockers = [\n" + "".join(f"  {_toml_string(item)},\n" for item in model["blockers"]) + "]"
    )
    lines.append(
        "limitations = [\n"
        + "".join(f"  {_toml_string(item)},\n" for item in model["limitations"])
        + "]"
    )
    lines.append("")
    lines.append("[gates]")
    for cell, state in model["gates"].items():
        lines.append(f"{cell} = {_toml_string(state)}")
    lines.append("")
    return "\n".join(lines).rstrip("\n") + "\n"


def render_support_markdown(model: dict[str, Any]) -> str:
    """Render the generated Zed debugger support documentation deterministically."""
    adapter = model["adapter"]
    cells = model["cells"]
    blockers = model["blockers"]
    limitations = model["limitations"]
    invalidators = model["invalidators"]
    public_pass = model["source_receipt_result"] == "pass"
    lines: list[str] = []
    lines.append("# Zed debugger support (perl-dap)")
    lines.append("")
    lines.append("<!-- Generated by `python scripts/zed_dap_asset_receipts.py project-dap-support`.")
    lines.append("     Do not edit: regenerate and commit. Owner: #9489. -->")
    lines.append("")
    if public_pass:
        lines.append("> **Status: public-registry journey proven at the recorded subjects.**")
        lines.append(">")
        lines.append("> Every proven cell binds exactly the receipt recorded above — the")
        lines.append("> observed platform, session kind, adapter identity, and public")
        lines.append("> release. Nothing on this page extends beyond that observation:")
        lines.append("> all other platforms, the PATH route, and every cell the receipt")
        lines.append("> does not record stay not proven. This debugger surface is")
        lines.append("> independent of the Zed LSP row: a DAP change never alters the LSP")
        lines.append("> verdict, and an LSP change never alters these cells.")
    else:
        lines.append("> **Status: planned / not proven.**")
        lines.append(">")
        lines.append("> No public-registry Zed debug session with `perl-dap` has been observed.")
        lines.append("> Installing the Perl extension does **not** by itself prove debugging, and")
        lines.append("> the presence of `perl-dap` in a release archive is not session proof.")
        lines.append("> This debugger surface is independent of the Zed LSP row: a DAP change")
        lines.append("> never alters the LSP verdict, and an LSP change never alters these cells.")
    lines.append("")
    lines.append(f"Source receipt: `{model['source_receipt']}`")
    lines.append("")
    lines.append(f"- result: `{model['source_receipt_result']}`")
    lines.append(f"- observed at: `{model['source_receipt_observed_at']}`")
    lines.append(f"- receipt digest: `{model['source_receipt_sha256']}`")
    lines.append("")
    lines.append("## Product identities")
    lines.append("")
    lines.append("| Surface | Identity |")
    lines.append("| --- | --- |")
    lines.append(f"| DAP adapter ID | `{adapter['adapter_id']}` |")
    lines.append(f"| DAP executable | `{adapter['executable']}` |")
    for server_id in adapter["separate_language_server_ids"]:
        lines.append(f"| Zed LSP server ID (separate) | `{server_id}` |")
    lines.append("")
    lines.append("The adapter ID never aliases a language-server ID, and no language-server")
    lines.append("executable, cache family, or receipt can satisfy the `perl-dap` route.")
    lines.append("")
    lines.append("## Evidence stages")
    lines.append("")
    lines.append("| Stage | State | Source |")
    lines.append("| --- | --- | --- |")
    for stage in model["stages"]:
        source = (
            model["static_stage_source"]
            if stage["id"] == STAGE_STATIC
            else model["exact_source_stage_source"]
            if stage["id"] == STAGE_EXACT_SOURCE
            else model["public_stage_source"]
        )
        lines.append(f"| `{stage['id']}` | `{stage['state']}` | `{source}` |")
    lines.append("")
    lines.append("Static authority cannot promote actual behavior. An exact-source observation")
    lines.append("can promote only exact-source cells. Only the public-registry journey")
    lines.append("(#9487) can promote public cells; #9486 exact-source evidence alone cannot")
    lines.append("create public debugger support.")
    lines.append("")
    lines.append("## Binary install routes")
    lines.append("")
    lines.append("| Route | State | Boundary |")
    lines.append("| --- | --- | --- |")
    for route in model["binary_routes"]:
        lines.append(f"| `{route['id']}` | `{route['state']}` | {route['boundary']} |")
    lines.append("")
    lines.append("Managed-download and PATH routes stay distinct: managed-public evidence")
    lines.append("can never promote the PATH row, and a PATH candidate can never satisfy the")
    lines.append("managed row.")
    lines.append("")
    lines.append("## Documented launch configuration shape")
    lines.append("")
    lines.append("The staged adapter schema supports exactly the `launch` session kind and")
    lines.append("requires `request` and `program`; `attach` is rejected with a typed error:")
    lines.append("")
    lines.append("```json")
    lines.append("{")
    lines.append('  "request": "launch",')
    lines.append('  "program": "/path/to/script.pl"')
    lines.append("}")
    lines.append("```")
    lines.append("")
    lines.append("## Debugger cells")
    lines.append("")
    lines.append("| Cell | Session kind | Static authority | Exact source | Public registry |")
    lines.append("| --- | --- | --- | --- | --- |")
    for cell in cells:
        kind = cell["session_kind"] or "n/a"
        lines.append(
            f"| `{cell['id']}` | {kind} | {cell['static_authority']} | "
            f"{cell[STAGE_EXACT_SOURCE]} | {cell[STAGE_PUBLIC]} |"
        )
    lines.append("")
    if public_pass:
        lines.append(
            "Public-registry cells are proven exactly as quoted from the receipt; the"
        )
        lines.append(
            "PATH route, unobserved platforms, and the exact-source column stay"
        )
        lines.append("unproven until their own matching receipts land.")
    else:
        lines.append("Cells promote individually from matching receipts only; no cell above is")
        lines.append("proven at any stage yet.")
    lines.append("")
    platform = model["platform"]
    lines.append("## Platforms")
    lines.append("")
    lines.append(f"- Observed asset-binding row: `{platform['os']}/{platform['architecture']}`")
    lines.append(f"  (`{platform['target']}`, {platform['asset_binding']}); public journey:")
    lines.append(f"  `{platform['public_journey']}`.")
    lines.append("- All other OS/architecture rows: `not_observed`. One platform cannot promote")
    lines.append("  another; the linux/macOS asset rows stay `managed_extracted_not_executed`")
    lines.append("  per the receipt limitations.")
    lines.append("")
    lines.append("## Current blockers")
    lines.append("")
    for blocker in blockers:
        lines.append(f"- {blocker}")
    if not blockers:
        lines.append("- none recorded")
    lines.append("")
    lines.append("## Invalidation")
    lines.append("")
    lines.append("Debugger cells invalidate when any bound DAP subject changes:")
    lines.append("")
    for invalidator in invalidators:
        lines.append(f"- {invalidator}")
    lines.append("")
    lines.append("Unrelated Zed LSP cells are not erased by a DAP subject change.")
    lines.append("")
    lines.append("## Limitations")
    lines.append("")
    for limitation in limitations:
        lines.append(f"- {limitation}")
    lines.append("")
    return "\n".join(lines).rstrip("\n") + "\n"


def check_or_write_projection(
    receipt_path: Path,
    contract_path: Path,
    asset_receipt_path: Path,
    manifest_path: Path,
    receipts_dir: Path,
    extension_manifest_path: Path,
    adapter_schema_path: Path,
    policy_output: Path,
    docs_output: Path,
    check: bool,
) -> None:
    """Validate, regenerate, and check-or-write the committed projection files."""
    model = project_dap_support(
        receipt_path,
        contract_path,
        asset_receipt_path,
        manifest_path,
        receipts_dir,
        extension_manifest_path,
        adapter_schema_path,
    )
    rendered_policy = render_support_toml(model)
    rendered_docs = render_support_markdown(model)
    if not check:
        policy_output.parent.mkdir(parents=True, exist_ok=True)
        docs_output.parent.mkdir(parents=True, exist_ok=True)
        # Write exact LF bytes so regeneration is byte-identical on every
        # platform; a host-newline write would surface as false drift under
        # `--check` on the other platform.
        policy_output.write_bytes(rendered_policy.encode("utf-8"))
        docs_output.write_bytes(rendered_docs.encode("utf-8"))
        return
    for path, rendered, label in (
        (policy_output, rendered_policy, "support registry"),
        (docs_output, rendered_docs, "support documentation"),
    ):
        # Compare exact bytes: the renderer and write path promise LF-only
        # output, so a host-newline or hand edit must surface as drift
        # instead of being normalized away.
        committed = path.read_bytes() if path.exists() else None
        if committed != rendered.encode("utf-8"):
            raise ReceiptError(
                f"the {label} {path.as_posix()} drifted from the projection of the "
                f"current receipts; regenerate it with `{REGENERATE_COMMAND}` "
                "and commit the result"
            )
