"""Source inventory helpers for the perl-parser facade authority check."""
from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

FORBIDDEN_KERNEL_DEPENDENCY_TOKENS = (
    "perl-lsp", "perllsp", "vscode", "perl-dap", "perl-workspace"
)

@dataclass(frozen=True)
class CargoTarget:
    kind: str
    name: str
    required_features: tuple[str, ...]


def load_toml(path: Path) -> dict[str, Any]:
    try:
        value = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ValueError(f"cannot read {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a TOML table")
    return value


def split_top_level(value: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    start = 0
    for index, character in enumerate(value):
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth < 0:
                raise ValueError(f"unbalanced Rust use tree: {value}")
        elif character == "," and depth == 0:
            part = value[start:index].strip()
            if part:
                parts.append(part)
            start = index + 1
    if depth != 0:
        raise ValueError(f"unbalanced Rust use tree: {value}")
    tail = value[start:].strip()
    if tail:
        parts.append(tail)
    return parts


def expand_use_tree(value: str) -> list[str]:
    value = re.sub(r"\s+", " ", value.strip())
    brace_index = value.find("{")
    if brace_index < 0:
        return [value]
    close_index = value.rfind("}")
    if close_index < brace_index or value[close_index + 1 :].strip():
        raise ValueError(f"unsupported Rust use tree: {value}")
    prefix = value[:brace_index].rstrip().removesuffix("::")
    result: list[str] = []
    for child in split_top_level(value[brace_index + 1 : close_index]):
        result.extend(expand_use_tree(f"{prefix}::{child}" if prefix else child))
    return result


def public_module_names(source: str) -> set[str]:
    return set(re.findall(
        r"(?m)^pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?=;|\{)", source
    ))


def rust_public_surface(path: Path) -> tuple[set[str], set[str]]:
    source = path.read_text(encoding="utf-8").split("\n#[cfg(test)]\nmod tests", 1)[0]
    modules = public_module_names(source)
    exports: set[str] = set()
    for match in re.finditer(r"(?ms)^pub use\s+(.+?);", source):
        exports.update(expand_use_tree(match.group(1)))
    return modules, exports


def incremental_surface(path: Path) -> tuple[set[str], set[str], set[str]]:
    source = path.read_text(encoding="utf-8")
    modules = public_module_names(source)
    exports: set[str] = set()
    for match in re.finditer(r"(?ms)^pub use\s+(.+?);", source):
        exports.update(expand_use_tree(match.group(1)))
    functions = set(re.findall(r"(?m)^pub fn ([A-Za-z_][A-Za-z0-9_]*)\s*\(", source))
    return modules, exports, functions


def feature_names(manifest: dict[str, Any]) -> set[str]:
    features = manifest.get("features")
    if not isinstance(features, dict):
        raise ValueError("perl-parser manifest has no [features] table")
    return set(features)


def default_features(manifest: dict[str, Any]) -> tuple[str, ...]:
    value = manifest["features"].get("default")
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        raise ValueError("perl-parser default feature list is invalid")
    return tuple(value)


def normalized_dependency_rows(table: dict[str, Any], context: str) -> dict[str, bool]:
    result: dict[str, bool] = {}
    for alias, value in table.items():
        if isinstance(value, str):
            package = alias
            optional = False
        elif isinstance(value, dict):
            package = value.get("package", alias)
            optional = value.get("optional") is True
        else:
            raise ValueError(f"{context}.{alias} dependency entry is invalid")
        if not isinstance(package, str) or not package.strip():
            raise ValueError(f"{context}.{alias}.package must be a non-empty string")
        if package in result:
            raise ValueError(f"{context} contains duplicate package identity: {package}")
        result[package] = optional
    return result


def dependency_rows(manifest: dict[str, Any]) -> dict[str, bool]:
    dependencies = manifest.get("dependencies")
    if not isinstance(dependencies, dict):
        raise ValueError("manifest has no [dependencies] table")
    return normalized_dependency_rows(dependencies, "dependencies")


def cargo_targets(manifest: dict[str, Any]) -> set[CargoTarget]:
    result: set[CargoTarget] = set()
    for key in ("bin", "bench", "example"):
        values = manifest.get(key, [])
        if not isinstance(values, list):
            raise ValueError(f"perl-parser [[{key}]] entries are invalid")
        for value in values:
            if not isinstance(value, dict) or not isinstance(value.get("name"), str):
                raise ValueError(f"perl-parser [[{key}]] entry is invalid")
            required = value.get("required-features", [])
            if not isinstance(required, list) or any(not isinstance(item, str) for item in required):
                raise ValueError(f"perl-parser [[{key}]] required-features are invalid")
            result.add(CargoTarget(key, value["name"], tuple(required)))
    return result


def dependency_tables(manifest: dict[str, Any]) -> Iterable[dict[str, Any]]:
    for key in ("dependencies", "dev-dependencies", "build-dependencies"):
        value = manifest.get(key)
        if isinstance(value, dict):
            yield value
    target = manifest.get("target")
    if isinstance(target, dict):
        for target_table in target.values():
            if not isinstance(target_table, dict):
                continue
            for key in ("dependencies", "dev-dependencies", "build-dependencies"):
                value = target_table.get(key)
                if isinstance(value, dict):
                    yield value


def discover_consumers(root: Path, facade_manifest: Path) -> set[str]:
    result: set[str] = set()
    for manifest_path in sorted(root.rglob("Cargo.toml")):
        if manifest_path == facade_manifest:
            continue
        try:
            manifest = load_toml(manifest_path)
        except ValueError:
            continue
        if any(
            "perl-parser" in normalized_dependency_rows(table, str(manifest_path))
            for table in dependency_tables(manifest)
        ):
            result.add(manifest_path.relative_to(root).as_posix())
    return result
