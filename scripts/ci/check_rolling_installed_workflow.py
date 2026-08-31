#!/usr/bin/env python3
"""Static recurrence checks for the rolling installed observation workflow."""

from __future__ import annotations

import pathlib
import sys

WORKFLOW = pathlib.Path(".github/workflows/rolling-installed-public-beta-observation.yml")
REQUIRED_ROWS = ("linux-minimum", "linux-current", "windows-current")
FORBIDDEN_MUTATIONS = (
    "cargo publish",
    "npm publish",
    "vsce publish",
    "ovsx publish",
    "docker push",
    "gh release create",
    "gh release upload",
    "git push",
)


class WorkflowError(RuntimeError):
    """The workflow no longer preserves the exact rolling-evidence boundary."""


def require(text: str, token: str, message: str) -> None:
    if token not in text:
        raise WorkflowError(message)


def validate(text: str) -> None:
    require(text, "permissions:\n  contents: read", "workflow must remain read-only")
    require(text, "cancel-in-progress: false", "rolling runs must not cancel in-progress evidence")
    require(
        text,
        "if: github.event_name == 'workflow_dispatch'\n    needs: contract",
        "heavy execution must remain manual and downstream of contract tests",
    )
    require(
        text,
        "Require current default-branch tip and resolve identities",
        "workflow must pin current protected-main identity",
    )
    require(
        text,
        'test "$(git rev-parse HEAD)" = "$EXPECTED_SHA"',
        "platform rows must verify their exact checkout SHA",
    )
    require(
        text,
        "cargo build --locked --release -p perllsp --bin perllsp -p perl-dap --bin perl-dap",
        "workflow must build release-profile server and DAP subjects",
    )
    require(
        text,
        "rolling_installed_observation.py package",
        "workflow must create the release-shaped product-unit artifact",
    )
    require(
        text,
        "npm run test:published:local",
        "workflow must execute the existing packaged VSIX journey",
    )
    require(
        text,
        "PERL_LSP_FIRST_HOUR_SERVER_PATH",
        "packaged journey must receive the exact built server path",
    )
    require(
        text,
        "PERL_LSP_SERVER_SOURCE_SHA",
        "packaged journey must bind the server to exact source identity",
    )
    require(
        text,
        "rolling_installed_observation.py row",
        "each platform must produce one typed row",
    )
    require(
        text,
        "if: always()\n        shell: bash",
        "row assembly must run after failed smoke execution",
    )
    require(
        text,
        "rolling_installed_observation.py fan-in",
        "workflow must produce the canonical pre-freeze fan-in packet",
    )
    require(
        text,
        "pre_freeze_public_beta_acceptance.json",
        "workflow must retain the named pre-freeze packet",
    )

    for row_id in REQUIRED_ROWS:
        require(text, f"row_id: {row_id}", f"workflow misses required row {row_id}")
    if text.count("row_id: linux-") != 2:
        raise WorkflowError("Linux minimum and current rows must remain separate")
    if text.count("row_id: windows-current") != 1:
        raise WorkflowError("Windows current row must exist exactly once")

    lowered = text.lower()
    for forbidden in FORBIDDEN_MUTATIONS:
        if forbidden in lowered:
            raise WorkflowError(f"rolling observation gained public mutation command: {forbidden}")


def replace_once(text: str, old: str, new: str, mutation: str) -> str:
    count = text.count(old)
    if count != 1:
        raise WorkflowError(
            f"negative-control setup {mutation} expected one {old!r}, found {count}"
        )
    return text.replace(old, new, 1)


def expect_failure(text: str, mutation: str) -> None:
    if mutation == "drop_windows":
        mutated = replace_once(text, "row_id: windows-current", "row_id: windows-dropped", mutation)
    elif mutation == "drop_exact_checkout":
        mutated = replace_once(
            text,
            'test "$(git rev-parse HEAD)" = "$EXPECTED_SHA"',
            "echo unchecked",
            mutation,
        )
    elif mutation == "workspace_build":
        mutated = replace_once(
            text,
            "cargo build --locked --release -p perllsp --bin perllsp -p perl-dap --bin perl-dap",
            "cargo build -p perllsp --bin perllsp",
            mutation,
        )
    elif mutation == "drop_server_identity":
        mutated = replace_once(
            text,
            "PERL_LSP_SERVER_SOURCE_SHA",
            "REMOVED_SERVER_SOURCE_SHA",
            mutation,
        )
    elif mutation == "add_publish":
        mutated = text + "\n# cargo publish\n"
    elif mutation == "cancel_running":
        mutated = replace_once(text, "cancel-in-progress: false", "cancel-in-progress: true", mutation)
    else:
        raise WorkflowError(f"unknown negative-control mutation {mutation}")

    try:
        validate(mutated)
    except WorkflowError:
        return
    raise WorkflowError(f"negative control did not fail: {mutation}")


def main() -> int:
    try:
        text = WORKFLOW.read_text(encoding="utf-8")
        validate(text)
        for mutation in (
            "drop_windows",
            "drop_exact_checkout",
            "workspace_build",
            "drop_server_identity",
            "add_publish",
            "cancel_running",
        ):
            expect_failure(text, mutation)
    except (OSError, WorkflowError) as error:
        print(f"rolling installed workflow check failed: {error}", file=sys.stderr)
        return 1

    print(
        "rolling installed workflow: exact main, distinct Linux/Windows rows, "
        "packaged VSIX execution, durable fan-in, and no-public-mutation boundary verified"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
