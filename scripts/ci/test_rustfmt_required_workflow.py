#!/usr/bin/env python3
"""Structural contracts for rustfmt prevention: advisory producer + required Rust Small path."""

from __future__ import annotations

import copy
import importlib.util
import re
import sys
import tomllib
import unittest

import yaml
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
YAML_HELPER = ROOT / "scripts" / "ci" / "workflow_security_ratchet.py"
SPEC = importlib.util.spec_from_file_location("workflow_security_ratchet", YAML_HELPER)
assert SPEC and SPEC.loader
yaml_structure = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = yaml_structure
SPEC.loader.exec_module(yaml_structure)

WORKFLOW_PATH = ROOT / ".github" / "workflows" / "ci.yml"
RUST_SMALL_WORKFLOW_PATH = ROOT / ".github" / "workflows" / "em-ci-routed-rust.yml"
POLICY_PATH = ROOT / ".ci" / "policies" / "required-checks.toml"
JOB_ID = "rust-formatting"
CONTEXT_NAME = "Rust formatting"
RUN_STEP = "Run candidate-bound rustfmt check"
VERIFY_STEP = "Verify candidate-bound rustfmt receipt"
UPLOAD_STEP = "Upload candidate-bound rustfmt receipt"
SUBJECT_EXPRESSION = (
    "github.event_name == 'pull_request' && github.event.pull_request.head.sha || "
    "github.event_name == 'merge_group' && github.event.merge_group.head_sha || github.sha"
)
RUST_SMALL_LANE_JOBS = (
    "rust-small-cx53",
    "rust-small-cx43",
    "rust-small-github",
    "rust-small-fallback",
)
RUST_SMALL_RESULT_JOB = "rust-small-result"
FMT_COMMAND = "cargo fmt --all -- --check"
CONTRACT_TEST_FILES = (
    "scripts/ci/test_rustfmt_check.py",
    "scripts/ci/test_rustfmt_required_workflow.py",
)
CARGO_FMT_RE = re.compile(r"cargo\s+fmt\b")


def load_triggers(source: str) -> dict[str, dict[str, Any]]:
    """Read the workflow's `on:` triggers with a real YAML parser.

    A line-based reader only sees block style, so a flow mapping such as
    `pull_request: { paths-ignore: ['**.md'] }` hid its own body and the
    docs-only guard passed on a workflow GitHub would skip for Markdown-only
    PRs. ci.yml already uses flow style (`merge_group: {}`), so this is a real
    spelling, not a hypothetical one. Parsing as YAML accepts every equivalent
    representation instead of one shape at a time.

    Scope, deliberately: this parses the WHOLE file, not the `on:` block alone.
    Trigger validation therefore depends on all of ci.yml being loadable by
    `safe_load`, and any construct anywhere in the file that PyYAML rejects
    reds this contract with "not valid YAML" even when the `on:` block itself
    is fine — stricter than the rest of the contract, which reads only the
    formatter job. The alternative, slicing the `on:` block out before parsing,
    needs a hand-rolled column-0 scan to find where the block ends, which is
    the exact class of reader this function replaced; letting YAML find the
    boundary is the point. If you add a YAML feature to ci.yml that PyYAML
    cannot load, expect it to surface here first.

    YAML 1.1 resolves the bare key `on` to boolean True, so both spellings are
    accepted.
    """
    try:
        document = yaml.safe_load(source)
    except yaml.YAMLError as error:
        raise AssertionError(f"workflow source is not valid YAML: {error}") from error
    if not isinstance(document, dict):
        raise AssertionError("workflow source is not a YAML mapping")
    node = document.get("on", document.get(True))
    if node is None:
        raise AssertionError("workflow declares no triggers")
    if isinstance(node, str):
        return {node: {}}
    if isinstance(node, list):
        return {str(event): {} for event in node}
    if not isinstance(node, dict):
        raise AssertionError("workflow triggers are not a mapping")
    triggers: dict[str, dict[str, Any]] = {}
    for event, body in node.items():
        if body is None:
            # A bare `pull_request:` is valid and means every activity type.
            triggers[str(event)] = {}
        elif isinstance(body, dict):
            triggers[str(event)] = body
        else:
            # Coercing an unsupported shape to {} would report an unconfigured
            # event body and let the docs-only and required-trigger checks pass
            # without the event configuration ever being valid.
            raise AssertionError(
                f"trigger {str(event)!r} has a {type(body).__name__} body; "
                "expected a mapping or an empty body"
            )
    return triggers


