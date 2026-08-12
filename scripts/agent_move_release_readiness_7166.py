#!/usr/bin/env python3
"""Execute the repaired #7166 migration source from the last known scaffold commit."""

from __future__ import annotations

import subprocess
from pathlib import Path

SOURCE_COMMIT = "371c7db105d858e99bfea35f6e30283e1a5c7256"
SOURCE_PATH = "scripts/agent_move_release_readiness_7166.py"


def replace_once(text: str, old: str, new: str, *, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one occurrence, found {count}")
    return text.replace(old, new, 1)


def repaired_source() -> str:
    source = subprocess.run(
        ["git", "show", f"{SOURCE_COMMIT}:{SOURCE_PATH}"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout

    source = replace_once(
        source,
        '''    text = text.replace(
        "perl_kwalitee::PerlKwaliteeProfile",
        "release_readiness::ReleaseReadinessProfile",
    )
''',
        '''    text = text.replace(
        "perl_kwalitee::PerlKwaliteeProfile",
        "release_readiness::ReleaseReadinessProfile",
    )
    text = text.replace(
        "perl_release_readiness::PerlKwaliteeProfile",
        "release_readiness::ReleaseReadinessProfile",
    )
''',
        label="migrated profile namespace",
    )

    source = replace_once(
        source,
        '''    alias_body = match.group("body")
    canonical_body = alias_body.replace("perl_kwalitee::", "release_readiness::")
''',
        '''    migrated_body = match.group("body")
    canonical_body = migrated_body.replace(
        "perl_release_readiness::", "release_readiness::"
    )
    alias_body = migrated_body.replace(
        "perl_release_readiness::", "perl_kwalitee::"
    )
''',
        label="dispatch namespace routing",
    )

    source = replace_once(
        source,
        '''    if "enum PerlKwaliteeCommand" in main:
        raise RuntimeError("old xtask subcommand enum still exists")
''',
        '''    if "enum PerlKwaliteeCommand" in main:
        raise RuntimeError("old xtask subcommand enum still exists")
    if "perl_release_readiness::PerlKwaliteeProfile" in main:
        raise RuntimeError("library namespace leaked into the xtask profile surface")
    for stale_call in [
        "perl_release_readiness::check(",
        "perl_release_readiness::report(",
        "perl_release_readiness::explain(",
        "perl_release_readiness::default_json_path(",
        "perl_release_readiness::default_markdown_path(",
    ]:
        if stale_call in main:
            raise RuntimeError(f"library namespace leaked into xtask dispatch: {stale_call}")
    for alias_call in [
        "perl_kwalitee::check(",
        "perl_kwalitee::report(",
        "perl_kwalitee::explain(",
        "perl_kwalitee::default_json_path(",
        "perl_kwalitee::default_markdown_path(",
    ]:
        if main.count(alias_call) != 1:
            raise RuntimeError(f"compatibility alias dispatch missing or duplicated: {alias_call}")
''',
        label="xtask namespace verifier",
    )

    source = replace_once(
        source,
        '''    for path in ROOT.rglob("*.rs"):
        if path in {SELF}:
            continue
''',
        '''    for path in ROOT.rglob("*.rs"):
        if path in {SELF, ROOT / "xtask/src/main.rs"}:
            continue
''',
        label="intentional compatibility-module exemption",
    )

    return source


def main() -> None:
    source = repaired_source()
    namespace = {
        "__name__": "__main__",
        "__file__": str(Path(__file__).resolve()),
    }
    exec(compile(source, SOURCE_PATH, "exec"), namespace)


if __name__ == "__main__":
    main()
