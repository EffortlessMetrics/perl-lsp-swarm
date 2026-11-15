#!/usr/bin/env python3
"""Compile the exact std-only FramingError source without building the workspace.

This is #13996 component proof, not #13998's complete error-graph proof.
No source mirror, Cargo dependency, network access, or persistent target is used.
Exit 0 = component proof passed; 1 = rejected candidate; 2 = instrument failure.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

SOURCE = "crates/perl-lsp-rs-core/src/transport/framing_error.rs"


def rust_string(value: str) -> str:
    # JSON's ordinary quoted UTF-8 string syntax also fits these Rust paths.
    # Reject controls: JSON's \uNNNN escapes are not Rust's \u{NNNN} syntax.
    if any(ord(char) < 32 for char in value):
        raise ValueError("source path contains a control character")
    return json.dumps(value, ensure_ascii=False)


def taxonomy_rejection(result: subprocess.CompletedProcess[str]) -> bool:
    """Only the intended unresolved external taxonomy is a passing control."""
    if result.returncode != 1:
        return False
    codes = []
    try:
        for line in result.stderr.splitlines():
            item = json.loads(line)
            if item.get("level") != "error":
                continue
            code = item.get("code")
            # rustc's final "aborting due to ..." diagnostic has no code.
            if code is None:
                if not item.get("message", "").startswith("aborting due to"):
                    return False
                continue
            codes.append(code.get("code"))
            if codes[-1] not in {"E0432", "E0433"}:
                return False
            if "perl_parser_core" not in item.get("message", ""):
                return False
    except (ValueError, TypeError, AttributeError):
        return False
    return bool(codes)


def execute(root: Path) -> tuple[int, dict]:
    root = root.resolve()
    source = (root / SOURCE).resolve(strict=True)
    source.relative_to(root)  # A symlink outside the checkout cannot supply proof.
    before = source.read_bytes()
    receipt: dict = {
        "source": SOURCE,
        "source_sha256": hashlib.sha256(before).hexdigest(),
        "scope": "exact production FramingError source; std-only compilation",
        "cases": [],
    }
    compiler = shutil.which(os.environ.get("RUSTC", "rustc"))
    if compiler is None:
        receipt.update(outcome="instrument_failure", reason="rustc_not_found")
        return 2, receipt
    # Preserve the rustc launcher basename; resolving a rustup symlink changes
    # its dispatch identity. cwd=root preserves this checkout's toolchain file.
    compiler = os.path.abspath(compiler)
    environment = dict(os.environ, RUSTUP_AUTO_INSTALL="0")
    version = subprocess.run(
        [compiler, "--version", "--verbose"], cwd=root,
        text=True, capture_output=True, timeout=30, check=False, env=environment,
    )
    receipt.update(compiler=compiler, compiler_version=version.stdout.strip())
    if version.returncode != 0:
        receipt.update(outcome="instrument_failure", reason="rustc_version_failed")
        return 2, receipt

    prefix = f"#[path = {rust_string(str(source))}]\npub mod facts;\n"
    direct = """
impl perl_parser_core::ErrorClass for facts::FramingError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        perl_parser_core::ErrorCategory::Protocol
    }
}
"""
    alias = """
mod taxonomy {
    pub use perl_parser_core::{ErrorCategory, ErrorClass as Classify};
}
impl taxonomy::Classify for facts::FramingError {
    fn error_class(&self) -> taxonomy::ErrorCategory {
        taxonomy::ErrorCategory::Protocol
    }
}
"""
    cases = [("neutral", ""), ("direct_taxonomy", direct), ("aliased_taxonomy", alias)]
    with tempfile.TemporaryDirectory(prefix="framing-error-probe-") as temporary:
        directory = Path(temporary)
        for name, addition in cases:
            driver = directory / f"{name}.rs"
            driver.write_text(prefix + addition, encoding="utf-8")
            command = [
                compiler, "--edition=2024", "--crate-name=framing_error_probe",
                "--crate-type=lib", "--emit=metadata", "--error-format=json",
                "-Dwarnings", str(driver), "-o", str(directory / f"{name}.rmeta"),
            ]
            result = subprocess.run(
                command, cwd=root, text=True, capture_output=True, timeout=60, check=False,
                env=environment,
            )
            if source.read_bytes() != before:
                receipt.update(outcome="instrument_failure", reason="source_changed_during_probe")
                return 2, receipt
            passed = result.returncode == 0 if name == "neutral" else taxonomy_rejection(result)
            receipt["cases"].append({
                "case": name, "passed": passed, "returncode": result.returncode,
                "command": command, "stderr": result.stderr,
            })
            if not passed:
                receipt.update(outcome="candidate_rejected", reason=f"{name}_failed")
                return 1, receipt
    if source.read_bytes() != before:
        receipt.update(outcome="instrument_failure", reason="source_changed_during_probe")
        return 2, receipt
    receipt.update(outcome="passed")
    return 0, receipt


def main() -> int:
    try:
        code, receipt = execute(Path(__file__).resolve().parents[2])
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        code, receipt = 2, {"outcome": "instrument_failure", "reason": str(error)}
    print(json.dumps(receipt, indent=2, ensure_ascii=False))
    return code


if __name__ == "__main__":
    sys.exit(main())
