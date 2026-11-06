#!/usr/bin/env python3
"""Bounded native evidence for one WSI-owned, as-is resource-rejection pilot.

This is an adapter, not an ownership registry, model translator, proof service or
replacement for Cargo admission. The implementation producer is callable only
from the existing hosted qualification job; local Cargo uses cargo-admitted.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import tomllib

ROOT = Path(__file__).resolve().parents[2]
SPEC = "crates/perl-workspace/tests/formal/workspace_index_resource_rejection.binding.json"
SCRIPT = "scripts/ci/workspace_index_model.py"
OWNER = "scripts/cargo_admitted.py"
sys.path.insert(0, str(ROOT / "scripts"))
from cargo_admitted import ClippyTree, Denied, child_discovery_capability

DIGEST_PREFIX = "sha256:v1:"
FORMAL = "crates/perl-workspace/tests/formal/"
PROPERTY = "resource-rejection-is-partial-and-nonpublishing"
AUTHORITY_ROWS = ("WSI-COORD-002", "WSI-COORD-004", "WSI-RES-001", "WSI-INSTR-001")
CONFORMANCE_CASES = (
    "first_resource_rejection_observer_routes_partial_and_preserves_accepted_file",
    "quiescent_observations_do_not_repeat_degradation_or_block_existing_file_update",
)
INVARIANTS = ("TypeOK", "AcceptedPreserved", "NoRejectedPublication",
              "NoFullQueryAfterRejection", "SingleDrain")
NEGATIVE_CONTROLS = (
    ("bypass", "NoFullQueryAfterRejection"),
    ("publish", "NoRejectedPublication"),
    ("repeat", "SingleDrain"),
    ("evict", "AcceptedPreserved"),
)
JAVA_INJECTION = ("JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS", "CLASSPATH",
                  "TLA_LIBRARY", "TLA_LIBRARY_PATH")
TLC_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"


def json_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def digest_bytes(value):
    return DIGEST_PREFIX + hashlib.sha256(value).hexdigest()


def file_hash(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def source_file(root, relative):
    path = Path(relative)
    if path.is_absolute() or ".." in path.parts or not path.parts:
        raise ValueError("binding input is not a repository-relative path")
    resolved = (root / path).resolve(strict=True)
    if not resolved.is_relative_to(root.resolve()) or not resolved.is_file():
        raise ValueError("binding input escapes the repository or is not a file")
    return resolved


def read_json(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON key: " + key)
            result[key] = value
        return result
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique)


def records(root, paths):
    if len(paths) != len(set(paths)):
        raise ValueError("duplicate binding input")
    return [
        {"path": path, "sha256": file_hash(source_file(root, path))}
        for path in sorted(paths)
    ]


def load_spec(root):
    spec = read_json(source_file(root, SPEC))
    if spec["schema_id"] != "perllsp.workspace-index-model.v1" or spec["behavior"] != "as_is":
        raise ValueError("this adapter handles only the as-is WSI pilot")
    if (spec["machine_id"] != "perllsp.workspace-index-lifecycle"
            or spec["authority_ledger"] != "policy/workspace-index-lifecycle-propositions.v1.tsv"
            or spec["authority_rows"] != list(AUTHORITY_ROWS)):
        raise ValueError("the pilot must join the existing WSI authority")
    prop = read_json(source_file(root, spec["property"]))
    if spec["property_id"] != PROPERTY or prop["id"] != PROPERTY:
        raise ValueError("property identity differs from the property artifact")
    if prop["invariants"] != list(INVARIANTS):
        raise ValueError("claimed invariant denominator differs from the fixed pilot")
    expected_controls = [
        {"config": FORMAL + "workspace_index_resource_rejection_" + suffix + ".cfg",
         "invariant": invariant}
        for suffix, invariant in NEGATIVE_CONTROLS
    ]
    if spec["negative_controls"] != expected_controls:
        raise ValueError("negative-control denominator differs from the fixed pilot")
    validate_config(root, spec["config"], None)
    for suffix, invariant in NEGATIVE_CONTROLS:
        validate_config(root, FORMAL + "workspace_index_resource_rejection_" + suffix + ".cfg", suffix)
    model_closure(root, spec)
    pin = read_json(source_file(root, spec["toolchain"]))
    if (pin["java_options"] != ["-Xms32m", "-Xmx256m", "-XX:+UseSerialGC"]
            or pin["tlc_options"] != ["-tool", "-workers", "1"]
            or pin["timeout_seconds"] != 30 or pin["max_output_bytes"] != 1048576):
        raise ValueError("native tool options or bounds differ from the fixed pilot")
    paths = [
        spec[key] for key in ("model", "config", "property", "assumptions", "toolchain", "authority_ledger")
    ]
    paths += [row["config"] for row in spec["negative_controls"]]
    paths += spec["implementation_sources"] + spec["adapter_sources"]
    records(root, sorted(set(paths)))
    if (spec["conformance_target"] != "workspace_index_resource_model"
            or spec["conformance_cases"] != list(CONFORMANCE_CASES)):
        raise ValueError("the production denominator differs from the fixed pilot")
    return spec


def validate_config(root, path, fault):
    # A fixed pilot grammar, not a partial general TLC parser. In particular,
    # CONSTRAINT, VIEW, symmetry reductions and hidden invariant removal refuse.
    text = source_file(root, path).read_text(encoding="utf-8")
    tokens = " ".join(line.split("\\*", 1)[0] for line in text.splitlines()).split()
    expected = [
        "SPECIFICATION", "Spec", "CONSTANTS", "Kept", "=", '"kept"',
        "Rejected", "=", '"rejected"',
        "ConsumeRejection", "=", "FALSE" if fault == "bypass" else "TRUE",
        "ClearRejection", "=", "FALSE" if fault == "repeat" else "TRUE",
        "PublishRejected", "=", "TRUE" if fault == "publish" else "FALSE",
        "DropAccepted", "=", "TRUE" if fault == "evict" else "FALSE",
    ]
    expected += (["INVARIANT", dict(NEGATIVE_CONTROLS)[fault]] if fault
                 else ["INVARIANTS", *INVARIANTS])
    expected += ["CHECK_DEADLOCK", "FALSE"]
    if tokens != expected:
        raise ValueError("native configuration weakens or changes the fixed pilot: " + path)


def model_closure(root, spec):
    path = source_file(root, spec["model"])
    text = "\n".join(line.split("\\*", 1)[0] for line in path.read_text(encoding="utf-8").splitlines())
    imports = re.findall(r"(?m)^EXTENDS\s+([^\n]+)", text)
    if imports != ["Naturals, FiniteSets"] or re.search(r"\bINSTANCE\b", text):
        raise ValueError("undeclared imported-model closure")
    pin = read_json(source_file(root, spec["toolchain"]))
    if pin["version"] != "1.7.4" or pin["sha256"] != TLC_SHA256 or pin["size_bytes"] != 2274532:
        raise ValueError("TLC pin differs from the reviewed pilot")
    return {
        "module": {"path": spec["model"], "sha256": file_hash(path)},
        "standard_imports": ["Naturals", "FiniteSets"],
        "standard_library": {"artifact": "tla2tools.jar", "sha256": pin["sha256"]},
    }


def implementation_preimage(root, spec, source_subject):
    if (not isinstance(source_subject, dict)
            or not re.fullmatch(r"[0-9a-f]{40}", source_subject.get("tree", ""))):
        raise ValueError("implementation closure requires the native tracked-source tree")
    return {
        "scope": "clean tracked repository tree plus direct correspondence anchors",
        "git_tree": source_subject["tree"],
        "direct_anchors": records(root, spec["implementation_sources"]),
        "external_dependency_identity": "Cargo.lock in the tracked tree; not a vendored-source census",
        "exclusions": ["untracked files", "unobserved build-environment configuration"],
    }


def binding(root, spec, evidence_kind, runtime, source_subject=None):
    model_only = evidence_kind == "model_check"
    if not model_only and evidence_kind != "implementation_conformance":
        raise ValueError("unsupported evidence kind")
    toolchain = {
        "inputs": records(root, [spec["toolchain"], SCRIPT, OWNER]),
        "observed_runtime": runtime,
    }
    scope = [SPEC, spec["config"]] + [row["config"] for row in spec["negative_controls"]]
    result = {
        "schema_id": "proof.state-machine-evidence-binding.v1",
        "machine_id": spec["machine_id"],
        "property_id": spec["property_id"],
        "evidence_kind": evidence_kind,
        "model_digest": digest_bytes(json_bytes(model_closure(root, spec))),
        "property_digest": digest_bytes(source_file(root, spec["property"]).read_bytes()),
        "assumptions_digest": digest_bytes(json_bytes(records(
            root, [spec["assumptions"], spec["authority_ledger"]]
        ))),
        "scope_digest": digest_bytes(json_bytes(records(root, scope))),
        "toolchain_digest": digest_bytes(json_bytes(toolchain)),
    }
    if not model_only:
        result["implementation_digest"] = digest_bytes(json_bytes(
            implementation_preimage(root, spec, source_subject)
        ))
        result["adapter_digest"] = digest_bytes(json_bytes(records(root, spec["adapter_sources"])))
    return result


def require_current_binding(receipt, expected, source_subject):
    if receipt.get("state_machine") != expected:
        raise ValueError("stale model/property/assumptions/scope/toolchain/source/adapter binding")
    if receipt.get("source_subject") != source_subject:
        raise ValueError("stale native source subject")
    # Structural equality is NOT provider-native proof validation.


def run_owned(command, cwd, output, name, seconds=30, limit=1024 * 1024, clean_java=False):
    """Capture a bounded command inside the existing Linux child-tree owner."""
    stdout_path, stderr_path = output / (name + ".stdout"), output / (name + ".stderr")
    result = {"command": command, "exit_code": None, "error": None, "settlement": None}
    tree = process = None
    started = time.monotonic()
    streams = []
    try:
        child_discovery_capability()
        tree = ClippyTree()
        with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
            tree.binding = True
            try:
                environment = {**os.environ, "RUSTUP_AUTO_INSTALL": "0"}
                if clean_java:
                    for key in JAVA_INJECTION:
                        environment.pop(key, None)
                process = subprocess.Popen(
                    command, cwd=cwd, env=environment,
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
                )
                tree.track(process.pid, process)
            finally:
                tree.binding = False
            if tree.cancelled:
                raise KeyboardInterrupt("interrupted while binding child")
            streams = [process.stdout, process.stderr]
            total = 0
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ, stdout)
                selector.register(process.stderr, selectors.EVENT_READ, stderr)
                while selector.get_map() or process.returncode is None:
                    if time.monotonic() - started >= seconds:
                        raise TimeoutError("whole-command deadline exceeded")
                    for key, _ in selector.select(0.05):
                        block = os.read(key.fd, 65536)
                        if not block:
                            selector.unregister(key.fileobj)
                            continue
                        room = max(0, limit - total)
                        key.data.write(block[:room])
                        total += len(block)
                        if total > limit:
                            raise ValueError("native output exceeded capture bound")
                    mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
                    try:
                        tree.reap_adopted()
                        process.poll()
                    finally:
                        signal.pthread_sigmask(signal.SIG_SETMASK, mask)
            result["exit_code"] = process.returncode
    except BaseException as error:
        result["error"] = type(error).__name__ + ": " + str(error)
    finally:
        if tree is not None:
            settlement = tree.finish(stop=result["error"] is not None, timeout=1, grace=0.1)
            if not settlement["tree_settled"]:
                # An escaped descendant is a failed invocation even if cleanup
                # can subsequently settle it. Never accept leader exit as closure.
                result["error"] = result["error"] or "descendants outlived native completion"
                settlement = tree.finish(stop=True, timeout=9, grace=0.1)
            result["settlement"] = settlement
            if not settlement["tree_settled"] or settlement["cancelled"]:
                result["error"] = result["error"] or "owned child scope did not settle cleanly"
        for stream in streams:
            stream.close()
    result["duration_ms"] = int((time.monotonic() - started) * 1000)
    for role, path in [("stdout", stdout_path), ("stderr", stderr_path)]:
        if path.exists():
            result[role] = {"path": path.name, "sha256": file_hash(path), "bytes": path.stat().st_size}
    return result


def require_run(result, output, exit_code=0):
    if result["error"] or result["exit_code"] != exit_code:
        raise ValueError("native invocation did not complete as required: " + str(result))
    settlement = result.get("settlement")
    if not settlement or not settlement["tree_settled"] or settlement["cancelled"]:
        raise ValueError("native child-tree completion is unproved")
    return (output / result["stdout"]["path"]).read_text(encoding="utf-8", errors="strict")


def classify_tlc(result, output, invariant=None):
    text = require_run(result, output, 12 if invariant else 0)
    frames = {}
    messages = native_messages(text)
    for code, severity, body in messages:
        frames.setdefault(code, []).append(body.strip())
    if len(frames.get(2185, [])) != 1 or len(frames.get(2186, [])) != 1:
        raise ValueError("missing native TLC start/finish frames")
    if invariant:
        expected = "Invariant " + invariant + " is violated."
        if (frames.get(2110) != [expected] or 2193 in frames or 2121 not in frames
                or len(frames.get(2217, [])) < 2
                or any(severity == 2 or (severity == 1 and code not in (2110, 2121))
                       for code, severity, _ in messages)):
            raise ValueError("negative control did not produce the named invariant counterexample")
        return {"result": "expected_counterexample", "invariant": invariant}
    if (len(frames.get(2193, [])) != 1 or len(frames.get(2199, [])) != 1
            or 2110 in frames or any(severity in (1, 2) for _, severity, _ in messages)):
        raise ValueError("TLC did not report a complete successful search")
    stats = re.fullmatch(
        r"([\d,]+) states generated,\s*([\d,]+) distinct states found,\s*([\d,]+) states left on queue\.",
        frames[2199][-1],
    )
    if not stats:
        raise ValueError("missing native TLC state denominator")
    generated, distinct, frontier = [int(value.replace(",", "")) for value in stats.groups()]
    if generated < 2 or distinct < 2 or frontier != 0:
        raise ValueError("empty or incomplete native TLC exploration")
    return {"result": "model_check_passed", "generated_states": generated, "distinct_states": distinct}


def native_messages(text):
    messages, current = [], None
    for line in text.splitlines():
        if line.startswith("@!@!@STARTMSG "):
            header = re.fullmatch(r"@!@!@STARTMSG (\d+):([0-4]) @!@!@", line)
            if current is not None or not header:
                raise ValueError("nested or malformed native frame")
            current = [int(header[1]), int(header[2]), []]
        elif line.startswith("@!@!@ENDMSG "):
            end = re.fullmatch(r"@!@!@ENDMSG (\d+) @!@!@", line)
            if current is None or not end or int(end[1]) != current[0]:
                raise ValueError("unmatched or malformed native frame end")
            messages.append((current[0], current[1], "\n".join(current[2])))
            current = None
        elif "@!@!@" in line:
            raise ValueError("malformed native frame marker")
        elif current is not None:
            current[2].append(line)
    if current is not None:
        raise ValueError("truncated native frame")
    return messages


def source_subject(root, output, label):
    values = {}
    for key, args in [("commit", ["rev-parse", "HEAD"]), ("tree", ["rev-parse", "HEAD^{tree}"])]:
        run = run_owned(["git", "-C", str(root), *args], root, output, label + "-" + key, 10)
        values[key] = require_run(run, output).strip()
    run = run_owned(["git", "-C", str(root), "diff", "--quiet", "HEAD", "--"], root, output, label + "-clean", 10)
    require_run(run, output)
    spec = load_spec(root)
    paths = [SPEC] + [spec[key] for key in ("model", "config", "property", "assumptions", "toolchain", "authority_ledger")]
    paths += [row["config"] for row in spec["negative_controls"]]
    paths += spec["implementation_sources"] + spec["adapter_sources"]
    paths += root_cargo_configs(root)
    run = run_owned(["git", "-C", str(root), "ls-files", "--error-unmatch", "--", *sorted(set(paths))],
                    root, output, label + "-tracked", 10)
    require_run(run, output)
    return values


def resolved_local_closure(metadata, root):
    packages = {row["id"]: row for row in metadata["packages"]}
    nodes = {row["id"]: row for row in metadata["resolve"]["nodes"]}
    matches = [key for key, row in packages.items() if Path(row["manifest_path"]).resolve() == root / "crates/perl-workspace/Cargo.toml"]
    if len(matches) != 1:
        raise ValueError("Cargo did not resolve exactly one production package")
    pending, seen, directories = matches[:], set(), set()
    while pending:
        key = pending.pop()
        if key in seen:
            continue
        seen.add(key)
        node, package = nodes[key], packages[key]
        pending.extend(item["pkg"] for item in node["deps"])
        if package.get("source") is None:
            directory = Path(package["manifest_path"]).resolve().parent
            if not directory.is_relative_to(root):
                raise ValueError("local dependency is outside the checkout")
            directories.add(directory.relative_to(root).as_posix() + "/")
    return directories


def select_needed(root, base, output):
    """Query Cargo's conservative all-feature graph; unknown selection runs."""
    if not base or not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40:
        return True, "unknown base"
    try:
        run = run_owned(["git", "-C", str(root), "diff", "--name-only", "-z", "--no-renames", base, "HEAD"], root, output, "scope-diff", 10)
        paths = [path for path in require_run(run, output).split("\0") if path]
        direct = ("crates/perl-workspace/", "scripts/ci/workspace_index_model", "scripts/ci/test_workspace_index_model")
        exact = {SCRIPT, OWNER, SPEC, "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".github/workflows/ci.yml",
                 "policy/workspace-index-lifecycle-propositions.v1.tsv", "xtask/tests/workspace_index_lifecycle_ledger.rs"}
        if any(path in exact or path.startswith(direct) or path.startswith(".cargo/") or path.endswith("/Cargo.toml") for path in paths):
            return True, "model/adapter/owner/build input changed"
        if all(path.endswith(".md") or path.startswith("docs/") for path in paths):
            return False, "documentation-only outside pilot inputs"
        run = run_owned(["cargo", "metadata", "--locked", "--format-version=1", "--all-features"],
                        root, output, "scope-cargo", 30, 8 * 1024 * 1024)
        directories = resolved_local_closure(json.loads(require_run(run, output)), root)
        if any(path.startswith(tuple(directories)) for path in paths):
            return True, "resolved local production/test dependency changed"
        if all(path.startswith("crates/") for path in paths):
            return False, "other packages outside conservative resolved closure"
        return True, "unknown non-package input"
    except (KeyError, OSError, ValueError, Denied):
        return True, "scope instrument unavailable or incomplete"


