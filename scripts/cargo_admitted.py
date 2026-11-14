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
import selectors
import shutil
import socket
import signal
import stat
import subprocess
import sys
import threading
import time
import uuid


class Denied(Exception):
    def __init__(self, message, details=None):
        super().__init__(message)
        self.details = details


def local_config_cli_args(worktree):
    path = worktree / ".cargo" / "config.local.toml"
    if path.is_file():
        return ["--config", str(path)]
    return []
class CapacityRefused(Denied):
    """Observed disk headroom is insufficient for the selected policy."""


class CargoConfigRefused(Denied):
    """Noncompiling Cargo setup could not validate the child environment."""


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
        raise Denied("Clippy requires -p PACKAGE (--all-targets | --test NAME) --profile agent --locked -- -D warnings")
    fields = args[1:-3]
    seen = set()
    while fields:
        flag = fields.pop(0)
        if flag in seen:
            raise Denied("duplicate Clippy field: " + flag)
        seen.add(flag)
        if flag in ("-p", "--profile", "--test"):
            if not fields:
                raise Denied("missing Clippy field value: " + flag)
            value = fields.pop(0)
            if flag == "-p" and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", value):
                raise Denied("Clippy requires one explicit package name, not a pattern/path/spec")
            if flag == "--profile" and value != "agent":
                raise Denied("staged Clippy requires profile agent")
            if flag == "--test" and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", value):
                raise Denied("Clippy requires one literal test target name, not a pattern/path/option")
        elif flag not in ("--all-targets", "--locked", "--offline", "--keep-going"):
            raise Denied("unsupported staged Clippy field: " + flag)
    if not {"-p", "--profile", "--locked"} <= seen:
        raise Denied("Clippy requires explicit package/agent/locked fields")
    if len(seen & {"--all-targets", "--test"}) != 1:
        raise Denied("Clippy requires exactly one of --all-targets or --test NAME")


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
    if pin != "1.100.0":
        raise Denied("staged Clippy driver protocol is qualified only for repository pin 1.100.0")
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
    local_config = worktree / ".cargo" / "config.local.toml"
    if local_config.is_file():
        candidates.add(local_config)
    for directory in (Path.cwd(), *Path.cwd().parents, paths["cargo_home"]):
        candidates.update(directory / ".cargo" / name for name in ("config", "config.toml"))
    candidates.update(paths["cargo_home"] / name for name in ("config", "config.toml"))
    observed = []
    for path in sorted(candidates):
        if not os.path.lexists(path):
            continue
        subject = file_subject(path)
        if path.name in ("config", "config.toml", "config.local.toml"):
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


def _proc_read(path, limit):
    with open(path, 'rb') as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise Denied('proc-parent observation exceeds byte bound')
    return data.decode('utf-8', 'surrogateescape')


