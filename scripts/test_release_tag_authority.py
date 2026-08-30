#!/usr/bin/env python3
"""Fail-closed tests for release-tag immutability and currentness authority."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

MODULE = Path(__file__).with_name("release_tag_authority.py")
SPEC = importlib.util.spec_from_file_location("release_tag_authority", MODULE)
assert SPEC is not None and SPEC.loader is not None
subject = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = subject
SPEC.loader.exec_module(subject)

SHA = "a" * 40
TAG = "v0.18.0-rc.1"


def ruleset() -> dict[str, object]:
    return {
        "id": 21821148,
        "name": "release-tags",
        "target": "tag",
        "source_type": "Repository",
        "source": "EffortlessMetrics/perl-lsp-swarm",
        "enforcement": "active",
        "conditions": {"ref_name": {"exclude": [], "include": ["refs/tags/v*"]}},
        "rules": [
            {"type": "deletion"},
            {"type": "non_fast_forward"},
            {"type": "update"},
        ],
        "bypass_actors": [],
        "current_user_can_bypass": "never",
    }


def tag_ref(sha: str = SHA) -> dict[str, object]:
    return {"ref": f"refs/tags/{TAG}", "object": {"type": "commit", "sha": sha}}


class ReleaseTagAuthorityTests(unittest.TestCase):
    def test_active_unbypassable_immutable_tag_is_current(self) -> None:
        subject.validate(ruleset(), tag_ref(), TAG, SHA)

    def test_moved_after_create_is_not_proven(self) -> None:
        with self.assertRaisesRegex(subject.TagAuthorityError, "currentness"):
            subject.validate(ruleset(), tag_ref("b" * 40), TAG, SHA)

    def test_current_live_shape_without_update_is_not_proven(self) -> None:
        value = ruleset()
        value["rules"] = [
            {"type": "deletion"},
            {"type": "non_fast_forward"},
        ]
        with self.assertRaisesRegex(subject.TagAuthorityError, "update/deletion"):
            subject.validate(value, tag_ref(), TAG, SHA)

    def test_missing_deletion_or_bypass_actor_is_not_proven(self) -> None:
        for mutation in ("missing-deletion", "bypass"):
            value = ruleset()
            if mutation == "missing-deletion":
                value["rules"] = [{"type": "update"}]
            else:
                value["bypass_actors"] = [{"actor_type": "OrganizationAdmin"}]
            with self.subTest(mutation=mutation), self.assertRaisesRegex(
                subject.TagAuthorityError, "NOT_PROVEN"
            ):
                subject.validate(value, tag_ref(), TAG, SHA)


if __name__ == "__main__":
    unittest.main()
