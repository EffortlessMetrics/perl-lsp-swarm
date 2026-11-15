#!/usr/bin/env python3
"""Exact-production-source error-graph proof for #13998.

Requires Python 3.11+. Run from a full candidate checkout. Uses a temporary Cargo workspace,
not the Perl workspace, and never downloads dependencies or toolchains. Exit 0
means the selected graph passed; 1 means candidate rejection; 2 means no proof.
This is not package, public-API, product, or runtime-conformance qualification.
The Cargo runner currently requires POSIX process-group settlement; other hosts
return NOT_PROVEN rather than clean a build with untracked descendants.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import signal
import subprocess
import sys
import tempfile
import tomllib

CORE = "crates/perl-lsp-rs-core/src/"
SOURCES = (
    CORE + "protocol/jsonrpc.rs",
    CORE + "transport/mod.rs",
    CORE + "transport/framing.rs",
    CORE + "transport/framing_error.rs",
    CORE + "transport/incoming.rs",
    CORE + "transport/incoming_error.rs",
    CORE + "runtime/cancellation/mod.rs",
    CORE + "transport/incoming_message.rs",
)
CONTRACT = "scripts/ci/fixtures/lsp_error_contract.rs"
STRICT_DECODE = "crates/perl-lsp-rs-core/tests/strict_transport_decode.rs"
ERROR_TYPES = (
    "protocol::JsonRpcError",
    "transport::FramingError",
    "transport::IncomingMessageError",
    "runtime::cancellation::CancellationError",
)
ROOT_NAME = "neutral-lsp-error-probe"
DIRECT = ("serde", "serde_json", "tracing")
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


class NotProven(RuntimeError):
    """The observation or execution instrument is unavailable/inconsistent."""


class Rejected(RuntimeError):
    """An executed check rejected this candidate or dependency graph."""


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def quoted(value: str) -> str:
    if any(ord(char) < 32 for char in value):
        raise NotProven("control character in Rust/TOML path")
    return json.dumps(value, ensure_ascii=False)


def snapshot(root: Path) -> dict[str, str]:
    result = {}
    for relative in (*SOURCES, CONTRACT, STRICT_DECODE, "Cargo.lock", "rust-toolchain.toml"):
        path = (root / relative).resolve(strict=True)
        path.relative_to(root)  # No external symlink may supply selected source.
        result[relative] = digest(path.read_bytes())
    return result


def registry_index(lock: dict) -> dict[tuple[str, str], str]:
    result = {}
    for package in lock.get("package", []):
        if package.get("source") != REGISTRY:
            continue
        key = (package["name"], package["version"])
        checksum = package.get("checksum", "")
        if not re.fullmatch(r"[0-9a-f]{64}", checksum) or key in result:
            raise Rejected(f"ambiguous or unchecked registry identity: {key}")
        result[key] = checksum
    return result


def selected_versions(lock: dict) -> dict[str, str]:
    index = registry_index(lock)
    result = {}
    for name in DIRECT:
        choices = [version for candidate, version in index if candidate == name]
        if len(choices) != 1:
            raise NotProven(f"expected one reviewed registry version for {name}: {choices}")
        result[name] = choices[0]
    return result


def manifest(root: Path, versions: dict[str, str]) -> str:
    if set(versions) != set(DIRECT):
        raise Rejected("direct dependency denominator changed")
    return f'''[package]
name = "{ROOT_NAME}"
version = "0.0.0"
edition = "2024"
publish = false
autotests = false

[workspace]
resolver = "2"

[dependencies]
serde = {{ version = "={versions['serde']}", features = ["derive"] }}
serde_json = "={versions['serde_json']}"
tracing = "={versions['tracing']}"

[[test]]
name = "error_contract"
path = {quoted(str(root / CONTRACT))}

[profile.dev]
debug = 0
incremental = false

[profile.test]
debug = 0
incremental = false
'''


def driver(root: Path) -> str:
    # Import actual files. In particular, transport/mod.rs resolves its actual
    # framing/incoming children; no stand-in FramingError or decoder is used.
    return f'''//! Source-linked probe; deliberately not a published runtime package.
#[path = {quoted(str(root / SOURCES[0]))}]
pub mod protocol;
#[path = {quoted(str(root / SOURCES[1]))}]
pub mod transport;
/// The real cancellation module is compiled, not a reconstructed error enum.
pub mod runtime {{
    #[path = {quoted(str(root / SOURCES[6]))}]
    pub mod cancellation;
}}
'''


def validate_lock(lock: dict, baseline: dict, versions: dict[str, str]) -> None:
    if lock.get("version") != 4:
        raise Rejected("isolated Cargo.lock must use version 4")
    trusted = registry_index(baseline)
    roots = 0
    actual_direct = set()
    seen = set()
    for package in lock.get("package", []):
        name, version = package["name"], package["version"]
        key = (name, version)
        if key in seen:
            raise Rejected(f"duplicate lock identity: {key}")
        seen.add(key)
        if name == ROOT_NAME:
            roots += 1
            if version != "0.0.0" or "source" in package or "checksum" in package:
                raise Rejected("invalid isolated root")
            actual_direct = {dep.split()[0] for dep in package.get("dependencies", [])}
            continue
        if package.get("source") != REGISTRY or name.startswith("perl-") or name == "perllsp":
            raise Rejected(f"non-neutral or non-registry dependency: {name}")
        if not trusted.get(key) or package.get("checksum") != trusted[key]:
            raise Rejected(f"unreviewed registry version/checksum: {key}")
    if roots != 1 or actual_direct != set(DIRECT):
        raise Rejected("missing root or changed direct dependency denominator")
    for name, version in versions.items():
        if (name, version) not in seen:
            raise Rejected(f"missing selected direct dependency: {name}")


def validate_metadata(metadata: dict, directory: Path, lock: dict) -> None:
    packages = metadata.get("packages", [])
    roots = [p for p in packages if p.get("name") == ROOT_NAME]
    if len(roots) != 1 or metadata.get("workspace_members") != [roots[0]["id"]]:
        raise Rejected("probe joined another workspace or lost its root")
    if Path(metadata["workspace_root"]).resolve() != directory.resolve():
        raise Rejected("Cargo resolved the surrounding workspace")
    if Path(roots[0]["manifest_path"]).resolve() != (directory / "Cargo.toml").resolve():
        raise Rejected("wrong isolated root manifest")
    admitted = {(p["name"], p["version"], p.get("source")) for p in lock["package"]}
    for package in packages:
        key = (package["name"], package["version"], package.get("source"))
        if key not in admitted or (package is not roots[0] and package.get("source") != REGISTRY):
            raise Rejected(f"resolved graph escaped the checked lock: {key}")
    identities = {(p["name"], p["version"], p.get("source")) for p in packages}
    # The lock may also retain inactive optional dependencies. Metadata's
    # resolved nodes, not lock row count, define the active graph.
    if len(packages) != len(identities) or not identities.issubset(admitted):
        raise Rejected("unregistered or duplicated resolved dependency graph")
    resolution = metadata.get("resolve") or {}
    nodes = resolution.get("nodes", [])
    node_ids = {node["id"] for node in nodes}
    if (resolution.get("root") != roots[0]["id"] or len(node_ids) != len(nodes)
            or node_ids != {package["id"] for package in packages}):
        raise Rejected("missing or inconsistent resolved dependency edges")
    root_node = next(node for node in nodes if node["id"] == roots[0]["id"])
    if {dep["name"] for dep in root_node.get("deps", [])} != set(DIRECT):
        raise Rejected("resolved direct dependency denominator changed")
    for node in nodes:
        if not set(node.get("dependencies", [])).issubset(node_ids):
            raise Rejected("resolved edge references a missing package")


def verify_compiled_sources(directory: Path, root: Path) -> list[str]:
    expected = {(root / relative).resolve() for relative in SOURCES}
    expected.add((directory / "src/lib.rs").resolve())
    return verify_compiler_inputs(directory, "neutral_lsp_error_probe-*.d", expected)


def verify_compiled_contract(directory: Path, root: Path) -> list[str]:
    # include_str! inputs in the real decoder suite are also compiler inputs.
    expected = {(root / path).resolve() for path in (
        CONTRACT, STRICT_DECODE, CORE + "transport/incoming.rs", CORE + "transport/framing.rs",
    )}
    return verify_compiler_inputs(directory, "error_contract-*.d", expected)


def verify_compiler_inputs(directory: Path, pattern: str, expected: set[Path]) -> list[str]:
    """Use rustc dep-info, not the driver's intended imports, as the read set."""
    depfiles = list(directory.rglob(pattern))
    if not depfiles:
        raise NotProven("compiler_dependency_file_missing")
    observed = set()
    for depfile in depfiles:
        lines = depfile.read_text(encoding="utf-8").replace("\\\n", " ").splitlines()
        if not lines or ": " not in lines[0]:
            raise NotProven("unrecognized_compiler_dependency_file")
        # rustc's Make-style escaping is accepted on POSIX. On a host where
        # path syntax does not round-trip, refuse rather than omit read inputs.
        for token in shlex.split(lines[0].split(": ", 1)[1]):
            path = Path(token)
            if not path.is_absolute():
                path = directory / path
            observed.add(path.resolve(strict=True))
    if observed != expected:
        raise Rejected("compiled_sources_omitted_copied_or_unregistered")
    return sorted(str(path) for path in observed)


