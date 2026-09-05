#!/usr/bin/env python3
"""Falsifiers for the bounded live GitHub enforcement observer.

Every test is offline. The observer's transport is injected, so no test
performs a network call, and the captured-response fixtures stand in for the
exact bytes GitHub would return.
"""

from __future__ import annotations

import base64
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
    # Time is a property of the capture, not a build-time override.
    capture.captured_at = observed_at
    return observer.build_snapshot(
        capture, source=source, static_receipt=static_receipt()
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
            static_receipt=static_receipt(),
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

    def test_detail_for_the_wrong_ruleset_is_not_attributed(self) -> None:
        """A swapped bundle entry must not transfer enforcement identity.

        Without the id check, one ruleset's conditions, enforcement, and
        bypass actors would be recorded against another ruleset's id.
        """
        detail = json.loads(ruleset_detail_response())
        detail["id"] = RULESET_ID + 1
        snapshot = observe(transport(detail=(200, body(detail))))
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertIn(
            f"{observer.RULESET_DETAIL_UNREADABLE}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")

    def test_required_status_checks_rule_without_parameters_fails_closed(self) -> None:
        """A rule this observer cannot read must not read as a no-op rule.

        `object_field` reads absent and null as an empty mapping, so a
        `required_status_checks` rule with no `parameters` would contribute
        zero contexts while the surface stayed observed and the permission
        could still reach `complete` — a ruleset that requires a check would
        be observed as one that requires none, understating live enforcement
        in both reconciler directions (false MATCH against checked-in policy,
        or false DRIFT on contexts the observer could not read).
        """
        for label, mutate in (
            ("absent", lambda rule: rule.pop("parameters")),
            ("explicit null", lambda rule: rule.__setitem__("parameters", None)),
        ):
            with self.subTest(mutation=label):
                detail = json.loads(ruleset_detail_response())
                mutate(detail["rules"][0])
                snapshot = observe(transport(detail=(200, body(detail))))
                self.assertEqual(snapshot["rulesets"]["items"], [])
                self.assertIn(
                    f"{observer.RULESET_DETAIL_UNREADABLE}:{RULESET_ID}",
                    snapshot["observation"]["limitations"],
                )
                self.assertNotEqual(snapshot["observation"]["permission"], "complete")

    def test_output_cannot_overwrite_input_evidence(self) -> None:
        """An output resolving onto an input must be rejected, not staged.

        The static receipt and, for `assemble`, the capture bundle are fully
        read before any output is staged. An output pointed at the same
        destination would overwrite that input only after it was consumed —
        the run would exit successfully with the receipt replaced by a
        derived artifact, and the next reconciler run would load malformed
        evidence and emit a typed NOT_PROVEN.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            bundle_bytes = json.dumps(capture.to_bundle()).encode("utf-8")
            receipt_bytes = json.dumps(static_receipt()).encode("utf-8")
            bundle.write_bytes(bundle_bytes)
            receipt.write_bytes(receipt_bytes)

            overlaps = (
                # snapshot onto the static receipt
                ["--snapshot", str(receipt)],
                # authority onto the static receipt
                ["--snapshot", str(root / "snapshot.json"),
                 "--authority", str(receipt),
                 "--authority-repository-id", str(REPOSITORY_ID)],
                # capture-bundle output onto the static receipt
                ["--snapshot", str(root / "snapshot.json"),
                 "--capture-bundle", str(receipt)],
            )
            for tail in overlaps:
                code = observer.main(
                    [
                        "assemble",
                        "--capture",
                        str(bundle),
                        "--repository",
                        REPOSITORY,
                        "--source",
                        "connector",
                        "--static-receipt",
                        str(receipt),
                        *tail,
                    ]
                )
                self.assertEqual(code, 1, f"{tail} was accepted")
                self.assertEqual(
                    receipt.read_bytes(),
                    receipt_bytes,
                    f"{tail} overwrote the static receipt",
                )

            # assemble's own bundle input is likewise protected.
            code = observer.main(
                [
                    "assemble",
                    "--capture",
                    str(bundle),
                    "--repository",
                    REPOSITORY,
                    "--source",
                    "connector",
                    "--static-receipt",
                    str(receipt),
                    "--snapshot",
                    str(bundle),
                ]
            )
            self.assertEqual(code, 1, "snapshot onto the bundle input was accepted")
            self.assertEqual(
                bundle.read_bytes(),
                bundle_bytes,
                "assemble overwrote the capture bundle",
            )

    def test_one_ruleset_context_from_two_apps_keeps_both(self) -> None:
        detail = json.loads(ruleset_detail_response())
        detail["rules"][0]["parameters"]["required_status_checks"] = [
            {"context": "Shared", "integration_id": APP_ID},
            {"context": "Shared", "integration_id": 4242},
        ]
        snapshot = observe(transport(detail=(200, body(detail))))
        self.assertEqual(
            snapshot["rulesets"]["items"][0]["required_status_checks"],
            [
                {"context": "Shared", "app_id": 4242},
                {"context": "Shared", "app_id": APP_ID},
            ],
        )
        model.validate_snapshot(snapshot)

    def test_unknown_enforcement_is_unrepresentable_not_invalid(self) -> None:
        """A future enforcement state must not poison the whole snapshot.

        The reconciler's vocabulary is closed, so emitting an unknown value
        would make it reject the entire document as invalid input and destroy
        every other surface's evidence along with it.
        """
        listing = json.loads(ruleset_list_response())
        listing[0]["enforcement"] = "quantum"
        detail = json.loads(ruleset_detail_response())
        detail["enforcement"] = "quantum"
        snapshot = observe(
            transport(rulesets=(200, body(listing)), detail=(200, body(detail)))
        )
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertIn(
            f"{observer.RULESET_DETAIL_UNREPRESENTABLE}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")
        # The rest of the document still validates and reconciles.
        model.validate_snapshot(snapshot)
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")

    def test_unknown_bypass_mode_is_reported_not_emitted(self) -> None:
        detail = json.loads(ruleset_detail_response())
        detail["bypass_actors"][0]["bypass_mode"] = "sometimes"
        snapshot = observe(transport(detail=(200, body(detail))))
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertIn(
            f"{observer.RULESET_DETAIL_UNREADABLE}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        model.validate_snapshot(snapshot)

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

    def test_unreadable_listing_target_or_id_is_not_silently_skipped(self) -> None:
        """A ruleset the observer cannot classify may be a branch ruleset.

        Skipping it as if it were a tag ruleset would hide live enforcement
        behind a listing that still reads as complete; a zero or negative id
        would pass here only to be rejected by the reconciler downstream.
        """
        for label, mutate in (
            ("target absent", lambda item: item.pop("target")),
            ("target null", lambda item: item.__setitem__("target", None)),
            ("target empty", lambda item: item.__setitem__("target", "")),
            ("target unknown", lambda item: item.__setitem__("target", "repo")),
            ("id zero", lambda item: item.__setitem__("id", 0)),
            ("id negative", lambda item: item.__setitem__("id", -RULESET_ID)),
        ):
            listing = json.loads(ruleset_list_response())
            mutate(listing[0])
            with self.subTest(mutation=label):
                snapshot = observe(transport(rulesets=(200, body(listing))))
                self.assertEqual(snapshot["rulesets"]["instrument_state"], "unreadable")
                self.assertEqual(snapshot["rulesets"]["items"], [])
                self.assertIn(
                    observer.RULESET_LIST_UNREADABLE,
                    snapshot["observation"]["limitations"],
                )
                self.assertNotEqual(snapshot["observation"]["permission"], "complete")

    def test_required_status_checks_rule_without_checks_fails_closed(self) -> None:
        """Parameters present but no checks list is unreadable, not empty."""
        for label, mutate in (
            ("absent", lambda params: params.pop("required_status_checks")),
            (
                "explicit null",
                lambda params: params.__setitem__("required_status_checks", None),
            ),
        ):
            detail = json.loads(ruleset_detail_response())
            mutate(detail["rules"][0]["parameters"])
            with self.subTest(mutation=label):
                snapshot = observe(transport(detail=(200, body(detail))))
                self.assertEqual(snapshot["rulesets"]["items"], [])
                self.assertIn(
                    f"{observer.RULESET_DETAIL_UNREADABLE}:{RULESET_ID}",
                    snapshot["observation"]["limitations"],
                )
                self.assertNotEqual(snapshot["observation"]["permission"], "complete")

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
            static_receipt=static_receipt(),
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
            static_receipt=static_receipt(),
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

    def test_one_context_from_two_apps_keeps_both_bindings(self) -> None:
        """Identity is (context, app_id), not context alone.

        Keying by context keeps only the last app, hiding a second, possibly
        conflicting, enforcement binding from reconciliation.
        """
        payload = {
            "required_status_checks": {
                "strict": True,
                "checks": [
                    {"context": "Shared", "app_id": APP_ID},
                    {"context": "Shared", "app_id": 4242},
                ],
            }
        }
        snapshot = observe(transport(classic=(200, body(payload))))
        self.assertEqual(
            snapshot["classic_branch_protection"]["required_status_checks"],
            # sorted by (context, app_id), so the lower app id comes first
            [
                {"context": "Shared", "app_id": 4242},
                {"context": "Shared", "app_id": APP_ID},
            ],
        )
        model.validate_snapshot(snapshot)

    def test_legacy_context_does_not_duplicate_a_richer_check(self) -> None:
        """`contexts` is the legacy view of what `checks` describes richly."""
        payload = {
            "required_status_checks": {
                "strict": True,
                "contexts": ["Classic Required", "Legacy Only"],
                "checks": [{"context": "Classic Required", "app_id": APP_ID}],
            }
        }
        snapshot = observe(transport(classic=(200, body(payload))))
        self.assertEqual(
            snapshot["classic_branch_protection"]["required_status_checks"],
            [
                {"context": "Classic Required", "app_id": APP_ID},
                {"context": "Legacy Only", "app_id": None},
            ],
        )

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

    def test_malformed_static_digests_are_rejected(self) -> None:
        """A SUCCESS receipt can still carry a digest the reconciler rejects.

        Binding it would produce a snapshot that looks complete and that the
        reconciler throws out as invalid input.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        cases = [
            ("subject_sha256", "abc"),
            ("subject_sha256", "Z" * 64),
            ("exact_source_sha256", ""),
        ]
        for field, value in cases:
            receipt = static_receipt()
            receipt[field] = value
            with self.subTest(field=field, value=value[:8]):
                with self.assertRaises(observer.ObserverError):
                    observer.build_snapshot(
                        capture, source="operator", static_receipt=receipt
                    )
        for field, value in (("repository_sha", "a" * 39), ("repository_sha", "g" * 40)):
            receipt = static_receipt()
            receipt["subjects"][field] = value
            with self.subTest(field=field):
                with self.assertRaises(observer.ObserverError):
                    observer.build_snapshot(
                        capture, source="operator", static_receipt=receipt
                    )
        receipt = static_receipt()
        receipt["subjects"]["policy"]["sha256"] = "b" * 63
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture, source="operator", static_receipt=receipt
            )

    def test_a_partial_write_cannot_leave_a_truncated_file(self) -> None:
        """Evidence is staged and renamed, never written in place.

        A truncated JSON file that still parses is worse than no file.
        """
        with tempfile.TemporaryDirectory() as raw:
            target = Path(raw) / "out.json"
            target.write_text('{"previous": true}', encoding="utf-8")
            import os as _os

            original = observer.os.fdopen

            class FailingStream:
                def __init__(self, fd: int) -> None:
                    self._fd = fd

                def __enter__(self) -> "FailingStream":
                    return self

                def __exit__(self, *exc) -> bool:
                    _os.close(self._fd)
                    return False

                def write(self, data: str) -> int:
                    raise OSError("no space left on device")

            observer.os.fdopen = lambda fd, *a, **k: FailingStream(fd)
            try:
                with self.assertRaises(OSError):
                    observer.write_json(target, {"new": True})
            finally:
                observer.os.fdopen = original
            self.assertEqual(
                target.read_text(encoding="utf-8"), '{"previous": true}'
            )
            self.assertEqual(
                list(Path(raw).glob("*.tmp")), [], "staging file left behind"
            )

    def test_each_write_stages_under_a_unique_name(self) -> None:
        """A shared staging path lets one run publish another's payload."""
        staged: list[str] = []
        original = observer.tempfile.mkstemp

        def recording(**kwargs):
            handle, name = original(**kwargs)
            staged.append(name)
            return handle, name

        observer.tempfile.mkstemp = recording
        try:
            with tempfile.TemporaryDirectory() as raw:
                target = Path(raw) / "out.json"
                for index in range(3):
                    observer.write_json(target, {"n": index})
                self.assertEqual(
                    json.loads(target.read_text(encoding="utf-8")), {"n": 2}
                )
                self.assertEqual(list(Path(raw).glob("*.tmp")), [])
        finally:
            observer.tempfile.mkstemp = original
        self.assertEqual(len(set(staged)), 3, f"reused staging name: {staged}")

    def test_non_success_static_receipt_cannot_be_bound(self) -> None:
        receipt = static_receipt()
        receipt["status"] = "FAILURE"
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture,
                source="operator",
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
                static_receipt=static_receipt(),
            )

    def test_observer_cannot_emit_a_fixture_observation(self) -> None:
        """A fixture source would launder synthetic data as live evidence."""
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture,
                source="fixture",
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
            static_receipt=static_receipt(),
        )
        imported = observer.build_snapshot(
            restored,
            source="connector",
            static_receipt=static_receipt(),
        )
        direct["observation"]["source"] = "connector"
        self.assertEqual(direct, imported)

    def valid_bundle(self) -> dict:
        """A bundle the observer itself produced, for one-thing-at-a-time edits.

        Hand-written literals rot: when the capture schema version moved, the
        old literals started failing at the version gate instead of at the
        check under test, and the suite stayed green either way. Deriving each
        case from a real bundle keeps every rejection attributable.
        """
        return observer.capture_live(REPOSITORY, "main", transport()).to_bundle()

    def test_the_unmutated_bundle_is_accepted(self) -> None:
        """Positive control: every rejection below must be the edit's doing."""
        restored = observer.Capture.from_bundle(self.valid_bundle())
        self.assertEqual(restored.repository, REPOSITORY)
        self.assertEqual(restored.branch, "main")

    def test_malformed_bundle_is_rejected(self) -> None:
        def wrong_version(bundle: dict) -> None:
            bundle["schema_version"] = observer.CAPTURE_VERSION + 1

        def entries_not_a_list(bundle: dict) -> None:
            bundle["entries"] = {}

        def entry_missing_fields(bundle: dict) -> None:
            bundle["entries"] = [{"key": "repository"}]

        def body_not_base64(bundle: dict) -> None:
            bundle["entries"][0]["body_base64"] = "!!!"

        for label, mutate in (
            ("wrong schema version", wrong_version),
            ("entries not a list", entries_not_a_list),
            ("entry missing fields", entry_missing_fields),
            ("body not base64", body_not_base64),
        ):
            bundle = self.valid_bundle()
            mutate(bundle)
            with self.subTest(label=label):
                with self.assertRaises(observer.ObserverError):
                    observer.Capture.from_bundle(bundle)

    def test_bundle_body_is_bounded_before_it_is_decoded(self) -> None:
        """Imported bytes get the same bound the live transport enforces.

        `from_bundle` used to decode `body_base64` unconditionally, so a
        connector bundle could carry a body far larger than any response
        the observer itself would have accepted.
        """
        oversized = "A" * (observer.MAX_BODY_BASE64_CHARS + 4)
        for label, value in (
            ("oversized", oversized),
            ("not a string", 12),
        ):
            bundle = self.valid_bundle()
            bundle["entries"][0]["body_base64"] = value
            with self.subTest(label=label):
                with self.assertRaises(observer.ObserverError):
                    observer.Capture.from_bundle(bundle)
        # Positive control: a body exactly at the bound still decodes.
        bundle = self.valid_bundle()
        bundle["entries"][0]["body_base64"] = base64.b64encode(
            b"x" * observer.MAX_RESPONSE_BYTES
        ).decode("ascii")
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
            bundle = self.valid_bundle()
            bundle["entries"][0]["status"] = status
            bundle["entries"][0]["transport_failed"] = failed
            with self.subTest(status=status, transport_failed=failed):
                with self.assertRaises(observer.ObserverError):
                    observer.Capture.from_bundle(bundle)

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
            static_receipt=static_receipt(),
        )
        self.assertEqual(snapshot["rulesets"]["items"], [])
        self.assertIn(
            f"{observer.RULESET_LIST_INCOMPLETE}:{RULESET_ID}",
            snapshot["observation"]["limitations"],
        )
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")

    def test_duplicate_bundle_keys_are_rejected(self) -> None:
        """A real capture records each key once, so two is ambiguous evidence.

        Accepting it would let the last entry silently replace the response
        the observer actually saw.
        """
        bundle = self.valid_bundle()
        bundle["entries"].append(dict(bundle["entries"][0]))
        with self.assertRaises(observer.ObserverError):
            observer.Capture.from_bundle(bundle)

    def test_bundle_transport_failed_must_be_a_boolean(self) -> None:
        """Cases chosen to slip past the status/transport_failed pairing check.

        `1 == True` and `0 == False` in Python, so these agree with the
        pairing rule and are rejected only by the type check itself. A string
        would be caught by the pairing check instead, proving nothing here.
        """
        for status, failed in ((None, 1), (200, 0)):
            bundle = self.valid_bundle()
            bundle["entries"][0]["status"] = status
            bundle["entries"][0]["transport_failed"] = failed
            with self.subTest(status=status, transport_failed=failed):
                with self.assertRaises(observer.ObserverError):
                    observer.Capture.from_bundle(bundle)