def proc_parent_child_ids():
    """Nominate visible direct children; only pidfd child waits bind authority."""
    started = time.monotonic()
    pid, tid = os.getpid(), threading.get_native_id()
    if sys.platform != 'linux' or platform.python_implementation() != 'CPython':
        raise Denied('proc-parent discovery requires native Linux CPython')
    if pid != tid or [entry.name for entry in Path('/proc/self/task').iterdir()] != [str(tid)]:
        raise Denied('proc-parent discovery requires the sole native owner thread')
    if signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL:
        raise Denied('proc-parent discovery requires default SIGCHLD')
    try:
        status = _proc_read('/proc/self/status', 65536)
        identities = {}
        for line in status.splitlines():
            key, separator, value = line.partition(':')
            if separator and key in ('Pid', 'Tgid', 'NSpid', 'NStgid'):
                if key in identities or value.split() != [str(pid)]:
                    raise Denied('proc-parent namespace identity is ambiguous')
                identities[key] = value
        if len(identities) != 4 or os.readlink('/proc/self') != str(pid) or os.readlink('/proc/thread-self') != f'{pid}/task/{tid}':
            raise Denied('proc-parent namespace does not match native owner')
        if os.readlink('/proc/self/ns/pid') != os.readlink('/proc/self/ns/pid_for_children'):
            raise Denied('proc-parent child namespace differs from owner')
        mounts = _proc_read('/proc/self/mountinfo', 1024 * 1024)
        roots = []
        for line in mounts.splitlines():
            before, separator, after = line.partition(' - ')
            fields, filesystem = before.split(), after.split()
            if not separator or len(fields) < 6 or len(filesystem) < 3:
                raise Denied('proc-parent mount observation is malformed')
            point = fields[4]
            if re.match(r'^/proc/(?:[0-9]+|self|thread-self)(?:/|$)', point):
                raise Denied('proc-parent numeric/task projection is overmounted')
            if point == '/proc':
                roots.append((fields, filesystem))
        # Stacked mounts may share /proc; even a hidden layer must be complete.
        if not roots or any(fields[3] != '/' or filesystem[0] != 'proc'
                            for fields, filesystem in roots):
            raise Denied('proc-parent discovery requires complete procfs roots at /proc')
        for fields, filesystem in roots:
            options = fields[5].split(',') + filesystem[2].split(',')
            if any(option.startswith('hidepid=') and option != 'hidepid=0' for option in options):
                raise Denied('proc-parent discovery refuses hidden PID visibility')
        root_identity = directory_identity(Path('/proc'))
        if time.monotonic() - started > 1:
            raise Denied('proc-parent discovery exceeds bounded turn')
        children, count = [], 0
        with os.scandir('/proc') as entries:
            for entry in entries:
                if not entry.name.isascii() or not entry.name.isdecimal():
                    continue
                count += 1
                if count > 32768 or time.monotonic() - started > 1:
                    raise Denied('proc-parent discovery exceeds bounded turn')
                try:
                    data = _proc_read('/proc/' + entry.name + '/stat', 65536)
                except (FileNotFoundError, ProcessLookupError):
                    continue  # A vanished candidate cannot authorize a signal.
                name, fields = data.rsplit(')', 1)
                identity, opening, _ = name.partition('(')
                native_pid = int(identity.strip())
                values = fields.split()
                if not opening or native_pid != int(entry.name) or native_pid <= 0 or len(values) < 20:
                    raise Denied('proc-parent stat identity is malformed')
                parent, birth = int(values[1]), int(values[19])
                if parent < 0 or birth < 0:
                    raise Denied('proc-parent stat identity is malformed')
                if parent == pid:
                    children.append(native_pid)
        if directory_identity(Path('/proc')) != root_identity:
            raise Denied('proc-parent root changed during discovery')
        if time.monotonic() - started > 1:
            raise Denied('proc-parent discovery exceeds bounded turn')
        return children
    except (OSError, ValueError, IndexError) as error:
        raise Denied('proc-parent ownership observation unavailable or malformed') from error


def child_discovery_capability(mode=None):
    """Probe an explicit discovery profile, not closure or signal authority."""
    mode = os.environ.get('CARGO_ADMITTED_CHILD_DISCOVERY', 'proc-children') if mode is None else mode
    if mode == 'proc-parent-pidfd':
        proc_parent_child_ids()
        return {'mode': mode, 'source': '/proc/<pid>/stat parent candidates',
                'authority': 'kernel pidfd child wait; final ECHILD (__WALL)'}
    if mode != 'proc-children':
        raise Denied('unsupported owned-child discovery profile')
    path = Path('/proc/self/task') / str(threading.get_native_id()) / 'children'
    try:
        data = path.read_text()
        if len(data) > 65536 or any(not token.isascii() or not token.isdecimal() for token in data.split()):
            raise Denied('invalid owned child table')
    except OSError as error:
        raise Denied('owned-child discovery requires readable live-thread proc children') from error
    return {'mode': mode, 'source': str(path),
            'authority': 'kernel pidfd child wait; final ECHILD (__WALL)'}


