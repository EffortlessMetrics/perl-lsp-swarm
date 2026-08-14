from __future__ import annotations

import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Generator

import sublime
from Debugger.modules import dap
from unittesting import DeferrableTestCase

# The pinned UnitTesting runner allows ~30s per Sublime launch for the whole
# deferred journey to finish and write its result file; keep every internal
# condition budget strictly below that window so a stuck journey fails with
# output instead of the runner timing out silently.
TIMEOUT_MS = 25_000


class TestLog:
    def __init__(self) -> None:
        self.messages: list[str] = []

    def __call__(self, kind: str, message: Any) -> None:
        self.messages.append(f"{kind}: {message}")

    def info(self, message: str) -> None:
        self.messages.append(f"info: {message}")


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _registry() -> dict[str, Any]:
    AdapterBase = getattr(dap, "AdapterConfiguration", None) or getattr(dap, "Adapter", None)
    registered = getattr(AdapterBase, "registered_types", None)
    if isinstance(registered, dict):
        return registered
    from Debugger.modules.dap.adapter import AdapterConfigurationRegistery

    registered = getattr(AdapterConfigurationRegistery, "registered_types", None)
    return registered if isinstance(registered, dict) else {}


def _complete_immediate(coroutine: Any) -> Any:
    try:
        coroutine.send(None)
    except StopIteration as complete:
        return complete.value
    raise AssertionError("adapter coroutine unexpectedly suspended")


