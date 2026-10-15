"""Opt-in native Windows proof for the badge producer's private Job Object.

Run with PERL_LSP_RUN_NATIVE_JOB_TESTS=1 and PERL_LSP_NATIVE_JOB_ROOT set to
an existing task-owned temporary directory. Uses only owned Python processes,
a 30-second child lifetime cap, and private target directories; no Cargo.
"""
import ctypes
from ctypes import wintypes
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

SCRIPT = Path(__file__).parents[1] / "generate-badges.py"
SPEC = importlib.util.spec_from_file_location("native_badge_subject", SCRIPT)
generator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generator)
BASE_WINDOWS_JOB = generator.WindowsJob

# The child receives its root only from the task-owned parent, acquires a
# non-delete-sharing handle before publishing readiness, and self-exits.
FIXTURE = r'''
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import sys
import time
root = Path(sys.argv[2]).resolve()
if sys.argv[1] == "child":
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                  ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL
    handle = kernel.CreateFileW(str(root / "target" / "held.bin"), 0x80000000,
                                3, None, 3, 0x80, None)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        receipt = root / "ready.tmp"
        receipt.write_text(json.dumps({"pid": os.getpid(), "parent": os.getppid()}), encoding="utf-8")
        receipt.replace(root / "ready.json")
        time.sleep(30)
    finally:
        kernel.CloseHandle(handle)
elif sys.argv[1] == "spawner":
    expires = time.monotonic() + 30
    (root / "spawner.json").write_text(json.dumps({"pid": os.getpid()}), encoding="utf-8")
    deadline = time.monotonic() + 10
    while not (root / "spawn-late").exists():
        if time.monotonic() >= deadline:
            raise RuntimeError("late-birth release timed out")
        time.sleep(0.01)
    child = subprocess.Popen([sys.executable, __file__, "child", str(root)],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, close_fds=True)
    time.sleep(max(0, expires - time.monotonic()))
else:
    child = subprocess.Popen([sys.executable, __file__, "child", str(root)],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, close_fds=True)
    deadline = time.monotonic() + 10
    while not (root / "ready.json").exists():
        if child.poll() is not None:
            raise RuntimeError("fixture child exited before readiness")
        if time.monotonic() >= deadline:
            raise RuntimeError("fixture child readiness timed out")
        time.sleep(0.01)
    print("owned native producer output", flush=True)
'''


@unittest.skipUnless(os.name == "nt" and os.environ.get("PERL_LSP_RUN_NATIVE_JOB_TESTS") == "1",
                     "requires explicitly admitted native Windows owned-process proof")
