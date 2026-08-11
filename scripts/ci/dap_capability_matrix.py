#!/usr/bin/env python3
"""Validate the canonical DAP initialize-capability inventory."""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path
from typing import Any, Mapping, Sequence

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from dap_capability_common import (  # noqa: E402
    AUTHORITY_MANIFEST_PATH,
    DOC_PATH,
    MATRIX_PATH,
    MATRIX_SCHEMA,
    PRODUCTION_ANCHOR,
    PRODUCTION_SOURCE_PATH,
    RECEIPT_SCHEMA,
    RECEIPT_SUBJECT_PATHS,
    MatrixError,
    object_value,
    read_json,
    validate_matrix,
    validate_run_identity,
    write_json,
)
from dap_capability_git import (  # noqa: E402
    assert_clean_tree,
    tracked_records,
    verify_candidate,
)
from dap_capability_source import (  # noqa: E402
    compare_inventory,
    extract_production_capabilities,
)
from dap_capability_upstream import (  # noqa: E402
    load_pinned_schema,
    validate_upstream_classification,
)

# Compatibility exports for focused tests and downstream imports.
_read_json = read_json
_write_receipt = write_json


def _canonical_argument_path(raw: str, expected: Path, context: str) -> Path:
    candidate = Path(raw)
    if candidate.is_absolute() or candidate.as_posix() != expected.as_posix():
        raise MatrixError(
            f"{context} is independently fixed as {expected.as_posix()!r}, got {raw!r}"
        )
    return expected


def validate_document(root: Path, rows: Mapping[str, Mapping[str, Any]]) -> None:
    path = root / DOC_PATH
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise MatrixError(f"cannot read capability guide {path}: {exc}") from exc
    boolean_count = sum(row.get("wire_type") == "boolean" for row in rows.values())
    array_count = sum(row.get("wire_type") == "array" for row in rows.values())
    required = (
        "#6688",
        "catalog-derived",
        "not backend-derived",
        "supportsInlineValues",
        "project extension",
        "fixed false",
        "candidate SHA",
        "Git object",
        "wire shape",
        str(len(rows)),
        str(boolean_count),
        str(array_count),
    )
    for marker in required:
        if marker not in text:
            raise MatrixError(f"capability guide is missing marker {marker!r}")


def run_check(args: argparse.Namespace) -> Mapping[str, Any]:
    root = Path(args.root).resolve()
    matrix_relative = _canonical_argument_path(
        args.matrix, MATRIX_PATH, "capability matrix path"
    )
    manifest_relative = _canonical_argument_path(
        args.authority_manifest,
        AUTHORITY_MANIFEST_PATH,
        "authority manifest path",
    )
    verify_candidate(root, args.repository_sha, str(args.run_id), str(args.run_attempt))
    assert_clean_tree(root)

    matrix_path = root / matrix_relative
    matrix, rows = validate_matrix(read_json(matrix_path))
    source_path = root / PRODUCTION_SOURCE_PATH
    try:
        source_text = source_path.read_text(encoding="utf-8")
    except OSError as exc:
        raise MatrixError(
            f"cannot read canonical production capability source {source_path}: {exc}"
        ) from exc
    production = extract_production_capabilities(source_text)
    compare_inventory(rows, production)

    schema_path = Path(args.schema).resolve() if args.schema else None
    schema, observed_schema = load_pinned_schema(
        root,
        root / manifest_relative,
        schema_path,
    )
    upstream_shapes = validate_upstream_classification(rows, schema)
    validate_document(root, rows)

    subjects = tracked_records(root, RECEIPT_SUBJECT_PATHS, args.repository_sha)
    assert_clean_tree(root)

    fixed_false = sum(row.get("basis") == "fixed_false" for row in rows.values())
    catalog_derived = sum(
        row.get("basis") == "catalog_derived_not_backend_derived"
        for row in rows.values()
    )
    receipt = {
        "schema_version": RECEIPT_SCHEMA,
        "created_unix_seconds": int(time.time()),
        "repository": {
            "sha": args.repository_sha,
            "clean_tree": True,
        },
        "run": {
            "id": str(args.run_id),
            "attempt": str(args.run_attempt),
        },
        "authority": {
            "matrix_schema": MATRIX_SCHEMA,
            "upstream_schema_sha256": observed_schema["sha256"],
            "upstream_git_blob_sha1": observed_schema["git_blob_sha1"],
            "subjects": subjects,
        },
        "production": {
            "source_path": PRODUCTION_SOURCE_PATH.as_posix(),
            "anchor": PRODUCTION_ANCHOR,
            "emitted_field_count": len(production),
            "expressions": dict(sorted(production.items())),
        },
        "rows": [
            dict(rows[name], upstream_types=upstream_shapes[name])
            for name in sorted(rows)
        ],
        "classification": {
            "standard": sum(
                row.get("classification") == "standard" for row in rows.values()
            ),
            "extension": sum(
                row.get("classification") == "extension" for row in rows.values()
            ),
            "fixed_false": fixed_false,
            "catalog_derived_not_backend_derived": catalog_derived,
            "boolean_wire_fields": sum(
                row.get("wire_type") == "boolean" for row in rows.values()
            ),
            "array_wire_fields": sum(
                row.get("wire_type") == "array" for row in rows.values()
            ),
        },
        "claim_boundary": (
            "inventory, wire-shape, and drift proof only; runtime capability truth "
            "remains unproven until backend/mode/prerequisite/behavior evidence is integrated"
        ),
    }
    write_json(Path(args.receipt), receipt)
    assert_clean_tree(root)
    return receipt


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".")
    parser.add_argument("--matrix", default=MATRIX_PATH.as_posix())
    parser.add_argument(
        "--authority-manifest", default=AUTHORITY_MANIFEST_PATH.as_posix()
    )
    parser.add_argument("--schema")
    parser.add_argument("--repository-sha", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--run-attempt", required=True)
    parser.add_argument("--receipt", required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        receipt = run_check(args)
    except (MatrixError, OSError) as exc:
        print(f"DAP capability matrix error: {exc}", file=sys.stderr)
        return 1
    print(f"DAP capability rows: {len(receipt['rows'])}")
    print(f"DAP capability receipt: {args.receipt}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
