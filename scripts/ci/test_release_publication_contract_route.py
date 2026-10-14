"""Keep #17628's release-shell qualification cheap, reachable and source-bound."""
from pathlib import Path
import re
import unittest

import check_release_prerequisite_graph as graph
from test_release_publication_admission import require

ROOT = Path(__file__).resolve().parents[2]

# The focused hosted producer is intentionally independent of Rust qualification.
CI_WORKFLOW = ROOT / ".github/workflows/release-publication-contract.yml"
CI_INPUTS = frozenset((
    ".github/workflows/release-publication-contract.yml",
    ".github/workflows/release.yml",
    "scripts/ci/test_release_publication_admission.py",
    "scripts/ci/test_release_publication_contract_route.py",
    "scripts/ci/release_publication_admission.py",
    "scripts/ci/check_release_prerequisite_graph.py",
    "policy/release-publication-admission.json",
))
CI_COMMAND = ["python3", "-B", "-m", "unittest", "discover", "-s", "scripts/ci",
              "-p", "test_release_publication*.py", "-v"]


def validate_publication_ci_route(source: str) -> None:
    """Check the narrow producer and every input without adding a YAML dependency."""
    require(set(graph._plain_fields(source, 0)) == {"name", "on", "permissions", "concurrency", "jobs"},
            "unsupported qualification workflow override")
    events = graph._field(source, "on", 0, mapping=True)
    require(set(graph._plain_fields(events, 2)) == {"pull_request", "push"},
            "qualification must use only PR and main-push events")
    for event in ("pull_request", "push"):
        block = graph._field(events, event, 2, mapping=True)
        paths = graph._field(block, "paths", 4, mapping=True)
        observed = []
        for line in paths.splitlines():
            if not line.strip():
                continue
            require(line.startswith("      - '") and line.endswith("'"),
                    "unsupported qualification input path syntax")
            observed.append(line[9:-1])
        require(len(observed) == len(CI_INPUTS) and set(observed) == CI_INPUTS,
                event + " qualification inputs are incomplete or duplicated")
        fields = graph._plain_fields(block, 4)
        if event == "pull_request":
            require(set(fields) == {"types", "paths"}, "PR qualification must include stacked branches")
            require(fields["types"] == "[opened, synchronize, reopened, ready_for_review]",
                    "ready-for-review qualification trigger is missing")
        else:
            require(set(fields) == {"branches", "paths"} and fields["branches"] == "[main]",
                    "landed-main qualification trigger changed")
    permissions = graph._field(source, "permissions", 0, mapping=True)
    require(graph._plain_fields(permissions, 2) == {"contents": "read"},
            "qualification must be read-only")
    jobs = graph._jobs_text(source)
    require(set(graph._plain_fields(jobs, 2)) == {"publication-contract"},
            "qualification must retain one bounded Python job")
    job = graph._job_block(source, "publication-contract")
    fields = graph._plain_fields(job, 4)
    require(set(fields) == {"name", "if", "runs-on", "timeout-minutes", "steps"},
            "unsupported qualification job override")
    require(fields["if"] == "${{ github.event_name != 'pull_request' || github.event.pull_request.draft == false }}",
            "qualification must wait for Ready PRs and retain main pushes")
    require(fields["runs-on"] == "ubuntu-24.04" and fields["timeout-minutes"] == "3",
            "qualification host or time budget changed")
    graph._default_shell_boundary(source)
    graph._job_shell_boundary(job)
    steps = graph._field(job, "steps", 4, mapping=True)
    names = re.findall(r"^      - name: (.*)$", steps, re.MULTILINE)
    require(names == ["Checkout", "Run publication admission regressions"],
            "qualification must execute directly after checkout, without a Rust build")
    checkout = graph._step(job, "Checkout")
    checkout_fields = graph._plain_fields(checkout, 8)
    require(set(checkout_fields) == {"uses", "with"}
            and re.fullmatch(r"actions/checkout@[0-9a-f]{40}", checkout_fields["uses"]),
            "qualification checkout must be pinned and unconditional")
    settings = graph._field(checkout, "with", 8, mapping=True)
    require(graph._plain_fields(settings, 10) == {"persist-credentials": "false"},
            "checkout must use the event subject without retaining credentials")
    run = graph._step(job, "Run publication admission regressions")
    require(set(graph._plain_fields(run, 8)) == {"run"},
            "qualification command must not be conditional, suppressed or retargeted")
    require(graph._commands(run) == [CI_COMMAND], "qualification must run the complete owning module")


class PublicationCiRouteTests(unittest.TestCase):
    def test_actual_ci_route_is_ready_only_and_runs_complete_python_proof(self):
        """Retain the cheap hosted producer for both the repair stack and main."""
        validate_publication_ci_route(CI_WORKFLOW.read_text(encoding="utf-8"))

    def test_ci_route_mutations_cannot_silently_drop_or_skip_qualification(self):
        """Detect realistic lost-trigger, empty-command and wrong-subject repairs."""
        source = CI_WORKFLOW.read_text(encoding="utf-8")
        mutations = [
            ("on:\n", "env:\n  BASH_ENV: injected.sh\non:\n"),
            ("on:\n", "on:\n  workflow_dispatch: {}\n"),
            ("    if: ${{ github.event_name != 'pull_request' || github.event.pull_request.draft == false }}", "    if: false"),
            (", ready_for_review", ""),
            ("  pull_request:\n", "  pull_request:\n    branches: [main]\n"),
            ("persist-credentials: false", "persist-credentials: true"),
            ("          persist-credentials: false", "          persist-credentials: false\n          ref: main"),
            ("  contents: read", "  contents: write"),
            ("    timeout-minutes: 3", "    timeout-minutes: 60"),
            ("      - name: Run publication admission regressions", "      - name: Run publication admission regressions\n        if: false"),
            ("      - name: Run publication admission regressions", "      - name: Run publication admission regressions\n        continue-on-error: true"),
            ("python3 -B -m unittest discover -s scripts/ci -p 'test_release_publication*.py' -v", "true"),
            ("python3 -B -m unittest discover -s scripts/ci -p 'test_release_publication*.py' -v", "python3 -B -m unittest discover -s scripts/ci -p nonexistent-tests.py -v"),
        ]
        mutations += [("      - '" + path + "'\n", "") for path in CI_INPUTS]
        for old, new in mutations:
            with self.subTest(selector=old, replacement=new):
                self.assertIn(old, source, "mutation selector disappeared")
                with self.assertRaises((RuntimeError, graph.GraphError)):
                    validate_publication_ci_route(source.replace(old, new, 1))


if __name__ == "__main__":
    unittest.main()