def model_check(root, spec, jar, output, runs):
    pin = read_json(source_file(root, spec["toolchain"]))
    if jar.stat().st_size != pin["size_bytes"] or file_hash(jar) != pin["sha256"]:
        raise ValueError("TLC jar does not match the reviewed immutable pin")
    # Execute a verified private copy, never a caller-owned mutable jar path.
    copied_jar = output / "tla2tools.jar"
    shutil.copyfile(jar, copied_jar)
    if copied_jar.stat().st_size != pin["size_bytes"] or file_hash(copied_jar) != pin["sha256"]:
        raise ValueError("TLC jar changed while capturing the private checker")
    copied_jar.chmod(0o400)
    java = shutil.which("java")
    if not java:
        raise ValueError("Java is unavailable; model check NOT_PROVEN")
    java = str(Path(java).resolve())
    version = run_owned([java, "-version"], root, output, "java-version", 10, clean_java=True)
    runs.append(version)
    require_run(version, output)
    runtime = {
        "java_executable_sha256": file_hash(Path(java)),
        "java_version_sha256": file_hash(output / version["stderr"]["path"]),
        "java_version_stdout_sha256": file_hash(output / version["stdout"]["path"]),
        "tlc_jar_sha256": file_hash(copied_jar),
        "python": sys.version,
        "removed_environment_keys": list(JAVA_INJECTION),
    }
    before = binding(root, spec, "model_check", runtime)
    checks = [{"config": spec["config"], "invariant": None}, *spec["negative_controls"]]
    results = []
    for index, case in enumerate(checks):
        metadir = output / ("states-" + str(index))
        metadir.mkdir()
        # Exact byte copies in an otherwise empty directory prevent an
        # untracked local Naturals.tla/FiniteSets.tla from shadowing the jar.
        inputs = output / ("inputs-" + str(index))
        inputs.mkdir()
        model_path = inputs / Path(spec["model"]).name
        config_path = inputs / Path(case["config"]).name
        shutil.copyfile(source_file(root, spec["model"]), model_path)
        shutil.copyfile(source_file(root, case["config"]), config_path)
        command = [
            java, *pin["java_options"], "-cp", str(copied_jar), "tlc2.TLC",
            *pin["tlc_options"], "-metadir", str(metadir), "-config",
            str(config_path), str(model_path),
        ]
        run = run_owned(command, inputs, output, "tlc-" + str(index),
                        pin["timeout_seconds"], pin["max_output_bytes"], clean_java=True)
        runs.append(run)
        results.append(classify_tlc(run, output, case.get("invariant")))
        if file_hash(copied_jar) != pin["sha256"]:
            raise ValueError("private TLC checker changed during execution")
    if binding(root, spec, "model_check", runtime) != before:
        raise ValueError("model binding changed during native execution")
    return before, {"checks": results, "runtime": runtime, "model_closure": model_closure(root, spec)}


