"""Child-only Git isolation for commands that select an explicit repository."""

import os
from functools import lru_cache
import shutil
import subprocess


@lru_cache(maxsize=8)
def _local_git_names(identity: tuple) -> frozenset[str]:
    # Git owns this list; rev-parse's list mode does not inspect a repository.
    # Cache only the schema, not an environment snapshot. The identity key
    # invalidates it when PATH selects another Git or that executable changes.
    # Exceptions are not cached: discovery failure must never imply no selectors.
    # Execute the observed binary, rather than resolving PATH a second time.
    names = frozenset(subprocess.check_output(
        ["git", "rev-parse", "--local-env-vars"], executable=identity[0], text=True
    ).split())
    if not names:
        raise RuntimeError("git returned an empty local environment-variable schema")
    return names


def isolated_git_env() -> dict[str, str]:
    executable = shutil.which("git")
    if executable is None:
        raise FileNotFoundError("git executable not found on PATH")
    path = os.path.realpath(executable)
    stat = os.stat(path)
    identity = (path, stat.st_dev, stat.st_ino, stat.st_size,
                stat.st_mtime_ns, stat.st_ctime_ns)
    names = _local_git_names(identity)
    return {
        key: value
        for key, value in os.environ.items()
        if key not in names
        and not key.startswith(("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"))
    }
