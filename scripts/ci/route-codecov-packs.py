#!/usr/bin/env python3
"""Emit a lightweight coverage-pack route for the Codecov workflow."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath
from typing import NamedTuple

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - CI uses Python 3.11+.
    print("tomllib is required; use Python 3.11 or newer", file=sys.stderr)
    raise


FALLBACK_PACK_ID = "patch-coverage-rust-focused"
NON_LCOV_SKIP_REASON = "non-LCOV CI policy/routing surface; covered by focused CI gates"
NON_SOURCE_LCOV_SKIP_REASON = "LCOV coverage pack matched only non-source files; covered by focused CI gates"
TEST_SUPPORT_CRATE_PREFIXES = (
    "crates/perl-lsp-ux-tests/",
    "crates/perl-tdd-support/",
    "crates/perl-test-generators/",
    "crates/perl-test-must/",
)
FEATURE_CFG_RE = re.compile(r'feature\s*=\s*"([^"]+)"')
# scripts/ci/route-codecov-packs.py -> repository root.  Resolving from the
# script keeps manifest and source lookups stable when the workflow runs the
# router from a directory other than the checkout root.
REPO_ROOT = Path(__file__).resolve().parents[2]


class BinaryTestTarget(NamedTuple):
    """One Cargo binary target that participates in `cargo test`."""

    name: str
    required_features: tuple[str, ...] = ()


class PackageTestTargets(NamedTuple):
    """Local test targets needed to instrument one changed package."""

    package_name: str
    has_lib: bool
    binaries: tuple[BinaryTestTarget, ...]


def crate_name_from_source_path(path: str) -> str | None:
    """Extract the crate directory name from a `crates/<name>/src/...` path."""
    if not path.startswith("crates/"):
        return None
    rest = path[len("crates/"):]
    slash = rest.find("/")
    if slash == -1:
        return None
    return rest[:slash]


def changed_crates(paths: list[str]) -> list[str]:
    """Return unique crate directory names owning changed LCOV source files."""
    seen: set[str] = set()
    result: list[str] = []
    for path in paths:
        if is_lcov_source_path(path):
            name = crate_name_from_source_path(path)
            if name and name not in seen:
                seen.add(name)
                result.append(name)
    return result


def changed_integration_test_targets(
    paths: list[str],
    repo_root: Path = REPO_ROOT,
) -> dict[str, list[tuple[str, tuple[str, ...]]]]:
    """Return changed top-level integration test targets by crate directory."""
    result: dict[str, list[tuple[str, tuple[str, ...]]]] = {}
    seen: set[tuple[str, str]] = set()
    for path in paths:
        parts = path.split("/")
        if len(parts) != 4 or parts[0] != "crates" or parts[2] != "tests":
            continue
        filename = parts[3]
        if not filename.endswith(".rs"):
            continue
        crate_name = parts[1]
        target = Path(filename).stem
        key = (crate_name, target)
        if key in seen:
            continue
        seen.add(key)
        result.setdefault(crate_name, []).append((target, tuple(required_features_for_test(path, repo_root))))
    return result


def required_features_for_test(path: str, repo_root: Path = REPO_ROOT) -> list[str]:
    """Return crate features gated by a changed integration test target."""
    test_path = repo_root / path
    if not test_path.exists():
        return []
    try:
        source = test_path.read_text(encoding="utf-8")
    except OSError:
        return []
    features: set[str] = set()
    for line in source.splitlines():
        stripped = line.strip()
        if not stripped.startswith("#![cfg("):
            continue
        features.update(FEATURE_CFG_RE.findall(stripped))
    return sorted(features)


def _target_features(target: dict[str, object]) -> tuple[str, ...]:
    raw_features = target.get("required-features") or []
    if not isinstance(raw_features, list) or not all(
        isinstance(feature, str) and feature for feature in raw_features
    ):
        raise ValueError("Cargo target required-features must be a list of non-empty strings")
    return tuple(sorted(set(raw_features)))


def _target_name_from_path(path_value: str, package_name: str) -> str:
    path = PurePosixPath(path_value.replace("\\", "/"))
    if path.name == "main.rs":
        if path.parent.name == "src":
            return package_name
        # A bare "main.rs" has no parent directory to name the target after.
        return path.parent.name or package_name
    return path.stem or package_name


def _lib_source_path(explicit_lib: dict[str, object], crate_name: str) -> PurePosixPath:
    """Return the effective library source path for occupancy accounting."""
    raw_path = explicit_lib.get("path")
    if raw_path is not None:
        if not isinstance(raw_path, str) or not raw_path:
            raise ValueError(f"Cargo lib path for changed crate {crate_name} must be a non-empty string")
        return PurePosixPath(raw_path.replace("\\", "/"))
    return PurePosixPath("src/lib.rs")


def _explicit_bin_source_path(
    name: str,
    raw_path: object,
    package_name: str,
    crate_root: Path,
    crate_name: str,
) -> PurePosixPath | None:
    """Resolve the source file a `[[bin]]` target occupies.

    Cargo infers a pathless binary's source from its name: `src/main.rs` when
    the target is named after the package, otherwise `src/bin/<name>.rs` or
    `src/bin/<name>/main.rs`.  Resolving it here is what lets autobin discovery
    skip files an explicit target already claims, instead of inventing a second
    target for the same source.
    """
    if raw_path is not None:
        if not isinstance(raw_path, str) or not raw_path:
            raise ValueError(f"Cargo bin path for changed crate {crate_name} must be a non-empty string")
        return PurePosixPath(raw_path.replace("\\", "/"))
    candidates: list[PurePosixPath] = []
    if name == package_name:
        candidates.append(PurePosixPath("src/main.rs"))
    candidates.append(PurePosixPath(f"src/bin/{name}.rs"))
    candidates.append(PurePosixPath(f"src/bin/{name}/main.rs"))
    for candidate in candidates:
        if (crate_root / candidate).is_file():
            return candidate
    # Cargo itself rejects a manifest whose binary has no discoverable source.
    # Report no occupancy rather than guessing a path that does not exist.
    return None


def _autobins_default(package: dict[str, object], manifest: dict[str, object]) -> bool:
    """Return Cargo's edition-aware autobin default for one package.

    A 2015-edition package that declares any target manually turns target
    auto-discovery off; 2018 and later keep it on.  Registering an
    auto-discovered `--bin` that Cargo does not have is a hard command failure,
    so honour the edition rather than assuming discovery is always active.
    """
    explicit = package.get("autobins")
    if explicit is not None:
        return explicit is not False
    edition = package.get("edition")
    if edition == "2015":
        has_manual_target = any(
            manifest.get(key) for key in ("lib", "bin", "example", "test", "bench")
        )
        return not has_manual_target
    return True


def package_test_targets(crate_name: str, repo_root: Path = REPO_ROOT) -> PackageTestTargets:
    """Derive testable lib/bin targets for one local Cargo package.

    Cargo's fallback route used to assume every changed package had a library
    and no ordinary binary unit tests.  Read the local manifest and source
    layout instead, including explicit targets and Cargo's autobin conventions.
    Targets are de-duplicated by the source file they occupy, so an explicit
    `[[bin]]` pointing into `src/bin/` does not also surface under its file name.
    """
    crate_root = repo_root / "crates" / crate_name
    manifest_path = crate_root / "Cargo.toml"
    try:
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ValueError(f"cannot read Cargo manifest for changed crate {crate_name}: {error}") from error
    except tomllib.TOMLDecodeError as error:
        raise ValueError(f"invalid Cargo manifest for changed crate {crate_name}: {error}") from error

    package = manifest.get("package")
    if not isinstance(package, dict):
        raise ValueError(f"Cargo manifest for changed crate {crate_name} has no [package] table")
    package_name = package.get("name")
    if not isinstance(package_name, str) or not package_name:
        raise ValueError(f"Cargo manifest for changed crate {crate_name} has no package name")

    explicit_lib = manifest.get("lib")
    if explicit_lib is not None and not isinstance(explicit_lib, dict):
        raise ValueError(f"Cargo manifest for changed crate {crate_name} has an invalid [lib] table")
    occupied: set[PurePosixPath] = set()
    if isinstance(explicit_lib, dict):
        lib_path = _lib_source_path(explicit_lib, crate_name)
        occupied.add(lib_path)
        has_lib = explicit_lib.get("test") is not False and (crate_root / lib_path).is_file()
    else:
        autolib = package.get("autolib", True) is not False
        has_lib = autolib and (crate_root / "src" / "lib.rs").is_file()
        if has_lib:
            occupied.add(PurePosixPath("src/lib.rs"))

    binaries: dict[str, BinaryTestTarget] = {}
    explicit_bins = manifest.get("bin") or []
    if not isinstance(explicit_bins, list):
        raise ValueError(f"Cargo manifest for changed crate {crate_name} has invalid [[bin]] entries")
    for target in explicit_bins:
        if not isinstance(target, dict):
            raise ValueError(f"Cargo manifest for changed crate {crate_name} has an invalid [[bin]] row")
        raw_name = target.get("name")
        raw_path = target.get("path")
        if raw_name is None:
            if raw_path is not None and not isinstance(raw_path, str):
                raise ValueError(f"Cargo bin path for changed crate {crate_name} must be a string")
            name = _target_name_from_path(raw_path or "src/main.rs", package_name)
        elif isinstance(raw_name, str) and raw_name:
            name = raw_name
        else:
            raise ValueError(f"Cargo bin name for changed crate {crate_name} must be non-empty")
        source = _explicit_bin_source_path(name, raw_path, package_name, crate_root, crate_name)
        if source is not None:
            occupied.add(source)
        if target.get("test") is False:
            continue
        if name in binaries:
            raise ValueError(f"Cargo manifest for changed crate {crate_name} repeats bin target {name}")
        binaries[name] = BinaryTestTarget(name, _target_features(target))

    if _autobins_default(package, manifest):
        src_root = crate_root / "src"
        discovered: list[tuple[str, PurePosixPath]] = []
        if (src_root / "main.rs").is_file():
            discovered.append((package_name, PurePosixPath("src/main.rs")))
        bin_root = src_root / "bin"
        if bin_root.is_dir():
            for entry in sorted(bin_root.iterdir(), key=lambda path: path.name):
                if entry.is_file() and entry.suffix == ".rs":
                    discovered.append((entry.stem, PurePosixPath(f"src/bin/{entry.name}")))
                elif entry.is_dir() and (entry / "main.rs").is_file():
                    discovered.append((entry.name, PurePosixPath(f"src/bin/{entry.name}/main.rs")))
        for name, source in discovered:
            if source in occupied or name in binaries:
                continue
            occupied.add(source)
            binaries[name] = BinaryTestTarget(name)

    return PackageTestTargets(
        package_name=package_name,
        has_lib=has_lib,
        binaries=tuple(sorted(binaries.values(), key=lambda target: target.name)),
    )


def binary_target_command(package_name: str, target: BinaryTestTarget) -> str:
    feature_arg = (
        f" --features {','.join(target.required_features)}" if target.required_features else ""
    )
    return (
        f"cargo llvm-cov test --no-report -p {package_name}{feature_arg} "
        f"--bin {target.name} --profile agent --locked"
    )


def augment_rust_focused_commands(
    base_commands: list[str],
    paths: list[str],
    repo_root: Path = REPO_ROOT,
) -> list[str]:
    """Append per-package unit/integration coverage commands to the fallback pack.

    The fallback pack is intentionally changed-package scoped.  Workspace-wide
    coverage is too expensive for Patch 95 and can turn a focused Rust change
    into a timeout before a coverage receipt is produced.  Package targets are
    derived from Cargo manifests so library-only, binary-only, and dual-target
    packages register the unit-test binaries that actually own changed source.

    DAP-style crates prove patch coverage through integration tests in
    ``tests/``.  Root cause (#1282): plain ``cargo test`` does not register the
    binary with cargo-llvm-cov's tracking file.  Every selected target therefore
    uses ``cargo llvm-cov test --no-report`` and defers LCOV generation to the
    single ``cargo llvm-cov report`` call.

    ``-- --test-threads=1`` forces serial execution within integration-test
    binaries because several workspace tests mutate global/process state.

    IMPORTANT: these commands are executed NON-FATALLY by
    ``generate-coverage-pack-commands.py``.  Assertion failures do not abort the
    coverage lane; the instrumented binary still writes LLVM coverage data.  The
    quality-gate verdict is the patch coverage number, not test pass/fail.
    """
    commands: list[str] = []
    for cmd in base_commands:
        if _is_deprecated_rust_focused_command(cmd):
            continue
        if cmd not in commands:
            commands.append(cmd)
    test_targets_by_crate = changed_integration_test_targets(paths, repo_root)
    for crate_name in changed_crates(paths):
        targets = package_test_targets(crate_name, repo_root)
        if targets.has_lib:
            lib_cmd = (
                f"cargo llvm-cov test --no-report -p {targets.package_name} "
                "--lib --profile agent --locked"
            )
            if lib_cmd not in commands:
                commands.append(lib_cmd)
        for binary in targets.binaries:
            command = binary_target_command(targets.package_name, binary)
            if command not in commands:
                commands.append(command)
        test_targets = test_targets_by_crate.get(crate_name, [])
        if test_targets:
            integration_cmds = [
                targeted_test_command(targets.package_name, target, features)
                for target, features in test_targets
            ]
        else:
            integration_cmds = [
                f"cargo llvm-cov test --no-report -p {targets.package_name} "
                "--tests --profile agent --locked -- --test-threads=1"
            ]
        for cmd in integration_cmds:
            if cmd not in commands:
                commands.append(cmd)
    if has_xtask_source_change(paths):
        cmd = "cargo llvm-cov test --no-report -p xtask --bin xtask --profile agent --locked"
        if cmd not in commands:
            commands.append(cmd)
    return commands


def _is_deprecated_rust_focused_command(cmd: str) -> bool:
    """Drop pre-#1529 broad commands if an older manifest is used."""
    return cmd.startswith("cargo llvm-cov test --no-report --workspace --lib ") or cmd.startswith(
        "cargo check --workspace "
    )


def has_xtask_source_change(paths: list[str]) -> bool:
    """Return whether the fallback pack owns an xtask source change."""
    return any(is_lcov_source_path(path) and path.startswith("xtask/src/") for path in paths)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Route changed files to Codecov coverage proof packs."
    )
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--manifest", default=".ci/coverage-packs.toml")
    parser.add_argument("--receipt", required=True)
    parser.add_argument("--summary", required=True)
    return parser.parse_args()


