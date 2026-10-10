#!/usr/bin/env python3
"""Select existing admission roots for PR Smoke; allocate nothing, grant no lease."""
import os
from pathlib import Path
import sys
import subprocess

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from cargo_admitted import Denied, resource_plan


def select(env):
    # The admission owner remains the sole path authority. Selection is not
    # capacity admission and must never synthesize CARGO_ADMITTED_RESOURCES.
    _, _, paths = resource_plan(env)
    values = {"CARGO_TARGET_DIR": str(paths["target"]),
              "CARGO_BUILD_BUILD_DIR": str(paths["build"])}
    for value in values.values():
        if "\n" in value or "\r" in value:
            raise ValueError("resource path cannot be represented in GitHub's environment file")
    return "".join(f"{name}={value}\n" for name, value in values.items())


def main():
    try:
        result = select(dict(os.environ))
        with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8", newline="") as stream:
            stream.write(result)
        return 0
    except (Denied, KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print("PR Smoke resource selection refused: " + str(error), file=sys.stderr)
        return 75


if __name__ == "__main__":
    sys.exit(main())
