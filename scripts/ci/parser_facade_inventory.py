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

DEPENDENCY_TABLE_CONTEXTS = {
    "dependencies": "normal",
    "dev-dependencies": "dev",
    "build-dependencies": "build",
}

IGNORED_MANIFEST_DIRECTORIES = {"target", ".git", "node_modules"}

FEATURE_PRODUCTION_DIRECTORIES = ("src",)
FEATURE_TEST_DIRECTORIES = ("tests", "benches", "examples")

FEATURE_GATE_PATTERN = re.compile(r"feature\s*=\s*\"([A-Za-z0-9_.+-]+)\"")

CFG_TEST_MODULE_PATTERN = re.compile(
    r"#\[cfg\(test\)\]\s*(?:pub\s+(?:\([^)]*\)\s*)?)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{"
)


@dataclass(frozen=True)
class CargoTarget:
    kind: str
    name: str
    required_features: tuple[str, ...]


@dataclass(frozen=True)
class DependencyFact:
    """One package identity observed across every manifest dependency table."""

    contexts: tuple[str, ...]
    optional: bool


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


def contextual_dependency_tables(manifest: dict[str, Any]) -> Iterable[tuple[str, dict[str, Any]]]:
    """Yield every dependency table with its normal/dev/build/target context."""
    for key, context in DEPENDENCY_TABLE_CONTEXTS.items():
        value = manifest.get(key)
        if isinstance(value, dict):
            yield context, value
    target = manifest.get("target")
    if isinstance(target, dict):
        for specification in sorted(target):
            target_table = target[specification]
            if not isinstance(target_table, dict):
                continue
            for key, context in DEPENDENCY_TABLE_CONTEXTS.items():
                value = target_table.get(key)
                if isinstance(value, dict):
                    yield f"target({specification}):{context}", value


def dependency_universe(manifest: dict[str, Any]) -> dict[str, DependencyFact]:
    """Inventory every declared dependency with the contexts that declare it.

    Optionality is not merged across contexts: a package declared optional in one
    table and required in another is a shape this schema cannot represent honestly,
    so it is rejected rather than collapsed to a single flag.
    """
    contexts_by_package: dict[str, set[str]] = {}
    optional_by_package: dict[str, dict[str, bool]] = {}
    for context, table in contextual_dependency_tables(manifest):
        for package, optional in normalized_dependency_rows(table, context).items():
            contexts_by_package.setdefault(package, set()).add(context)
            optional_by_package.setdefault(package, {})[context] = optional
    facts: dict[str, DependencyFact] = {}
    for package, contexts in contexts_by_package.items():
        flags = optional_by_package[package]
        if len(set(flags.values())) > 1:
            optional_in = sorted(c for c, v in flags.items() if v)
            required_in = sorted(c for c, v in flags.items() if not v)
            raise ValueError(
                f"dependency {package} has mixed optionality: optional in "
                f"{','.join(optional_in)} but required in {','.join(required_in)}"
            )
        facts[package] = DependencyFact(tuple(sorted(contexts)), next(iter(flags.values())))
    if not facts:
        raise ValueError("manifest declares no dependencies in any context")
    return facts


def strip_cfg_test_modules(source: str) -> str:
    """Remove `#[cfg(test)] mod ... { ... }` blocks from Rust source.

    A feature gated only inside a test module gates test code, not production
    code, so `src/` alone is not a sound production proxy without this.
    """
    parts: list[str] = []
    index = 0
    while True:
        match = CFG_TEST_MODULE_PATTERN.search(source, index)
        if match is None:
            parts.append(source[index:])
            return "".join(parts)
        parts.append(source[index : match.start()])
        depth = 1
        cursor = match.end()
        while cursor < len(source) and depth:
            character = source[cursor]
            if character == "{":
                depth += 1
            elif character == "}":
                depth -= 1
            cursor += 1
        index = cursor


def feature_source_gates(
    crate_root: Path, directories: Iterable[str], skip_test_modules: bool = False
) -> set[str]:
    """Collect every feature name reached by a `feature = "..."` cfg predicate."""
    gates: set[str] = set()
    for directory in directories:
        base = crate_root / directory
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            source = path.read_text(encoding="utf-8", errors="replace")
            if skip_test_modules:
                source = strip_cfg_test_modules(source)
            gates.update(FEATURE_GATE_PATTERN.findall(source))
    return gates