def expected_contract_tests(root: Path) -> set[str]:
    """Bind named execution to the nine contract and 29 real decoder tests.

    These selected files use top-level #[test] functions. Refuse another
    shape rather than guessing Rust syntax or quietly shrinking coverage.
    """
    expected = set()
    for path, prefix, count in ((CONTRACT, "", 9), (STRICT_DECODE, "strict_transport_decode::", 29)):
        source = (root / path).read_text(encoding="utf-8")
        names = re.findall(r"(?m)^#\[test\]\s*\nfn ([A-Za-z_]\w*)\(", source)
        if len(names) != count or len(set(names)) != count:
            raise Rejected(f"test_source_denominator_changed:{path}")
        expected.update(prefix + name for name in names)
    return expected


def verify_test_execution(result: subprocess.CompletedProcess[str], expected: set[str] | None = None) -> list[str]:
    """Require named passing tests and one complete, unfiltered libtest suite."""
    summaries = re.findall(
        r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored; 0 measured; (\d+) filtered out;",
        result.stdout,
    )
    names = re.findall(r"(?m)^test ([A-Za-z_][A-Za-z_0-9:]*) \.\.\. ok$", result.stdout)
    if (result.returncode != 0 or len(summaries) != 1
            or summaries[0] != (str(len(names)), "0", "0")
            or not names or len(names) != len(set(names))
            or (expected is not None and set(names) != expected)):
        raise Rejected("production_and_contract_tests_not_proven")
    return sorted(names)


