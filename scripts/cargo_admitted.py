#!/usr/bin/env python3
"""Bounded Cargo admission. No artifact deletion or ownership inference."""
import hashlib
import ctypes
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import socket
import signal
import stat
import subprocess
import sys
import time
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
    if args and args[0] == "clippy":
        validate_clippy(args)
        return
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


def validate_clippy(args):
    # One staged developer proof shape, not a general external-command or flag
    # adapter. Keep package/profile/work explicit; no help/version/fix/no-work.
    if os.name != "posix" or sys.platform != "linux":
        raise Denied("staged Clippy currently admits Linux only; other native hosts require qualification")
    if len(args) < 10 or args[-3:] != ["--", "-D", "warnings"]:
        raise Denied("Clippy requires -p PACKAGE --all-targets --profile agent --locked -- -D warnings")
    fields = args[1:-3]
    seen = set()
    while fields:
        flag = fields.pop(0)
        if flag in seen:
            raise Denied("duplicate Clippy field: " + flag)
        seen.add(flag)
        if flag in ("-p", "--profile"):
            if not fields:
                raise Denied("missing Clippy field value: " + flag)
            value = fields.pop(0)
            if flag == "-p" and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", value):
                raise Denied("Clippy requires one explicit package name, not a pattern/path/spec")
            if flag == "--profile" and value != "agent":
                raise Denied("staged Clippy requires profile agent")
        elif flag not in ("--all-targets", "--locked", "--offline"):
            raise Denied("unsupported staged Clippy field: " + flag)
    if not {"-p", "--profile", "--all-targets", "--locked"} <= seen:
        raise Denied("Clippy requires explicit package/all-targets/agent/locked fields")


CLIPPY_LINT_ARGS = "-D__CLIPPY_HACKERY__warnings__CLIPPY_HACKERY__"


def clippy_environment(env):
    # Direct executable selectors, compiler flags and loader injection cannot
    # silently change the identified toolchain or weaken this finite lint proof.
    for name, value in env.items():
        if (name.startswith(("CLIPPY_", "LD_", "DYLD_")) or name in (
                "CARGO", "RUSTC", "RUSTDOC", "SYSROOT", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS",
                "CARGO_BUILD_RUSTFLAGS", "CARGO_BUILD_RUSTC", "CARGO_BUILD_RUSTDOC",
                "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
                "CARGO_BUILD_TARGET")) and value:
            raise Denied("unsupported Clippy executable/compiler environment selector: " + name)


def file_subject(path):
    path = native_path(str(path))
    before = path.stat()
    if not stat.S_ISREG(before.st_mode) or before.st_size > 512 * 1024 ** 2:
        raise Denied("toolchain subject must be a bounded regular file: " + str(path))
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(64 * 1024), b""):
            digest.update(chunk)
    after = path.stat()
    facts = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    if facts(before) != facts(after):
        raise Denied("toolchain subject changed during observation: " + str(path))
    return {"path": str(path), "file_identity": facts(after), "sha256": digest.hexdigest()}


def clippy_toolchain(env, worktree):
    # Resolve the existing rustup installation at the repository pin, never a
    # proxy/PATH search or an installation transition. This is local file identity
    # binding, not a signed-distribution authenticity claim or OS sandbox.
    pin_file = worktree / "rust-toolchain.toml"
    if pin_file.stat().st_size > 16384:
        raise Denied("toolchain pin is oversized")
    try:
        import tomllib
    except ImportError:
        raise Denied("staged Clippy requires Python 3.11+ for toolchain TOML validation")
    try:
        table = tomllib.loads(pin_file.read_text(encoding="utf-8")).get("toolchain", {})
    except (ValueError, UnicodeError) as error:
        raise Denied("invalid repository toolchain pin: " + str(error))
    pin = table.get("channel") if isinstance(table, dict) else None
    if not isinstance(pin, str) or not re.fullmatch(r"\d+\.\d+\.\d+", pin) or "path" in table:
        raise Denied("Clippy requires one numeric repository toolchain pin")
    if pin != "1.95.0":
        raise Denied("staged Clippy driver protocol is qualified only for repository pin 1.95.0")
    machine = platform.machine().lower()
    machine = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    host = {("linux", "x86_64"): "x86_64-unknown-linux-gnu",
            ("linux", "aarch64"): "aarch64-unknown-linux-gnu",
            ("darwin", "x86_64"): "x86_64-apple-darwin",
            ("darwin", "aarch64"): "aarch64-apple-darwin"}.get((sys.platform, machine))
    if host is None:
        raise Denied("Clippy toolchain host is unsupported")
    selected = pin + "-" + host
    if env.get("RUSTUP_TOOLCHAIN") not in (None, "", pin, selected):
        raise Denied("RUSTUP_TOOLCHAIN differs from the repository's native pinned toolchain")
    home = native_path(env.get("RUSTUP_HOME", str(Path.home() / ".rustup")))
    root = native_path(str(home / "toolchains" / selected))
    names = ("cargo", "rustc", "rustdoc", "cargo-clippy", "clippy-driver")
    subjects = {name: file_subject(root / "bin" / name) for name in names}
    runtime = sorted(set(root.glob("lib/librustc_driver*")) | set(root.glob("lib/libLLVM*")))
    if not runtime:
        raise Denied("installed compiler runtime identity is unavailable")
    subjects.update({"runtime-" + str(i): file_subject(path) for i, path in enumerate(runtime)})
    # Do not run tools during preflight. Bounded version observation belongs to
    # the already-owned lease, immediately before the first resource-bearing call.
    return {"pin": pin, "host": host, "root": str(root), "subjects": subjects,
            "pin_subject": file_subject(pin_file)}


