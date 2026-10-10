#!/usr/bin/env python3
"""Issue146 repository obligations, sequential under the caller's Cargo lease.

Run through the existing gate runner before integration libtest. This owns no
new resource roots, admission policy, cleanup, retries, or subprocess supervisor.
The root must admit the entire gate workload, including strict Clippy and libtest.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
COMMANDS = (
    ("check", "--package", "perl-parser", "--message-format", "json", "--locked", "--offline"),
    ("build", "--package", "perl-parser", "--locked", "--offline"),
    ("clippy", "--package", "perl-parser", "--locked", "--offline", "--", "-D", "warnings"),
    ("test", "--package", "perl-parser", "--lib", "--locked", "--offline"),
)


def admitted_resources(env, root=ROOT):
    """Refuse ambient raw Cargo, stale leases and overlapping output resources."""
    receipt = json.loads(env.get("CARGO_ADMITTED_RESOURCES", "null"))
    if not isinstance(receipt, dict) or Path(receipt["worktree"]).resolve() != root.resolve():
        raise ValueError("preparation requires this worktree's live cargo-admitted resources")
    resources = receipt["resources"]
    paths = []
    for key, variable in (("target", "CARGO_TARGET_DIR"), ("build", "CARGO_BUILD_BUILD_DIR")):
        path = Path(resources[key])
        if not path.is_absolute() or not path.is_dir() or path.resolve() != path:
            raise ValueError(f"{key} is not an existing native canonical resource")
        if env.get(variable) != str(path):
            raise ValueError(f"{variable} differs from admitted resource")
        paths.append(path)
    target, build = paths
    if target == build or target in build.parents or build in target.parents:
        raise ValueError("final and intermediate resources overlap")
    if target.parent != build.parent or target.name != "target" or build.name != "build":
        raise ValueError("resources are not the caller's private worktree pair")
    if target.parent.parent.name != "worktrees":
        raise ValueError("resources are not worktree-private")
    lease = Path(receipt["lease"])
    marker = Path(receipt["lease_marker"])
    if lease.parent != target.parent.parent.parent or marker.parent != lease:
        raise ValueError("resources and lease have different ownership domains")
    if (not lease.is_dir() or lease.resolve() != lease or not marker.is_dir()
            or marker.resolve() != marker or marker.is_symlink()):
        raise ValueError("preparation lease is no longer owned")
    return receipt


def warnings_anchored_in_parser(stderr):
    lines = stderr.replace("\\", "/").splitlines()
    for index, line in enumerate(lines):
        if line.startswith("warning: "):
            anchor = next((s for s in lines[index + 1:] if s.lstrip().startswith("-->")), "")
            if "crates/perl-parser/src" in anchor:
                return True
    return False


def run(env=None, invoke=subprocess.run):
    env = dict(os.environ if env is None else env)
    for args in COMMANDS:
        receipt = admitted_resources(env)
        # Same final AND intermediate roots throughout, before outer libtest.
        # The existing runner bounds/owns this complete sequential command tree.
        resources = receipt["resources"]
        command = ["cargo", "--config", "unstable.unstable-options=false",
                   "--config", "build.build-dir=" + json.dumps(resources["build"]),
                   "--config", 'build.rustc-wrapper=""',
                   "--config", 'build.rustc-workspace-wrapper=""',
                   args[0], "--target-dir", resources["target"], *args[1:]]
        output = invoke(command, cwd=ROOT, env=env, capture_output=True,
                        text=True, encoding="utf-8", errors="replace")
        print(output.stdout, end="", flush=True)
        print(output.stderr, end="", file=sys.stderr, flush=True)
        if output.returncode:
            return output.returncode if output.returncode > 0 else 1
        admitted_resources(env)
        if args[0] == "check" and any(message in output.stdout for message in (
                "cannot find value `signature`", "failed to resolve: could not find `tower_lsp`")):
            print("architectural parser compilation diagnostic remains", file=sys.stderr)
            return 1
        if args[0] == "build" and warnings_anchored_in_parser(output.stderr):
            print("perl-parser build contains anchored warnings", file=sys.stderr)
            return 1
    # The existing gate runner validates this invocation's nonzero completed
    # libtest population; an exit zero alone never qualifies parser preparation.
    return 0


if __name__ == "__main__":
    try:
        sys.exit(run())
    except (KeyError, TypeError, ValueError, OSError) as error:
        print(f"parser preparation refused: {error}", file=sys.stderr)
        sys.exit(1)