def load_workflow(text: str | None = None) -> dict[str, Any]:
    """Parse the formatter contract out of ci.yml, or out of `text` when supplied.

    Accepting source text lets the mutation tests drop the governed context from
    real workflow source instead of from an already-parsed dictionary (#9564).
    """
    source = WORKFLOW_PATH.read_text(encoding="utf-8") if text is None else text
    lines = source.splitlines()
    triggers = load_triggers(source)
    job: dict[str, Any] = {"env": {}, "steps": []}
    section = ""
    current_step: dict[str, Any] | None = None
    nested: dict[str, str] | None = None
    index = 0
    while index < len(lines):
        parsed = yaml_structure._parse_key_line(lines[index])
        if parsed and parsed.indent == 0:
            # Any top-level key ends the previous section, so `concurrency`,
            # `permissions`, and `env` children cannot be mistaken for jobs.
            section = parsed.key if parsed.key == "jobs" else ""
        elif section in {"jobs", "other-job"} and parsed and parsed.indent == 2:
            section = "formatter" if parsed.key == JOB_ID else "other-job"
        elif section == "formatter" and parsed:
            value = yaml_structure._strip_scalar(parsed.value)
            if parsed.list_item and parsed.indent == 8 and parsed.key == "name":
                current_step = {"name": value}
                job["steps"].append(current_step)
                nested = None
            elif parsed.indent == 4:
                if parsed.key not in {"env", "steps"}:
                    job[parsed.key] = value
                current_step = None
                nested = None
            elif parsed.indent == 6 and current_step is None:
                job["env"][parsed.key] = value
            elif parsed.indent == 8 and current_step is not None:
                if parsed.key in {"with"}:
                    nested = {}
                    current_step[parsed.key] = nested
                elif parsed.key == "run" and value in {"|", ">"}:
                    block: list[str] = []
                    cursor = index + 1
                    while cursor < len(lines):
                        candidate = lines[cursor]
                        if candidate.strip() and len(candidate) - len(candidate.lstrip()) <= 8:
                            break
                        block.append(candidate[10:] if len(candidate) >= 10 else "")
                        cursor += 1
                    current_step["run"] = "\n".join(block)
                    index = cursor - 1
                else:
                    current_step[parsed.key] = value
                    nested = None
            elif parsed.indent == 10 and current_step is not None and nested is not None:
                nested[parsed.key] = value
        index += 1
    return {"on": triggers, "jobs": {JOB_ID: job}}


def load_policy(text: str | None = None) -> dict[str, object]:
    """Load the required-checks policy from disk, or from `text` when supplied."""
    source = POLICY_PATH.read_text(encoding="utf-8") if text is None else text
    return tomllib.loads(source)


def named_step(job: dict[str, Any], name: str) -> dict[str, Any]:
    matches = [item for item in job.get("steps", []) if item.get("name") == name]
    if len(matches) != 1:
        raise AssertionError(f"expected exactly one {name!r} step, found {len(matches)}")
    return matches[0]