class ClippyTree:
    """One exclusively owned Linux CLI scope; not a general process executor.

    Kernel adoption/waits cover detached descendants. Unrelated daemons need no
    inspection in the default profile. An explicit proc-parent profile only
    nominates candidates; waitability + pidfds authorize signals, never PPID.
    """
    WALL = 0x40000000  # Linux __WALL also includes non-SIGCHLD clone children.

    def __init__(self, child_discovery=None):
        if platform.python_implementation() != "CPython" or len(list(Path('/proc/self/task').iterdir())) != 1:
            raise Denied("Clippy tree requires a single native CPython thread")
        if (not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal")
                or not hasattr(os, "P_PIDFD") or signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL):
            raise Denied("Clippy tree requires pidfd waits/signals and default SIGCHLD")
        self.child_discovery = os.environ.get('CARGO_ADMITTED_CHILD_DISCOVERY', 'proc-children') if child_discovery is None else child_discovery
        if self.child_discovery != 'proc-children':
            child_discovery_capability(self.child_discovery)
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
            fd, prior_process = self.handles[pid]
            if prior_process is None or prior_process.returncode is None:
                return
            # Popen already consumed this leader. Its numeric PID can now name
            # a new adopted child; never let the stale returncode hide it.
            self.handles.pop(pid)
            os.close(fd)
        fd = os.pidfd_open(pid)
        try:
            os.waitid(os.P_PIDFD, fd, os.WEXITED | os.WNOHANG | os.WNOWAIT | self.WALL)
        except BaseException:
            os.close(fd)
            raise
        self.handles[pid] = (fd, process)

    def probe(self, command, env, *, timeout=15, output_limit=16384):
        deadline = time.monotonic() + timeout
        self.binding = True
        try:
            process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, start_new_session=True)
            self.track(process.pid, process)
        finally:
            self.binding = False
        output = {process.stdout: bytearray(), process.stderr: bytearray()}
        observed = 0
        try:
            if self.cancelled:
                raise KeyboardInterrupt("Clippy probe cancelled after child binding")
            with selectors.DefaultSelector() as selector:
                for stream in output:
                    selector.register(stream, selectors.EVENT_READ)
                while selector.get_map():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise subprocess.TimeoutExpired(command, timeout)
                    for key, _ in selector.select(remaining):
                        chunk = os.read(key.fd, min(4096, output_limit - observed + 1))
                        observed += len(chunk)
                        if observed > output_limit:
                            raise Denied("Cargo setup output exceeded its observation bound")
                        if chunk:
                            output[key.fileobj].extend(chunk)
                        else:
                            selector.unregister(key.fileobj)
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(command, timeout)
                process.wait(timeout=remaining)
            stdout, stderr = bytes(output[process.stdout]), bytes(output[process.stderr])
        finally:
            process.stdout.close()
            process.stderr.close()
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
        result.check_returncode()
        return result

    def child_ids(self):
        if self.child_discovery == 'proc-parent-pidfd':
            return proc_parent_child_ids()
        try:
            text = (Path('/proc/self/task') / str(os.getpid()) / 'children').read_text()
        except FileNotFoundError:
            return []  # unavailable: never substitute a whole-host scan.
        if len(text) > 65536:
            raise Denied("owned child table exceeds observation bound")
        return [int(pid) for pid in text.split()]

    def reap_adopted(self):
        """Drain exited adopted children while a Popen leader is still running.

        Nested tests cannot wait for an orphan adopted by this subreaper. Leaving
        that zombie until Cargo exits makes their synchronous cleanup impossible.
        WNOWAIT locates an exited, kernel-owned child without consuming Popen's
        leader status. Its unreaped PID remains reserved through the exact wait.
        No process-table scan, live-child signal, or whole-tree closure is implied.
        Call with cancellation blocked, in the same single-threaded wait owner.
        """
        if len(list(Path('/proc/self/task').iterdir())) != 1:
            raise Denied("Clippy tree acquired a competing native thread")
        owned = ctypes.c_int()
        if (self.libc.prctl(37, ctypes.byref(owned), 0, 0, 0) != 0 or owned.value != 1
                or signal.getsignal(signal.SIGCHLD) != signal.SIG_DFL):
            raise Denied("Clippy tree lost subreaper/reaping ownership")
        # Bound each service turn so a stream of exited descendants cannot
        # starve leader polling or pending cancellation. Final ECHILD still
        # belongs exclusively to finish(), after Popen has returned/unwound.
        for _ in range(128):
            try:
                event = os.waitid(os.P_ALL, 0, os.WEXITED | os.WNOHANG | os.WNOWAIT | self.WALL)
            except ChildProcessError:
                return
            if event is None:
                return
            handle = self.handles.get(event.si_pid)
            if handle is not None and handle[1] is not None and handle[1].returncode is None:
                return  # Popen exclusively owns its leader's wait and status.
            pid, status = os.waitpid(event.si_pid, os.WNOHANG | self.WALL)
            if pid != event.si_pid:
                raise Denied("adopted-child wait did not consume the observed exited child")
            self.reaped.append({"pid": pid, "exit_code": os.waitstatus_to_exitcode(status)})
            if handle is not None:
                self.handles.pop(pid)
                os.close(handle[0])

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
                "child_discovery": self.child_discovery,
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
            # reserves its PID until the next masked wait.
            # The Popen wait holds the mask for at most 0.01 seconds; the
            # adopted-child drain adds at most 128 nonblocking reaps per turn.
            # Service them while nested consumers can still require actual
            # reaping before their own return.
            mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
            try:
                if tree is not None:
                    tree.reap_adopted()
                try:
                    return process.wait(timeout=0.01)
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
    ceiling = None
    if budget is not None:
        report = dict(budget)
        policy = report["policy"]
        ceiling = report.get("max_used_pct")
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
        used_pct = (usage.total - usage.free) * 100 / usage.total
        if usage.free < required:
            row["shortfall_bytes"] = required - usage.free
            row["used_pct"] = used_pct
            raise CapacityRefused("insufficient disk headroom at " + str(path)
                                  + " free_bytes=" + str(usage.free)
                                  + " required_free_bytes=" + str(required)
                                  + " shortfall_bytes=" + str(row["shortfall_bytes"])
                                  + " used_pct=" + str(row["used_pct"]), row)
        if policy == "percent" and ceiling is not None and used_pct >= ceiling:
            required_free_for_pct = usage.total * (100 - ceiling) / 100
            # Admission requires used_pct strictly below the ceiling, so the
            # smallest passing free target is one byte past the threshold.
            additional_free_needed = math.floor(required_free_for_pct) + 1 - usage.free
            row["used_pct"] = used_pct
            row["max_used_pct"] = ceiling
            row["required_free_bytes_for_pct"] = required_free_for_pct
            row["pct_shortfall_bytes"] = additional_free_needed
            row["additional_free_bytes_needed"] = additional_free_needed
            raise CapacityRefused("volume used percentage at or above MAX_USED_PCT at " + str(path)
                                  + " free_bytes=" + str(usage.free)
                                  + " used_pct=" + str(used_pct)
                                  + " max_used_pct=" + str(ceiling)
                                  + " required_free_bytes_for_pct=" + str(required_free_for_pct)
                                  + " additional_free_bytes_needed=" + str(additional_free_needed), row)
        observations.append(row)
    report["destinations"] = observations
    return report


