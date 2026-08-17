#!/usr/bin/env python3
"""Materialize the reviewed #9152 repair on the stacked static contract."""

from __future__ import annotations

from pathlib import Path

MODEL = Path("scripts/ci/reconcile_github_enforcement_snapshot.py")
TESTS = Path("scripts/ci/test_reconcile_github_enforcement_snapshot.py")
DOCS = Path("docs/ci/github-enforcement-snapshot.md")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: marker matched {count} times")
    return text.replace(old, new, 1)


def replace_block(text: str, start: str, end: str, new: str, label: str) -> str:
    start_index = text.find(start)
    if start_index < 0:
        raise SystemExit(f"{label}: start marker missing")
    end_index = text.find(end, start_index)
    if end_index < 0:
        raise SystemExit(f"{label}: end marker missing")
    return text[:start_index] + new.rstrip() + "\n\n" + text[end_index:]


def patch_model() -> None:
    text = MODEL.read_text(encoding="utf-8")
    text = text.replace("GITHUB_ACTIONS_APP_ID = 15368\n", "AUTHORITY_VERSION = 1\n")
    text = replace_once(
        text,
        '''        {"subject_sha256", "policy_sha256", "repository_sha"},
''',
        '''        {
            "subject_sha256",
            "exact_source_sha256",
            "policy_sha256",
            "repository_sha",
        },
''',
        "snapshot static fields",
    )
    text = replace_once(
        text,
        '''            "policy_sha256": hex_digest(
                static["policy_sha256"],
                "snapshot.static_contract.policy_sha256",
                64,
            ),
            "repository_sha": hex_digest(
''',
        '''            "exact_source_sha256": hex_digest(
                static["exact_source_sha256"],
                "snapshot.static_contract.exact_source_sha256",
                64,
            ),
            "policy_sha256": hex_digest(
                static["policy_sha256"],
                "snapshot.static_contract.policy_sha256",
                64,
            ),
            "repository_sha": hex_digest(
''',
        "snapshot static normalization",
    )

    validate_static = r'''
def validate_static(raw: Any) -> dict[str, Any]:
    raw = obj(raw, "static_receipt")
    for field in (
        "schema_version",
        "status",
        "subject_sha256",
        "exact_source_sha256",
        "subjects",
    ):
        if field not in raw:
            raise ContractError(f"static_receipt missing {field}")
    if raw["schema_version"] != STATIC_VERSION:
        raise ContractError(
            f"static_receipt.schema_version must be {STATIC_VERSION}"
        )
    status = text(raw["status"], "static_receipt.status")
    if status != "SUCCESS":
        subject = raw["subject_sha256"]
        if subject is not None:
            subject = hex_digest(subject, "static_receipt.subject_sha256", 64)
        return {"status": status, "subject_sha256": subject}

    subjects = obj(raw["subjects"], "static_receipt.subjects")
    for field in ("repository_sha", "policy", "contexts"):
        if field not in subjects:
            raise ContractError(f"static_receipt.subjects missing {field}")
    policy = obj(subjects["policy"], "static_receipt.subjects.policy")
    if "sha256" not in policy:
        raise ContractError("static_receipt.subjects.policy missing sha256")

    contexts: list[dict[str, Any]] = []
    names: set[str] = set()
    for index, entry in enumerate(
        seq(subjects["contexts"], "static_receipt.subjects.contexts")
    ):
        field = f"static_receipt.subjects.contexts[{index}]"
        entry = obj(entry, field)
        for required in ("name", "policy_role", "enforcement"):
            if required not in entry:
                raise ContractError(f"{field} missing {required}")
        name = text(entry["name"], f"{field}.name")
        if name in names:
            raise ContractError(f"duplicate static context: {name}")
        names.add(name)
        row = {
            "name": name,
            "policy_role": text(entry["policy_role"], f"{field}.policy_role"),
            "enforcement": text(entry["enforcement"], f"{field}.enforcement"),
        }
        producer = entry.get("producer")
        if producer is not None:
            row["producer"] = text(producer, f"{field}.producer")
        if "classic_app_id" in entry:
            row["classic_app_id"] = positive_int(
                entry["classic_app_id"], f"{field}.classic_app_id"
            )
        if "ruleset_integration_id" in entry:
            row["ruleset_integration_id"] = positive_int(
                entry["ruleset_integration_id"],
                f"{field}.ruleset_integration_id",
            )
        contexts.append(row)
    contexts.sort(key=lambda row: row["name"])
    return {
        "status": status,
        "subject_sha256": hex_digest(
            raw["subject_sha256"], "static_receipt.subject_sha256", 64
        ),
        "exact_source_sha256": hex_digest(
            raw["exact_source_sha256"],
            "static_receipt.exact_source_sha256",
            64,
        ),
        "repository_sha": hex_digest(
            subjects["repository_sha"],
            "static_receipt.subjects.repository_sha",
            40,
        ),
        "policy_sha256": hex_digest(
            policy["sha256"],
            "static_receipt.subjects.policy.sha256",
            64,
        ),
        "contexts": contexts,
    }
'''
    text = replace_block(text, "def validate_static(", "def app_sort(", validate_static, "validate_static")

    live_union = r'''
def live_union(
    snapshot: dict[str, Any],
) -> tuple[
    list[dict[str, Any]],
    list[dict[str, Any]],
    list[dict[str, Any]],
]:
    rows: dict[str, dict[str, list[dict[str, Any]]]] = {}
    excluded: list[dict[str, Any]] = []
    limitations: list[dict[str, Any]] = []

    def add_classic(item: dict[str, Any]) -> None:
        row = rows.setdefault(item["context"], {"classic": [], "ruleset": []})
        observation = {"app_id": item["app_id"]}
        if observation in row["classic"]:
            raise ContractError(
                f"duplicate classic context/app observation: {item['context']!r}"
            )
        row["classic"].append(observation)

    def add_ruleset(item: dict[str, Any], ruleset_id: int) -> None:
        row = rows.setdefault(item["context"], {"classic": [], "ruleset": []})
        observation = {
            "ruleset_id": ruleset_id,
            "integration_id": item["app_id"],
        }
        if observation in row["ruleset"]:
            raise ContractError(
                "duplicate ruleset/context/integration observation: "
                f"{ruleset_id}:{item['context']}"
            )
        row["ruleset"].append(observation)

    for item in snapshot["classic_branch_protection"]["required_status_checks"]:
        add_classic(item)

    for ruleset in snapshot["rulesets"]["items"]:
        targeting = ruleset["targeting"]["status"]
        if ruleset["enforcement"] != "active":
            excluded.append(
                {
                    "id": ruleset["id"],
                    "name": ruleset["name"],
                    "reason": "inactive",
                    "enforcement": ruleset["enforcement"],
                    "targeting": targeting,
                }
            )
            continue
        if targeting == "NOT_TARGETED":
            excluded.append(
                {
                    "id": ruleset["id"],
                    "name": ruleset["name"],
                    "reason": "untargeted",
                    "enforcement": ruleset["enforcement"],
                    "targeting": targeting,
                }
            )
            continue
        if targeting == "NOT_PROVEN":
            limitations.append(
                {
                    "code": "ruleset_targeting_not_proven",
                    "message": (
                        f"ruleset {ruleset['id']} ({ruleset['name']}) has "
                        "unsupported or ambiguous default-branch selectors"
                    ),
                }
            )
            continue
        for item in ruleset["required_status_checks"]:
            add_ruleset(item, ruleset["id"])

    union: list[dict[str, Any]] = []
    for context in sorted(rows):
        classic = sorted(rows[context]["classic"], key=lambda row: app_sort(row["app_id"]))
        ruleset = sorted(
            rows[context]["ruleset"],
            key=lambda row: (row["ruleset_id"], app_sort(row["integration_id"])),
        )
        sources = [source for source, values in (("classic", classic), ("ruleset", ruleset)) if values]
        source_bindings: list[dict[str, Any]] = []
        if classic:
            source_bindings.append({"source": "classic", "observations": classic})
        if ruleset:
            source_bindings.append({"source": "ruleset", "observations": ruleset})
        all_app_ids = {row["app_id"] for row in classic}
        all_app_ids.update(row["integration_id"] for row in ruleset)
        union.append(
            {
                "context": context,
                "app_ids": sorted(all_app_ids, key=app_sort),
                "sources": sources,
                "source_class": "both" if sources == ["classic", "ruleset"] else sources[0],
                "source_bindings": source_bindings,
            }
        )
    return (
        union,
        sorted(excluded, key=lambda row: row["id"]),
        sorted(limitations, key=lambda row: (row["code"], row["message"])),
    )
'''
    text = replace_block(text, "def live_union(", "def invalid_receipt(", live_union, "live_union")

    authority_and_reconcile = r'''
def validate_authority(raw: Any) -> dict[str, Any]:
    raw = obj(raw, "reconciliation_authority")
    closed(
        raw,
        "reconciliation_authority",
        {
            "schema_version",
            "producer",
            "repository",
            "evaluated_at",
            "max_observation_age_seconds",
            "max_future_skew_seconds",
        },
    )
    if raw["schema_version"] != AUTHORITY_VERSION:
        raise ContractError(
            f"reconciliation_authority.schema_version must be {AUTHORITY_VERSION}"
        )
    repository = obj(raw["repository"], "reconciliation_authority.repository")
    closed(
        repository,
        "reconciliation_authority.repository",
        {"full_name", "repository_id", "default_branch"},
    )
    return {
        "schema_version": AUTHORITY_VERSION,
        "producer": text(raw["producer"], "reconciliation_authority.producer"),
        "repository": {
            "full_name": text(
                repository["full_name"],
                "reconciliation_authority.repository.full_name",
            ),
            "repository_id": positive_int(
                repository["repository_id"],
                "reconciliation_authority.repository.repository_id",
            ),
            "default_branch": text(
                repository["default_branch"],
                "reconciliation_authority.repository.default_branch",
            ),
        },
        "evaluated_at": timestamp(
            raw["evaluated_at"], "reconciliation_authority.evaluated_at"
        ),
        "max_observation_age_seconds": positive_int(
            raw["max_observation_age_seconds"],
            "reconciliation_authority.max_observation_age_seconds",
        ),
        "max_future_skew_seconds": positive_int(
            raw["max_future_skew_seconds"],
            "reconciliation_authority.max_future_skew_seconds",
        ),
    }


def parsed_timestamp(value: str) -> datetime:
    return datetime.fromisoformat(value[:-1] + "+00:00" if value.endswith("Z") else value)


def reconcile(snapshot_raw: Any, static_raw: Any, authority_raw: Any) -> dict[str, Any]:
    try:
        snapshot = validate_snapshot(snapshot_raw)
        static = validate_static(static_raw)
        authority = validate_authority(authority_raw)
        union, excluded, union_limitations = live_union(snapshot)
    except ContractError as error:
        return invalid_receipt(str(error))

    base = {
        "schema_version": RECEIPT_VERSION,
        "repository": snapshot["repository"],
        "observation": snapshot["observation"],
        "reconciliation_authority": authority,
        "reconciliation_authority_sha256": digest(authority),
        "snapshot_sha256": digest(snapshot),
        "static_contract_subject_sha256": static.get("subject_sha256"),
        "surface_states": {
            "classic_branch_protection": snapshot["classic_branch_protection"]["instrument_state"],
            "rulesets": snapshot["rulesets"]["instrument_state"],
        },
        "evidence_digests": {
            "classic_branch_protection": snapshot["classic_branch_protection"]["response_sha256"],
            "ruleset_list": snapshot["rulesets"]["list_response_sha256"],
            "ruleset_details": [
                {"id": ruleset["id"], "sha256": ruleset["detail_response_sha256"]}
                for ruleset in snapshot["rulesets"]["items"]
            ],
        },
        "ruleset_inventory": snapshot["rulesets"]["items"],
    }
    limitations: list[dict[str, Any]] = list(union_limitations)

    expected_repository = authority["repository"]
    for code, observed, expected in (
        ("repository_name_mismatch", snapshot["repository"]["full_name"], expected_repository["full_name"]),
        ("repository_id_mismatch", snapshot["repository"]["repository_id"], expected_repository["repository_id"]),
        ("default_branch_mismatch", snapshot["repository"]["default_branch"], expected_repository["default_branch"]),
    ):
        if observed != expected:
            limitations.append({"code": code, "message": f"observed={observed}, expected={expected}"})

    observed_at = parsed_timestamp(snapshot["repository"]["observed_at"])
    evaluated_at = parsed_timestamp(authority["evaluated_at"])
    age_seconds = (evaluated_at - observed_at).total_seconds()
    if age_seconds > authority["max_observation_age_seconds"]:
        limitations.append(
            {
                "code": "observation_stale",
                "message": (
                    f"age_seconds={int(age_seconds)}, maximum="
                    f"{authority['max_observation_age_seconds']}"
                ),
            }
        )
    if age_seconds < -authority["max_future_skew_seconds"]:
        limitations.append(
            {
                "code": "observation_from_future",
                "message": (
                    f"future_seconds={int(-age_seconds)}, maximum="
                    f"{authority['max_future_skew_seconds']}"
                ),
            }
        )

    if static["status"] != "SUCCESS":
        limitations.append(
            {
                "code": "static_contract_not_success",
                "message": f"static contract status is {static['status']}",
            }
        )
    else:
        for code, observed, expected in (
            ("static_subject_mismatch", snapshot["static_contract"]["subject_sha256"], static["subject_sha256"]),
            ("exact_source_mismatch", snapshot["static_contract"]["exact_source_sha256"], static["exact_source_sha256"]),
            ("policy_digest_mismatch", snapshot["static_contract"]["policy_sha256"], static["policy_sha256"]),
            ("static_repository_sha_mismatch", snapshot["static_contract"]["repository_sha"], static["repository_sha"]),
            ("branch_sha_mismatch", snapshot["repository"]["branch_sha"], static["repository_sha"]),
        ):
            if observed != expected:
                limitations.append({"code": code, "message": f"observed={observed}, expected={expected}"})

    if snapshot["classic_branch_protection"]["branch"] != snapshot["repository"]["default_branch"]:
        limitations.append(
            {"code": "classic_branch_mismatch", "message": "classic protection targets another branch"}
        )
    for surface, state in base["surface_states"].items():
        if state != "observed":
            limitations.append(
                {"code": f"{surface}_not_observed", "message": f"{surface} instrument state is {state}"}
            )
    if snapshot["observation"]["permission"] != "complete":
        limitations.append(
            {
                "code": "observation_permission_incomplete",
                "message": f"permission is {snapshot['observation']['permission']}",
            }
        )
    if snapshot["observation"]["source"] not in LIVE_OBSERVATION_SOURCES:
        limitations.append(
            {
                "code": "non_live_observation_source",
                "message": f"source {snapshot['observation']['source']} cannot establish current live enforcement",
            }
        )
    limitations.extend(
        {"code": "capture_limitation", "message": message}
        for message in snapshot["observation"]["limitations"]
    )
    if limitations:
        return {
            **base,
            "status": "NOT_PROVEN",
            "live_union": union,
            "excluded_rulesets": excluded,
            "differences": [],
            "limitations": sorted(limitations, key=lambda row: (row["code"], row["message"])),
        }

    expected: dict[str, dict[str, Any]] = {}
    static_by_name = {row["name"]: row for row in static["contexts"]}
    differences: list[dict[str, Any]] = []
    for row in static["contexts"]:
        if row["policy_role"] != "required":
            continue
        if row["enforcement"] not in ENFORCEMENT_SOURCES:
            differences.append(
                {
                    "code": "required_policy_has_unsupported_enforcement",
                    "context": row["name"],
                    "expected": row["enforcement"],
                    "observed": None,
                }
            )
            continue
        expected[row["name"]] = row

    observed = {row["context"]: row for row in union}
    for name in sorted(expected):
        wanted = expected[name]
        actual = observed.get(name)
        wanted_sources = ENFORCEMENT_SOURCES[wanted["enforcement"]]
        if actual is None:
            differences.append(
                {
                    "code": "policy_context_missing_live",
                    "context": name,
                    "expected": sorted(wanted_sources),
                    "observed": [],
                }
            )
            continue
        actual_sources = frozenset(actual["sources"])
        if actual_sources != wanted_sources:
            differences.append(
                {
                    "code": "enforcement_source_mismatch",
                    "context": name,
                    "expected": sorted(wanted_sources),
                    "observed": sorted(actual_sources),
                }
            )
        bindings = {binding["source"]: binding["observations"] for binding in actual["source_bindings"]}
        if "classic_app_id" in wanted:
            observed_classic = [row["app_id"] for row in bindings.get("classic", [])]
            if not observed_classic or any(value != wanted["classic_app_id"] for value in observed_classic):
                differences.append(
                    {
                        "code": "classic_app_identity_mismatch",
                        "context": name,
                        "expected": wanted["classic_app_id"],
                        "observed": observed_classic,
                    }
                )
        if "ruleset_integration_id" in wanted:
            observed_rulesets = bindings.get("ruleset", [])
            mismatches = [
                row for row in observed_rulesets
                if row["integration_id"] != wanted["ruleset_integration_id"]
            ]
            if not observed_rulesets or mismatches:
                differences.append(
                    {
                        "code": "ruleset_integration_identity_mismatch",
                        "context": name,
                        "expected": wanted["ruleset_integration_id"],
                        "observed": mismatches if observed_rulesets else [],
                    }
                )

    for name in sorted(set(observed) - set(expected)):
        static_row = static_by_name.get(name)
        if static_row is not None and static_row["policy_role"] == "required":
            continue
        if static_row is None:
            code = "live_context_missing_from_policy"
            expected_value: Any = None
        else:
            code = "live_context_role_mismatch"
            expected_value = static_row["policy_role"]
        differences.append(
            {
                "code": code,
                "context": name,
                "expected": expected_value,
                "observed": observed[name]["sources"],
            }
        )
    differences.sort(key=lambda row: (row["context"], row["code"]))
    return {
        **base,
        "status": "DRIFT" if differences else "MATCH",
        "live_union": union,
        "excluded_rulesets": excluded,
        "differences": differences,
        "limitations": [],
    }
'''
    text = replace_block(text, "def reconcile(", "def read_json(", authority_and_reconcile, "reconcile")

    text = replace_once(
        text,
        '''    compare.add_argument("--static-receipt", type=Path, required=True)
    compare.add_argument("--receipt", type=Path)
''',
        '''    compare.add_argument("--static-receipt", type=Path, required=True)
    compare.add_argument("--authority", type=Path, required=True)
    compare.add_argument("--receipt", type=Path)
''',
        "cli authority argument",
    )
    text = replace_once(
        text,
        '''            static_raw = read_json(args.static_receipt)
            result = reconcile(snapshot_raw, static_raw)
''',
        '''            static_raw = read_json(args.static_receipt)
            authority_raw = read_json(args.authority)
            result = reconcile(snapshot_raw, static_raw, authority_raw)
''',
        "cli authority read",
    )
    MODEL.write_text(text, encoding="utf-8")


