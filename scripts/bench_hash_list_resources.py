#!/usr/bin/env python3
"""Paired Linux parser resource observations; no builds or artifact cleanup.

Supply two immutable parser_resources executables built with the same compiler,
profile and counter. The native debug projection is a differential control, not
AST equality; focused Rust tests independently pin the changed list semantics.
"""

import argparse
import hashlib
import json
import os
import pathlib
import platform
import statistics
import subprocess
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def nested(depth, width):
    return (
        "my $h = "
        + "".join(f"(key{i} => " for i in range(depth))
        + "[" + ",".join(map(str, range(width))) + "]"
        + ")" * depth + "; my $after = 7;\n"
    )


def observe(command, stdout_path, stderr_path):
    # Reap this one child ourselves: wait4 returns its own user/system CPU and
    # Linux maximum resident set, avoiding cumulative RUSAGE_CHILDREN maxima.
    with stdout_path.open("w") as stdout, stderr_path.open("w") as stderr:
        child = subprocess.Popen(command, stdout=stdout, stderr=stderr)
        deadline = time.monotonic() + 60
        memory = {"samples": 0, "observed_rss_kib": 0, "observed_hwm_kib": 0}
        while True:
            pid, status, usage = os.wait4(child.pid, os.WNOHANG)
            if pid:
                child.returncode = os.waitstatus_to_exitcode(status)
                break
            if time.monotonic() >= deadline:
                child.kill()
                _, status, _ = os.wait4(child.pid, 0)
                child.returncode = os.waitstatus_to_exitcode(status)
                raise RuntimeError("owned benchmark child exceeded 60 seconds")
            # wait4's lifetime maximum can include this Python parent's
            # inherited footprint before exec. Sample the child's current mm
            # only after its actual executable matches the immutable subject.
            try:
                proc = pathlib.Path(f"/proc/{child.pid}")
                if os.path.samefile(proc / "exe", command[0]):
                    status_text = (proc / "status").read_text()
                    values = {line.split()[0][:-1]: int(line.split()[1])
                              for line in status_text.splitlines()
                              if line.startswith(("VmRSS:", "VmHWM:"))}
                    if "VmRSS" in values and "VmHWM" in values:
                        memory["samples"] += 1
                        memory["observed_rss_kib"] = max(memory["observed_rss_kib"], values["VmRSS"])
                        memory["observed_hwm_kib"] = max(memory["observed_hwm_kib"], values["VmHWM"])
            except (FileNotFoundError, ProcessLookupError):
                pass  # An owned child can exit between the two observations.
            time.sleep(0.001)
    if child.returncode != 0:
        raise subprocess.CalledProcessError(child.returncode, command)
    if memory["samples"] == 0:
        raise RuntimeError("native memory unavailable: no sample after exact executable identity")
    return usage, memory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=pathlib.Path, required=True)
    parser.add_argument("--candidate", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--ordinary", type=pathlib.Path, action="append", default=[])
    args = parser.parse_args()
    if args.iterations <= 0 or args.rounds <= 0:
        parser.error("iterations and rounds must be positive")
    if platform.system() != "Linux" or not hasattr(os, "wait4"):
        parser.error("requires Linux os.wait4 for per-process CPU/RSS observations")
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    identities = {label: {"path": str(path), "sha256": digest(path)} for label, path in binaries.items()}
    cases = []
    for width in (1, 256, 2048):
        for depth in (0, 4, 8, 16, 24, 32):
            name = f"nested-{depth}-{width}"
            path = args.output / f"{name}.pl"
            path.write_text(nested(depth, width))
            cases.append((name, path, "generated", depth, width))
    controls = {
        "multi": "my $h = (alpha => 1, beta => 2, gamma => 3); my $after = 7;\n",
        "odd": "my $h = (a => 1, 2); my $after = 7;\n",
        "even-comma": "my $h = (1, 2, 3, 4); my $after = 7;\n",
        "chained": "my $h = (a => 1 => b => 2); my $after = 7;\n",
        "malformed": "my $h = (alpha => [1,2], beta => 3; my $after = 7;\n",
    }
    for name, source in controls.items():
        path = args.output / f"{name}.pl"
        path.write_text(source)
        cases.append((name, path, "control", None, None))
    for index, path in enumerate(args.ordinary):
        cases.append((f"ordinary-{index}-{path.name}", path.resolve(), "ordinary", None, None))
    manifest = []
    rows = []
    for name, path, family, depth, width in cases:
        manifest.append({"name": name, "path": str(path.resolve()), "sha256": digest(path),
                         "bytes": path.stat().st_size, "family": family, "depth": depth, "width": width})
        oracles = {}
        for label, executable in binaries.items():
            run = subprocess.run([str(executable), str(path), "1", "--oracle"],
                                 capture_output=True, text=True, check=True, timeout=30)
            oracle = run.stdout.splitlines()[0]
            if not oracle.startswith("oracle="):
                raise RuntimeError(f"missing complete projection for {name}/{label}")
            (args.output / f"{name}.{label}.oracle.txt").write_text(oracle + "\n")
            oracles[label] = oracle
        if oracles["baseline"] != oracles["candidate"]:
            raise RuntimeError(f"projection/spans/diagnostic/budget drift for {name}")
        for round_number in range(args.rounds):
            # Alternate which executable runs first to reduce simple order bias.
            order = ("baseline", "candidate") if round_number % 2 == 0 else ("candidate", "baseline")
            for label in order:
                timing = args.output / f"{name}.{label}.{round_number}.time.txt"
                command = [str(binaries[label]), str(path), str(args.iterations)]
                stdout_path = args.output / f"{name}.{label}.{round_number}.stdout.txt"
                stderr_path = args.output / f"{name}.{label}.{round_number}.stderr.txt"
                usage, memory = observe(command, stdout_path, stderr_path)
                timing.write_text(json.dumps({"user_seconds": usage.ru_utime,
                    "system_seconds": usage.ru_stime, "wait4_max_rss_kib": usage.ru_maxrss, **memory}))
                metrics = dict(piece.split("=", 1) for piece in stdout_path.read_text().split())
                rows.append({"case": name, "binary": label, "round": round_number,
                             "command": command, "user_seconds": usage.ru_utime, "system_seconds": usage.ru_stime,
                             "cpu_seconds": usage.ru_utime + usage.ru_stime,
                             "wait4_max_rss_kib": usage.ru_maxrss, **memory,
                             **{key: int(value) for key, value in metrics.items()}})
        (args.output / "raw.json").write_text(json.dumps(rows, indent=2))
        print(f"observed {name}: oracle agrees; {args.rounds} paired rounds", flush=True)
    summary = []
    for case in manifest:
        item = dict(case)
        for label in binaries:
            observations = [row for row in rows if row["case"] == case["name"] and row["binary"] == label]
            item[label] = {key: statistics.median(row[key] for row in observations)
                           for key in ("parse_ns", "drop_ns", "allocation_calls", "requested_bytes", "user_seconds", "system_seconds", "cpu_seconds", "wait4_max_rss_kib", "samples", "observed_rss_kib", "observed_hwm_kib")}
        summary.append(item)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2))
    (args.output / "manifest.json").write_text(json.dumps({"binaries": identities, "cases": manifest,
        "iterations": args.iterations, "rounds": args.rounds, "platform": platform.platform()}, indent=2))
    if any(digest(path) != identities[label]["sha256"] for label, path in binaries.items()):
        raise RuntimeError("executable changed during measurement")


if __name__ == "__main__":
    main()