def forbidden_dependency_failure(result: subprocess.CompletedProcess[str]) -> bool:
    # Cargo 101 alone is not a passing negative control: require only the
    # intended compiler diagnostics, not missing dependencies or another error.
    if result.returncode != 101:
        return False
    errors = []
    for line in result.stdout.splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            return False
        if not isinstance(item, dict):
            return False
        if item.get("reason") != "compiler-message":
            continue
        message = item.get("message", {})
        if not isinstance(message, dict):
            return False
        if message.get("level") != "error":
            continue
        code = message.get("code") or {}
        if not isinstance(code, dict) or code.get("code") not in {"E0432", "E0433"}:
            return False
        if "perl_parser_core" not in message.get("message", ""):
            return False
        errors.append(message)
    return bool(errors)


def mutation(error_type: str, alias: bool) -> str:
    if error_type not in ERROR_TYPES:
        raise Rejected("unknown error-family control")
    prefix = ""
    namespace = "perl_parser_core"
    if alias:
        prefix = "mod taxonomy { pub use perl_parser_core::{ErrorClass, ErrorCategory}; }\n"
        namespace = "taxonomy"
    return prefix + f'''
impl {namespace}::ErrorClass for {error_type} {{
    fn error_class(&self) -> {namespace}::ErrorCategory {{
        {namespace}::ErrorCategory::Protocol
    }}
}}
'''


def run_process(command: list[str], *, cwd: Path, env: dict, timeout: float = 240):
    """Own the Cargo process group so a timeout cannot leave rustc writers."""
    if os.name != "posix":
        raise NotProven("process_group_settlement_not_supported_on_this_host")
    with subprocess.Popen(command, cwd=cwd, env=env, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          start_new_session=True) as process:
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate()
            raise
    return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)


