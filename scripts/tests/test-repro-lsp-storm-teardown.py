#!/usr/bin/env python3
"""Self-test for the #18349 teardown-race tolerance in repro_lsp_storm.py.

Drives the real harness (`scripts/repro_lsp_storm.py`) against a stub LSP
server that reproduces the nightly fingerprint: it answers `initialize`,
consumes the whole workload, answers the harness's post-workload
`workspace/symbol` barrier request (the workload-completion evidence the
tolerance is gated on), and only then self-exits — closing stdio so the
harness's later `shutdown` write fails on a dead peer. With
`STUB_DIE_ON_FIRST_DIDCLOSE=1` the stub instead self-exits on the first
`textDocument/didClose`, which is a death MID-workload.

Covers three properties:

1. a server self-exit AFTER the barrier-confirmed workload completes is
   tolerated — the run succeeds, the RSS samples (including `settled`) reach
   the JSON artifact, and the race is noted in the `teardown_race` output
   field;
2. a server death MID-workload still fails the run with no JSON artifact
   (the barrier is never reached, so no tolerance applies);
3. the downstream `assert_rss_plateau.py` assertion is untouched: it still
   passes a flat series and still fails a regressing one.
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
HARNESS = REPO_ROOT / "scripts" / "repro_lsp_storm.py"
PLATEAU = REPO_ROOT / "scripts" / "assert_rss_plateau.py"

# Stub perllsp: LSP framing on stdio, one initialize response, then the
# workload is consumed silently, the post-workload workspace/symbol barrier
# is answered, and the process self-exits — the #18349 teardown shape
# (server delivery-failure policy ends the process after the workload, so
# the harness's shutdown write fails on the dead peer).
# STUB_DIE_ON_FIRST_DIDCLOSE=1 switches to a death on the first
# textDocument/didClose instead, which is MID-workload for n_files >= 2.
STUB_SOURCE = """#!/usr/bin/env python3
import json
import os
import sys

DIE_ON_FIRST_DIDCLOSE = os.environ.get("STUB_DIE_ON_FIRST_DIDCLOSE") == "1"


