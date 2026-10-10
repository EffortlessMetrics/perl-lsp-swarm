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
sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import cargo_admitted as owner

LIMIT = 4 * 1024 * 1024


def prepared_output(row, env):
    if owner.ROUTED_MEASUREMENTS not in env:
        return None
    # Current canonical projection only. A partial/stale selector fails in
    # replay; it must never fall back to another compiler measurement.
    import routed_nested_prepare
    return routed_nested_prepare.replay(row, env)


def prepare(env=None, label="disallowed-fields"):
    env = dict(os.environ if env is None else env)
    worktree, slot, paths = owner.resource_plan(env)
    if (slot / "cargo-active").exists():
        raise owner.Denied("cannot prepare fixture while the Cargo owner lease is active")
    if label not in ("disallowed-fields", "lock-union"):
        raise owner.Denied("unknown fixed fixture")
    root = owner.native_path(str(paths["temp"] / (label + "-17479")))
    template = worktree / (".spec/17479-nested-admission/" + label + "-fixture")
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
    binding = (owner.disallowed_fixture if label == "disallowed-fields" else owner.lock_union_fixture)(worktree, paths)
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
    result = prepared_output(owner.DISALLOWED_FIXTURE_ROW, env)
    if result is None:
        result = invoke(command, env=child, cwd=cwd, capture_output=True, text=True,
                        encoding="utf-8", errors="strict")
    owner.nested_command(owner.DISALLOWED_FIXTURE_ROW, env)
    diagnostic_success(result.stdout, result.returncode, cwd)
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    return result.returncode


def owning_test(env=None, invoke=subprocess.run, fixture_row=None, test_row=None, test_name=None,
                copy_prefix="disallowed-fields-owning"):
    env = dict(os.environ if env is None else env)
    fixture_row = fixture_row or owner.DISALLOWED_FIXTURE_ROW
    test_row = test_row or owner.DISALLOWED_TEST_ROW
    test_name = test_name or owner.DISALLOWED_TEST
    selections = {
        owner.DISALLOWED_TEST_ROW: (owner.DISALLOWED_FIXTURE_ROW, owner.DISALLOWED_TEST, "disallowed-fields-owning"),
        owner.LOCK_UNION_TEST_ROW: (owner.LOCK_UNION_ROW, owner.LOCK_UNION_TEST, "lock-union-owning"),
        owner.PREPARATION_CONTROL_ROW: (owner.LOCK_UNION_ROW, owner.PREPARATION_CONTROL_TEST, "preparation-control-owning"),
        owner.LOCK_PARTITION_TEST_ROW: (owner.LOCK_FIXTURE_ROWS, owner.LOCK_PARTITION_TESTS, "lock-remaining-owning"),
        owner.JSONRPC_TEST_ROW: (owner.JSONRPC_LOCK_ROW, owner.JSONRPC_TESTS, "jsonrpc-owning"),
        owner.PARSER_OCCUPANCY_TEST_ROW: (owner.PARSER_OCCUPANCY_ROW, owner.PARSER_OCCUPANCY_TESTS, "parser-occupancy-owning"),
    }
    if selections.get(test_row) != (fixture_row, test_name, copy_prefix):
        raise owner.Denied("unsupported owning-test selection")
    fixture_rows = fixture_row if isinstance(fixture_row, tuple) else (fixture_row,)
    for row in (*fixture_rows, test_row):
        owner.nested_command(row, env)
    command, child, cwd = owner.nested_command(test_row, env)
    result = invoke(command, env=child, cwd=cwd, capture_output=True, text=True,
                    encoding="utf-8", errors="strict")
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    owner.nested_command(test_row, env)
    if result.returncode:
        return result.returncode if result.returncode > 0 else 1
    lines = result.stdout.splitlines()
    names = (test_name,) if isinstance(test_name, str) else test_name
    count = len(names)
    expected_results = ["test " + name + " ... ok" for name in sorted(names)]
    if ([line for line in lines if re.fullmatch(r"running [0-9]+ tests?", line)] != [f"running {count} test" + ("s" if count != 1 else "")]
            or [line for line in lines if re.fullmatch(r"test .* \.\.\. (ok|FAILED|ignored.*)", line)] != expected_results
            or len([line for line in lines if line.startswith("test result:")]) != 1
            or not any(re.fullmatch(rf"test result: ok\. {count} passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s", line) for line in lines)):
        raise owner.Denied("missing exact current owning fixture-test pass")
    if test_row == owner.JSONRPC_TEST_ROW:
        descriptor = json.loads(env["CARGO_ADMITTED_RESOURCES"])
        snapshot, _ = owner.bounded_json(descriptor["nested_snapshot"]["path"])
        owner.jsonrpc_measurements(snapshot["plan"], descriptor)
    capture_owning_artifact(result.stdout, env, test_row, test_name, copy_prefix)
    return 0