def root_cargo_configs(root):
    paths = []
    for relative in (".cargo/config", ".cargo/config.toml"):
        path = root / relative
        if path.is_symlink():
            raise ValueError("root Cargo configuration must be a tracked regular file")
        if path.exists():
            source_file(root, relative)
            paths.append(relative)
    return paths


def conformance_environment(root, environment):
    # One admitted hosted profile, not another Cargo environment/config resolver.
    forbidden = {
        "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS",
        "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC_BOOTSTRAP",
        "RUSTUP_TOOLCHAIN", "CARGO_TARGET_DIR",
    }
    prefixes = ("CARGO_BUILD_", "CARGO_PROFILE_", "CARGO_TARGET_", "CARGO_ALIAS_")
    selected = sorted(key for key in environment if key in forbidden or key.startswith(prefixes))
    if selected:
        raise ValueError("unowned Cargo/compiler environment selectors: " + ", ".join(selected))
    # The pinned dtolnay/rust-toolchain action exports this owned CI profile.
    if "CARGO_INCREMENTAL" in environment and environment["CARGO_INCREMENTAL"] != "0":
        raise ValueError("unowned Cargo/compiler environment selectors: CARGO_INCREMENTAL")
    cargo_home = Path(environment.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve()
    external = [cargo_home / "config", cargo_home / "config.toml"]
    external += [parent / ".cargo" / name for parent in root.parents for name in ("config", "config.toml")]
    if any(path.is_file() for path in external) or (root / ".cargo/config.local.toml").exists():
        raise ValueError("unowned external or local Cargo configuration")
    config_paths = root_cargo_configs(root)
    for relative in config_paths:
        config = tomllib.loads(source_file(root, relative).read_text(encoding="utf-8"))
        if "include" in config:
            raise ValueError("Cargo configuration includes are outside this fixed profile")
    relevant = ("CARGO_", "RUST", "CC", "CXX", "CFLAGS", "CXXFLAGS", "LDFLAGS", "AR", "PERL_")
    public_identity = {}
    for key, value in sorted(environment.items()):
        if not key.startswith(relevant):
            continue
        # Authentication is not build semantics. Do not retain even a digest of
        # low-entropy credentials in an uploadable receipt.
        if any(part in key.upper() for part in ("TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH", "KEY")):
            continue
        if re.search(r"://[^/\s]*@", value):
            raise ValueError("credential-bearing build selector is unsupported: " + key)
        public_identity[key] = digest_bytes(value.encode("utf-8"))
    return {
        "profile": "existing hosted qualification; no external compiler/Cargo selectors",
        "environment_digests": public_identity,
        "root_cargo_configs": records(root, config_paths),
        "excluded_environment": "nonsemantic authentication inputs; no credential hashes retained",
    }


def implementation_check(root, spec, output, runs, source_subject):
    # This consumes the installed qualification job's already-owned build route.
    # It is not a new way to bypass local cargo-admitted preflight.
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise ValueError("implementation capture is hosted-only; use cargo-admitted for local proof")
    runtime = {"python": sys.version, "build_environment": conformance_environment(root, os.environ)}
    for tool, args in [("cargo", ["--version"]), ("rustc", ["-vV"])]:
        run = run_owned([tool, *args], root, output, tool + "-version", 10)
        runs.append(run)
        runtime[tool] = require_run(run, output)
    before = binding(root, spec, "implementation_conformance", runtime, source_subject)
    command = ["cargo", "test", "-p", "perl-workspace", "--test", spec["conformance_target"],
               "--locked", "--no-run", "--message-format=json"]
    build = run_owned(command, root, output, "conformance-build", 180, 8 * 1024 * 1024)
    runs.append(build)
    messages = [json.loads(line) for line in require_run(build, output).splitlines() if line.strip()]
    executables = {
        row["executable"] for row in messages
        if row.get("reason") == "compiler-artifact" and row.get("executable")
        and row["target"]["name"] == spec["conformance_target"] and "test" in row["target"]["kind"]
        and Path(row["target"]["src_path"]).resolve() == root / "crates/perl-workspace/tests/workspace_index_resource_model.rs"
    }
    if len(executables) != 1:
        raise ValueError("Cargo did not emit exactly the selected production test executable")
    executable = Path(executables.pop())
    if executable.is_symlink() or not executable.is_file():
        raise ValueError("selected test executable is not a regular owned build result")
    copied = output / "conformance-test"
    shutil.copy2(executable, copied)
    binary_digest = file_hash(copied)
    copied.chmod(0o500)
    test = run_owned([str(copied), "--test-threads=1", "--color", "never"],
                     root, output, "conformance-test", 30)
    runs.append(test)
    text = require_run(test, output)
    for name in spec["conformance_cases"]:
        if not re.search(r"^test " + re.escape(name) + r" \.\.\. ok$", text, re.MULTILINE):
            raise ValueError("production case did not run and pass: " + name)
    if not re.search(r"^test result: ok\. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;", text, re.MULTILINE):
        raise ValueError("production test denominator differs from the binding")
    if (file_hash(copied) != binary_digest
            or conformance_environment(root, os.environ) != runtime["build_environment"]
            or binding(root, spec, "implementation_conformance", runtime, source_subject) != before):
        raise ValueError("native executable or source/adapter changed during conformance")
    return before, {"runtime": runtime, "executable_sha256": binary_digest,
                    "executed_cases": spec["conformance_cases"],
                    "implementation_preimage": implementation_preimage(root, spec, source_subject)}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["check", "select", "model", "conformance"])
    parser.add_argument("--jar", type=Path)
    parser.add_argument("--base", default="")
    args = parser.parse_args(argv)
    spec = load_spec(ROOT)
    if args.mode == "check":
        # Validate bindings only. No native-check receipt or green proof claim.
        binding(ROOT, spec, "model_check", {"status": "not_executed"})
        records(ROOT, spec["implementation_sources"] + spec["adapter_sources"])
        print("binding inputs present; native proof NOT_PROVEN")
        return 0
    parent = ROOT / "target/receipts/workspace-index-model"
    parent.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix=args.mode + "-", dir=parent))
    if args.mode == "select":
        needed, reason = select_needed(ROOT, args.base, output)
        print("needed=" + str(needed).lower())
        print("reason=" + reason)
        return 0
    # The exact fresh path is published before any native work can fail.
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write("receipt_dir=" + str(output) + "\n")
    receipt = {
        "schema_id": "perllsp.workspace-index-native-evidence.v1",
        "status": "NOT_PROVEN", "runs": [], "claim_ceiling": "scoped model or component evidence; not native cargo-proof qualification",
    }
    try:
        subject = source_subject(ROOT, output, "before")
        receipt["source_subject"] = subject
        if args.mode == "model":
            if not args.jar:
                raise ValueError("--jar is required; this adapter never downloads a checker")
            bound, details = model_check(ROOT, spec, args.jar, output, receipt["runs"])
        else:
            bound, details = implementation_check(ROOT, spec, output, receipt["runs"], subject)
        receipt["state_machine"], receipt["native"] = bound, details
        if source_subject(ROOT, output, "after") != subject:
            raise ValueError("native source subject changed during execution")
        receipt["status"] = "PASSED_SCOPED"
    except (OSError, ValueError, KeyError, Denied) as error:
        receipt["error"] = type(error).__name__ + ": " + str(error)
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(str(output / "receipt.json"))
    return 0 if receipt["status"] == "PASSED_SCOPED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