def read_frame(stream):
    headers = {}
    while True:
        line = stream.readline()
        if not line:
            return None
        if line in (b"\\r\\n", b"\\n"):
            break
        key, _, value = line.decode("ascii", errors="replace").partition(":")
        headers[key.strip().lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    return stream.read(length) if length > 0 else b""


def respond(message, result):
    payload = json.dumps(
        {"jsonrpc": "2.0", "id": message.get("id"), "result": result}
    ).encode("utf-8")
    sys.stdout.buffer.write(
        b"Content-Length: " + str(len(payload)).encode("ascii") + b"\\r\\n\\r\\n" + payload
    )
    sys.stdout.buffer.flush()


def main():
    while True:
        body = read_frame(sys.stdin.buffer)
        if body is None:
            return 0
        message = json.loads(body.decode("utf-8"))
        method = message.get("method")
        if method == "initialize":
            respond(message, {"capabilities": {}})
        elif method == "textDocument/didClose":
            if DIE_ON_FIRST_DIDCLOSE:
                # MID-workload death: the harness's next workload write hits
                # the dead peer before the barrier is ever reached.
                return 0
        elif method == "workspace/symbol":
            # The harness's post-workload barrier: answering it confirms the
            # whole workload stream was consumed; exiting here is the
            # #18349 post-workload self-exit the tolerance covers.
            respond(message, {"symbols": []})
            return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
"""


def _write_stub(scratch: Path) -> str:
    """Write the stub server and return a path Popen can execute directly."""
    stub_py = scratch / "lsp_stub.py"
    stub_py.write_text(STUB_SOURCE, encoding="utf-8")
    if sys.platform == "win32":
        # CreateProcess cannot exec a shebang script; wrap it in a .cmd that
        # calls this interpreter by absolute path (no PATH dependence).
        launcher = scratch / "lsp_stub.cmd"
        launcher.write_bytes(
            (
                "@echo off\r\n"
                f'"{sys.executable}" "{os.path.abspath(stub_py)}" %*\r\n'
            ).encode("utf-8")
        )
        return str(launcher)
    os.chmod(stub_py, os.stat(stub_py).st_mode | stat.S_IEXEC)
    return str(stub_py)


def _run_harness(
    binary: str,
    scratch: Path,
    n_files: int,
    *,
    stub_dies_on_first_didclose: bool = False,
) -> subprocess.CompletedProcess[bytes]:
    json_out = scratch / f"storm-{n_files}.json"
    csv_out = scratch / f"storm-{n_files}.csv"
    env = dict(os.environ)
    if stub_dies_on_first_didclose:
        env["STUB_DIE_ON_FIRST_DIDCLOSE"] = "1"
    return subprocess.run(
        [
            sys.executable,
            str(HARNESS),
            "--binary",
            binary,
            "--n-files",
            str(n_files),
            "--n-changes",
            "1",
            "--sample-every",
            "1",
            "--settle-seconds",
            "0.3",
            "--json-out",
            str(json_out),
            "--csv-out",
            str(csv_out),
        ],
        capture_output=True,
        timeout=120,
        cwd=str(REPO_ROOT),
        env=env,
    )


class ReproLspStormTeardownTest(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = Path(tempfile.mkdtemp(prefix="storm-teardown-selftest-"))
        self.binary = _write_stub(self.scratch)

    def tearDown(self) -> None:
        shutil.rmtree(self.scratch, ignore_errors=True)

    def test_post_workload_server_exit_is_tolerated_and_reaches_plateau_input(self) -> None:
        """#18349: tolerated teardown race keeps the run green with full samples."""
        run = _run_harness(self.binary, self.scratch, n_files=1)

        self.assertEqual(
            run.returncode,
            0,
            f"harness must succeed through the tolerated teardown race;\n"
            f"stdout={run.stdout.decode(errors='replace')}\n"
            f"stderr={run.stderr.decode(errors='replace')}",
        )
        stderr = run.stderr.decode(errors="replace")
        self.assertIn("teardown race tolerated", stderr)
        self.assertIn("workspace/symbol barrier", stderr)

        payload = json.loads((self.scratch / "storm-1.json").read_text(encoding="utf-8"))
        self.assertEqual(payload["schema"], 1)
        phases = [sample["phase"] for sample in payload["samples"]]
        # The samples already collected before the race must all survive, so
        # the plateau assertion sees the same input it would have had without
        # the race.
        self.assertEqual(phases, ["initialized", "churn", "settled"])
        self.assertEqual(payload["teardown_race"]["stage"], "shutdown")
        # The dead-peer race surfaces as a failed shutdown write
        # (BrokenPipeError, or OSError EINVAL on Windows) or as the server
        # closing stdout before answering shutdown (TimeoutError), depending
        # on whether the write beats the peer's exit into the pipe buffer.
        self.assertIn(
            payload["teardown_race"]["error"],
            {"BrokenPipeError", "OSError", "TimeoutError"},
        )
        # The race note must record the evidence the tolerance was gated on:
        # the barrier response, not just a successful pipe write.
        self.assertEqual(
            payload["teardown_race"]["workload_confirmed_by"],
            "workspace/symbol barrier response",
        )
        # The CSV artifact is written too (nightly also consumes it).
        csv_text = (self.scratch / "storm-1.csv").read_text(encoding="utf-8")
        self.assertIn("elapsed_s,phase,file_index,rss_kb", csv_text)
        self.assertIn("settled", csv_text)

    def test_mid_workload_server_death_still_fails_the_run(self) -> None:
        """The tolerance covers only the barrier-confirmed post-workload shutdown."""
        run = _run_harness(
            self.binary,
            self.scratch,
            n_files=2,
            stub_dies_on_first_didclose=True,
        )

        self.assertNotEqual(
            run.returncode,
            0,
            f"mid-workload server death must fail the run;\n"
            f"stdout={run.stdout.decode(errors='replace')}\n"
            f"stderr={run.stderr.decode(errors='replace')}",
        )
        stderr = run.stderr.decode(errors="replace")
        # The stub dies at file 0's didClose; the death must surface before
        # the post-workload barrier is ever reached — as a mid-workload pipe
        # failure (BrokenPipeError, OSError EINVAL on Windows) when the next
        # write hits the dead peer, or at the fatal barrier wait
        # (TimeoutError) if that write slipped into the pipe buffer. Either
        # way the run fails with no JSON artifact.
        self.assertTrue(
            "BrokenPipeError" in stderr or "OSError" in stderr or "TimeoutError" in stderr,
            f"expected the mid-workload pipe failure in the traceback, got:\n{stderr}",
        )
        self.assertFalse(
            (self.scratch / "storm-2.json").exists(),
            "a failed run must not publish a memory JSON artifact",
        )

    def test_plateau_assertion_still_bites_after_tolerance(self) -> None:
        """Tolerance must not weaken assert_rss_plateau: flat passes, growth fails."""

        def plateau(json_name: str) -> subprocess.CompletedProcess[bytes]:
            return subprocess.run(
                [sys.executable, str(PLATEAU), str(self.scratch / json_name)],
                capture_output=True,
                timeout=60,
                cwd=str(REPO_ROOT),
            )

        flat = self.scratch / "flat.json"
        flat.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "samples": [
                        {"phase": "churn", "file_index": i, "rss_kb": 10000} for i in range(1, 9)
                    ],
                }
            ),
            encoding="utf-8",
        )
        flat_run = plateau("flat.json")
        self.assertEqual(
            flat_run.returncode,
            0,
            f"flat series must pass the unchanged assertion: {flat_run.stdout.decode(errors='replace')}",
        )
        self.assertIn('"passed": true', flat_run.stdout.decode(errors="replace"))

        growing = self.scratch / "growing.json"
        growing.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "samples": [
                        # 2000 KB/file clears the material-growth floor
                        # (2048 KB) as well as the slope limit, so the
                        # regression cannot slip through either gate.
                        {"phase": "churn", "file_index": i, "rss_kb": 10000 + 2000 * i}
                        for i in range(1, 9)
                    ],
                }
            ),
            encoding="utf-8",
        )
        growing_run = plateau("growing.json")
        self.assertNotEqual(
            growing_run.returncode,
            0,
            f"regressing series must still fail the assertion: {growing_run.stdout.decode(errors='replace')}",
        )
        # SystemExit's failure message (which names the exceeded limit) is
        # printed to stderr; the stdout summary already shows passed=false.
        self.assertIn("slope exceeded limit", growing_run.stderr.decode(errors="replace"))


if __name__ == "__main__":
    unittest.main()