class NativeWindowsBadgeJobProof(unittest.TestCase):
    def setUp(self):
        parent = Path(os.environ["PERL_LSP_NATIVE_JOB_ROOT"]).resolve(strict=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="badge-job-", dir=parent)
        self.root = Path(self.temporary.name).resolve()
        self.assertTrue(self.root.is_relative_to(parent))
        self.target = self.root / "target"
        self.target.mkdir()
        (self.target / "held.bin").write_bytes(b"private native settlement fixture")
        self.fixture = self.root / "fixture.py"
        self.fixture.write_text(FIXTURE, encoding="utf-8")
        self.child_handle = None
        self.job = None
        self.leader = None
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        self.kernel.WaitForSingleObject.restype = wintypes.DWORD
        self.kernel.IsProcessInJob.argtypes = [wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL)]
        self.kernel.IsProcessInJob.restype = wintypes.BOOL
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.CloseHandle.restype = wintypes.BOOL
        self.addCleanup(self.cleanup_owned_fixture)

    def cleanup_owned_fixture(self):
        failures = []
        try:
            if self.job is not None and self.job._handle is not None:
                try:
                    failures.extend(BASE_WINDOWS_JOB.terminate(self.job))
                except BaseException as error:
                    failures.append(f"job cleanup raised: {error}")
                    failures.extend(self.job.close())
            if self.leader is not None:
                try:
                    # Only the Popen handle created by this test may be signalled.
                    if self.leader.poll() is None:
                        self.leader.kill()
                    self.leader.wait(timeout=5)
                except (OSError, subprocess.TimeoutExpired) as error:
                    failures.append(f"leader cleanup failed: {error}")
                finally:
                    for stream in (self.leader.stdin, self.leader.stdout, self.leader.stderr):
                        if stream is not None:
                            stream.close()
            if self.child_handle is not None:
                if self.kernel.WaitForSingleObject(self.child_handle, 5000) != 0:
                    failures.append("owned descendant did not exit")
        finally:
            if self.child_handle is not None:
                self.kernel.CloseHandle(self.child_handle)
            self.temporary.cleanup()
        self.assertEqual(failures, [], "owned fixture cleanup incomplete")

    def observe_live_descendant_after_leader_exit(self):
        receipt = json.loads((self.root / "ready.json").read_text(encoding="utf-8"))
        if self.child_handle is None:
            self.child_handle = self.kernel.OpenProcess(0x00100000 | 0x1000, False, receipt["pid"])
        self.assertTrue(self.child_handle, "could not bind owned fixture child handle")
        member = wintypes.BOOL()
        self.assertTrue(self.kernel.IsProcessInJob(self.child_handle, self.job._handle, ctypes.byref(member)))
        self.assertTrue(member.value, "fixture child escaped the assigned job")
        self.assertEqual(self.leader.poll(), 0, "leader has not exited successfully")
        self.assertEqual(self.kernel.WaitForSingleObject(self.child_handle, 0), 258,
                         "descendant must still be live after leader exit")
        self.assertEqual(self.job.active_processes(), 1)
        with self.assertRaises(PermissionError) as blocked:
            self.remove_private_target()
        self.assertEqual(blocked.exception.winerror, 32, "expected the child's sharing violation")
        self.assertTrue((self.target / "held.bin").exists())
        print("NATIVE_PRE: leader_exit=0 child_live=true job_active=1 target_release=sharing_violation_32", flush=True)

    def remove_private_target(self):
        # Never apply recursive removal to a computed path outside this fixture.
        self.assertEqual(self.target.resolve().parent, self.root)
        self.assertEqual(self.target.name, "target")
        shutil.rmtree(self.target)

    def assert_settled_and_release_target(self):
        self.assertEqual(self.kernel.WaitForSingleObject(self.child_handle, 0), 0,
                         "descendant still active after reported settlement")
        self.remove_private_target()
        self.assertFalse(self.target.exists())
        print("NATIVE_POST: child_exited=true target_release=removed", flush=True)

    def launch_owned_tree(self):
        self.job = generator.WindowsJob()
        self.leader = subprocess.Popen(
            [sys.executable, str(SCRIPT.resolve()), generator.WINDOWS_JOB_LAUNCHER_FLAG,
             sys.executable, str(self.fixture), "leader", str(self.root)],
            cwd=self.root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP,
        )
        self.job.assign(self.leader)
        self.leader.stdin.write(b"\0")
        self.leader.stdin.close()
        self.assertEqual(self.leader.wait(timeout=15), 0)
        self.assertEqual(self.leader.stdout.read(), b"owned native producer output\r\n")
        self.assertEqual(self.leader.stderr.read(), b"")
        self.observe_live_descendant_after_leader_exit()

    def test_native_tree_settlement_releases_retained_target(self):
        self.launch_owned_tree()
        self.assertEqual(self.job.terminate(), [])
        self.assert_settled_and_release_target()

    def test_leader_only_success_mutant_is_rejected(self):
        self.launch_owned_tree()
        # The realistic wrong owner reports success after waiting for the leader.
        leader_only_success = []
        self.assertEqual(leader_only_success, [])
        with self.assertRaisesRegex(AssertionError, "descendant still active"):
            self.assert_settled_and_release_target()
        print("NATIVE_NEGATIVE: leader_only_success=rejected_by_native_process_handle", flush=True)
        self.assertEqual(self.job.active_processes(), 1)
        self.assertEqual(self.job.terminate(), [])
        self.assert_settled_and_release_target()

    def test_descendant_born_after_snapshot_cannot_authorize_success(self):
        owner = self
        totals = []
        class LateBirthJob(BASE_WINDOWS_JOB):
            triggered = False

            def total_processes(self):
                total = super().total_processes()
                totals.append(total)
                return total

            def process_handles(self):
                handles = super().process_handles()
                if self.triggered:
                    return handles
                self.triggered = True
                try:
                    (owner.root / "spawn-late").touch()
                    deadline = time.monotonic() + 10
                    while not (owner.root / "ready.json").exists():
                        if time.monotonic() >= deadline:
                            raise AssertionError("late child did not publish its lock receipt")
                        time.sleep(0.01)
                    receipt = json.loads((owner.root / "ready.json").read_text(encoding="utf-8"))
                    owner.child_handle = owner.kernel.OpenProcess(0x00100000 | 0x1000, False, receipt["pid"])
                    owner.assertTrue(owner.child_handle)
                    member = wintypes.BOOL()
                    owner.assertTrue(owner.kernel.IsProcessInJob(owner.child_handle, self._handle, ctypes.byref(member)))
                    owner.assertTrue(member.value)
                    owner.assertEqual(owner.kernel.WaitForSingleObject(owner.child_handle, 0), 258)
                    with owner.assertRaises(PermissionError) as blocked:
                        owner.remove_private_target()
                    owner.assertEqual(blocked.exception.winerror, 32)
                    print(f"NATIVE_LATE_PRE: bound_handles={len(handles)} late_child_live=true target_release=sharing_violation_32", flush=True)
                    return handles
                except BaseException:
                    for handle in handles:
                        self._kernel32.CloseHandle(handle)
                    raise

        self.job = LateBirthJob()
        self.leader = subprocess.Popen(
            [sys.executable, str(SCRIPT.resolve()), generator.WINDOWS_JOB_LAUNCHER_FLAG,
             sys.executable, str(self.fixture), "spawner", str(self.root)],
            cwd=self.root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP,
        )
        self.job.assign(self.leader)
        self.leader.stdin.write(b"\0")
        self.leader.stdin.close()
        deadline = time.monotonic() + 10
        while not (self.root / "spawner.json").exists():
            if time.monotonic() >= deadline:
                self.fail("owned spawner did not start")
            time.sleep(0.01)
        failures = self.job.terminate()
        try:
            self.assertTrue(any("membership changed" in failure for failure in failures), failures)
            self.assertEqual(len(totals), 2)
            self.assertGreater(totals[1], totals[0])
            print(f"NATIVE_LATE_RESULT: lifetime_counts={totals} post_snapshot_birth=refused_success", flush=True)
        finally:
            self.assertEqual(self.kernel.WaitForSingleObject(self.child_handle, 5000), 0)
            self.remove_private_target()
            self.assertFalse(self.target.exists())
            print("NATIVE_LATE_POST: owned_child_exited=true private_target_removed=true", flush=True)

    def test_real_run_ripr_settles_after_successful_leader_and_pipe_eof(self):
        owner = self
        class ObservedJob(generator.WindowsJob):
            def assign(self, process):
                owner.job = self
                owner.leader = process
                return super().assign(process)

            def terminate(self):
                owner.observe_live_descendant_after_leader_exit()
                return super().terminate()

        wrapper = self.root / "owned-producer.cmd"
        wrapper.write_text(f'@"{sys.executable}" "{self.fixture}" leader "{self.root}"\n', encoding="utf-8")
        with mock.patch.object(generator, "WindowsJob", ObservedJob), mock.patch.dict(
            os.environ, {"RIPR_BIN": str(wrapper)}
        ):
            output = generator.run_ripr(self.root, timeout_seconds=15)
        self.assertEqual(output.strip(), "owned native producer output")
        self.assert_settled_and_release_target()


if __name__ == "__main__":
    unittest.main()
