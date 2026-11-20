"""Check the tracked Python self-test inventory, or execute one local disposition."""

import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
DIRECTORY = Path("scripts/tests")
MANIFEST = ROOT / ".ci/python-self-tests.tsv"
EXCEPTIONS = {
    "qualify_admitted_clippy.py": "tool",
    "test_measure_cargo_storage.py": "owned-19530",
    "test_measure_cargo_storage_boundaries.py": "owned-19530",
    "test_review_feedback.py": "owned-18154",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", metavar="FILENAME", help="execute one named local test")
    args = parser.parse_args()
    output = subprocess.check_output(
        ["git", "ls-files", "-z", "--", DIRECTORY], cwd=ROOT
    )
    files = set()
    for entry in output.split(b"\0"):
        path = Path(os.fsdecode(entry))
        if path.parent == DIRECTORY and path.suffix == ".py":
            files.add(path.name)
    if not files:
        parser.exit(1, "No tracked top-level Python files found in scripts/tests\n")

    dispositions = {}
    for line in MANIFEST.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != 2 or fields[1] != EXCEPTIONS.get(fields[0], "local"):
            parser.exit(1, f"Invalid disposition row: {line!r}\n")
        name, disposition = fields
        if name in dispositions:
            parser.exit(1, f"Duplicate disposition: {name}\n")
        dispositions[name] = disposition

    errors = [f"Missing execution disposition: {name}" for name in sorted(files - dispositions.keys())]
    errors.extend(f"Stale disposition: {name}" for name in sorted(dispositions.keys() - files))
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")

    if args.run is None:
        for name in sorted(files):
            print(f"{name}\t{dispositions[name]}")
        return 0
    if dispositions.get(args.run) != "local":
        parser.error(f"{args.run!r} has no local execution disposition")
    # Script operands preserve sibling imports. Some suites also import scripts.*.
    env = os.environ.copy()
    env["PYTHONPATH"] = os.pathsep.join(filter(None, [str(ROOT), env.get("PYTHONPATH")]))
    result = subprocess.run([sys.executable, DIRECTORY / args.run], cwd=ROOT, env=env)
    return result.returncode if result.returncode >= 0 else 128 - result.returncode


if __name__ == "__main__":
    sys.exit(main())
