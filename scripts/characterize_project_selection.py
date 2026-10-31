#!/usr/bin/env python3
"""Linux research instrument for #11266, separate from production discovery.

All filesystem subjects come from a checked manifest. Selection is observed
through the supplied production binary, using independently blocking source.
Assertions characterize current behavior, not desired product semantics.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

import jsonschema

REPO = Path(__file__).resolve().parents[1]
DATA = REPO / "testdata/project-check-selection"
POLICY = REPO / "policy/project-check-selection.v1.json"
SOURCE = REPO / "crates/perl-lsp-rs/src/cli/check_project.rs"
BLOCKING = "my $selection_probe = ;\n"
TIMEOUT_SECONDS = 10
MAXIMUM_FIXTURE_FILES = 256
WALKER_PATH = "crates/perl-lsp-rs/src/cli/check_project.rs"


def digest(path):
    """Bind evidence to exact bytes, without reading unrelated state."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_json(path):
    """Reject duplicate keys instead of silently overwriting evidence."""

    def unique_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate key: {key}")
            result[key] = value
        return result

    return json.loads(
        path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs
    )


def validate_source_commit(commit):
    """Verify the declared Git subject, separately from binary provenance."""
    identity = subprocess.run(
        ["git", "cat-file", "-e", f"{commit}^{{commit}}"],
        cwd=REPO,
        capture_output=True,
        timeout=TIMEOUT_SECONDS,
        check=False,
    )
    if identity.returncode != 0:
        raise ValueError("declared source commit is not an available commit")
    blob = subprocess.run(
        ["git", "show", f"{commit}:{WALKER_PATH}"],
        cwd=REPO,
        capture_output=True,
        timeout=TIMEOUT_SECONDS,
        check=False,
    )
    if blob.returncode != 0 or hashlib.sha256(
        blob.stdout
    ).hexdigest() != digest(SOURCE):
        raise ValueError("declared commit walker differs from observed source")


def literal_report_subjects(case):
    """Describe created path spellings, without filesystem discovery."""
    subjects = {"."}
    for entry in case["entries"]:
        path = Path(entry["path"])
        if "native_name_hex" in entry:
            path = path.with_name(
                os.fsdecode(bytes.fromhex(entry["native_name_hex"]))
            )
        try:
            relative = path.relative_to(case["root"])
        except ValueError:
            continue  # Outside-root and alias subjects are explicitly listed.
        subjects.update(
            display_path(part) for part in [relative, *relative.parents]
        )
    return subjects