def capture_owning_artifact(stdout, env, test_row=None, test_name=None,
                            copy_prefix="disallowed-fields-owning"):
    """Preserve the actual Cargo-reported harness before releasing its owner."""
    if len(stdout.encode('utf-8')) > 64*1024*1024:
        raise owner.Denied("owning-harness compiler output exceeds bounded reader")
    descriptor = json.loads(env["CARGO_ADMITTED_RESOURCES"])
    test_row = test_row or owner.DISALLOWED_TEST_ROW
    test_name = test_name or owner.DISALLOWED_TEST
    if (test_row, test_name, copy_prefix) not in (
            (owner.DISALLOWED_TEST_ROW, owner.DISALLOWED_TEST, "disallowed-fields-owning"),
            (owner.LOCK_UNION_TEST_ROW, owner.LOCK_UNION_TEST, "lock-union-owning"),
            (owner.PREPARATION_CONTROL_ROW, owner.PREPARATION_CONTROL_TEST, "preparation-control-owning"),
            (owner.LOCK_PARTITION_TEST_ROW, owner.LOCK_PARTITION_TESTS, "lock-remaining-owning"),
            (owner.JSONRPC_TEST_ROW, owner.JSONRPC_TESTS, "jsonrpc-owning"),
            (owner.PARSER_OCCUPANCY_TEST_ROW, owner.PARSER_OCCUPANCY_TESTS, "parser-occupancy-owning")):
        raise owner.Denied("unsupported owning-artifact selection")
    snapshot, _ = owner.bounded_json(descriptor["nested_snapshot"]["path"])
    worktree = Path(descriptor["worktree"])
    integrations = {owner.JSONRPC_TEST_ROW: ("xtask/Cargo.toml", "xtask/tests/lsp_jsonrpc_dependency_probe.rs", "lsp_jsonrpc_dependency_probe"),
                    owner.PARSER_OCCUPANCY_TEST_ROW: ("crates/perl-parser/Cargo.toml", "crates/perl-parser/tests/collapsible_if_occupancy.rs", "collapsible_if_occupancy")}
    manifest_path, source_path, target_name = integrations.get(test_row, ("xtask/Cargo.toml", "xtask/src/main.rs", "xtask"))
    target_kind = ["test"] if test_row in integrations else ["bin"]
    target_source, manifest = worktree / source_path, worktree / manifest_path
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
                and event.get('manifest_path') == str(manifest)
                and isinstance(target,dict) and target.get('name') == target_name
                and target.get('kind') == target_kind
                and target.get('src_path') == str(target_source)
                and isinstance(profile,dict) and profile.get('test') is True):
            matches.append(event)
    if len(matches) != 1 or terminals != [True]:
        raise owner.Denied("missing/duplicate current owning-harness artifact or terminal")
    event = matches[0]
    path = owner.native_path(event['executable'])
    if (path.parent != Path(descriptor['resources']['build'])/'debug/deps'
            or not path.name.startswith(target_name + '-') or str(path) not in event.get('filenames', [])
            or not os.access(path, os.X_OK)):
        raise owner.Denied("owning harness has wrong executable/root")
    owner.nested_command(test_row, env)
    original = owner.file_subject(path)
    copy = Path(descriptor['resources']['temp']) / (copy_prefix + '-' + str(os.getpid()))
    with path.open('rb') as source, copy.open('xb') as destination:
        shutil.copyfileobj(source, destination, 1024*1024)
    copied = owner.file_subject(copy)
    if copied['sha256'] != original['sha256'] or owner.file_subject(path) != original:
        raise owner.Denied("owning harness changed while capturing evidence")
    copy.chmod(0o444)
    owner.nested_command(test_row, env)
    evidence = {'schema_version':1, 'tested_source':snapshot['plan']['source']['head'],
                'toolchain_pin':snapshot['plan']['toolchain'].get('pin'),
                'artifact':original, 'copy':owner.file_subject(copy), 'cargo_artifact':event,
                'snapshot':descriptor['nested_snapshot'], 'owner_process':descriptor['owner_process'],
                'lease':descriptor['lease'], 'lease_identity':descriptor['lease_identity'],
                'lease_marker':descriptor['lease_marker'], 'marker_identity':descriptor['marker_identity'],
                'copied_under_original_live_owner':True}
    evidence['named_test' if isinstance(test_name, str) else 'named_tests'] = test_name
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
