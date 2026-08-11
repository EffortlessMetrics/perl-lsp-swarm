"""Shared types and canonical identities for the DAP capability matrix."""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any, Mapping

MATRIX_SCHEMA = "dap_capability_matrix.v2"
RECEIPT_SCHEMA = "dap_capability_matrix_receipt.v2"
MATRIX_PATH = Path(".ci/dap/capability-matrix.json")
PRODUCTION_SOURCE_PATH = Path("crates/perl-dap/src/debug_adapter/process.rs")
PRODUCTION_ANCHOR = "let capabilities = json!({"
UPSTREAM_DEFINITION = "Capabilities"
AUTHORITY_MANIFEST_PATH = Path(".ci/dap/protocol-authority.json")
DOC_PATH = Path("docs/reference/DAP_CAPABILITY_MATRIX.md")
WORKFLOW_PATH = Path(".github/workflows/dap-capability-matrix.yml")
RUST_CONTRACT_PATH = Path("crates/perl-dap/tests/dap_capability_matrix_contract.rs")
VALIDATOR_PATHS = (
    Path("scripts/ci/dap_capability_matrix.py"),
    Path("scripts/ci/dap_capability_common.py"),
    Path("scripts/ci/dap_capability_source.py"),
    Path("scripts/ci/dap_capability_upstream.py"),
    Path("scripts/ci/dap_capability_git.py"),
    Path("scripts/tests/test_dap_capability_matrix.py"),
)
RECEIPT_SUBJECT_PATHS = (
    MATRIX_PATH,
    PRODUCTION_SOURCE_PATH,
    AUTHORITY_MANIFEST_PATH,
    DOC_PATH,
    WORKFLOW_PATH,
    RUST_CONTRACT_PATH,
    *VALIDATOR_PATHS,
)
ALLOWED_CLASSIFICATIONS = {"standard", "extension"}
ALLOWED_BASES = {
    "catalog_derived_not_backend_derived",
    "fixed_false",
    "unversioned_extension_catalog_derived",
}
ALLOWED_WIRE_TYPES = {"boolean", "array"}
EXPRESSION = re.compile(r"^(?:false|[a-z][a-z0-9_]*)$")
OWNER = re.compile(r"^#[1-9][0-9]*$")
HEX40 = re.compile(r"^[0-9a-f]{40}$")


class MatrixError(RuntimeError):
    """A fail-closed capability inventory error."""


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise MatrixError(f"missing JSON input: {path}") from exc
    except json.JSONDecodeError as exc:
        raise MatrixError(f"malformed JSON in {path}: {exc}") from exc
    except OSError as exc:
        raise MatrixError(f"cannot read {path}: {exc}") from exc


def write_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def object_value(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise MatrixError(f"{context} must be a JSON object")
    return value


def array_value(value: Any, context: str) -> list[Any]:
    if not isinstance(value, list):
        raise MatrixError(f"{context} must be a JSON array")
    return value


def string_value(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise MatrixError(f"{context} must be a non-empty string")
    return value


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def validate_matrix(raw: Any) -> tuple[Mapping[str, Any], dict[str, Mapping[str, Any]]]:
    matrix = object_value(raw, "capability matrix")
    if matrix.get("schema_version") != MATRIX_SCHEMA:
        raise MatrixError(
            f"capability matrix schema must be {MATRIX_SCHEMA!r}, "
            f"got {matrix.get('schema_version')!r}"
        )

    source = object_value(matrix.get("source"), "matrix.source")
    expected_source = {
        "path": PRODUCTION_SOURCE_PATH.as_posix(),
        "anchor": PRODUCTION_ANCHOR,
        "upstream_definition": UPSTREAM_DEFINITION,
    }
    if dict(source) != expected_source:
        raise MatrixError(
            "matrix.source must equal the independent production authority: "
            f"expected {expected_source!r}, got {dict(source)!r}"
        )

    rows = array_value(matrix.get("rows"), "matrix.rows")
    if not rows:
        raise MatrixError("capability matrix must contain at least one row")

    indexed: dict[str, Mapping[str, Any]] = {}
    extension_count = 0
    for index, raw_row in enumerate(rows):
        row = object_value(raw_row, f"matrix.rows[{index}]")
        expected_keys = {
            "wire_name",
            "classification",
            "expression",
            "wire_type",
            "basis",
            "owner",
        }
        if set(row) != expected_keys:
            raise MatrixError(
                f"matrix row {index} keys must be exactly {sorted(expected_keys)!r}, "
                f"got {sorted(row)!r}"
            )
        name = string_value(row.get("wire_name"), f"matrix.rows[{index}].wire_name")
        if name in indexed:
            raise MatrixError(f"duplicate capability wire name: {name}")
        classification = string_value(
            row.get("classification"), f"matrix.rows[{index}].classification"
        )
        if classification not in ALLOWED_CLASSIFICATIONS:
            raise MatrixError(f"unsupported classification for {name}: {classification}")
        expression = string_value(row.get("expression"), f"matrix.rows[{index}].expression")
        if EXPRESSION.fullmatch(expression) is None:
            raise MatrixError(f"unsafe or non-canonical expression for {name}: {expression!r}")
        wire_type = string_value(row.get("wire_type"), f"matrix.rows[{index}].wire_type")
        if wire_type not in ALLOWED_WIRE_TYPES:
            raise MatrixError(f"unsupported wire type for {name}: {wire_type!r}")
        basis = string_value(row.get("basis"), f"matrix.rows[{index}].basis")
        if basis not in ALLOWED_BASES:
            raise MatrixError(f"unsupported advertisement basis for {name}: {basis}")
        owner = string_value(row.get("owner"), f"matrix.rows[{index}].owner")
        if OWNER.fullmatch(owner) is None:
            raise MatrixError(f"capability row {name} has invalid owner {owner!r}")

        if expression == "false":
            if basis != "fixed_false":
                raise MatrixError(f"literal-false capability {name} must use fixed_false basis")
            if wire_type != "boolean":
                raise MatrixError(f"literal-false capability {name} must have boolean wire type")
        elif basis == "fixed_false":
            raise MatrixError(f"non-false capability {name} cannot use fixed_false basis")

        if classification == "extension":
            extension_count += 1
            if basis != "unversioned_extension_catalog_derived":
                raise MatrixError(f"extension capability {name} lacks extension basis")
            if wire_type != "boolean":
                raise MatrixError(f"current capability extension {name} must be boolean")
        elif basis == "unversioned_extension_catalog_derived":
            raise MatrixError(f"standard capability {name} cannot use extension basis")

        indexed[name] = row

    if extension_count != 1 or "supportsInlineValues" not in indexed:
        raise MatrixError("supportsInlineValues must be the single current capability extension")
    if indexed["supportsInlineValues"].get("classification") != "extension":
        raise MatrixError("supportsInlineValues must be classified as an extension")
    return matrix, indexed


def validate_run_identity(repository_sha: str, run_id: str, run_attempt: str) -> None:
    if HEX40.fullmatch(repository_sha) is None:
        raise MatrixError("repository SHA must be lowercase 40-character hexadecimal")
    if not run_id.isdigit() or int(run_id) <= 0:
        raise MatrixError("run ID must be a positive decimal integer")
    if not run_attempt.isdigit() or int(run_attempt) <= 0:
        raise MatrixError("run attempt must be a positive decimal integer")
