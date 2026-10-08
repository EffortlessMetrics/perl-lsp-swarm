#!/usr/bin/env python3
"""Command-local Cargo runner for Windows xtask gates.

Cargo supplies the executable and its runtime environment. Execute a byte-verified
private copy so nested Cargo can replace its own output on Windows. The enclosing
admitted command retains ownership of Cargo, this runner, and every descendant.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib


def native_host():
    result = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True)
    hosts = [line.removeprefix("host: ") for line in result.stdout.splitlines()
             if line.startswith("host: ")]
    if len(hosts) != 1 or "windows" not in hosts[0]:
        raise ValueError("prepared xtask runner requires one native Windows host")
    return hosts[0]


def check_configuration(cwd, env, host):
    """Refuse customization we cannot preserve; never overwrite a user runner."""
    if env.get("CARGO_BUILD_TARGET", host) != host:
        raise ValueError("cross-target xtask runner is not supported")
    if any(key.startswith("CARGO_TARGET_") and key.endswith("_RUNNER")
           for key in env):
        raise ValueError("existing Cargo runner environment is not supported")
    directories = [cwd, *cwd.parents]
    config_dirs = [directory / ".cargo" for directory in directories]
    config_dirs.append(Path(env.get("CARGO_HOME", Path.home() / ".cargo")))
    for directory in dict.fromkeys(config_dirs):
        for name in ("config", "config.toml"):
            path = directory / name
            if not path.exists():
                continue
            with path.open("rb") as stream:
                config = tomllib.load(stream)
            if "include" in config:
                raise ValueError(f"included Cargo configuration is not supported: {path}")
            target = config.get("build", {}).get("target", host)
            if target != host:
                raise ValueError(f"cross-target Cargo configuration is not supported: {path}")
            if any("runner" in settings for settings in config.get("target", {}).values()):
                raise ValueError(f"existing Cargo runner configuration is not supported: {path}")
            configured_env = config.get("env", {})
            if any(key == "CARGO_BUILD_TARGET" or
                   (key.startswith("CARGO_TARGET_") and key.endswith("_RUNNER"))
                   for key in configured_env):
                raise ValueError(f"Cargo target environment configuration is ambiguous: {path}")


def runner_configuration():
    host = native_host()
    check_configuration(Path.cwd().resolve(), os.environ, host)
    runner = [sys.executable, str(Path(__file__).resolve())]
    return f"target.{host}.runner = {json.dumps(runner)}"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def run_copy(executable, arguments):
    source = Path(executable).resolve(strict=True)
    if source.name.lower() != "xtask.exe" or not source.is_file():
        raise ValueError("Cargo must supply the xtask executable")
    directory = Path(tempfile.mkdtemp(prefix="xtask-gates-runner-"))
    copied = directory / source.name
    # No fallback to the mapped Cargo output, including on copy/hash failure.
    before = digest(source)
    shutil.copyfile(source, copied)
    if digest(source) != before or digest(copied) != before:
        raise ValueError(f"xtask executable changed during handoff; retained {directory}")
    print(f"prepared xtask runner sha256={before}", file=sys.stderr, flush=True)
    status = subprocess.call([str(copied), *arguments])
    # Only remove our one verified file after its child exited. If another owned
    # descendant still maps it, Windows refuses removal and evidence is retained.
    if digest(copied) != before:
        raise ValueError(f"prepared xtask runner changed; retained {directory}")
    try:
        copied.unlink()
        directory.rmdir()
    except OSError as error:
        print(f"prepared xtask runner retained at {directory}: {error}", file=sys.stderr)
    return status


def main(arguments):
    if os.name != "nt":
        raise ValueError("prepared xtask runner is Windows-only")
    if arguments == ["--runner-config"]:
        print(runner_configuration())
        return 0
    if len(arguments) < 2 or arguments[1] != "gates":
        raise ValueError("prepared xtask runner accepts only Cargo's xtask gates invocation")
    return run_copy(arguments[0], arguments[1:])


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"prepared xtask runner refused: {error}", file=sys.stderr)
        raise SystemExit(1)