def target_required_features(manifest: dict[str, Any]) -> set[str]:
    """Features that gate whether a Cargo bin/bench/example target is built at all."""
    return {
        feature
        for target in cargo_targets(manifest)
        for feature in target.required_features
    }


def feature_isolation(manifest: dict[str, Any], crate_root: Path) -> dict[str, str]:
    """Classify what each declared feature actually isolates.

    A feature name is a production boundary only when it selects dependencies,
    gates `src/`, or gates whether a Cargo target is built at all through
    `required-features`. A feature that gates only test, bench, or example source is
    a test profile, and a feature that gates nothing is taxonomy; neither may be
    presented as an architectural boundary.
    """
    features = manifest.get("features")
    if not isinstance(features, dict):
        raise ValueError("perl-parser manifest has no [features] table")
    declared = set(features)
    packages = set(dependency_universe(manifest))
    gated = feature_source_gates(
        crate_root, FEATURE_PRODUCTION_DIRECTORIES, skip_test_modules=True
    )
    test_gated = feature_source_gates(crate_root, FEATURE_TEST_DIRECTORIES)
    test_gated |= feature_source_gates(
        crate_root, FEATURE_PRODUCTION_DIRECTORIES
    ) - gated
    target_gated = target_required_features(manifest)

    def closure(name: str) -> set[str]:
        """Every feature enabled by `name`, including itself.

        A feature that enables another inherits its effects: `default` controls
        production whenever anything it turns on does.
        """
        seen: set[str] = set()
        pending = [name]
        while pending:
            current = pending.pop()
            if current in seen:
                continue
            seen.add(current)
            for entry in features.get(current, []):
                if isinstance(entry, str) and entry in declared:
                    pending.append(entry)
        return seen
    for name, entries in features.items():
        if not isinstance(entries, list) or any(not isinstance(entry, str) for entry in entries):
            raise ValueError(f"feature {name} must select a string list")

    result: dict[str, str] = {}
    for name in features:
        reached = closure(name)
        selects_dependency = False
        for member in reached:
            for entry in features.get(member, []):
                if entry.startswith("dep:"):
                    selects_dependency = True
                elif "/" in entry:
                    if entry.split("/", 1)[0].removesuffix("?") in packages:
                        selects_dependency = True
                elif entry in packages:
                    selects_dependency = True
        gates_source = bool(reached & gated)
        gates_target = bool(reached & target_gated)
        gates_test = bool(reached & test_gated)
        enables_feature = reached != {name}
        if selects_dependency and gates_source:
            result[name] = "dependencies_and_source"
        elif selects_dependency:
            result[name] = "dependencies_only"
        elif gates_source:
            result[name] = "source_only"
        elif gates_target:
            result[name] = "target_only"
        elif gates_test:
            result[name] = "test_source_only"
        elif enables_feature:
            result[name] = "feature_aggregate"
        else:
            result[name] = "taxonomy_only"
    return result


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


def discover_consumer_contexts(root: Path, facade_manifest: Path) -> dict[str, tuple[str, ...]]:
    """Map each workspace consumer of the facade to the contexts that reach it.

    A crate that only test-depends on the facade is not evidence for keeping a
    production surface, so the reaching context is retained rather than collapsed.
    """
    result: dict[str, tuple[str, ...]] = {}
    for manifest_path in sorted(root.rglob("Cargo.toml")):
        if manifest_path == facade_manifest:
            continue
        # Build output is not a workspace consumer: `cargo package` writes
        # target/package/<crate>/Cargo.toml, which would otherwise appear as an
        # unclassified consumer and fail the check on a developer's tree.
        if set(manifest_path.relative_to(root).parts) & IGNORED_MANIFEST_DIRECTORIES:
            continue
        # A tracked manifest that cannot be parsed must not vanish from the
        # denominator: an unreadable consumer is unknown, never absent.
        manifest = load_toml(manifest_path)
        contexts = {
            context
            for context, table in contextual_dependency_tables(manifest)
            if "perl-parser" in normalized_dependency_rows(table, str(manifest_path))
        }
        if contexts:
            result[manifest_path.relative_to(root).as_posix()] = tuple(sorted(contexts))
    return result


def discover_consumers(root: Path, facade_manifest: Path) -> set[str]:
    return set(discover_consumer_contexts(root, facade_manifest))
