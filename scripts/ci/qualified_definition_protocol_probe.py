#!/usr/bin/env python3
"""Bounded three-file definition regression against an existing server binary."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import threading
import time
from urllib.parse import unquote

REPO = Path(__file__).resolve().parents[2]
CALLER = """package Scale00::Mod00;
use strict;
use warnings;
use lib '../lib';
use Scale24::Mod00 ();
sub compute_0 { return "caller\\n"; }
print Scale24::Mod00::compute_0();
print compute_0();
"""
TARGET_TEXT = """package Scale24::Mod00;
use strict;
use warnings;
sub compute_0 { return "target\\n"; }
1;
"""
DECOY = "package Decoy;\nsub compute_0 { return 'decoy'; }\n1;\n"
PACKAGE_BLOCK = "package Caller { Other::compute_0(); }\n"
QUALIFIED_ALIAS = "package Caller;\n*Other::compute_0 = sub { return 2; };\nprint Other::compute_0();\n"
CONSTANT_VALUE = "package Caller;\nuse constant VALUE => Other::compute_0();\nprint Caller::VALUE();\nprint VALUE();\n"
LABEL_CALL = "package Caller;\ngoto MARK;\nMARK: print Other::compute_0();\n"
ARITHMETIC_CALLS = """package Caller;
MULTIPLY: 2 * Other::compute_0();
ADJACENT_MULTIPLY: 2*Other::compute_0();
MODULO: 2 % Other::compute_0();
ADJACENT_MODULO: 2%Other::compute_0();
"""
ARITHMETIC_CASES = ("multiply", "adjacent_multiply", "modulo", "adjacent_modulo")
FORMAT_CALL = "package Caller;\nformat REPORT =\n@<<<<\nOther::compute_0()\n.\n"
QUALIFIED_FORMAT_CALL = FORMAT_CALL.replace("format REPORT", "format Other::REPORT")
MOO_CALL = "package Caller;\nuse Moo;\nhas 'value' => (is => 'ro', reader => undef, default => sub { Other::compute_0(); });\nour $kept = 7;\n$Caller::kept;\n$kept;\n"
SAME_NAME_PACKAGE = "package Other::compute_0 { Other::compute_0(); }\n"
SUPER_CASES = (
    ("missing", "SUPER::missing();"),
    ("same_name", "SUPER::helper();"),
    ("qualified_missing", "Caller::SUPER::missing();"),
    ("method_missing", "$self->SUPER::missing();"),
    ("qualified_method_missing", "$self->Caller::SUPER::missing();"),
)
SUPER_MISSING = ("package Caller;\nsub helper {\n    my $self = shift;\n"
                 + "".join(f"    {statement}\n" for _, statement in SUPER_CASES) + "}\n")
SUPER_PARENT = ("package Base;\nsub override { 'base' }\npackage Caller;\n"
                "our @ISA = ('Base');\nsub override { 'caller' }\n"
                "sub invoke { shift->SUPER::override() }\nprint Caller->invoke();\n")
REQUEST_SECONDS = 5
SESSION_SECONDS = 30


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def stock_environment() -> dict[str, str]:
    environment = os.environ.copy()
    for key in list(environment):
        upper = key.upper()
        if upper.startswith("PERL_LSP") or upper in {"LSP_TEST_FALLBACKS", "LC_ALL", "LC_CTYPE", "LANG"}:
            environment.pop(key)
    return environment


def at_declaration(locations: object, path: Path, line: int, end_line: int | None = None,
                   start_characters: tuple[int, ...] | None = None) -> bool:
    if not isinstance(locations, list) or len(locations) != 1:
        return False
    location = locations[0]
    return (
        isinstance(location, dict)
        and unquote(location.get("uri", "")) == path.as_uri()
        and location.get("range", {}).get("start", {}).get("line") == line
        and location.get("range", {}).get("end", {}).get("line") == (line if end_line is None else end_line)
        and (start_characters is None
             or location.get("range", {}).get("start", {}).get("character") in start_characters)
    )


def assertions(report: dict, caller: Path, target: Path) -> dict[str, bool]:
    observations = report["observations"]
    before = observations.get("qualified_before_target_open")
    return {
        "missing_index_never_returns_wrong_package": (
            "qualified_before_target_open" in observations
            and (before is None or before == [] or at_declaration(before, target, 3, start_characters=(0, 4)))
        ),
        "bare_local_declaration_retained": at_declaration(observations.get("bare_local_control"), caller, 5, start_characters=(0, 4)),
        "target_didOpen_recovers_exact_declaration": at_declaration(observations.get("qualified_after_target_open"), target, 3, start_characters=(0, 4)),
        "unresolved_call_never_returns_enclosing_package": (
            "qualified_inside_package_block" in observations
            and observations["qualified_inside_package_block"] in (None, [])
        ),
        "exact_qualified_alias_retained_after_edit": at_declaration(observations.get("qualified_alias_after_edit"), caller, 1, start_characters=(0,)),
        "constant_value_never_returns_containing_constant": (
            "qualified_inside_constant_value" in observations
            and observations["qualified_inside_constant_value"] in (None, [])
        ),
        "qualified_constant_navigation_retained": at_declaration(observations.get("qualified_constant_control"), caller, 1, start_characters=(0, 13)),
        "bare_constant_navigation_retained": at_declaration(observations.get("bare_constant_control"), caller, 1, start_characters=(0, 13)),
        "qualified_call_never_returns_containing_label": (
            "qualified_inside_labeled_statement" in observations
            and observations["qualified_inside_labeled_statement"] in (None, [])
        ),
        "goto_label_navigation_retained": at_declaration(observations.get("goto_label_control"), caller, 2, start_characters=(0,)),
        "label_declaration_navigation_retained": at_declaration(observations.get("label_declaration_control"), caller, 2, start_characters=(0,)),
        "format_value_never_returns_containing_format": (
            "qualified_inside_format_value" in observations
            and observations["qualified_inside_format_value"] in (None, [])
        ),
        # Name/header selection and full-declaration spans are both valid
        # format targets. Always require this file and the declaration line.
        "format_declaration_navigation_retained": any(
            at_declaration(observations.get("format_declaration_control"), caller, 1, end_line, (0, 7))
            for end_line in (1, 4, 5)
        ),
        "attribute_default_never_returns_containing_scalar": (
            "qualified_inside_attribute_default" in observations
            and observations["qualified_inside_attribute_default"] in (None, [])
        ),
        "qualified_variable_navigation_retained": at_declaration(observations.get("qualified_variable_control"), caller, 3, start_characters=(4,)),
        "bare_variable_navigation_retained": at_declaration(observations.get("bare_variable_control"), caller, 3, start_characters=(4,)),
        "same_name_package_cannot_stand_in_for_callable": (
            "qualified_call_same_name_package" in observations
            and observations["qualified_call_same_name_package"] in (None, [])
        ),
        "same_name_package_declaration_retained": at_declaration(observations.get("same_name_package_declaration_control"), caller, 0, start_characters=(0, 8)),
        **{f"{case}_call_never_returns_containing_label": (
            f"qualified_{case}_call" in observations
            and observations[f"qualified_{case}_call"] in (None, [])
        ) for case in ARITHMETIC_CASES},
        "qualified_format_value_never_returns_containing_format": (
            "qualified_inside_qualified_format_value" in observations
            and observations["qualified_inside_qualified_format_value"] in (None, [])
        ),
        "qualified_format_declaration_navigation_retained": any(
            at_declaration(observations.get("qualified_format_declaration_control"), caller, 1, end_line, (0, 7))
            for end_line in (1, 4, 5)
        ),
        "qualified_format_name_end_navigation_retained": any(
            at_declaration(observations.get("qualified_format_name_end_control"), caller, 1, end_line, (0, 7))
            for end_line in (1, 4, 5)
        ),
        **{f"super_{case}_does_not_select_unproved_callable": (
            f"super_{case}_call" in observations
            and observations[f"super_{case}_call"] in (None, [])
        ) for case, _ in SUPER_CASES},
        "super_enclosing_helper_declaration_retained": any(
            at_declaration(observations.get("super_helper_declaration_control"), caller, 1, end_line, (0, 4))
            for end_line in (1, 8)
        ),
        "super_proven_parent_override_retained": at_declaration(
            observations.get("super_parent_override_control"), caller, 1, start_characters=(0, 4)),
        "utf16_negotiated": report.get("position_encoding") == "utf-16",
        "server_clean_exit": report.get("exit") == 0 and report.get("cleanup") == "protocol_exit_reaped",
        "binary_unchanged": (
            isinstance(report.get("binary_sha256_before"), str)
            and re.fullmatch(r"[0-9a-f]{64}", report["binary_sha256_before"]) is not None
            and report["binary_sha256_before"] == report.get("binary_sha256_after")
        ),
    }


class SupersededRequest(ValueError):
    """A document edit overtook a request; retain the response before retrying."""


class Session:
    def __init__(self, binary: Path, app: Path, environment: dict[str, str], report: dict):
        self.report = report
        self.start = time.perf_counter()
        self.deadline = self.start + SESSION_SECONDS
        self.serial = 0
        self.messages: queue.Queue = queue.Queue(maxsize=32)
        self.errors: list[str] = []
        self.stderr_bytes = 0
        self.traffic_bytes = 0
        command = [str(binary), "--stdio", "--file-watchers", "false"]
        self.process = subprocess.Popen(command, cwd=app, env=environment, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        report.update(pid=self.process.pid, command=command)
        self.readers = [threading.Thread(target=self.read_messages, daemon=True),
                        threading.Thread(target=self.read_errors, daemon=True)]
        for reader in self.readers:
            reader.start()

    def read_errors(self):
        for line in iter(lambda: self.process.stderr.readline(8192), b""):
            if self.stderr_bytes < 65536:
                self.errors.append(line[:65536 - self.stderr_bytes].decode("utf-8", "replace"))
                self.stderr_bytes += len(line)

    def read_messages(self):
        try:
            while True:
                headers = {}
                while True:
                    line = self.process.stdout.readline(8192)
                    if not line:
                        return
                    if line in (b"\r\n", b"\n"):
                        break
                    key, value = line.decode("ascii").split(":", 1)
                    headers[key.lower()] = value.strip()
                length = int(headers["content-length"])
                if not 0 < length <= 1024 * 1024:
                    raise ValueError("invalid or oversized protocol frame")
                data = self.process.stdout.read(length)
                if len(data) != length:
                    raise ValueError("truncated protocol frame")
                self.messages.put(json.loads(data), timeout=REQUEST_SECONDS)
        except Exception as error:
            try:
                self.messages.put({"instrument_error": repr(error)}, timeout=REQUEST_SECONDS)
            except queue.Full:
                pass

    def frame(self, direction: str, message: dict):
        self.traffic_bytes += len(json.dumps(message).encode("utf-8"))
        if self.traffic_bytes > 4 * 1024 * 1024:
            raise ValueError("protocol receipt exceeds 4 MiB")
        self.report["frames"].append({"direction": direction,
            "elapsed_ms": round((time.perf_counter() - self.start) * 1000, 2), "message": message})

    def send(self, message: dict):
        self.frame("send", message)
        data = json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        self.process.stdin.write(f"Content-Length: {len(data)}\r\n\r\n".encode("ascii") + data)
        self.process.stdin.flush()

    def notify(self, method: str, params: object):
        self.send({"jsonrpc": "2.0", "method": method, "params": params})

    def receive(self, deadline: float) -> dict:
        message = self.messages.get(timeout=max(0.001, min(deadline, self.deadline) - time.perf_counter()))
        if "instrument_error" in message:
            raise ValueError(message["instrument_error"])
        self.frame("receive", message)
        if "id" in message and "method" in message:
            answer = [{} for _ in message.get("params", {}).get("items", [])] if message["method"] == "workspace/configuration" else None
            self.send({"jsonrpc": "2.0", "id": message["id"], "result": answer})
        return message

    def request(self, method: str, params: object, operation_deadline: float | None = None):
        self.serial += 1
        serial = self.serial
        start = time.perf_counter()
        self.send({"jsonrpc": "2.0", "id": serial, "method": method, "params": params})
        deadline = min(start + REQUEST_SECONDS, self.deadline,
                       operation_deadline if operation_deadline is not None else self.deadline)
        while True:
            response = self.receive(deadline)
            if response.get("id") == serial and "method" not in response:
                break
        self.report["requests"].append({"method": method, "params": params,
            "elapsed_ms": round((time.perf_counter() - start) * 1000, 2), "response": response})
        error = response.get("error")
        if (isinstance(error, dict) and error.get("code") == -32800
                and str(error.get("message", "")).startswith("Request superseded: document moved")):
            raise SupersededRequest(str(error))
        if "error" in response or "result" not in response:
            raise ValueError(f"invalid {method} response: {response}")
        return response["result"]

    def request_after_edit(self, method: str, params: object):
        deadline = min(time.perf_counter() + REQUEST_SECONDS, self.deadline)
        while time.perf_counter() < deadline:
            try:
                return self.request(method, params, operation_deadline=deadline)
            except SupersededRequest:
                pass
        raise TimeoutError(f"{method} remained superseded after an edit")

    def wait_ready(self):
        deadline = min(time.perf_counter() + REQUEST_SECONDS, self.deadline)
        while not any(frame["direction"] == "receive"
                      and frame["message"].get("method") == "perl-lsp/index-ready"
                      and frame["message"].get("params", {}).get("ready") is True
                      for frame in self.report["frames"]):
            self.receive(deadline)

    def wait_target(self, target: Path, name: str = "compute_0", package: str = "Scale24::Mod00"):
        deadline = min(time.perf_counter() + REQUEST_SECONDS, self.deadline)
        while time.perf_counter() < deadline:
            symbols = self.request("workspace/symbol", {"query": name})
            if any(symbol.get("containerName") == package
                   and unquote(symbol.get("location", {}).get("uri", "")) == target.as_uri()
                   for symbol in symbols):
                return symbols
            try:
                self.receive(min(time.perf_counter() + 0.025, deadline))
            except queue.Empty:
                pass
        raise TimeoutError("target didOpen did not become visible in the index")

    def close(self):
        try:
            self.request("shutdown", None)
            self.notify("exit", None)
            self.process.wait(timeout=4)
            self.report["cleanup"] = "protocol_exit_reaped"
        except Exception as error:
            if self.process.poll() is None:
                self.process.kill()
            self.process.wait(timeout=4)
            self.report["cleanup"] = f"owned_process_killed_reaped: {error!r}"
        for reader in self.readers:
            reader.join(timeout=1)
        self.report.update(exit=self.process.returncode, stderr="".join(self.errors),
                           elapsed_ms=round((time.perf_counter() - self.start) * 1000, 2))
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            stream.close()


def one_shot(command: list[str], app: Path, environment: dict[str, str], report: dict):
    start = time.perf_counter()
    result = subprocess.run(command, cwd=app, env=environment, capture_output=True, timeout=8)
    entry = {"command": command, "exit": result.returncode,
             "elapsed_ms": round((time.perf_counter() - start) * 1000, 2),
             "stdout": result.stdout.decode("utf-8", "replace"),
             "stderr": result.stderr.decode("utf-8", "replace")}
    report["cli"].append(entry)
    if result.returncode != 0:
        raise ValueError(f"CLI command failed: {entry}")
    return entry


def run_probe(binary: Path, fixture: Path, report: dict):
    environment = stock_environment()
    app = fixture / "app"
    caller = app / "main.pl"
    target = fixture / "lib" / "Scale24" / "Mod00.pm"
    app.mkdir(parents=True)
    target.parent.mkdir(parents=True)
    for path, text in [(caller, CALLER), (app / "Decoy.pm", DECOY), (target, TARGET_TEXT)]:
        path.write_bytes(text.encode("utf-8"))
    report.update(frames=[], requests=[], observations={}, cli=[],
                  binary_sha256_before=digest(binary),
                  fixture={"workspace_root": str(app), "target_outside_workspace": str(target),
                      "files": [{"path": str(path.relative_to(fixture)), "bytes": path.stat().st_size,
                                 "sha256": digest(path)} for path in (caller, app / "Decoy.pm", target)]},
                  timeouts={"request_seconds": REQUEST_SECONDS, "session_seconds": SESSION_SECONDS})
    session = None
    try:
        version = one_shot([str(binary), "--version"], app, environment, report)
        revision = re.search(r"Git commit:\s*([0-9a-f]{7,40})\b", version["stdout"])
        if revision is None or not report["source_sha"].startswith(revision.group(1)):
            raise ValueError("binary version revision differs from its declared source")
        report["binary_revision"] = revision.group(1)
        one_shot([str(binary), "--identity-json"], app, environment, report)
        perl = shutil.which("perl", path=environment.get("PATH"))
        if perl is None:
            raise FileNotFoundError("Perl is required for the independent binding oracle")
        report["perl"] = {"path": perl, "sha256": digest(Path(perl))}
        oracle = one_shot([perl, "main.pl"], app, environment, report)
        if oracle["stdout"].splitlines() != ["target", "caller"]:
            raise ValueError(f"independent Perl binding failed: {oracle}")
        session = Session(binary, app, environment, report)
        initialized = session.request("initialize", {"processId": os.getpid(), "rootUri": app.as_uri(),
            "workspaceFolders": [{"uri": app.as_uri(), "name": "bounded-caller-workspace"}],
            "clientInfo": {"name": "qualified-definition-protocol-probe", "version": "1"},
            "capabilities": {"general": {"positionEncodings": ["utf-16"]}, "workspace": {"configuration": False}}})
        report["position_encoding"] = initialized.get("capabilities", {}).get("positionEncoding")
        session.notify("initialized", {})
        session.notify("textDocument/didOpen", {"textDocument": {"uri": caller.as_uri(),
            "languageId": "perl", "version": 1, "text": CALLER}})
        session.wait_ready()
        report["observations"]["caller_symbols"] = session.request("textDocument/documentSymbol", {"textDocument": {"uri": caller.as_uri()}})
        report["observations"]["index_before_target_open"] = session.request("workspace/symbol", {"query": "compute_0"})
        qualified_params = {"textDocument": {"uri": caller.as_uri()}, "position": {"line": 6, "character": 24}}
        report["observations"]["qualified_before_target_open"] = session.request("textDocument/definition", qualified_params)
        report["observations"]["bare_local_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 7, "character": 8}})
        session.notify("textDocument/didOpen", {"textDocument": {"uri": target.as_uri(),
            "languageId": "perl", "version": 1, "text": TARGET_TEXT}})
        report["observations"]["index_after_target_open"] = session.wait_target(target)
        report["observations"]["qualified_after_target_open"] = session.request("textDocument/definition", qualified_params)
        # Use the same open buffer and process: these are edits, not extra files.
        report["fixture"]["open_buffer_variants"] = [
            {"version": version, "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(), "text": text}
            for version, text in ((2, PACKAGE_BLOCK), (3, QUALIFIED_ALIAS), (4, CONSTANT_VALUE),
                                  (5, LABEL_CALL), (6, FORMAT_CALL), (7, MOO_CALL), (8, SAME_NAME_PACKAGE),
                                  (9, ARITHMETIC_CALLS), (10, QUALIFIED_FORMAT_CALL),
                                  (11, SUPER_MISSING), (12, SUPER_PARENT))
        ]
        one_shot([perl, "-c", "-e", PACKAGE_BLOCK], app, environment, report)
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 2},
                       "contentChanges": [{"text": PACKAGE_BLOCK}]})
        report["observations"]["qualified_inside_package_block"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 0, "character": PACKAGE_BLOCK.index("compute_0") + 2}})
        alias_oracle = one_shot([perl, "-e", QUALIFIED_ALIAS], app, environment, report)
        if alias_oracle["stdout"] != "2":
            raise ValueError(f"independent qualified alias binding failed: {alias_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 3},
                       "contentChanges": [{"text": QUALIFIED_ALIAS}]})
        report["observations"]["qualified_alias_after_edit"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 2, "character": 15}})
        # Supply the compile-time callee only to Perl's independent oracle. It is
        # deliberately unavailable in the LSP workspace/open buffers after this edit.
        constant_oracle = one_shot([perl, "-I../lib", "-MScale24::Mod00", "-e",
            "BEGIN { *Other::compute_0 = \\&Scale24::Mod00::compute_0; }\n" + CONSTANT_VALUE], app, environment, report)
        if constant_oracle["stdout"].splitlines() != ["target", "target"]:
            raise ValueError(f"independent constant binding failed: {constant_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 4},
                       "contentChanges": [{"text": CONSTANT_VALUE}]})
        constant_lines = CONSTANT_VALUE.splitlines()
        report["observations"]["qualified_inside_constant_value"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 1, "character": constant_lines[1].index("compute_0") + 2}})
        report["observations"]["qualified_constant_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 2, "character": constant_lines[2].index("VALUE") + 2}})
        report["observations"]["bare_constant_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 3, "character": constant_lines[3].index("VALUE") + 2}})
        label_oracle = one_shot([perl, "-I../lib", "-MScale24::Mod00", "-e",
            "BEGIN { *Other::compute_0 = \\&Scale24::Mod00::compute_0; }\n" + LABEL_CALL], app, environment, report)
        if label_oracle["stdout"].splitlines() != ["target"]:
            raise ValueError(f"independent label binding failed: {label_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 5},
                       "contentChanges": [{"text": LABEL_CALL}]})
        label_lines = LABEL_CALL.splitlines()
        report["observations"]["qualified_inside_labeled_statement"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 2, "character": label_lines[2].index("compute_0") + 2}})
        report["observations"]["goto_label_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 1, "character": 7}})
        report["observations"]["label_declaration_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 2, "character": 2}})
        format_oracle = one_shot([perl, "-I../lib", "-MScale24::Mod00", "-e",
            "BEGIN { *Other::compute_0 = \\&Scale24::Mod00::compute_0; }\n" + FORMAT_CALL
            + "$~ = 'REPORT'; write;\n"], app, environment, report)
        if format_oracle["stdout"].splitlines() != ["targe"]:
            raise ValueError(f"independent format binding failed: {format_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 6},
                       "contentChanges": [{"text": FORMAT_CALL}]})
        report["observations"]["qualified_inside_format_value"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 3, "character": 9}})
        report["observations"]["format_declaration_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 1, "character": 9}})
        variable_oracle = one_shot([perl, "-e", "package Caller; our $kept = 7; print $Caller::kept;"], app, environment, report)
        if variable_oracle["stdout"] != "7":
            raise ValueError(f"independent qualified variable binding failed: {variable_oracle}")
        report["language_oracle_limits"] = ["Moo runtime dependency/default execution not exercised; attribute case tests parser-produced metadata"]
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 7},
                       "contentChanges": [{"text": MOO_CALL}]})
        report["observations"]["qualified_inside_attribute_default"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 2, "character": MOO_CALL.splitlines()[2].index("compute_0") + 2}})
        report["observations"]["qualified_variable_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 4, "character": 11}})
        report["observations"]["bare_variable_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 5, "character": 3}})
        package_oracle = one_shot([perl, "-I../lib", "-MScale24::Mod00", "-e",
            "BEGIN { *Other::compute_0 = \\&Scale24::Mod00::compute_0; }\n" + SAME_NAME_PACKAGE
            + "print Other::compute_0();\n"], app, environment, report)
        if package_oracle["stdout"].splitlines() != ["target"]:
            raise ValueError(f"independent package/callable role binding failed: {package_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 8},
                       "contentChanges": [{"text": SAME_NAME_PACKAGE}]})
        report["observations"]["qualified_call_same_name_package"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 0, "character": SAME_NAME_PACKAGE.rindex("compute_0") + 2}})
        report["observations"]["same_name_package_declaration_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 0, "character": SAME_NAME_PACKAGE.index("compute_0") + 2}})
        arithmetic_oracle = one_shot([perl, "-e",
            "BEGIN { *Other::compute_0 = sub { 3 }; }\n" + ARITHMETIC_CALLS
            + "print qq{operator-ok\\n};\n"], app, environment, report)
        if arithmetic_oracle["stdout"].splitlines() != ["operator-ok"]:
            raise ValueError(f"independent arithmetic/call role binding failed: {arithmetic_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 9},
                       "contentChanges": [{"text": ARITHMETIC_CALLS}]})
        for line, case in enumerate(ARITHMETIC_CASES, 1):
            report["observations"][f"qualified_{case}_call"] = session.request_after_edit("textDocument/definition", {
                "textDocument": {"uri": caller.as_uri()},
                "position": {"line": line, "character": ARITHMETIC_CALLS.splitlines()[line].index("compute_0") + 2}})
        qualified_format_oracle = one_shot([perl, "-I../lib", "-MScale24::Mod00", "-e",
            "BEGIN { *Other::compute_0 = \\&Scale24::Mod00::compute_0; }\n" + QUALIFIED_FORMAT_CALL
            + "$~ = 'Other::REPORT'; write;\n"], app, environment, report)
        if qualified_format_oracle["stdout"].splitlines() != ["targe"]:
            raise ValueError(f"independent qualified format binding failed: {qualified_format_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 10},
                       "contentChanges": [{"text": QUALIFIED_FORMAT_CALL}]})
        report["observations"]["qualified_inside_qualified_format_value"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 3, "character": 9}})
        report["observations"]["qualified_format_declaration_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 1, "character": QUALIFIED_FORMAT_CALL.splitlines()[1].index("REPORT") + 2}})
        report["observations"]["qualified_format_name_end_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 1, "character": QUALIFIED_FORMAT_CALL.splitlines()[1].index("REPORT") + len("REPORT")}})
        # Missing ancestors must not turn SUPER into permission to select the
        # enclosing/same-named Caller subroutine. Perl independently rejects all
        # five call shapes; the final edit retains a genuine Base override.
        for case, statement in SUPER_CASES:
            oracle_source = ("package Caller; sub helper { my $self = shift; " + statement
                             + " } my $self = bless {}, 'Caller'; eval { $self->helper(); };"
                             + "die 'unexpected successful SUPER call' unless $@; print 'missing:' . $@;")
            oracle = one_shot([perl, "-e", oracle_source], app, environment, report)
            name = "helper" if case == "same_name" else "missing"
            if (not oracle["stdout"].startswith("missing:") or name not in oracle["stdout"]
                    or re.search(r"Undefined subroutine|Can't locate object method", oracle["stdout"]) is None):
                raise ValueError(f"independent missing SUPER binding failed ({case}): {oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 11},
                       "contentChanges": [{"text": SUPER_MISSING}]})
        for line, (case, statement) in enumerate(SUPER_CASES, start=3):
            name = "helper" if case == "same_name" else "missing"
            report["observations"][f"super_{case}_call"] = session.request_after_edit("textDocument/definition", {
                "textDocument": {"uri": caller.as_uri()},
                "position": {"line": line, "character": 4 + statement.index(name) + 2}})
        report["observations"]["super_helper_declaration_control"] = session.request("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()}, "position": {"line": 1, "character": 6}})
        parent_oracle = one_shot([perl, "-e", SUPER_PARENT], app, environment, report)
        if parent_oracle["stdout"] != "base":
            raise ValueError(f"independent SUPER inheritance binding failed: {parent_oracle}")
        session.notify("textDocument/didChange", {"textDocument": {"uri": caller.as_uri(), "version": 12},
                       "contentChanges": [{"text": SUPER_PARENT}]})
        report["observations"]["super_index_after_edit"] = session.wait_target(caller, "override", "Base")
        report["observations"]["super_parent_override_control"] = session.request_after_edit("textDocument/definition", {
            "textDocument": {"uri": caller.as_uri()},
            "position": {"line": 5, "character": SUPER_PARENT.splitlines()[5].index("override") + 2}})
    except Exception as error:
        report["instrument_failure"] = repr(error)
    finally:
        if session is not None:
            session.close()
        report["binary_sha256_after"] = digest(binary)
        report["assertions"] = assertions(report, caller, target)
    return "instrument_failure" not in report and all(report["assertions"].values())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--receipt-dir", required=True, type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", args.source_sha):
        parser.error("--source-sha must be an exact Git revision")
    binary = args.binary.resolve(strict=True)
    output = args.receipt_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    receipt = output / "qualified_definition_protocol.json"
    if receipt.exists():
        parser.error("refusing to overwrite an existing protocol receipt")
    report = {"schema_version": 1, "scope": "three-file external-library qualified definition protocol",
              "source_sha": args.source_sha, "binary": str(binary), "binary_bytes": binary.stat().st_size,
              "instrument_sha256": digest(Path(__file__)),
              "not_proven": ["1000-file ScanTimeout/index catch-up/resource report", "healthy-workspace second-open divergence",
                             "installed/editor accepted-v2 72-cell-plus-human acceptance"]}
    try:
        observed = subprocess.run(["git", "rev-parse", "HEAD", "HEAD^{tree}"], cwd=REPO,
                                  check=True, capture_output=True, text=True, timeout=5).stdout.splitlines()
        if observed[0] != args.source_sha:
            raise ValueError(f"checkout {observed[0]} differs from requested source {args.source_sha}")
        report["source_tree"] = observed[1]
        if subprocess.run(["git", "diff", "--exit-code", "HEAD", "--"], cwd=REPO,
                          capture_output=True, timeout=5).returncode != 0:
            raise ValueError("tracked source differs from the declared checkout")
        success = run_probe(binary, output / "fixture", report)
        report["result"] = "pass" if success else "fail"
    except Exception as error:
        report.update(result="fail", instrument_failure=repr(error))
    receipt.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"source_sha": args.source_sha, "result": report["result"],
                      "assertions": report.get("assertions"), "instrument_failure": report.get("instrument_failure"),
                      "receipt": str(receipt)}, ensure_ascii=True))
    return 0 if report["result"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
