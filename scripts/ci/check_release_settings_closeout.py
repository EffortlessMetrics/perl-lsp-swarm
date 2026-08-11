#!/usr/bin/env python3
"""Validate and project the live GitHub release-settings closeout packet."""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from datetime import date
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_RECEIPT = ROOT / ".ci/security/release-settings-closeout.json"
DEFAULT_MARKDOWN = ROOT / "docs/security/release-settings-closeout.md"

FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
ISSUE = re.compile(r"^#\d+$")
EVIDENCE = re.compile(r"^(https://github\.com/.+|#\d+|[0-9a-f]{40})$")
STATES = {"proven", "not_proven", "failed"}
DISPOSITIONS = {"required", "deferred", "not_applicable", "not_proven"}
PRIMARY_CHANNELS = {"github_release", "crates_io", "vscode_marketplace", "open_vsx"}
CONDITIONAL_CHANNELS = {"containers", "homebrew", "windows_metadata"}
CHANNEL_ORDER = [
    "github_release",
    "crates_io",
    "vscode_marketplace",
    "open_vsx",
    "containers",
    "homebrew",
    "windows_metadata",
]
REQUIRED_CODEOWNER_SURFACES = {
    "release_workflows",
    "release_policy",
    "release_notes",
    "release_history",
    "topology_and_freeze",
    "publication_scripts",
}


class ReceiptError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ReceiptError(message)


