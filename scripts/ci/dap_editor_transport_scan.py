"""Source, docs, and consumer scans for the DAP editor-transport inventory."""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Mapping

from dap_editor_transport_schema import (
    TransportInventoryError,
    is_current_supported_client,
    is_test_only_path,
)

BIND_RE = re.compile(r"TcpListener::bind")
RUN_SOCKET_RE = re.compile(r"(?:pub(?:\([^)]+\))?\s+)?fn run_socket\b")
CFG_TEST_MOD_RE = re.compile(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{")
SOCKET_FLAG_RE = re.compile(r"pub socket:\s*bool")
PORT_FLAG_RE = re.compile(r"pub port:\s*Option<u16>")
TRANSPORT_FLATTEN_RE = re.compile(r"transport:\s*perl_lsp_rs_core::runtime::launcher::TransportArgs")
FIRST_MILE_SOCKET_RES = (
    re.compile(r"perl-dap\s+--socket"),
    re.compile(r"use socket mode", re.IGNORECASE),
    re.compile(r"run native dap over a tcp socket", re.IGNORECASE),
    re.compile(r"native tcp mode", re.IGNORECASE),
    re.compile(r"^#{1,6}\s+attach over tcp\s*$", re.IGNORECASE),
    re.compile(r"^#{1,6}\s+tcp attach\s*$", re.IGNORECASE),
)
RETIREMENT_LINE_RE = re.compile(
    r"retir|not a supported|not supported|do not use|must not be used|scheduled removal",
    re.IGNORECASE,
)
DEBUG_ADAPTER_SERVER_RE = re.compile(r"DebugAdapterServer")
PRODUCTION_SRC_ROOT = Path("crates/perl-dap/src")
TRANSPORT_ARGS_PATH = Path("crates/perl-lsp-rs-core/src/runtime/launcher/mod.rs")
PERL_DAP_MAIN = Path("crates/perl-dap/src/main.rs")
NATIVE_EDITOR_TRANSPORT = Path("crates/perl-dap/src/debug_adapter/transport.rs")
NATIVE_EDITOR_LIFECYCLE = Path("crates/perl-dap/src/server/lifecycle.rs")
FN_START_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:<[^>]*>)?\s*\(")
BIND_HELPER_CALL_RE = re.compile(r"\bbind_editor_listener\s*\(")
BIND_HELPER_FN_RE = re.compile(r"\bfn\s+bind_editor_listener\b")


def production_source(text: str) -> str:
    """Drop `#[cfg(test)] mod ... { ... }` blocks; keep the rest of the file.

    A leading `#[cfg(test)] use ...` must not hide later production binds
    (`peer_launch.rs` is the live witness).
    """
    out: list[str] = []
    cursor = 0
    for match in CFG_TEST_MOD_RE.finditer(text):
        out.append(text[cursor : match.start()])
        depth = 0
        index = match.end() - 1
        while index < len(text):
            char = text[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    index += 1
                    break
            index += 1
        cursor = index
    out.append(text[cursor:])
    return "".join(out)


def _brace_block(text: str, brace: int) -> str:
    depth = 0
    index = brace
    while index < len(text):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : index]
        index += 1
    return text[brace + 1 :]


def rust_function_bodies(text: str) -> dict[str, str]:
    """Map `fn name` → body text. Last definition of a name wins."""
    bodies: dict[str, str] = {}
    for match in FN_START_RE.finditer(text):
        name = match.group(1)
        index = match.end() - 1
        depth = 0
        brace: int | None = None
        while index < len(text):
            char = text[index]
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
            elif char == "{" and depth == 0:
                brace = index
                break
            elif char == ";" and depth == 0:
                break
            index += 1
        if brace is None:
            continue
        bodies[name] = _brace_block(text, brace)
    return bodies


def _read_text(root: Path, relative: str) -> str:
    path = root / relative
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise TransportInventoryError(f"missing path {relative}") from exc
    except OSError as exc:
        raise TransportInventoryError(f"cannot read {relative}: {exc}") from exc


def scan_bind_sites(root: Path, inventory: Mapping[str, Any]) -> list[str]:
    errors: list[str] = []
    src_root = root / PRODUCTION_SRC_ROOT
    if not src_root.is_dir():
        errors.append(f"missing production source root {PRODUCTION_SRC_ROOT}")
        return errors

    claimed: dict[str, list[dict[str, Any]]] = {}
    for site in inventory.get("bind_sites", []):
        if not isinstance(site, dict):
            continue
        path = site.get("path")
        if not isinstance(path, str):
            continue
        claimed.setdefault(path, []).append(site)
        site_path = root / path
        if not site_path.is_file():
            errors.append(f"bind_site {site.get('id')!r} path {path} does not exist")
            continue
        text = production_source(_read_text(root, path))
        if not BIND_RE.search(text):
            errors.append(
                f"bind_site {site.get('id')!r} claims {path} but production source has no TcpListener::bind"
            )

    for rust_file in sorted(src_root.rglob("*.rs")):
        relative = rust_file.relative_to(root).as_posix()
        text = production_source(rust_file.read_text(encoding="utf-8"))
        bind_count = len(BIND_RE.findall(text))
        if bind_count == 0:
            continue
        claimed_count = len(claimed.get(relative, []))
        if claimed_count == 0:
            errors.append(
                f"production TcpListener::bind in {relative} has no bind_site owner"
            )
        elif claimed_count != bind_count:
            errors.append(
                f"production TcpListener::bind count in {relative} is {bind_count} "
                f"but bind_sites claims {claimed_count}"
            )

    return errors


def scan_retired_native_editor_listener(root: Path, inventory: Mapping[str, Any]) -> list[str]:
    """Reject a returned native or external-peer editor TCP listener."""
    errors: list[str] = []
    for site in inventory.get("bind_sites", []):
        if not isinstance(site, dict):
            continue
        ident = site.get("id")
        if site.get("transport_id") == "native-editor-tcp" or ident == "native-editor-socket":
            errors.append(
                f"native editor TCP bind site {ident!r} returned after #10565 retirement"
            )
        if (
            site.get("transport_id") == "external-peer-editor-tcp"
            or ident == "external-peer-editor-listener"
        ):
            errors.append(
                f"external-peer editor TCP bind site {ident!r} returned after #10566 retirement"
            )

    for relative in (NATIVE_EDITOR_TRANSPORT, NATIVE_EDITOR_LIFECYCLE):
        try:
            text = production_source(_read_text(root, str(relative)))
        except TransportInventoryError as exc:
            errors.append(str(exc))
            continue
        if relative == NATIVE_EDITOR_TRANSPORT and BIND_RE.search(text):
            errors.append(
                f"{relative.as_posix()} production source regained a native editor TcpListener::bind"
            )
        if RUN_SOCKET_RE.search(text):
            errors.append(
                f"{relative.as_posix()} production source regained native editor run_socket admission"
            )

    try:
        main_text = production_source(_read_text(root, str(PERL_DAP_MAIN)))
    except TransportInventoryError as exc:
        errors.append(str(exc))
        return errors

    bodies = rust_function_bodies(main_text)
    main_body = bodies.get("main")
    if main_body is None:
        errors.append(
            f"{PERL_DAP_MAIN.as_posix()} production source lost fn main; native socket admission cannot be ratcheted"
        )
    else:
        if "native_editor_socket_retired" not in main_body:
            errors.append(
                f"{PERL_DAP_MAIN.as_posix()} fn main no longer fails native --socket via native_editor_socket_retired"
            )
        if BIND_HELPER_CALL_RE.search(main_body):
            errors.append(
                f"{PERL_DAP_MAIN.as_posix()} fn main regained a native editor bind_editor_listener call after #10565"
            )

    if BIND_HELPER_FN_RE.search(main_text):
        errors.append(
            f"{PERL_DAP_MAIN.as_posix()} fn bind_editor_listener returned after #10566"
        )
    if BIND_RE.search(main_text):
        errors.append(
            f"{PERL_DAP_MAIN.as_posix()} production source regained TcpListener::bind after #10566"
        )
    for name, body in bodies.items():
        if name == "bind_editor_listener":
            continue
        if BIND_HELPER_CALL_RE.search(body):
            errors.append(
                f"{PERL_DAP_MAIN.as_posix()} fn {name} calls bind_editor_listener after #10566"
            )
    return errors


def scan_cli_flags(root: Path, inventory: Mapping[str, Any]) -> list[str]:
    errors: list[str] = []
    try:
        launcher = _read_text(root, str(TRANSPORT_ARGS_PATH))
        main = _read_text(root, str(PERL_DAP_MAIN))
    except TransportInventoryError as exc:
        return [str(exc)]

    if not SOCKET_FLAG_RE.search(launcher) or not PORT_FLAG_RE.search(launcher):
        errors.append("TransportArgs no longer declares public --socket/--port; update the inventory")
    if not TRANSPORT_FLATTEN_RE.search(main):
        errors.append("perl-dap no longer flattens TransportArgs; public editor socket flags would be unowned")

    flags = {row.get("flag"): row for row in inventory.get("cli_flags", []) if isinstance(row, dict)}
    for flag in ("--socket", "--port"):
        row = flags.get(flag)
        if row is None:
            errors.append(f"public CLI flag {flag} is missing from the inventory")
            continue
        if row.get("applies_to") != "perl-dap":
            errors.append(f"CLI flag {flag} must be inventoried against perl-dap")
        if row.get("disposition") != "retire":
            errors.append(f"CLI flag {flag} must be classified retire, not supported")
    return errors


def scan_first_mile(root: Path, inventory: Mapping[str, Any]) -> list[str]:
    errors: list[str] = []
    for relative in inventory.get("first_mile_surfaces", []):
        if not isinstance(relative, str):
            continue
        try:
            text = _read_text(root, relative)
        except TransportInventoryError as exc:
            errors.append(str(exc))
            continue
        for line_no, line in enumerate(text.splitlines(), start=1):
            if RETIREMENT_LINE_RE.search(line):
                continue
            for pattern in FIRST_MILE_SOCKET_RES:
                if pattern.search(line):
                    errors.append(
                        f"{relative}:{line_no}: stale editor-socket product run mode: {line.strip()}"
                    )
    return errors


def scan_clients(root: Path, inventory: Mapping[str, Any]) -> list[str]:
    errors: list[str] = []
    for row in inventory.get("clients", []):
        if not isinstance(row, dict):
            continue
        ident = row.get("id")
        evidence_paths = row.get("evidence_paths") or []
        if not isinstance(evidence_paths, list):
            errors.append(f"client {ident!r} evidence_paths must be an array")
            continue

        if row.get("dap_claimed") is False:
            if evidence_paths:
                errors.append(f"client {ident!r} does not claim DAP but lists evidence_paths")
            continue

        if not evidence_paths:
            if row.get("evidence_stage") not in {"none", "not_proven"}:
                errors.append(f"client {ident!r} is missing evidence_paths")
            continue

        blobs: list[str] = []
        product_blobs: list[str] = []
        product_evidence = False
        for relative in evidence_paths:
            if not isinstance(relative, str):
                errors.append(f"client {ident!r} evidence path is not a string")
                continue
            try:
                text = _read_text(root, relative)
            except TransportInventoryError as exc:
                errors.append(f"client {ident!r}: {exc}")
                continue
            blobs.append(text)
            if not is_test_only_path(relative):
                product_evidence = True
                product_blobs.append(text)
        combined = "\n".join(blobs)
        # Shipped/package transport claims must be true of product evidence.
        # A listed test fixture cannot supply required markers or hide a stale
        # product descriptor.
        marker_haystack = (
            "\n".join(product_blobs)
            if row.get("evidence_stage") in {"shipped", "package"} and product_blobs
            else combined
        )

        if row.get("evidence_stage") in {"shipped", "package"} and not product_evidence:
            errors.append(
                f"client {ident!r} uses only test-only evidence to satisfy a product client row"
            )

        for marker in row.get("required_markers") or []:
            if not isinstance(marker, str) or marker not in marker_haystack:
                errors.append(
                    f"client {ident!r} declared transport disagrees with fixtures: missing {marker!r}"
                )
        for marker in row.get("forbidden_markers") or []:
            if isinstance(marker, str) and marker in marker_haystack:
                errors.append(
                    f"client {ident!r} declared transport disagrees with fixtures: forbidden {marker!r} present"
                )

        if row.get("transport") == "stdio" and DEBUG_ADAPTER_SERVER_RE.search(marker_haystack):
            errors.append(
                f"client {ident!r} is declared stdio but evidence launches DebugAdapterServer"
            )

        if is_current_supported_client(row) and row.get("editor_socket_required") is True:
            errors.append(
                f"STOP_TRAIN: current supported client {ident!r} requires editor TCP; "
                "amend #7486 with this exact client/receipt before removal"
            )
        if row.get("editor_socket_required") is True and row.get("blocks_retirement") is not True:
            if is_current_supported_client(row):
                errors.append(
                    f"client {ident!r} requires editor TCP but does not set blocks_retirement"
                )
        if row.get("blocks_retirement") and not is_current_supported_client(row):
            if row.get("support_status") == "unsupported":
                continue
            errors.append(
                f"client {ident!r} is not a current supported consumer and must not block retirement"
            )
    return errors


def scan_relays(root: Path, inventory: Mapping[str, Any]) -> list[str]:
    errors: list[str] = []
    src_root = root / PRODUCTION_SRC_ROOT
    if not src_root.is_dir():
        return errors

    claimed = {
        row.get("path")
        for row in inventory.get("dap_to_dap_relays", [])
        if isinstance(row, dict)
    }
    for rust_file in sorted(src_root.rglob("*.rs")):
        relative = rust_file.relative_to(root).as_posix()
        text = production_source(rust_file.read_text(encoding="utf-8"))
        if "VS Code <-> Native DAP Adapter <-> TCP Socket <-> Perl::LanguageServer DAP" in text and relative not in claimed:
            errors.append(f"DAP-to-DAP proxy {relative} is not inventoried")
    for row in inventory.get("dap_to_dap_relays", []):
        if not isinstance(row, dict):
            continue
        path = row.get("path")
        if not isinstance(path, str):
            continue
        try:
            text = production_source(_read_text(root, path))
        except TransportInventoryError as exc:
            errors.append(str(exc))
            continue
        if "TcpAttach" not in text and "Perl::LanguageServer DAP" not in text:
            errors.append(f"relay {row.get('id')!r} path {path} is not a DAP-to-DAP proxy witness")
        if row.get("authority") == "product":
            errors.append(f"relay {row.get('id')!r} must not be a product editor transport")
    return errors


def evaluate_ruling(inventory: Mapping[str, Any], scan_errors: list[str]) -> list[str]:
    errors: list[str] = []
    stop = [item for item in scan_errors if item.startswith("STOP_TRAIN:")]
    ruling = inventory.get("ruling_status")
    if stop:
        if ruling == "accepted":
            errors.append(
                "inventory ruling_status=accepted despite a current supported client that requires editor TCP"
            )
        return errors
    if ruling != "accepted":
        errors.append(
            f"no supported client requires editor TCP, so ruling_status must be accepted, got {ruling!r}"
        )
    if inventory.get("tcp_required_supported_client") not in {None, ""}:
        errors.append(
            "tcp_required_supported_client must be empty when no supported client requires editor TCP"
        )
    return errors