def validate_artifacts(policy, manifest, receipt=None):
    """Validate strict schemas, identity and disposition invariants."""
    schema = read_json(DATA / "schema.v1.json")
    for name, document in [
        ("policy", policy),
        ("manifest", manifest),
        ("receipt", receipt),
    ]:
        if document is not None:
            selected = {
                "$schema": schema["$schema"],
                "$defs": schema["$defs"],
                "$ref": f"#/$defs/{name}",
            }
            jsonschema.Draft202012Validator.check_schema(selected)
            jsonschema.Draft202012Validator(selected).validate(document)
    validate_source_commit(manifest["source_commit"])
    ids = [row["id"] for row in policy["rows"]]
    if len(set(ids)) != len(ids):
        raise ValueError("duplicate policy row")
    # Policy identity moves with every load-bearing row (including rationale).
    subject = {
        key: policy[key]
        for key in ("rows", "architecture", "selection", "discovery_budget")
    }
    row_bytes = json.dumps(
        subject, sort_keys=True, separators=(",", ":")
    ).encode()
    if (
        policy["policy_id"]
        != "project_check_selection.v1:"
        + hashlib.sha256(row_bytes).hexdigest()
    ):
        raise ValueError("policy semantics moved without identity movement")
    case_ids = [case["id"] for case in manifest["cases"]]
    if len(set(case_ids)) != len(case_ids):
        raise ValueError("duplicate fixture case")
    for case in manifest["cases"]:
        if len(case["entries"]) > MAXIMUM_FIXTURE_FILES or (
            case["current"]["scanned"] is not None
            and case["current"]["scanned"] > MAXIMUM_FIXTURE_FILES
        ):
            raise ValueError("fixture exceeds admitted logical-file ceiling")
        if (
            Path(case["root"]).is_absolute()
            or ".." in Path(case["root"]).parts
        ):
            raise ValueError("fixture root escapes temporary tree")
        if not set(case["policy_rows"]).issubset(ids):
            raise ValueError("fixture refers to unknown policy row")
        for entry in case["entries"]:
            path = entry["path"]
            if Path(path).is_absolute() or ".." in Path(path).parts:
                raise ValueError("fixture creation escapes temporary tree")
            if "native_name_hex" in entry:
                native = bytes.fromhex(entry["native_name_hex"])
                if (
                    b"/" in native
                    or b"\0" in native
                    or native in (b".", b"..")
                ):
                    raise ValueError(
                        "native fixture name is not one path component"
                    )
            if "target" in entry and entry["kind"] != "symlink":
                if (
                    Path(entry["target"]).is_absolute()
                    or ".." in Path(entry["target"]).parts
                ):
                    raise ValueError("hardlink target escapes temporary tree")
            if entry["kind"] == "symlink":
                # Outside-root targets stay within this owned temporary tree.
                target = Path("/fixture") / path
                resolved = Path(
                    os.path.normpath(target.parent / entry["target"])
                )
                if Path(
                    entry["target"]
                ).is_absolute() or not resolved.is_relative_to("/fixture"):
                    raise ValueError("symlink target escapes temporary tree")
        subjects = set(case["report_subjects"])
        if not literal_report_subjects(case).issubset(subjects):
            raise ValueError("report inventory omits a literal created path")
        if not set(
            case["current"]["selected_display"]
            + case["current"]["unscanned_display"]
        ).issubset(subjects):
            raise ValueError(
                "expected report subject is absent from inventory"
            )
        for subject in subjects:
            if Path(subject).is_absolute() or ".." in Path(subject).parts:
                raise ValueError(
                    "report inventory subject escapes fixture root"
                )
    if receipt is not None:
        if receipt["policy_id"] != policy["policy_id"]:
            raise ValueError("receipt policy identity mismatch")
        if {o["id"] for o in receipt["observations"]} != set(case_ids):
            raise ValueError("receipt fixture coverage mismatch")
        if len(receipt["observations"]) != len(case_ids):
            raise ValueError("duplicate observation")
        if (
            receipt["source_commit"] != manifest["source_commit"]
            or receipt["not_proven"] != manifest["not_proven"]
        ):
            raise ValueError("receipt source or platform limitations mismatch")
        if receipt["architecture"] != policy["architecture"]:
            raise ValueError(
                "receipt architecture differs from selection ruling"
            )
        for observation in receipt["observations"]:
            case = next(
                c for c in manifest["cases"] if c["id"] == observation["id"]
            )
            if observation["policy_rows"] != case["policy_rows"]:
                raise ValueError(
                    "observation lacks explicit policy dispositions"
                )
            if observation["current"] != case["current"]:
                raise ValueError(
                    "observation differs from characterized fixture"
                )


def display_path(path):
    """Match Rust's lossy Path::display for invalid Unix bytes."""
    return os.fsencode(path).decode("utf-8", errors="replace")


def observe(output, root, subjects):
    """Join legacy text to literal fixture subjects; refuse ambiguous joins."""
    stdout = output.stdout.decode("utf-8", errors="replace")
    stderr = output.stderr.decode("utf-8", errors="replace")
    count = re.search(r"^Files scanned: (\d+)$", stdout, flags=re.MULTILINE)
    prefix = display_path(root).rstrip("/") + "/"
    selected = []
    unscanned = []
    section = None
    for line in stdout.splitlines():
        if line == "Parse errors:":
            section = "selected_display"
        elif line.startswith("Paths not scanned:"):
            section = "unscanned_display"
        elif not line:
            section = None
        elif section is not None and (
            line.startswith("  " + prefix)
            or line.startswith("  " + display_path(root) + ": ")
        ):
            matches = [
                subject
                for subject in subjects
                if line.startswith(
                    "  "
                    + (
                        display_path(root)
                        if subject == "."
                        else prefix + subject
                    )
                    + ": "
                )
            ]
            if len(matches) != 1:
                raise AssertionError(
                    "unrecognized or ambiguous fixture path in legacy text"
                )
            observed = selected if section == "selected_display" else unscanned
            if matches[0] not in observed:
                observed.append(matches[0])
    return (
        {
            "selected_display": sorted(selected),
            "scanned": int(count[1]) if count else None,
            "unscanned_display": sorted(unscanned),
            "exit": output.returncode,
        },
        selected,
        stdout,
        stderr,
    )