def revalidate_clippy_toolchain(plan):
    for expected in (*plan["subjects"].values(), plan["pin_subject"]):
        if file_subject(Path(expected["path"])) != expected:
            raise Denied("Clippy toolchain identity changed before launch: " + expected["path"])


def clippy_version_check(plan, env, tree=None):
    versions = {}
    for name in ("cargo", "rustc", "cargo-clippy", "clippy-driver"):
        command = [plan["subjects"][name]["path"], "-vV" if name == "rustc" else "--version"]
        result = (tree.probe(command, env) if tree else
                  subprocess.run(command, env=env, capture_output=True, timeout=15, check=True))
        if len(result.stdout) + len(result.stderr) > 16384:
            raise Denied("Clippy setup output exceeded its observation bound")
        versions[name] = result.stdout.decode("utf-8").strip()
    rustc = versions["rustc"]
    if (not versions["cargo"].startswith("cargo " + plan["pin"] + " ")
            or "\nrelease: " + plan["pin"] + "\n" not in rustc + "\n"
            or "\nhost: " + plan["host"] + "\n" not in rustc + "\n"):
        raise Denied("Cargo/rustc version or native host differs from the pinned subject")
    match = re.search(r"^commit-hash: ([0-9a-f]{40})$", rustc, re.M)
    clippy = re.fullmatch(r"clippy 0\.1\.(\d+) \(([0-9a-f]{10,40}) \d{4}-\d{2}-\d{2}\)", versions["cargo-clippy"])
    if (match is None or clippy is None or versions["cargo-clippy"] != versions["clippy-driver"]
            or clippy[1] != plan["pin"].split(".")[1] or not match[1].startswith(clippy[2])):
        raise Denied("Clippy driver/wrapper commit differs from the pinned compiler")
    revalidate_clippy_toolchain(plan)
    return versions


def clippy_configuration(worktree, paths):
    try:
        import tomllib
    except ImportError:
        raise Denied("staged Clippy requires Python 3.11+ for bounded Cargo TOML validation")
    candidates = {worktree / "clippy.toml", worktree / ".clippy.toml"}
    for directory in (Path.cwd(), *Path.cwd().parents, paths["cargo_home"]):
        candidates.update(directory / ".cargo" / name for name in ("config", "config.toml"))
    candidates.update(paths["cargo_home"] / name for name in ("config", "config.toml"))
    observed = []
    for path in sorted(candidates):
        if not os.path.lexists(path):
            continue
        subject = file_subject(path)
        if path.name in ("config", "config.toml"):
            if path.stat().st_size > 65536:
                raise Denied("staged Clippy Cargo configuration exceeds 64 KiB")
            try:
                config = tomllib.loads(path.read_text(encoding="utf-8"))
            except (ValueError, UnicodeError) as error:
                raise Denied("invalid staged Clippy Cargo configuration: " + str(error))
            if "include" in config:
                raise Denied("staged Clippy does not admit Cargo configuration includes")
            child_env = config.get("env", {})
            if not isinstance(child_env, dict):
                raise Denied("invalid Cargo environment table")
            if any(name.startswith(("LD_", "DYLD_")) for name in child_env):
                raise Denied("staged Clippy does not admit Cargo loader environment entries")
            if file_subject(path) != subject:
                raise Denied("Cargo configuration changed during validation")
        observed.append(subject)
    return observed