def load(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise ReceiptError(f"receipt not found: {path}") from exc
    except json.JSONDecodeError as exc:
        raise ReceiptError(f"invalid JSON in {path}: {exc}") from exc
    require(isinstance(value, dict), "receipt root must be an object")
    return value


def validate_refs(refs: Any, field: str, *, required: bool) -> None:
    require(isinstance(refs, list), f"{field} must be an array")
    require(
        all(isinstance(item, str) and EVIDENCE.fullmatch(item) for item in refs),
        f"{field} contains an unsupported evidence reference",
    )
    require(refs == sorted(set(refs)), f"{field} must be sorted and unique")
    if required:
        require(bool(refs), f"{field} requires live evidence")
        require(
            any(item.startswith(REPOSITORY_EVIDENCE_PREFIX) for item in refs),
            f"{field} requires a durable URL for this repository, not only source, issue, or unrelated repository identifiers",
        )


def validate_state_block(block: Any, name: str) -> str:
    require(isinstance(block, dict), f"{name} must be an object")
    state = block.get("state")
    require(state in STATES, f"{name}.state must be one of {sorted(STATES)}")
    limitation = block.get("limitation")
    if state == "proven":
        require(limitation is None, f"{name}: proven control cannot retain a limitation")
    else:
        require(
            isinstance(limitation, str) and limitation.strip(),
            f"{name}: non-proven control requires a limitation",
        )
    validate_refs(
        block.get("evidence_refs"),
        f"{name}.evidence_refs",
        required=state == "proven",
    )
    return state


def validate_subject(data: dict[str, Any]) -> dict[str, Any]:
    subject = data.get("subject")
    require(isinstance(subject, dict), "subject must be an object")
    require(
        subject.get("repository") == "EffortlessMetrics/perl-lsp-swarm",
        "subject.repository must name this repository",
    )
    require(
        isinstance(subject.get("source_sha"), str)
        and FULL_SHA.fullmatch(subject["source_sha"]),
        "subject.source_sha must be a full lowercase SHA",
    )
    topology = subject.get("topology_digest")
    require(
        topology is None or (isinstance(topology, str) and DIGEST.fullmatch(topology)),
        "subject.topology_digest must be null or sha256:<64 hex>",
    )
    try:
        date.fromisoformat(subject.get("observed_at"))
    except (TypeError, ValueError) as exc:
        raise ReceiptError("subject.observed_at must be YYYY-MM-DD") from exc
    require(
        isinstance(subject.get("observer"), str) and subject["observer"].strip(),
        "subject.observer is required",
    )
    return subject


def validate_channels(data: dict[str, Any]) -> dict[str, str]:
    rows = data.get("channel_dispositions")
    require(isinstance(rows, list), "channel_dispositions must be an array")
    found: dict[str, str] = {}
    expected = PRIMARY_CHANNELS | CONDITIONAL_CHANNELS
    for index, row in enumerate(rows):
        require(isinstance(row, dict), f"channel_dispositions[{index}] must be an object")
        channel = row.get("channel")
        require(channel in expected, f"unsupported channel {channel!r}")
        require(channel not in found, f"duplicate channel disposition {channel}")
        disposition = row.get("disposition")
        require(
            disposition in DISPOSITIONS,
            f"{channel}: disposition must be one of {sorted(DISPOSITIONS)}",
        )
        owner = row.get("owner")
        require(
            isinstance(owner, str) and ISSUE.fullmatch(owner),
            f"{channel}: owner must be an issue reference",
        )
        limitation = row.get("limitation")
        if disposition == "required":
            require(
                limitation is None,
                f"{channel}: required channel cannot retain a limitation",
            )
        else:
            require(
                isinstance(limitation, str) and limitation.strip(),
                f"{channel}: non-required disposition requires a limitation",
            )
        found[channel] = disposition
    require(
        set(found) == expected,
        f"channel dispositions must cover exactly {sorted(expected)}",
    )
    require(
        [row["channel"] for row in rows] == CHANNEL_ORDER,
        "channel dispositions must use canonical order",
    )
    require(
        all(found[channel] == "required" for channel in PRIMARY_CHANNELS),
        "all primary release channels must remain required",
    )
    return found


def validate_environments(settings: dict[str, Any], channels: dict[str, str]) -> list[str]:
    rows = settings.get("environments")
    require(isinstance(rows, list), "settings.environments must be an array")
    required_channels = {
        name for name, disposition in channels.items() if disposition == "required"
    }
    found: dict[str, str] = {}
    environment_names: set[str] = set()
    for index, row in enumerate(rows):
        name = f"settings.environments[{index}]"
        state = validate_state_block(row, name)
        channel = row.get("channel")
        require(
            channel in required_channels,
            f"{name}: environment is not for a required channel",
        )
        require(channel not in found, f"duplicate environment for {channel}")
        environment_name = row.get("name")
        require(
            isinstance(environment_name, str) and environment_name.strip(),
            f"{name}.name is required",
        )
        require(
            environment_name not in environment_names,
            f"duplicate environment name {environment_name}",
        )
        environment_names.add(environment_name)
        reviewers = row.get("required_reviewers")
        require(
            isinstance(reviewers, list)
            and all(isinstance(item, str) and item.strip() for item in reviewers),
            f"{name}.required_reviewers must be an array of identities",
        )
        require(
            reviewers == sorted(set(reviewers)),
            f"{name}.required_reviewers must be sorted and unique",
        )
        publication_jobs = row.get("publication_jobs")
        require(
            isinstance(publication_jobs, list)
            and all(isinstance(item, str) and item.strip() for item in publication_jobs),
            f"{name}.publication_jobs must be an array of job identities",
        )
        require(
            publication_jobs == sorted(set(publication_jobs)),
            f"{name}.publication_jobs must be sorted and unique",
        )
        branch_policy = row.get("deployment_branch_policy")
        require(
            branch_policy is None
            or branch_policy in {"protected_branches", "selected_branches"},
            f"{name}.deployment_branch_policy is invalid",
        )
        require(
            isinstance(row.get("secret_scope_verified"), bool),
            f"{name}.secret_scope_verified must be boolean",
        )
        if state == "proven":
            require(
                bool(reviewers),
                f"{name}: proven environment requires a human reviewer",
            )
            require(
                branch_policy is not None,
                f"{name}: proven environment requires a deployment branch policy",
            )
            require(
                row["secret_scope_verified"],
                f"{name}: proven environment requires verified secret scope",
            )
            require(
                publication_jobs,
                f"{name}: proven environment requires a publication job binding",
            )
        found[channel] = state
    require(
        set(found) == required_channels,
        f"environments must cover exactly required channels {sorted(required_channels)}",
    )
    expected_order = [channel for channel in CHANNEL_ORDER if channel in required_channels]
    require(
        [row["channel"] for row in rows] == expected_order,
        "environments must use canonical required-channel order",
    )
    return list(found.values())


def compute_state(data: dict[str, Any]) -> str:
    subject = data["subject"]
    channels = {
        row["channel"]: row["disposition"] for row in data["channel_dispositions"]
    }
    settings = data["settings"]
    states = [
        settings[name]["state"]
        for name in (
            "immutable_releases",
            "tag_ruleset",
            "branch_ruleset",
            "actions_policy",
            "codeowners",
        )
    ]
    states.extend(row["state"] for row in settings["environments"])
    if "failed" in states:
        return "failed"
    unresolved_channels = any(value == "not_proven" for value in channels.values())
    if (
        subject["topology_digest"] is None
        or unresolved_channels
        or any(state != "proven" for state in states)
    ):
        return "not_proven"
    return "proven"


def validate(data: dict[str, Any]) -> str:
    require(data.get("schema_version") == 1, "schema_version must be 1")
    subject = validate_subject(data)
    channels = validate_channels(data)
    settings = data.get("settings")
    require(isinstance(settings, dict), "settings must be an object")

    immutable = settings.get("immutable_releases")
    immutable_state = validate_state_block(immutable, "settings.immutable_releases")
    require(
        immutable.get("enabled") is None or isinstance(immutable.get("enabled"), bool),
        "settings.immutable_releases.enabled must be null or boolean",
    )
    if immutable_state == "proven":
        require(immutable["enabled"], "proven immutable releases must be enabled")

    tag = settings.get("tag_ruleset")
    tag_state = validate_state_block(tag, "settings.tag_ruleset")
    require(tag.get("pattern") == "v*", "tag ruleset must protect v*")
    require(
        tag.get("enforcement") is None
        or tag.get("enforcement") in {"active", "disabled"},
        "tag_ruleset.enforcement is invalid",
    )
    require(
        isinstance(tag.get("bypass_actors"), list)
        and all(isinstance(item, str) and item.strip() for item in tag["bypass_actors"]),
        "tag_ruleset.bypass_actors must be an array of identities",
    )
    require(
        tag["bypass_actors"] == sorted(set(tag["bypass_actors"])),
        "tag_ruleset.bypass_actors must be sorted and unique",
    )
    require(
        isinstance(tag.get("administrator_bypass_reviewed"), bool),
        "tag_ruleset.administrator_bypass_reviewed must be boolean",
    )
    if tag_state == "proven":
        require(tag["enforcement"] == "active", "proven tag ruleset must be active")
        require(
            tag["administrator_bypass_reviewed"],
            "proven tag ruleset requires explicit bypass review",
        )

    branch = settings.get("branch_ruleset")
    branch_state = validate_state_block(branch, "settings.branch_ruleset")
    contexts = branch.get("required_contexts")
    require(
        isinstance(contexts, list)
        and all(isinstance(item, str) and item.strip() for item in contexts),
        "branch_ruleset.required_contexts must be an array of check names",
    )
    require(
        contexts == sorted(set(contexts)),
        "branch_ruleset.required_contexts must be sorted and unique",
    )
    require(
        isinstance(branch.get("bypass_actors"), list)
        and all(
            isinstance(item, str) and item.strip() for item in branch["bypass_actors"]
        ),
        "branch_ruleset.bypass_actors must be an array of identities",
    )
    require(
        branch["bypass_actors"] == sorted(set(branch["bypass_actors"])),
        "branch_ruleset.bypass_actors must be sorted and unique",
    )
    require(
        isinstance(branch.get("administrator_bypass_reviewed"), bool),
        "branch_ruleset.administrator_bypass_reviewed must be boolean",
    )
    if branch_state == "proven":
        require(bool(contexts), "proven branch ruleset requires exact required contexts")
        require(
            branch["administrator_bypass_reviewed"],
            "proven branch ruleset requires explicit bypass review",
        )

    actions = settings.get("actions_policy")
    actions_state = validate_state_block(actions, "settings.actions_policy")
    require(
        actions.get("default_workflow_permissions") in {None, "read", "write"},
        "actions_policy.default_workflow_permissions is invalid",
    )
    require(
        actions.get("fork_pull_request_write_tokens") is None
        or isinstance(actions.get("fork_pull_request_write_tokens"), bool),
        "actions_policy.fork_pull_request_write_tokens must be null or boolean",
    )
    require(
        actions.get("workflow_pr_creation_and_approval") is None
        or isinstance(actions.get("workflow_pr_creation_and_approval"), bool),
        "actions_policy.workflow_pr_creation_and_approval must be null or boolean",
    )
    if actions_state == "proven":
        require(
            actions["default_workflow_permissions"] == "read",
            "proven Actions policy requires read-only default permissions",
        )
        require(
            actions["fork_pull_request_write_tokens"] is False,
            "proven Actions policy must deny fork PR write tokens",
        )
        require(
            actions["workflow_pr_creation_and_approval"] is not None,
            "proven Actions policy must record the combined workflow PR creation/approval posture",
        )

    codeowners = settings.get("codeowners")
    codeowners_state = validate_state_block(codeowners, "settings.codeowners")
    surfaces = codeowners.get("covered_surfaces")
    require(
        isinstance(surfaces, list)
        and all(isinstance(item, str) and item.strip() for item in surfaces),
        "codeowners.covered_surfaces must be an array",
    )
    require(
        surfaces == sorted(set(surfaces)),
        "codeowners.covered_surfaces must be sorted and unique",
    )
    if codeowners_state == "proven":
        require(
            set(surfaces) == REQUIRED_CODEOWNER_SURFACES,
            f"proven CODEOWNERS must cover exactly {sorted(REQUIRED_CODEOWNER_SURFACES)}",
        )

    validate_environments(settings, channels)
    declared = data.get("declared_overall_state")
    require(declared in STATES, "declared_overall_state is invalid")
    computed = compute_state(data)
    require(
        declared == computed,
        f"declared_overall_state={declared} contradicts computed state {computed}",
    )
    if declared == "proven":
        require(
            subject["topology_digest"] is not None,
            "proven closeout requires a release topology digest",
        )
        require(
            subject["observer"] != "not_proven",
            "proven closeout requires an identified observer",
        )
    limitations = data.get("limitations")
    require(
        isinstance(limitations, list)
        and all(isinstance(item, str) and item.strip() for item in limitations),
        "limitations must be an array of non-empty strings",
    )
    if declared == "proven":
        require(not limitations, "proven closeout cannot retain limitations")
    else:
        require(bool(limitations), "non-proven closeout requires limitations")
    return computed


def render(data: dict[str, Any]) -> str:
    state = validate(data)
    subject = data["subject"]
    settings = data["settings"]
    channel_counts = Counter(
        row["disposition"] for row in data["channel_dispositions"]
    )
    lines = [
        "# Release settings closeout",
        "",
        "> This document projects a live-settings receipt. Checked-in expectations do not prove that",
        "> GitHub repository, ruleset, environment, token, or secret controls are active.",
        "",
        f"**Overall state:** `{state}`",
        "",
        "## Subject",
        "",
        f"- Repository: `{subject['repository']}`",
        f"- Source SHA: `{subject['source_sha']}`",
        f"- Topology digest: `{subject['topology_digest'] or 'not_proven'}`",
        f"- Observed: `{subject['observed_at']}` by `{subject['observer']}`",
        "",
        "## Channel dispositions",
        "",
        "| Channel | Disposition | Owner | Limitation |",
        "| --- | --- | --- | --- |",
    ]
    for row in data["channel_dispositions"]:
        lines.append(
            f"| `{row['channel']}` | `{row['disposition']}` | {row['owner']} | "
            f"{row['limitation'] or '—'} |"
        )
    counts = ", ".join(
        f"`{name}`={channel_counts[name]}" for name in sorted(channel_counts)
    )
    lines.extend(
        [
            "",
            f"Disposition counts: {counts}.",
            "",
            "## Live controls",
            "",
            "| Control | State |",
            "| --- | --- |",
        ]
    )
    for name in (
        "immutable_releases",
        "tag_ruleset",
        "branch_ruleset",
        "actions_policy",
        "codeowners",
    ):
        lines.append(f"| `{name}` | `{settings[name]['state']}` |")
    for row in settings["environments"]:
        lines.append(
            f"| environment `{row['name']}` (`{row['channel']}`) | `{row['state']}` |"
        )
    lines.extend(
        [
            "",
            "## Evidence procedure",
            "",
            "1. Read the effective repository, Actions, branch/tag ruleset, and environment settings from GitHub.",
            "2. Record exact values and an identified observer; never include secret values.",
            "3. Attach durable GitHub URLs to the observation or administrator closeout comment.",
            "4. Bind the packet to the exact source SHA and release-topology digest.",
            "5. Mark a control `proven` only after its expected value and negative direction are both checked.",
            "6. Run the checker; a checklist or source declaration alone remains `not_proven`.",
            "",
            "## Limitations",
            "",
        ]
    )
    if data["limitations"]:
        lines.extend(f"- {item}" for item in data["limitations"])
    else:
        lines.append("None.")
    lines.extend(
        [
            "",
            "Regenerate with:",
            "",
            "```bash",
            "python3 scripts/ci/check_release_settings_closeout.py --write",
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def check_or_write(receipt: Path, markdown: Path, write: bool) -> None:
    rendered = render(load(receipt))
    if write:
        markdown.parent.mkdir(parents=True, exist_ok=True)
        markdown.write_text(rendered, encoding="utf-8")
        return
    try:
        actual = markdown.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ReceiptError(f"generated Markdown not found: {markdown}") from exc
    require(
        actual == rendered,
        "generated Markdown is stale: run check_release_settings_closeout.py --write",
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, default=DEFAULT_RECEIPT)
    parser.add_argument("--markdown", type=Path, default=DEFAULT_MARKDOWN)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    try:
        check_or_write(args.receipt, args.markdown, args.write)
    except ReceiptError as exc:
        print(f"release settings closeout failed: {exc}", file=sys.stderr)
        return 1
    action = "wrote" if args.write else "validated"
    print(f"{action} {args.receipt} and {args.markdown}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
