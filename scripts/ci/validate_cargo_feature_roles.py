#!/usr/bin/env python3
"""Validate the Cargo feature role registry (#8409).

This is an offline source-contract validator.  It discovers every Cargo
feature declared by a workspace member, joins the discovered consumer
evidence, and reconciles both against ``policy/cargo-feature-roles.toml``.

The registry classifies feature ROLE and records consumer evidence.  It is
deliberately not a second authority for supported build combinations
(``#3790``) or for product capability maturity and advertisement
(``#6731``); a row that tried to carry those propositions is rejected by the
strict key check.

Discovery is host-independent: it reads manifests and ``.rs`` sources under
the repository root, sorts every collection, and never invokes Cargo, Git or
a network.
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

REGISTRY = Path("policy/cargo-feature-roles.toml")
POLICY_NAME = "cargo-feature-roles"
SCHEMA_VERSION = 1

# Primary roles from #8409.  Hard-coded here so that adding a role to the
# registry file cannot by itself widen the accepted vocabulary.
ROLES = (
    "build_composition",
    "product_profile",
    "experimental_opt_in",
    "test_only",
    "legacy_alias",
    "aspirational_milestone",
)

# Observed consumption signals, independent of a feature's role. A feature can
# carry several at once, so `consumers` is the exact sorted set rather than one
# winner: collapsing to a single value would hide the gain or loss of every
# signal but the highest-priority one, which is most of the drift worth
# catching. An empty list means nothing consumes the feature at all.
CONSUMER_SIGNALS = (
    "cfg_gated",
    "composition",
    "propagated",
    "required_features",
)

# Roles that must never be reachable from a crate's own `default` feature.
# Enabling a test group, an experiment, or an unimplemented milestone by
# default is the misclassification #8121 exists to prevent.
DEFAULT_RESTRICTED_ROLES = frozenset(
    {"experimental_opt_in", "test_only", "aspirational_milestone"}
)

# Roles whose rows must state where the feature is going.
MIGRATION_REQUIRED_ROLES = frozenset(
    {"test_only", "legacy_alias", "aspirational_milestone"}
)

# Cargo's implicit default feature.  It is registered like any other feature
# but is exempt from the "unconsumed rows need a migration" rule: an empty
# default set is a legitimate, terminal build-composition statement.
DEFAULT_FEATURE = "default"

REQUIRED_ROW_KEYS = frozenset({"crate", "name", "role", "owner", "consumers"})
OPTIONAL_ROW_KEYS = frozenset({"migration", "name_exception", "note"})
ALLOWED_ROW_KEYS = REQUIRED_ROW_KEYS | OPTIONAL_ROW_KEYS

OWNER_RE = re.compile(r"^#\d+$")

# A consumer occurrence is a `feature = "NAME"` predicate lexically inside a
# cfg form.  Nested predicates (`cfg(all(not(windows), feature = "x"))`) and
# `cfg_attr` both occur in this workspace, so the spans are found by paren
# matching rather than by a regex that cannot balance parentheses.
CFG_TOKEN_RE = re.compile(r"(?<![A-Za-z0-9_])cfg(?:_attr)?!?\s*\(")
RAW_STRING_RE = re.compile(r"b?r(?P<hashes>#*)\"")


def scan_lexical(text: str) -> tuple[str, list[tuple[int, int]]]:
    """Blank out comments and report string-literal ranges.

    Returns `(blanked, string_ranges)` where `blanked` has every comment
    replaced by spaces (preserving offsets, so ranges stay comparable) and
    `string_ranges` covers each string/char literal body including its
    delimiters. Commented-out or quoted `cfg(...)` text is not real
    consumption, so counting it would let a stale `cfg_gated` row survive
    after the last real gate is deleted.
    """
    out = list(text)
    strings: list[tuple[int, int]] = []
    index = 0
    length = len(text)
    while index < length:
        char = text[index]
        # Line comment
        if char == "/" and index + 1 < length and text[index + 1] == "/":
            end = text.find("\n", index)
            end = length if end == -1 else end
            for position in range(index, end):
                out[position] = " "
            index = end
            continue
        # Block comment (Rust nests them)
        if char == "/" and index + 1 < length and text[index + 1] == "*":
            depth = 1
            cursor = index + 2
            while cursor < length and depth:
                if text.startswith("/*", cursor):
                    depth += 1
                    cursor += 2
                elif text.startswith("*/", cursor):
                    depth -= 1
                    cursor += 2
                else:
                    cursor += 1
            for position in range(index, cursor):
                if out[position] != "\n":
                    out[position] = " "
            index = cursor
            continue
        # Raw string: r"..." / r#"..."# / br#"..."#
        raw = RAW_STRING_RE.match(text, index)
        if raw and not (index and (text[index - 1].isalnum() or text[index - 1] == "_")):
            hashes = raw.group("hashes")
            terminator = '"' + hashes
            end = text.find(terminator, raw.end())
            end = length if end == -1 else end + len(terminator)
            strings.append((index, end))
            index = end
            continue
        # Ordinary string
        if char == '"':
            cursor = index + 1
            while cursor < length:
                if text[cursor] == "\\":
                    cursor += 2
                    continue
                if text[cursor] == '"':
                    cursor += 1
                    break
                cursor += 1
            strings.append((index, cursor))
            index = cursor
            continue
        # Char literal, distinguished from a lifetime (`'a`)
        if char == "'":
            if index + 1 < length and text[index + 1] == "\\":
                end = text.find("'", index + 2)
                if end != -1:
                    strings.append((index, end + 1))
                    index = end + 1
                    continue
            elif index + 2 < length and text[index + 2] == "'":
                strings.append((index, index + 3))
                index += 3
                continue
            index += 1
            continue
        index += 1
    return "".join(out), strings


def in_ranges(position: int, ranges: list[tuple[int, int]]) -> bool:
    return any(start <= position < end for start, end in ranges)


def cfg_spans(text: str, strings: list[tuple[int, int]] | None = None) -> list[tuple[int, int]]:
    """Return (start, end) offsets of every top-level cfg predicate body."""
    strings = strings or []
    spans: list[tuple[int, int]] = []
    for match in CFG_TOKEN_RE.finditer(text):
        if in_ranges(match.start(), strings):
            continue  # `cfg(` inside a string literal is data, not a gate
        start = match.end()
        if spans and start < spans[-1][1]:
            continue  # already covered by an enclosing cfg form
        depth = 1
        index = start
        while index < len(text) and depth:
            char = text[index]
            # A parenthesis inside a literal is text, not structure:
            # `cfg_attr(doc = "close ) here", feature = "x")` would otherwise
            # close the span early and lose the feature entirely.
            if in_ranges(index, strings):
                index += 1
                continue
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
            index += 1
        if depth == 0:
            spans.append((start, index - 1))
    return spans


# Names that assert test or support status rather than build composition.
TEST_IMPLYING_RE = re.compile(r"test|stress|repro|doc-coverage")
EXPERIMENT_IMPLYING_RE = re.compile(r"experimental")
# Names that assert roadmap position rather than composition.
ROADMAP_IMPLYING_RE = re.compile(r"phase\d|-v\d$|refactor")


class ValidationError(ValueError):
    """Raised when the validator cannot establish its bounded contract."""


@dataclass(frozen=True)
class FeatureFacts:
    """Discovered facts about one (crate, feature) pair."""

    crate: str
    name: str
    manifest: str
    kind: str  # "explicit" | "implicit_optional_dep"
    edges: tuple[str, ...]
    cfg_uses: int
    inbound_refs: tuple[str, ...]
    required_by_targets: tuple[str, ...]
    in_default_closure: bool

    @property
    def key(self) -> tuple[str, str]:
        return (self.crate, self.name)

    def observed_signals(self) -> tuple[str, ...]:
        """Every way this feature is currently consumed, sorted."""
        signals = set()
        if self.cfg_uses > 0:
            signals.add("cfg_gated")
        if self.edges:
            signals.add("composition")
        if self.inbound_refs:
            signals.add("propagated")
        if self.required_by_targets:
            signals.add("required_features")
        return tuple(sorted(signals))


@dataclass
class Registry:
    schema_version: int
    policy: str
    roles: tuple[str, ...]
    consumer_signals: tuple[str, ...]
    authority: dict[str, str]
    rows: list[dict[str, object]] = field(default_factory=list)


def member_dirs(root: Path) -> list[Path]:
    """Resolve workspace member directories from the root manifest."""
    manifest = root / "Cargo.toml"
    try:
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    except OSError as error:
        raise ValidationError(f"cannot read {manifest}: {error}") from error
    except tomllib.TOMLDecodeError as error:
        raise ValidationError(f"cannot parse {manifest}: {error}") from error
    workspace = data.get("workspace")
    if not isinstance(workspace, dict):
        raise ValidationError("root Cargo.toml declares no [workspace]")
    members = workspace.get("members")
    if not isinstance(members, list) or not members:
        raise ValidationError("root Cargo.toml declares no workspace members")
    # `[workspace].exclude` removes a directory from GLOB expansion. It does not
    # override a path listed literally in `members`: Cargo treats an explicit
    # member as a member. Applying it to literal entries would silently drop a
    # crate that really is in the workspace, which is the worse error here.
    excluded: set[Path] = set()
    for pattern in workspace.get("exclude") or []:
        if not isinstance(pattern, str):
            raise ValidationError(f"non-string workspace exclude: {pattern!r}")
        if any(char in pattern for char in "*?["):
            excluded.update(root.glob(pattern))
        else:
            excluded.add(root / pattern)
    excluded = {path.resolve() for path in excluded}

    resolved: set[Path] = set()
    for pattern in members:
        if not isinstance(pattern, str):
            raise ValidationError(f"non-string workspace member: {pattern!r}")
        # `Path.glob` rejects patterns with no parts (".") and needlessly walks
        # for literal paths, so resolve non-glob members directly.
        is_glob = any(char in pattern for char in "*?[")
        candidates = root.glob(pattern) if is_glob else iter([root / pattern])
        for path in candidates:
            if is_glob and path.resolve() in excluded:
                continue
            if (path / "Cargo.toml").is_file():
                resolved.add(path)
    if not resolved:
        raise ValidationError("no workspace member manifests resolved")
    return sorted(resolved)


def optional_dependencies(manifest: dict) -> set[str]:
    """Every optional dependency that can create an implicit Cargo feature.

    Cargo creates an implicit feature for an optional dependency in
    `[dependencies]`, `[build-dependencies]`, and either of those under a
    `[target.<cfg>]` table. `[dev-dependencies]` are excluded because Cargo
    rejects an optional dev-dependency outright ("dev-dependencies are not
    allowed to be optional"), so treating one as a feature would invent a
    feature that cannot exist.
    """
    tables: list[dict] = []

    def collect(table: object) -> None:
        if isinstance(table, dict):
            tables.append(table)

    collect(manifest.get("dependencies"))
    collect(manifest.get("build-dependencies"))
    targets = manifest.get("target")
    if isinstance(targets, dict):
        for target in targets.values():
            if isinstance(target, dict):
                collect(target.get("dependencies"))
                collect(target.get("build-dependencies"))

    return {
        name
        for table in tables
        for name, spec in table.items()
        if isinstance(spec, dict) and spec.get("optional") is True
    }


CARGO_TARGET_TABLES = ("lib", "bin", "test", "example", "bench")


def required_feature_targets(manifest: dict) -> dict[str, list[str]]:
    """Map each feature to the Cargo targets whose `required-features` list it.

    A binary, test, example, or bench that declares
    `required-features = ["cli"]` is a real consumer: the feature selects
    whether that target builds at all. Ignoring it made `perl-parser/cli`
    look unconsumed even though `[[bin]] perl-parse` requires it.
    """
    consumers: dict[str, list[str]] = {}
    for table_name in CARGO_TARGET_TABLES:
        table = manifest.get(table_name)
        entries = table if isinstance(table, list) else [table]
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            required = entry.get("required-features")
            if not isinstance(required, list):
                continue
            label = entry.get("name")
            label = f"{table_name}:{label}" if isinstance(label, str) else table_name
            for feature in required:
                if isinstance(feature, str):
                    consumers.setdefault(feature, []).append(label)
    return {feature: sorted(set(labels)) for feature, labels in consumers.items()}


def default_closure(features: dict[str, list[str]]) -> set[str]:
    """Intra-crate transitive closure of the crate's own `default` feature.

    Cross-crate propagation (`default = ["dep/feature"]`) is deliberately out
    of scope: supported build combinations across crates are #3790's subject,
    not this registry's.
    """
    if DEFAULT_FEATURE not in features:
        return set()
    seen: set[str] = set()
    stack = [DEFAULT_FEATURE]
    while stack:
        current = stack.pop()
        for edge in features.get(current, []):
            if edge.startswith("dep:") or "/" in edge:
                continue
            if edge not in seen:
                seen.add(edge)
                stack.append(edge)
    return seen


ANY_FEATURE_RE = re.compile(r"feature\s*=\s*\"([A-Za-z0-9_.+-]+)\"")


def count_cfg_uses_in_source(text: str) -> dict[str, int]:
    """Count every `feature = "NAME"` predicate inside a real cfg form.

    Comments are blanked and quoted `cfg(...)` text is skipped first, so only
    predicates the compiler actually sees are counted. The feature name's own
    quotes are a string literal by definition, so the `feature` keyword — not
    the quoted name — is what must lie outside a string.
    """
    blanked, strings = scan_lexical(text)
    return count_cfg_uses_in_scanned(blanked, strings)


def count_cfg_uses_in_scanned(
    blanked: str, strings: list[tuple[int, int]]
) -> dict[str, int]:
    """Count cfg-gated feature predicates in already-scanned source.

    Split from `count_cfg_uses_in_source` so a crate walk can reuse one lexical
    scan per file for both consumer counting and include-edge resolution.
    """
    counts: dict[str, int] = {}
    spans = cfg_spans(blanked, strings)
    if not spans:
        return counts
    for match in ANY_FEATURE_RE.finditer(blanked):
        if in_ranges(match.start(), strings):
            continue
        if any(start <= match.start() and match.end() <= end for start, end in spans):
            name = match.group(1)
            counts[name] = counts.get(name, 0) + 1
    return counts


# Directories that hold build output, tooling state, or test data rather than
# compiled crate source.
#
# `target/` can hold tens of thousands of generated `.rs` files on a warm
# runner, so scanning it would make the evidence depend on whether the tree had
# been built. Fixture directories matter for a different reason: Cargo compiles
# `src/**`, `build.rs`, and the *top-level* files of `tests/`, `benches/` and
# `examples/`, but never `tests/fixtures/**`. A `.rs` file there is data read by
# a test, not a gate the compiler sees — `crates/perl-lexer/tests/fixtures/`
# holds `#[cfg(feature = "simd")]` selectors that exist precisely to be scanned
# as text, and counting them credited `simd` with three consumers it does not
# have.
SKIPPED_DIRS = frozenset(
    {
        "target",
        ".git",
        "node_modules",
        ".cargo",
        "vendor",
        "fixtures",
        "testdata",
        "test_data",
        "snapshots",
    }
)


# A file named by a literal `include!("...")` or `#[path = "..."]` is compiled
# into the crate even when it lives under a directory the walk above skips, so
# the scan set has to be closed over those edges or the exclusion becomes a
# silent false negative. `crates/perl-lexer/src/lexer/helpers/cursor.rs`
# includes `tests/fixtures/ripr_seam_proof_peek_char_unit.inc` into production
# `src/`, which is exactly that case.
#
# Only literal string targets are followed. `include!(concat!(env!("OUT_DIR"),
# ...))` is deliberately not resolved: that names generated build output, which
# is the very thing skipping `target/` exists to keep out of the evidence.
INCLUDE_TARGET_RE = re.compile(r"include!\s*\(\s*\"([^\"\n]+)\"\s*\)")
PATH_ATTR_TARGET_RE = re.compile(r"#\s*\[\s*path\s*=\s*\"([^\"\n]+)\"\s*\]")


def included_targets(
    source: Path, blanked: str, strings: list[tuple[int, int]]
) -> list[Path]:
    """Files compiled into `source` by a literal include! or #[path]."""
    targets: list[Path] = []
    for pattern in (INCLUDE_TARGET_RE, PATH_ATTR_TARGET_RE):
        for match in pattern.finditer(blanked):
            # An `include!` sitting inside a string literal is inert text, the
            # same reason a quoted cfg predicate is not a consumer.
            if in_ranges(match.start(), strings):
                continue
            targets.append(source.parent / match.group(1))
    return targets


def crate_source_texts(
    crate_dir: Path,
) -> list[tuple[Path, str, list[tuple[int, int]]]]:
    """Compiled sources of a crate, lexically scanned, each file read once.

    Returns `(path, blanked, string_ranges)` per file so the single scan serves
    both consumer counting and include-edge resolution.
    """
    sources: list[Path] = []
    stack = [crate_dir]
    while stack:
        current = stack.pop()
        try:
            entries = sorted(current.iterdir())
        except OSError as error:
            # A source tree this validator cannot read is an instrument
            # failure, not an absence of consumers. Swallowing it would look
            # exactly like a feature losing its last cfg gate.
            raise ValidationError(f"cannot scan {current}: {error}") from error
        for entry in entries:
            if entry.is_symlink():
                continue
            if entry.is_dir():
                if entry.name in SKIPPED_DIRS or entry.name.startswith("."):
                    continue
                stack.append(entry)
            elif entry.suffix == ".rs":
                sources.append(entry)

    # Close over compiled include edges. Paths are resolved so a file reached
    # both by the walk and by an include! is scanned once, not counted twice.
    scanned: dict[Path, tuple[str, list[tuple[int, int]]]] = {}
    queue = sorted({source.resolve() for source in sources})
    while queue:
        current = queue.pop()
        if current in scanned:
            continue
        try:
            text = current.read_text(encoding="utf-8", errors="replace")
        except OSError as error:
            raise ValidationError(f"cannot read {current}: {error}") from error
        blanked, strings = scan_lexical(text)
        scanned[current] = (blanked, strings)
        for target in included_targets(current, blanked, strings):
            try:
                resolved = target.resolve()
            except OSError as error:
                raise ValidationError(f"cannot resolve {target}: {error}") from error
            if resolved in scanned or not resolved.is_file():
                continue
            queue.append(resolved)
    return sorted(
        (path, blanked, strings)
        for path, (blanked, strings) in scanned.items()
    )


def crate_sources(crate_dir: Path) -> list[Path]:
    """Rust sources belonging to a crate, excluding build output."""
    return [source for source, _blanked, _strings in crate_source_texts(crate_dir)]


def count_cfg_uses(crate_dir: Path) -> dict[str, int]:
    """Count cfg-gated feature predicates across one crate's Rust sources."""
    totals: dict[str, int] = {}
    for _source, blanked, strings in crate_source_texts(crate_dir):
        for name, count in count_cfg_uses_in_scanned(blanked, strings).items():
            totals[name] = totals.get(name, 0) + count
    return totals


def discover(root: Path) -> dict[tuple[str, str], FeatureFacts]:
    """Discover every governed (crate, feature) pair and its consumers."""
    manifests: dict[str, tuple[Path, dict]] = {}
    for member in member_dirs(root):
        manifest_path = member / "Cargo.toml"
        try:
            data = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as error:
            raise ValidationError(f"cannot parse {manifest_path}: {error}") from error
        package = data.get("package")
        if not isinstance(package, dict) or not isinstance(package.get("name"), str):
            raise ValidationError(f"{manifest_path} declares no package name")
        manifests[package["name"]] = (member, data)

    # Inbound references: which feature enables this (crate, feature)?
    inbound: dict[tuple[str, str], set[str]] = {}
    for crate, (_member, data) in manifests.items():
        features = data.get("features", {})
        if not isinstance(features, dict):
            raise ValidationError(f"{crate} declares a non-table [features]")
        for feature, edges in features.items():
            if not isinstance(edges, list):
                raise ValidationError(f"{crate}/{feature} declares non-list edges")
            for edge in edges:
                if not isinstance(edge, str) or edge.startswith("dep:"):
                    continue
                if "/" in edge:
                    dep, target = edge.split("/", 1)
                    dep = dep.rstrip("?")
                    target = target.lstrip("?")
                    if dep in manifests:
                        inbound.setdefault((dep, target), set()).add(
                            f"{crate}/{feature}"
                        )
                else:
                    inbound.setdefault((crate, edge), set()).add(f"{crate}/{feature}")

    discovered: dict[tuple[str, str], FeatureFacts] = {}
    for crate, (member, data) in manifests.items():
        features = {
            name: list(edges) for name, edges in data.get("features", {}).items()
        }
        closure = default_closure(features)
        # Exact edge tokens: a substring test would let `dep:serde_derive`
        # suppress the unrelated implicit feature `serde`.
        declared_edges = {
            edge for edges in features.values() for edge in edges if isinstance(edge, str)
        }
        # An optional dependency creates an implicit feature only when no
        # feature references it as `dep:<name>`.
        implicit = sorted(
            dep
            for dep in optional_dependencies(data)
            if f"dep:{dep}" not in declared_edges
        )
        relative = member.relative_to(root).as_posix()
        cfg_counts = count_cfg_uses(member)
        required_by = required_feature_targets(data)
        for name in sorted(features):
            key = (crate, name)
            discovered[key] = FeatureFacts(
                crate=crate,
                name=name,
                manifest=f"{relative}/Cargo.toml",
                kind="explicit",
                edges=tuple(features[name]),
                cfg_uses=cfg_counts.get(name, 0),
                inbound_refs=tuple(sorted(inbound.get(key, ()))),
                required_by_targets=tuple(required_by.get(name, ())),
                in_default_closure=name in closure,
            )
        for name in implicit:
            key = (crate, name)
            if key in discovered:
                continue
            discovered[key] = FeatureFacts(
                crate=crate,
                name=name,
                manifest=f"{relative}/Cargo.toml",
                kind="implicit_optional_dep",
                edges=(),
                cfg_uses=cfg_counts.get(name, 0),
                inbound_refs=tuple(sorted(inbound.get(key, ()))),
                required_by_targets=tuple(required_by.get(name, ())),
                in_default_closure=name in closure,
            )
    return discovered


def load_registry(root: Path) -> Registry:
    path = root / REGISTRY
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except OSError as error:
        raise ValidationError(f"cannot read {REGISTRY}: {error}") from error
    except tomllib.TOMLDecodeError as error:
        raise ValidationError(f"cannot parse {REGISTRY}: {error}") from error
    rows = data.get("feature", [])
    if not isinstance(rows, list):
        raise ValidationError("[[feature]] must be an array of tables")
    authority = data.get("authority", {})
    if not isinstance(authority, dict):
        raise ValidationError("[authority] must be a table")
    return Registry(
        schema_version=data.get("schema_version"),
        policy=data.get("policy"),
        roles=tuple(data.get("roles", ())),
        consumer_signals=tuple(data.get("consumer_signals", ())),
        authority={k: v for k, v in authority.items() if isinstance(v, str)},
        rows=rows,
    )


def find_cycles(edges: dict[str, list[str]]) -> list[str]:
    """Report intra-crate feature cycles as sorted, stable strings."""
    cycles: set[str] = set()
    colour: dict[str, int] = {}

    def visit(node: str, stack: list[str]) -> None:
        colour[node] = 1
        stack.append(node)
        for edge in edges.get(node, []):
            if edge.startswith("dep:") or "/" in edge or edge not in edges:
                continue
            if colour.get(edge, 0) == 1:
                start = stack.index(edge)
                cycle = stack[start:] + [edge]
                cycles.add(" -> ".join(cycle))
            elif colour.get(edge, 0) == 0:
                visit(edge, stack)
        stack.pop()
        colour[node] = 2

    for node in sorted(edges):
        if colour.get(node, 0) == 0:
            visit(node, [])
    return sorted(cycles)


def validate(
    registry: Registry, discovered: dict[tuple[str, str], FeatureFacts]
) -> list[str]:
    """Reconcile the registry against discovery.  Pure; returns errors."""
    errors: list[str] = []

    if registry.schema_version != SCHEMA_VERSION:
        errors.append(
            f"schema_version must be {SCHEMA_VERSION}, found {registry.schema_version!r}"
        )
    if registry.policy != POLICY_NAME:
        errors.append(f"policy must be {POLICY_NAME!r}, found {registry.policy!r}")
    if tuple(registry.roles) != ROLES:
        errors.append(
            "declared roles must match the #8409 vocabulary exactly: "
            f"expected {list(ROLES)}, found {list(registry.roles)}"
        )
    if tuple(registry.consumer_signals) != CONSUMER_SIGNALS:
        errors.append(
            "declared consumer_signals must match exactly: "
            f"expected {list(CONSUMER_SIGNALS)}, found "
            f"{list(registry.consumer_signals)}"
        )
    for required in ("build_combinations", "product_maturity"):
        if required not in registry.authority:
            errors.append(
                f"[authority] must name the separate {required} authority "
                "so this registry does not become a second one"
            )

    seen: dict[tuple[str, str], int] = {}
    order: list[tuple[str, str]] = []
    for index, row in enumerate(registry.rows):
        label = f"row {index + 1}"
        if not isinstance(row, dict):
            errors.append(f"{label}: not a table")
            continue
        unknown = sorted(set(row) - ALLOWED_ROW_KEYS)
        if unknown:
            errors.append(
                f"{label}: unknown key(s) {unknown}; maturity, advertisement and "
                "supported-combination claims belong to their own authorities"
            )
        missing = sorted(REQUIRED_ROW_KEYS - set(row))
        if missing:
            errors.append(f"{label}: missing required key(s) {missing}")
            continue
        crate = row["crate"]
        name = row["name"]
        if not isinstance(crate, str) or not isinstance(name, str):
            errors.append(f"{label}: crate and name must be strings")
            continue
        key = (crate, name)
        label = f"{crate}/{name}"
        order.append(key)
        if key in seen:
            errors.append(
                f"{label}: duplicate row (already declared at row {seen[key] + 1})"
            )
            continue
        seen[key] = index

        role = row["role"]
        if role not in ROLES:
            errors.append(f"{label}: unknown role {role!r}")
        owner = row["owner"]
        if not isinstance(owner, str) or not OWNER_RE.match(owner):
            errors.append(f"{label}: owner must be an issue reference like '#8409'")
        consumers = row["consumers"]
        if not isinstance(consumers, list) or any(
            signal not in CONSUMER_SIGNALS for signal in consumers
        ):
            errors.append(
                f"{label}: consumers must be a list drawn from "
                f"{list(CONSUMER_SIGNALS)}, found {consumers!r}"
            )
            consumers = None

        facts = discovered.get(key)
        if facts is None:
            errors.append(
                f"{label}: stale row — no such Cargo feature is declared by any "
                "workspace member"
            )
            continue

        observed = facts.observed_signals()
        if consumers is not None:
            # `consumers` is a set, so membership is the semantic check;
            # ordering is only a determinism requirement and is reported
            # separately so an ordering nit never reads as evidence drift.
            if set(consumers) != set(observed):
                errors.append(
                    f"{label}: declares consumers={sorted(set(consumers))} but "
                    f"discovery observed {list(observed)} "
                    f"(cfg_uses={facts.cfg_uses}, edges={list(facts.edges)}, "
                    f"inbound={list(facts.inbound_refs)}, "
                    f"required_by={list(facts.required_by_targets)}) in "
                    f"{facts.manifest}"
                )
            elif list(consumers) != list(observed):
                errors.append(
                    f"{label}: consumers must be sorted and deduplicated for a "
                    f"deterministic registry; write {list(observed)}"
                )

        migration = row.get("migration")
        needs_migration = role in MIGRATION_REQUIRED_ROLES or (
            not observed and name != DEFAULT_FEATURE
        )
        if needs_migration and not (isinstance(migration, str) and migration.strip()):
            errors.append(
                f"{label}: role={role!r} observed={list(observed)} requires an "
                "explicit 'migration' disposition"
            )

        if role in DEFAULT_RESTRICTED_ROLES and facts.in_default_closure:
            errors.append(
                f"{label}: role={role!r} is reachable from the crate's own default "
                "feature; a default build must not enable it"
            )

        exception = row.get("name_exception")
        has_exception = isinstance(exception, str) and exception.strip()
        if TEST_IMPLYING_RE.search(name) and role != "test_only" and not has_exception:
            errors.append(
                f"{label}: the name asserts test status but role={role!r}; either "
                "classify it test_only or record a 'name_exception'"
            )
        if (
            EXPERIMENT_IMPLYING_RE.search(name)
            and role != "experimental_opt_in"
            and not has_exception
        ):
            errors.append(
                f"{label}: the name asserts experimental status but role={role!r}; "
                "either classify it experimental_opt_in or record a 'name_exception'"
            )
        if ROADMAP_IMPLYING_RE.search(name) and not has_exception:
            errors.append(
                f"{label}: the name asserts roadmap position rather than build "
                "composition; record a 'name_exception' naming the migration owner"
            )

    if order != sorted(order):
        misplaced = [
            f"{crate}/{name}"
            for index, (crate, name) in enumerate(order)
            if index and order[index - 1] > (crate, name)
        ]
        errors.append(
            "rows must be sorted by (crate, name) so the registry stays "
            f"deterministic; first out of order: {misplaced[:5]}"
        )

    for key in sorted(set(discovered) - set(seen)):
        facts = discovered[key]
        errors.append(
            f"{facts.crate}/{facts.name}: unregistered Cargo feature "
            f"({facts.kind}) declared in {facts.manifest}; add a classified row "
            f"to {REGISTRY.as_posix()} with "
            f"consumers = {list(facts.observed_signals())} "
            f"(cfg_uses={facts.cfg_uses})"
        )

    by_crate: dict[str, dict[str, list[str]]] = {}
    for (crate, name), facts in discovered.items():
        by_crate.setdefault(crate, {})[name] = list(facts.edges)
    for crate in sorted(by_crate):
        for cycle in find_cycles(by_crate[crate]):
            errors.append(f"{crate}: feature cycle {cycle}")

    return errors


def explain(discovered: dict[tuple[str, str], FeatureFacts]) -> str:
    lines = []
    for key in sorted(discovered):
        facts = discovered[key]
        lines.append(
            f"{facts.crate}/{facts.name}\t{facts.kind}\t"
            f"observed={','.join(facts.observed_signals()) or '-'}\t"
            f"cfg_uses={facts.cfg_uses}\t"
            f"default_closure={facts.in_default_closure}\t"
            f"edges={','.join(facts.edges) or '-'}\t"
            f"inbound={','.join(facts.inbound_refs) or '-'}"
        )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--explain",
        action="store_true",
        help="print discovered feature evidence instead of validating",
    )
    args = parser.parse_args()
    root = args.repo_root.resolve()
    try:
        discovered = discover(root)
        if args.explain:
            print(explain(discovered))
            return 0
        errors = validate(load_registry(root), discovered)
    except ValidationError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2
    if errors:
        for error in errors:
            print(f"FAIL: {error}")
        print(
            f"\n{len(errors)} problem(s). Re-run locally with:\n"
            f"  python3 scripts/ci/validate_cargo_feature_roles.py\n"
            f"  python3 scripts/ci/validate_cargo_feature_roles.py --explain",
            file=sys.stderr,
        )
        return 1
    crates = len({crate for crate, _ in discovered})
    print(
        f"OK: cargo-feature-roles rows={len(discovered)} crates={crates} "
        f"registry={REGISTRY.as_posix()}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