def validate_contract(workflow: dict[str, Any], policy: dict[str, object]) -> None:
    triggers = workflow.get("on", {})
    for event in ("pull_request", "merge_group", "push"):
        if event not in triggers:
            raise AssertionError(f"workflow must report on {event}")
    pull_request = triggers.get("pull_request", {})
    if "paths" in pull_request or "paths-ignore" in pull_request:
        raise AssertionError("formatter context must remain terminal for docs-only PRs")

    job = workflow.get("jobs", {}).get(JOB_ID)
    if not isinstance(job, dict):
        raise AssertionError("formatter job is missing")
    if job.get("name") != CONTEXT_NAME:
        raise AssertionError("formatter context name drifted")
    if job.get("continue-on-error") == "true":
        raise AssertionError("formatter job must fail on formatter or instrument failure")
    if "needs" in job or "if" in job:
        raise AssertionError("formatter context must run terminally on every triggered subject")
    if job.get("env", {}).get("SUBJECT_SHA") != "${{ " + SUBJECT_EXPRESSION + " }}":
        raise AssertionError("formatter subject must bind PR, merge-group, and push commits exactly")

    checkout = named_step(job, "Checkout exact formatter subject")
    checkout_with = checkout.get("with", {})
    if checkout_with.get("ref") != "${{ env.SUBJECT_SHA }}":
        raise AssertionError("checkout must use the exact formatter subject")
    if checkout_with.get("persist-credentials") != "false":
        raise AssertionError("formatter checkout must not persist workflow credentials")

    install = named_step(job, "Install pinned formatter toolchain")
    install_with = install.get("with", {})
    if install_with.get("toolchain") != "1.95.0" or install_with.get("components") != "rustfmt":
        raise AssertionError("formatter must remain pinned to Rust 1.95.0 rustfmt")

    run_step = named_step(job, RUN_STEP)
    if run_step.get("continue-on-error") == "true":
        raise AssertionError("formatter run step must not continue on error")
    run = run_step.get("run", "")
    for required_fragment in (
        "scripts/ci/rustfmt_check.py",
        '--candidate-sha "$SUBJECT_SHA"',
        '--candidate-tree-sha "$SUBJECT_TREE_SHA"',
        "rustup which --toolchain 1.95.0 rustfmt",
        "rustup which --toolchain 1.95.0 rustc",
        '--rustc "$rustc_bin"',
    ):
        if required_fragment not in run:
            raise AssertionError(f"formatter invocation missing {required_fragment!r}")
    if "|| true" in run:
        raise AssertionError("formatter failure must not be ignored")

    verify_step = named_step(job, VERIFY_STEP)
    if verify_step.get("continue-on-error") == "true":
        raise AssertionError("receipt verifier must not continue on error")
    verify_run = verify_step.get("run", "")
    for required_fragment in (
        "scripts/ci/verify_rustfmt_receipt.py",
        '--receipt "$RECEIPT_PATH"',
        "--producer scripts/ci/rustfmt_check.py",
        '--candidate-sha "$SUBJECT_SHA"',
        '--candidate-tree-sha "$SUBJECT_TREE_SHA"',
        '--rustc "$rustc_bin"',
    ):
        if required_fragment not in verify_run:
            raise AssertionError(f"receipt verifier missing {required_fragment!r}")
    if "|| true" in verify_run:
        raise AssertionError("receipt verifier must not be bypassed")
    steps = job.get("steps", [])
    if steps.index(verify_step) <= steps.index(run_step) or steps.index(verify_step) >= steps.index(named_step(job, UPLOAD_STEP)):
        raise AssertionError("receipt verifier must run after producer and before upload")

    upload = named_step(job, UPLOAD_STEP)
    upload_with = upload.get("with", {})
    if upload.get("if") != "always()":
        raise AssertionError("formatter receipt upload must always run")
    if upload_with.get("name") != "rustfmt-check-${{ env.SUBJECT_SHA }}":
        raise AssertionError("formatter artifact name must bind the exact subject")
    if upload_with.get("path") != "${{ env.RECEIPT_PATH }}":
        raise AssertionError("formatter artifact path must use the receipt path")
    if upload_with.get("if-no-files-found") != "error":
        raise AssertionError("missing formatter receipt must fail closed")

    entries = [item for item in policy.get("checks", []) if item.get("name") == CONTEXT_NAME]
    if len(entries) != 1:
        raise AssertionError("policy must contain exactly one formatter context")
    entry = entries[0]
    if entry.get("workflow") != ".github/workflows/ci.yml":
        raise AssertionError("formatter policy must name the owning workflow")
    if entry.get("job") != JOB_ID:
        raise AssertionError("formatter policy must name the owning job")
    # Policy schema v2 separates merge-policy role from live GitHub enforcement.
    # "advisory" is a `policy_role`; the enforcement source must stay unclaimed
    # until a reviewed settings change actually protects the context.
    if (
        entry.get("required") is not False
        or entry.get("policy_role") != "advisory"
        or entry.get("enforcement") != "neither"
    ):
        raise AssertionError("formatter policy must remain advisory before post-merge promotion")
    if "post-merge promotion target" not in str(entry.get("reason", "")).lower():
        raise AssertionError("formatter policy must retain the post-merge promotion target")


class RustfmtRequiredWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = load_workflow()
        self.policy = load_policy()

    def job(self, workflow: dict[str, Any]) -> dict[str, Any]:
        return workflow["jobs"][JOB_ID]

    def test_checked_in_workflow_and_policy_are_coherent(self) -> None:
        validate_contract(self.workflow, self.policy)

    def test_missing_merge_group_subject_fails_closed(self) -> None:
        broken = copy.deepcopy(self.workflow)
        self.job(broken)["env"]["SUBJECT_SHA"] = "${{ github.sha }}"
        with self.assertRaisesRegex(AssertionError, "bind PR, merge-group, and push"):
            validate_contract(broken, self.policy)

    def test_run_step_cannot_continue_on_error(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), RUN_STEP)["continue-on-error"] = "true"
        with self.assertRaisesRegex(AssertionError, "run step must not continue"):
            validate_contract(broken, self.policy)

    def test_verifier_cannot_be_removed_or_bypassed(self) -> None:
        broken = copy.deepcopy(self.workflow)
        self.job(broken)["steps"] = [step for step in self.job(broken)["steps"] if step.get("name") != VERIFY_STEP]
        with self.assertRaisesRegex(AssertionError, "expected exactly one"):
            validate_contract(broken, self.policy)
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), VERIFY_STEP)["continue-on-error"] = "true"
        with self.assertRaisesRegex(AssertionError, "verifier must not continue"):
            validate_contract(broken, self.policy)
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), VERIFY_STEP)["run"] += "\ntrue || true"
        with self.assertRaisesRegex(AssertionError, "verifier must not be bypassed"):
            validate_contract(broken, self.policy)

    def test_wrong_artifact_name_is_rejected(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), UPLOAD_STEP)["with"]["name"] = "rustfmt-check-latest"
        with self.assertRaisesRegex(AssertionError, "artifact name"):
            validate_contract(broken, self.policy)

    def test_wrong_artifact_path_is_rejected(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), UPLOAD_STEP)["with"]["path"] = "target/other.json"
        with self.assertRaisesRegex(AssertionError, "artifact path"):
            validate_contract(broken, self.policy)

    def test_upload_without_always_is_rejected_despite_decoy(self) -> None:
        broken = copy.deepcopy(self.workflow)
        job = self.job(broken)
        named_step(job, UPLOAD_STEP).pop("if")
        job["steps"].append({"name": "Decoy always", "if": "always()", "run": "true"})
        with self.assertRaisesRegex(AssertionError, "upload must always run"):
            validate_contract(broken, self.policy)

    def test_missing_receipt_cannot_report_green(self) -> None:
        broken = copy.deepcopy(self.workflow)
        named_step(self.job(broken), UPLOAD_STEP)["with"]["if-no-files-found"] = "warn"
        with self.assertRaisesRegex(AssertionError, "must fail closed"):
            validate_contract(broken, self.policy)

    def test_premature_required_policy_is_rejected(self) -> None:
        broken = copy.deepcopy(self.policy)
        entry = next(item for item in broken["checks"] if item.get("name") == CONTEXT_NAME)
        entry["required"] = True
        entry["policy_role"] = "required"
        entry["enforcement"] = "github-ruleset"
        with self.assertRaisesRegex(AssertionError, "remain advisory"):
            validate_contract(self.workflow, broken)


def replace_pull_request_trigger(workflow_text: str, replacement: str) -> str:
    """Swap the entire `on.pull_request` block for `replacement`.

    The real block carries comment lines and a `types:` key, so replacing only
    its first line would leave them dangling and produce invalid YAML rather
    than the equivalent workflow the mutation is meant to express.

    The block ends at the next sibling key at the same indent, found by scan
    rather than by naming whichever trigger happens to follow. Hardcoding
    `merge_group:` as the delimiter meant that reordering `on:` moved the
    boundary silently past `push:`, so the mutation expressed more than it
    said. Fails closed with a named error, like every other helper here.
    """
    lines = workflow_text.splitlines(keepends=True)
    start = next(
        (
            index
            for index, line in enumerate(lines)
            if line.rstrip("\n") == "  pull_request:"
        ),
        None,
    )
    if start is None:
        raise AssertionError("`on.pull_request` block is not present in workflow source")
    end = next(
        (
            index
            for index in range(start + 1, len(lines))
            if lines[index].strip()
            and lines[index].startswith("  ")
            and not lines[index].startswith("   ")
        ),
        None,
    )
    if end is None:
        raise AssertionError(
            "`on.pull_request` block has no following sibling key to bound it"
        )
    return "".join(lines[:start] + [replacement] + lines[end:])


def drop_governed_job(workflow_text: str) -> str:
    """Delete the whole `rust-formatting:` job block from workflow source.

    This reproduces the exact #9564 regression shape: the governed context
    disappears and every other job in the workflow survives untouched.
    """
    lines = workflow_text.splitlines(keepends=True)
    header = f"  {JOB_ID}:"
    start = next(
        (index for index, line in enumerate(lines) if line.rstrip("\n") == header),
        None,
    )
    if start is None:
        raise AssertionError(f"{JOB_ID!r} job block is not present in workflow source")
    end = len(lines)
    for cursor in range(start + 1, len(lines)):
        candidate = lines[cursor].rstrip("\n")
        if candidate.startswith("  ") and not candidate.startswith("   "):
            end = cursor
            break
    return "".join(lines[:start] + lines[end:])