def read_only_git_environment(env):
    """Disable Git observation side effects without changing repository selection."""
    result = {key: value for key, value in env.items()
              if not key.startswith(("GIT_TRACE", "GIT_REDIRECT_"))}
    result.update(GIT_OPTIONAL_LOCKS="0", GIT_TRACE2="0",
                  GIT_TRACE2_EVENT="0", GIT_TRACE2_PERF="0")
    return result


def git_path(*args, env=None, output_reader=None):
    # Read bytes: text mode also translates CR/LF that may belong to a POSIX
    # filename. Git emits one terminal LF; preserve every preceding path byte.
    command = ["git", "rev-parse", *args]
    output = (subprocess.check_output(command, env=env) if output_reader is None
              else output_reader(command, env))
    if not output.endswith(b"\n"):
        raise Denied("Git path output is missing its terminal newline")
    return native_path(os.fsdecode(output[:-1]))


def private_worktree_root(slot, worktree):
    """Resolve private storage from an already canonical native worktree path."""
    return slot / "worktrees" / hashlib.sha256(os.fsencode(worktree)).hexdigest()


def resource_plan(env, *, git_output_reader=None):
    # Cargo uses the invocation directory, not Git's repository-location
    # overrides. Refuse an ambiguous subject instead of silently retargeting it.
    for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"):
        if name in env:
            raise Denied(name + " repository-location override is unsupported; unset it and invoke from the intended worktree")
    git_options = {"env": env}
    if git_output_reader is not None:
        git_options["output_reader"] = git_output_reader
    worktree = git_path("--show-toplevel", **git_options)
    common = git_path("--path-format=absolute", "--git-common-dir", **git_options)
    identity = str(common).casefold() if os.name == "nt" else str(common)
    key = hashlib.sha256(os.fsencode(identity)).hexdigest()[:16]
    base = native_path(env.get("DEVPLANE", str(Path.home() / ".cache" / "devplane")))
    # Keep one admission/lease domain per common repository and OS/host.
    # Cargo fingerprints deliberately omit the workspace absolute path, so
    # serializing divergent worktrees does not make their build state compatible.
    slot = base / (socket.gethostname() + "-" + sys.platform + "-" + key)
    # native_path already resolves the actual root; do not invent Unicode/case
    # equivalence between distinct filesystem paths. fsencode is lossless on POSIX.
    private = private_worktree_root(slot, worktree)
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


