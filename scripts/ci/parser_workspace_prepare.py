#!/usr/bin/env python3
"""Parser/DAP repository obligations, sequential under the existing admitted owner.

Run through the existing gate runner before integration libtest. This owns no
new resource roots, admission policy, cleanup, retries, or subprocess supervisor.
The root must admit the entire gate workload, including strict Clippy and libtest.
"""
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cargo_admitted import Denied, nested_command

ROWS = {
    "parser": ("parser-check", "parser-build", "parser-clippy", "parser-lib"),
    "dap": ("dap-lsp-build", "dap-core-build", "dap-bin-build", "dap-clippy"),
}


def warnings_anchored_in_parser(stderr):
    lines = stderr.replace("\\", "/").splitlines()
    for index, line in enumerate(lines):
        if line.startswith("warning: "):
            anchor = next((s for s in lines[index + 1:] if s.lstrip().startswith("-->")), "")
            if "crates/perl-parser/src" in anchor:
                return True
    return False


def run(env=None, invoke=subprocess.run, mode="parser"):
    env = dict(os.environ if env is None else env)
    # Admit the entire fixed mode before starting any product work. Preserve
    # live revalidation for each child rather than caching rendered commands.
    for row in ROWS[mode]:
        nested_command(row, env)
    for row in ROWS[mode]:
        # The existing owner alone validates membership, original identities,
        # exact source/tools/config/environment and both private resource roots.
        command, child_env, cwd = nested_command(row, env)
        output = invoke(command, cwd=cwd, env=child_env, capture_output=True,
                        text=True, encoding="utf-8", errors="replace")
        print(output.stdout, end="", flush=True)
        print(output.stderr, end="", file=sys.stderr, flush=True)
        if output.returncode:
            return output.returncode if output.returncode > 0 else 1
        nested_command(row, env)
        if row == "parser-check" and any(message in output.stdout for message in (
                "cannot find value `signature`", "failed to resolve: could not find `tower_lsp`")):
            print("architectural parser compilation diagnostic remains", file=sys.stderr)
            return 1
        if row == "parser-build" and warnings_anchored_in_parser(output.stderr):
            print("perl-parser build contains anchored warnings", file=sys.stderr)
            return 1
    # The existing gate runner validates this invocation's nonzero completed
    # libtest population; an exit zero alone never qualifies parser preparation.
    return 0


if __name__ == "__main__":
    try:
        if sys.argv[1:] not in ([], ["--dap"]):
            raise ValueError("expected no arguments or --dap")
        sys.exit(run(mode="dap" if sys.argv[1:] else "parser"))
    except (Denied, KeyError, TypeError, ValueError, OSError) as error:
        print(f"parser preparation refused: {error}", file=sys.stderr)
        sys.exit(1)