def run_binary(binary, args, environment):
    """Bound each synchronous CLI invocation; no compiler or daemon."""
    return subprocess.run(
        [str(binary), *args],
        env=environment,
        capture_output=True,
        timeout=TIMEOUT_SECONDS,
        check=False,
    )


def materialize(tree, case, reverse=False):
    """Create literal entries, without walking or classifying the tree."""
    entries = list(reversed(case["entries"])) if reverse else case["entries"]
    for entry in entries:
        if entry["kind"] in ("symlink", "hardlink"):
            continue
        path = tree / entry["path"]
        if "native_name_hex" in entry:
            path = path.parent / os.fsdecode(
                bytes.fromhex(entry["native_name_hex"])
            )
        path.parent.mkdir(parents=True, exist_ok=True)
        if entry["kind"] == "directory":
            path.mkdir(exist_ok=True)
        else:
            path.write_text(entry.get("content", BLOCKING), encoding="utf-8")
    for entry in case["entries"]:
        path = tree / entry["path"]
        if entry["kind"] == "symlink":
            path.parent.mkdir(parents=True, exist_ok=True)
            path.symlink_to(entry["target"])
        elif entry["kind"] == "hardlink":
            path.parent.mkdir(parents=True, exist_ok=True)
            os.link(tree / entry["target"], path)
    if case["root_kind"] == "directory":
        (tree / case["root"]).mkdir(parents=True, exist_ok=True)
    restricted = []
    for entry in case["entries"]:
        if entry.get("unreadable", False):
            path = tree / entry["path"]
            path.chmod(0)
            restricted.append(path)
    return restricted