def child_environment_config(prefix, values, env, tree):
    """Ask Cargo to merge only our fixed env entries, without loading/building a workspace.

    Cargo accepts string or table [env] values, but cannot merge between those
    shapes. A scalar never forces over the admitted process environment. Still
    project its admitted value: a later type change must fail, not bypass policy.
    """
    deadline = time.monotonic() + 15
    remaining_output = 65536
    attempts, maximum = 0, 2 * len(values) + 3
    last_error = None

    def render(names, scalar=False):
        result = []
        for name in names:
            key, value = "env." + name, json.dumps(values[name])
            fields = [(key, value)] if scalar else [
                (key + ".value", value), (key + ".force", "true"), (key + ".relative", "false")]
            for key, value in fields:
                result.extend(("--config", key + "=" + value))
        return result

    def refuse(reason):
        diagnostic = (last_error.stderr or b"").decode("utf-8", "replace").strip() if last_error else ""
        raise CargoConfigRefused("Cargo child environment setup: " + reason +
                                 ("\n" + diagnostic[-4096:] if diagnostic else ""))

    def accepts(projection):
        nonlocal attempts, remaining_output, last_error
        remaining = deadline - time.monotonic()
        if remaining <= 0 or attempts >= maximum or remaining_output <= 0:
            refuse("configuration observation bound exhausted")
        attempts += 1
        command = [*prefix, *projection, "locate-project", "--message-format=json"]
        try:
            if tree is not None:
                result = tree.probe(command, env, timeout=remaining,
                                    output_limit=min(16384, remaining_output))
            else:
                # Existing non-Linux routes retain their legacy settlement
                # limitation. Discard output rather than capture without a bound.
                result = subprocess.run(command, env=env, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.DEVNULL, timeout=remaining, check=True)
        except subprocess.CalledProcessError as error:
            result = last_error = error
            if error.returncode != 101:
                refuse("configuration probe exited " + str(error.returncode))
        except (Denied, OSError, subprocess.TimeoutExpired) as error:
            raise CargoConfigRefused("Cargo child environment setup: " + str(error)) from error
        remaining_output -= len(result.stdout or b"") + len(result.stderr or b"")
        if remaining_output < 0 or time.monotonic() > deadline:
            refuse("configuration observation bound exhausted")
        return result.returncode == 0

    projection = render(values)
    if accepts(projection):
        return projection
    projection = render(values, scalar=True)
    if accepts(projection):
        return projection
    projection = []
    for name in values:
        entry = render([name])
        if not accepts(entry):
            entry = render([name], scalar=True)
            if not accepts(entry):
                refuse("neither supported configuration shape for " + name)
        projection.extend(entry)
    if not accepts(projection):
        refuse("combined configuration projection rejected")
    return projection


