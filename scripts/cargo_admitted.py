#!/usr/bin/env python3
"""Bounded Cargo admission. No artifact deletion or ownership inference."""
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import socket
import stat
import subprocess
import sys
import uuid


class Denied(Exception):
    def __init__(self, message, details=None):
        super().__init__(message)
        self.details = details


def native_path(value):
    # Python on Windows does not understand Git Bash /c/... environment paths.
    if value.startswith(("\\\\", "//")):
        raise Denied("network/foreign storage is unsupported")
    if os.name == "nt" and value.startswith("/"):
        if re.match(r"^/[a-zA-Z]/", value):
            value = value[1] + ":" + value[2:]
        else:
            raise Denied("use an absolute native Windows path, not a WSL/MSYS path")
    if os.name != "nt" and (re.match(r"^[a-zA-Z]:", value) or value.startswith("/mnt/")):
        raise Denied("foreign Windows storage is unsupported; use host-local storage")
    path = Path(value).expanduser()
    if "{" in str(path) or "}" in str(path):
        raise Denied("Cargo template braces are unsupported in resource paths")
    if not path.is_absolute():
        raise Denied("storage paths must be absolute")
    # Reject links/junctions at every existing level rather than silently adopting
    # another resource's identity. Never delete or break a link.
    for part in (path, *path.parents):
        reparse = False
        try:
            reparse = bool(getattr(part.lstat(), "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))
        except FileNotFoundError:
            pass
        if part.is_symlink() or reparse:
            raise Denied("linked storage path is unsupported: " + str(part))
    return path.resolve()


def validate_args(args):
    allowed = {"build", "check", "test", "run", "bench", "doc"}
    if not args or args[0] not in allowed:
        raise Denied("expected build/check/test/run/bench/doc; aliases, +toolchain, clean and external commands are unsupported")
    for arg in args[1:]:
        if arg == "--":
            break  # arguments to the test/program, not Cargo configuration
        # Cargo/Clap accepts clusters such as -vj8. Refuse all joined short
        # tokens except pure verbosity, rather than duplicating its value grammar.
        if (arg.startswith("-") and not arg.startswith("--") and len(arg) > 2
                and not re.fullmatch(r"-v{2,}", arg)):
            raise Denied("joined short options/values are unsupported; spell them separately: " + arg)
        if (arg.startswith(("--config", "--target-dir", "--manifest-path", "--jobs", "-j", "-Z", "-C", "--lockfile-path", "--out-dir", "--artifact-dir", "--build-dir"))
                or arg.startswith("+")):
            raise Denied("configuration/path/job override is unsupported: " + arg)


# A policy file is an explicit caller declaration, not a measured forecast or
# an OS quota. Keep its reader and arithmetic bounded before any allocation.
BUDGET_FILE_LIMIT = 64 * 1024
MAX_BUDGET_BYTES = 2 ** 63 - 1
CAPACITY_ENV = ("CARGO_STORAGE_POLICY", "MIN_FREE_GB", "MAX_USED_PCT",
                "CARGO_EXPECTED_GROWTH_GB", "CARGO_STORAGE_BUDGET_EVIDENCE")


def admission_options(args):
    preflight, budget_file = False, None
    remaining = list(args)
    while remaining and remaining[0] in ("--preflight", "--budget-file"):
        option = remaining.pop(0)
        if option == "--preflight":
            if preflight:
                raise Denied("duplicate --preflight option")
            preflight = True
        else:
            if budget_file is not None or not remaining or not remaining[0]:
                raise Denied("--budget-file requires one path and may appear only once")
            budget_file = remaining.pop(0)
    validate_args(remaining)
    return preflight, budget_file, remaining


def budget_scope(args, env, worktree, paths, jobs):
    # Hash build-related overrides without printing their values (which may be
    # private). Forced output paths/jobs are represented by their effective
    # values below. This is request scope, not a full source/config/cache digest.
    forced = {"RUSTUP_AUTO_INSTALL", "CARGO_ADMITTED_RESOURCES", "CARGO_BUILD_JOBS", "CARGO_BUILD_BUILD_DIR",
              "CARGO_TARGET_DIR", "CARGO_HOME", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", *CAPACITY_ENV}
    build_env = {key: value for key, value in env.items()
                 if key not in forced and (key.startswith(("RUST", "CARGO_"))
                 or key in ("PATH", "CC", "CXX", "AR", "CFLAGS", "CXXFLAGS", "LDFLAGS"))}
    encoded = json.dumps(build_env, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return {"hostname": socket.gethostname(), "platform": sys.platform,
            "working_directory": str(Path.cwd().resolve()), "worktree": str(worktree),
            "operation": args[0],
            "cargo_argv_sha256": hashlib.sha256(json.dumps(args, ensure_ascii=True, separators=(",", ":")).encode("utf-8")).hexdigest(),
            "resources": {key: str(value) for key, value in paths.items()},
            "jobs": jobs, "build_environment_sha256": hashlib.sha256(encoded).hexdigest()}


def read_budget_file(filename, scope, env):
    if any(name in env for name in CAPACITY_ENV):
        raise Denied("--budget-file conflicts with capacity environment settings; unset them rather than silently overriding policy")
    path = native_path(filename)
    # Refuse links and non-regular files. O_NONBLOCK prevents an intervening FIFO
    # replacement from hanging this read; O_NOFOLLOW protects the final component
    # on platforms that provide it. This is not an adversarial filesystem sandbox.
    flags = os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        stream = os.fdopen(descriptor, "rb")
    except BaseException:
        # Ownership has not transferred, including on interruption.
        os.close(descriptor)
        raise
    with stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > BUDGET_FILE_LIMIT:
            raise Denied("budget file must be a regular file of at most 65536 bytes")
        raw = stream.read(BUDGET_FILE_LIMIT + 1)
    if len(raw) > BUDGET_FILE_LIMIT:
        raise Denied("budget file exceeds 65536 bytes")

    def object_pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise Denied("duplicate key in budget file")
            result[key] = value
        return result

    def invalid_constant(value):
        raise Denied("nonfinite JSON constant in budget file")

    try:
        data = json.loads(raw.decode("utf-8-sig"), object_pairs_hook=object_pairs,
                          parse_constant=invalid_constant)
    except (ValueError, RecursionError) as error:
        raise Denied("invalid budget JSON") from error
    fields = {"schema_version", "scope", "reserve_bytes", "expected_growth_bytes", "basis"}
    if not isinstance(data, dict) or set(data) != fields:
        raise Denied("budget file has missing or unknown fields")
    if type(data["schema_version"]) is not int or data["schema_version"] != 1:
        raise Denied("unsupported budget schema_version (expected integer 1)")
    # JSON comparison preserves distinctions Python equality would erase, such
    # as true == 1. Unknown scope fields, wrong cwd/argv/resources/jobs/overrides
    # are refusals, not approximations of the requested operation.
    if json.dumps(data["scope"], sort_keys=True) != json.dumps(scope, sort_keys=True):
        raise Denied("budget scope differs from this host/cwd/worktree/Cargo request; rerun --preflight and requalify the budget")
    for name in ("reserve_bytes", "expected_growth_bytes"):
        if type(data[name]) is not int or not 0 < data[name] <= MAX_BUDGET_BYTES:
            raise Denied(name + " must be a positive integer byte count within signed 64-bit range")
    if data["reserve_bytes"] + data["expected_growth_bytes"] > MAX_BUDGET_BYTES:
        raise Denied("reserve plus expected growth exceeds signed 64-bit range")
    basis = data["basis"]
    if not isinstance(basis, str) or not basis.strip() or len(basis) > 4096:
        raise Denied("budget basis must be a nonempty reference of at most 4096 characters")
    return {"policy": "budget-file", "reserve_bytes": data["reserve_bytes"],
            "expected_total_growth_bytes": data["expected_growth_bytes"],
            "evidence_reference": basis, "basis_verified": False,
            "budget_sha256": hashlib.sha256(raw).hexdigest()}


def check_capacity(paths, env, budget=None):
    if budget is not None:
        report = dict(budget)
        policy = report["policy"]
        required = report["reserve_bytes"] + report["expected_total_growth_bytes"]
    else:
        policy = env.get("CARGO_STORAGE_POLICY", "percent")
        if policy not in ("percent", "byte-budget"):
            raise Denied("unknown CARGO_STORAGE_POLICY")
        try:
            reserve_gib = float(env.get("MIN_FREE_GB", "40"))
            floor = reserve_gib * 1024 ** 3
            if not math.isfinite(floor) or floor <= 0:
                raise ValueError()
            report = {"policy": policy, "reserve_bytes": floor}
            if policy == "byte-budget":
                growth = float(env.get("CARGO_EXPECTED_GROWTH_GB", "")) * 1024 ** 3
                evidence = env.get("CARGO_STORAGE_BUDGET_EVIDENCE", "").strip()
                if reserve_gib < 40 or not math.isfinite(growth) or growth <= 0 or not evidence:
                    raise ValueError()
                required = floor + growth
                if not math.isfinite(required):
                    raise ValueError()
                report.update(expected_total_growth_bytes=growth, evidence_reference=evidence)
            else:
                ceiling = float(env.get("MAX_USED_PCT", "85"))
                if not math.isfinite(ceiling) or not 0 < ceiling <= 100:
                    raise ValueError()
                required = floor
                report["max_used_pct"] = ceiling
        except ValueError:
            raise Denied("invalid capacity policy inputs; byte-budget requires reserve >=40 GiB, positive finite total growth and an evidence reference")

    report["required_free_bytes"] = required
    observations = []
    for path in paths:
        ancestor = path
        while not ancestor.exists():
            parent = ancestor.parent
            if parent == ancestor:
                raise Denied("no existing resource ancestor: " + str(path))
            ancestor = parent
        if not ancestor.is_dir():
            raise Denied("resource ancestor is not a directory: " + str(ancestor))
        try:
            usage = shutil.disk_usage(ancestor)
        except OSError as error:
            raise Denied("disk observation unavailable at " + str(path),
                         {"path": str(path), "reason": "capacity_unavailable"}) from error
        if (type(usage.total) is not int or type(usage.free) is not int
                or usage.total <= 0 or not 0 <= usage.free <= usage.total):
            raise Denied("invalid disk observation at " + str(path),
                         {"path": str(path), "reason": "capacity_unavailable"})
        # The same TOTAL growth bound is checked independently at every
        # destination. Never sum it for paths sharing a volume. Applying the
        # entire bound on different volumes is intentionally conservative.
        row = {"path": str(path), "observed_ancestor": str(ancestor),
               "free_bytes": usage.free, "required_free_bytes": required,
               "headroom_bytes": usage.free - required}
        if usage.free < required or (policy == "percent" and
                (usage.total - usage.free) * 100 / usage.total >= ceiling):
            row["shortfall_bytes"] = max(0, required - usage.free)
            row["used_pct"] = (usage.total - usage.free) * 100 / usage.total
            raise Denied("insufficient disk headroom at " + str(path)
                         + " free_bytes=" + str(usage.free)
                         + " required_free_bytes=" + str(required)
                         + " shortfall_bytes=" + str(row["shortfall_bytes"])
                         + " used_pct=" + str(row["used_pct"]), row)
        observations.append(row)
    report["destinations"] = observations
    return report


def git_path(*args, env=None):
    # Read bytes: text mode also translates CR/LF that may belong to a POSIX
    # filename. Git emits one terminal LF; preserve every preceding path byte.
    output = subprocess.check_output(["git", "rev-parse", *args], env=env)
    if not output.endswith(b"\n"):
        raise Denied("Git path output is missing its terminal newline")
    return native_path(os.fsdecode(output[:-1]))


def resource_plan(env):
    # Cargo uses the invocation directory, not Git's repository-location
    # overrides. Refuse an ambiguous subject instead of silently retargeting it.
    for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"):
        if name in env:
            raise Denied(name + " repository-location override is unsupported; unset it and invoke from the intended worktree")
    worktree = git_path("--show-toplevel", env=env)
    common = git_path("--path-format=absolute", "--git-common-dir", env=env)
    identity = str(common).casefold() if os.name == "nt" else str(common)
    key = hashlib.sha256(os.fsencode(identity)).hexdigest()[:16]
    base = native_path(env.get("DEVPLANE", str(Path.home() / ".cache" / "devplane")))
    # Keep one admission/lease domain per common repository and OS/host.
    # Cargo fingerprints deliberately omit the workspace absolute path, so
    # serializing divergent worktrees does not make their build state compatible.
    slot = base / (socket.gethostname() + "-" + sys.platform + "-" + key)
    # native_path already resolves the actual root; do not invent Unicode/case
    # equivalence between distinct filesystem paths. fsencode is lossless on POSIX.
    worktree_key = hashlib.sha256(os.fsencode(worktree)).hexdigest()
    private = slot / "worktrees" / worktree_key
    target, build = private / "target", private / "build"
    for name, expected in (("CARGO_TARGET_DIR", target), ("CARGO_BUILD_BUILD_DIR", build)):
        if env.get(name) and native_path(env[name]) != expected:
            raise Denied(name + " override unsupported; use this worktree's private target/build paths")
    paths = {"target": target, "build": build,
             "cargo_home": native_path(env.get("CARGO_HOME", str(Path.home() / ".cargo"))),
             "temp": native_path(env.get("TMPDIR", env.get("TEMP", str(slot / "tmp"))))}
    # Windows programs commonly use TEMP/TMP rather than TMPDIR: unify all three.
    for path in (slot, *paths.values()):
        native_path(str(path))
    return worktree, slot, paths


def directory_identity(path):
    metadata = path.lstat()
    if not stat.S_ISDIR(metadata.st_mode) or path.is_symlink():
        raise Denied("lease directory was replaced: " + str(path))
    return metadata.st_dev, metadata.st_ino


def owns_lease(lock, identity, marker):
    try:
        return directory_identity(lock) == identity and directory_identity(marker) is not None
    except (OSError, Denied):
        return False


def release_lease(lock, identity, marker=None):
    # A copied token cannot transfer the original directory's ownership. Never
    # recursively delete a lease or release replacement/unrecognized evidence.
    try:
        if directory_identity(lock) != identity:
            return
        if marker is not None:
            if not owns_lease(lock, identity, marker):
                return
            marker.rmdir()
        lock.rmdir()
    except (OSError, Denied):
        # Retaining evidence is safer than masking the operation's result.
        pass


def main(args=None):
    args = sys.argv[1:] if args is None else args
    preflight, scope = False, None
    try:
        preflight, budget_file, args = admission_options(args)
        env = os.environ.copy()
        env["RUSTUP_AUTO_INSTALL"] = "0"
        worktree, slot, paths = resource_plan(env)
        if env.get("RUSTC_WRAPPER") or env.get("RUSTC_WORKSPACE_WRAPPER"):
            raise Denied("compiler wrappers have unverified storage; use a separately admitted route")
        jobs = int(env.get("CARGO_BUILD_JOBS", "2"))
        if not 1 <= jobs <= 4:
            raise Denied("CARGO_BUILD_JOBS must be between 1 and 4")
        scope = budget_scope(args, env, worktree, paths, jobs)
        budget = read_budget_file(budget_file, scope, env) if budget_file else None
        admission = check_capacity([slot, *paths.values()], env, budget)
        lock = slot / "cargo-active"
        if preflight:
            if os.path.lexists(lock):
                raise Denied("slot is active or awaits owner verification: " + str(lock))
            print("cargo-admitted preflight: " + json.dumps({
                "verdict": "PASS", "scope": scope, "admission": admission,
                "lease": str(lock), "lease_acquired": False,
                "limitation": "read-only snapshot; not a reservation or a verified growth measurement",
            }), file=sys.stderr, flush=True)
            return 0
        slot.mkdir(parents=True, exist_ok=True)
        try:
            lock.mkdir()
        except FileExistsError:
            raise Denied("slot is active or awaits owner verification: " + str(lock))
        identity = directory_identity(lock)
        marker = lock / ("owner-" + uuid.uuid4().hex)
        try:
            marker.mkdir()
        except BaseException:
            release_lease(lock, identity)
            raise
        # Once launch is attempted, preserve the conservative interrupted/spawn
        # failure rule. Earlier refusal can release only this unlaunched lease.
        completed, launch_attempted = False, False
        try:
            admission = check_capacity([slot, *paths.values()], env, budget)
            for path in (slot, *paths.values()):
                if native_path(str(path)) != path:
                    raise Denied("resource path changed after admission: " + str(path))
            for path in paths.values():
                path.mkdir(parents=True, exist_ok=True)
            env.update(CARGO_TARGET_DIR=str(paths["target"]),
                       CARGO_BUILD_BUILD_DIR=str(paths["build"]),
                       CARGO_HOME=str(paths["cargo_home"]), CARGO_INCREMENTAL="0",
                       CARGO_BUILD_JOBS=str(jobs), TMPDIR=str(paths["temp"]),
                       TEMP=str(paths["temp"]), TMP=str(paths["temp"]))
            descriptor = {"worktree": str(worktree), "scope": scope,
                          "resources": {k: str(v) for k, v in paths.items()},
                          "lease": str(lock), "pid": os.getpid(), "admission": admission, "disposition": "retained with reason: reusable bounded slot"}
            # Preparation children consume the same live lease/resources,
            # sequentially before libtest; this is not a nested admission slot.
            descriptor["lease_marker"] = str(marker)
            env["CARGO_ADMITTED_RESOURCES"] = json.dumps(descriptor)
            print("cargo-admitted resources: " + json.dumps(descriptor), file=sys.stderr, flush=True)
            command = ["cargo", "--config", "unstable.unstable-options=false",
                       "--config", "build.build-dir=" + json.dumps(str(paths["build"])),
                       "--config", 'build.rustc-wrapper=""',
                       "--config", 'build.rustc-workspace-wrapper=""',
                       *args[:1], "--target-dir", str(paths["target"]), *args[1:]]
            # Cargo's [env] force=true can override process environment for
            # rustc/build scripts. Override those config entries as well.
            for name in ("TEMP", "TMP", "TMPDIR", "RUSTUP_AUTO_INSTALL"):
                child_value = "0" if name == "RUSTUP_AUTO_INSTALL" else str(paths["temp"])
                for field, value in (("value", json.dumps(child_value)), ("force", "true"), ("relative", "false")):
                    command[1:1] = ["--config", "env." + name + "." + field + "=" + value]
            for path in (slot, *paths.values()):
                if native_path(str(path)) != path:
                    raise Denied("resource path changed before launch: " + str(path))
            if not owns_lease(lock, identity, marker):
                raise Denied("lease ownership changed before Cargo launch")
            launch_attempted = True
            result = subprocess.call(command, env=env)
            # Cargo uses 101 for ordinary failure and may also panic with 101.
            # Retain for signals/NT termination/unfamiliar exits, but no exit
            # status proves all descendants ended: root verifies other consumers.
            completed = result in (0, 101)
            return result
        finally:
            if completed or not launch_attempted:
                release_lease(lock, identity, marker)
            else:
                print("cargo-admitted: lease retained; root must verify all consumers before release", file=sys.stderr)
    except (Denied, OSError, ValueError, subprocess.CalledProcessError) as error:
        print("cargo-admitted: DENY: " + str(error), file=sys.stderr)
        if preflight:
            print("cargo-admitted preflight: " + json.dumps({
                "verdict": "DENY", "scope": scope, "lease_acquired": False,
                "reason": str(error), "observation": getattr(error, "details", None),
            }), file=sys.stderr, flush=True)
        return 75


if __name__ == "__main__":
    sys.exit(main())