def changed_files(base: str, head: str) -> list[str]:
    output = subprocess.check_output(
        ["git", "diff", "--name-only", f"{base}...{head}"],
        text=True,
    )
    return [line.strip().replace("\\", "/") for line in output.splitlines() if line.strip()]


def matches_pattern(path: str, pattern: str) -> bool:
    pattern = pattern.replace("\\", "/")
    if pattern.startswith("*."):
        return path.endswith(pattern[1:])
    if pattern.endswith("/"):
        return path.startswith(pattern)
    return path == pattern or path.startswith(pattern)


def pack_matches(pack: dict[str, object], paths: list[str]) -> bool:
    patterns = pack.get("files") or []
    if not isinstance(patterns, list):
        return False
    return any(
        isinstance(pattern, str) and matches_pattern(path, pattern)
        for path in paths
        for pattern in patterns
    )


def is_lcov_source_path(path: str) -> bool:
    if not path.endswith(".rs"):
        return False
    if path.startswith("xtask/tests/") or "/tests/" in path:
        return False
    if is_test_support_crate_path(path):
        return False
    return path.startswith("xtask/src/") or path.startswith("crates/")


def is_test_support_crate_path(path: str) -> bool:
    normalized = path.replace("\\", "/")
    return any(normalized.startswith(prefix) for prefix in TEST_SUPPORT_CRATE_PREFIXES)


