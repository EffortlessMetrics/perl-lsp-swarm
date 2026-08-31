#!/usr/bin/env python3
"""Falsifiers for the bounded live GitHub enforcement observer.

Every test is offline. The observer's transport is injected, so no test
performs a network call, and the captured-response fixtures stand in for the
exact bytes GitHub would return.
"""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

OBSERVER_PATH = Path(__file__).with_name("observe_github_enforcement.py")
MODEL_PATH = Path(__file__).with_name(
    "reconcile_github_enforcement_snapshot.py"
)


def load(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


observer = load(OBSERVER_PATH, "observe_github_enforcement")
model = load(MODEL_PATH, "github_enforcement_snapshot")

REPOSITORY = "EffortlessMetrics/perl-lsp-swarm"
REPOSITORY_ID = 1244101844
SHA = "a" * 40
POLICY = "b" * 64
SUBJECT = "c" * 64
EXACT_SOURCE = "9" * 64
APP_ID = 15368
RULESET_ID = 16664791


# ---------------------------------------------------------------------------
# Raw response fixtures — what GitHub actually returns on each surface.
# ---------------------------------------------------------------------------


def body(payload) -> bytes:
    return json.dumps(payload).encode("utf-8")


def repository_response() -> bytes:
    return body(
        {
            "id": REPOSITORY_ID,
            "full_name": REPOSITORY,
            "default_branch": "main",
        }
    )


def branch_head_response() -> bytes:
    return body({"ref": "refs/heads/main", "object": {"sha": SHA, "type": "commit"}})


def classic_response() -> bytes:
    return body(
        {
            "required_status_checks": {
                "strict": True,
                "contexts": ["Classic Required", "Both Required"],
                "checks": [
                    {"context": "Classic Required", "app_id": APP_ID},
                    {"context": "Both Required", "app_id": APP_ID},
                ],
            }
        }
    )


def ruleset_list_response() -> bytes:
    return body(
        [
            {
                "id": RULESET_ID,
                "name": f"ruleset-{RULESET_ID}",
                "target": "branch",
                "source_type": "Repository",
                "source": REPOSITORY,
                "enforcement": "active",
            }
        ]
    )


def ruleset_detail_response() -> bytes:
    return body(
        {
            "id": RULESET_ID,
            "name": f"ruleset-{RULESET_ID}",
            "target": "branch",
            "source_type": "Repository",
            "source": REPOSITORY,
            "enforcement": "active",
            "conditions": {
                "ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}
            },
            "bypass_actors": [
                {
                    "actor_id": None,
                    "actor_type": "OrganizationAdmin",
                    "bypass_mode": "always",
                }
            ],
            "rules": [
                {
                    "type": "required_status_checks",
                    "parameters": {
                        "strict_required_status_checks_policy": False,
                        "do_not_enforce_on_create": False,
                        "required_status_checks": [
                            {
                                "context": "Ruleset Required",
                                "integration_id": APP_ID,
                            },
                            {
                                "context": "Both Required",
                                "integration_id": APP_ID,
                            },
                        ],
                    },
                }
            ],
        }
    )


class RecordingTransport:
    """An injected transport that records every path it was asked to read."""

    def __init__(self, responses: dict[str, tuple[int, bytes]]) -> None:
        self.responses = responses
        self.requested: list[str] = []

    def __call__(self, path: str) -> "observer.ApiResult":
        self.requested.append(path)
        for prefix, (status, payload) in self.responses.items():
            if path == prefix:
                return observer.ApiResult(status, payload)
        return observer.ApiResult(404, body({"message": "Not Found"}))


def transport(
    *,
    classic: tuple[int, bytes] | None = None,
    rulesets: tuple[int, bytes] | None = None,
    detail: tuple[int, bytes] | None = None,
) -> RecordingTransport:
    return RecordingTransport(
        {
            f"repos/{REPOSITORY}": (200, repository_response()),
            f"repos/{REPOSITORY}/git/ref/heads/main": (
                200,
                branch_head_response(),
            ),
            f"repos/{REPOSITORY}/branches/main/protection": (
                classic if classic is not None else (200, classic_response())
            ),
            f"repos/{REPOSITORY}/rulesets?includes_parents=true&per_page=100": (
                rulesets
                if rulesets is not None
                else (200, ruleset_list_response())
            ),
            f"repos/{REPOSITORY}/rulesets/{RULESET_ID}": (
                detail if detail is not None else (200, ruleset_detail_response())
            ),
        }
    )


def static_receipt() -> dict:
    return {
        "schema_version": 2,
        "status": "SUCCESS",
        "subject_sha256": SUBJECT,
        "exact_source_sha256": EXACT_SOURCE,
        "subjects": {
            "repository_sha": SHA,
            "policy": {
                "path": ".ci/policies/required-checks.toml",
                "sha256": POLICY,
                "version": 2,
                "source": "github-enforcement-union",
            },
            "contexts": [
                {
                    "name": "Classic Required",
                    "policy_role": "required",
                    "enforcement": "github-branch-protection",
                    "producer": "repository-job",
                    "classic_app_id": APP_ID,
                },
                {
                    "name": "Ruleset Required",
                    "policy_role": "required",
                    "enforcement": "github-ruleset",
                    "producer": "repository-job",
                    "ruleset_integration_id": APP_ID,
                },
                {
                    "name": "Both Required",
                    "policy_role": "required",
                    "enforcement": "github-branch-protection+ruleset",
                    "producer": "repository-job",
                    "classic_app_id": APP_ID,
                    "ruleset_integration_id": APP_ID,
                },
            ],
        },
    }


def authority(snapshot: dict, evaluated_at: str = "2026-08-16T00:05:00Z") -> dict:
    """Operator-declared identity, deliberately not read from the snapshot."""
    return observer.build_authority(
        producer="github-enforcement-observer",
        declared_repository=REPOSITORY,
        declared_repository_id=REPOSITORY_ID,
        declared_branch="main",
        max_age_seconds=3600,
        max_future_skew_seconds=300,
        evaluated_at=evaluated_at,
    )


def observe(
    which: RecordingTransport | None = None,
    *,
    source: str = "trusted_default_branch",
    observed_at: str = "2026-08-16T00:00:00Z",
) -> dict:
    capture = observer.capture_live(
        REPOSITORY, "main", which if which is not None else transport()
    )
    return observer.build_snapshot(
        capture,
        source=source,
        branch="main",
        static_receipt=static_receipt(),
        observed_at=observed_at,
    )


class CompleteObservation(unittest.TestCase):
    def test_complete_observation_reconciles_to_a_live_verdict(self) -> None:
        """The observer's output must be consumable evidence, not just valid.

        This is the load-bearing end-to-end proof: raw API bytes in, a real
        MATCH out of the #9152 reconciler, with no NOT_PROVEN limitation.
        """
        snapshot = observe()
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "MATCH", result["limitations"])
        self.assertEqual(result["limitations"], [])
        self.assertEqual(result["differences"], [])

    def test_snapshot_validates_against_the_p2_input_contract(self) -> None:
        snapshot = observe()
        normalized = model.validate_snapshot(snapshot)
        self.assertEqual(normalized["schema_version"], 1)

    def test_live_union_carries_both_source_bindings(self) -> None:
        snapshot = observe()
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        both = next(
            row for row in result["live_union"] if row["context"] == "Both Required"
        )
        self.assertEqual(both["source_class"], "both")

    def test_observer_reads_only_the_bounded_get_surface(self) -> None:
        """Read-only proof: the observer touches no mutating endpoint."""
        recorder = transport()
        observer.capture_live(REPOSITORY, "main", recorder)
        self.assertEqual(
            recorder.requested,
            [
                f"repos/{REPOSITORY}",
                f"repos/{REPOSITORY}/git/ref/heads/main",
                f"repos/{REPOSITORY}/branches/main/protection",
                f"repos/{REPOSITORY}/rulesets?includes_parents=true&per_page=100",
                f"repos/{REPOSITORY}/rulesets/{RULESET_ID}",
            ],
        )


