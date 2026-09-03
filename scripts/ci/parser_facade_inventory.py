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

CFG_TEST_OUT_OF_LINE_PATTERN = re.compile(
    r"#\[cfg\s*\(\s*test\s*\)\]\s*"
    r"(?:(?:#\[[^\]]*\]\s*)*)"
    r"(?:pub\s+(?:\([^)]*\)\s*)?)?mod\s+"
    r"([A-Za-z_][A-Za-z0-9_]*)\s*;"
)

CFG_TEST_MODULE_PATTERN = re.compile(
    r"#\[cfg\s*\(\s*test\s*\)\]\s*"
    r"(?:(?:#\[[^\]]*\]\s*)*)"
    r"(?:pub\s+(?:\([^)]*\)\s*)?)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{"
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


def implicit_feature_names(manifest: dict[str, Any]) -> set[str]:
    features = manifest.get("features", {})
    explicit_dependency_features = {
        entry.removeprefix("dep:")
        for values in features.values()
        if isinstance(values, list)
        for entry in values
        if isinstance(entry, str) and entry.startswith("dep:")
    }
    return {
        alias
        for _, table in contextual_dependency_tables(manifest)
        for alias, value in table.items()
        if isinstance(value, dict)
        and value.get("optional") is True
        and alias not in features
        and alias not in explicit_dependency_features
    }


def feature_names(manifest: dict[str, Any]) -> set[str]:
    features = manifest.get("features")
    if not isinstance(features, dict):
        raise ValueError("perl-parser manifest has no [features] table")
    names = set(features)
    return names | implicit_feature_names(manifest)


def default_features(manifest: dict[str, Any]) -> tuple[str, ...]:
    value = manifest["features"].get("default")
    if not isinstance(value, list) or any(not isinstance(item, str) for item in value):
        raise ValueError("perl-parser default feature list is invalid")
    return tuple(value)


def normalized_dependency_rows(
    table: dict[str, Any], context: str, workspace_packages: dict[str, str] | None = None
) -> dict[str, bool]:
    result: dict[str, bool] = {}
    for alias, value in table.items():
        if isinstance(value, str):
            package = alias
            optional = False
        elif isinstance(value, dict):
            package = value.get("package", alias)
            if value.get("workspace") is True and workspace_packages is not None:
                package = workspace_packages.get(alias, package)
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
    def matching_brace(start: int) -> int:
        depth = 1
        cursor = start
        while cursor < len(source):
            character = source[cursor]
            if source.startswith("//", cursor):
                newline = source.find("\n", cursor + 2)
                cursor = len(source) if newline < 0 else newline + 1
                continue
            if source.startswith("/*", cursor):
                cursor += 2
                comment_depth = 1
                while cursor < len(source) and comment_depth:
                    if source.startswith("/*", cursor):
                        comment_depth += 1
                        cursor += 2
                    elif source.startswith("*/", cursor):
                        comment_depth -= 1
                        cursor += 2
                    else:
                        cursor += 1
                continue
            if character == "r":
                marker = cursor + 1
                while marker < len(source) and source[marker] == "#":
                    marker += 1
                if marker < len(source) and source[marker] == '"':
                    hashes = marker - cursor - 1
                    terminator = '"' + ("#" * hashes)
                    end = source.find(terminator, marker + 1)
                    cursor = len(source) if end < 0 else end + len(terminator)
                    continue
            if character == '"':
                cursor += 1
                while cursor < len(source):
                    if source[cursor] == "\\":
                        cursor += 2
                    elif source[cursor] == '"':
                        cursor += 1
                        break
                    else:
                        cursor += 1
                continue
            if character == "'":
                # Lifetimes also use an apostrophe, so recognize the compact
                # one-codepoint forms we need to protect rather than scanning to
                # an arbitrary later apostrophe. This covers braces and escaped
                # braces without hiding syntax after a lifetime such as `'a`.
                if cursor + 2 < len(source) and source[cursor + 2] == "'":
                    cursor += 3
                    continue
                if (
                    cursor + 3 < len(source)
                    and source[cursor + 1] == "\\"
                    and source[cursor + 3] == "'"
                ):
                    cursor += 4
                    continue
            if character == "{":
                depth += 1
            elif character == "}":
                depth -= 1
                if depth == 0:
                    return cursor + 1
            cursor += 1
        return len(source)

    parts: list[str] = []
    index = 0
    masked = mask_rust_non_code(source)
    while True:
        match = CFG_TEST_MODULE_PATTERN.search(masked, index)
        if match is None:
            parts.append(source[index:])
            return "".join(parts)
        parts.append(source[index : match.start()])
        index = matching_brace(match.end())


def mask_rust_non_code(source: str) -> str:
    """Blank comments and literals while preserving code layout.

    Inventory regexes must not treat text in comments or string literals as
    source predicates. Newlines are retained so diagnostics and module paths
    remain stable.
    """
    chars = list(source)

    def blank(start: int, end: int) -> None:
        for position in range(start, min(end, len(chars))):
            if chars[position] != "\n":
                chars[position] = " "

    cursor = 0
    while cursor < len(source):
        if source.startswith("//", cursor):
            end = source.find("\n", cursor + 2)
            end = len(source) if end < 0 else end
            blank(cursor, end)
            cursor = end
            continue
        if source.startswith("/*", cursor):
            end = cursor + 2
            depth = 1
            while end < len(source) and depth:
                if source.startswith("/*", end):
                    depth += 1
                    end += 2
                elif source.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            blank(cursor, end)
            cursor = end
            continue
        if source[cursor] == "r":
            marker = cursor + 1
            while marker < len(source) and source[marker] == "#":
                marker += 1
            if marker < len(source) and source[marker] == '"':
                hashes = marker - cursor - 1
                terminator = '"' + ("#" * hashes)
                close = source.find(terminator, marker + 1)
                end = len(source) if close < 0 else close + len(terminator)
                preserve = re.search(r"feature\s*=\s*$", source[max(0, cursor - 40) : cursor])
                if not preserve:
                    blank(cursor, end)
                cursor = end
                continue
        if source[cursor] == '"':
            end = cursor + 1
            while end < len(source):
                if source[end] == "\\":
                    end += 2
                elif source[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
            preserve = re.search(r"feature\s*=\s*$", source[max(0, cursor - 40) : cursor])
            if not preserve:
                blank(cursor, end)
            cursor = end
            continue
        if source[cursor] == "'" and cursor + 2 < len(source):
            if source[cursor + 2] == "'":
                blank(cursor, cursor + 3)
                cursor += 3
                continue
            if cursor + 3 < len(source) and source[cursor + 1] == "\\" and source[cursor + 3] == "'":
                blank(cursor, cursor + 4)
                cursor += 4
                continue
        cursor += 1
    return "".join(chars)


def cfg_test_module_paths(path: Path, source: str) -> set[Path]:
    """Return out-of-line module files hidden by `#[cfg(test)]`."""
    masked = mask_rust_non_code(source)
    paths: set[Path] = set()
    # Rust places children of `foo.rs` below the sibling `foo/` directory,
    # while children of `foo/mod.rs` remain beside that module file.
    module_base = (
        path.parent
        if path.name in {"lib.rs", "main.rs", "mod.rs"}
        else path.with_suffix("")
    )
    for match in CFG_TEST_OUT_OF_LINE_PATTERN.finditer(masked):
        name = match.group(1)
        # `#[path = "..."]` changes the module file location. Recover only
        # an attribute immediately adjacent to this declaration; a broad
        # lookback can incorrectly borrow a path from an unrelated module.
        raw_prefix = source[: match.start()]
        masked_prefix = masked[: match.start()]
        raw_lines = raw_prefix.splitlines(keepends=True)
        masked_lines = masked_prefix.splitlines(keepends=True)
        attributes: list[str] = []
        bracket_depth = 0
        for raw_line, masked_line in zip(reversed(raw_lines), reversed(masked_lines)):
            stripped = masked_line.strip()
            if bracket_depth:
                attributes.append(raw_line)
                bracket_depth += masked_line.count("[") - masked_line.count("]")
                continue
            if not stripped:
                attributes.append(raw_line)
                continue
            if stripped.startswith("]"):
                # A multiline attribute may close on its own line; retain it
                # while walking back to the opening `#[...` line.
                attributes.append(raw_line)
                continue
            if stripped.startswith("#"):
                attributes.append(raw_line)
                bracket_depth += masked_line.count("[") - masked_line.count("]")
                continue
            break
        path_match = re.findall(
            r"#\[\s*path\s*=\s*\"([^\"]+)\"\s*\]",
            "".join(reversed(attributes)),
            flags=re.DOTALL,
        )
        if path_match:
            # An explicit path is relative to the declaring source file,
            # including when that source is a non-mod.rs module.
            module = path.parent / path_match[-1]
            paths.add(module)
            paths.add(module.parent / module.stem / "mod.rs")
        else:
            paths.add(module_base / f"{name}.rs")
            paths.add(module_base / name / "mod.rs")
    return paths


def feature_source_gates(
    crate_root: Path, directories: Iterable[str], skip_test_modules: bool = False
) -> set[str]:
    """Collect every feature name reached by a `feature = "..."` cfg predicate.

    Attribution is file-wise across the given directories. A gate inside a module
    that is itself feature-gated still counts for its own feature, which
    over-states isolation rather than hiding a gate.
    """
    gates: set[str] = set()
    excluded: set[Path] = set()
    excluded_roots: set[Path] = set()
    if skip_test_modules:
        for directory in directories:
            base = crate_root / directory
            if base.is_dir():
                for path in base.rglob("*.rs"):
                    source = path.read_text(encoding="utf-8", errors="replace")
                    excluded.update(cfg_test_module_paths(path, source))
        # An out-of-line module owns a complete filesystem subtree. Excluding
        # only `tests.rs`/`tests/mod.rs` still lets `tests/helper.rs` leak into
        # the production denominator.
        for module_path in excluded:
            excluded_roots.add(
                module_path.parent
                if module_path.name == "mod.rs"
                else module_path.with_suffix("")
            )
    for directory in directories:
        base = crate_root / directory
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.rs")):
            if path in excluded or any(
                path == root or root in path.parents for root in excluded_roots
            ):
                continue
            source = path.read_text(encoding="utf-8", errors="replace")
            if skip_test_modules:
                source = strip_cfg_test_modules(source)
            gates.update(FEATURE_GATE_PATTERN.findall(mask_rust_non_code(source)))
    return gates


PRODUCTION_TARGET_KINDS = {"bin"}


def target_required_features(
    manifest: dict[str, Any], kinds: set[str], package_root: Path
) -> set[str]:
    """Features that gate whether a Cargo target of the given kinds is built at all.

    A binary is a shipped deliverable, so gating one is a production boundary. A
    bench or example is a development surface, matching how `benches/` and
    `examples/` source is treated.
    """
    return {
        feature
        for target in cargo_targets(manifest)
        if target.kind in kinds
        for feature in target.required_features
    }


def dependency_aliases(manifest: dict[str, Any]) -> set[str]:
    """Manifest keys for dependencies, which differ from package identity when renamed."""
    return {
        alias
        for _, table in contextual_dependency_tables(manifest)
        for alias in table
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
    implicit = implicit_feature_names(manifest)
    features = {**features, **{name: [name] for name in implicit}}
    declared = set(features)
    # Feature entries name the manifest key, which differs from package identity
    # when a dependency is renamed, so both forms must resolve.
    packages = set(dependency_universe(manifest)) | dependency_aliases(manifest)
    gated = feature_source_gates(
        crate_root, FEATURE_PRODUCTION_DIRECTORIES, skip_test_modules=True
    )
    # A gate reachable only inside a `#[cfg(test)]` module is a test profile.
    test_gated = feature_source_gates(crate_root, FEATURE_TEST_DIRECTORIES)
    test_gated |= feature_source_gates(crate_root, FEATURE_PRODUCTION_DIRECTORIES) - gated
    target_gated = target_required_features(manifest, PRODUCTION_TARGET_KINDS, crate_root)
    test_gated |= target_required_features(
        manifest,
        {target.kind for target in cargo_targets(manifest, crate_root)} - PRODUCTION_TARGET_KINDS,
        crate_root,
    )

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


def cargo_targets(manifest: dict[str, Any], package_root: Path | None = None) -> set[CargoTarget]:
    """Inventory explicit and conventional Cargo targets.

    Explicit target stanzas win over conventional discovery at the same path.  The
    package-level ``auto*`` switches are honored, so a disabled family contributes no
    implicit targets while explicit stanzas remain visible.
    """
    result: set[CargoTarget] = set()
    explicit_paths: set[Path] = set()
    explicit_names: set[tuple[str, str]] = set()
    discovered_paths: dict[tuple[str, str], Path] = {}
    package = manifest.get("package", {})
    if not isinstance(package, dict):
        raise ValueError("perl-parser [package] table is invalid")
    package_name = package.get("name")
    if not isinstance(package_name, str) or not package_name:
        raise ValueError("perl-parser package name is invalid")

    # Cargo's default target path is not just the flat `<root>/<name>.rs` form.
    # Keep every conventional spelling suppressed when an explicit target has the
    # same name; otherwise the inventory can report a duplicate target that Cargo
    # treats as one explicit declaration.
    def default_paths(kind: str, name: str) -> tuple[Path, ...]:
        root = {"bin": "src/bin", "bench": "benches", "example": "examples", "test": "tests"}[kind]
        if kind == "bin" and name == package_name:
            return (Path("src/main.rs"), Path(root) / f"{name}.rs", Path(root) / name / "main.rs")
        return (Path(root) / f"{name}.rs", Path(root) / name / "main.rs")
    for key in ("bin", "bench", "example", "test"):
        values = manifest.get(key, [])
        if not isinstance(values, list):
            raise ValueError(f"perl-parser [[{key}]] entries are invalid")
        for value in values:
            if not isinstance(value, dict) or not isinstance(value.get("name"), str):
                raise ValueError(f"perl-parser [[{key}]] entry is invalid")
            required = value.get("required-features", [])
            if not isinstance(required, list) or any(not isinstance(item, str) for item in required):
                raise ValueError(f"perl-parser [[{key}]] required-features are invalid")
            identity = (key, value["name"])
            if identity in explicit_names:
                raise ValueError(f"duplicate Cargo target name: {key}:{value['name']}")
            explicit_names.add(identity)
            result.add(CargoTarget(key, value["name"], tuple(required)))
            if package_root is not None:
                explicit = value.get("path")
                if explicit is not None and (not isinstance(explicit, str) or not explicit):
                    raise ValueError(f"perl-parser [[{key}]].path is invalid")
                paths = (Path(explicit),) if explicit is not None else default_paths(key, value["name"])
                explicit_paths.update(paths)
    if package_root is None:
        return result

    auto_enabled: dict[str, bool] = {}
    for kind, option in (("bin", "autobins"), ("bench", "autobenches"),
                         ("example", "autoexamples"), ("test", "autotests")):
        value = package.get(option, True)
        if not isinstance(value, bool):
            raise ValueError(f"perl-parser package {option} must be a boolean")
        auto_enabled[kind] = value
    roots = {"bench": "benches", "example": "examples", "test": "tests"}
    if auto_enabled["bin"]:
        candidates = []
        main = package_root / "src/main.rs"
        if main.is_file():
            candidates.append((Path("src/main.rs"), package_name))
        candidates.extend(
            (path.relative_to(package_root), path.stem)
            for path in (package_root / "src/bin").glob("*.rs")
            if path.is_file()
        )
        candidates.extend(
            (path.relative_to(package_root), path.parent.name)
            for path in (package_root / "src/bin").glob("*/main.rs")
            if path.is_file()
        )
    else:
        candidates = []
    for path, name in candidates:
        if not isinstance(name, str):
            continue
        identity = ("bin", name)
        if path in explicit_paths:
            continue
        if identity in explicit_names:
            raise ValueError(f"duplicate Cargo target name: bin:{name}")
        previous = discovered_paths.get(identity)
        if previous is not None:
            raise ValueError(
                f"duplicate Cargo target name: bin:{name} ({previous} and {path})"
            )
        discovered_paths[identity] = path
        result.add(CargoTarget("bin", name, ()))
    for kind, directory in roots.items():
        if not auto_enabled[kind]:
            continue
        root = package_root / directory
        if not root.is_dir():
            continue
        # Cargo discovers only `<root>/*.rs` and `<root>/*/main.rs`.  A recursive
        # walk would mistake Rust modules nested below a conventional target for
        # independent targets (for example examples/foo/helpers.rs).
        for path in root.glob("*.rs"):
            if path.is_file():
                relative = path.relative_to(package_root)
                name = path.stem
                identity = (kind, name)
                if relative in explicit_paths:
                    continue
                if identity in explicit_names:
                    raise ValueError(f"duplicate Cargo target name: {kind}:{name}")
                previous = discovered_paths.get(identity)
                if previous is not None:
                    raise ValueError(
                        f"duplicate Cargo target name: {kind}:{name} ({previous} and {relative})"
                    )
                discovered_paths[identity] = relative
                result.add(CargoTarget(kind, name, ()))
        for path in root.glob("*/main.rs"):
            if path.is_file():
                relative = path.relative_to(package_root)
                name = path.parent.name
                identity = (kind, name)
                if relative in explicit_paths:
                    continue
                if identity in explicit_names:
                    raise ValueError(f"duplicate Cargo target name: {kind}:{name}")
                previous = discovered_paths.get(identity)
                if previous is not None:
                    raise ValueError(
                        f"duplicate Cargo target name: {kind}:{name} ({previous} and {relative})"
                    )
                discovered_paths[identity] = relative
                result.add(CargoTarget(kind, name, ()))
    return result


def discover_consumer_contexts(root: Path, facade_manifest: Path) -> dict[str, tuple[str, ...]]:
    """Map each workspace consumer of the facade to the contexts that reach it.

    A crate that only test-depends on the facade is not evidence for keeping a
    production surface, so the reaching context is retained rather than collapsed.
    """
    result: dict[str, tuple[str, ...]] = {}
    workspace_path = root / "Cargo.toml"
    workspace = load_toml(workspace_path) if workspace_path.is_file() else {}
    workspace_dependencies = workspace.get("workspace", {}).get("dependencies", {})
    if not isinstance(workspace_dependencies, dict):
        workspace_dependencies = {}
    workspace_packages = {
        alias: value.get("package", alias)
        for alias, value in workspace_dependencies.items()
        if isinstance(value, dict) and isinstance(value.get("package", alias), str)
    }
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
            if "perl-parser" in normalized_dependency_rows(
                table, str(manifest_path), workspace_packages
            )
        }
        if contexts:
            result[manifest_path.relative_to(root).as_posix()] = tuple(sorted(contexts))
    return result


def discover_consumers(root: Path, facade_manifest: Path) -> set[str]:
    return set(discover_consumer_contexts(root, facade_manifest))
