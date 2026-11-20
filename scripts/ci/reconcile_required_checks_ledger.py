#!/usr/bin/env python3
"""Compare checked-in required-check names against live GitHub enforcement JSON.

The ledger is `.ci/policies/required-checks.toml`. Only plural ``[[checks]]``
rows with ``required = true`` are the merge-gate inventory; singular
``[[check]]`` rows are workflow-trigger-lint input and are not compared here.

Live names are the union of:

- ``GET /repos/{owner}/{repo}/rules/branches/{branch}`` required_status_checks
  contexts (rules that already apply to the branch);
- ``GET /repos/{owner}/{repo}/branches/{branch}/protection`` required status
  check contexts.

The command is read-only. It never mutates branch protection, rulesets, or
the ledger. Drift (missing or extra context names) exits 1. Unreadable or
malformed evidence exits 2 as ``NOT_PROVEN``; a 403 on classic protection is
not treated as an empty surface.

Usage:
  python3 scripts/ci/reconcile_required_checks_ledger.py \\
      --snapshot path/to/combined.json
  python3 scripts/ci/reconcile_required_checks_ledger.py \\
      --rules rules.json --protection protection.json
  python3 scripts/ci/reconcile_required_checks_ledger.py \\
      --fetch --repository OWNER/NAME [--branch main]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

DEFAULT_LEDGER = Path(".ci/policies/required-checks.toml")
DEFAULT_BRANCH = "main"


class ReconcileError(ValueError):
    """Closed-contract failure; maps to NOT_PROVEN."""


@dataclass(frozen=True)
class GhResult:
    returncode: int
    stdout: str
    stderr: str


GhRunner = Callable[[list[str]], GhResult]


@dataclass(frozen=True)
class DiffReport:
    status: str
    missing: tuple[str, ...]
    extra: tuple[str, ...]


def ledger_required_names(toml_text: str) -> frozenset[str]:
    """Return ``[[checks]]`` names with ``required = true``.

    Singular ``[[check]]`` blocks are ignored on purpose: they describe the
    workflow-trigger contract, not the GitHub-protected status-context set.
    """
    data = tomllib.loads(toml_text)
    blocks = data.get("checks", [])
    if not isinstance(blocks, list):
        raise ReconcileError("ledger [[checks]] must be an array")
    names: set[str] = set()
    for index, block in enumerate(blocks):
        if not isinstance(block, dict):
            raise ReconcileError(f"ledger [[checks]][{index}] must be a table")
        if block.get("required") is not True:
            continue
        name = block.get("name")
        if not isinstance(name, str) or not name.strip():
            raise ReconcileError(
                f"ledger [[checks]][{index}] required row is missing name"
            )
        names.add(name)
    return frozenset(names)


def live_required_names(
    rules: Any, protection: Any
) -> frozenset[str]:
    """Union of ruleset and classic required status-check context names."""
    names: set[str] = set()
    names.update(_rules_contexts(rules))
    names.update(_protection_contexts(protection))
    return frozenset(names)


def _rules_contexts(rules: Any) -> set[str]:
    if not isinstance(rules, list):
        raise ReconcileError("rules payload must be a JSON array")
    names: set[str] = set()
    for index, rule in enumerate(rules):
        if not isinstance(rule, dict):
            raise ReconcileError(f"rules[{index}] must be an object")
        if rule.get("type") != "required_status_checks":
            continue
        parameters = rule.get("parameters")
        if not isinstance(parameters, dict):
            raise ReconcileError(
                f"rules[{index}] required_status_checks rule is missing parameters"
            )
        checks = parameters.get("required_status_checks")
        if not isinstance(checks, list):
            raise ReconcileError(
                f"rules[{index}] required_status_checks must be a list"
            )
        for check_index, check in enumerate(checks):
            if not isinstance(check, dict):
                raise ReconcileError(
                    f"rules[{index}].required_status_checks[{check_index}] "
                    "must be an object"
                )
            context = check.get("context")
            if not isinstance(context, str) or not context:
                raise ReconcileError(
                    f"rules[{index}].required_status_checks[{check_index}] "
                    "is missing context"
                )
            names.add(context)
    return names


def _protection_contexts(protection: Any) -> set[str]:
    if protection is None or _is_missing_protection(protection):
        return set()
    if not isinstance(protection, dict):
        raise ReconcileError("protection payload must be an object or null")
    required = protection.get("required_status_checks")
    if required is None:
        return set()
    if not isinstance(required, dict):
        raise ReconcileError(
            "protection.required_status_checks must be an object"
        )
    names: set[str] = set()
    contexts = required.get("contexts")
    if contexts is not None:
        if not isinstance(contexts, list):
            raise ReconcileError(
                "protection.required_status_checks.contexts must be a list"
            )
        for index, context in enumerate(contexts):
            if not isinstance(context, str) or not context:
                raise ReconcileError(
                    f"protection.required_status_checks.contexts[{index}] "
                    "must be a string"
                )
            names.add(context)
    checks = required.get("checks")
    if checks is not None:
        if not isinstance(checks, list):
            raise ReconcileError(
                "protection.required_status_checks.checks must be a list"
            )
        for index, check in enumerate(checks):
            if not isinstance(check, dict):
                raise ReconcileError(
                    f"protection.required_status_checks.checks[{index}] "
                    "must be an object"
                )
            context = check.get("context")
            if not isinstance(context, str) or not context:
                raise ReconcileError(
                    f"protection.required_status_checks.checks[{index}] "
                    "is missing context"
                )
            names.add(context)
    return names


def _is_missing_protection(payload: Any) -> bool:
    if not isinstance(payload, dict):
        return False
    status = payload.get("status")
    if status == "404" or status == 404:
        return True
    message = payload.get("message")
    return isinstance(message, str) and "not protected" in message.lower()


def diff_contexts(
    ledger_names: frozenset[str], live_names: frozenset[str]
) -> DiffReport:
    missing = tuple(sorted(ledger_names - live_names))
    extra = tuple(sorted(live_names - ledger_names))
    status = "DRIFT" if missing or extra else "MATCH"
    return DiffReport(status=status, missing=missing, extra=extra)


def encode_path_segment(value: str) -> str:
    if not value:
        raise ReconcileError("path segment must be a non-empty string")
    return urllib.parse.quote(value, safe="")


def encode_repository(repository: str) -> str:
    parts = repository.split("/")
    if len(parts) != 2 or not all(parts):
        raise ReconcileError("repository must be exactly owner/name")
    owner, name = parts
    return f"{encode_path_segment(owner)}/{encode_path_segment(name)}"


def default_gh_runner(args: list[str]) -> GhResult:
    completed = subprocess.run(
        ["gh", *args],
        capture_output=True,
        text=True,
        check=False,
    )
    return GhResult(completed.returncode, completed.stdout, completed.stderr)


def _parse_json(text: str, field: str) -> Any:
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise ReconcileError(f"{field} is not valid JSON: {error}") from error


def _http_status(payload: Any, stderr: str) -> str | None:
    if isinstance(payload, dict):
        status = payload.get("status")
        if isinstance(status, int):
            return str(status)
        if isinstance(status, str) and status:
            return status
    if "HTTP 404" in stderr or "Not Found" in stderr:
        return "404"
    if "HTTP 403" in stderr or "HTTP 401" in stderr:
        return "403"
    return None


def fetch_live_surfaces(
    repository: str,
    branch: str,
    runner: GhRunner,
) -> tuple[Any, Any]:
    """Read the two documented GitHub endpoints. Protection 404 is absence."""
    repo = encode_repository(repository)
    ref = encode_path_segment(branch)
    rules_path = f"repos/{repo}/rules/branches/{ref}"
    protection_path = f"repos/{repo}/branches/{ref}/protection"

    rules_result = runner(["api", rules_path])
    rules_payload = _parse_json(rules_result.stdout or "{}", "rules")
    if rules_result.returncode != 0:
        raise ReconcileError(
            f"ruleset surface unreadable ({rules_path}): "
            f"{(rules_result.stderr or rules_result.stdout).strip() or 'gh api failed'}"
        )

    protection_result = runner(["api", protection_path])
    protection_payload = _parse_json(
        protection_result.stdout or "null", "protection"
    )
    if protection_result.returncode == 0:
        return rules_payload, protection_payload
    if _is_missing_protection(protection_payload) or _http_status(
        protection_payload, protection_result.stderr
    ) == "404":
        return rules_payload, None
    raise ReconcileError(
        "classic branch protection unreadable "
        f"({protection_path}): treating an unreadable surface as empty would "
        "understate live enforcement"
    )


def load_json_file(path: Path, field: str) -> Any:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise ReconcileError(f"failed to read {field}: {path}: {error}") from error
    return _parse_json(text, field)


def load_snapshot(path: Path) -> tuple[Any, Any]:
    payload = load_json_file(path, "snapshot")
    if not isinstance(payload, dict):
        raise ReconcileError("snapshot must be an object")
    if "rules" not in payload:
        raise ReconcileError("snapshot is missing rules")
    if "protection" not in payload:
        raise ReconcileError("snapshot is missing protection")
    unknown = set(payload) - {"rules", "protection"}
    if unknown:
        raise ReconcileError(
            f"snapshot unsupported fields: {sorted(unknown)}"
        )
    return payload["rules"], payload["protection"]


def explain(report: DiffReport, required: frozenset[str]) -> str:
    lines = [f"required-checks ledger vs live enforcement: {report.status}"]
    if report.status == "MATCH":
        lines.append("- required: " + ", ".join(sorted(required)))
        return "\n".join(lines)
    if report.missing:
        lines.append("- missing from live: " + ", ".join(report.missing))
    if report.extra:
        lines.append("- extra on live: " + ", ".join(report.extra))
    return "\n".join(lines)


def main(
    argv: list[str] | None = None,
    *,
    gh_runner: GhRunner | None = None,
) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--ledger",
        type=Path,
        default=DEFAULT_LEDGER,
        help="path to required-checks.toml",
    )
    parser.add_argument(
        "--snapshot",
        type=Path,
        help="combined JSON object {rules, protection}",
    )
    parser.add_argument(
        "--rules",
        type=Path,
        help="JSON array from GET /repos/{o}/{r}/rules/branches/{branch}",
    )
    parser.add_argument(
        "--protection",
        type=Path,
        help="JSON object from GET /repos/{o}/{r}/branches/{branch}/protection",
    )
    parser.add_argument(
        "--fetch",
        action="store_true",
        help="read both surfaces with `gh api` (optional; needs a token that can read them)",
    )
    parser.add_argument("--repository", help="owner/name for --fetch")
    parser.add_argument("--branch", default=DEFAULT_BRANCH)
    args = parser.parse_args(argv)

    modes = [
        args.snapshot is not None,
        args.rules is not None or args.protection is not None,
        args.fetch,
    ]
    if sum(bool(mode) for mode in modes) != 1:
        print("required-checks ledger vs live enforcement: NOT_PROVEN")
        print(
            "exactly one of --snapshot, --rules/--protection, or --fetch "
            "is required",
            file=sys.stderr,
        )
        return 2
    if (args.rules is None) != (args.protection is None):
        print("required-checks ledger vs live enforcement: NOT_PROVEN")
        print("--rules and --protection must be supplied together", file=sys.stderr)
        return 2
    if args.fetch and not args.repository:
        print("required-checks ledger vs live enforcement: NOT_PROVEN")
        print("--fetch requires --repository owner/name", file=sys.stderr)
        return 2

    try:
        ledger_text = args.ledger.read_text(encoding="utf-8")
        required = ledger_required_names(ledger_text)
        if args.snapshot is not None:
            rules, protection = load_snapshot(args.snapshot)
        elif args.fetch:
            rules, protection = fetch_live_surfaces(
                args.repository,
                args.branch,
                gh_runner or default_gh_runner,
            )
        else:
            rules = load_json_file(args.rules, "rules")
            protection = load_json_file(args.protection, "protection")
        live = live_required_names(rules, protection)
        report = diff_contexts(required, live)
    except OSError as error:
        print("required-checks ledger vs live enforcement: NOT_PROVEN")
        print(f"failed to read ledger: {error}", file=sys.stderr)
        return 2
    except ReconcileError as error:
        print("required-checks ledger vs live enforcement: NOT_PROVEN")
        print(str(error), file=sys.stderr)
        return 2

    print(explain(report, required))
    return 0 if report.status == "MATCH" else 1


if __name__ == "__main__":
    raise SystemExit(main())