class PartialAccessIsNotProven(unittest.TestCase):
    """The issue's first two negative controls, proven through the reconciler."""

    def test_classic_readable_but_rulesets_forbidden_is_not_proven(self) -> None:
        snapshot = observe(transport(rulesets=(403, body({"message": "Forbidden"}))))
        self.assertEqual(snapshot["rulesets"]["instrument_state"], "unreadable")
        self.assertEqual(snapshot["observation"]["permission"], "partial")
        self.assertIn(
            observer.RULESET_LIST_FORBIDDEN, snapshot["observation"]["limitations"]
        )
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("rulesets_not_observed", codes(result))

    def test_rulesets_readable_but_classic_forbidden_is_not_proven(self) -> None:
        snapshot = observe(transport(classic=(403, body({"message": "Forbidden"}))))
        self.assertEqual(
            snapshot["classic_branch_protection"]["instrument_state"], "unreadable"
        )
        self.assertIn(
            observer.CLASSIC_FORBIDDEN, snapshot["observation"]["limitations"]
        )
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("classic_branch_protection_not_observed", codes(result))

    def test_both_surfaces_forbidden_yields_unknown_permission(self) -> None:
        snapshot = observe(
            transport(
                classic=(403, body({"message": "Forbidden"})),
                rulesets=(403, body({"message": "Forbidden"})),
            )
        )
        self.assertEqual(snapshot["observation"]["permission"], "unknown")
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")

    def test_transport_failure_is_an_error_surface_not_an_empty_one(self) -> None:
        recorder = transport()
        original = recorder.__call__

        def failing(path: str):
            if path.endswith("/protection"):
                return observer.ApiResult(None, b"", transport_failed=True)
            return original(path)

        capture = observer.capture_live(REPOSITORY, "main", failing)
        snapshot = observer.build_snapshot(
            capture,
            source="operator",
            branch="main",
            static_receipt=static_receipt(),
            observed_at="2026-08-16T00:00:00Z",
        )
        self.assertEqual(
            snapshot["classic_branch_protection"]["instrument_state"], "error"
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")


class NeverAnEmptyUnion(unittest.TestCase):
    """An unreadable surface must never read as `nothing is enforced`."""

    def test_forbidden_surface_carries_no_rows_and_no_digest(self) -> None:
        snapshot = observe(transport(classic=(403, body({"message": "Forbidden"}))))
        classic = snapshot["classic_branch_protection"]
        self.assertEqual(classic["required_status_checks"], [])
        self.assertIsNone(classic["response_sha256"])
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")

    def test_invariant_rejects_a_laundered_empty_surface(self) -> None:
        """Directly falsify the wrong implementation this guard exists for.

        A permission failure presented as an observed-but-empty surface with
        complete permission would reconcile as real evidence of no
        enforcement. The invariant must reject it.
        """
        snapshot = observe()
        snapshot["classic_branch_protection"]["instrument_state"] = "unreadable"
        snapshot["classic_branch_protection"]["required_status_checks"] = []
        snapshot["classic_branch_protection"]["response_sha256"] = None
        snapshot["observation"]["permission"] = "complete"
        snapshot["observation"]["limitations"] = []
        with self.assertRaises(observer.ObserverError):
            observer.enforce_no_empty_surface_claim(snapshot)

    def test_invariant_rejects_rows_on_an_unobserved_surface(self) -> None:
        snapshot = observe()
        snapshot["rulesets"]["instrument_state"] = "unreadable"
        with self.assertRaises(observer.ObserverError):
            observer.enforce_no_empty_surface_claim(snapshot)

    def test_unprotected_branch_is_missing_not_unreadable(self) -> None:
        """A definitive 404 absence is not the same evidence class as a 403.

        An implementation that lumps every non-200 together would report a
        permission failure as an absent instrument, or vice versa.
        """
        snapshot = observe(
            transport(classic=(404, body({"message": "Branch not protected"})))
        )
        classic = snapshot["classic_branch_protection"]
        self.assertEqual(classic["instrument_state"], "missing")
        self.assertEqual(classic["required_status_checks"], [])
        self.assertEqual(snapshot["observation"]["permission"], "complete")
        self.assertEqual(snapshot["observation"]["limitations"], [])
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("classic_branch_protection_not_observed", codes(result))


class RulesetObservation(unittest.TestCase):
    def test_ruleset_integration_id_is_carried_as_app_id(self) -> None:
        """GitHub sends `integration_id` on rulesets; the contract wants app_id.

        An implementation reading `app_id` from a ruleset payload would find
        nothing and silently emit an unbound identity.
        """
        snapshot = observe()
        checks = snapshot["rulesets"]["items"][0]["required_status_checks"]
        self.assertEqual(
            checks,
            [
                {"context": "Both Required", "app_id": APP_ID},
                {"context": "Ruleset Required", "app_id": APP_ID},
            ],
        )

    def test_ruleset_detail_failure_names_the_ruleset_and_downgrades(self) -> None:
        snapshot = observe(transport(detail=(403, body({"message": "Forbidden"}))))
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertEqual(snapshot["observation"]["permission"], "partial")
        self.assertIn(
            f"{observer.RULESET_DETAIL_FORBIDDEN}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")

    def test_non_branch_rulesets_are_not_branch_enforcement(self) -> None:
        listing = body(
            [
                {
                    "id": 42,
                    "name": "tag-ruleset",
                    "target": "tag",
                    "source_type": "Repository",
                    "source": REPOSITORY,
                    "enforcement": "active",
                }
            ]
        )
        snapshot = observe(transport(rulesets=(200, listing)))
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertEqual(snapshot["observation"]["permission"], "complete")

    def test_unrepresentable_ref_conditions_are_reported_not_dropped(self) -> None:
        """Silently dropping a ruleset would understate live enforcement."""
        detail = json.loads(ruleset_detail_response())
        detail["conditions"]["ref_name"]["include"] = []
        snapshot = observe(transport(detail=(200, body(detail))))
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertIn(
            f"{observer.RULESET_DETAIL_UNREPRESENTABLE}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")

    def test_truncated_ruleset_listing_is_reported_not_silently_subset(
        self,
    ) -> None:
        """A second page of rulesets would understate live enforcement.

        The listing is requested at the maximum page size, but a repository
        that outgrows one page must not produce a `complete` observation built
        from a subset of its rulesets.
        """
        recorder = transport()
        listing_path = (
            f"repos/{REPOSITORY}/rulesets?includes_parents=true&per_page=100"
        )
        inner = recorder.__call__

        def paginated(path: str):
            result = inner(path)
            if path == listing_path:
                return observer.ApiResult(
                    result.status,
                    result.body,
                    link='<https://api.github.com/x?page=2>; rel="next"',
                )
            return result

        capture = observer.capture_live(REPOSITORY, "main", paginated)
        snapshot = observer.build_snapshot(
            capture,
            source="operator",
            branch="main",
            static_receipt=static_receipt(),
            observed_at="2026-08-16T00:00:00Z",
        )
        self.assertIn(
            observer.RULESET_LIST_TRUNCATED, snapshot["observation"]["limitations"]
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")

        # The connector shape must not launder the truncation away.
        restored = observer.Capture.from_bundle(capture.to_bundle())
        imported = observer.build_snapshot(
            restored,
            source="connector",
            branch="main",
            static_receipt=static_receipt(),
            observed_at="2026-08-16T00:00:00Z",
        )
        self.assertIn(
            observer.RULESET_LIST_TRUNCATED, imported["observation"]["limitations"]
        )

    def test_listing_is_requested_at_the_maximum_page_size(self) -> None:
        recorder = transport()
        observer.capture_live(REPOSITORY, "main", recorder)
        self.assertIn(
            f"repos/{REPOSITORY}/rulesets?includes_parents=true&per_page=100",
            recorder.requested,
        )

    def test_inactive_ruleset_is_observed_and_left_to_the_reconciler(self) -> None:
        detail = json.loads(ruleset_detail_response())
        detail["enforcement"] = "evaluate"
        listing = json.loads(ruleset_list_response())
        listing[0]["enforcement"] = "evaluate"
        snapshot = observe(
            transport(rulesets=(200, body(listing)), detail=(200, body(detail)))
        )
        self.assertEqual(snapshot["rulesets"]["items"][0]["enforcement"], "evaluate")
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        excluded = {row["id"]: row["reason"] for row in result["excluded_rulesets"]}
        self.assertEqual(excluded.get(RULESET_ID), "inactive")


class ClassicObservation(unittest.TestCase):
    def test_contexts_without_checks_do_not_invent_an_app_id(self) -> None:
        payload = {
            "required_status_checks": {
                "strict": False,
                "contexts": ["Legacy Context"],
                "checks": [],
            }
        }
        snapshot = observe(transport(classic=(200, body(payload))))
        self.assertEqual(
            snapshot["classic_branch_protection"]["required_status_checks"],
            [{"context": "Legacy Context", "app_id": None}],
        )

    def test_any_source_sentinel_is_not_an_app_identity(self) -> None:
        """Classic protection uses app_id -1 for "any source".

        Rejecting it as a non-positive integer would make the classic surface
        unreadable for a common, legitimate configuration; carrying it through
        as -1 would invent an app binding that does not exist.
        """
        payload = {
            "required_status_checks": {
                "strict": True,
                "contexts": ["Classic Required"],
                "checks": [{"context": "Classic Required", "app_id": -1}],
            }
        }
        snapshot = observe(transport(classic=(200, body(payload))))
        classic = snapshot["classic_branch_protection"]
        self.assertEqual(classic["instrument_state"], "observed")
        self.assertEqual(
            classic["required_status_checks"],
            [{"context": "Classic Required", "app_id": None}],
        )

    def test_any_source_against_a_declared_app_binding_is_drift(self) -> None:
        """The mapping must not launder a real disagreement into a match."""
        payload = {
            "required_status_checks": {
                "strict": True,
                "checks": [
                    {"context": "Classic Required", "app_id": -1},
                    {"context": "Both Required", "app_id": APP_ID},
                ],
            }
        }
        snapshot = observe(transport(classic=(200, body(payload))))
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "DRIFT")
        self.assertIn(
            "classic_app_identity_mismatch",
            {row["code"] for row in result["differences"]},
        )

    def test_duplicate_bypass_actors_do_not_invalidate_the_snapshot(self) -> None:
        """The reconciler rejects a duplicate bypass identity outright."""
        detail = json.loads(ruleset_detail_response())
        detail["bypass_actors"] = [
            {"actor_id": None, "actor_type": "OrganizationAdmin", "bypass_mode": "always"},
            {"actor_id": None, "actor_type": "OrganizationAdmin", "bypass_mode": "always"},
        ]
        snapshot = observe(transport(detail=(200, body(detail))))
        self.assertEqual(len(snapshot["rulesets"]["items"][0]["bypass_actors"]), 1)
        model.validate_snapshot(snapshot)

    def test_protection_without_status_checks_is_observed_and_empty(self) -> None:
        snapshot = observe(transport(classic=(200, body({"allow_forks": True}))))
        classic = snapshot["classic_branch_protection"]
        self.assertEqual(classic["instrument_state"], "observed")
        self.assertIsNone(classic["strict"])
        self.assertEqual(classic["required_status_checks"], [])
        self.assertIsNotNone(classic["response_sha256"])


class EvidenceBinding(unittest.TestCase):
    def test_snapshot_binds_the_exact_static_subject(self) -> None:
        snapshot = observe()
        self.assertEqual(
            snapshot["static_contract"],
            {
                "subject_sha256": SUBJECT,
                "exact_source_sha256": EXACT_SOURCE,
                "policy_sha256": POLICY,
                "repository_sha": SHA,
            },
        )

    def test_non_success_static_receipt_cannot_be_bound(self) -> None:
        receipt = static_receipt()
        receipt["status"] = "FAILURE"
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture,
                source="operator",
                branch="main",
                static_receipt=receipt,
            )

    def test_response_digests_are_taken_from_the_exact_bytes(self) -> None:
        import hashlib

        snapshot = observe()
        self.assertEqual(
            snapshot["classic_branch_protection"]["response_sha256"],
            hashlib.sha256(classic_response()).hexdigest(),
        )
        self.assertEqual(
            snapshot["rulesets"]["items"][0]["detail_response_sha256"],
            hashlib.sha256(ruleset_detail_response()).hexdigest(),
        )

    def test_unreadable_repository_identity_fails_closed(self) -> None:
        """No identity means no bindable subject, so no snapshot at all."""
        recorder = RecordingTransport({})
        capture = observer.capture_live(REPOSITORY, "main", recorder)
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture,
                source="operator",
                branch="main",
                static_receipt=static_receipt(),
            )

    def test_observer_cannot_emit_a_fixture_observation(self) -> None:
        """A fixture source would launder synthetic data as live evidence."""
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture,
                source="fixture",
                branch="main",
                static_receipt=static_receipt(),
            )


