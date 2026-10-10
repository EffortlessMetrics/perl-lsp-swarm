#!/usr/bin/env python3
"""Exact-source perllsp handoff under the existing nested admission owner.

This adapter is deliberately not activated in gate policy yet. It retains the
existing build then routed compile/runtime shape; every leaf belongs to the
outer finite plan. No new lease, allocator, descendant supervisor or fallback.
"""
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import cargo_admitted as owner

HANDOFF = "CARGO_ADMITTED_PERLLSP_HANDOFF"
PYTHON = "CARGO_ADMITTED_PYTHON"
LIMIT = 64 * 1024 * 1024


def context(env):
    # Reuse live membership/source/tool/config/roots/original-marker checks.
    owner.nested_command("perllsp-build", env)
    descriptor = json.loads(env["CARGO_ADMITTED_RESOURCES"])
    snapshot, _ = owner.bounded_json(descriptor["nested_snapshot"]["path"])
    return {"descriptor": descriptor, "plan": snapshot["plan"],
            "platform": sys.platform, "profile": "debug",
            "validator": owner.file_subject(Path(__file__)),
            "python": owner.file_subject(Path(sys.executable).resolve(strict=True))}


def artifact_path(message, binding):
    descriptor = binding["descriptor"]
    workspace = Path(descriptor["worktree"])
    target = message.get("target", {})
    profile = message.get("profile", {})
    if not isinstance(target, dict) or not isinstance(profile, dict):
        raise owner.Denied("invalid compiler artifact target/profile")
    if (message.get("manifest_path") != str(workspace / "crates/perllsp/Cargo.toml")
            or target.get("name") != "perllsp" or target.get("kind") != ["bin"]
            or target.get("src_path") != str(workspace / "crates/perllsp/src/main.rs")
            or profile.get("test") is not False):
        return None
    path = Path(message["executable"]) if isinstance(message.get("executable"), str) else None
    suffix = ".exe" if binding["platform"] == "win32" else ""
    # Select Cargo's actual reported executable, then constrain its ownership
    # and mode. Never manufacture the path from a target-directory guess.
    if (not isinstance(message.get("filenames"), list) or path is None
            or not path.is_absolute() or path.name != "perllsp" + suffix
            or path.parent != Path(descriptor["resources"]["target"]) / binding["profile"]
            or str(path) not in message.get("filenames", [])):
        raise owner.Denied("perllsp compiler artifact has wrong executable/profile/root")
    return path


def capture(stdout, env):
    binding = context(env)
    if len(stdout.encode("utf-8")) > LIMIT:
        raise owner.Denied("perllsp artifact log exceeds bounded reader")
    matches, terminal = [], []
    finished = False
    for line in stdout.splitlines():
        if not line.startswith("{"):
            continue
        try:
            message = json.loads(line)
        except ValueError as error:
            raise owner.Denied("malformed compiler artifact JSON") from error
        if not isinstance(message, dict):
            raise owner.Denied("invalid compiler artifact message")
        reason = message.get("reason")
        if finished:
            raise owner.Denied("compiler JSON after terminal build message")
        if reason == "compiler-artifact":
            path = artifact_path(message, binding)
            if path is not None:
                matches.append((path, message))
        if reason == "build-finished":
            terminal.append(message.get("success") is True)
            finished = True
    if len(matches) != 1 or terminal != [True]:
        raise owner.Denied("missing/duplicate perllsp artifact or unsuccessful terminal build")
    path, message = matches[0]
    receipt = {"schema_version": 1, "binding": binding,
               "artifact": owner.file_subject(path), "cargo_artifact": message}
    # The live owner survives all consumers. File identity and digest supplement
    # successful current compilation; they do not substitute for its subject.
    location = Path(binding["descriptor"]["resources"]["temp"]) / ("perllsp-handoff-" + str(os.getpid()) + ".json")
    with location.open("x", encoding="utf-8") as output:
        json.dump(receipt, output)
    child = dict(env)
    child.update({HANDOFF: json.dumps(owner.file_subject(location)),
                  PYTHON: binding["python"]["path"], "PERL_LSP_BIN": str(path)})
    validate(child, "debug")
    return child


def validate(env, profile, probe=subprocess.run):
    if profile not in ("debug", "release"):
        raise owner.Denied("unknown consumer profile")
    try:
        subject = json.loads(env[HANDOFF])
        receipt, current_subject = owner.bounded_json(subject["path"])
        if (current_subject != subject or type(receipt["schema_version"]) is not int
                or receipt["schema_version"] != 1):
            raise owner.Denied("perllsp handoff changed or unsupported")
        binding = context(env)
        if receipt["binding"] != binding or binding["profile"] != profile:
            raise owner.Denied("perllsp handoff source/platform/profile/ownership mismatch")
        if env.get(PYTHON) != binding["python"]["path"]:
            raise owner.Denied("perllsp validator interpreter changed")
        path = artifact_path(receipt["cargo_artifact"], binding)
        if path is None or env.get("PERL_LSP_BIN") != str(path):
            raise owner.Denied("perllsp executable handoff mismatch")
        if owner.file_subject(path) != receipt["artifact"] or path.is_symlink():
            raise owner.Denied("perllsp executable mutated or symlinked")
        if sys.platform != "win32" and not os.access(path, os.X_OK):
            raise owner.Denied("perllsp artifact is not executable")
        # Detect invalid executable format/permissions/loaders before accepting
        # a resolver candidate. Version output is not behavioral gate proof.
        result = probe([str(path), "--version"], env=dict(env), capture_output=True, timeout=10)
        if result.returncode != 0:
            raise owner.Denied("perllsp executable version probe failed")
        if owner.file_subject(path) != receipt["artifact"] or context(env) != binding:
            raise owner.Denied("perllsp ownership/artifact changed during probe")
        return path
    except (KeyError, TypeError, ValueError, OSError, subprocess.TimeoutExpired) as error:
        raise owner.Denied("invalid/stale/unspawnable perllsp handoff") from error


def run(mode, env=None, invoke=subprocess.run):
    env = dict(os.environ if env is None else env)
    row = {"--compile": "routed-compile", "--runtime": "routed-runtime"}[mode]
    owner.nested_command(row, env)  # whole-mode admission before any build
    command, child, cwd = owner.nested_command("perllsp-build", env)
    output = invoke(command, env=child, cwd=cwd, capture_output=True,
                    text=True, encoding="utf-8", errors="strict")
    print(output.stdout, end="", flush=True)
    print(output.stderr, end="", file=sys.stderr, flush=True)
    if output.returncode:
        return output.returncode if output.returncode > 0 else 1
    child = capture(output.stdout, env)
    command, child, cwd = owner.nested_command(row, child)
    validate(child, "debug")
    # Inherited output preserves the existing gate's libtest evidence parser.
    status = invoke(command, env=child, cwd=cwd).returncode
    if status:
        return status if status > 0 else 1
    validate(child, "debug")
    owner.nested_command(row, child)
    return 0


if __name__ == "__main__":
    try:
        args = sys.argv[1:]
        if len(args) == 2 and args[0] == "--resolve":
            print(validate(dict(os.environ), args[1]))
        elif args in (["--compile"], ["--runtime"]):
            sys.exit(run(args[0]))
        else:
            raise owner.Denied("expected --compile, --runtime or --resolve PROFILE")
    except (owner.Denied, KeyError, TypeError, ValueError, OSError) as error:
        print("perllsp preparation refused: " + str(error), file=sys.stderr)
        sys.exit(1)
