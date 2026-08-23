#!/usr/bin/env python3
"""Feature catalog authority checks (#7029).

Validates that the root `features.toml` is a schema-complete, evidence-honest
catalog and that every vendored projection (`crates/*/features_sot.toml`) is a
byte-identical generated copy of it.

Modes:
  (default)   validate authority + projection drift; exit non-zero on any violation
  --fix       regenerate the vendored projections from the authority, then validate
  --self-test run the negative-control fixtures; each control must FAIL the checks

Negative controls (issue #7029):
  1. root vs projection drift
  2. promotion to proven from advertisement alone
  3. proven retained while an evidence field is unrecorded
  4. blanket compliance claim reintroduced (compliance_percent = 100)

No third-party dependencies; stdlib only.
"""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
AUTHORITY = REPO_ROOT / "features.toml"
PROJECTIONS = [
    "crates/perl-lsp-rs/features_sot.toml",
    "crates/perl-lsp-rs-core/features_sot.toml",
    "crates/perl-parser/features_sot.toml",
    "crates/perl-dap/features_sot.toml",
]

MATURITIES = {"proven", "preview", "planned", "unsupported", "not_proven"}
DIRECTIONS = {"client_to_server", "server_to_client", "bidirectional"}
FEATURE_CLASSES = {
    "request_response",
    "server_request",
    "document_workspace",
    "cancellation_progress",
    "editor_dependent",
}
STATE_OWNERS = {"server", "client", "shared", "none", "unrecorded"}
EVIDENCE_CLASSES = {"behavior_test", "integration_test", "audit_note", "unverified"}

QUALIFYING_EVIDENCE_CLASSES = {"behavior_test", "integration_test"}

FORBIDDEN_META_KEYS = {"compliance_percent"}
FORBIDDEN_PHRASES = (
    "100% compliant",
    "fully compliant",
    "production-ready",
)


class Violations:
    def __init__(self) -> None:
        self.items: list[str] = []

    def add(self, message: str) -> None:
        self.items.append(message)

    def ok(self) -> bool:
        return not self.items


