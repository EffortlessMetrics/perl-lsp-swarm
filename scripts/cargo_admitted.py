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
    return {"path": str(path), "file_identity": list(facts(after)), "sha256": digest.hexdigest()}


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


def clippy_configuration(worktree, paths, directory=None):
    try:
        import tomllib
    except ImportError:
        raise Denied("staged Clippy requires Python 3.11+ for bounded Cargo TOML validation")
    candidates = {worktree / "clippy.toml", worktree / ".clippy.toml"}
    directory = Path.cwd() if directory is None else directory
    for ancestor in (directory, *directory.parents, paths["cargo_home"]):
        candidates.update(ancestor / ".cargo" / name for name in ("config", "config.toml"))
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
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        raise KeyboardInterrupt(operation + " cancelled; lease retained for native owner verification")
    try:
        for signum in (signal.SIGINT, signal.SIGTERM):
            previous[signum] = signal.signal(signum, cancelled)
        process = subprocess.Popen(command, env=env, start_new_session=True)
        if tree is not None:
            tree.track(process.pid, process)
        print("cargo-admitted " + operation + " launch: " + json.dumps({"pid": process.pid,
              "process_group": process.pid, "lease": str(lock), "host": socket.gethostname(),
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


# Finite compatibility request fields for #17479, not a second executor or a
# completeness certificate for the selected gate/runtime inventory.
NESTED_PACKAGES = ("perl-dap", "perl-incremental-parsing", "perl-lsp-perltidy",
                   "perl-lsp-rs", "perl-lsp-rs-core", "perl-parser",
                   "perl-parser-bench", "perllsp", "xtask")
DISALLOWED_FIXTURE_ROW = "xtask-disallowed-fields-fixture"
DISALLOWED_TEST_ROW = "xtask-disallowed-fields-test"
DISALLOWED_TEST = "tasks::check_lint_policy::tests::config::disallowed_fields::configured_field_is_rejected_by_clippy"
DISALLOWED_FILES = ("Cargo.toml", "Cargo.lock", "clippy.toml", "src/lib.rs")
NESTED_COMMANDS = {
    DISALLOWED_FIXTURE_ROW: ("clippy", "--offline", "--quiet", "--lib", "--no-deps", "--message-format=json", "--", "-D", "warnings"),
    DISALLOWED_TEST_ROW: ("test", "-p", "xtask", "--bin", "xtask", "--locked", "--message-format=json", DISALLOWED_TEST,
                          "--", "--exact", "--test-threads=1", "--color", "never"),
    "parser-check": ("check", "--package", "perl-parser", "--message-format", "json", "--locked", "--offline"),
    "parser-build": ("build", "--package", "perl-parser", "--locked", "--offline"),
    "parser-clippy": ("clippy", "--package", "perl-parser", "--locked", "--offline", "--", "-D", "warnings"),
    "parser-lib": ("test", "--package", "perl-parser", "--lib", "--locked", "--offline"),
    "dap-lsp-build": ("build", "-p", "perl-lsp-rs", "--message-format=short", "--locked"),
    "dap-core-build": ("build", "-p", "perl-lsp-rs-core", "--message-format=short", "--locked"),
    "dap-bin-build": ("build", "-p", "perl-dap", "--bin", "perl-dap", "--locked"),
    "dap-clippy": ("clippy", "-p", "perl-dap", "--lib", "--locked", "--", "-D", "warnings", "-A", "clippy::wildcard_imports"),
    "perllsp-build": ("build", "-p", "perllsp", "--locked", "--message-format=json"),
    "perllsp-handoff-support": ("test", "-p", "perl-lsp-rs", "--locked", "--test", "cli_smoke", "health_prints_ok",
                                "--", "--exact", "--test-threads=1", "--color", "never"),
    "perllsp-handoff-common": ("test", "-p", "perl-lsp-rs", "--locked", "--test", "binary_version_test",
                               "lsp_server_version_matches_crate_version", "--", "--exact", "--test-threads=1", "--color", "never"),
    "parser-doc": ("doc", "--no-deps", "--package", "perl-parser"),
    "parser-tree": ("tree", "-p", "perl-parser", "--edges", "normal"),
    "incremental-metadata": ("metadata", "--no-deps", "--format-version", "1"),
    "xtask-build": ("build", "-p", "xtask", "--locked"),
    "lsp-inline-compile": ("test", "-p", "perl-lsp-rs", "--locked", "--test", "lsp_inline_completion_registration_tests", "--no-run"),
    "core-inline-compile": ("test", "-p", "perl-lsp-rs-core", "--locked", "--lib", "inline_completion", "--no-run"),
}
for _name in ("cross_file_goto_definition_tests", "navigation_regression_tests"):
    NESTED_COMMANDS["navigation-" + _name] = ("test", "-p", "perl-lsp-rs", "--locked", "--test", _name,
                                            "--", "--test-threads=1", "--color", "never")
for _name, _suffix in (("routed-compile", ("--no-run",)), ("routed-runtime", ())):
    NESTED_COMMANDS[_name] = ("test", "--locked", "--tests", *sum((("-p", p) for p in NESTED_PACKAGES), ()), *_suffix)


def bounded_json(path):
    path = native_path(str(path))
    subject = file_subject(path)
    if subject["file_identity"][2] > BUDGET_FILE_LIMIT:
        raise Denied("nested request exceeds 64 KiB")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise Denied("duplicate nested request key")
            result[key] = value
        return result
    try:
        data = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique)
    except (ValueError, UnicodeError) as error:
        raise Denied("invalid nested request JSON") from error
    if file_subject(path) != subject:
        raise Denied("nested request changed during observation")
    return data, subject


def nested_source(worktree, env=None):
    # Exact committed Git subject, not a claim of hermetic build-script inputs.
    def git(*args):
        return subprocess.check_output(["git", "-C", str(worktree), *args], text=True,
                                       env={k: v for k, v in (os.environ if env is None else env).items() if not k.startswith("GIT_")}).strip()
    if git("status", "--porcelain", "--untracked-files=all"):
        raise Denied("nested plan requires a clean exact committed source")
    return {"head": git("rev-parse", "HEAD"), "lock": file_subject(worktree / "Cargo.lock")}


def compiler_environment(env):
    flags = {"RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_ENCODED_RUSTDOCFLAGS",
             "CARGO_BUILD_RUSTFLAGS", "CARGO_BUILD_RUSTDOCFLAGS"}
    return {k: v for k, v in env.items() if k in flags or k.startswith("CARGO_PROFILE_")
            or (k.startswith("CARGO_TARGET_") and k.endswith(("_RUSTFLAGS", "_RUSTDOCFLAGS")))}


def nested_flags(values, docs=False):
    # A finite compatibility cell: codegen debug choices cannot cap/suppress
    # lints or change cfg/targets/plugins. Unknown flags need their own owner.
    for name, value in values.items():
        if name.startswith("CARGO_PROFILE_"):
            continue
        tokens = value.split("\x1f") if "ENCODED" in name else value.split()
        tokens = [v for v in tokens if v]
        if docs and tokens == ["-W", "missing_docs"]:
            continue
        while tokens:
            token = tokens.pop(0)
            if token == "-C" and tokens:
                token += tokens.pop(0)
            if token not in ("-Cdebuginfo=0", "-Cdebuginfo=1", "-Cdebuginfo=2", "-Cdebuginfo=line-tables-only"):
                raise Denied("unsupported nested compiler/doc flag input: " + name)


def nested_selectors(env):
    if any(env.get(name) for name in ("GIT_SSH", "GIT_SSH_COMMAND", "GIT_PROXY_COMMAND")):
        raise Denied("nested Git network executable selector is unsupported")
    if env.get("RUSTC_BOOTSTRAP"):
        raise Denied("nested bootstrap compiler selector is unsupported")
    if any(name.startswith("CARGO_TARGET_") and name.endswith(("_LINKER", "_RUNNER")) and value
           for name, value in env.items()):
        raise Denied("nested target linker/runner requires executable identity qualification")


def nested_build_environment(env):
    return {k: v for k, v in env.items() if k in ("PATH", "CC", "CXX", "AR", "CFLAGS", "CXXFLAGS", "LDFLAGS")}


def nested_network(env):
    return {k: v for k, v in env.items() if k.startswith(("CARGO_NET_", "CARGO_HTTP_"))
            or k.upper() in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
                             "SSL_CERT_FILE", "SSL_CERT_DIR", "CURL_CA_BUNDLE")}


def nested_plan(filename, env, worktree, paths):
    if sys.platform != "linux":
        raise Denied("nested plan requires qualified native Linux ownership")
    data, request_subject = bounded_json(filename)
    if (not isinstance(data, dict) or set(data) != {"schema_version", "rows"}
            or type(data["schema_version"]) is not int or data["schema_version"] != 1
            or not isinstance(data["rows"], list) or not 1 <= len(data["rows"]) <= len(NESTED_COMMANDS)
            or any(not isinstance(r, str) or r not in NESTED_COMMANDS for r in data["rows"])
            or len(set(data["rows"])) != len(data["rows"])):
        raise Denied("unknown/duplicate nested operation or request field")
    # Permit declared ordinary compiler/profile flags, but never ambient tool,
    # wrapper or loader selectors. Each effective value is frozen in the plan.
    nested_flags(compiler_environment(env))
    nested_selectors(env)
    selectors = dict(env)
    for name in compiler_environment(env):
        selectors.pop(name, None)
    clippy_environment(selectors)
    toolchain = clippy_toolchain(env, worktree)
    fixture = None
    if DISALLOWED_FIXTURE_ROW in data["rows"] or DISALLOWED_TEST_ROW in data["rows"]:
        if DISALLOWED_FIXTURE_ROW not in data["rows"]:
            raise Denied("owning disallowed-fields test requires its finite fixture row")
        fixture = disallowed_fixture(worktree, paths)
    plan = {"request": data, "request_subject": request_subject,
            "source": nested_source(worktree), "toolchain": toolchain,
            "configuration": nested_configuration(worktree, paths, (Path(fixture["cwd"]),) if fixture else ()),
            "compiler_environment": compiler_environment(env), "network_environment": nested_network(env), "build_environment": nested_build_environment(env),
            "renderer": file_subject(Path(__file__))}
    if fixture:
        plan["disallowed_fields_fixture"] = fixture
    return plan


def disallowed_fixture(worktree, paths):
    """Only this checked-in dependency-free fixture; never arbitrary manifests.

    Inputs and both output directories must already exist before admission.
    Captured identities supplement fixed paths and exact template bytes.
    """
    root = native_path(str(paths["temp"] / "disallowed-fields-17479"))
    template = worktree / ".spec/17479-nested-admission/disallowed-fields-fixture"
    files = {}
    for name in DISALLOWED_FILES:
        if any(path.stat().st_size > BUDGET_FILE_LIMIT for path in (template / name, root / name)):
            raise Denied("disallowed-fields fixture input exceeds bounded template")
        expected, actual = file_subject(template / name), file_subject(root / name)
        if expected["file_identity"][2] > BUDGET_FILE_LIMIT or actual["sha256"] != expected["sha256"]:
            raise Denied("disallowed-fields fixture differs from its current template: " + name)
        files[name] = {"template": expected, "generated": actual}
    directories = {}
    for name, path in (("cwd", root), ("target", root / "target"), ("build", root / "build")):
        native_path(str(path))
        directories[name] = {"path": str(path), "identity": list(directory_identity(path))}
    return {"cwd": str(root), "directories": directories, "files": files,
            "adapter": file_subject(worktree / "scripts/ci/disallowed_fields_prepare.py"),
            "python": file_subject(Path(sys.executable).resolve(strict=True))}


def nested_configuration(worktree, paths, extra_directories=()):
    # Include package-local config discovery for the two declared non-root rows.
    observed = {}
    for directory in (worktree, worktree / "crates/perl-parser", worktree / "crates/perl-incremental-parsing", *extra_directories):
        for item in clippy_configuration(worktree, paths, directory):
            if Path(item["path"]).name in ("config", "config.toml"):
                import tomllib
                config = tomllib.loads(Path(item["path"]).read_text(encoding="utf-8"))
                def inspect(table):
                    for key, value in table.items():
                        if key in ("rustflags", "rustdocflags"):
                            text = "\x1f".join(value) if isinstance(value, list) else value
                            if not isinstance(text, str):
                                raise Denied("invalid nested configured compiler flags")
                            nested_flags({"CARGO_ENCODED_RUSTFLAGS": text})
                        if key in ("linker", "runner", "rustc", "rustdoc", "rustc-wrapper", "rustc-workspace-wrapper", "target") and value and not isinstance(value, dict):
                            raise Denied("unsupported nested configuration selector: " + key)
                        if isinstance(value, dict):
                            inspect(value)
                inspect(config)
                for name, setting in config.get("env", {}).items():
                    value = setting.get("value", "") if isinstance(setting, dict) else setting
                    nested_selectors({name: value})
                    projected = compiler_environment({name: value})
                    nested_flags(projected)
                    if not projected:
                        clippy_environment({name: value})
                        if name in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER") and value:
                            raise Denied("unsupported nested configured wrapper")
            observed[item["path"]] = item

    return [observed[key] for key in sorted(observed)]


def process_fact(pid):
    # Directed ancestry only, never a whole-host process scan or environment read.
    raw = (Path('/proc') / str(pid) / 'stat').read_text()
    fields = raw[raw.rindex(')') + 2:].split()
    return {"pid": pid, "parent": int(fields[1]), "start": fields[19]}


def nested_descendant(owner):
    current = process_fact(os.getpid())
    for _ in range(128):
        if current["pid"] == owner["pid"]:
            if current != owner:
                raise Denied("nested owner process identity changed")
            return
        if current["parent"] <= 1:
            break
        current = process_fact(current["parent"])
    raise Denied("nested caller is not in the live owner's ancestry")


def nested_runtime_contract(args, paths, toolchain):
    # Only the observed native debug xtask bootstrap has a loader protocol.
    # Cargo's general build-script link-search extension remains unsupported.
    prefix = args[:args.index("--")] if "--" in args else args
    if prefix not in (["run", "-p", "xtask", "--bin", "xtask", "--locked"],
                      ["run", "-p", "xtask", "--bin", "xtask", "--locked", "--offline"]):
        return None
    return {"profile": "debug", "executable": str(paths["target"] / "debug/xtask"),
            "loader_paths": [str(paths["target"] / "debug"),
                             str(paths["build"] / "debug/deps"),
                             str(Path(toolchain["root"]) / "lib/rustlib" / toolchain["host"] / "lib")]}


def nested_runtime_ancestry(owner, runtime):
    # Native Linux cargo run execs xtask in the original product-root process.
    # Require that exact executable as a direct child of the existing owner,
    # rather than inventing a live Cargo ancestor after exec replacement.
    current = process_fact(os.getpid())
    for _ in range(128):
        if current == owner:
            break
        executable = Path('/proc') / str(current["pid"]) / 'exe'
        selected = runtime["executable"]
        if os.readlink(executable) == selected:
            expected = native_path(selected).stat()
            running = executable.stat()
            if (expected.st_dev, expected.st_ino) != (running.st_dev, running.st_ino):
                raise Denied("nested Cargo runtime executable identity changed")
            if current["parent"] == owner["pid"] and process_fact(current["parent"]) == owner:
                return
        if current["parent"] <= 1:
            break
        current = process_fact(current["parent"])
    raise Denied("nested loader lacks the bound direct-owner xtask runtime ancestry")


def nested_runtime_loader(env, plan, receipt, paths):
    if "LD_LIBRARY_PATH" not in env:
        return
    runtime = plan.get("cargo_runtime")
    expected = nested_runtime_contract(["run", "-p", "xtask", "--bin", "xtask", "--locked"], paths, plan["toolchain"])
    if runtime != expected or env["LD_LIBRARY_PATH"] != ":".join(expected["loader_paths"]):
        raise Denied("nested Cargo runtime loader list is not the bound exact protocol")
    for value in expected["loader_paths"]:
        path = native_path(value)
        if str(path) != value or not path.is_dir():
            raise Denied("nested Cargo runtime loader path changed")
    nested_runtime_ancestry(receipt["owner_process"], runtime)


def nested_command(row, env=None):
    """Render one bound leaf; no resource-bearing spawn or lease transition.

    Validation may invoke read-only Git commands under the existing owner.

    Consumers retain their existing logging/watchdog/result semantics. The
    original Linux owner supervises every descendant through final ECHILD.
    This is not a security sandbox for arbitrary same-user repository code.
    """
    env = dict(os.environ if env is None else env)
    try:
        receipt = json.loads(env["CARGO_ADMITTED_RESOURCES"])
        snapshot, subject = bounded_json(receipt["nested_snapshot"]["path"])
        if subject != receipt["nested_snapshot"] or snapshot["descriptor"] != {k: v for k, v in receipt.items() if k != "nested_snapshot"}:
            raise Denied("nested descriptor/snapshot mismatch")
        nested_descendant(receipt["owner_process"])
        plan = snapshot["plan"]
        lock, marker = native_path(receipt["lease"]), native_path(receipt["lease_marker"])
        if (marker.parent != lock
                or directory_identity(lock) != tuple(receipt["lease_identity"])
                or directory_identity(marker) != tuple(receipt["marker_identity"])):
            raise Denied("nested original lease/marker identity changed")
        clippy_environment({k: v for k, v in env.items() if k.startswith(("LD_", "DYLD_")) and k != "LD_LIBRARY_PATH"})
        revalidate_clippy_toolchain(plan["toolchain"])
        bound_paths = {k: native_path(v) for k, v in receipt["resources"].items()}
        nested_runtime_loader(env, plan, receipt, bound_paths)
        validation_env = dict(env)
        validation_env.pop("LD_LIBRARY_PATH", None)
        worktree, slot, paths = resource_plan(validation_env)
        if str(worktree) != receipt["worktree"] or {k: str(v) for k,v in paths.items()} != receipt["resources"]:
            raise Denied("nested canonical resources changed")
        if lock != slot / "cargo-active":
            raise Denied("nested canonical lease changed")
        for key, variable in (("target", "CARGO_TARGET_DIR"), ("build", "CARGO_BUILD_BUILD_DIR"),
                              ("cargo_home", "CARGO_HOME"), ("temp", "TMPDIR"), ("temp", "TEMP"), ("temp", "TMP")):
            if env.get(variable) != str(paths[key]):
                raise Denied("nested environment/resource mismatch: " + variable)
        if env.get("CARGO_BUILD_JOBS") != str(receipt["scope"]["jobs"]) or env.get("CARGO_INCREMENTAL") != "0" or env.get("RUSTUP_AUTO_INSTALL") != "0":
            raise Denied("nested resource control changed")
        selectors = dict(env)
        # Validate below, after the original owner and executable checks.
        # Only this one Cargo-generated variable can cross the runtime seam.
        selectors.pop("LD_LIBRARY_PATH", None)
        for name in compiler_environment(env):
            selectors.pop(name, None)
        for name, key in (("CARGO", "cargo"), ("RUSTC", "rustc"), ("RUSTDOC", "rustdoc")):
            if selectors.get(name) != plan["toolchain"]["subjects"][key]["path"]:
                raise Denied("nested executable selector changed: " + name)
            selectors.pop(name)
        clippy_environment(selectors)
        if selectors.get("RUSTC_WRAPPER") or selectors.get("RUSTC_WORKSPACE_WRAPPER"):
            raise Denied("nested compiler wrapper changed")
        if row not in plan["request"]["rows"] or row not in NESTED_COMMANDS:
            raise Denied("nested operation is not a member of the admitted plan")
        # Even read-only Git must not inherit a runtime loader projection.
        if nested_source(worktree, validation_env) != plan["source"] or file_subject(Path(__file__)) != plan["renderer"]:
            raise Denied("nested source/renderer changed")
        fixture = plan.get("disallowed_fields_fixture")
        if fixture is not None and disallowed_fixture(worktree, paths) != fixture:
            raise Denied("disallowed-fields fixture inputs/cwd/output roots changed")
        if row in (DISALLOWED_FIXTURE_ROW, DISALLOWED_TEST_ROW) and fixture is None:
            raise Denied("disallowed-fields fixture is not bound to this workload")
        if nested_configuration(worktree, paths, (Path(fixture["cwd"]),) if fixture else ()) != plan["configuration"]:
            raise Denied("nested Cargo/Clippy configuration changed")
        expected = dict(plan["compiler_environment"])
        if row == "parser-doc":
            expected["RUSTFLAGS"] = "-W missing_docs"
            expected["CARGO_ENCODED_RUSTFLAGS"] = "-W\x1fmissing_docs"
        actual = compiler_environment(env)
        nested_flags(actual, docs=row == "parser-doc")
        nested_selectors(env)
        if nested_build_environment(env) != plan["build_environment"]:
            raise Denied("nested build-tool environment changed")
        if nested_network(env) != plan["network_environment"]:
            raise Denied("nested network environment changed")
        if env.get("RUSTUP_TOOLCHAIN") not in (None, "", plan["toolchain"].get("pin"), plan["toolchain"].get("pin", "") + "-" + plan["toolchain"].get("host", "")):
            raise Denied("nested rustup selector changed")
        if actual != plan["compiler_environment"] and actual != expected:
            raise Denied("nested compiler/profile environment changed")
        env.pop("LD_LIBRARY_PATH", None)
        env.update(expected)
        return render_nested(row, env, worktree, paths, plan["toolchain"], fixture)
    except (KeyError, TypeError, ValueError, OSError) as error:
        raise Denied("invalid/stale nested handoff: " + str(error)) from error


def render_nested(row, env, worktree, paths, toolchain, fixture=None):
    args = list(NESTED_COMMANDS[row])
    cwd = worktree / "crates/perl-parser" if row == "parser-doc" else worktree
    if row == "incremental-metadata":
        cwd = worktree / "crates/perl-incremental-parsing"
    if row == DISALLOWED_FIXTURE_ROW:
        cwd = Path(fixture["cwd"])
        paths = {**paths, **{name: Path(fixture["directories"][name]["path"]) for name in ("target", "build")}}
    exact = toolchain["subjects"]
    controlled = {"CARGO": exact["cargo"]["path"], "RUSTC": exact["rustc"]["path"],
                  "RUSTDOC": exact["rustdoc"]["path"], "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": "",
                  "CARGO_TARGET_DIR": str(paths["target"]), "CARGO_BUILD_BUILD_DIR": str(paths["build"]),
                  "CARGO_BUILD_JOBS": env["CARGO_BUILD_JOBS"], "CARGO_INCREMENTAL": "0",
                  "RUSTUP_AUTO_INSTALL": "0", **compiler_environment(env)}
    if args[0] == "clippy":
        controlled.update(RUSTC_WORKSPACE_WRAPPER=exact["clippy-driver"]["path"],
                          SYSROOT=toolchain["root"], CLIPPY_CONF_DIR=str(cwd),
                          CLIPPY_ARGS="".join(value + "__CLIPPY_HACKERY__" for value in args[args.index("--") + 1:]))
    for name in ("TEMP", "TMP", "TMPDIR"):
        controlled[name] = str(paths["temp"])
    if row == DISALLOWED_TEST_ROW:
        controlled["CARGO_ADMITTED_FIXTURE_PYTHON"] = fixture["python"]["path"]
    env.update(controlled)
    command = [exact["cargo"]["path"]]
    for name, value in controlled.items():
        for field, setting in (("value", json.dumps(value)), ("force", "true"), ("relative", "false")):
            command += ["--config", "env." + name + "." + field + "=" + setting]
    command += ["--config", "unstable.unstable-options=false", "--config", "build.build-dir=" + json.dumps(str(paths["build"])),
                "--config", 'build.rustc-wrapper=""', "--config", 'build.rustc-workspace-wrapper=""', args[0]]
    if args[0] not in ("metadata", "tree"):
        command += ["--target-dir", str(paths["target"])]
    command += args[1:]
    if args[0] == "clippy":
        position = command.index("clippy")
        command = [exact["cargo-clippy"]["path"], "clippy", *command[1:position], *command[position + 1:]]
    return command, env, cwd


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
    args = list(sys.argv[1:] if args is None else args)
    if args[:1] == ["--nested-row"]:
        try:
            if len(args) != 2:
                raise Denied("--nested-row accepts one bound row identifier, no raw arguments")
            command, child_env, cwd = nested_command(args[1])
            os.chdir(cwd)
            os.execve(command[0], command, child_env)
        except (Denied, OSError) as error:
            print("cargo-admitted nested: DENY: " + str(error), file=sys.stderr)
            return 75
    nested_file = None
    if args[:1] == ["--nested-plan"]:
        if len(args) < 3:
            print("cargo-admitted: --nested-plan requires path and Cargo request", file=sys.stderr)
            return 75
        nested_file, args = args[1], args[2:]
    preflight, scope = False, None
    try:
        preflight, budget_file, args = admission_options(args)
        clippy = args[0] == "clippy"
        owned_linux = sys.platform == "linux"
        managed = clippy or owned_linux
        operation = "Clippy" if clippy else "Cargo"
        env = os.environ.copy()
        if env.get("CARGO_ADMITTED_RESOURCES"):
            raise Denied("nested callers must use bound leaf rendering, not reacquire a parent lease")
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
        nested = nested_plan(nested_file, env, worktree, paths) if nested_file else None
        if nested is not None:
            if clippy or Path.cwd().resolve() != worktree:
                raise Denied("nested owner requires a builtin operation at workspace root")
            nested["cargo_runtime"] = nested_runtime_contract(args, paths, nested["toolchain"])
            scope["nested_plan_sha256"] = hashlib.sha256(json.dumps(nested, sort_keys=True).encode()).hexdigest()
            toolchain = nested["toolchain"]
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
            if nested is not None:
                descriptor.update(owner_process=process_fact(os.getpid()))
                snapshot_path = slot / ("nested-plan-" + marker.name + ".json")
                payload = json.dumps({"descriptor": descriptor, "plan": nested}, sort_keys=True).encode()
                if len(payload) > BUDGET_FILE_LIMIT:
                    raise Denied("owned nested snapshot exceeds 64 KiB")
                fd = os.open(snapshot_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(fd, "wb") as stream:
                    stream.write(payload)
                descriptor["nested_snapshot"] = file_subject(snapshot_path)
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
                if nested is not None:
                    launch_attempted = True
                    clippy_version_check(toolchain, env, tree)
                    if (nested_source(worktree) != nested["source"]
                            or nested_configuration(worktree, paths) != nested["configuration"]):
                        raise Denied("nested setup changed before launch")
                    command[0] = toolchain["subjects"]["cargo"]["path"]
                    for name, key in (("CARGO", "cargo"), ("RUSTC", "rustc"), ("RUSTDOC", "rustdoc")):
                        env[name] = toolchain["subjects"][key]["path"]
                    env["RUSTC_WRAPPER"] = env["RUSTC_WORKSPACE_WRAPPER"] = ""
                    for name in ("CARGO", "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", *compiler_environment(env)):
                        for field, value in (("value", json.dumps(env[name])), ("force", "true"), ("relative", "false")):
                            command[1:1] = ["--config", "env." + name + "." + field + "=" + value]
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
