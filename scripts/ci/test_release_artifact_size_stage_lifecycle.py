#!/usr/bin/env python3
"""Standalone, native-host lifecycle proof; no Cargo build or macOS claim.

Slot-12 interface: --script PATH --evidence-dir NEW_DIRECTORY. The script under
test receives only its existing argv and RELEASE_ARTIFACT_SIZE_ROOT interface.
All fixtures and shims are owned by this invocation and retained for inspection.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import shutil
import subprocess
import sys
import tarfile
import time


TARGET = "x86_64-apple-darwin"  # A layout label, never a cross-build claim.
VERSION = "9.9.9"
PACKAGE = f"perllsp-{VERSION}-{TARGET}"
BINARIES = ("perllsp", "perl-dap")
VARIANTS = ("baseline", "candidate")


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def snapshot(directory):
    result = {}
    for path in sorted(directory.rglob("*")):
        name = str(path.relative_to(directory))
        if path.is_symlink():
            result[name] = {"link": os.readlink(path)}
        elif path.is_file():
            data = path.read_bytes()
            result[name] = {"sha256": digest(data), "bytes": len(data),
                            "mode": path.stat().st_mode & 0o777}
        elif path.is_dir():
            result[name] = {"directory": True}
    return result


class Proof:
    def __init__(self, script, evidence):
        self.script = script
        self.evidence = evidence
        self.events = []
        self.tools = {name: shutil.which(name) for name in ("bash", "cc", "strip", "tar", "mv")}
        require(all(self.tools.values()), f"missing tools: {self.tools}")
        self.templates = evidence / "native-inputs"
        self.templates.mkdir()
        self.commands = []
        for revision in (1, 2, 3):
            source = self.templates / f"witness-{revision}.c"
            binary = self.templates / f"witness-{revision}"
            source.write_text(f'#include <stdio.h>\nint main(void) {{ puts("witness {revision}"); return 0; }}\n')
            command = [self.tools["cc"], "-g", "-O0", str(source), "-o", str(binary)]
            result = subprocess.run(command, capture_output=True, text=True)
            self.commands.append({"argv": command, "exit": result.returncode,
                                  "stdout": result.stdout, "stderr": result.stderr})
            require(result.returncode == 0, f"fixture compilation failed: {result.stderr}")

    def seed(self, root, revision):
        build = root / "target" / TARGET / "release"
        build.mkdir(parents=True, exist_ok=True)
        for binary in BINARIES:
            shutil.copy2(self.templates / f"witness-{revision}", build / binary)

    def stage(self, root, variant, shim=None):
        env = os.environ.copy()
        env["RELEASE_ARTIFACT_SIZE_ROOT"] = str(root)
        if shim:
            env["PATH"] = str(shim) + os.pathsep + env["PATH"]
        argv = [self.tools["bash"], str(self.script), variant, TARGET, VERSION]
        result = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=30)
        self.events.append({"argv": argv, "root": str(root), "shim": str(shim) if shim else None,
                            "exit": result.returncode, "stdout": result.stdout,
                            "stderr": result.stderr})
        return result

    def valid(self, root, variant):
        stage = root / "target/shadow" / variant
        package = stage / PACKAGE
        archive = stage / f"{PACKAGE}.tar.gz"
        expected = {str(p.relative_to(stage)): p.read_bytes() for p in package.rglob("*") if p.is_file()}
        require(set(expected) == {f"{PACKAGE}/{name}" for name in
                                (*BINARIES, "README.md", "LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt")},
                "package inventory differs from release layout")
        # An independent reader compares every payload byte, not only the two
        # hashes checked by the producer's own archive validation.
        with tarfile.open(archive, "r:gz") as contents:
            members = contents.getmembers()
            require(len({m.name for m in members}) == len(members), "duplicate archive members")
            require({m.name for m in members if m.isdir()} == {PACKAGE}, "unexpected archive directories")
            require(all(m.isfile() or m.isdir() for m in members), "unexpected archive entry type")
            actual = {m.name: contents.extractfile(m).read() for m in contents if m.isfile()}
        require(actual == expected, "package/archive payload byte mismatch")
        manifest = package.joinpath("SHA256SUMS.txt").read_text().splitlines()
        require(len(manifest) == 2, "checksum manifest must name exactly two binaries")
        for line, binary in zip(manifest, BINARIES):
            checksum, name = line.split()
            require(name == binary and checksum == digest(package.joinpath(binary).read_bytes()),
                    f"invalid checksum manifest for {binary}")
            require(package.joinpath(binary).stat().st_size < root.joinpath("target", TARGET, "release", binary).stat().st_size,
                    f"{binary} did not shrink: strip control is vacuous")

    def fixture(self, name, empty_variant=None):
        root = self.evidence / name
        root.mkdir()
        for extra in ("README.md", "LICENSE-APACHE", "LICENSE-MIT"):
            root.joinpath(extra).write_text(f"{extra}: lifecycle fixture\n")
        for revision, variant in enumerate(VARIANTS, 1):
            if variant == empty_variant:
                continue
            self.seed(root, revision)
            result = self.stage(root, variant)
            require(result.returncode == 0, f"initial witness failed: {result.stderr}")
            self.valid(root, variant)
        # Unknown/possibly active siblings must survive; cleanup may touch only
        # the current producer's own mktemp allocation and replaced witness.
        root.joinpath("target/shadow/.baseline.stage-unowned").mkdir()
        root.joinpath("target/shadow/.baseline.stage-unowned/KEEP").write_bytes(b"active sibling\n")
        for variant in VARIANTS:
            root.joinpath("target/shadow", variant).mkdir(parents=True, exist_ok=True)
            root.joinpath("target/shadow", variant, "KEEP").write_bytes(b"unrelated variant data\n")
            root.joinpath("target/shadow", variant, f"{PACKAGE}.backup").write_bytes(b"package-prefixed sibling\n")
            root.joinpath("target/shadow", variant, ".stage.unowned").mkdir()
            root.joinpath("target/shadow", variant, ".stage.unowned/KEEP").write_bytes(b"active transaction sibling\n")
        self.seed(root, 3)
        return root

    def shim(self, root, tool, body):
        directory = root / "owned-shim"
        directory.mkdir()
        path = directory / tool
        path.write_text("#!/usr/bin/env bash\nset -eu\n" + body)
        path.chmod(0o755)
        return directory

    def owned_transaction(self, root, variant):
        stage = root / "target/shadow" / variant
        lock = stage / f".{PACKAGE}.stage-lock"
        require(lock.is_dir() and not lock.is_symlink(), "producer did not retain its lock")
        marker = lock / "transaction"
        require(marker.is_file() and not marker.is_symlink(), "lock has no regular transaction pointer")
        pointer = marker.read_text()
        require(pointer.endswith("\n") and len(pointer.splitlines()) == 1,
                "transaction pointer is not one exact WORK_DIR line")
        relative = Path(pointer.removesuffix("\n"))
        require(not relative.is_absolute(), "transaction pointer is not root-relative")
        txn = root / relative
        require(txn.parent == stage and txn.name.startswith(".stage.") and txn.name != ".stage.unowned",
                "transaction pointer does not name this fixture's owned allocation")
        require(txn.is_dir() and not txn.is_symlink(), "pointed transaction is not a regular directory")
        return lock, txn

    def verify_backups(self, root, variant, before):
        lock, txn = self.owned_transaction(root, variant)
        prefix = f"{variant}/{PACKAGE}/"
        original = {p.removeprefix(prefix): v for p, v in before.items() if p.startswith(prefix)}
        require(snapshot(txn / "previous-package") == original, "old package backup differs")
        archive = txn / "previous-archive"
        require(archive.is_file(), "both backups were not moved before publication")
        expected = before[f"{variant}/{PACKAGE}.tar.gz"]
        require(digest(archive.read_bytes()) == expected["sha256"], "old archive backup differs")
        return lock, txn

    def recover_owned(self, root, variant, before):
        lock, txn = self.owned_transaction(root, variant)
        prefix = f"{variant}/{PACKAGE}/"
        previous = txn / "previous-package"
        original = {p.removeprefix(prefix): v for p, v in before.items() if p.startswith(prefix)}
        require(snapshot(previous) == original, "recoverable old package lost bytes")
        record = {"path": str(txn), "lock": str(lock), "pointer": (lock / "transaction").read_text(),
                  "previous_package": snapshot(previous)}
        package = root / "target/shadow" / variant / PACKAGE
        require(not package.exists(), "refusing to overwrite an unexpected recovery destination")
        previous.rename(package)
        archive = root / "target/shadow" / variant / f"{PACKAGE}.tar.gz"
        if (txn / "previous-archive").exists():
            require(not archive.exists(), "refusing to overwrite an unexpected recovered archive")
            require(digest((txn / "previous-archive").read_bytes()) ==
                    before[f"{variant}/{PACKAGE}.tar.gz"]["sha256"], "recoverable archive lost bytes")
            (txn / "previous-archive").rename(archive)
        shadow = root / "target/shadow"
        owned = (str(txn.relative_to(shadow)), str(lock.relative_to(shadow)))
        restored = {p: v for p, v in snapshot(shadow).items()
                    if not any(p == item or p.startswith(item + "/") for item in owned)}
        require(restored == before, "recovered witness/siblings differ before cleanup")
        # This fixture created the lock and proved its exact pointer and backup
        # bytes. No glob or other invocation's transaction is reclaimed.
        shutil.rmtree(txn)
        require((lock / "transaction").read_text() == record["pointer"], "recovery lock pointer changed")
        (lock / "transaction").unlink()
        lock.rmdir()
        require(snapshot(shadow) == before, "recovery cleanup changed unrelated data")
        record["recovery_settled"] = True
        return record

    def boundary_shim(self, root, variant, boundary, fail_archive=False):
        coordination = root / "owned-coordination"
        coordination.mkdir()
        directory = root / "owned-shim"
        directory.mkdir()
        config = {"coordination": str(coordination), "boundary": boundary,
                  "package": str(root / "target/shadow" / variant / PACKAGE),
                  "archive": str(root / "target/shadow" / variant / f"{PACKAGE}.tar.gz"),
                  "real_mv": self.tools["mv"], "fail_archive": fail_archive}
        body = '''import json, os, pathlib, sys, time
config = CONFIG
args = sys.argv[1:]
source, destination = (pathlib.Path(p).absolute() for p in args[-2:])
phase = None
if source.name == "previous-package" and config["fail_archive"]:
    phase = "rollback"
elif str(destination) == config[config["boundary"]] and source.name not in ("previous-package", "previous-archive"):
    phase = "promotion"
if phase:
    base = pathlib.Path(config["coordination"])
    ready = base / (phase + ".ready.json")
    if ready.exists():
        print("unexpected repeated controlled mv boundary", file=sys.stderr)
        sys.exit(84)
    pending = ready.with_suffix(".pending")
    pending.write_text(json.dumps({"argv": args, "pid": os.getpid(), "parent_pid": os.getppid(),
                                   "source": str(source), "destination": str(destination)}))
    pending.replace(ready)
    deadline = time.monotonic() + 20
    while not (base / (phase + ".release")).exists():
        if time.monotonic() >= deadline:
            print("controlled mv boundary was never released", file=sys.stderr)
            sys.exit(85)
        time.sleep(0.01)
    if phase == "promotion" and config["fail_archive"]:
        print("injected-overlap-archive-promotion", file=sys.stderr)
        sys.exit(72)
os.execv(config["real_mv"], [config["real_mv"], *args])
'''
        path = directory / "mv"
        path.write_text("#!" + sys.executable + "\n" + body.replace("CONFIG", repr(config), 1))
        path.chmod(0o755)
        return directory, coordination

    def launch(self, root, variant, shim):
        env = os.environ.copy()
        env["RELEASE_ARTIFACT_SIZE_ROOT"] = str(root)
        env["PATH"] = str(shim) + os.pathsep + env["PATH"]
        argv = [self.tools["bash"], str(self.script), variant, TARGET, VERSION]
        process = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        return process, {"argv": argv, "root": str(root), "shim": str(shim), "pid": process.pid}

    def wait_boundary(self, process, coordination, phase):
        ready = coordination / f"{phase}.ready.json"
        deadline = time.monotonic() + 15
        while not ready.exists():
            require(process.poll() is None, f"producer exited before {phase} boundary")
            require(time.monotonic() < deadline, f"producer never reached {phase} boundary")
            time.sleep(0.01)
        observed = json.loads(ready.read_text())
        require(observed["parent_pid"] == process.pid, "controlled mv is not the producer's direct child")
        return observed

    def settle(self, process, event, coordination):
        # Always release both bounded shims before communicating. The producer
        # synchronously waits for each mv child; successful communication reaps
        # the producer only after those children complete. A timeout is failure.
        for phase in ("promotion", "rollback"):
            (coordination / f"{phase}.release").touch()
        try:
            stdout, stderr = process.communicate(timeout=30)
        except subprocess.TimeoutExpired:
            process.terminate()  # Only this invocation's exact Popen child.
            try:
                stdout, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                stdout, stderr = process.communicate(timeout=5)
            event.update({"exit": process.returncode, "stdout": stdout, "stderr": stderr,
                          "settlement_timeout": True})
            self.events.append(event)
            raise AssertionError("producer did not settle after all owned boundaries were released")
        event.update({"exit": process.returncode, "stdout": stdout, "stderr": stderr})
        self.events.append(event)
        return subprocess.CompletedProcess(event["argv"], process.returncode, stdout, stderr)

    def refuse_competitor(self, root, variant, record, label):
        shadow = root / "target/shadow"
        paused = snapshot(shadow)
        output = self.stage(root, variant)
        require(output.returncode == 1 and "staging lock unavailable" in output.stderr,
                f"competitor was not refused by the owned lock: {output.stderr}")
        require(snapshot(shadow) == paused, "refused competitor changed owner transaction/witness/siblings")
        record[label] = {"exit": output.returncode, "shadow_unchanged": True,
                         "paused_shadow": paused}

    def lifecycle_case(self, kind, variant, boundary=None, signum=None):
        suffix = f"-{boundary}-{signal.Signals(signum).name}" if signum else ""
        name = f"{kind}-{variant}{suffix}"
        event_start = len(self.events)
        root = self.fixture(name)
        shadow = root / "target/shadow"
        before = snapshot(shadow)
        record = {"case": name, "before": before}
        process = None
        try:
            if kind == "foreign-lock":
                lock = root / "target/shadow" / variant / f".{PACKAGE}.stage-lock"
                lock.mkdir()
                (lock / "transaction").write_text(f"target/shadow/{variant}/.stage.unowned\n")
                (lock / "KEEP").write_bytes(b"foreign lock owner\n")
                record["before"] = snapshot(shadow)
                self.refuse_competitor(root, variant, record, "foreign_owner_refusal")
            else:
                fail_archive = kind == "overlap-rollback"
                boundary = boundary or ("archive" if fail_archive else "package")
                shim, coordination = self.boundary_shim(root, variant, boundary, fail_archive)
                process, event = self.launch(root, variant, shim)
                record["promotion_boundary"] = self.wait_boundary(process, coordination, "promotion")
                self.verify_backups(root, variant, before)
                package = root / "target/shadow" / variant / PACKAGE
                prepared = Path(record["promotion_boundary"]["source"]) if boundary == "package" else package
                expected_package = snapshot(prepared)
                require(expected_package["perllsp"] != before[f"{variant}/{PACKAGE}/perllsp"],
                        "paused new witness is indistinguishable from previous witness")
                if signum:
                    process.send_signal(signum)
                    record["signal"] = {"name": signal.Signals(signum).name, "target_pid": process.pid}
                else:
                    # A has already copied revision 3 into its private prepared
                    # pair. B receives distinguishable revision 2 on the same
                    # newly owned fixture, not another output root.
                    old_input = digest(root.joinpath("target", TARGET, "release", "perllsp").read_bytes())
                    self.seed(root, 2)
                    competitor_input = digest(root.joinpath("target", TARGET, "release", "perllsp").read_bytes())
                    require(competitor_input != old_input,
                            "competitor input is indistinguishable")
                    record["input_identity"] = {"owner_sha256": old_input,
                                                "competitor_sha256": competitor_input}
                    self.refuse_competitor(root, variant, record, "publication_refusal")
                    if fail_archive:
                        (coordination / "promotion.release").touch()
                        record["rollback_boundary"] = self.wait_boundary(process, coordination, "rollback")
                        self.owned_transaction(root, variant)
                        self.refuse_competitor(root, variant, record, "rollback_refusal")
                output = self.settle(process, event, coordination)
                process = None
                if signum:
                    require(output.returncode == 128 + signum,
                            f"wrong interrupted status: {output.returncode}: {output.stderr}")
                    require(snapshot(shadow) == before, "signal did not restore previous pair/siblings")
                elif fail_archive:
                    require(output.returncode != 0 and "injected-overlap-archive-promotion" in output.stderr,
                            f"rollback overlap did not reach injected failure: {output.stderr}")
                    require(snapshot(shadow) == before, "rollback overlap did not restore previous pair/siblings")
                else:
                    require(output.returncode == 0, f"overlap owner failed: {output.stderr}")
                    require(snapshot(package) == expected_package, "owner did not publish its prepared witness")
                    self.valid(root, variant)
                    prefix = f"{variant}/{PACKAGE}"
                    def replaced(path):
                        return path == prefix or path.startswith(prefix + "/") or path == prefix + ".tar.gz"
                    require({p: v for p, v in before.items() if not replaced(p)} ==
                            {p: v for p, v in snapshot(shadow).items() if not replaced(p)},
                            "overlap owner changed unrelated siblings or retained temporary/lock data")
            record["status"] = "PASS"
        except Exception as error:
            record.update({"status": "FAIL", "error": str(error)})
        finally:
            if process is not None:
                try:
                    self.settle(process, event, coordination)
                except Exception as error:
                    record.update({"status": "FAIL", "settlement_error": str(error)})
        record["after"] = snapshot(shadow)
        record["commands"] = self.events[event_start:]
        return record

    def case(self, kind, variant):
        name = f"{kind}-{variant}"
        event_start = len(self.events)
        root = self.fixture(name, empty_variant=variant if kind == "success" else None)
        shadow = root / "target/shadow"
        before = snapshot(shadow)
        record = {"case": name, "before": before}
        try:
            shim = None
            if kind == "missing-second":
                root.joinpath("target", TARGET, "release", "perl-dap").unlink()
                expected_error = "missing target/"
            elif kind == "strip-second":
                root.joinpath("target", TARGET, "release", "perl-dap").write_bytes(b"not an executable\n")
                expected_error = "refusing to package unstripped perl-dap"
            elif kind == "tar-create":
                shim = self.shim(root, "tar", 'if [ "$1" = czf ]; then echo injected-tar-create >&2; exit 71; fi\nexec ' + self.tools["tar"] + ' "$@"\n')
                expected_error = "injected-tar-create"
            elif kind == "archive-mismatch":
                shim = self.shim(root, "tar", self.tools["tar"] + ' "$@"\nif [ "$1" = xzf ]; then printf corrupt >> "$4/' + PACKAGE + '/perl-dap"; fi\n')
                expected_error = "archive member perl-dap does not match"
            elif kind in ("publish-failure", "rollback-failure"):
                final = str(root.joinpath("target/shadow", variant, f"{PACKAGE}.tar.gz"))
                relative = f"target/shadow/{variant}/{PACKAGE}.tar.gz"
                # Fail once, after the package rename, and allow rollback moves.
                observe = str(root / "promotion-observed-perllsp")
                installed = str(root.joinpath("target/shadow", variant, PACKAGE, "perllsp"))
                rollback = ('case "$1" in */previous-package) echo injected-rollback >&2; exit 73;; esac\n'
                            if kind == "rollback-failure" else "")
                shim = self.shim(root, "mv", 'destination=""\nfor arg in "$@"; do destination="$arg"; done\nif [ "$destination" = "' + final + '" ] || [ "$destination" = "' + relative + '" ]; then\n  if [ ! -e "' + str(root / "injected-once") + '" ]; then cp "' + installed + '" "' + observe + '"; touch "' + str(root / "injected-once") + '"; echo injected-publish >&2; exit 72; fi\nfi\n' + rollback + 'exec ' + self.tools["mv"] + ' "$@"\n')
                expected_error = "injected-publish"
            else:
                expected_error = None
            selected = variant if kind != "unknown-variant" else "adopted"
            output = self.stage(root, selected, shim)
            after = snapshot(shadow)
            record.update({"after": after, "exit": output.returncode})
            if kind in ("success", "replace"):
                require(output.returncode == 0, f"success route failed: {output.stderr}")
                self.valid(root, variant)
                other = "candidate" if variant == "baseline" else "baseline"
                require({p: v for p, v in before.items() if p.startswith(other + "/")} ==
                        {p: v for p, v in after.items() if p.startswith(other + "/")},
                        "other variant changed")
                prefix = f"{variant}/{PACKAGE}"
                def replaced_path(path):
                    return path == prefix or path.startswith(prefix + "/") or path == prefix + ".tar.gz"
                require({p: v for p, v in before.items() if not replaced_path(p)} ==
                        {p: v for p, v in after.items() if not replaced_path(p)},
                        "unknown sibling changed or owned temporary data survived")
                if kind == "success":
                    require(f"{prefix}/perllsp" not in before, "initial-publication fixture already staged")
                else:
                    require(before[f"{prefix}/perllsp"] != after[f"{prefix}/perllsp"],
                            "restage did not replace the old witness")
            else:
                require(output.returncode != 0, "negative control unexpectedly succeeded")
                if expected_error:
                    require(expected_error in output.stderr, f"wrong failure reason: {output.stderr}")
                else:
                    require(output.returncode == 2, "unknown variant is not a usage error")
                if kind in ("publish-failure", "rollback-failure"):
                    observed = root.joinpath("promotion-observed-perllsp").read_bytes()
                    record["promotion_observed"] = {"sha256": digest(observed), "bytes": len(observed)}
                    require(digest(observed) != before[f"{variant}/{PACKAGE}/perllsp"]["sha256"],
                            "rename failure was injected before new package promotion")
                if kind == "rollback-failure":
                    require("staging rollback incomplete; retained recovery data at " in output.stderr,
                            "failed rollback did not name retained recovery data")
                    record["retained_recovery"] = self.recover_owned(root, variant, before)
                    after = snapshot(shadow)
                    record.update({"after_recovery": after, "recovery_settled": True})
                require(before == after, "original witness/sibling hashes or temporary inventory changed")
            record["status"] = "PASS"
        except Exception as error:
            record.update({"status": "FAIL", "error": str(error), "after": snapshot(shadow)})
        record["commands"] = self.events[event_start:]
        return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--script", type=Path, default=Path(__file__).with_name("release_artifact_size_stage.sh"))
    parser.add_argument("--evidence-dir", type=Path, required=True, help="New owned directory; will not reuse an existing path")
    parser.add_argument("--case", choices=("all", "loss", "lifecycle"), default="all")
    args = parser.parse_args()
    script = args.script.resolve(strict=True)
    evidence = args.evidence_dir.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    shutil.copy2(script, evidence / "subject.sh")
    proof = Proof(evidence / "subject.sh", evidence)
    kinds = () if args.case == "lifecycle" else (("missing-second", "strip-second") if args.case == "loss" else (
        "success", "missing-second", "strip-second", "tar-create", "archive-mismatch", "publish-failure", "rollback-failure", "replace"))
    cases = [proof.case(kind, variant) for kind in kinds for variant in VARIANTS]
    if args.case == "all":
        cases.append(proof.case("unknown-variant", "baseline"))
    if args.case in ("all", "lifecycle"):
        for variant in VARIANTS:
            cases.append(proof.lifecycle_case("foreign-lock", variant))
            cases.append(proof.lifecycle_case("overlap-success", variant))
            cases.append(proof.lifecycle_case("overlap-rollback", variant))
            for boundary in ("package", "archive"):
                for signum in (signal.SIGINT, signal.SIGTERM):
                    cases.append(proof.lifecycle_case("signal", variant, boundary, signum))
    for case in cases:
        print(f'{case["status"]} {case["case"]}' + (f': {case["error"]}' if "error" in case else ""))
    passed = sum(case["status"] == "PASS" for case in cases)
    report = {"script": str(script), "script_sha256": digest(script.read_bytes()),
              "harness_sha256": digest(Path(__file__).read_bytes()), "host": platform.platform(),
              "claim_boundary": "native-host ELF/Mach-O staging lifecycle only; no native macOS safe-ICF measurement",
              "tools": proof.tools, "compile_commands": proof.commands,
              "native_inputs": snapshot(proof.templates), "passed": passed, "total": len(cases), "cases": cases}
    evidence.joinpath("report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f'{passed}/{len(cases)} passed; evidence: {evidence / "report.json"}')
    return 0 if passed == len(cases) else 1


if __name__ == "__main__":
    sys.exit(main())