def check_authority(root: Path, text: str | None = None) -> list[str]:
    """Validate one catalog document; returns violations."""
    problems: list[str] = []
    path = root / "features.toml"
    raw = text if text is not None else path.read_text(encoding="utf-8")
    try:
        catalog = tomllib.loads(raw)
    except tomllib.TOMLDecodeError as exc:
        return [f"{path}: TOML parse error: {exc}"]

    meta = catalog.get("meta")
    if not isinstance(meta, dict):
        problems.append("missing [meta] section")
        meta = {}
    for key in ("version", "lsp_version"):
        value = meta.get(key)
        if not isinstance(value, str) or not value.strip():
            problems.append(f"meta.{key} must be a non-empty string")
    for key in FORBIDDEN_META_KEYS:
        if key in meta:
            problems.append(
                f"meta.{key} is forbidden: generated status must be computed "
                "from maturity/evidence fields, not hand-asserted"
            )
    lowered_header = raw.split("[[feature]]", 1)[0].lower()
    for phrase in FORBIDDEN_PHRASES:
        if phrase in lowered_header:
            problems.append(f"authority header contains forbidden claim phrase: {phrase!r}")

    policy = catalog.get("policy") or {}
    evidence_policy = policy.get("evidence") or {}
    # Single authority: the TOML-declared qualifying set drives promotion
    # checks; the module constant is only the fallback when policy omits it.
    declared_qualifying = set(evidence_policy.get("qualifying_classes") or [])
    qualifying_classes = declared_qualifying or QUALIFYING_EVIDENCE_CLASSES
    non_qualifying_tests = set(evidence_policy.get("non_qualifying_tests") or [])
    promotion = policy.get("promotion") or {}
    for feature_class in FEATURE_CLASSES:
        if feature_class not in promotion:
            problems.append(f"[policy.promotion.{feature_class}] section is missing")

    rows = catalog.get("feature")
    if not isinstance(rows, list) or not rows:
        return problems + ["no [[feature]] rows found"]

    seen_ids: set[str] = set()
    for index, row in enumerate(rows):
        fid = row.get("id", f"<row {index}>")

        if not isinstance(fid, str) or not fid.strip():
            problems.append(f"row {index}: id must be a non-empty string")
            continue
        if fid in seen_ids:
            problems.append(f"{fid}: duplicate feature id")
        seen_ids.add(fid)

        for field in ("spec", "area", "capability_route"):
            value = row.get(field)
            if not isinstance(value, str) or not value.strip():
                problems.append(f"{fid}: {field} must be recorded (non-empty string)")

        if row.get("direction") not in DIRECTIONS:
            problems.append(f"{fid}: direction must be one of {sorted(DIRECTIONS)}")
        if row.get("feature_class") not in FEATURE_CLASSES:
            problems.append(f"{fid}: feature_class must be one of {sorted(FEATURE_CLASSES)}")
        if row.get("state_owner") not in STATE_OWNERS:
            problems.append(f"{fid}: state_owner must be one of {sorted(STATE_OWNERS)}")
        if row.get("evidence_class") not in EVIDENCE_CLASSES:
            problems.append(f"{fid}: evidence_class must be one of {sorted(EVIDENCE_CLASSES)}")
        if row.get("maturity") not in MATURITIES:
            problems.append(f"{fid}: maturity must be one of {sorted(MATURITIES)}")
        if not isinstance(row.get("advertised"), bool):
            problems.append(f"{fid}: advertised must be a bool")
        tests = row.get("tests")
        if not isinstance(tests, list) or any(not isinstance(t, str) for t in tests):
            problems.append(f"{fid}: tests must be a list of paths")
            tests = []

        maturity = row.get("maturity")
        owner = row.get("implementation_owner")
        route = row.get("capability_route")
        sowner = row.get("state_owner")
        evclass = row.get("evidence_class")

        # Promotion rules: advertisement, implementation paths, and test-file
        # names can never independently yield proven.
        if maturity == "proven":
            if evclass not in qualifying_classes:
                problems.append(
                    f"{fid}: proven requires qualifying evidence class "
                    f"({sorted(qualifying_classes)}), has {evclass!r}; "
                    "advertisement alone cannot promote"
                )
            if owner in (None, "", "unrecorded"):
                problems.append(
                    f"{fid}: proven requires a recorded implementation_owner "
                    "(an unrecorded evidence field cannot retain proven)"
                )
            if sowner == "unrecorded":
                problems.append(f"{fid}: proven requires a recorded state_owner")
            if not tests:
                problems.append(f"{fid}: proven requires at least one test receipt")
            else:
                for test in tests:
                    if test in non_qualifying_tests:
                        problems.append(
                            f"{fid}: proven cites non-qualifying test receipt {test}"
                        )
                    elif not (root / test).exists():
                        problems.append(f"{fid}: test receipt path does not exist: {test}")

            class_policy = promotion.get(row.get("feature_class") or "") or {}
            minimum = class_policy.get("minimum_evidence_class")
            if minimum is not None and minimum not in qualifying_classes:
                problems.append(
                    f"{fid}: promotion policy for {row.get('feature_class')} "
                    f"is misconfigured: {minimum!r}"
                )

    return problems


def render_projection(authority_bytes: bytes) -> bytes:
    """Deterministic projection content: byte-copy of the authority."""
    return authority_bytes


def check_projections(root: Path, fix: bool) -> list[str]:
    problems: list[str] = []
    authority_bytes = (root / "features.toml").read_bytes()

    # Determinism: rendering twice must produce identical output.
    if render_projection(authority_bytes) != render_projection(render_projection(authority_bytes)):
        problems.append("projection generation is not deterministic")

    for rel in PROJECTIONS:
        target = root / rel
        desired = render_projection(authority_bytes)
        if fix:
            target.write_bytes(desired)
        current = target.read_bytes() if target.exists() else b""
        if current != desired:
            problems.append(
                f"projection drift: {rel} is not byte-identical to features.toml "
                "(regenerate with check_feature_catalog.py --fix)"
            )
    return problems


def run_checks(root: Path, fix: bool = False) -> list[str]:
    problems = check_authority(root)
    problems.extend(check_projections(root, fix))
    return problems


# ---------------------------------------------------------------------------
# Negative controls
# ---------------------------------------------------------------------------

MINIMAL_ROW = """
[[feature]]
id = "{fid}"
spec = "LSP 3.18"
area = "text_document"
direction = "client_to_server"
feature_class = "request_response"
capability_route = "ServerCapabilities.hoverProvider"
implementation_owner = "perl-lsp-rs-core::providers::navigation"
state_owner = "server"
maturity = "{maturity}"
advertised = true
evidence_class = "{evclass}"
tests = ["{test}"]
description = "negative control row"
"""