def policy_entry_index(blocks: list[str], name: str) -> int:
    """Locate the single `[[checks]]` block declaring `name`, or fail closed."""
    hits = [index for index, block in enumerate(blocks) if f'name = "{name}"' in block]
    if len(hits) != 1:
        raise AssertionError(f"expected exactly one {name!r} policy entry, found {len(hits)}")
    return hits[0]


def drop_policy_entry(policy_text: str, name: str) -> str:
    """Delete exactly one `[[checks]]` entry by context name from policy source."""
    blocks = policy_text.split("[[checks]]")
    del blocks[policy_entry_index(blocks, name)]
    return "[[checks]]".join(blocks)


def mutate_policy_entry(policy_text: str, name: str, old: str, new: str) -> str:
    """Rewrite `old` into `new` inside exactly the named `[[checks]]` entry.

    Scoping the rewrite to one entry keeps the mutation independent of key order
    and of every unrelated context in the policy file.
    """
    blocks = policy_text.split("[[checks]]")
    index = policy_entry_index(blocks, name)
    if old not in blocks[index]:
        raise AssertionError(f"{old!r} is not present in the {name!r} policy entry")
    blocks[index] = blocks[index].replace(old, new, 1)
    return "[[checks]]".join(blocks)


class GovernedContextSourceMutationTests(unittest.TestCase):
    """Prove the contract rejects the governed context being dropped from real source.

    The dict-level mutations above prove `validate_contract` discriminates *after*
    parsing succeeded. They cannot fail if the hand-rolled workflow parser simply
    stops seeing the job, which is exactly how the #6858 rebase dropped the
    `Rust formatting` governed context with nothing going red (#9564).
    """

    def setUp(self) -> None:
        self.workflow_text = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.policy_text = POLICY_PATH.read_text(encoding="utf-8")

    def validate_source(self, workflow_text: str, policy_text: str) -> None:
        """Run the contract against workflow and policy source text."""
        validate_contract(load_workflow(workflow_text), load_policy(policy_text))

    def test_unmutated_source_satisfies_the_contract(self) -> None:
        # Anti-vacuity anchor: the mutations below must be the reason the contract
        # fails. Also proves the optional-text path agrees with the disk path.
        self.validate_source(self.workflow_text, self.policy_text)

    def test_trigger_parsing_does_not_absorb_unrelated_top_level_sections(self) -> None:
        # Regression: the `on` section never terminated, so `concurrency`,
        # `permissions`, and `env` children (`group`, `contents`,
        # `CARGO_TERM_COLOR`, ...) were accepted as if they were workflow events.
        triggers = load_workflow(self.workflow_text)["on"]
        for leaked in ("group", "cancel-in-progress", "contents", "CARGO_TERM_COLOR"):
            self.assertNotIn(leaked, triggers)
        self.assertEqual(
            set(triggers), {"pull_request", "merge_group", "push", "workflow_dispatch"}
        )

    # The next four tests are a deliberate 2x2: {paths, paths-ignore} x
    # {block, flow}. A diagonal of two would cover the contract today, because
    # the axes are resolved in different functions — representation in
    # load_triggers, key in validate_contract. The full matrix is what detects
    # that separation breaking, and it costs four assertions. If you prune
    # these, prune to a diagonal deliberately; do not drop an axis.
    def test_docs_only_path_filter_fails_closed(self) -> None:
        # Regression: trigger bodies parsed as empty dicts, so validate_contract's
        # docs-only guard was unreachable on real source no matter what it said.
        broken = self.workflow_text.replace(
            "  pull_request:\n    branches:",
            "  pull_request:\n    paths:\n      - '**.rs'\n    branches:",
            1,
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "terminal for docs-only"):
            self.validate_source(broken, self.policy_text)

    def test_paths_ignore_filter_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            "  pull_request:\n    branches:",
            "  pull_request:\n    paths-ignore:\n      - '**.md'\n    branches:",
            1,
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "terminal for docs-only"):
            self.validate_source(broken, self.policy_text)

    def test_flow_style_docs_only_filter_fails_closed(self) -> None:
        # A line-based reader saw `pull_request: { ... }` as an empty body and
        # let the docs-only guard pass on a workflow GitHub would skip for
        # Markdown-only PRs. ci.yml already uses flow style (`merge_group: {}`),
        # so this spelling is real, not hypothetical.
        broken = replace_pull_request_trigger(
            self.workflow_text,
            "  pull_request: { branches: [ main, master ], paths-ignore: ['**.md'] }\n",
        )
        self.assertNotEqual(broken, self.workflow_text)
        self.assertIn("paths-ignore", load_triggers(broken)["pull_request"])
        with self.assertRaisesRegex(AssertionError, "terminal for docs-only"):
            self.validate_source(broken, self.policy_text)

    def test_flow_style_paths_filter_fails_closed(self) -> None:
        broken = replace_pull_request_trigger(
            self.workflow_text,
            "  pull_request: { branches: [ main, master ], paths: ['**.rs'] }\n",
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "terminal for docs-only"):
            self.validate_source(broken, self.policy_text)

    def test_non_mapping_trigger_body_fails_closed(self) -> None:
        # Regression: every non-mapping body was coerced to {}, so a workflow
        # whose event configuration is not even valid reported an unconfigured
        # event and satisfied both the required-trigger and docs-only checks.
        for spelling in ("true", "[branches]", "'yes'", "42"):
            broken = replace_pull_request_trigger(
                self.workflow_text, f"  pull_request: {spelling}\n"
            )
            self.assertNotEqual(broken, self.workflow_text)
            with self.assertRaisesRegex(AssertionError, "expected a mapping"):
                self.validate_source(broken, self.policy_text)

    def test_empty_trigger_body_is_accepted(self) -> None:
        # Positive control for the check above: a bare `pull_request:` is legal
        # GitHub Actions, so rejecting non-mappings must not reject null bodies.
        relaxed = replace_pull_request_trigger(self.workflow_text, "  pull_request:\n")
        self.assertEqual(load_triggers(relaxed)["pull_request"], {})
        self.validate_source(relaxed, self.policy_text)

    def test_unparseable_workflow_source_fails_closed(self) -> None:
        # A corrupted or truncated ci.yml must red the gate with a named error,
        # not escape as a raw parser exception.
        with self.assertRaisesRegex(AssertionError, "not valid YAML"):
            load_triggers(self.workflow_text + "\n  : : :\n\t- bad\n")

    def test_trigger_replacement_stops_at_the_next_sibling_key(self) -> None:
        # Regression: the boundary was found by hardcoding `merge_group:`, so
        # reordering `on:` moved it silently past `push:` and the mutation
        # expressed more than it said — the failure mode this suite exists to
        # catch, in the suite's own helper.
        reordered = self.workflow_text.replace("  merge_group: {}\n", "", 1).replace(
            "  workflow_dispatch:\n", "  merge_group: {}\n  workflow_dispatch:\n", 1
        )
        self.assertNotEqual(reordered, self.workflow_text)
        swapped = replace_pull_request_trigger(
            reordered, "  pull_request: { branches: [ main, master ] }\n"
        )
        self.assertIn("push", load_triggers(swapped))
        self.assertIn("merge_group", load_triggers(swapped))

    def test_trigger_replacement_helper_fails_closed(self) -> None:
        # The helper carries the same fail-closed duty as the rest: a named
        # error, never a raw ValueError and never a silent wrong region.
        with self.assertRaisesRegex(AssertionError, "not present in workflow source"):
            replace_pull_request_trigger("on:\n  push: {}\n", "  pull_request: {}\n")
        with self.assertRaisesRegex(AssertionError, "no following sibling key"):
            replace_pull_request_trigger(
                "on:\n  pull_request:\n    branches: [ main ]\n", "x\n"
            )

    def test_quoted_on_key_is_read_the_same_as_the_bare_key(self) -> None:
        # YAML 1.1 resolves a bare `on` to boolean True; the quoted spelling
        # stays a string. Both must yield the same trigger set.
        quoted = self.workflow_text.replace("\non:\n", '\n"on":\n', 1)
        self.assertNotEqual(quoted, self.workflow_text)
        self.assertEqual(load_triggers(quoted), load_triggers(self.workflow_text))
        self.validate_source(quoted, self.policy_text)

    def test_dropping_a_required_trigger_from_source_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            "  push:\n    branches: [ main, master ]\n", "", 1
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "must report on push"):
            self.validate_source(broken, self.policy_text)

    def test_dropping_the_governed_job_from_workflow_source_fails_closed(self) -> None:
        broken = drop_governed_job(self.workflow_text)
        self.assertNotIn(f"  {JOB_ID}:\n", broken)
        self.assertLess(len(broken), len(self.workflow_text))
        # The mutation stays surgical: the rest of the workflow is still there.
        self.assertIn("\njobs:\n", broken)
        self.assertIn("pull_request:", broken)
        with self.assertRaisesRegex(AssertionError, "formatter context name drifted"):
            self.validate_source(broken, self.policy_text)

    def test_renaming_the_governed_context_in_workflow_source_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            f"name: {CONTEXT_NAME}", "name: Rust format check", 1
        )
        self.assertNotEqual(broken, self.workflow_text)
        with self.assertRaisesRegex(AssertionError, "formatter context name drifted"):
            self.validate_source(broken, self.policy_text)

    def test_dropping_the_policy_entry_from_source_fails_closed(self) -> None:
        broken = drop_policy_entry(self.policy_text, CONTEXT_NAME)
        self.assertNotIn(f'name = "{CONTEXT_NAME}"', broken)
        with self.assertRaisesRegex(AssertionError, "exactly one formatter context"):
            self.validate_source(self.workflow_text, broken)

    def test_repointing_the_policy_entry_in_source_fails_closed(self) -> None:
        broken = mutate_policy_entry(
            self.policy_text, CONTEXT_NAME, f'job = "{JOB_ID}"', 'job = "rust-fmt"'
        )
        self.assertNotEqual(broken, self.policy_text)
        with self.assertRaisesRegex(AssertionError, "must name the owning job"):
            self.validate_source(self.workflow_text, broken)

    def test_promoting_the_policy_entry_in_source_fails_closed(self) -> None:
        # The advisory-before-promotion rule must bind real policy source too.
        broken = mutate_policy_entry(
            self.policy_text, CONTEXT_NAME, "required = false", "required = true"
        )
        self.assertNotEqual(broken, self.policy_text)
        with self.assertRaisesRegex(AssertionError, "remain advisory"):
            self.validate_source(self.workflow_text, broken)


