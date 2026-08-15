#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
from copy import deepcopy
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest


MODULE_PATH = Path(__file__).with_name("project_contributor_topology.py")
SPEC = importlib.util.spec_from_file_location("contributor_topology", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ContributorTopologyTests(unittest.TestCase):
    def fixture_root(self, temporary: str) -> Path:
        root = Path(temporary)
        identity = root / MODULE.PRODUCT_IDENTITY_PATH
        identity.parent.mkdir(parents=True, exist_ok=True)
        identity.write_text(
            """schema_version = 1

[product]
name = "perl-lsp"
public_repository = "EffortlessMetrics/perl-lsp"
development_repository = "EffortlessMetrics/perl-lsp-swarm"
""",
            encoding="utf-8",
        )
        sync = root / MODULE.SYNC_PROTOCOL_PATH
        sync.parent.mkdir(parents=True, exist_ok=True)
        sync.write_text(
            """# perl-lsp Sync Protocol

`perl-lsp-swarm` is the active development source of truth. `perl-lsp` is the
release, history, and canonical package-lineage repo.

| Repo | Authority |
|---|---|
| `perl-lsp-swarm/main` | Active development |
| `perl-lsp/master` | Release lineage |

#### Mechanics: history-preserving complete-tree merge

git merge -s ours --no-commit swarm/main
git read-tree -u --reset swarm/main
""",
            encoding="utf-8",
        )
        return root

    def observation(self, **overrides: object) -> dict[str, object]:
        result: dict[str, object] = {
            "status": "PROVEN",
            "source": "fixture",
            "observed_at": "2026-08-15T10:30:00Z",
            "limitation": None,
            "development_repository": "EffortlessMetrics/perl-lsp-swarm",
            "development_branch": "main",
            "development_sha": "a" * 40,
            "publication_repository": "EffortlessMetrics/perl-lsp",
            "publication_branch": "master",
            "publication_sha": "b" * 40,
            "prepared_swarm_sha": None,
            "publication_join_sha": None,
            "public_release_tag": None,
            "channels": {},
        }
        result.update(overrides)
        return result

    def write_observation(self, root: Path, value: object) -> Path:
        path = root / "observation.json"
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def test_missing_live_observation_is_not_proven(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            projection = MODULE.build_projection(root)
            self.assertEqual(projection["observation"]["status"], "NOT_PROVEN")
            self.assertEqual(projection["observation"]["stage"], "not_proven")
            self.assertIsNone(projection["observation"]["development_sha"])
            self.assertEqual(
                projection["static"]["development_repository"],
                "EffortlessMetrics/perl-lsp-swarm",
            )
            MODULE.validate_projection(projection, root)

    def test_development_only_stage_does_not_imply_public_availability(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation = self.write_observation(root, self.observation())
            projection = MODULE.build_projection(root, observation)
            self.assertEqual(projection["observation"]["stage"], "development_only")
            self.assertIsNone(projection["observation"]["public_release_tag"])
            self.assertEqual(projection["observation"]["channels"], {})

    def test_prepared_candidate_is_distinct(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation = self.write_observation(
                root, self.observation(prepared_swarm_sha="c" * 40)
            )
            projection = MODULE.build_projection(root, observation)
            self.assertEqual(projection["observation"]["stage"], "prepared_candidate")
            self.assertIsNone(projection["observation"]["publication_join_sha"])

    def test_post_join_pre_release_is_distinct(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation = self.write_observation(
                root,
                self.observation(
                    prepared_swarm_sha="c" * 40,
                    publication_join_sha="d" * 40,
                ),
            )
            projection = MODULE.build_projection(root, observation)
            self.assertEqual(
                projection["observation"]["stage"], "post_join_pre_release"
            )
            self.assertIsNone(projection["observation"]["public_release_tag"])

    def test_public_release_keeps_channel_state_separate(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation = self.write_observation(
                root,
                self.observation(
                    prepared_swarm_sha="c" * 40,
                    publication_join_sha="d" * 40,
                    public_release_tag="v0.18.0",
                    channels={
                        "crates_io": "AVAILABLE",
                        "open_vsx": "NOT_PROVEN",
                    },
                ),
            )
            projection = MODULE.build_projection(root, observation)
            self.assertEqual(projection["observation"]["stage"], "public_release")
            self.assertEqual(
                projection["observation"]["channels"]["open_vsx"], "NOT_PROVEN"
            )

    def test_swapped_branches_fail_static_validation(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            sync = root / MODULE.SYNC_PROTOCOL_PATH
            text = sync.read_text(encoding="utf-8")
            text = text.replace("perl-lsp-swarm/main", "perl-lsp-swarm/master")
            text = text.replace("perl-lsp/master", "perl-lsp/main")
            sync.write_text(text, encoding="utf-8")
            with self.assertRaises(MODULE.ContributorTopologyError):
                MODULE.build_projection(root)

    def test_observation_repository_mismatch_fails(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation = self.write_observation(
                root,
                self.observation(
                    development_repository="EffortlessMetrics/perl-lsp"
                ),
            )
            with self.assertRaises(MODULE.ContributorTopologyError):
                MODULE.build_projection(root, observation)

    def test_available_channel_without_release_tag_fails(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation = self.write_observation(
                root, self.observation(channels={"crates_io": "AVAILABLE"})
            )
            with self.assertRaises(MODULE.ContributorTopologyError):
                MODULE.build_projection(root, observation)

    def test_join_without_prepared_candidate_fails(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation = self.write_observation(
                root, self.observation(publication_join_sha="d" * 40)
            )
            with self.assertRaises(MODULE.ContributorTopologyError):
                MODULE.build_projection(root, observation)

    def test_not_proven_can_retain_partial_observations_without_stage_claim(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            observation_value = self.observation(
                status="NOT_PROVEN",
                limitation="publication ruleset API unavailable",
                publication_sha=None,
                source="github-readonly",
            )
            observation = self.write_observation(root, observation_value)
            projection = MODULE.build_projection(root, observation)
            self.assertEqual(projection["observation"]["stage"], "not_proven")
            self.assertEqual(projection["observation"]["development_sha"], "a" * 40)
            self.assertIsNone(projection["observation"]["publication_sha"])

    def test_source_change_makes_checked_projection_stale(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            projection = MODULE.build_projection(root)
            sync = root / MODULE.SYNC_PROTOCOL_PATH
            sync.write_text(sync.read_text(encoding="utf-8") + "\nExtra.\n", encoding="utf-8")
            with self.assertRaises(MODULE.ContributorTopologyError):
                MODULE.validate_projection(projection, root)

    def test_digest_is_deterministic_across_channel_input_order(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            first = self.observation(
                prepared_swarm_sha="c" * 40,
                publication_join_sha="d" * 40,
                public_release_tag="v0.18.0",
                channels={"open_vsx": "NOT_PROVEN", "crates_io": "AVAILABLE"},
            )
            second = deepcopy(first)
            second["channels"] = {"crates_io": "AVAILABLE", "open_vsx": "NOT_PROVEN"}
            first_path = self.write_observation(root, first)
            first_projection = MODULE.build_projection(root, first_path)
            second_path = root / "observation-second.json"
            second_path.write_text(json.dumps(second), encoding="utf-8")
            second_projection = MODULE.build_projection(root, second_path)
            self.assertEqual(
                first_projection["projection_digest"],
                second_projection["projection_digest"],
            )

    def test_unknown_observation_field_fails_closed(self) -> None:
        with TemporaryDirectory() as temporary:
            root = self.fixture_root(temporary)
            value = self.observation(unexpected=True)
            observation = self.write_observation(root, value)
            with self.assertRaises(MODULE.ContributorTopologyError):
                MODULE.build_projection(root, observation)


if __name__ == "__main__":
    unittest.main()