def targeted_test_command(crate_name: str, target: str, features: tuple[str, ...]) -> str:
    feature_arg = f" --features {','.join(features)}" if features else ""
    return (
        f"cargo llvm-cov test --no-report -p {crate_name}{feature_arg} "
        f"--test {target} --profile agent --locked -- --test-threads=1"
    )


def pack_matches_lcov_source(pack: dict[str, object], paths: list[str]) -> bool:
    patterns = pack.get("files") or []
    if not isinstance(patterns, list):
        return False
    return any(
        is_lcov_source_path(path)
        and isinstance(pattern, str)
        and matches_pattern(path, pattern)
        for path in paths
        for pattern in patterns
    )


def is_lcov_pack(pack: dict[str, object]) -> bool:
    return pack.get("lcov") is not False


def non_lcov_matches(packs: list[dict[str, object]], paths: list[str]) -> list[dict[str, object]]:
    return [
        pack
        for pack in packs
        if pack.get("id") != FALLBACK_PACK_ID
        and not is_lcov_pack(pack)
        and pack_matches(pack, paths)
    ]


def lcov_matches_without_source(
    packs: list[dict[str, object]], paths: list[str]
) -> list[dict[str, object]]:
    return [
        pack
        for pack in packs
        if pack.get("id") != FALLBACK_PACK_ID
        and is_lcov_pack(pack)
        and pack_matches(pack, paths)
        and not pack_matches_lcov_source(pack, paths)
    ]


