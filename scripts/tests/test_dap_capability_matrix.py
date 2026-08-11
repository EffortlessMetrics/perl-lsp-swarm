#!/usr/bin/env python3
"""Falsifiers for the DAP initialize-capability authority."""

from __future__ import annotations

import copy
import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "ci" / "dap_capability_matrix.py"
SPEC = importlib.util.spec_from_file_location("dap_capability_matrix", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def row(
    name: str,
    expression: str,
    *,
    classification: str = "standard",
    wire_type: str = "boolean",
    basis: str = "catalog_derived_not_backend_derived",
) -> dict:
    return {
        "wire_name": name,
        "classification": classification,
        "expression": expression,
        "wire_type": wire_type,
        "basis": basis,
        "owner": "#6688",
    }


def matrix() -> dict:
    return {
        "schema_version": MODULE.MATRIX_SCHEMA,
        "source": {
            "path": MODULE.PRODUCTION_SOURCE_PATH.as_posix(),
            "anchor": MODULE.PRODUCTION_ANCHOR,
            "upstream_definition": "Capabilities",
        },
        "rows": [
            row("supportsAlpha", "supports_alpha"),
            row("supportsBeta", "false", basis="fixed_false"),
            row(
                "supportsInlineValues",
                "supports_inline_values",
                classification="extension",
                basis="unversioned_extension_catalog_derived",
            ),
            row(
                "exceptionBreakpointFilters",
                "exception_breakpoint_filters",
                wire_type="array",
            ),
        ],
    }


def source() -> str:
    return '''
fn initialize() {
    let capabilities = json!({
        "supportsAlpha": supports_alpha,
        "supportsBeta": false,
        "supportsInlineValues": supports_inline_values,
        "exceptionBreakpointFilters": exception_breakpoint_filters
    });
}
'''


def schema(*, include_extension: bool = False) -> dict:
    properties = {
        "supportsAlpha": {"type": "boolean"},
        "supportsBeta": {"type": "boolean"},
        "exceptionBreakpointFilters": {"type": "array"},
    }
    if include_extension:
        properties["supportsInlineValues"] = {"type": "boolean"}
    return {"definitions": {"Capabilities": {"properties": properties}}}


class DapCapabilityMatrixTests(unittest.TestCase):
    def assertMatrixError(self, callback) -> None:  # noqa: N802
        with self.assertRaises(MODULE.MatrixError):
            callback()

    def _validated(self):
        return MODULE.validate_matrix(matrix())

    def test_happy_path_reconciles_source_and_upstream_shapes(self) -> None:
        _matrix, rows = self._validated()
        production = MODULE.extract_production_capabilities(source())
        MODULE.compare_inventory(rows, production)
        observed = MODULE.validate_upstream_classification(rows, schema())
        self.assertEqual(observed["supportsAlpha"], ["boolean"])
        self.assertEqual(observed["exceptionBreakpointFilters"], ["array"])

    def test_source_authority_is_not_matrix_selectable(self) -> None:
        for key, value in (
            ("path", "fixtures/decoy.rs"),
            ("path", "/tmp/decoy.rs"),
            ("anchor", "let decoy = json!({"),
            ("upstream_definition", "InitializeResponse"),
        ):
            with self.subTest(key=key, value=value):
                candidate = matrix()
                candidate["source"][key] = value
                self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))

    def test_duplicate_and_unknown_rows_fail(self) -> None:
        candidate = matrix()
        candidate["rows"].append(copy.deepcopy(candidate["rows"][0]))
        self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))
        candidate = matrix()
        candidate["rows"][0]["classification"] = "planned"
        self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))

    def test_row_keys_and_wire_types_are_closed(self) -> None:
        candidate = matrix()
        candidate["rows"][0]["extra"] = "ignored"
        self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))
        candidate = matrix()
        candidate["rows"][0]["wire_type"] = "object"
        self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))
        candidate = matrix()
        candidate["rows"][1]["wire_type"] = "array"
        self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))

    def test_expression_basis_and_owner_invariants_fail_closed(self) -> None:
        mutations = (
            ("expression", "supports_alpha || true"),
            ("basis", "fixed_false"),
            ("owner", "later"),
        )
        for key, value in mutations:
            with self.subTest(key=key):
                candidate = matrix()
                candidate["rows"][0][key] = value
                self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))

    def test_extension_identity_basis_and_shape_are_fixed(self) -> None:
        candidate = matrix()
        candidate["rows"].append(
            row(
                "supportsOtherExtension",
                "supports_other",
                classification="extension",
                basis="unversioned_extension_catalog_derived",
            )
        )
        self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))
        candidate = matrix()
        candidate["rows"][2]["wire_type"] = "array"
        self.assertMatrixError(lambda: MODULE.validate_matrix(candidate))

    def test_anchor_must_exist_exactly_once(self) -> None:
        self.assertMatrixError(
            lambda: MODULE.extract_production_capabilities(
                source().replace(MODULE.PRODUCTION_ANCHOR, "missing")
            )
        )
        self.assertMatrixError(
            lambda: MODULE.extract_production_capabilities(source() + source())
        )
        self.assertMatrixError(
            lambda: MODULE.extract_production_capabilities(
                source(), "let decoy = json!({"
            )
        )

    def test_parser_consumes_same_line_multiline_and_comments(self) -> None:
        compact = '''
fn initialize() {
    let capabilities = json!({
        "supportsAlpha": supports_alpha, "supportsBeta": false, // bounded
        /* extension */ "supportsInlineValues":
            supports_inline_values,
        "exceptionBreakpointFilters": exception_breakpoint_filters /* array */
    });
}
'''
        self.assertEqual(
            MODULE.extract_production_capabilities(compact),
            {
                "supportsAlpha": "supports_alpha",
                "supportsBeta": "false",
                "supportsInlineValues": "supports_inline_values",
                "exceptionBreakpointFilters": "exception_breakpoint_filters",
            },
        )

    def test_unmatched_or_complex_field_syntax_is_rejected(self) -> None:
        variants = (
            source().replace("supports_alpha,", "supports_alpha || true,"),
            source().replace(
                '"supportsBeta": false,',
                '"supportsBeta": false, spread!(other),',
            ),
            source().replace(
                '"supportsBeta": false,',
                '"supportsBeta" false,',
            ),
            source().replace("    });", ""),
        )
        for candidate in variants:
            with self.subTest(candidate=candidate):
                self.assertMatrixError(
                    lambda candidate=candidate: MODULE.extract_production_capabilities(candidate)
                )

    def test_inventory_add_remove_and_expression_drift_fail(self) -> None:
        _matrix, rows = self._validated()
        variants = (
            source().replace(
                '"supportsBeta": false,',
                '"supportsBeta": false, "supportsGamma": supports_gamma,',
            ),
            source().replace('        "supportsBeta": false,\n', ""),
            source().replace("supports_alpha", "supports_core"),
        )
        for candidate in variants:
            with self.subTest(candidate=candidate):
                production = MODULE.extract_production_capabilities(candidate)
                self.assertMatrixError(lambda: MODULE.compare_inventory(rows, production))

    def test_standard_membership_and_exact_wire_shape_are_required(self) -> None:
        _matrix, rows = self._validated()
        missing = schema()
        missing["definitions"]["Capabilities"]["properties"].pop("supportsAlpha")
        self.assertMatrixError(
            lambda: MODULE.validate_upstream_classification(rows, missing)
        )
        wrong = schema()
        wrong["definitions"]["Capabilities"]["properties"]["supportsAlpha"]["type"] = "array"
        self.assertMatrixError(
            lambda: MODULE.validate_upstream_classification(rows, wrong)
        )
        widened = schema()
        widened["definitions"]["Capabilities"]["properties"]["supportsAlpha"]["type"] = [
            "boolean",
            "null",
        ]
        self.assertMatrixError(
            lambda: MODULE.validate_upstream_classification(rows, widened)
        )

    def test_extension_that_appears_upstream_requires_reclassification(self) -> None:
        _matrix, rows = self._validated()
        self.assertMatrixError(
            lambda: MODULE.validate_upstream_classification(
                rows, schema(include_extension=True)
            )
        )

    def test_run_identity_is_exact_and_positive(self) -> None:
        MODULE.validate_run_identity("a" * 40, "123", "2")
        for sha, run_id, attempt in (
            ("abc", "123", "2"),
            ("A" * 40, "123", "2"),
            ("a" * 40, "0", "2"),
            ("a" * 40, "123", "0"),
            ("a" * 40, "run", "2"),
        ):
            with self.subTest(sha=sha, run_id=run_id, attempt=attempt):
                self.assertMatrixError(
                    lambda sha=sha, run_id=run_id, attempt=attempt: MODULE.validate_run_identity(
                        sha, run_id, attempt
                    )
                )


if __name__ == "__main__":
    unittest.main()
