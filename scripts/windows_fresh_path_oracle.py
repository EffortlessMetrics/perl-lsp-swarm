#!/usr/bin/env python3
"""Windows fresh-process PATH oracle for #7832.

Rebuilds PATH from Machine + User scopes and resolves a command the way a
fresh Windows process would after User-PATH persistence — without consulting
the caller's process PATH and without harness injection.

This is an installed-product *PATH rebuild* oracle, not a live-host installer
proof. Callers supply Machine/User PATH fixtures (or read live scopes on
Windows). Absolute-path launches and parent-process PATH edits cannot satisfy
the lookup.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path
from typing import Iterable, Optional


def join_machine_user_path(machine: Optional[str], user: Optional[str]) -> str:
    """Join Machine then User PATH the way Windows builds a fresh process PATH."""
    parts: list[str] = []
    for scope in (machine, user):
        if scope is None:
            continue
        trimmed = scope.strip().strip(";")
        if trimmed:
            parts.append(trimmed)
    return ";".join(parts)


def _path_entries(path_value: str) -> list[str]:
    entries: list[str] = []
    for raw in path_value.split(";"):
        entry = raw.strip().strip('"')
        if entry:
            entries.append(entry)
    return entries


def candidate_names(command: str, pathext: Optional[str]) -> list[str]:
    """Windows command lookup candidates (PATHEXT-aware).

    On case-sensitive hosts, include both the PATHEXT spelling and a lowercase
    variant so fixture proofs still exercise the Windows name surface.
    """
    names = [command]
    if os.name == "nt" or pathext is not None:
        ext_list = (pathext if pathext is not None else ".COM;.EXE;.BAT;.CMD").split(";")
        _stem, existing_ext = os.path.splitext(command)
        if existing_ext:
            return [command]
        for ext in ext_list:
            ext = ext.strip()
            if not ext:
                continue
            if not ext.startswith("."):
                ext = "." + ext
            names.append(f"{command}{ext}")
            lower = ext.lower()
            if lower != ext:
                names.append(f"{command}{lower}")
    else:
        # Cross-platform CI still proves the Windows name surface used by install.ps1.
        names.extend([f"{command}.exe", f"{command}.cmd", f"{command}.bat"])
    # Preserve order, drop duplicates. On Windows, PATH lookup is case-insensitive;
    # on Linux CI keep both PATHEXT and lowercase spellings so fixtures resolve.
    seen: set[str] = set()
    ordered: list[str] = []
    for name in names:
        key = name.lower() if os.name == "nt" else name
        if key in seen:
            continue
        seen.add(key)
        ordered.append(name)
    return ordered


def resolve_on_rebuilt_path(
    command: str,
    *,
    machine_path: Optional[str],
    user_path: Optional[str],
    pathext: Optional[str] = None,
) -> Optional[Path]:
    """Resolve *command* using only Machine+User rebuilt PATH."""
    rebuilt = join_machine_user_path(machine_path, user_path)
    names = candidate_names(command, pathext)
    for entry in _path_entries(rebuilt):
        directory = Path(entry)
        for name in names:
            candidate = directory / name
            try:
                if candidate.is_file():
                    return candidate.resolve()
            except OSError:
                continue
    return None


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_live_scope(scope: str) -> Optional[str]:
    """Read a live Windows environment scope when available."""
    if os.name != "nt":
        return None
    try:
        import winreg  # type: ignore
    except ImportError:
        return None

    root = winreg.HKEY_LOCAL_MACHINE if scope == "Machine" else winreg.HKEY_CURRENT_USER
    subkey = (
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"
        if scope == "Machine"
        else r"Environment"
    )
    try:
        with winreg.OpenKey(root, subkey) as key:
            value, _ = winreg.QueryValueEx(key, "Path")
            if isinstance(value, str):
                return value
    except OSError:
        return None
    return None


def build_receipt(
    *,
    command: str,
    machine_path: Optional[str],
    user_path: Optional[str],
    expected_path: Optional[Path],
    expected_sha256: Optional[str],
    process_path: str,
    pathext: Optional[str],
) -> dict:
    rebuilt = join_machine_user_path(machine_path, user_path)
    resolved = resolve_on_rebuilt_path(
        command,
        machine_path=machine_path,
        user_path=user_path,
        pathext=pathext,
    )

    process_pollution = False
    for entry in _path_entries(process_path):
        try:
            if Path(entry).resolve() == (
                expected_path.parent.resolve() if expected_path is not None else None
            ):
                process_pollution = True
                break
        except OSError:
            continue

    identity_ok = False
    resolved_sha: Optional[str] = None
    if resolved is not None:
        try:
            resolved_sha = file_sha256(resolved)
        except OSError:
            resolved_sha = None
        if expected_path is not None:
            try:
                identity_ok = resolved.resolve() == expected_path.resolve()
            except OSError:
                identity_ok = False
            if expected_sha256 is not None and resolved_sha is not None:
                identity_ok = identity_ok and resolved_sha == expected_sha256
        elif expected_sha256 is not None and resolved_sha is not None:
            identity_ok = resolved_sha == expected_sha256

    if resolved is None:
        result = "command_not_found_on_rebuilt_path"
    elif expected_path is not None or expected_sha256 is not None:
        result = "exact_identity_match" if identity_ok else "wrong_ambient_binary"
    else:
        result = "resolved"

    return {
        "oracle": "windows_fresh_path_rebuild",
        "issue": 7832,
        "command": command,
        "path_ownership": "fixture_or_live_machine_user",
        "persistence_action": "none_oracle_only",
        "session_requirement": "fresh_process_rebuilds_machine_plus_user",
        "rebuilt_path_entry_count": len(_path_entries(rebuilt)),
        "process_path_ignored": True,
        "process_path_contained_expected_dir": process_pollution,
        "fresh_process_lookup": command,
        "resolved_executable": str(resolved) if resolved is not None else None,
        "resolved_sha256": resolved_sha,
        "expected_executable": str(expected_path) if expected_path is not None else None,
        "expected_sha256": expected_sha256,
        "exact_identity_match": identity_ok if resolved is not None else False,
        "result": result,
        "manual_path_steps": 0 if result == "exact_identity_match" else 1,
    }


def run_self_test() -> int:
    """Discriminating fixture-scope self-test (spawned oracle, no live User PATH)."""
    import subprocess
    import tempfile

    failures = 0

    def check(name: str, ok: bool, detail: str = "") -> None:
        nonlocal failures
        if ok:
            print(f"PASS  {name}")
        else:
            failures += 1
            print(f"FAIL  {name}", file=sys.stderr)
            if detail:
                print(f"      {detail}", file=sys.stderr)

    with tempfile.TemporaryDirectory(prefix="plsp-path-oracle-") as tmp:
        root = Path(tmp)
        install_dir = root / "install-bin"
        ambient_dir = root / "ambient-bin"
        harness_dir = root / "harness-only-bin"
        unrelated_dir = root / "unrelated-bin"
        for directory in (install_dir, ambient_dir, harness_dir, unrelated_dir):
            directory.mkdir()
        (install_dir / "perllsp.exe").write_text("exact-installed-subject-v2\n", encoding="utf-8")
        (ambient_dir / "perllsp.exe").write_text("older-ambient-subject-v1\n", encoding="utf-8")
        (harness_dir / "perllsp.exe").write_text("harness-injected-subject\n", encoding="utf-8")
        expected = (install_dir / "perllsp.exe").resolve()
        expected_sha = file_sha256(expected)
        script = str(Path(__file__).resolve())

        def spawn(machine: str, user: str, polluted: str) -> tuple[int, dict]:
            env = {
                "PATH": polluted,
                "SYSTEMROOT": os.environ.get("SYSTEMROOT", r"C:\Windows"),
                "WINDIR": os.environ.get("WINDIR", r"C:\Windows"),
            }
            completed = subprocess.run(
                [
                    sys.executable,
                    script,
                    "--command",
                    "perllsp",
                    "--machine-path",
                    machine,
                    "--user-path",
                    user,
                    "--expected",
                    str(expected),
                    "--expected-sha256",
                    expected_sha,
                    "--pathext",
                    ".COM;.EXE;.BAT;.CMD",
                    "--require-exact",
                ],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
            try:
                receipt = json.loads(completed.stdout or "{}")
            except json.JSONDecodeError:
                receipt = {"parse_error": completed.stdout, "stderr": completed.stderr}
            return completed.returncode, receipt

        rc, receipt = spawn(str(unrelated_dir), str(install_dir), f"{harness_dir}{os.pathsep}/usr/bin")
        check(
            "user-path subject wins over harness-only process PATH",
            rc == 0
            and receipt.get("result") == "exact_identity_match"
            and receipt.get("process_path_ignored") is True
            and receipt.get("process_path_contained_expected_dir") is False
            and receipt.get("resolved_sha256") == expected_sha,
            json.dumps(receipt),
        )

        rc, receipt = spawn(str(unrelated_dir), str(root / "empty-user"), f"{install_dir}{os.pathsep}/usr/bin")
        check(
            "harness process PATH injection cannot satisfy fresh-process identity",
            rc != 0
            and receipt.get("result") == "command_not_found_on_rebuilt_path"
            and receipt.get("exact_identity_match") is False,
            json.dumps(receipt),
        )

        rc, receipt = spawn(str(ambient_dir), str(install_dir), f"/usr/bin")
        check(
            "older Machine PATH ambient subject fails exact-identity receipt",
            rc != 0
            and receipt.get("result") == "wrong_ambient_binary"
            and "ambient-bin" in str(receipt.get("resolved_executable") or ""),
            json.dumps(receipt),
        )

        rc, receipt = spawn("", "", f"{install_dir}{os.pathsep}/usr/bin")
        check(
            "empty Machine+User scopes yield no fresh-process resolution",
            rc != 0
            and receipt.get("result") == "command_not_found_on_rebuilt_path"
            and receipt.get("rebuilt_path_entry_count") == 0,
            json.dumps(receipt),
        )

    print(f"\nFresh-process PATH oracle: self-test failures={failures}")
    return 1 if failures else 0


def parse_args(argv: Optional[Iterable[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Resolve a command from a Machine+User rebuilt PATH (no process PATH)."
    )
    parser.add_argument("--command", default="perllsp", help="Command name to resolve")
    parser.add_argument(
        "--machine-path",
        default=None,
        help="Fixture Machine PATH. Omit to read live Machine PATH on Windows.",
    )
    parser.add_argument(
        "--user-path",
        default=None,
        help="Fixture User PATH. Omit to read live User PATH on Windows.",
    )
    parser.add_argument(
        "--expected",
        default=None,
        help="Absolute path of the exact installed subject (identity check).",
    )
    parser.add_argument(
        "--expected-sha256",
        default=None,
        help="Expected SHA-256 of the installed subject.",
    )
    parser.add_argument(
        "--pathext",
        default=None,
        help="Optional PATHEXT override (defaults to Windows-like list).",
    )
    parser.add_argument(
        "--require-exact",
        action="store_true",
        help="Exit non-zero unless the resolved file matches --expected/--expected-sha256.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run discriminating fixture-scope spawn tests and exit.",
    )
    return parser.parse_args(list(argv) if argv is not None else None)


def main(argv: Optional[Iterable[str]] = None) -> int:
    args = parse_args(argv)
    if args.self_test:
        return run_self_test()

    machine = args.machine_path
    user = args.user_path
    if machine is None:
        machine = read_live_scope("Machine")
    if user is None:
        user = read_live_scope("User")

    expected = Path(args.expected).resolve() if args.expected else None
    receipt = build_receipt(
        command=args.command,
        machine_path=machine,
        user_path=user,
        expected_path=expected,
        expected_sha256=args.expected_sha256,
        process_path=os.environ.get("PATH", ""),
        pathext=args.pathext,
    )
    json.dump(receipt, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")

    if args.require_exact:
        return 0 if receipt["result"] == "exact_identity_match" else 1
    if receipt["resolved_executable"] is None:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