def load_rust_small_workflow_text() -> str:
    return RUST_SMALL_WORKFLOW_PATH.read_text(encoding="utf-8")


def job_bodies(workflow_text: str) -> dict[str, str]:
    """Return indent-2 GitHub Actions job bodies keyed by job id."""
    bodies: dict[str, list[str]] = {}
    current: str | None = None
    in_jobs = False
    for line in workflow_text.splitlines():
        if line == "jobs:":
            in_jobs = True
            current = None
            continue
        if in_jobs and line and not line.startswith((" ", "\t")):
            in_jobs = False
            current = None
            continue
        if not in_jobs:
            continue
        if (
            line.startswith("  ")
            and not line.startswith("   ")
            and line.rstrip().endswith(":")
            and not line.lstrip().startswith("-")
        ):
            current = line.strip()[:-1]
            bodies[current] = [line]
        elif current is not None:
            bodies[current].append(line)
    return {job_id: "\n".join(lines) for job_id, lines in bodies.items()}


def active_code_lines(text: str) -> list[str]:
    lines: list[str] = []
    for raw in text.splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        lines.append(raw)
    return lines


def validate_rust_small_fmt_contract(workflow_text: str) -> None:
    jobs = job_bodies(workflow_text)
    missing = [job_id for job_id in RUST_SMALL_LANE_JOBS if job_id not in jobs]
    if missing:
        raise AssertionError(f"required Rust Small lane jobs missing: {missing}")

    for job_id in RUST_SMALL_LANE_JOBS:
        body = jobs[job_id]
        active_lines = active_code_lines(body)
        if not any(line.strip().strip("'\"") == FMT_COMMAND for line in active_lines):
            raise AssertionError(
                f"{job_id} must run workspace-wide {FMT_COMMAND!r}; "
                "commenting it out or deleting it is a silent revert of #12320"
            )
        active = "\n".join(active_lines)
        if "git diff --name-only" in active:
            raise AssertionError(
                f"{job_id} reintroduced changed-file narrowing around rustfmt"
            )
        if CARGO_FMT_RE.search(active) and re.search(
            r"cargo\s+fmt\b(?![^\n]*--all)", active
        ):
            raise AssertionError(
                f"{job_id} must keep cargo fmt --all; dropping --all reintroduces "
                "changed-file or crate-local narrowing"
            )
        if re.search(r"cargo\s+fmt\b[^\n]*--files\b", active):
            raise AssertionError(
                f"{job_id} must not pass --files to cargo fmt / rustfmt"
            )

    result_job = jobs.get(RUST_SMALL_RESULT_JOB)
    if not isinstance(result_job, str):
        raise AssertionError("Perl LSP Rust Small Result job is missing")
    result_active_lines = active_code_lines(result_job)
    result_active = "\n".join(result_active_lines)
    if not any(
        line.strip().startswith("python3 -m unittest") for line in result_active_lines
    ):
        raise AssertionError(
            "Perl LSP Rust Small Result must invoke the rustfmt prevention tests"
        )
    for test_file in CONTRACT_TEST_FILES:
        if test_file not in result_active:
            raise AssertionError(
                f"Perl LSP Rust Small Result must run {test_file} so a silent "
                "revert fails a required check"
            )
    prove = _named_step_body(result_job, "Prove rustfmt prevention contract")
    if re.search(r"^\s+if:", prove, re.MULTILINE):
        raise AssertionError(
            "rustfmt prevention contract must not be skipped with if: "
            "(draft-skip stays in the evaluate step, #10006)"
        )
    if "continue-on-error: true" in prove:
        raise AssertionError("rustfmt prevention contract must not continue on error")
    if "router was skipped (draft PR" not in result_job:
        raise AssertionError(
            "draft-skip remains owned by #10006; do not absorb it into this contract"
        )