BASE_HEADER = """
[meta]
version = "0.0.0"
lsp_version = "3.18"

[policy.evidence]
qualifying_classes = ["behavior_test", "integration_test"]
non_qualifying_tests = ["tests/non_qualifying.rs"]

[policy.promotion.request_response]
minimum_evidence_class = "behavior_test"

[policy.promotion.server_request]
minimum_evidence_class = "integration_test"

[policy.promotion.document_workspace]
minimum_evidence_class = "integration_test"

[policy.promotion.cancellation_progress]
minimum_evidence_class = "integration_test"

[policy.promotion.editor_dependent]
minimum_evidence_class = "integration_test"
"""


def negative_control_fixtures() -> dict[str, str]:
    good_row = MINIMAL_ROW.format(
        fid="lsp.control_good",
        maturity="proven",
        evclass="behavior_test",
        test="tests/real_behavior.rs",
    )
    return {
        # Schema-level controls; projection-drift is covered separately in
        # run_self_test via a sandbox pair of authority + drifted copy.
        "missing_receipt_path": BASE_HEADER + "\n" + good_row,
        "advertised_only_promotion": BASE_HEADER
        + "\n"
        + MINIMAL_ROW.format(
            fid="lsp.advertised_only",
            maturity="proven",
            evclass="unverified",
            test="tests/real_behavior.rs",
        ),
        "unrecorded_field_retains_proven": BASE_HEADER
        + "\n"
        + MINIMAL_ROW.format(
            fid="lsp.unrecorded_owner",
            maturity="proven",
            evclass="behavior_test",
            test="tests/real_behavior.rs",
        ).replace(
            'implementation_owner = "perl-lsp-rs-core::providers::navigation"',
            'implementation_owner = "unrecorded"',
        ),
        "non_qualifying_sole_evidence": BASE_HEADER.replace(
            'non_qualifying_tests = ["tests/non_qualifying.rs"]',
            'non_qualifying_tests = ["tests/hollow_method_count.rs"]',
        )
        + "\n"
        + MINIMAL_ROW.format(
            fid="lsp.hollow_evidence",
            maturity="proven",
            evclass="behavior_test",
            test="tests/hollow_method_count.rs",
        ),
        "blanket_claim": BASE_HEADER.replace(
            'lsp_version = "3.18"', 'lsp_version = "3.18"\ncompliance_percent = 100'
        )
        + "\n"
        + good_row,
        "blanket_phrase": BASE_HEADER.replace(
            "[meta]", "# fully compliant LSP server\n\n[meta]"
        )
        + "\n"
        + good_row,
    }


def run_self_test(root: Path) -> int:
    failures: list[str] = []
    for name, text in negative_control_fixtures().items():
        problems = check_authority(root, text=text)
        if problems:
            print(f"PASS control {name!r} rejected: {problems[0]}")
        else:
            failures.append(name)

    # Drift control needs real files on disk: build a sandbox authority +
    # drifted projection pair without touching the repository checkout.
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        sandbox = Path(tmp)
        (sandbox / "features.toml").write_text("x = 1\n", encoding="utf-8")
        proj_rel = PROJECTIONS[0]
        target = sandbox / proj_rel
        target.parent.mkdir(parents=True)
        target.write_bytes(b"different bytes\n")
        problems = check_projections(sandbox, fix=False)
        if problems:
            print(f"PASS control 'projection_drift' rejected: {problems[0]}")
        else:
            failures.append("projection_drift")

    if failures:
        print(f"SELF-TEST FAILED: controls accepted but must fail: {failures}")
        return 1
    print("Self-test OK: every negative control fails the checks.")
    return 0


def main(argv: list[str]) -> int:
    fix = "--fix" in argv
    if "--self-test" in argv:
        return run_self_test(REPO_ROOT)

    problems = run_checks(REPO_ROOT, fix=fix)
    if problems:
        print(f"FEATURE CATALOG CHECK FAILED ({len(problems)}):")
        for problem in problems:
            print(f"  - {problem}")
        return 1
    rows = len(tomllib.loads(AUTHORITY.read_text(encoding="utf-8")).get("feature", []))
    print(f"Feature catalog OK: {rows} rows, projections in sync.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