class ClippyTree:
    """One exclusively owned Linux CLI scope; not a general process executor.

    Kernel adoption/waits cover detached descendants. Unrelated daemons need no
    inspection. Only our own children file may locate cancellation candidates;
    waitability + pidfds authorize signals, never a numeric PID/PPID guess.
    """
    WALL = 0x40000000  # Linux __WALL also includes non-SIGCHLD clone children.

    def __init__(self):
        if platform.python_implementation() != "CPython" or len(list(Path('/proc/self/task').iterdir())) != 1:
            raise Denied("Clippy tree requires a single native CPython thread")
        if (not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal")
                or not hasattr(os, "P_PIDFD") or signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL):
            raise Denied("Clippy tree requires pidfd waits/signals and default SIGCHLD")
        self.libc = ctypes.CDLL(None, use_errno=True)
        prior = ctypes.c_int()
        if self.libc.prctl(37, ctypes.byref(prior), 0, 0, 0) != 0 or prior.value != 0:
            raise Denied("Clippy tree requires an unclaimed subreaper scope")
        try:
            os.waitid(os.P_ALL, 0, os.WEXITED | os.WNOHANG | os.WNOWAIT | self.WALL)
        except ChildProcessError:
            pass
        else:
            raise Denied("Clippy tree refuses existing unrelated children")
        fd = os.pidfd_open(os.getpid())
        try:
            try:
                os.waitid(os.P_PIDFD, fd, os.WEXITED | os.WNOHANG | os.WNOWAIT | self.WALL)
            except ChildProcessError:
                pass  # self is not a child: pidfd wait support confirmed.
            else:
                raise Denied("invalid Clippy pidfd child-wait probe")
        finally:
            os.close(fd)
        # CPython PyOS_setsig installs fresh sigaction flags (SA_ONSTACK),
        # explicitly clearing SA_NOCLDWAIT rather than inspecting only handler.
        signal.signal(signal.SIGCHLD, signal.SIG_DFL)
        if self.libc.prctl(36, 1, 0, 0, 0) != 0:
            raise Denied("native Clippy subreaper setup failed")
        if self.libc.prctl(37, ctypes.byref(prior), 0, 0, 0) != 0 or prior.value != 1:
            raise Denied("native Clippy subreaper ownership was not established")
        self.handles, self.reaped, self.errors = {}, [], []
        self.cancelled = False
        self.stopping = False
        self.binding = False
        self.previous = {}
        for signum in (signal.SIGINT, signal.SIGTERM):
            self.previous[signum] = signal.signal(signum, self.interrupted)

    def interrupted(self, signum, frame):
        self.cancelled = True
        if self.binding:
            return  # defer until the just-spawned child's handle is recorded.
        raise KeyboardInterrupt("Clippy cancelled; owned descendants must settle")

    def track(self, pid, process=None):
        if pid in self.handles:
            return
        fd = os.pidfd_open(pid)
        try:
            os.waitid(os.P_PIDFD, fd, os.WEXITED | os.WNOHANG | os.WNOWAIT | self.WALL)
        except BaseException:
            os.close(fd)
            raise
        self.handles[pid] = (fd, process)

    def probe(self, command, env):
        self.binding = True
        try:
            process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, start_new_session=True)
            self.track(process.pid, process)
        finally:
            self.binding = False
        if self.cancelled:
            raise KeyboardInterrupt("Clippy probe cancelled after child binding")
        stdout, stderr = process.communicate(timeout=15)
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
        result.check_returncode()
        return result

    def child_ids(self):
        try:
            text = (Path('/proc/self/task') / str(os.getpid()) / 'children').read_text()
        except FileNotFoundError:
            return []  # unavailable: never substitute a whole-host scan.
        if len(text) > 65536:
            raise Denied("owned child table exceeds observation bound")
        return [int(pid) for pid in text.split()]

    def finish(self, stop=False, timeout=30, grace=2):
        self.stopping |= stop or self.cancelled
        started = time.monotonic()
        settled = False
        child_table_available = (Path('/proc/self/task') / str(os.getpid()) / 'children').exists()
        def pending(signum, frame):
            self.cancelled = True  # repeated cancellation cannot interrupt reap.
            self.stopping = True
        for signum in self.previous:
            signal.signal(signum, pending)
        try:
            while time.monotonic() - started < timeout:
                if len(list(Path('/proc/self/task').iterdir())) != 1:
                    raise Denied("Clippy tree acquired a competing native thread")
                owned = ctypes.c_int()
                if (self.libc.prctl(37, ctypes.byref(owned), 0, 0, 0) != 0 or owned.value != 1
                        or signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL):
                    raise Denied("Clippy tree lost subreaper/reaping ownership")
                # Only one reaper is active: all Popen wait/communicate calls
                # have returned or unwound before this adopted-child drain.
                while True:
                    try:
                        pid, status = os.waitpid(-1, os.WNOHANG | self.WALL)
                    except ChildProcessError:
                        settled = True  # ECHILD is positive kernel closure.
                        break
                    if pid == 0:
                        break  # live children remain; never treat as closure.
                    self.reaped.append({"pid": pid, "exit_code": os.waitstatus_to_exitcode(status)})
                    if pid in self.handles:
                        fd, process = self.handles.pop(pid)
                        if process is not None:
                            process.returncode = os.waitstatus_to_exitcode(status)
                        os.close(fd)
                if settled:
                    break
                if self.stopping:
                    child_path = Path('/proc/self/task') / str(os.getpid()) / 'children'
                    child_table_available = child_path.exists()
                    for pid in self.child_ids():
                        self.track(pid)
                    for pid, (fd, process) in list(self.handles.items()):
                        if process is not None and process.returncode is not None:
                            continue  # Popen already reaped this known leader.
                        # Validate kernel waitability, then signal its bound
                        # descriptor. ECHILD/ambiguity never authorizes a signal.
                        os.waitid(os.P_PIDFD, fd, os.WEXITED | os.WNOHANG | os.WNOWAIT | self.WALL)
                        try:
                            signal.pidfd_send_signal(fd, signal.SIGKILL if time.monotonic() - started >= grace else signal.SIGTERM)
                        except ProcessLookupError:
                            pass
                time.sleep(0.05)
        except (OSError, Denied, ValueError) as error:
            self.errors.append(str(error))
        finally:
            if settled:
                for fd, process in self.handles.values():
                    os.close(fd)
                self.handles.clear()
                if self.libc.prctl(36, 0, 0, 0, 0) != 0:
                    self.errors.append("cannot restore settled subreaper scope")
            # Unsettled descendants retain the subreaper and handles until this
            # owner exits; no false transfer/restoration or generic reclamation.
            for signum, handler in self.previous.items():
                signal.signal(signum, handler)
        return {"tree_settled": settled and not self.errors,
                "proof": "kernel ECHILD (__WALL)" if settled else None,
                "cancelled": self.cancelled, "termination_requested": self.stopping,
                "reaped_descendants": self.reaped,
                "owned_child_table_available": child_table_available,
                "errors": self.errors, "pending_bound_pids": [pid for pid, (fd, process) in self.handles.items()
                                                               if process is None or process.returncode is None],
                "reason": "all owned descendants reaped" if settled else "owned descendants unproven; deadline/error retains lease"}


