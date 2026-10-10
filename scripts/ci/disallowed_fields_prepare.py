#!/usr/bin/env python3
"""One finite compiler fixture under the existing Cargo admission owner.

Preparation copies only the checked-in fixture, without running tools. The
owned direct and test modes reuse nested_command; no lease/executor is added.
"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import cargo_admitted as owner

LIMIT = 4 * 1024 * 1024


def prepare(env=None):
    env = dict(os.environ if env is None else env)
    worktree, slot, paths = owner.resource_plan(env)
    if (slot / "cargo-active").exists():
        raise owner.Denied("cannot prepare fixture while the Cargo owner lease is active")
    root = owner.native_path(str(paths["temp"] / "disallowed-fields-17479"))
    template = worktree / ".spec/17479-nested-admission/disallowed-fields-fixture"
    for directory in (root, root / "src", root / "target", root / "build"):
        owner.native_path(str(directory)).mkdir(parents=True, exist_ok=True)
    for name in owner.DISALLOWED_FILES:
        source, destination = template / name, owner.native_path(str(root / name))
        subject = owner.file_subject(source)
        if subject["file_identity"][2] > owner.BUDGET_FILE_LIMIT:
            raise owner.Denied("fixture template exceeds bounded input")
        data = source.read_bytes()
        if owner.file_subject(source) != subject:
            raise owner.Denied("fixture template changed while copying")
        if destination.exists():
            if destination.stat().st_size > owner.BUDGET_FILE_LIMIT or destination.read_bytes() != data:
                raise owner.Denied("existing generated fixture differs; preserve it for diagnosis")
        else:
            with destination.open("xb") as output:
                output.write(data)
    binding = owner.disallowed_fixture(worktree, paths)
    print(json.dumps({"prepared": binding["cwd"], "compiler_launched": False}))


def diagnostic_success(stdout, returncode, root):
    """A deliberate current fixture error, not arbitrary compiler/tool failure."""
    if returncode <= 0 or len(stdout.encode("utf-8")) > LIMIT:
        raise owner.Denied("fixture must fail compilation with bounded current JSON")
    found, terminal = False, False
    source = root / "src/lib.rs"
    for line in stdout.splitlines():
        if not line or terminal:
            raise owner.Denied("blank or post-terminal fixture JSON")
        try:
            event = json.loads(line)
        except ValueError as error:
            raise owner.Denied("malformed fixture compiler JSON") from error
        if not isinstance(event, dict):
            raise owner.Denied("fixture compiler JSON must contain objects")
        if event.get("reason") == "build-finished":
            if event.get("success") is not False:
                raise owner.Denied("fixture compilation did not report unsuccessful terminal")
            terminal = True
        if event.get("reason") != "compiler-message":
            continue
        message = event.get("message", {})
        if not isinstance(message, dict):
            raise owner.Denied("invalid fixture diagnostic")
        code = message.get("code")
        if (not isinstance(code, dict) or code.get("code") != "clippy::disallowed_fields"
                or message.get("level") != "error"):
            continue
        target = event.get("target")
        if (event.get("manifest_path") != str(root / "Cargo.toml")
                or not isinstance(target, dict) or target.get("src_path") != str(source)):
            raise owner.Denied("fixture diagnostic belongs to another manifest/source")
        spans = message.get("spans", [])
        if not isinstance(spans, list) or not any(
                isinstance(span, dict) and span.get("is_primary") is True
                and span.get("file_name") in ("src/lib.rs", str(source))
                and span.get("line_start") == 4 for span in spans):
            raise owner.Denied("fixture diagnostic lacks the current field-access span")
        found = True
    if not found or not terminal:
        raise owner.Denied("missing current fixture error or unsuccessful terminal compilation")
    return True


def fixture(env=None, invoke=subprocess.run):
    env = dict(os.environ if env is None else env)
    command, child, cwd = owner.nested_command(owner.DISALLOWED_FIXTURE_ROW, env)
    descriptor = json.loads(env["CARGO_ADMITTED_RESOURCES"])
    snapshot, _ = owner.bounded_json(descriptor["nested_snapshot"]["path"])
    binding = snapshot["plan"]["disallowed_fields_fixture"]
    if owner.file_subject(Path(sys.executable).resolve(strict=True)) != binding["python"]:
        raise owner.Denied("fixture interpreter differs from admitted native interpreter")
    result = invoke(command, env=child, cwd=cwd, capture_output=True, text=True,
                    encoding="utf-8", errors="strict")
    owner.nested_command(owner.DISALLOWED_FIXTURE_ROW, env)
    diagnostic_success(result.stdout, result.returncode, cwd)
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    return result.returncode


def owning_test(env=None, invoke=subprocess.run):
    env = dict(os.environ if env is None else env)
    for row in (owner.DISALLOWED_FIXTURE_ROW, owner.DISALLOWED_TEST_ROW):
        owner.nested_command(row, env)
    command, child, cwd = owner.nested_command(owner.DISALLOWED_TEST_ROW, env)
    result = invoke(command, env=child, cwd=cwd, capture_output=True, text=True,
                    encoding="utf-8", errors="strict")
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    owner.nested_command(owner.DISALLOWED_TEST_ROW, env)
    if result.returncode:
        return result.returncode if result.returncode > 0 else 1
    lines = result.stdout.splitlines()
    if ([line for line in lines if re.fullmatch(r"running [0-9]+ tests?", line)] != ["running 1 test"]
            or [line for line in lines if re.fullmatch(r"test .* \.\.\. (ok|FAILED|ignored.*)", line)]
            != ["test " + owner.DISALLOWED_TEST + " ... ok"]
            or len([line for line in lines if line.startswith("test result:")]) != 1
            or not any(re.fullmatch(r"test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s", line) for line in lines)):
        raise owner.Denied("missing exact current owning fixture-test pass")
    capture_owning_artifact(result.stdout, env)
    return 0


def capture_owning_artifact(stdout, env):
    """Preserve the actual Cargo-reported harness before releasing its owner."""
    if len(stdout.encode('utf-8')) > 64*1024*1024:
        raise owner.Denied("owning-harness compiler output exceeds bounded reader")
    descriptor = json.loads(env["CARGO_ADMITTED_RESOURCES"])
    snapshot, _ = owner.bounded_json(descriptor["nested_snapshot"]["path"])
    worktree = Path(descriptor["worktree"])
    matches, terminals = [], []
    for line in stdout.splitlines():
        if not line.startswith('{'):
            continue
        event = json.loads(line)
        if not isinstance(event, dict):
            raise owner.Denied("invalid owning-harness compiler JSON")
        if event.get('reason') == 'build-finished':
            terminals.append(event.get('success') is True)
        target, profile = event.get('target'), event.get('profile')
        if (event.get('reason') == 'compiler-artifact'
                and event.get('manifest_path') == str(worktree/'xtask/Cargo.toml')
                and isinstance(target,dict) and target.get('name') == 'xtask'
                and target.get('kind') == ['bin']
                and target.get('src_path') == str(worktree/'xtask/src/main.rs')
                and isinstance(profile,dict) and profile.get('test') is True):
            matches.append(event)
    if len(matches) != 1 or terminals != [True]:
        raise owner.Denied("missing/duplicate current owning-harness artifact or terminal")
    event = matches[0]
    path = owner.native_path(event['executable'])
    if (path.parent != Path(descriptor['resources']['build'])/'debug/deps'
            or not path.name.startswith('xtask-') or str(path) not in event.get('filenames', [])
            or not os.access(path, os.X_OK)):
        raise owner.Denied("owning harness has wrong executable/root")
    owner.nested_command(owner.DISALLOWED_TEST_ROW, env)
    original = owner.file_subject(path)
    copy = Path(descriptor['resources']['temp']) / ('disallowed-fields-owning-' + str(os.getpid()))
    with path.open('rb') as source, copy.open('xb') as destination:
        shutil.copyfileobj(source, destination, 1024*1024)
    copied = owner.file_subject(copy)
    if copied['sha256'] != original['sha256'] or owner.file_subject(path) != original:
        raise owner.Denied("owning harness changed while capturing evidence")
    copy.chmod(0o444)
    owner.nested_command(owner.DISALLOWED_TEST_ROW, env)
    evidence = {'schema_version':1, 'tested_source':snapshot['plan']['source']['head'],
                'toolchain_pin':snapshot['plan']['toolchain'].get('pin'),
                'artifact':original, 'copy':owner.file_subject(copy), 'cargo_artifact':event,
                'snapshot':descriptor['nested_snapshot'], 'owner_process':descriptor['owner_process'],
                'lease':descriptor['lease'], 'lease_identity':descriptor['lease_identity'],
                'lease_marker':descriptor['lease_marker'], 'marker_identity':descriptor['marker_identity'],
                'copied_under_original_live_owner':True, 'named_test':owner.DISALLOWED_TEST}
    with copy.with_suffix('.json').open('x',encoding='utf-8') as record:
        json.dump(evidence,record)
    print('Owning fixture harness preserved: '+json.dumps({'path':str(copy),'sha256':copied['sha256']}),flush=True)


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["--prepare"]:
            prepare()
        elif sys.argv[1:] == ["--direct"]:
            fixture()
        elif sys.argv[1:] == ["--fixture"]:
            sys.exit(fixture())  # Rust retains its deliberate-failure oracle.
        elif sys.argv[1:] == ["--test"]:
            sys.exit(owning_test())
        else:
            raise owner.Denied("expected --prepare, --direct, --fixture or --test")
    except (owner.Denied, KeyError, TypeError, ValueError, OSError) as error:
        print("disallowed-fields fixture refused: " + str(error), file=sys.stderr)
        sys.exit(75)
