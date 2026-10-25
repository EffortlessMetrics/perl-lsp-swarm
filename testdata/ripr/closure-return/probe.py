#!/usr/bin/env python3
"""Bounded #18352 fixture oracle and real-producer replay; never emits facts itself."""

import argparse
import difflib
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent
CASES = ("direct", "bound", "conditional", "evaluated", "instances")
CLASSES = "files,owners,changes,tests,oracles,relations,dynamic_boundaries,limitations,provenance"
INITIALIZERS = {
    "direct": "my $n = $changed ? 5 : 40;",
    "bound": "my $n = 5;",
    "conditional": "return $pick ? sub { 5 } : sub { 40 };",
    "evaluated": "return eval 'sub { 5 }';",
    "instances": "my $n = $changed ? 5 : 40;",
}


def run(command, cwd):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=30)
    return {"argv": command, "exit": result.returncode,
            "stdout": result.stdout, "stderr": result.stderr}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def runtime():
    receipt = {"perl": run(["perl", "-e", 'print "$^V\\n"'], HERE), "runs": []}
    baseline = run(["prove", "-Ilib", *[f"t/{case}.t" for case in CASES]], HERE)
    require(baseline["exit"] == 0 and "Tests=10" in baseline["stdout"],
            f"fixture baseline did not execute all 10 assertions: {baseline}")
    receipt["runs"].append({"subject": "committed fixture", **baseline})
    # Controlled mutations live in an owned temporary copy, never in the fixture.
    for case in ("direct", "bound"):
        with tempfile.TemporaryDirectory(prefix="closure-runtime-") as temporary:
            root = Path(temporary) / "fixture"
            shutil.copytree(HERE / "lib", root / "lib")
            shutil.copytree(HERE / "t", root / "t")
            source = root / "lib/ClosureReturn.pm"
            original = source.read_text()
            needle = INITIALIZERS[case]
            require(original.count(needle) == 1, "mutation anchor is not unique")
            source.write_text(original.replace(needle, needle.replace("5", "6")))
            positive = run(["prove", "-Ilib", f"t/{case}.t"], root)
            require(positive["exit"] == 1 and "Failed 3/3 subtests" in positive["stdout"],
                    f"mutation was not killed by the three invocation forms: {positive}")
            receipt["runs"].append({"subject": f"{case} initializer 5 -> 6", **positive})
            if case == "direct":
                negative = run(["prove", "-Ilib", "t/instances.t"], root)
                require(negative["exit"] == 0 and "Tests=1" in negative["stdout"],
                        f"other-instance refusal control did not survive: {negative}")
                receipt["runs"].append({"subject": "same mutation, other-instance oracle", **negative})
    return receipt


def summarize(packet):
    """Projection for inspection only; original producer bytes remain in packet.json."""
    return {key: packet[key] for key in (
        "schema_version", "producer", "packet_status", "packet_fingerprint",
        "owners", "changes", "tests", "oracles", "relations", "dynamic_boundaries", "limitations")}


def replay(producer, output):
    producer = producer.resolve(strict=True)
    # Refuse overwrite: a previous receipt/output remains independently inspectable.
    output.mkdir(parents=True, exist_ok=False)
    source = (HERE / "lib/ClosureReturn.pm").read_text()
    receipt = {"producer_path": str(producer),
               "producer_sha256": hashlib.sha256(producer.read_bytes()).hexdigest(), "runs": []}
    for case in CASES:
        directory = output / case
        root = directory / "fixture"
        (root / "lib").mkdir(parents=True)
        (root / "t").mkdir()
        (root / "lib/ClosureReturn.pm").write_text(source)
        shutil.copyfile(HERE / f"t/{case}.t", root / "t/case.t")
        # Head file is real. The supplied diff changes only the selected initializer.
        # This is unverified caller-supplied diff provenance, as current v1 specifies.
        needle = INITIALIZERS[case]
        require(source.count(needle) == 1, "diff anchor is not unique")
        base = source.replace(needle, needle.replace("5", "4"))
        patch = "".join(difflib.unified_diff(
            base.splitlines(keepends=True), source.splitlines(keepends=True),
            fromfile="a/lib/ClosureReturn.pm", tofile="b/lib/ClosureReturn.pm", n=0))
        (directory / "change.patch").write_text(patch)
        command = [str(producer), "ripr-facts", "--schema", "ripr-perl-facts-v1",
                   "--root", "fixture", "--base", "fixture-before", "--head", "fixture-after",
                   "--fact-classes", CLASSES, "--diff", "change.patch", "--out", "packet.json"]
        observation = run(command, directory)
        (directory / "command.json").write_text(json.dumps(observation, indent=2) + "\n")
        require(observation["exit"] == 0, f"producer failed: {observation}")
        raw = (directory / "packet.json").read_bytes()
        packet = json.loads(raw)
        # Baseline characterization, not v2 acceptance: keep unexpected new output visible.
        require(len(packet["changes"]) == 1 and packet["oracles"] and packet["relations"],
                f"{case}: fixture did not exercise change/oracle/relation path")
        require(all(r["oracle_id"] is None for r in packet["relations"]),
                f"{case}: linkage differs from the source-backed v1 expectation; inspect raw packet")
        require(all(o.get("target_owner_id") is None and o.get("observed_sink") is None
                    for o in packet["oracles"]), f"{case}: oracle linkage changed")
        if case == "evaluated":
            require(any(b["kind"] == "eval_or_string_code" for b in packet["dynamic_boundaries"]),
                    "eval boundary was not reported")
        (directory / "inspection.json").write_text(json.dumps(summarize(packet), indent=2) + "\n")
        receipt["runs"].append({"case": case, "argv": command, "exit": observation["exit"],
                                "raw_packet_sha256": hashlib.sha256(raw).hexdigest()})
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--runtime", action="store_true")
    group.add_argument("--producer", type=Path, help="existing immutable producer executable")
    parser.add_argument("--out", type=Path, help="new output directory for raw producer packets")
    args = parser.parse_args()
    if args.producer and not args.out:
        parser.error("--producer requires --out")
    print(json.dumps(runtime() if args.runtime else replay(args.producer, args.out), indent=2))


if __name__ == "__main__":
    main()
