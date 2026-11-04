"""Child-only Git isolation for commands that select an explicit repository."""

import os
import subprocess


def isolated_git_env() -> dict[str, str]:
    # Git owns this list; rev-parse's list mode does not inspect a repository.
    names = subprocess.check_output(
        ["git", "rev-parse", "--local-env-vars"], text=True
    ).split()
    return {
        key: value
        for key, value in os.environ.items()
        if key not in names
        and not key.startswith(("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"))
    }