def call_clippy(command, env, lock, tree=None, operation="Clippy"):
    # Staged ownership model: attributable POSIX group for native cancellation,
    # but never infer whole-tree settlement from this leader's exit. The caller
    # retains its lease until the optional owned kernel tree scope proves closure.
    process = None
    detached = operation == "Clippy"
    if not detached and tree is None:
        raise Denied("builtin Cargo requires its bound native owner")
    published = False
    pending = []
    previous = {}
    def cancelled(signum, frame):
        if not published:
            pending[:] = [signum]
            return
        # The unreaped leader retains its PID. If already reaped, preserve the
        # lease and let native owner verification handle descendants; do not
        # signal a group using an unbound/reusable numeric ID.
        if process.returncode is None:
            try:
                if detached:
                    os.killpg(process.pid, signal.SIGTERM)
                # Builtin Cargo shares its caller's group: unwind into the
                # existing tree owner, which signals only bound, waitable pidfds.
            except ProcessLookupError:
                pass
        raise KeyboardInterrupt(operation + " cancelled; lease retained for native owner verification")
    try:
        for signum in (signal.SIGINT, signal.SIGTERM):
            previous[signum] = signal.signal(signum, cancelled)
        process = subprocess.Popen(command, env=env, start_new_session=detached)
        if tree is not None:
            tree.track(process.pid, process)
        print("cargo-admitted " + operation + " launch: " + json.dumps({"pid": process.pid,
              "process_group": process.pid if detached else os.getpgrp(),
              "session_inherited": not detached, "lease": str(lock), "host": socket.gethostname(),
              "terminality": "awaiting owner verification"}), file=sys.stderr, flush=True)
        published = True
        if pending:
            cancelled(pending[0], None)
        while True:
            # Block delivery only across bounded wait/reap and returncode update.
            # Otherwise a signal between waitpid and that update could target a
            # recycled numeric group ID. A timed-out leader remains unreaped and
            # reserves its PID until the next masked wait. Cancellation latency
            # from this critical section is bounded to 0.2 seconds.
            mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
            try:
                try:
                    return process.wait(timeout=0.2)
                except subprocess.TimeoutExpired:
                    pass
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, mask)
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


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
    forced = {"RUSTUP_AUTO_INSTALL", "CARGO_BUILD_JOBS", "CARGO_BUILD_BUILD_DIR",
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


def owns_lease(lock, identity, marker, marker_identity):
    try:
        return directory_identity(lock) == identity and directory_identity(marker) == marker_identity
    except (OSError, Denied):
        return False


def release_lease(lock, identity, marker=None, marker_identity=None):
    # A copied token cannot transfer the original directory's ownership. Never
    # recursively delete a lease or release replacement/unrecognized evidence.
    try:
        if directory_identity(lock) != identity:
            return
        if marker is not None:
            if not owns_lease(lock, identity, marker, marker_identity):
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
        clippy = args[0] == "clippy"
        owned_linux = sys.platform == "linux"
        managed = clippy or owned_linux
        operation = "Clippy" if clippy else "Cargo"
        env = os.environ.copy()
        env["RUSTUP_AUTO_INSTALL"] = "0"
        worktree, slot, paths = resource_plan(env)
        if env.get("RUSTC_WRAPPER") or env.get("RUSTC_WORKSPACE_WRAPPER"):
            raise Denied("compiler wrappers have unverified storage; use a separately admitted route")
        toolchain = None
        if clippy:
            if Path.cwd().resolve() != worktree:
                raise Denied("staged Clippy must be invoked from the canonical worktree root")
            clippy_environment(env)
            toolchain = clippy_toolchain(env, worktree)
        jobs = int(env.get("CARGO_BUILD_JOBS", "2"))
        if not 1 <= jobs <= 4:
            raise Denied("CARGO_BUILD_JOBS must be between 1 and 4")
        scope = budget_scope(args, env, worktree, paths, jobs)
        if clippy:
            scope["clippy_setup"] = {"toolchain": toolchain,
                                     "configuration": clippy_configuration(worktree, paths),
                                     "renderer": file_subject(Path(__file__))}
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
            marker_identity = directory_identity(marker)
        except BaseException:
            release_lease(lock, identity)
            raise
        # Once launch is attempted, preserve the conservative interrupted/spawn
        # failure rule. Earlier refusal can release only this unlaunched lease.
        completed, launch_attempted = False, False
        tree, result, interrupted = None, None, False
        try:
            admission = check_capacity([slot, *paths.values()], env, budget)
            if not owns_lease(lock, identity, marker, marker_identity):
                raise Denied("lease ownership changed before resource allocation")
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
            if managed:
                # Persist the original owner observation before any setup/product
                # child starts. Recovery must not adopt a replacement directory
                # or reconstruct ownership from age/current contents after death.
                descriptor.update(lease_identity=list(identity), lease_marker=str(marker),
                                  marker_identity=list(marker_identity))
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
            if clippy:
                exact = toolchain["subjects"]
                controlled = {"CARGO": exact["cargo"]["path"],
                              "RUSTC": exact["rustc"]["path"], "RUSTDOC": exact["rustdoc"]["path"],
                              "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": exact["clippy-driver"]["path"],
                              "SYSROOT": toolchain["root"],
                              "CARGO_ENCODED_RUSTFLAGS": "", "RUSTFLAGS": "",
                              "CLIPPY_ARGS": CLIPPY_LINT_ARGS, "CLIPPY_CONF_DIR": str(worktree),
                              "CARGO_TARGET_DIR": str(paths["target"]),
                              "CARGO_BUILD_BUILD_DIR": str(paths["build"]),
                              "CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": str(jobs)}
                env.update(controlled)
                # Override forced Cargo [env] as well, before nested Cargo sees
                # compiler/lint inputs. Real lint fail/clean controls test this.
                for name, value in controlled.items():
                    for field, setting in (("value", json.dumps(value)), ("force", "true"), ("relative", "false")):
                        command[1:1] = ["--config", "env." + name + "." + field + "=" + setting]
                position = command.index("clippy")
                command = [exact["cargo-clippy"]["path"], "clippy", "--message-format=json-render-diagnostics",
                           *command[1:position], "--target", toolchain["host"], *command[position + 1:]]
                if clippy_configuration(worktree, paths) != scope["clippy_setup"]["configuration"]:
                    raise Denied("Clippy configuration changed after admission")
                revalidate_clippy_toolchain(toolchain)
                # Version probes are setup, not package work. Hold the same
                # conservative lease before invoking these installed tools.
                tree = ClippyTree()
                print("cargo-admitted Clippy tree: " + json.dumps({"owner_pid": os.getpid(),
                      "subreaper": True, "prior_children": "none", "single_native_thread": True,
                      "lease": str(lock)}), file=sys.stderr, flush=True)
                launch_attempted = True
                versions = clippy_version_check(toolchain, env, tree)
                print("cargo-admitted Clippy setup: " + json.dumps(versions), file=sys.stderr, flush=True)
                if clippy_configuration(worktree, paths) != scope["clippy_setup"]["configuration"]:
                    raise Denied("Clippy configuration changed during setup")
            elif owned_linux:
                # Reuse the proven owner, without changing builtin Cargo policy,
                # toolchain resolution or argument/environment construction.
                tree = ClippyTree()
                print("cargo-admitted Cargo tree: " + json.dumps({"owner_pid": os.getpid(),
                      "subreaper": True, "prior_children": "none", "single_native_thread": True,
                      "lease": str(lock)}), file=sys.stderr, flush=True)
            for path in (slot, *paths.values()):
                if native_path(str(path)) != path:
                    raise Denied("resource path changed before launch: " + str(path))
            if not owns_lease(lock, identity, marker, marker_identity):
                raise Denied("lease ownership changed before Cargo launch")
            launch_attempted = True
            if clippy:
                result = call_clippy(command, env, lock, tree)
            elif owned_linux:
                result = call_clippy(command, env, lock, tree, operation="Cargo")
            else:
                result = subprocess.call(command, env=env)
            # Cargo uses 101 for ordinary failure and may also panic with 101.
            # Retain for signals/NT termination/unfamiliar exits, but no exit
            # status proves all descendants ended: root verifies other consumers.
            completed = not managed and result in (0, 101)
            if managed:
                print("cargo-admitted " + operation + " product: " + json.dumps({"exit_code": result,
                      "lease": str(lock), "terminality": "awaiting owner verification",
                      "lease_released": False}), file=sys.stderr, flush=True)
        except KeyboardInterrupt:
            if not managed:
                raise
            interrupted = True
            if tree is not None:
                tree.cancelled = True
        finally:
            settlement = None
            if tree is not None:
                if sys.exc_info()[0] is KeyboardInterrupt:
                    tree.cancelled = True
                settlement = tree.finish(stop=interrupted or sys.exc_info()[0] is not None)
                completed = settlement["tree_settled"]
            if completed or not launch_attempted:
                try:
                    release_lease(lock, identity, marker, marker_identity)
                except KeyboardInterrupt:
                    if not managed:
                        raise
                    interrupted = True
            else:
                print("cargo-admitted: lease retained; root must verify all consumers before release", file=sys.stderr)
            if settlement is not None:
                released = not os.path.lexists(lock)
                settlement["cancelled"] = interrupted or settlement.get("cancelled", False)
                print("cargo-admitted " + operation + " settlement: " + json.dumps({**settlement,
                      "lease": str(lock), "lease_identity": list(identity), "lease_marker": str(marker),
                      "marker_identity": list(marker_identity),
                      "lease_released": released}), file=sys.stderr, flush=True)
                # A positive product code cannot mask failed terminality/release.
                completed = completed and released
        if managed:
            if not completed:
                return 75
            if interrupted or (settlement is not None and settlement.get("cancelled", False)):
                return 130
        return result
    except (Denied, OSError, ValueError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        print("cargo-admitted: DENY: " + str(error), file=sys.stderr)
        if preflight:
            print("cargo-admitted preflight: " + json.dumps({
                "verdict": "DENY", "scope": scope, "lease_acquired": False,
                "reason": str(error), "observation": getattr(error, "details", None),
            }), file=sys.stderr, flush=True)
        return 75


if __name__ == "__main__":
    sys.exit(main())
