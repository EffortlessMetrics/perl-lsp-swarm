"""Exercise the shipped demo against a freshly built perllsp stdio binary.

Usage: python scripts/check-demo-workspace-wire.py --binary /path/to/perllsp
The caller supplies the exact binary built from this checkout; there is no PATH fallback.
"""

import argparse
import hashlib
import json
import queue
import subprocess
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEMO = ROOT / "demo_workspace"


class Client:
    def __init__(self, binary: Path):
        self.process = subprocess.Popen(
            [str(binary), "--stdio"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self.messages = queue.Queue()
        self.pending = []
        self.next_id = 1
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=self._drain_stderr, daemon=True).start()

    def _read(self):
        stream = self.process.stdout
        try:
            while True:
                headers = {}
                while line := stream.readline():
                    if line == b"\r\n":
                        break
                    name, value = line.decode("ascii").split(":", 1)
                    headers[name.lower()] = value.strip()
                if not headers:
                    break
                size = int(headers["content-length"])
                self.messages.put(json.loads(stream.read(size)))
        except Exception as error:
            self.messages.put({"reader_error": str(error)})

    def _drain_stderr(self):
        self.stderr = []
        for line in self.process.stderr:
            self.stderr.append(line.decode("utf-8", "replace").rstrip())
            self.stderr = self.stderr[-40:]

    def send(self, message):
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.process.stdin.flush()

    def take(self, predicate, timeout=20):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for index, message in enumerate(self.pending):
                if predicate(message):
                    return self.pending.pop(index)
            try:
                message = self.messages.get(timeout=min(0.25, deadline - time.monotonic()))
            except queue.Empty:
                continue
            if "reader_error" in message:
                raise AssertionError(message)
            if "method" in message and "id" in message:
                # Default client settings: the demo must be clean without configuration.
                self.send({"id": message["id"], "result": [{}]})
            elif predicate(message):
                return message
            else:
                self.pending.append(message)
        raise AssertionError(f"timed out; pending={self.pending[-5:]}; stderr={self.stderr}")

    def request(self, method, params):
        request_id = self.next_id
        self.next_id += 1
        self.send({"id": request_id, "method": method, "params": params})
        response = self.take(lambda message: message.get("id") == request_id)
        assert "error" not in response, response
        return response.get("result")

    def notification(self, method, params):
        self.send({"method": method, "params": params})

    def diagnostics(self, uri, version, predicate):
        message = self.take(
            lambda item: item.get("method") == "textDocument/publishDiagnostics"
            and item.get("params", {}).get("uri") == uri
            and item.get("params", {}).get("version") == version
            and predicate(item["params"]["diagnostics"]),
            timeout=30,
        )
        return message["params"]["diagnostics"]

    def assert_demo_stays_clean(self, uris):
        """Catch a later full diagnostic pass replacing an early empty report."""
        deadline = time.monotonic() + 10
        quiet_until = time.monotonic() + 2
        while time.monotonic() < min(deadline, quiet_until):
            for message in self.pending[:]:
                if message.get("method") == "textDocument/publishDiagnostics":
                    params = message.get("params", {})
                    if params.get("uri") in uris and params.get("version") == 1:
                        assert not params.get("diagnostics"), message
                        self.pending.remove(message)
                        quiet_until = time.monotonic() + 2
            try:
                message = self.messages.get(timeout=min(0.25, quiet_until - time.monotonic()))
            except queue.Empty:
                continue
            if "reader_error" in message:
                raise AssertionError(message)
            if "method" in message and "id" in message:
                self.send({"id": message["id"], "result": [{}]})
            elif message.get("method") == "textDocument/publishDiagnostics" and (
                message.get("params", {}).get("uri") in uris
                and message.get("params", {}).get("version") == 1
            ):
                assert not message["params"]["diagnostics"], message
                quiet_until = time.monotonic() + 2
            else:
                self.pending.append(message)


def position(source, token, after=False):
    offset = source.index(token) + (len(token) if after else 0)
    return {"line": source.count("\n", 0, offset),
            "character": offset - source.rfind("\n", 0, offset) - 1}


def locations(result):
    if isinstance(result, list):
        return result
    if isinstance(result, dict):
        return [result]
    return []


def run(binary):
    assert binary.is_file(), binary
    print(f"binary={binary} sha256={hashlib.sha256(binary.read_bytes()).hexdigest()}")
    client = Client(binary)
    root_uri = DEMO.as_uri()
    try:
        client.request("initialize", {
            "processId": None, "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": "demo_workspace"}],
            "capabilities": {"workspace": {"configuration": True},
                             "textDocument": {"publishDiagnostics": {"versionSupport": True}}},
        })
        client.notification("initialized", {})
        client.take(lambda item: item.get("method") == "perl-lsp/index-ready"
                    and item.get("params", {}).get("ready") is True, timeout=60)

        sources = {name: (DEMO / name).read_text(encoding="utf-8")
                   for name in ("main.pl", "lib/Utils.pm", "lib/Database.pm")}
        uris = {name: (DEMO / name).as_uri() for name in sources}
        for name, source in sources.items():
            client.notification("textDocument/didOpen", {"textDocument": {
                "uri": uris[name], "languageId": "perl", "version": 1, "text": source}})
        for name in sources:
            assert client.diagnostics(uris[name], 1, lambda items: not items) == [], name
        print("default diagnostics: clean for all three demo files")

        main = sources["main.pl"]
        for package, expected in (("Utils::", {"load_data", "process_data"}),
                                  ("Database::", {"save"})):
            result = client.request("textDocument/completion", {
                "textDocument": {"uri": uris["main.pl"]},
                "position": position(main, package, after=True)})
            items = result if isinstance(result, list) else result.get("items", [])
            labels = {item["label"] for item in items}
            assert expected <= labels, (package, expected, labels)
        print("project package completion: Utils and Database symbols present")

        call = position(main, "process_data")
        call["character"] += 3
        document = {"textDocument": {"uri": uris["main.pl"]}, "position": call}
        hover = client.request("textDocument/hover", document)
        assert hover, hover
        definitions = locations(client.request("textDocument/definition", document))
        declaration_line = position(sources["lib/Utils.pm"], "sub process_data")["line"]
        assert any(item.get("uri") == uris["lib/Utils.pm"] and
                   item.get("range", {}).get("start", {}).get("line") == declaration_line
                   for item in definitions), definitions
        references = locations(client.request("textDocument/references", {
            **document, "context": {"includeDeclaration": True}}))
        assert any(item.get("uri") == uris["main.pl"] and
                   item.get("range", {}).get("start", {}).get("line") == call["line"]
                   for item in references), references
        print("hover, cross-file definition and call-site references: present")

        control_uri = (DEMO / "lib" / "DeadCodeControl.pm").as_uri()
        client.notification("textDocument/didOpen", {"textDocument": {
            "uri": control_uri, "languageId": "perl", "version": 1,
            "text": "package DeadCodeControl;\nsub unused_probe { return 42; }\n1;\n"}})
        control = client.diagnostics(control_uri, 1, lambda items: any(
            item.get("code") == "dead-code-subroutine" for item in items))
        assert any(item.get("code") == "dead-code-subroutine"
                   and item.get("severity") == 4 and 1 in item.get("tags", [])
                   for item in control), control
        client.assert_demo_stays_clean(set(uris.values()))
        print("negative control: dead-code hint with Unnecessary tag")

        broken = main.replace("Utils::process_data($data)", "Utils::process_data($data")
        assert broken != main
        client.notification("textDocument/didChange", {"textDocument": {
            "uri": uris["main.pl"], "version": 2}, "contentChanges": [{"text": broken}]})
        client.diagnostics(uris["main.pl"], 2, lambda items: any(
            item.get("severity") == 1 for item in items))
        client.notification("textDocument/didChange", {"textDocument": {
            "uri": uris["main.pl"], "version": 3}, "contentChanges": [{"text": main}]})
        client.diagnostics(uris["main.pl"], 3, lambda items: not items)
        print("edit/error/clear: versioned diagnostics passed")
        client.request("shutdown", None)
        client.notification("exit", None)
    finally:
        if client.process.poll() is None:
            client.process.terminate()
            client.process.wait(timeout=5)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    run(args.binary.resolve())