class AcquisitionBinding(unittest.TestCase):
    """A bundle carries when and where it was taken, not when it was imported."""

    def test_imported_capture_keeps_its_acquisition_time(self) -> None:
        """An old bundle must not become fresh evidence by being assembled.

        Minting `observed_at` at import time would defeat the authority's
        staleness bound entirely: any stale capture could produce a current
        live verdict.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        bundle = capture.to_bundle()
        bundle["captured_at"] = "2020-01-01T00:00:00Z"
        restored = observer.Capture.from_bundle(bundle)
        snapshot = observer.build_snapshot(
            restored, source="connector", static_receipt=static_receipt()
        )
        self.assertEqual(
            snapshot["repository"]["observed_at"], "2020-01-01T00:00:00Z"
        )
        result = model.reconcile(snapshot, static_receipt(), authority(snapshot))
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("observation_stale", codes(result))

    def test_capture_time_is_stamped_before_the_requests(self) -> None:
        stamps = []

        def recording(path: str):
            stamps.append(observer.utc_now())
            return transport()(path)

        capture = observer.capture_live(REPOSITORY, "main", recording)
        self.assertTrue(capture.captured_at <= stamps[0])

    def test_a_bundle_cannot_be_relabelled_onto_another_branch(self) -> None:
        """Two branches at the same SHA would otherwise reconcile silently."""
        capture = observer.capture_live(REPOSITORY, "main", transport())
        bundle = capture.to_bundle()
        bundle["branch"] = "release"
        restored = observer.Capture.from_bundle(bundle)
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                restored, source="connector", static_receipt=static_receipt()
            )

    def test_snapshot_branch_comes_from_the_capture(self) -> None:
        capture = observer.capture_live(REPOSITORY, "release", transport())
        # The fixture only answers for main, so a release capture cannot bind.
        with self.assertRaises(observer.ObserverError):
            observer.build_snapshot(
                capture, source="operator", static_receipt=static_receipt()
            )

    def test_bundle_without_acquisition_identity_fails_closed(self) -> None:
        capture = observer.capture_live(REPOSITORY, "main", transport())
        for missing in ("repository", "branch", "captured_at"):
            bundle = capture.to_bundle()
            del bundle[missing]
            with self.assertRaises(observer.ObserverError):
                observer.Capture.from_bundle(bundle)

    def test_an_imported_capture_cannot_claim_a_higher_trust_source(
        self,
    ) -> None:
        """An import is an import, whatever the command line says.

        `trusted_default_branch` asserts the evidence came from a
        repository-owned job on the default branch, and `operator` that
        someone ran the requests themselves. A bundle is bytes on disk: the
        observer cannot tell where it came from, so it may only ever claim
        `connector`, which carries the trust of whoever imported it. Without
        this, the source ladder's top rung would be reachable by passing a
        different flag — the same relabelling defect as re-dating a stale
        bundle or moving it onto another branch.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        restored = observer.Capture.from_bundle(capture.to_bundle())
        for claimed in ("trusted_default_branch", "operator"):
            with self.assertRaises(observer.ObserverError):
                observer.build_snapshot(
                    restored, source=claimed, static_receipt=static_receipt()
                )
        # The honest label still works.
        snapshot = observer.build_snapshot(
            restored, source="connector", static_receipt=static_receipt()
        )
        self.assertEqual(snapshot["observation"]["source"], "connector")

    def test_a_live_capture_is_not_forced_to_claim_connector(self) -> None:
        """Negative control: the restriction must bind imports only."""
        capture = observer.capture_live(REPOSITORY, "main", transport())
        snapshot = observer.build_snapshot(
            capture, source="trusted_default_branch", static_receipt=static_receipt()
        )
        self.assertEqual(
            snapshot["observation"]["source"], "trusted_default_branch"
        )

    def test_build_snapshot_has_no_acquisition_time_override(self) -> None:
        """A caller override would reopen the staleness hole it closed.

        The acquisition time is a property of the capture. An argument that
        could replace it would let any caller present an old capture as fresh.
        """
        import inspect

        parameters = inspect.signature(observer.build_snapshot).parameters
        self.assertNotIn("observed_at", parameters)
        self.assertNotIn("branch", parameters)

    def test_bundle_captured_at_must_be_a_real_timestamp(self) -> None:
        capture = observer.capture_live(REPOSITORY, "main", transport())
        for stamp in ("not-a-time", "2026-08-16T00:00:00"):
            bundle = capture.to_bundle()
            bundle["captured_at"] = stamp
            with self.assertRaises(observer.ObserverError):
                observer.Capture.from_bundle(bundle)


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

    def test_mixed_capture_generations_are_refused_by_the_consumer(self) -> None:
        """A snapshot and authority from different runs cannot reconcile.

        Staging every payload before any rename shrinks the window, but a
        multi-file rename is not one transaction: a failure between renames
        could leave one destination new and another old. The observer cannot
        close that on POSIX, so this pins the property that actually protects
        the consumer — the reconciler's own freshness bounds refuse either
        mixed pairing, because the authority states its evaluation time
        independently of the observation.

        The bound is real but not unlimited: two runs closer together than
        the skew allowance are indistinguishable, which is why this asserts a
        window rather than absolute detection.
        """
        older = observe(observed_at="2026-08-16T00:00:00Z")
        newer = observe(observed_at="2026-08-16T09:00:00Z")

        # New snapshot left beside an authority from the earlier run.
        stale_authority = authority(older, evaluated_at="2026-08-16T00:05:00Z")
        result = model.reconcile(newer, static_receipt(), stale_authority)
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("observation_from_future", codes(result))

        # And the opposite order: old snapshot, authority from the later run.
        fresh_authority = authority(newer, evaluated_at="2026-08-16T09:05:00Z")
        result = model.reconcile(older, static_receipt(), fresh_authority)
        self.assertEqual(result["status"], "NOT_PROVEN")
        self.assertIn("observation_stale", codes(result))

        # Negative control: one coherent generation still reconciles.
        matched = authority(newer, evaluated_at="2026-08-16T09:05:00Z")
        self.assertIn(
            model.reconcile(newer, static_receipt(), matched)["status"],
            {"MATCH", "DRIFT"},
        )


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
                    "connector",
                    "--static-receipt",
                    str(receipt),
                    "--snapshot",
                    str(root / "snapshot.json"),
                ]
            )
            self.assertEqual(code, 2)

    def test_assemble_does_not_offer_a_higher_trust_source(self) -> None:
        """The command line refuses the label before any work is done.

        `build_snapshot` already refuses it, so this pins the outer seam
        independently: removing the narrowing would otherwise leave the suite
        green and the refusal would arrive only after the bundle had been
        read and parsed.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            snapshot = root / "snapshot.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            receipt.write_text(json.dumps(static_receipt()), encoding="utf-8")
            argv = [
                "assemble",
                "--capture",
                str(bundle),
                "--repository",
                REPOSITORY,
                "--source",
                "trusted_default_branch",
                "--static-receipt",
                str(receipt),
                "--snapshot",
                str(snapshot),
            ]
            with self.assertRaises(SystemExit) as raised:
                observer.main(argv)
            self.assertEqual(raised.exception.code, 2)
            self.assertFalse(snapshot.exists())
            # Negative control: `capture` still accepts the same label, so the
            # narrowing binds the import path and not the source list itself.
            self.assertIn(
                "trusted_default_branch", observer.LIVE_SOURCES
            )

    def test_a_failed_write_does_not_publish_a_mismatched_output_set(
        self,
    ) -> None:
        """Validation is not the only way a run can end up half-published.

        Building every payload first stops an *invalid* authority reaching
        disk, but an I/O failure partway through the writes would still leave
        a fresh snapshot beside an authority from an earlier run — the
        incoherent pair that check exists to prevent. Every payload is
        therefore staged before any destination is replaced.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            snapshot = root / "snapshot.json"
            auth = root / "authority.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            receipt.write_text(json.dumps(static_receipt()), encoding="utf-8")
            # Evidence from an earlier run that must not be paired with a
            # fresh snapshot.
            snapshot.write_text('{"run": "earlier"}', encoding="utf-8")
            auth.write_text('{"run": "earlier"}', encoding="utf-8")

            real_stage = observer.stage_json
            calls = []

            def failing_stage(path, payload):
                calls.append(path)
                # Fail while staging the second output, after the first has
                # already been staged successfully.
                if len(calls) == 2:
                    raise OSError(28, "No space left on device")
                return real_stage(path, payload)

            observer.stage_json = failing_stage
            try:
                with self.assertRaises(OSError):
                    observer.main(
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
                            "--static-receipt",
                            str(receipt),
                            "--snapshot",
                            str(snapshot),
                            "--authority",
                            str(auth),
                        ]
                    )
            finally:
                observer.stage_json = real_stage

            # Neither destination was replaced, so the pair on disk is still
            # the coherent earlier one rather than one new and one old file.
            self.assertEqual(
                json.loads(snapshot.read_text(encoding="utf-8")),
                {"run": "earlier"},
            )
            self.assertEqual(
                json.loads(auth.read_text(encoding="utf-8")),
                {"run": "earlier"},
            )
            # And no staging file was left behind.
            self.assertEqual(
                sorted(p.name for p in root.iterdir()),
                ["authority.json", "capture.json", "snapshot.json", "static.json"],
            )

    def test_outputs_cannot_share_a_destination(self) -> None:
        """Two payloads writing one path silently lose one of them.

        Each output is staged and replaced in turn, so a shared destination
        ends up holding whichever payload was written last while the run still
        exits successfully. A caller who asked for a snapshot can be handed an
        authority instead, with nothing saying so.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            shared = root / "out.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            receipt.write_text(json.dumps(static_receipt()), encoding="utf-8")
            (root / "sub").mkdir()

            collisions = (
                ["--snapshot", str(shared), "--authority", str(shared),
                 "--authority-repository-id", str(REPOSITORY_ID)],
                ["--snapshot", str(shared), "--capture-bundle", str(shared)],
                # A spelling pathlib does not normalise away: `..` is kept in
                # the Path (it can change meaning through a symlink), so only
                # resolving the real path catches this collision.
                ["--snapshot", str(shared), "--capture-bundle",
                 str(root / "sub" / ".." / "out.json")],
            )
            for tail in collisions:
                code = observer.main(
                    [
                        "assemble",
                        "--capture",
                        str(bundle),
                        "--repository",
                        REPOSITORY,
                        "--source",
                        "connector",
                        "--static-receipt",
                        str(receipt),
                        *tail,
                    ]
                )
                self.assertEqual(code, 1, f"{tail} was accepted")
                self.assertFalse(
                    shared.exists(), f"{tail} wrote a colliding output"
                )

    def test_a_long_destination_name_can_still_be_staged(self) -> None:
        """`mkstemp` appends to the prefix it is given.

        An unbounded prefix makes the staging name exceed NAME_MAX for a
        destination whose own name is near the limit, so a perfectly valid
        output path would produce no evidence file at all.
        """
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            target = root / ("o" * 250 + ".json")
            observer.write_json(target, {"written": True})
            self.assertEqual(
                json.loads(target.read_text(encoding="utf-8")), {"written": True}
            )
            self.assertEqual([p.name for p in root.iterdir()], [target.name])

    def test_no_output_is_written_when_the_authority_is_invalid(self) -> None:
        """A partial write pairs a fresh snapshot with a stale authority.

        That pair looks coherent to a consumer and is not, so nothing is
        written until every requested payload has been built and validated.
        """
        capture = observer.capture_live(REPOSITORY, "main", transport())
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            bundle = root / "capture.json"
            receipt = root / "static.json"
            snapshot = root / "snapshot.json"
            auth = root / "authority.json"
            bundle.write_text(json.dumps(capture.to_bundle()), encoding="utf-8")
            receipt.write_text(json.dumps(static_receipt()), encoding="utf-8")
            auth.write_text('{"stale": true}', encoding="utf-8")
            code = observer.main(
                [
                    "assemble",
                    "--capture",
                    str(bundle),
                    "--repository",
                    REPOSITORY,
                    "--authority-repository-id",
                    "0",  # rejected: must be a positive integer
                    "--source",
                    "connector",
                    "--static-receipt",
                    str(receipt),
                    "--snapshot",
                    str(snapshot),
                    "--authority",
                    str(auth),
                ]
            )
            self.assertEqual(code, 1)
            self.assertFalse(snapshot.exists(), "snapshot written despite failure")
            self.assertEqual(
                auth.read_text(encoding="utf-8"), '{"stale": true}',
                "pre-existing authority was overwritten",
            )

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
                    "connector",
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

        def read(self, size: int | None = None) -> bytes:
            return self._payload if size is None else self._payload[:size]

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
        forbidden = b'{"message":"Forbidden"}'
        # The observer reads error bodies under the same size bound as a
        # success, so the fake must accept the requested size.
        error.read = lambda size=-1: forbidden[:size] if size >= 0 else forbidden
        result, _ = self.run_with(error)
        self.assertEqual(result.status, 403)
        self.assertTrue(result.forbidden)
        self.assertFalse(result.ok)

    def test_an_oversized_response_is_not_read_into_memory(self) -> None:
        """An unbounded read lets one response terminate the observer."""
        oversized = b"x" * (observer.MAX_RESPONSE_BYTES + 10)
        result, _ = self.run_with(self.FakeResponse(200, oversized))
        self.assertTrue(result.transport_failed)
        self.assertEqual(result.body, b"")
        self.assertFalse(result.ok)

    def test_a_response_at_the_bound_is_still_read(self) -> None:
        payload = b"x" * observer.MAX_RESPONSE_BYTES
        result, _ = self.run_with(self.FakeResponse(200, payload))
        self.assertFalse(result.transport_failed)
        self.assertEqual(len(result.body), observer.MAX_RESPONSE_BYTES)

    def test_an_oversized_error_body_is_also_bounded(self) -> None:
        """The bound must cover the error path, not only the success path.

        A non-2xx response is read to classify the surface, so an unbounded
        read there terminates the observer just as surely — and an error
        status is exactly where a misbehaving host is most likely to send
        something enormous.
        """
        import urllib.error

        error = urllib.error.HTTPError(
            "https://api.github.com/repos/o/r", 500, "Server Error", {}, None
        )
        oversized = b"x" * (observer.MAX_RESPONSE_BYTES + 10)
        error.read = lambda size=-1: oversized[:size] if size >= 0 else oversized
        result, _ = self.run_with(error)
        self.assertTrue(result.transport_failed)
        self.assertEqual(result.body, b"")

    def test_an_error_body_at_the_bound_is_still_retained(self) -> None:
        """Negative control: the bound must not discard ordinary error text."""
        import urllib.error

        error = urllib.error.HTTPError(
            "https://api.github.com/repos/o/r", 403, "Forbidden", {}, None
        )
        payload = b'{"message":"Forbidden"}'
        error.read = lambda size=-1: payload[:size] if size >= 0 else payload
        result, _ = self.run_with(error)
        self.assertEqual(result.status, 403)
        self.assertTrue(result.forbidden)
        self.assertEqual(result.body, payload)

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

    def test_falsy_rule_parameters_do_not_read_as_an_empty_rule(self) -> None:
        """`parameters: false` must not silently contribute zero checks.

        `x or {}` turns every falsy value into an empty mapping, which makes
        the `isinstance` guard behind it dead code. The rule then contributes
        no contexts and no limitation, so a malformed ruleset reaches the
        reconciler as a *complete* observation that is simply missing required
        contexts — a false DRIFT built out of evidence nobody could read.
        """
        for malformed in (False, 0, [], ""):
            detail = json.loads(ruleset_detail_response())
            detail["rules"][0]["parameters"] = malformed
            snapshot = observe(transport(detail=(200, body(detail))))
            # Refused as an unreadable detail, never silently empty.
            self.assertIn(
                f"ruleset_detail_unreadable:{RULESET_ID}",
                snapshot["observation"]["limitations"],
                f"{malformed!r} was accepted as an empty parameter set",
            )
            self.assertNotEqual(snapshot["observation"]["permission"], "complete")

    def test_identity_must_satisfy_the_reconcilers_own_contract(self) -> None:
        """A snapshot the reconciler calls invalid must never be written.

        The reconciler requires `owner/name` exactly, a positive repository
        id, and a hexadecimal branch SHA. Emitting anything else produces a
        document it rejects as malformed input — which destroys every other
        surface's evidence with it, rather than reporting one unreadable
        surface. The observer must refuse at capture time instead.
        """
        for full_name in ("owner/name/extra", "/name", "owner/", "/"):
            payload = json.loads(repository_response())
            payload["full_name"] = full_name
            with self.assertRaises(observer.ObserverError, msg=full_name):
                observe(self.with_repository(payload))

        for repository_id in (0, -1):
            payload = json.loads(repository_response())
            payload["id"] = repository_id
            with self.assertRaises(observer.ObserverError, msg=str(repository_id)):
                observe(self.with_repository(payload))

        # 40 characters, but not a commit SHA.
        head = json.loads(branch_head_response())
        head["object"]["sha"] = "z" * 40
        broken = transport()
        broken.responses[f"repos/{REPOSITORY}/git/ref/heads/main"] = (
            200,
            body(head),
        )
        with self.assertRaises(observer.ObserverError):
            observe(broken)

    @staticmethod
    def with_repository(payload: dict) -> "RecordingTransport":
        which = transport()
        which.responses[f"repos/{REPOSITORY}"] = (200, body(payload))
        return which

    def test_non_object_containers_raise_typed_errors(self) -> None:
        """A truthy non-object must not reach `.get` and raise AttributeError.

        These paths fail closed either way, but an untyped `AttributeError`
        escapes the observer's error contract instead of becoming a limitation
        or a typed refusal.
        """
        head = json.loads(branch_head_response())
        head["object"] = "not-an-object"
        broken = transport()
        broken.responses[f"repos/{REPOSITORY}/git/ref/heads/main"] = (
            200,
            body(head),
        )
        with self.assertRaises(observer.ObserverError):
            observe(broken)

        detail = json.loads(ruleset_detail_response())
        detail["conditions"] = "not-an-object"
        snapshot = observe(transport(detail=(200, body(detail))))
        self.assertNotEqual(snapshot["observation"]["permission"], "complete")

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

    def test_authority_refuses_values_the_reconciler_would_reject(self) -> None:
        """Fail at the point the operator supplied the value, not downstream."""
        base = {
            "producer": "p",
            "declared_repository": REPOSITORY,
            "declared_repository_id": REPOSITORY_ID,
            "declared_branch": "main",
            "max_age_seconds": 3600,
            "max_future_skew_seconds": 300,
        }
        for override in (
            {"producer": ""},
            {"producer": "   "},
            {"declared_branch": ""},
            {"max_age_seconds": 0},
            {"max_age_seconds": -1},
            {"max_age_seconds": True},
            {"max_future_skew_seconds": 0},
            {"max_future_skew_seconds": False},
            {"evaluated_at": "not-a-time"},
            {"evaluated_at": "2026-08-16T00:00:00"},
        ):
            with self.assertRaises(observer.ObserverError):
                observer.build_authority(**{**base, **override})

    def test_authority_normalizes_a_non_utc_evaluation_time(self) -> None:
        built = observer.build_authority(
            producer="p",
            declared_repository=REPOSITORY,
            declared_repository_id=REPOSITORY_ID,
            declared_branch="main",
            max_age_seconds=3600,
            max_future_skew_seconds=300,
            evaluated_at="2026-08-16T00:05:00-04:00",
        )
        self.assertEqual(built["evaluated_at"], "2026-08-16T04:05:00Z")

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
                    "connector",
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


class LoadJsonTests(unittest.TestCase):
    def test_unreadable_input_is_a_typed_observer_error(self) -> None:
        """Any OSError on an input becomes a typed failure, not a traceback."""
        with tempfile.TemporaryDirectory() as raw:
            with self.assertRaises(observer.ObserverError):
                observer.load_json(Path(raw), "static receipt")


if __name__ == "__main__":
    unittest.main()