def selected_packs(packs: list[dict[str, object]], paths: list[str]) -> list[dict[str, object]]:
    fallback = next((pack for pack in packs if pack.get("id") == FALLBACK_PACK_ID), None)
    selected = [
        pack
        for pack in packs
        if pack.get("id") != FALLBACK_PACK_ID
        and is_lcov_pack(pack)
        and pack_matches(pack, paths)
        and pack_matches_lcov_source(pack, paths)
    ]
    non_lcov_selected = non_lcov_matches(packs, paths)
    selected_needs_fallback = fallback is not None and any(
        is_lcov_source_path(path)
        and not any(pack_matches(pack, [path]) for pack in selected)
        and not any(pack_matches(pack, [path]) for pack in non_lcov_selected)
        for path in paths
    )
    if selected_needs_fallback:
        selected.append(fallback)
    if selected:
        return selected
    if non_lcov_matches(packs, paths):
        return []
    return []


def normalize_pack(
    pack: dict[str, object],
    paths: list[str] | None = None,
    repo_root: Path = REPO_ROOT,
) -> dict[str, object]:
    commands: list[str] = list(pack.get("commands") or [])
    if pack.get("id") == FALLBACK_PACK_ID and paths is not None:
        commands = augment_rust_focused_commands(commands, paths, repo_root)
    return {
        "id": str(pack.get("id", "")),
        "files": list(pack.get("files") or []),
        "commands": commands,
        "coverage_filters": list(pack.get("coverage_filters") or []),
    }


