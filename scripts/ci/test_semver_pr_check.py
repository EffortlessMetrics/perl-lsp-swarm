#!/usr/bin/env python3
"""Behavioral probes for scripts/ci/semver_pr_check.sh (#17208).

The PR-time semver rail must not report a passing step when
cargo-semver-checks detects a break. These tests drive the script with a
stub `cargo` in PATH; they do not invoke the real tool or network.
"""

from __future__ import annotations

import os
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "ci" / "semver_pr_check.sh"
CI_YML = ROOT / ".github/workflows/ci.yml"


def write_stub_cargo(bin_dir: Path, failing_crates: set[str]) -> None:
    bin_dir.mkdir(parents=True, exist_ok=True)
    cargo_path = bin_dir / "cargo"
    failing = ",".join(sorted(failing_crates))
    cargo_path.write_text(
        textwrap.dedent(
            f"""\
            #!/usr/bin/env bash
            set -euo pipefail
            if [[ "${{1:-}}" != "semver-checks" ]]; then
              echo "stub cargo: unexpected argv $*" >&2
              exit 127
            fi
            shift
            crate=""
            while [[ $# -gt 0 ]]; do
              case "$1" in
                -p) crate="$2"; shift 2 ;;
                *) shift ;;
              esac
            done
            failing="{failing}"
            if [[ ",${{failing}}," == *",${{crate}},"* ]]; then
              echo "stub: breaking ${{crate}}" >&2
              exit 1
            fi
            echo "stub: compatible ${{crate}}"
            exit 0
            """
        ),
        encoding="utf-8",
    )
    cargo_path.chmod(0o755)


def run_check(
    repo: Path,
    *,
    baseline: str = "v0.17.0",
    failing: set[str] | None = None,
) -> subprocess.CompletedProcess[str]:
    failing = failing or set()
    env = os.environ.copy()
    env["SEMVER_BASELINE"] = baseline
    with tempfile.TemporaryDirectory() as tmp:
        stub_bin = Path(tmp) / "bin"
        write_stub_cargo(stub_bin, failing)
        env["PATH"] = f"{stub_bin}:{env['PATH']}"
        return subprocess.run(
            ["bash", str(SCRIPT), str(repo)],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )


def write_min_repo(base: Path, crates: list[str]) -> None:
    ratchet = base / ".ci/public-api-baselines"
    ratchet.mkdir(parents=True)
    (ratchet / "ratchet-crates.txt").write_text(
        "\n".join(crates) + "\n",
        encoding="utf-8",
    )


class SemverPrCheckScriptTests(unittest.TestCase):
    def test_all_compatible_exits_zero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            write_min_repo(repo, ["perl-uri", "perl-dap"])
            result = run_check(repo)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            self.assertIn("semver: perl-uri compatible", result.stdout)

    def test_detected_break_exits_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            write_min_repo(repo, ["perl-uri", "perl-lsp-rs-core"])
            result = run_check(repo, failing={"perl-lsp-rs-core"})
            self.assertNotEqual(result.returncode, 0, "detected break must not pass")
            self.assertIn("Breaking changes detected in perl-lsp-rs-core", result.stdout)
            self.assertIn("semver-pr:", result.stdout)

    def test_empty_ratchet_list_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            write_min_repo(repo, [])
            result = run_check(repo)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("lists no crates", result.stdout)

    def test_duplicate_ratchet_entry_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            write_min_repo(repo, ["perl-uri", "perl-uri"])
            result = run_check(repo)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("lists a crate twice", result.stdout)


class SemverPrWorkflowContractTests(unittest.TestCase):
    def test_ci_yml_invokes_script_without_swallowing_loop(self) -> None:
        workflow = CI_YML.read_text(encoding="utf-8").replace("\r\n", "\n")
        semver = workflow.split("\n  semver-pr:\n", 1)[1].split("\n  pr-smoke:", 1)[0]
        self.assertIn("bash scripts/ci/semver_pr_check.sh", semver)
        self.assertNotIn("continue-on-error: true", semver)
        # Old fail-open loop: warning-only else branch with no terminal exit.
        self.assertNotRegex(
            semver,
            r"else\s*\n\s*echo \"::warning::Breaking changes detected",
        )


if __name__ == "__main__":
    unittest.main()