class PrivateAllocation:
    """Creation evidence for two new roots, never consumer or cleanup authority.

    Native Linux only. Existing unrecorded state remains usable but unknown. The
    record is immutable and self-bound to its original file identity; copied
    bytes cannot establish the creation evidence of a replacement object.
    """
    LIMIT = 16 * 1024
    SCHEMA = "cargo_private_allocation.v1"

    def __init__(self, worktree, slot, paths, env):
        self.worktree, self.slot, self.paths, self.env = worktree, slot, paths, env
        self.private = private_worktree_root(slot, worktree)
        self.path = self.private / "allocation.json"
        self.raw = None
        self.source = None
        self.receipt = {"status": "unknown", "reason": "platform_not_qualified",
                        "consumer_custody": "unknown", "reclaim_eligibility": "unknown"}
        self.supported = sys.platform == "linux"
        if not self.supported:
            return
        if paths["target"] != self.private / "target" or paths["build"] != self.private / "build":
            raise Denied("private allocation paths differ from the admitted worktree member")
        if os.path.lexists(self.path):
            self.raw, record = self._read()
            self._receipt(record)
        else:
            absent = all(not os.path.lexists(paths[key]) for key in ("target", "build"))
            self.receipt["reason"] = "private_roots_not_allocated" if absent else "preexisting_unrecorded_state"
            if absent:
                self.source = self._source()

    @staticmethod
    def _directory(path):
        path = native_path(str(path))
        identity = directory_identity(path)
        if identity[0] < 0 or identity[1] <= 0:
            raise Denied("private allocation directory identity is unavailable")
        return {"path": str(path), "identity": list(identity)}

    def _source(self):
        outputs = {}

        def read(command, env):
            key = tuple(command)
            if key not in outputs:
                outputs[key] = subprocess.check_output([command[0], "-C", str(self.worktree), *command[1:]],
                                                      env=env, timeout=5)
            return outputs[key]

        env = read_only_git_environment(self.env)
        if resource_plan(env, git_output_reader=read) != (self.worktree, self.slot, self.paths):
            raise Denied("private allocation resource plan changed during admission")
        options = {"env": env, "output_reader": read}
        common = git_path("--path-format=absolute", "--git-common-dir", **options)
        administrative = git_path("--absolute-git-dir", **options)
        return {"hostname": socket.gethostname(), "platform": sys.platform,
                "worktree": self._directory(self.worktree), "common_dir": self._directory(common),
                "git_dir": self._directory(administrative)}

    def _subject(self):
        return {**self._source(), "private_member": self._directory(self.private),
                "target": self._directory(self.paths["target"]),
                "build": self._directory(self.paths["build"])}

    @staticmethod
    def _file_facts(metadata):
        return (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns,
                metadata.st_ctime_ns, metadata.st_mode, metadata.st_nlink)

    def _read(self):
        native_path(str(self.path))
        fd = os.open(self.path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
        try:
            before = os.fstat(fd)
            if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_ino <= 0
                    or before.st_size > self.LIMIT):
                raise Denied("private allocation record must be a bounded singly linked regular file")
            raw = bytearray()
            while len(raw) <= self.LIMIT:
                chunk = os.read(fd, min(4096, self.LIMIT + 1 - len(raw)))
                if not chunk:
                    break
                raw.extend(chunk)
            if (len(raw) != before.st_size or len(raw) > self.LIMIT
                    or self._file_facts(before) != self._file_facts(os.fstat(fd))
                    or self._file_facts(before) != self._file_facts(self.path.lstat())):
                raise Denied("private allocation record changed during observation")
        finally:
            os.close(fd)

        def pairs(entries):
            result = {}
            for key, value in entries:
                if key in result:
                    raise Denied("duplicate private allocation record field")
                result[key] = value
            return result

        try:
            record = json.loads(bytes(raw), object_pairs_hook=pairs)
        except (ValueError, UnicodeError, RecursionError) as error:
            raise Denied("invalid private allocation record") from error
        fields = {"schema_version", "allocation_id", "creator_source_sha256", "record_identity", "subject"}
        if (not isinstance(record, dict) or set(record) != fields
                or record["schema_version"] != self.SCHEMA
                or not isinstance(record["allocation_id"], str)
                or not re.fullmatch(r"[0-9a-f]{32}", record["allocation_id"])
                or not isinstance(record["creator_source_sha256"], str)
                or not re.fullmatch(r"[0-9a-f]{64}", record["creator_source_sha256"])):
            raise Denied("private allocation record schema is invalid")
        # JSON equality preserves true-versus-1 and unknown fields as refusals.
        expected_identity = [before.st_dev, before.st_ino]
        if (json.dumps(record["record_identity"]) != json.dumps(expected_identity)
                or json.dumps(record["subject"], sort_keys=True) != json.dumps(self._subject(), sort_keys=True)):
            raise Denied("private allocation record no longer matches original filesystem identities")
        return bytes(raw), record

    def _receipt(self, record):
        self.receipt = {"status": "matching_creation_record", "record_path": str(self.path),
                        "allocation_id": record["allocation_id"],
                        "record_sha256": hashlib.sha256(self.raw).hexdigest(),
                        "consumer_custody": "unknown", "reclaim_eligibility": "unknown"}

    def revalidate(self):
        if not self.supported:
            return
        if self.raw is None:
            if os.path.lexists(self.path):
                raise Denied("private allocation record appeared during admission")
            if self.source is not None and self._source() != self.source:
                raise Denied("private allocation source identity changed during admission")
        elif self._read()[0] != self.raw:
            raise Denied("private allocation record changed during admission")

    def record_created(self, created, lock, identity, marker, marker_identity):
        if not self.supported or self.raw is not None:
            return
        self.revalidate()
        if self.source is None or not all(created.get(key) for key in ("target", "build")):
            self.receipt["reason"] = "preexisting_unrecorded_state"
            return
        if not owns_lease(lock, identity, marker, marker_identity):
            raise Denied("lease ownership changed before private allocation recording")
        subject = self._subject()
        if any(subject[key]["identity"] != list(created[key]) for key in ("target", "build")):
            raise Denied("new private allocation roots changed before recording")
        creator = file_subject(Path(__file__))["sha256"]
        parent = os.open(self.private, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            opened = os.fstat(parent)
            if [opened.st_dev, opened.st_ino] != subject["private_member"]["identity"]:
                raise Denied("private allocation container changed before recording")
            fd = os.open(self.path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                         0o600, dir_fd=parent)
            try:
                metadata = os.fstat(fd)
                record = {"schema_version": self.SCHEMA, "allocation_id": uuid.uuid4().hex,
                          "creator_source_sha256": creator,
                          "record_identity": [metadata.st_dev, metadata.st_ino], "subject": subject}
                raw = (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
                if len(raw) > self.LIMIT:
                    raise Denied("private allocation record exceeds its byte bound")
                written = 0
                while written < len(raw):
                    count = os.write(fd, raw[written:])
                    if count <= 0:
                        raise OSError("private allocation record write made no progress")
                    written += count
                os.fsync(fd)
            finally:
                os.close(fd)
            os.fsync(parent)
        finally:
            os.close(parent)
        observed, record = self._read()
        if observed != raw:
            raise Denied("private allocation record readback differs from written bytes")
        self.raw = observed
        self._receipt(record)


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
        discovery_mode = env.get('CARGO_ADMITTED_CHILD_DISCOVERY', 'proc-children')
        if discovery_mode != 'proc-children' and not owned_linux:
            raise Denied('alternate owned-child discovery requires native Linux')
        if managed and owned_linux:
            child_discovery_capability(discovery_mode)  # before scope/lease/allocation
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
        if preflight and os.path.lexists(lock):
            raise Denied("slot is active or awaits owner verification: " + str(lock))
        allocation = PrivateAllocation(worktree, slot, paths, env)
        if preflight:
            print("cargo-admitted preflight: " + json.dumps({
                "verdict": "PASS", "scope": scope, "admission": admission,
                "lease": str(lock), "lease_acquired": False,
                "private_allocation_provenance": allocation.receipt,
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
            allocation.revalidate()
            created = {}
            for name, path in paths.items():
                try:
                    path.mkdir(parents=True)
                    created[name] = directory_identity(path)
                except FileExistsError:
                    directory_identity(path)
            allocation.record_created(created, lock, identity, marker, marker_identity)
            env.update(CARGO_TARGET_DIR=str(paths["target"]),
                       CARGO_BUILD_BUILD_DIR=str(paths["build"]),
                       CARGO_HOME=str(paths["cargo_home"]), CARGO_INCREMENTAL="0",
                       CARGO_BUILD_JOBS=str(jobs), TMPDIR=str(paths["temp"]),
                       TEMP=str(paths["temp"]), TMP=str(paths["temp"]))
            descriptor = {"worktree": str(worktree), "scope": scope,
                          "resources": {k: str(v) for k, v in paths.items()},
                          "private_allocation_provenance": allocation.receipt,
                          "lease": str(lock), "pid": os.getpid(), "admission": admission, "disposition": "retained with reason: reusable bounded slot"}
            if owned_linux:
                descriptor['child_discovery'] = discovery_mode
            if managed:
                # Persist the original owner observation before any setup/product
                # child starts. Recovery must not adopt a replacement directory
                # or reconstruct ownership from age/current contents after death.
                descriptor.update(lease_identity=list(identity), lease_marker=str(marker),
                                  marker_identity=list(marker_identity))
            print("cargo-admitted resources: " + json.dumps(descriptor), file=sys.stderr, flush=True)
            local_config = local_config_cli_args(worktree)
            prefix = ["cargo", *local_config,
                      "--config", "unstable.unstable-options=false",
                      "--config", "build.build-dir=" + json.dumps(str(paths["build"])),
                      "--config", 'build.rustc-wrapper=""',
                      "--config", 'build.rustc-workspace-wrapper=""']
            # Cargo's [env] force=true can override process environment for
            # rustc/build scripts/tests. Nested Cargo must inherit the admitted
            # paths, jobs and incremental policy too. These entries follow the
            # local file; CARGO_HOME is forbidden in Cargo's [env] table.
            child_values = {name: env[name] for name in (
                "TEMP", "TMP", "TMPDIR", "RUSTUP_AUTO_INSTALL", "CARGO_TARGET_DIR",
                "CARGO_BUILD_BUILD_DIR", "CARGO_INCREMENTAL", "CARGO_BUILD_JOBS")}
            if clippy:
                exact = toolchain["subjects"]
                controlled = {"CARGO": exact["cargo"]["path"],
                              "RUSTC": exact["rustc"]["path"], "RUSTDOC": exact["rustdoc"]["path"],
                              "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": exact["clippy-driver"]["path"],
                              "SYSROOT": toolchain["root"],
                              "CARGO_ENCODED_RUSTFLAGS": "", "RUSTFLAGS": "",
                              "CLIPPY_ARGS": CLIPPY_LINT_ARGS, "CLIPPY_CONF_DIR": str(worktree)}
                env.update(controlled)
                # Override forced Cargo [env] as well, before nested Cargo sees
                # compiler/lint inputs. Real lint fail/clean controls test this.
                child_values.update(controlled)
                prefix[0] = exact["cargo"]["path"]
                if clippy_configuration(worktree, paths) != scope["clippy_setup"]["configuration"]:
                    raise Denied("Clippy configuration changed after admission")
                revalidate_clippy_toolchain(toolchain)
                # Version probes are setup, not package work. Hold the same
                # conservative lease before invoking these installed tools.
                tree = ClippyTree(child_discovery=discovery_mode)
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
                tree = ClippyTree(child_discovery=discovery_mode)
                print("cargo-admitted Cargo tree: " + json.dumps({"owner_pid": os.getpid(),
                      "subreaper": True, "prior_children": "none", "single_native_thread": True,
                      "lease": str(lock)}), file=sys.stderr, flush=True)
            launch_attempted = True
            projection = child_environment_config(prefix, child_values, env, tree)
            command = [*prefix, *projection, *args[:1], "--target-dir", str(paths["target"]), *args[1:]]
            if clippy:
                if clippy_configuration(worktree, paths) != scope["clippy_setup"]["configuration"]:
                    raise Denied("Clippy configuration changed during setup")
                position = command.index("clippy")
                command = [exact["cargo-clippy"]["path"], "clippy", "--message-format=json-render-diagnostics",
                           *command[1:position], "--target", toolchain["host"], *command[position + 1:]]
            for path in (slot, *paths.values()):
                if native_path(str(path)) != path:
                    raise Denied("resource path changed before launch: " + str(path))
            if not owns_lease(lock, identity, marker, marker_identity):
                raise Denied("lease ownership changed before Cargo launch")
            allocation.revalidate()
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
        if isinstance(error, CapacityRefused) and not preflight:
            print("cargo-admitted: next: run scripts/cargo-admitted --preflight with the "
                  "same admission options and exact Cargo arguments to inspect request scope "
                  "and private output roots (read-only; no capacity reservation).",
                  file=sys.stderr, flush=True)
        if preflight:
            print("cargo-admitted preflight: " + json.dumps({
                "verdict": "DENY", "scope": scope, "lease_acquired": False,
                "reason": str(error), "observation": getattr(error, "details", None),
            }), file=sys.stderr, flush=True)
        return 75


if __name__ == "__main__":
    sys.exit(main())