def write_summary(path: Path, receipt: dict[str, object]) -> None:
    packs = receipt["coverage_proof_packs"]
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write("# Changed-File Coverage Route\n\n")
        handle.write(f"- base: `{receipt['base']}`\n")
        handle.write(f"- head: `{receipt['head']}`\n")
        handle.write(f"- changed files: `{len(receipt['changed_files'])}`\n")
        if packs:
            handle.write("- coverage proof packs:\n")
            for pack in packs:
                handle.write(f"  - `{pack['id']}`\n")
        else:
            handle.write("- coverage proof packs: skipped-by-policy\n")
            skipped = receipt.get("skipped_by_policy") or {}
            if skipped:
                handle.write("- skipped proof packs:\n")
                for pack_id, reason in skipped.items():
                    handle.write(f"  - `{pack_id}`: {reason}\n")


def main() -> int:
    args = parse_args()
    manifest = tomllib.loads(Path(args.manifest).read_text(encoding="utf-8"))
    packs = [pack for pack in manifest.get("pack", []) if isinstance(pack, dict)]
    paths = changed_files(args.base, args.head)
    coverage_packs = [normalize_pack(pack, paths) for pack in selected_packs(packs, paths)]
    coverage_pack_ids = [pack["id"] for pack in coverage_packs]
    skipped_by_policy = {
        str(pack.get("id", "")): NON_LCOV_SKIP_REASON for pack in non_lcov_matches(packs, paths)
    }
    skipped_by_policy.update(
        {
            str(pack.get("id", "")): NON_SOURCE_LCOV_SKIP_REASON
            for pack in lcov_matches_without_source(packs, paths)
        }
    )
    receipt = {
        "schema_version": "ci_route.v1",
        "provider_action": "changed_file_proof_routing",
        "claim_boundary": (
            "Advisory lightweight Codecov coverage-pack route; selected packs "
            "feed manual routed coverage diagnostics"
        ),
        "base": args.base,
        "head": args.head,
        "changed_files": paths,
        "changed_surfaces": coverage_pack_ids,
        "required_proof_packs": [],
        "skipped_by_policy": skipped_by_policy,
        "coverage_pack_selector": coverage_pack_ids,
        "coverage_proof_packs": coverage_packs,
        "estimated_lem": 1,
    }
    receipt_path = Path(args.receipt)
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    write_summary(Path(args.summary), receipt)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