class SublimePerlDapAdapterJourney(DeferrableTestCase):
    @classmethod
    def setUpClass(cls) -> Generator[Any, None, None]:
        super().setUpClass()
        cls.binary = Path(os.environ["PERLLSP_SUBLIME_DAP_BINARY"]).resolve()
        cls.fixture = Path(os.environ["PERLLSP_SUBLIME_DAP_FIXTURE"]).resolve()
        cls.runtime_receipt_path = Path(
            os.environ["PERLLSP_SUBLIME_DAP_RUNTIME_RECEIPT"]
        ).resolve()
        cls.host_receipt_path = Path(os.environ["PERLLSP_SUBLIME_DAP_HOST_RECEIPT"]).resolve()

        if not cls.binary.is_file():
            raise AssertionError(f"exact-source perl-dap binary missing: {cls.binary}")
        if not cls.fixture.is_file():
            raise AssertionError(f"DAP fixture missing: {cls.fixture}")
        if not cls.runtime_receipt_path.is_file():
            raise AssertionError(f"DAP runtime receipt missing: {cls.runtime_receipt_path}")

        settings = sublime.load_settings("LSP-perllsp.sublime-settings")
        settings.set("dap_path", str(cls.binary))

        cls.window = sublime.active_window()
        cls.window.set_project_data(
            {
                "folders": [{"path": str(cls.fixture.parent)}],
                "debugger_configurations": [
                    {
                        "name": "Malicious project override negative control",
                        "type": "perl",
                        "request": "launch",
                        "program": str(cls.fixture),
                        "cwd": str(cls.fixture.parent),
                        "dap_path": str(cls.fixture.parent / "wrong-perl-dap"),
                        "adapterPath": str(cls.fixture.parent / "wrong-adapter"),
                    }
                ],
            }
        )

        yield {"condition": lambda: "perl" in _registry(), "timeout": TIMEOUT_MS}

    def test_debugger_adapter_launch_and_bound_runtime_evidence(self) -> None:
        registry = _registry()
        self.assertIn("perl", registry)
        adapter = registry["perl"]
        self.assertTrue(type(adapter).__module__.endswith("debugger_adapter"))

        log = TestLog()
        configuration = {
            "request": "launch",
            "program": str(self.fixture),
            "cwd": str(self.fixture.parent),
        }
        transport = _complete_immediate(adapter.start(log, configuration))
        self.assertIsInstance(transport, dap.StdioTransport)
        self.assertEqual(transport.command, [str(self.binary), "--stdio"])
        self.assertEqual(transport.cwd, str(self.fixture.parent))

        snippets = adapter.configuration_snippets
        self.assertTrue(snippets)
        body = snippets[0]["body"]
        self.assertEqual(body["type"], "perl")
        self.assertEqual(body["request"], "launch")
        self.assertEqual(body["program"], r"\${file}")
        self.assertEqual(body["cwd"], r"\${workspaceFolder}")

        # Exercise Debugger's actual StdioTransport process launcher against the
        # trusted binary. Full protocol behavior is proven by the bound runtime
        # receipt in the same workflow; this host leg proves registration,
        # command identity, process launch, and cleanup inside Sublime.
        transport.log = log
        transport.configuration = type(
            "Configuration",
            (),
            {"variables": {"folder": str(self.fixture.parent)}},
        )()
        _complete_immediate(transport.setup())
        self.assertIsNotNone(transport.process)
        process = transport.process
        assert process is not None
        self.assertIsNone(process.process.poll(), "Debugger did not keep perl-dap running on stdio")
        launched_pid = process.pid
        launched_args = list(process.process.args)
        self.assertEqual(launched_args, [str(self.binary), "--stdio"])
        transport.dispose()
        self.assertIsNotNone(process.process.poll(), "Debugger left the perl-dap process alive")

        runtime = json.loads(self.runtime_receipt_path.read_text(encoding="utf-8"))
        self.assertEqual(runtime["kind"], "perl_dap_runtime")
        self.assertEqual(runtime["stage"], "exact_source_local")
        self.assertEqual(runtime["source_sha"], os.environ["PERLLSP_SOURCE_SHA"])
        self.assertEqual(runtime["binary"]["sha256"], _sha256(self.binary))
        for name, value in runtime["assertions"].items():
            self.assertIs(value, True, f"runtime assertion was not earned: {name}")

        project_data = self.window.project_data() or {}
        malicious = project_data["debugger_configurations"][0]
        self.assertNotEqual(launched_args[0], malicious["dap_path"])
        self.assertNotEqual(launched_args[0], malicious["adapterPath"])

        assertions = {
            "debugger_loaded": True,
            "adapter_registered": True,
            "trusted_binary_authority": True,
            "direct_stdio_transport": True,
            "exact_binary_launched": True,
            "adapter_process_cleanup": True,
            "launch_configuration": True,
            "runtime_breakpoint_verified_hit": runtime["assertions"]["breakpoint_verified_hit"],
            "runtime_stack_scopes_variables": runtime["assertions"]["stack_scopes_variables"],
            "runtime_step_over": runtime["assertions"]["step_over"],
            "runtime_continue_termination": runtime["assertions"]["continue_termination"],
            "runtime_restart": runtime["assertions"]["restart"],
            "runtime_process_cleanup": runtime["assertions"]["process_cleanup"],
        }
        receipt = {
            "schema_version": 1,
            "kind": "sublime_debugger_host",
            "stage": "exact_source_local",
            "source_sha": os.environ["PERLLSP_SOURCE_SHA"],
            "recorded_at": datetime.now(timezone.utc).isoformat(),
            "host": {
                "name": "Sublime Text",
                "version": sublime.version(),
                "platform": sublime.platform(),
                "arch": sublime.arch(),
            },
            "debugger": {
                "repository": "daveleroy/SublimeDebugger",
                "version": os.environ["PERLLSP_DEBUGGER_VERSION"],
                "ref": os.environ["PERLLSP_DEBUGGER_REF"],
            },
            "adapter": {
                "type": "perl",
                "module": type(adapter).__module__,
                "class": type(adapter).__name__,
                "transport": "stdio",
            },
            "binary": {
                "path": str(self.binary),
                "sha256": _sha256(self.binary),
                "version": runtime["binary"]["version"],
                "command": launched_args,
                "pid": launched_pid,
            },
            "fixture": {
                "path": str(self.fixture),
                "sha256": _sha256(self.fixture),
            },
            "runtime_receipt": {
                "kind": runtime["kind"],
                "path": str(self.runtime_receipt_path),
                "sha256": _sha256(self.runtime_receipt_path),
                "source_sha": runtime["source_sha"],
                "binary_sha256": runtime["binary"]["sha256"],
            },
            "assertions": assertions,
        }
        self.host_receipt_path.parent.mkdir(parents=True, exist_ok=True)
        self.host_receipt_path.write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    @classmethod
    def tearDownClass(cls) -> None:
        sublime.load_settings("LSP-perllsp.sublime-settings").erase("dap_path")
        super().tearDownClass()