def execute(root: Path) -> tuple[int, dict]:
    receipt: dict = {"schema": 1, "scope": "selected production error graph", "steps": []}
    try:
        tools = {name: shutil.which(os.environ.get(name.upper(), name)) for name in ("cargo", "rustc")}
        if not all(tools.values()):
            raise NotProven("cargo_or_rustc_not_found")
        root = root.resolve(strict=True)
        before = snapshot(root)
        contract_tests = expected_contract_tests(root)
        receipt["expected_contract_tests"] = sorted(contract_tests)
        baseline = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
        versions = selected_versions(baseline)
        toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
        receipt.update(sources=before, denominator=list(ERROR_TYPES), dependencies=versions, toolchain=toolchain)
        environment = dict(os.environ)
        for name in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET", "RUSTDOCFLAGS"):
            environment.pop(name, None)
        environment.update(RUSTUP_AUTO_INSTALL="0", RUSTUP_TOOLCHAIN=toolchain,
                           CARGO_NET_OFFLINE="true", CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0",
                           RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="",
                           RUSTC=os.path.abspath(tools["rustc"]))
        with tempfile.TemporaryDirectory(prefix="lsp-error-closure-") as temporary:
            directory = Path(temporary)
            environment["CARGO_TARGET_DIR"] = str(directory / "target")
            environment["CARGO_BUILD_BUILD_DIR"] = str(directory / "build")
            (directory / "src").mkdir()
            (directory / "Cargo.toml").write_text(manifest(root, versions), encoding="utf-8")
            source = driver(root)
            library = directory / "src/lib.rs"
            library.write_text(source, encoding="utf-8")
            # Seed accepted versions, then let Cargo prune to the independent
            # graph. Neither the root workspace nor its patches are imported.
            (directory / "Cargo.lock").write_bytes((root / "Cargo.lock").read_bytes())

            def run(name: str, args: list[str], *, compiler: bool = False):
                command = [os.path.abspath(tools["rustc" if compiler else "cargo"]), *args]
                result = run_process(command, cwd=directory, env=environment, timeout=240)
                receipt["steps"].append({"name": name, "command": command, "returncode": result.returncode,
                                         "stdout": result.stdout, "stderr": result.stderr})
                if snapshot(root) != before:
                    raise NotProven("selected_sources_changed_during_probe")
                return result

            for name, args, compiler in (("compiler", ["--version", "--verbose"], True),
                                          ("cargo", ["--version", "--verbose"], False)):
                if run(name, args, compiler=compiler).returncode != 0:
                    raise NotProven(f"{name}_unavailable_for_selected_toolchain")
            if run("lock", ["metadata", "--offline", "--format-version=1"]).returncode != 0:
                raise NotProven("isolated_offline_dependencies_unavailable")
            lock_bytes = (directory / "Cargo.lock").read_bytes()
            lock = tomllib.loads(lock_bytes.decode())
            validate_lock(lock, baseline, versions)
            receipt["lock_sha256"] = digest(lock_bytes)
            metadata = run("metadata", ["metadata", "--offline", "--locked", "--format-version=1"])
            if metadata.returncode != 0:
                raise NotProven("dependency_metadata_unavailable")
            resolved = json.loads(metadata.stdout)
            validate_metadata(resolved, directory, lock)
            receipt["resolved_graph"] = resolved
            checked = ["check", "--lib", "--offline", "--locked", "--message-format=json"]
            if run("neutral", checked).returncode != 0:
                raise Rejected("selected_production_graph_did_not_compile")
            receipt["compiler_read_set"] = verify_compiled_sources(directory, root)
            # Separate invocations bind each named suite; a summary alone or
            # another nine passing tests cannot stand in for the real consumer.
            receipt["production_tests"] = verify_test_execution(run(
                "production-tests", ["test", "--offline", "--locked", "--lib", "--", "--format=pretty"],
            ))
            receipt["contract_tests"] = verify_test_execution(run(
                "contract-tests", ["test", "--offline", "--locked", "--test", "error_contract", "--", "--format=pretty"],
            ), contract_tests)
            receipt["contract_compiler_read_set"] = verify_compiled_contract(directory, root)
            for error_type in ERROR_TYPES:
                for alias in (False, True):
                    library.write_text(source + mutation(error_type, alias), encoding="utf-8")
                    result = run(f"{'alias' if alias else 'direct'}:{error_type}", checked)
                    if not forbidden_dependency_failure(result):
                        raise Rejected("taxonomy_control_failed_or_failed_for_an_unrelated_reason")
            library.write_text(source, encoding="utf-8")
            if run("restored-neutral", checked).returncode != 0:
                raise Rejected("restored_graph_failed")
            if (directory / "Cargo.lock").read_bytes() != lock_bytes:
                raise NotProven("lock_changed_during_probe")
        receipt["outcome"] = "passed"
        return 0, receipt
    except Rejected as error:
        receipt.update(outcome="candidate_rejected", reason=str(error))
        return 1, receipt
    except (NotProven, OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.TimeoutExpired) as error:
        receipt.update(outcome="not_proven", reason=str(error))
        return 2, receipt


if __name__ == "__main__":
    code, report = execute(Path(__file__).resolve().parents[2])
    print(json.dumps(report, indent=2, ensure_ascii=False))
    sys.exit(code)