class CaptureBundle(unittest.TestCase):
    def test_round_trip_preserves_response_digests(self) -> None:
        """The connector shape must not re-serialize the evidence it hashes."""
        capture = observer.capture_live(REPOSITORY, "main", transport())
        restored = observer.Capture.from_bundle(capture.to_bundle())
        direct = observer.build_snapshot(
            capture,
            source="operator",
            branch="main",
            static_receipt=static_receipt(),
            observed_at="2026-08-16T00:00:00Z",
        )
        imported = observer.build_snapshot(
            restored,
            source="connector",
            branch="main",
            static_receipt=static_receipt(),
            observed_at="2026-08-16T00:00:00Z",
        )
        direct["observation"]["source"] = "connector"
        self.assertEqual(direct, imported)

    def test_malformed_bundle_is_rejected(self) -> None:
        for bundle in (
            {"schema_version": 2, "entries": []},
            {"schema_version": 1, "entries": {}},
            {"schema_version": 1, "entries": [{"key": "repository"}]},
            {
                "schema_version": 1,
                "entries": [
                    {"key": "repository", "status": 200, "body_base64": "!!!"}
                ],
            },
        ):
            with self.assertRaises(observer.ObserverError):
                observer.Capture.from_bundle(bundle)

    def test_bundle_cannot_represent_a_state_the_transport_never_emits(
        self,
    ) -> None:
        """Imported evidence must be a state the live observer could produce.

        A status alongside a transport failure, or neither, describes an
        outcome no real capture can reach; admitting it would let connector
        evidence claim something the observer never saw.
        """
        for status, failed in ((200, True), (None, False), (403, True)):
            with self.assertRaises(observer.ObserverError):
                observer.Capture.from_bundle(
                    {
                        "schema_version": 1,
                        "entries": [
                            {
                                "key": "repository",
                                "status": status,
                                "transport_failed": failed,
                                "body_base64": "",
                            }
                        ],
                    }
                )

    def test_bundle_missing_a_listed_ruleset_detail_is_reported(self) -> None:
        """A bundle can name a ruleset it never captured.

        Without this the `ruleset_list_incomplete` branch is unreachable in
        the suite: an implementation that raised, or that dropped the ruleset
        with no limitation at all, would still pass.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        bundle = capture.to_bundle()
        bundle["entries"] = [
            entry
            for entry in bundle["entries"]
            if entry["key"] != observer.ruleset_key(RULESET_ID)
        ]
        restored = observer.Capture.from_bundle(bundle)
        snapshot = observer.build_snapshot(
            restored,
            source="connector",
            branch="main",
            static_receipt=static_receipt(),
            observed_at="2026-08-16T00:00:00Z",
        )
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertIn(
            f"{observer.RULESET_LIST_INCOMPLETE}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")

    def test_bundle_transport_failed_must_be_a_boolean(self) -> None:
        with self.assertRaises(observer.ObserverError):
            observer.Capture.from_bundle(
                {
                    "schema_version": 1,
                    "entries": [
                        {
                            "key": "repository",
                            "status": None,
                            "transport_failed": "yes",
                            "body_base64": "",
                        }
                    ],
                }
            )


class FreshnessBinding(unittest.TestCase):
    def test_stale_observation_is_rejected_by_the_authority(self) -> None:
        """A stale capture cannot satisfy a newer subject."""
        snapshot = observe()
        stale = authority(snapshot, evaluated_at="2026-08-17T00:00:00Z")
        result = model.reconcile(snapshot, static_receipt(), stale)
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("observation_stale", codes(result))

    def test_authority_restates_identity_independently(self) -> None:
        snapshot = observe()
        built = authority(snapshot)
        self.assertEqual(built["repository"]["full_name"], REPOSITORY)
        self.assertEqual(built["repository"]["repository_id"], REPOSITORY_ID)
        self.assertEqual(built["max_observation_age_seconds"], 3600)


class CommandLine(unittest.TestCase):
    def test_assemble_writes_snapshot_and_authority_from_a_bundle(self) -> None:
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            snapshot = root / "snapshot.json"
            auth = root / "authority.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            receipt.write_text(json.dumps(static_receipt()), encoding="utf-8")
            code = observer.main(
                [
                    "assemble",
                    "--capture",
                    str(bundle),
                    "--repository",
                    REPOSITORY,
                    "--authority-repository-id",
                    str(REPOSITORY_ID),
                    "--source",
                    "connector",
                    "--branch",
                    "main",
                    "--static-receipt",
                    str(receipt),
                    "--snapshot",
                    str(snapshot),
                    "--authority",
                    str(auth),
                ]
            )
            self.assertEqual(code, 0)
            written = json.loads(snapshot.read_text(encoding="utf-8"))
            self.assertEqual(written["observation"]["permission"], "complete")
            result = model.reconcile(
                written,
                static_receipt(),
                json.loads(auth.read_text(encoding="utf-8")),
            )
            self.assertIn(result["status"], {"MATCH", "DRIFT"})

    def test_incomplete_observation_exits_non_zero(self) -> None:
        capture = observer.capture_live(
            REPOSITORY,
            "main",
            transport(rulesets=(403, body({"message": "Forbidden"}))),
        )
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            receipt.write_text(json.dumps(static_receipt()), encoding="utf-8")
            code = observer.main(
                [
                    "assemble",
                    "--capture",
                    str(bundle),
                    "--repository",
                    REPOSITORY,
                    "--source",
                    "operator",
                    "--static-receipt",
                    str(receipt),
                    "--snapshot",
                    str(root / "snapshot.json"),
                ]
            )
            self.assertEqual(code, 2)

    def test_missing_static_receipt_fails_closed(self) -> None:
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            code = observer.main(
                [
                    "assemble",
                    "--capture",
                    str(bundle),
                    "--repository",
                    REPOSITORY,
                    "--source",
                    "operator",
                    "--static-receipt",
                    str(root / "absent.json"),
                    "--snapshot",
                    str(root / "snapshot.json"),
                ]
            )
            self.assertEqual(code, 1)


class HttpTransport(unittest.TestCase):
    """The real transport, proven without a network call.

    Every other test injects a fake transport, so without these the live
    `http_transport` is unexercised and its Link/status plumbing could be
    removed with the suite still green.
    """

    class FakeResponse:
        def __init__(self, status: int, payload: bytes, link: str = "") -> None:
            self.status = status
            self.headers = {"Link": link} if link else {}
            self._payload = payload

        def read(self) -> bytes:
            return self._payload

        def __enter__(self):
            return self

        def __exit__(self, *exc) -> bool:
            return False

    class FakeOpener:
        def __init__(self, fake, captured) -> None:
            self.fake = fake
            self.captured = captured

        def open(self, request, timeout=None):
            self.captured["url"] = request.full_url
            self.captured["method"] = request.get_method()
            self.captured["headers"] = dict(request.header_items())
            if isinstance(self.fake, Exception):
                raise self.fake
            return self.fake

    def run_with(self, fake, path: str = "repos/o/r"):
        import urllib.request

        original = urllib.request.build_opener
        captured = {}
        captured["handlers"] = ()

        def fake_build_opener(*handlers):
            captured["handlers"] = handlers
            return self.FakeOpener(fake, captured)

        urllib.request.build_opener = fake_build_opener
        try:
            result = observer.http_transport("secret-token")(path)
        finally:
            urllib.request.build_opener = original
        return result, captured

    def test_transport_is_built_with_the_no_redirect_handler(self) -> None:
        """A redirect would forward the bearer token to another host.

        urllib's default opener copies request headers onto the redirected
        request without checking the host, so the handler must be installed.
        """
        _, captured = self.run_with(self.FakeResponse(200, b"[]"))
        self.assertIn(observer.NoRedirect, captured["handlers"])

    def test_no_redirect_handler_refuses_to_follow(self) -> None:
        handler = observer.NoRedirect()
        self.assertIsNone(
            handler.redirect_request(
                None, None, 302, "Found", {}, "https://evil.example/x"
            )
        )

    def test_link_header_reaches_the_result(self) -> None:
        link = '<https://api.github.com/x?page=2>; rel="next"'
        result, _ = self.run_with(self.FakeResponse(200, b"[]", link))
        self.assertEqual(result.status, 200)
        self.assertTrue(result.has_next_page)

    def test_absent_link_header_is_not_a_next_page(self) -> None:
        result, _ = self.run_with(self.FakeResponse(200, b"[]"))
        self.assertFalse(result.has_next_page)

    def test_request_is_a_get_to_the_github_api_with_the_token(self) -> None:
        _, captured = self.run_with(self.FakeResponse(200, b"[]"))
        self.assertEqual(captured["method"], "GET")
        self.assertEqual(captured["url"], "https://api.github.com/repos/o/r")
        self.assertEqual(
            captured["headers"].get("Authorization"), "Bearer secret-token"
        )

    def test_http_error_becomes_a_status_not_an_exception(self) -> None:
        import urllib.error

        error = urllib.error.HTTPError(
            "https://api.github.com/repos/o/r", 403, "Forbidden", {}, None
        )
        error.read = lambda: b'{"message":"Forbidden"}'
        result, _ = self.run_with(error)
        self.assertEqual(result.status, 403)
        self.assertTrue(result.forbidden)
        self.assertFalse(result.ok)

    def test_host_error_is_a_transport_failure_with_no_retained_text(self) -> None:
        import urllib.error

        result, _ = self.run_with(
            urllib.error.URLError("host unreachable at /home/runner/secret")
        )
        self.assertTrue(result.transport_failed)
        self.assertIsNone(result.status)
        self.assertEqual(result.body, b"")


class PathEncoding(unittest.TestCase):
    """A branch name is not a safe path fragment."""

    def test_reserved_characters_in_a_branch_are_encoded(self) -> None:
        recorder = RecordingTransport({})
        observer.capture_live(REPOSITORY, "feature/a?b#c", recorder)
        self.assertIn(
            f"repos/{REPOSITORY}/branches/feature%2Fa%3Fb%23c/protection",
            recorder.requested,
        )
        self.assertIn(
            f"repos/{REPOSITORY}/git/ref/heads/feature%2Fa%3Fb%23c",
            recorder.requested,
        )
        for path in recorder.requested:
            self.assertNotIn("#", path)

    def test_a_branch_cannot_inject_extra_path_segments(self) -> None:
        recorder = RecordingTransport({})
        observer.capture_live(REPOSITORY, "../../rulesets", recorder)
        self.assertIn(
            "repos/EffortlessMetrics/perl-lsp-swarm"
            "/branches/..%2F..%2Frulesets/protection",
            recorder.requested,
        )

    def test_repository_must_be_exactly_owner_and_name(self) -> None:
        for bad in ("owner", "owner/name/extra", "/name", "owner/"):
            with self.assertRaises(observer.ObserverError):
                observer.capture_live(bad, "main", RecordingTransport({}))


class MalformedSurfaces(unittest.TestCase):
    """A 200 we cannot parse is an unreadable surface, not a lost capture.

    Aborting would discard identity and branch-head evidence that is still
    bindable, and would leave the reconciler with nothing to judge.
    """

    def test_malformed_classic_body_is_unreadable_not_fatal(self) -> None:
        snapshot = observe(transport(classic=(200, b"{not json")))
        classic = snapshot["classic_branch_protection"]
        self.assertEqual(classic["instrument_state"], "unreadable")
        self.assertEqual(classic["required_status_checks"], [])
        self.assertIsNone(classic["response_sha256"])
        self.assertIn(
            observer.CLASSIC_UNREADABLE, snapshot["observation"]["limitations"]
        )
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")

    def test_classic_body_of_the_wrong_shape_is_unreadable(self) -> None:
        snapshot = observe(transport(classic=(200, body(["not", "an", "object"]))))
        self.assertEqual(
            snapshot["classic_branch_protection"]["instrument_state"], "unreadable"
        )

    def test_malformed_ruleset_listing_is_unreadable_not_fatal(self) -> None:
        snapshot = observe(transport(rulesets=(200, b"[[[")))
        rulesets = snapshot["rulesets"]
        self.assertEqual(rulesets["instrument_state"], "unreadable")
        self.assertEqual(rulesets["items"], [])
        self.assertIsNone(rulesets["list_response_sha256"])
        self.assertIn(
            observer.RULESET_LIST_UNREADABLE, snapshot["observation"]["limitations"]
        )

    def test_malformed_ruleset_detail_is_reported_per_ruleset(self) -> None:
        snapshot = observe(transport(detail=(200, b"{broken")))
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertIn(
            f"{observer.RULESET_DETAIL_UNREADABLE}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")

    def test_identity_failure_still_fails_closed_with_no_snapshot(self) -> None:
        """Unlike a surface, a missing subject leaves nothing to bind."""
        capture = observer.capture_live(REPOSITORY, "main", RecordingTransport({}))
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture,
                source="operator",
                branch="main",
                static_receipt=static_receipt(),
            )


class AuthorityIndependence(unittest.TestCase):
    """The authority must not be derived from the thing it authenticates."""

    def test_authority_uses_declared_identity_not_the_observation(self) -> None:
        snapshot = observe()
        built = observer.build_authority(
            producer="p",
            declared_repository="Other/repo",
            declared_repository_id=99,
            declared_branch="release",
            max_age_seconds=3600,
            max_future_skew_seconds=300,
            evaluated_at="2026-08-16T00:05:00Z",
        )
        self.assertEqual(built["repository"]["full_name"], "Other/repo")
        self.assertEqual(built["repository"]["repository_id"], 99)
        self.assertEqual(built["repository"]["default_branch"], "release")

    def test_a_mismatched_declaration_is_caught_by_the_reconciler(self) -> None:
        """This is the whole point: disagreement must surface, not be erased.

        An implementation that copied identity out of the snapshot could never
        produce this mismatch, so this test fails against it.
        """
        snapshot = observe()
        built = observer.build_authority(
            producer="p",
            declared_repository="Other/repo",
            declared_repository_id=99,
            declared_branch="main",
            max_age_seconds=3600,
            max_future_skew_seconds=300,
            evaluated_at="2026-08-16T00:05:00Z",
        )
        result = model.reconcile(snapshot, static_receipt(), built)
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("repository_name_mismatch", codes(result))

    def test_declared_identity_is_validated(self) -> None:
        for repository, identifier in (
            ("owner", 1),
            ("owner/name/extra", 1),
            ("owner/name", 0),
            ("owner/name", -1),
            ("owner/name", True),
        ):
            with self.assertRaises(observer.ObserverError):
                observer.build_authority(
                    producer="p",
                    declared_repository=repository,
                    declared_repository_id=identifier,
                    declared_branch="main",
                    max_age_seconds=3600,
                    max_future_skew_seconds=300,
                )

    def test_cli_refuses_to_write_an_authority_without_declared_id(self) -> None:
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            receipt.write_text(json.dumps(static_receipt()), encoding="utf-8")
            code = observer.main(
                [
                    "assemble",
                    "--capture",
                    str(bundle),
                    "--repository",
                    REPOSITORY,
                    "--source",
                    "operator",
                    "--static-receipt",
                    str(receipt),
                    "--snapshot",
                    str(root / "snapshot.json"),
                    "--authority",
                    str(root / "authority.json"),
                ]
            )
            self.assertEqual(code, 1)
            self.assertFalse((root / "authority.json").exists())


class Privacy(unittest.TestCase):
    def test_limitations_never_carry_raw_host_text(self) -> None:
        """Limitations are a closed vocabulary, not relayed API prose."""
        secret = "token ghp_examplevalue is invalid at /home/runner/work"
        snapshot = observe(
            transport(
                classic=(403, body({"message": secret})),
                rulesets=(500, body({"message": secret})),
                detail=(500, body({"message": secret})),
            )
        )
        rendered = json.dumps(snapshot)
        self.assertNotIn("ghp_examplevalue", rendered)
        self.assertNotIn("/home/runner", rendered)
        for limitation in snapshot["observation"]["limitations"]:
            self.assertIn(
                limitation.split(":")[0],
                {
                    observer.CLASSIC_FORBIDDEN,
                    observer.CLASSIC_UNREADABLE,
                    observer.RULESET_LIST_FORBIDDEN,
                    observer.RULESET_LIST_UNREADABLE,
                    observer.RULESET_DETAIL_FORBIDDEN,
                    observer.RULESET_DETAIL_UNREADABLE,
                    observer.RULESET_DETAIL_UNREPRESENTABLE,
                    observer.RULESET_LIST_INCOMPLETE,
                },
            )

    def test_token_is_read_from_environment_and_not_retained(self) -> None:
        self.assertEqual(observer.resolve_token({"GITHUB_TOKEN": "value"}), "value")
        self.assertEqual(observer.resolve_token({"GH_TOKEN": "other"}), "other")
        self.assertIsNone(observer.resolve_token({}))
        built = observer.headers("value")
        self.assertEqual(built["Authorization"], "Bearer value")
        snapshot = observe()
        self.assertNotIn("value", json.dumps(snapshot))


def codes(result: dict) -> set[str]:
    return {row["code"] for row in result["limitations"]}


if __name__ == "__main__":
    unittest.main()