def patch_tests() -> None:
    text = TESTS.read_text(encoding="utf-8")
    text = replace_once(
        text,
        'SUBJECT = "c" * 64\nCLASSIC_DIGEST = "d" * 64\n',
        'SUBJECT = "c" * 64\nEXACT_SOURCE = "9" * 64\nCLASSIC_DIGEST = "d" * 64\n',
        "test constants",
    )
    text = replace_once(
        text,
        '''        "subject_sha256": SUBJECT,
        "subjects": {
''',
        '''        "subject_sha256": SUBJECT,
        "exact_source_sha256": EXACT_SOURCE,
        "subjects": {
''',
        "static exact source",
    )
    text = text.replace(
        '''                    "producer": "repository-job",
                },
                {
                    "name": "Ruleset Required",
''',
        '''                    "producer": "repository-job",
                    "classic_app_id": 15368,
                },
                {
                    "name": "Ruleset Required",
''',
        1,
    )
    text = text.replace(
        '''                    "producer": "repository-job",
                },
                {
                    "name": "Both Required",
''',
        '''                    "producer": "repository-job",
                    "ruleset_integration_id": 15368,
                },
                {
                    "name": "Both Required",
''',
        1,
    )
    text = text.replace(
        '''                    "producer": "repository-job",
                },
                {
                    "name": "Advisory",
''',
        '''                    "producer": "repository-job",
                    "classic_app_id": 15368,
                    "ruleset_integration_id": 15368,
                },
                {
                    "name": "Advisory",
''',
        1,
    )
    text = replace_once(
        text,
        '''            "subject_sha256": SUBJECT,
            "policy_sha256": POLICY,
            "repository_sha": SHA,
''',
        '''            "subject_sha256": SUBJECT,
            "exact_source_sha256": EXACT_SOURCE,
            "policy_sha256": POLICY,
            "repository_sha": SHA,
''',
        "snapshot exact source",
    )
    helper = '''

def authority() -> dict:
    return {
        "schema_version": 1,
        "producer": "trusted-default-branch-observer",
        "repository": {
            "full_name": "EffortlessMetrics/perl-lsp-swarm",
            "repository_id": 1244101844,
            "default_branch": "main",
        },
        "evaluated_at": "2026-08-16T00:05:00Z",
        "max_observation_age_seconds": 3600,
        "max_future_skew_seconds": 300,
    }


def reconcile(candidate: dict, static: dict | None = None, auth: dict | None = None) -> dict:
    return model.reconcile(
        candidate,
        static_receipt() if static is None else static,
        authority() if auth is None else auth,
    )
'''
    text = replace_once(text, "\n\nclass EnforcementSnapshotTests", helper + "\n\nclass EnforcementSnapshotTests", "test helper")
    text = text.replace("model.reconcile(", "reconcile(")
    text = text.replace("return reconcile(\n        candidate,", "return model.reconcile(\n        candidate,", 1)
    text = text.replace(
        '''                {
                    "source": "classic",
                    "app_ids": [15368],
                    "ruleset_ids": [],
                },
                {
                    "source": "ruleset",
                    "app_ids": [15368],
                    "ruleset_ids": [16664791],
                },
''',
        '''                {
                    "source": "classic",
                    "observations": [{"app_id": 15368}],
                },
                {
                    "source": "ruleset",
                    "observations": [
                        {"ruleset_id": 16664791, "integration_id": 15368}
                    ],
                },
''',
    )
    text = text.replace('if row["code"] == "app_identity_mismatch"', 'if row["code"] == "classic_app_identity_mismatch"')
    text = text.replace('mismatch["observed"], {"classic": [None]}', 'mismatch["observed"], [None]')

    extra = '''
    def test_unbound_ruleset_does_not_inherit_classic_binding(self) -> None:
        static = static_receipt()
        row = next(row for row in static["subjects"]["contexts"] if row["name"] == "Both Required")
        row.pop("ruleset_integration_id")
        candidate = snapshot()
        candidate["rulesets"]["items"][0]["required_status_checks"][1]["app_id"] = None
        receipt = reconcile(candidate, static)
        self.assertEqual("MATCH", receipt["status"])

    def test_ruleset_binding_is_checked_for_every_contributing_ruleset(self) -> None:
        candidate = snapshot()
        candidate["rulesets"]["items"].append(
            ruleset(99, check("Ruleset Required", 4242))
        )
        receipt = reconcile(candidate)
        self.assertEqual("DRIFT", receipt["status"])
        finding = next(
            row for row in receipt["differences"]
            if row["code"] == "ruleset_integration_identity_mismatch"
        )
        self.assertEqual(99, finding["observed"][0]["ruleset_id"])

    def test_cross_repository_snapshot_is_not_proven(self) -> None:
        candidate = snapshot()
        candidate["repository"]["full_name"] = "Other/Repository"
        candidate["repository"]["repository_id"] = 42
        receipt = reconcile(candidate)
        self.assertEqual("NOT_PROVEN", receipt["status"])
        codes = {row["code"] for row in receipt["limitations"]}
        self.assertIn("repository_name_mismatch", codes)
        self.assertIn("repository_id_mismatch", codes)

    def test_missing_reconciliation_authority_is_not_proven(self) -> None:
        receipt = model.reconcile(snapshot(), static_receipt(), None)
        self.assertEqual("NOT_PROVEN", receipt["status"])
        self.assertIn("reconciliation_authority", receipt["limitations"][0]["message"])

    def test_stale_and_future_observations_are_not_proven(self) -> None:
        for observed_at, code in (
            ("2026-08-15T20:00:00Z", "observation_stale"),
            ("2026-08-16T00:20:00Z", "observation_from_future"),
        ):
            with self.subTest(code=code):
                candidate = snapshot()
                candidate["repository"]["observed_at"] = observed_at
                receipt = reconcile(candidate)
                self.assertEqual("NOT_PROVEN", receipt["status"])
                self.assertIn(code, {row["code"] for row in receipt["limitations"]})

'''
    text = replace_once(text, '\n\nif __name__ == "__main__":\n    unittest.main()\n', extra + '\nif __name__ == "__main__":\n    unittest.main()\n', "extra tests")
    text = replace_once(
        text,
        '''            static_path = root / "static.json"
            receipt_path = root / "receipt.json"
''',
        '''            static_path = root / "static.json"
            authority_path = root / "authority.json"
            receipt_path = root / "receipt.json"
''',
        "cli test authority path",
    )
    text = replace_once(
        text,
        '''            static_path.write_text(
                json.dumps(static_receipt()), encoding="utf-8"
            )
            status = model.main(
''',
        '''            static_path.write_text(
                json.dumps(static_receipt()), encoding="utf-8"
            )
            authority_path.write_text(
                json.dumps(authority()), encoding="utf-8"
            )
            status = model.main(
''',
        "cli test authority write",
    )
    text = replace_once(
        text,
        '''                    "--static-receipt",
                    str(static_path),
                    "--receipt",
''',
        '''                    "--static-receipt",
                    str(static_path),
                    "--authority",
                    str(authority_path),
                    "--receipt",
''',
        "cli test authority arg",
    )
    TESTS.write_text(text, encoding="utf-8")


def patch_docs() -> None:
    text = DOCS.read_text(encoding="utf-8")
    appendix = '''

## Reconciliation authority

`reconcile` requires a third, closed offline input that independently states the expected repository full name, numeric repository ID, default branch, evaluation time, maximum observation age, and future-clock-skew allowance. Snapshot self-report cannot authenticate itself. Missing authority, repository mismatch, stale observation, or implausibly future observation yields `NOT_PROVEN`.

## Source-specific bindings

The static contract supplies independent optional `classic_app_id` and `ruleset_integration_id` values. Producer identity supplies neither. Classic observations remain `{context, app_id}`; ruleset observations remain paired `{ruleset_id, context, integration_id}`. When a binding is declared, every contributing observation for that source must match it. When it is absent, the observed value remains receipt-visible but creates no inferred binding verdict.
'''
    if "## Reconciliation authority" not in text:
        text = text.rstrip() + appendix + "\n"
    DOCS.write_text(text, encoding="utf-8")


def main() -> None:
    patch_model()
    patch_tests()
    patch_docs()


if __name__ == "__main__":
    main()
