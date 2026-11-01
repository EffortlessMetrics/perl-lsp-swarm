#!/usr/bin/env python3
"""Catalog-backed regression tests for LSP_MISSING_FEATURES_REPORT.md."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
FEATURES_TOML = REPO_ROOT / "features.toml"
REPORT_PATH = REPO_ROOT / "docs" / "reference" / "LSP_MISSING_FEATURES_REPORT.md"

FEATURE_ID = re.compile(r"`((?:lsp|dap)\.[a-z0-9_]+)`")

INVENTORY_START_MARKERS = (
    "## Missing / Not Advertised LSP Features",
    "## Withdrawn formatting routes",
    "## Related DAP Gaps",
)
INVENTORY_END_MARKERS = (
    "## Test Coverage Notes",
    "## How to Update This Report",
)

WITHDRAWN_FORMATTING_IDS = frozenset(
    {
        "lsp.range_formatting",
        "lsp.ranges_formatting",
        "lsp.on_type_formatting",
        "lsp.will_save_wait_until",
    }
)

WHOLE_DOCUMENT_FORMATTING_ID = "lsp.formatting"


def parse_features_catalog(path: Path) -> dict[str, bool]:
    """Return feature id -> advertised flag from features.toml."""
    text = path.read_text(encoding="utf-8")
    blocks = re.split(r"\n\[\[feature\]\]\n", text)
    catalog: dict[str, bool] = {}
    for block in blocks[1:]:
        id_match = re.search(r'^id = "([^"]+)"', block, re.MULTILINE)
        adv_match = re.search(r"^advertised = (true|false)", block, re.MULTILINE)
        if not id_match or not adv_match:
            continue
        catalog[id_match.group(1)] = adv_match.group(1) == "true"
    if not catalog:
        raise ValueError(f"no features parsed from {path}")
    return catalog


def unadvertised_catalog_ids(catalog: dict[str, bool]) -> set[str]:
    return {feature_id for feature_id, advertised in catalog.items() if not advertised}


def extract_inventory_feature_ids(report_text: str) -> set[str]:
    """Collect feature ids listed in missing/withdrawn/DAP inventory sections."""
    lines = report_text.splitlines()
    in_inventory = False
    ids: set[str] = set()
    for line in lines:
        if any(marker in line for marker in INVENTORY_START_MARKERS):
            in_inventory = True
        if in_inventory and any(marker in line for marker in INVENTORY_END_MARKERS):
            in_inventory = False
        if in_inventory:
            ids.update(FEATURE_ID.findall(line))
    return ids


def verify_catalog_parity(
    catalog: dict[str, bool],
    report_ids: set[str],
    *,
    whole_document_id: str = WHOLE_DOCUMENT_FORMATTING_ID,
) -> None:
    expected = unadvertised_catalog_ids(catalog)
    missing_from_report = expected - report_ids
    if missing_from_report:
        raise AssertionError(
            "missing-row control: report omits advertised=false catalog rows: "
            + ", ".join(sorted(missing_from_report))
        )
    falsely_advertised = {
        feature_id
        for feature_id in report_ids
        if catalog.get(feature_id) is True
    }
    if falsely_advertised:
        raise AssertionError(
            "false-advertisement control: report lists advertised=true rows: "
            + ", ".join(sorted(falsely_advertised))
        )
    extra_in_report = report_ids - expected
    if extra_in_report:
        raise AssertionError(
            "report lists ids that are not advertised=false in features.toml: "
            + ", ".join(sorted(extra_in_report))
        )
    if whole_document_id in report_ids:
        raise AssertionError(
            f"whole-document formatting control: {whole_document_id} must not appear "
            "in missing/withdrawn inventory"
        )


class LspMissingFeaturesReportTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.catalog = parse_features_catalog(FEATURES_TOML)
        cls.report_text = REPORT_PATH.read_text(encoding="utf-8")
        cls.report_ids = extract_inventory_feature_ids(cls.report_text)

    def test_catalog_parity_passes_on_current_report(self) -> None:
        verify_catalog_parity(self.catalog, self.report_ids)

    def test_withdrawn_formatting_section_lists_four_routes(self) -> None:
        section_start = self.report_text.find("## Withdrawn formatting routes")
        self.assertNotEqual(section_start, -1)
        section_end = self.report_text.find("## Related DAP Gaps", section_start)
        self.assertNotEqual(section_end, -1)
        section = self.report_text[section_start:section_end]
        listed = set(FEATURE_ID.findall(section))
        self.assertEqual(listed, WITHDRAWN_FORMATTING_IDS)

    def test_missing_row_mutant_omitting_catalog_row_fails(self) -> None:
        victim = sorted(unadvertised_catalog_ids(self.catalog))[0]
        mutant_report = self.report_text.replace(f"`{victim}`", "`deleted.mutant.placeholder`", 1)
        mutant_ids = extract_inventory_feature_ids(mutant_report)
        with self.assertRaisesRegex(AssertionError, "missing-row control"):
            verify_catalog_parity(self.catalog, mutant_ids)

    def test_false_advertisement_mutant_listing_advertised_row_fails(self) -> None:
        marker = "## Related DAP Gaps"
        insert_at = self.report_text.index(marker) + len(marker)
        mutant_report = (
            self.report_text[:insert_at]
            + f"\n\n- **`{WHOLE_DOCUMENT_FORMATTING_ID}`** - mutant row\n"
            + self.report_text[insert_at:]
        )
        mutant_ids = extract_inventory_feature_ids(mutant_report)
        with self.assertRaisesRegex(AssertionError, "false-advertisement control|whole-document"):
            verify_catalog_parity(self.catalog, mutant_ids)

    def test_flip_catalog_row_mutant_fails_false_advertisement_control(self) -> None:
        victim = next(iter(WITHDRAWN_FORMATTING_IDS))
        catalog_text = FEATURES_TOML.read_text(encoding="utf-8")
        needle = f'id = "{victim}"'
        start = catalog_text.index(needle)
        end = catalog_text.index("\n[[feature]]", start + 1)
        block = catalog_text[start:end]
        mutant_block = block.replace("advertised = false", "advertised = true", 1)
        mutant_catalog_text = catalog_text[:start] + mutant_block + catalog_text[end:]
        mutant_catalog = parse_features_catalog_from_text(mutant_catalog_text)
        with self.assertRaisesRegex(
            AssertionError, "false-advertisement control|not advertised=false"
        ):
            verify_catalog_parity(mutant_catalog, self.report_ids)


def parse_features_catalog_from_text(text: str) -> dict[str, bool]:
    blocks = re.split(r"\n\[\[feature\]\]\n", text)
    catalog: dict[str, bool] = {}
    for block in blocks[1:]:
        id_match = re.search(r'^id = "([^"]+)"', block, re.MULTILINE)
        adv_match = re.search(r"^advertised = (true|false)", block, re.MULTILINE)
        if id_match and adv_match:
            catalog[id_match.group(1)] = adv_match.group(1) == "true"
    return catalog


if __name__ == "__main__":
    unittest.main()