def _named_step_body(job_text: str, step_name: str) -> str:
    marker = f"- name: {step_name}"
    start = job_text.find(marker)
    if start < 0:
        raise AssertionError(f"missing step {step_name!r}")
    rest = job_text[start + len(marker) :]
    next_step = rest.find("\n      - name:")
    return rest if next_step < 0 else rest[:next_step]


class RustSmallRequiredFmtTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow_text = load_rust_small_workflow_text()

    def test_checked_in_rust_small_fmt_contract_holds(self) -> None:
        validate_rust_small_fmt_contract(self.workflow_text)

    def test_echo_decoy_fmt_command_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            FMT_COMMAND, f'echo "{FMT_COMMAND}"', 1
        )
        with self.assertRaisesRegex(AssertionError, "silent revert of #12320"):
            validate_rust_small_fmt_contract(broken)

    def test_fmt_command_with_or_true_fails_closed(self) -> None:
        broken = self.workflow_text.replace(FMT_COMMAND, f"{FMT_COMMAND} || true", 1)
        with self.assertRaisesRegex(AssertionError, "silent revert of #12320"):
            validate_rust_small_fmt_contract(broken)

    def test_commenting_out_one_lane_fmt_fails_closed(self) -> None:
        broken = self.workflow_text.replace(FMT_COMMAND, f"# {FMT_COMMAND}", 1)
        with self.assertRaisesRegex(AssertionError, "silent revert of #12320"):
            validate_rust_small_fmt_contract(broken)

    def test_dropping_all_flag_fails_closed(self) -> None:
        broken = self.workflow_text.replace(FMT_COMMAND, "cargo fmt -- --check", 1)
        with self.assertRaisesRegex(AssertionError, "dropping --all|silent revert"):
            validate_rust_small_fmt_contract(broken)

    def test_changed_file_narrowing_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            FMT_COMMAND,
            "cargo fmt --all -- --check --files $(git diff --name-only origin/main)",
            1,
        )
        with self.assertRaisesRegex(AssertionError, "changed-file narrowing|--files|silent revert"):
            validate_rust_small_fmt_contract(broken)

    def test_echo_decoy_unittest_invocation_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            "python3 -m unittest", 'echo "python3 -m unittest"', 1
        )
        with self.assertRaisesRegex(
            AssertionError, "must invoke the rustfmt prevention tests"
        ):
            validate_rust_small_fmt_contract(broken)

    def test_removing_result_job_test_invocation_fails_closed(self) -> None:
        broken = "\n".join(
            line
            for line in self.workflow_text.splitlines()
            if "python3 -m unittest" not in line
            and all(test_file not in line for test_file in CONTRACT_TEST_FILES)
        )
        with self.assertRaisesRegex(
            AssertionError, "must invoke the rustfmt prevention tests|must run "
        ):
            validate_rust_small_fmt_contract(broken)

    def test_skipping_the_contract_step_with_if_fails_closed(self) -> None:
        broken = self.workflow_text.replace(
            "- name: Prove rustfmt prevention contract\n        shell: bash",
            "- name: Prove rustfmt prevention contract\n        if: false\n        shell: bash",
        )
        with self.assertRaisesRegex(AssertionError, "must not be skipped with if"):
            validate_rust_small_fmt_contract(broken)


if __name__ == "__main__":
    unittest.main()