def characterize(binary, policy, manifest, log_dir=None):
    """Return deterministic evidence for measured cases and explicit limits."""
    validate_artifacts(policy, manifest)
    if sys.platform != "linux" or os.geteuid() == 0:
        raise ValueError(
            "requires non-root Linux; permission/platform rows "
            "otherwise not proven"
        )
    observations = []
    order_changed = []
    with tempfile.TemporaryDirectory(prefix="selection-11266-") as temp:
        work = Path(temp)
        traps = work / "traps"
        traps.mkdir()
        marker = work / "interpreter-invoked"
        trap = (
            '#!/bin/sh\nprintf invoked >> "$SELECTION_EXEC_MARKER"\nexit 97\n'
        )
        for name in ("perl", "perl5", "perl5.40"):
            executable = traps / name
            executable.write_text(trap, encoding="utf-8")
            executable.chmod(0o700)
        environment = os.environ.copy()
        environment["PATH"] = (
            str(traps) + os.pathsep + environment.get("PATH", "")
        )
        environment["SELECTION_EXEC_MARKER"] = str(marker)
        # A positive control demonstrates the PATH trap itself works.
        control = subprocess.run(
            [str(traps / "perl")],
            env=environment,
            capture_output=True,
            timeout=TIMEOUT_SECONDS,
            check=False,
        )
        if control.returncode != 97 or not marker.exists():
            raise AssertionError("interpreter trap positive control failed")
        marker.unlink()
        blocking = work / "blocking.pl"
        blocking.write_text(BLOCKING, encoding="utf-8")
        strict = run_binary(binary, ["--check", str(blocking)], environment)
        if (
            strict.returncode != 1
            or b"error" not in strict.stdout + strict.stderr
        ):
            raise AssertionError(
                "independent blocking-source control "
                "is not revealed by --check"
            )
        for case in manifest["cases"]:
            observed_orders = []
            variants = [False, True] if case["shuffle"] else [False]
            for reverse in variants:
                with tempfile.TemporaryDirectory(
                    dir=work, prefix=case["id"] + "-"
                ) as case_temp:
                    tree = Path(case_temp)
                    restricted = materialize(tree, case, reverse)
                    try:
                        root = tree / case["root"]
                        output = run_binary(
                            binary, ["--check-project", str(root)], environment
                        )
                        current, order, stdout, stderr = observe(
                            output, root, case["report_subjects"]
                        )
                        if current != case["current"]:
                            raise AssertionError(
                                f"CHARACTERIZATION changed for {case['id']}: "
                                f"expected {case['current']!r}, "
                                f"observed {current!r}"
                            )
                        for text in case["stdout_contains"]:
                            if text not in stdout:
                                raise AssertionError(
                                    f"missing evidence {text!r} "
                                    f"in {case['id']}"
                                )
                        for text in case["stderr_contains"]:
                            if text not in stderr:
                                raise AssertionError(
                                    f"missing stderr evidence {text!r} "
                                    f"in {case['id']}"
                                )
                        observed_orders.append(order)
                        if log_dir is not None:
                            log_dir.mkdir(parents=True, exist_ok=True)
                            normalized = stdout.replace(
                                display_path(tree), "<fixture>"
                            )
                            normalized += stderr.replace(
                                display_path(tree), "<fixture>"
                            )
                            variant = "reverse" if reverse else "forward"
                            name = f"{case['id']}-{variant}.txt"
                            (log_dir / name).write_text(
                                normalized, encoding="utf-8"
                            )
                    finally:
                        for path in restricted:
                            path.chmod(0o700)
            if (
                len(observed_orders) == 2
                and observed_orders[0] != observed_orders[1]
            ):
                order_changed.append(case["id"])
            observations.append(
                {
                    "id": case["id"],
                    "policy_rows": case["policy_rows"],
                    "current": case["current"],
                }
            )
        # A parseable BEGIN side effect creates the marker if executed.
        begin = work / "begin.pl"
        begin.write_text(
            'BEGIN { open my $fh, ">", $ENV{SELECTION_EXEC_MARKER}; '
            'print $fh "executed"; close $fh; }\n1;\n',
            encoding="utf-8",
        )
        source_control = run_binary(
            binary, ["--check", str(begin)], environment
        )
        if source_control.returncode != 0 or marker.exists():
            raise AssertionError("BEGIN no-execution control failed")
    return {
        "schema": "project_check_selection_decision.v1",
        "policy_id": policy["policy_id"],
        "architecture": policy["architecture"],
        "source_commit": manifest["source_commit"],
        "source_sha256": digest(SOURCE),
        "binary_sha256": digest(binary),
        "fixture_sha256": digest(DATA / "fixtures.v1.json"),
        "platform": "linux-nonroot",
        "observations": observations,
        "ordering": (
            "Unsorted production traversal; only observed sets are "
            "canonicalized in this receipt. Raw order is research evidence, "
            "never desired semantics."
        ),
        "instrument": {
            "blocking_control": "--check exit 1",
            "begin_control": "--check exit 0; marker absent",
            "path_trap_control": (
                "fake interpreter exit 97; marker present, then cleared"
            ),
            "timeout_seconds_per_process": TIMEOUT_SECONDS,
            "maximum_fixture_files": MAXIMUM_FIXTURE_FILES,
        },
        "not_proven": manifest["not_proven"],
    }, order_changed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        type=Path,
        help="immutable copy of admitted production perllsp",
    )
    receipts = parser.add_mutually_exclusive_group()
    receipts.add_argument("--write-receipt", type=Path)
    receipts.add_argument("--check-receipt", type=Path)
    parser.add_argument("--log-dir", type=Path)
    args = parser.parse_args()
    policy = read_json(POLICY)
    manifest = read_json(DATA / "fixtures.v1.json")
    validate_artifacts(policy, manifest)
    if args.binary is None:
        receipt = read_json(DATA / "linux-decision.v1.json")
        validate_artifacts(policy, manifest, receipt)
        if receipt["source_sha256"] != digest(SOURCE) or receipt[
            "fixture_sha256"
        ] != digest(DATA / "fixtures.v1.json"):
            raise ValueError(
                "checked decision is stale against "
                "production source or fixtures"
            )
        print(
            f"validated {len(policy['rows'])} policy rows and "
            f"{len(manifest['cases'])} decision cases"
        )
        return
    binary = args.binary.resolve(strict=True)
    if not binary.is_file():
        raise ValueError("binary must be a regular file")
    receipt, order_changed = characterize(
        binary, policy, manifest, args.log_dir
    )
    validate_artifacts(policy, manifest, receipt)
    encoded = (
        json.dumps(receipt, ensure_ascii=True, sort_keys=True, indent=2) + "\n"
    )
    if (
        args.check_receipt is not None
        and args.check_receipt.read_bytes() != encoded.encode("utf-8")
    ):
        raise ValueError(
            "second-generation receipt differs; "
            "characterize and review the changed subject"
        )
    if args.write_receipt is not None:
        args.write_receipt.write_text(encoded, encoding="utf-8")
    print(
        f"characterized {len(receipt['observations'])} cases; "
        f"creation-order changes: {order_changed}"
    )


if __name__ == "__main__":
    main()
