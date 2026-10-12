#!/usr/bin/env python3
"""Issue17613: isolated classifier cache measurement; ordinary Cargo freshness."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import time

DEPENDENCY_KEY = "v0-rust-ci-platform-scope-7e6547106fbfe5fc2dcfef40df96fb2c8c04770ef1b958f47d362aed75ddf921-Linux-x64-0c7f9597-df4e1969"
ANSI = re.compile(r"\x1b\[[0-9;]*m")
HASH = re.compile(r"-([0-9a-f]{16})(?:\.|$)")
MAX_FILES, MAX_BYTES = 20000, 4 * 1024**3


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def write(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")


def command(root: Path, *args: str) -> str:
    return subprocess.check_output(args, cwd=root, text=True).strip()


def identity(root: Path, dependency: dict, tools: dict | None = None) -> dict:
    if command(root, "git", "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("source checkout is dirty; HEAD alone cannot bind artifacts")
    forbidden = ["FEATURES_TOML_OVERRIDE", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS",
                 "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTC", "RUSTDOC"]
    forbidden += [key for key in os.environ if key.startswith(("CARGO_PROFILE_", "CARGO_BUILD_", "CARGO_TARGET_"))]
    if any(os.environ.get(key) for key in forbidden):
        raise ValueError("unsupported override: " + ",".join(k for k in forbidden if os.environ.get(k)))
    tools = tools or {"rustc": command(root, "rustc", "-vV"), "cargo": command(root, "cargo", "-vV"),
                      "cc": command(root, "cc", "--version"), "image": os.environ.get("ImageVersion", "unknown")}
    return {"schema": 1, "head": command(root, "git", "rev-parse", "HEAD"),
            "base": command(root, "git", "merge-base", "origin/main", "HEAD"),
            "root": str(root.resolve()), "tools": tools, "dependency": dependency,
            "recipe": digest(Path(__file__).read_bytes()),
            "config": digest((root / ".cargo/config.toml").read_bytes()),
            "lock": digest((root / "Cargo.lock").read_bytes()),
            "profile": "dev", "features": "xtask default", "target": "native",
            "incremental": os.environ.get("CARGO_INCREMENTAL"),
            "backtrace": os.environ.get("RUST_BACKTRACE")}


def dependency_record(value: dict, current_ref: str | None = None) -> dict:
    if value.get("total_count", len(value["actions_caches"])) > len(value["actions_caches"]):
        raise ValueError("complete dependency cache metadata is required")
    rows = [row for row in value["actions_caches"] if row["key"] == DEPENDENCY_KEY
            and row["ref"] == "refs/heads/main"]
    if len(rows) != 1:
        raise ValueError("exact main dependency archive is unavailable or ambiguous")
    current_ref = current_ref or os.environ.get("GITHUB_REF")
    if current_ref != "refs/heads/main" and any(row["key"] == DEPENDENCY_KEY
            and row["ref"] == current_ref and row["version"] == rows[0]["version"] for row in value["actions_caches"]):
        raise ValueError("current-ref cache shadows the recorded main dependency archive")
    return {k: rows[0][k] for k in ["id", "key", "version", "size_in_bytes", "ref"]}


def safe_file(base: Path, relative: str) -> Path:
    rel = PurePosixPath(relative)
    if rel.is_absolute() or not rel.parts or any(p in {".", ".."} for p in rel.parts):
        raise ValueError("unsafe artifact path")
    path = base.joinpath(*rel.parts)
    if not path.is_relative_to(base):
        raise ValueError("artifact escaped its root")
    for parent in [path, *path.parents]:
        if parent == base.parent:
            break
        if parent.is_symlink():
            raise ValueError("symlink artifact or parent")
    return path


def regular(path: Path) -> os.stat_result:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise ValueError("artifact must be a regular file: " + str(path))
    return info


def artifact_rows(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text().splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get("reason") in {"compiler-artifact", "build-script-executed"}:
            rows.append(row)
    return rows


def selection(root: Path, rows: list[dict], stderr: str) -> tuple[list[Path], list[dict]]:
    """Exact observed hashes only; no package-prefix artifact sweep."""
    target = root / "target"
    selected: set[Path] = set()
    units: list[dict] = []
    packages: dict[str, str] = {}

    def add(path: Path) -> None:
        relative = path.relative_to(root).as_posix()
        if not relative.startswith("target/debug/"):
            raise ValueError("unsupported artifact layout/profile")
        regular(safe_file(root, relative))
        selected.add(path)

    def tree(directory: Path) -> None:
        if not directory.is_dir() or directory.is_symlink():
            raise ValueError("missing closure directory: " + str(directory))
        for path in sorted(directory.rglob("*")):
            if path.is_symlink():
                raise ValueError("symlink in closure")
            if path.is_file():
                add(path)

    def fingerprint(package: str, unit_hash: str, tag: str) -> None:
        directory = target / "debug/.fingerprint" / (package + "-" + unit_hash)
        required = [tag, tag + ".json"]
        if not tag.startswith("run-build-script-"):
            required += ["dep-" + tag, "invoked.timestamp"]
        for name in required:
            regular(directory / name)
        json_names = {p.name for p in directory.glob("*.json")}
        if json_names != {tag + ".json"}:
            raise ValueError("unselected target in fingerprint unit")
        tree(directory)
        units.append({"package": package, "hash": unit_hash, "tag": tag,
                      "required": [(directory / n).relative_to(root).as_posix() for n in required]})

    for row in rows:
        if row["reason"] != "compiler-artifact":
            continue
        manifest = Path(row["manifest_path"])
        if not manifest.is_relative_to(root):
            continue
        # Manifest parent is the package name in this repository; xtask is too.
        package = manifest.parent.name
        packages[row["package_id"]] = package
        kind, name = row["target"]["kind"], row["target"]["name"]
        if row["profile"].get("test") or kind not in [["lib"], ["proc-macro"], ["custom-build"], ["bin"]]:
            raise ValueError("unselected test/example/bench target")
        if kind == ["bin"] and (package != "xtask" or name != "xtask"):
            raise ValueError("unselected helper bin")
        files = [Path(p) for p in row["filenames"]]
        for path in files:
            add(path)
        if kind == ["custom-build"]:
            unit_hash = HASH.search(files[0].parent.name).group(1)
            tree(files[0].parent)
            fingerprint(package, unit_hash, "build-script-build-script-build")
        elif kind == ["bin"]:
            clean_stderr = ANSI.sub("", stderr)
            commands = [line for line in clean_stderr.splitlines()
                        if "--crate-name xtask " in line and "--crate-type bin " in line]
            hashes = set(re.findall(r"\.fingerprint/xtask-([0-9a-f]{16})/bin-xtask\b", clean_stderr.replace("\\", "/")))
            if len(hashes) != 1:
                raise ValueError("normal runner unit hash is unavailable or ambiguous")
            unit_hash = hashes.pop()
            intermediate = set()
            for line in commands:
                directory = Path(re.search(r"--out-dir ([^ ]+)", line).group(1).strip("'\"`"))
                if not directory.is_absolute():
                    directory = root / directory
                suffix = re.search(r"extra-filename=(-[0-9a-f]{16})", line)
                filename = "xtask" + (suffix.group(1) if suffix else "")
                intermediate.add(directory / filename)
            if len(intermediate) != 1:
                raise ValueError("normal runner intermediate path is ambiguous or absent")
            binary = intermediate.pop()
            add(binary)
            add(binary.with_name(binary.name + ".d"))
            fingerprint(package, unit_hash, "bin-xtask")
        else:
            hashes = {HASH.search(path.name).group(1) for path in files}
            if len(hashes) != 1:
                raise ValueError("ambiguous library unit")
            unit_hash = hashes.pop()
            add(files[0].parent / (name.replace("-", "_") + "-" + unit_hash + ".d"))
            fingerprint(package, unit_hash, "lib-" + name)
    for row in rows:
        if row["reason"] == "build-script-executed" and row["package_id"] in packages:
            package = packages[row["package_id"]]
            out_dir = Path(row["out_dir"])
            unit_hash = HASH.search(out_dir.parent.name).group(1)
            metadata = [out_dir.parent / name for name in ["invoked.timestamp", "output", "root-output", "stderr"]]
            for path in metadata:
                regular(path)
            tree(out_dir.parent)
            fingerprint(package, unit_hash, "run-build-script-build-script-build")
            units[-1]["required"] += [p.relative_to(root).as_posix() for p in metadata]
            if not any(p.is_file() for p in out_dir.rglob("*")):
                raise ValueError("generated build-script outputs are absent")
    if not units or not any(u["tag"] == "bin-xtask" for u in units):
        raise ValueError("runner closure missing")
    return sorted(selected), units


def capture(root: Path, output: Path, snapshot: Path, binding: dict) -> dict:
    rows = artifact_rows(output / "cold.cargo.jsonl")
    files, units = selection(root, rows, (output / "cold.cargo.stderr").read_text())
    records, links = [], {}
    logical, allocated = 0, 0
    for path in files:
        info = regular(path)
        relative = path.relative_to(root).as_posix()
        dest = safe_file(snapshot / "files", relative)
        dest.parent.mkdir(parents=True, exist_ok=True)
        key = (info.st_dev, info.st_ino)
        if len(records) >= MAX_FILES or info.st_size > MAX_BYTES or (key not in links and logical + info.st_size > MAX_BYTES):
            raise ValueError("selected overlay exceeds diagnostic bound")
        if key in links:
            os.link(snapshot / "files" / links[key], dest)
        else:
            shutil.copy2(path, dest)
            links[key] = relative
            logical += info.st_size
            allocated += getattr(info, "st_blocks", 0) * 512
        records.append({"path": relative, "bytes": info.st_size, "sha256": file_digest(dest),
                        "mode": stat.S_IMODE(info.st_mode), "link_group": links[key]})
    if len(records) > MAX_FILES or logical > MAX_BYTES:
        raise ValueError("selected overlay exceeds diagnostic bound")
    value = {"schema": 1, "binding": binding, "units": units, "files": records,
             "logical_bytes": logical, "allocated_bytes": allocated}
    write(snapshot / "manifest.json", value)
    write(output / "overlay-manifest.json", value)
    write(output / "overlay-manifest-sha.json", {"sha256": digest((snapshot / "manifest.json").read_bytes())})
    return value


def validate(snapshot: Path, binding: dict, expected_sha: str) -> dict:
    data = (snapshot / "manifest.json").read_bytes()
    if digest(data) != expected_sha:
        raise ValueError("overlay is not the producer's manifest")
    value = json.loads(data)
    if value["schema"] != 1 or value["binding"] != binding:
        raise ValueError("source/config/toolchain/dependency/recipe binding mismatch")
    records = value["files"]
    if not records or len(records) > MAX_FILES:
        raise ValueError("invalid overlay file count")
    declared = {row["path"] for row in records}
    if len(declared) != len(records):
        raise ValueError("duplicate path")
    actual = {p.relative_to(snapshot / "files").as_posix() for p in (snapshot / "files").rglob("*") if p.is_file()}
    if actual != declared:
        raise ValueError("overlay has missing or extra files")
    if not value["units"] or not any(u["tag"] == "bin-xtask" for u in value["units"]):
        raise ValueError("runner unit absent")
    if any(p not in declared for u in value["units"] for p in u["required"]):
        raise ValueError("required Cargo support file absent")
    total = 0
    by_path = {row["path"]: row for row in records}
    for row in records:
        if not row["path"].startswith("target/debug/") or row["link_group"] not in declared:
            raise ValueError("unselected path or invalid link group")
        group = by_path[row["link_group"]]
        if any(row[k] != group[k] for k in ["bytes", "sha256", "mode"]):
            raise ValueError("inconsistent hardlink group")
        path = safe_file(snapshot / "files", row["path"])
        info = regular(path)
        if info.st_size > MAX_BYTES or info.st_size != row["bytes"] or stat.S_IMODE(info.st_mode) != row["mode"] or file_digest(path) != row["sha256"]:
            raise ValueError("corrupt overlay file")
        total += info.st_size
    if total > MAX_BYTES * 2:
        raise ValueError("declared overlay exceeds bound")
    return value


def install(snapshot: Path, root: Path, value: dict) -> None:
    for row in value["files"]:
        dest = safe_file(root, row["path"])
        dest.parent.mkdir(parents=True, exist_ok=True)
        if row["link_group"] == row["path"]:
            shutil.copy2(safe_file(snapshot / "files", row["path"]), dest)
        else:
            if dest.exists():
                regular(dest)
                dest.unlink()
            os.link(safe_file(root, row["link_group"]), dest)


def check_receipts(subject: dict, scope: dict, binding: dict) -> None:
    if subject.get("schema_version") != "ci-subject.v1" or subject.get("status") != "resolved":
        raise ValueError("invalid subject receipt")
    if subject.get("head_sha") != binding["head"] or subject.get("base_sha") != binding["base"]:
        raise ValueError("wrong subject binding")
    if scope.get("schema_version") != 2 or scope.get("head_sha") != binding["head"]:
        raise ValueError("wrong scope schema/head")
    overrides = scope.get("platform_overrides", {})
    if type(overrides.get("windows_runner")) is not bool:
        raise ValueError("missing/null/non-boolean windows_runner")
    crates = overrides.get("windows_test_crates", [])
    if not isinstance(crates, list) or any(not isinstance(x, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", x) for x in crates):
        raise ValueError("invalid windows_test_crates")


def classify(root: Path, output: Path, phase: str, binding: dict, reference: Path | None) -> dict:
    started = time.perf_counter()
    subject_path = output / (phase + ".subject.json")
    env = dict(os.environ, CARGO_LOG="cargo::core::compiler::fingerprint=debug")
    args = ["cargo", "run", "-p", "xtask", "--locked", "-vv", "--message-format=json", "--", "ci-subject",
            "--base-sha", binding["base"], "--head-sha", binding["head"], "--receipt", str(subject_path)]
    first = subprocess.run(args, cwd=root, env=env, text=True, capture_output=True)
    (output / (phase + ".cargo.jsonl")).write_text(first.stdout)
    (output / (phase + ".cargo.stderr")).write_text(first.stderr)
    if first.returncode:
        raise ValueError("ci-subject command failed: " + str(first.returncode))
    second = subprocess.run(["cargo", "run", "-p", "xtask", "--locked", "--", "ci-scope", "--subject",
                             str(subject_path), "--format", "json"], cwd=root, env=env, text=True, capture_output=True)
    (output / (phase + ".scope.json")).write_text(second.stdout)
    (output / (phase + ".scope.stderr")).write_text(second.stderr)
    if second.returncode:
        raise ValueError("ci-scope command failed: " + str(second.returncode))
    subject, scope = json.loads(subject_path.read_text()), json.loads(second.stdout)
    check_receipts(subject, scope, binding)
    if reference:
        if subject != json.loads((reference / "cold.subject.json").read_text()) or scope != json.loads((reference / "cold.scope.json").read_text()):
            raise ValueError("classifier coverage/output differs from producer")
    rows = [r for r in artifact_rows(output / (phase + ".cargo.jsonl")) if r["reason"] == "compiler-artifact"]
    dirty = [r for r in rows if not r["fresh"]]
    workspace = [r for r in dirty if Path(r["manifest_path"]).is_relative_to(root)]
    executable = [r["executable"] for r in rows if r.get("executable") and r["target"]["name"] == "xtask"]
    if len(executable) != 1:
        raise ValueError("normal Cargo runner artifact binding missing")
    result = {"phase": phase, "classification_seconds": time.perf_counter() - started,
              "nonfresh_workspace_events": len(workspace), "nonfresh_registry_events": len(dirty) - len(workspace),
              "nonfresh_packages": sorted({r["package_id"] for r in dirty}),
              "nonfresh_workspace_package_count": len({r["package_id"] for r in workspace}),
              "observed_workspace_package_count": len({r["package_id"] for r in rows if Path(r["manifest_path"]).is_relative_to(root)}),
              "workspace_cold": all(not r["fresh"] for r in rows if Path(r["manifest_path"]).is_relative_to(root)),
              "runner_sha256_after_execution": file_digest(Path(executable[0])),
              "subject": subject, "platform_overrides": scope["platform_overrides"]}
    write(output / (phase + ".result.json"), result)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["start", "prepare", "classify", "capture", "install", "controls", "finish"])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--snapshot", type=Path)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--phase", default="cold")
    parser.add_argument("--dependency-info", type=Path)
    parser.add_argument("--archive-info", type=Path)
    args = parser.parse_args()
    root, output = Path.cwd().resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if args.operation == "start":
        write(output / "clock.json", {"monotonic": time.perf_counter(), "unix": time.time()})
        return
    if args.operation == "prepare":
        dependency = dependency_record(json.loads(args.dependency_info.read_text()))
        binding = identity(root, dependency)
        if "rustc 1.95.0" not in binding["tools"]["rustc"] or binding["incremental"] != "0":
            raise ValueError("unsupported compiler/incremental setting")
        key = "classifier-overlay-v1-" + os.environ["GITHUB_RUN_ID"] + "-" + os.environ["GITHUB_RUN_ATTEMPT"] + "-" + digest(canonical(binding))
        clock = json.loads((output / "clock.json").read_text())
        write(output / "trial.json", {"binding": binding, "key": key, "started_monotonic": clock["monotonic"],
                                      "started_unix": clock["unix"]})
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
                stream.write("overlay_key=" + key + "\n")
        return
    trial = json.loads((output / "trial.json").read_text())
    binding = identity(root, trial["binding"]["dependency"])
    if binding != trial["binding"]:
        raise ValueError("trial input changed")
    if args.operation == "classify":
        value = classify(root, output, args.phase, binding, args.reference)
        value["elapsed_since_trial_start_seconds"] = time.perf_counter() - trial["started_monotonic"]
        if args.phase != "warm":
            value["end_to_end_restore_build_classify_seconds"] = value["elapsed_since_trial_start_seconds"]
        write(output / (args.phase + ".result.json"), value)
        if value["nonfresh_registry_events"]:
            raise ValueError("NOT_PROVEN: registry cache was not dependency-warm")
        if args.phase in {"cold", "baseline"} and not value["workspace_cold"]:
            raise ValueError("NOT_PROVEN: workspace-cold baseline was contaminated")
    elif args.operation == "capture":
        capture(root, output, args.snapshot, binding)
    elif args.operation == "install":
        expected = json.loads((args.reference / "overlay-manifest-sha.json").read_text())["sha256"]
        value = validate(args.snapshot, binding, expected)
        install(args.snapshot, root, value)
        write(output / "installed.json", {"manifest_sha": expected, "files": len(value["files"])})
    elif args.operation == "controls":
        expected = json.loads((output / "overlay-manifest-sha.json").read_text())["sha256"]
        validate(args.snapshot, binding, expected)
        results = []
        for field in ["head", "config", "tools", "dependency", "incremental"]:
            altered = dict(binding)
            altered[field] = "deliberately mismatched input"
            try:
                validate(args.snapshot, altered, expected)
            except ValueError:
                results.append({"field": field, "rejected_before_install_or_execution": True})
            else:
                raise ValueError("negative control accepted stale " + field)
        write(output / "actual-overlay-negative-controls.json", results)
    else:
        value = {"binding": binding, "overlay_key": trial["key"],
                 "total_observed_trial_seconds": time.perf_counter() - trial["started_monotonic"],
                 "dependency_archive_bytes": binding["dependency"]["size_in_bytes"]}
        if args.archive_info:
            rows = [r for r in json.loads(args.archive_info.read_text())["actions_caches"] if r["key"] == trial["key"]]
            if len(rows) != 1:
                raise ValueError("saved/restored overlay archive identity is unavailable or ambiguous")
            value["overlay_archive"] = [{k:r[k] for k in ["id", "key", "version", "size_in_bytes", "ref"]} for r in rows]
        write(output / "finished.json", value)
    print(json.dumps({"operation": args.operation, "result": "pass"}))


if __name__ == "__main__":
    main()
