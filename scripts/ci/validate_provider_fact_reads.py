#!/usr/bin/env python3
"""Validate the provider fact-read inventory and its generated projection."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from collections import Counter
from pathlib import Path, PurePosixPath
from typing import Any, Iterable

INVENTORY = PurePosixPath("policy/provider-fact-reads.toml")
EXPECTED_SCHEMA = 2
EXPECTED_POLICY = "provider-fact-reads"
EXPECTED_OWNER = 6815
EXPECTED_STATUS = PurePosixPath("docs/project/status/provider_fact_reads.md")
EXPECTED_PROVIDERS = (
    "completion",
    "definition",
    "references",
    "hover",
    "diagnostics",
    "rename",
    "safe_delete",
    "workspace_symbols",
    "document_symbols",
    "semantic_tokens",
)
EXPECTED_PRODUCERS = (
    "current_document",
    "workspace_index",
    "semantic_queries",
    "semantic_shadow",
    "runtime_mixed",
)
EXPECTED_PROOF_CLASSES = ("mixed", "shadow", "edit_authorizing")
EXPECTED_DISPOSITIONS = (
    "port_candidate",
    "intentional_provider_policy",
    "retire_after_parity",
)
EXPECTED_READ_IDS = (
    "completion.current_document_snapshot",
    "completion.workspace_candidates",
    "completion.receiver_shadow",
    "definition.navigation_candidates",
    "references.reference_candidates",
    "hover.semantic_and_fallback",
    "diagnostics.push_publication",
    "diagnostics.pull_document",
    "diagnostics.pull_workspace",
    "rename.lexical_and_workspace_plan",
    "safe_delete.semantic_plan",
    "workspace_symbols.index_query",
    "document_symbols.current_snapshot",
    "semantic_tokens.full",
    "semantic_tokens.delta",
    "semantic_tokens.range",
)
ALLOWED_ANCHOR_KINDS = {"rust_fn", "rust_call"}
REQUIRED_TEXT_FIELDS = (
    "id",
    "provider",
    "request_class",
    "query",
    "producer",
    "proof_class",
    "readiness_input",
    "fallback",
    "duplicate_interpretation",
    "migration_disposition",
    "replacement_owner",
)
OWNER_TOKEN = re.compile(r"^#[1-9][0-9]*$")
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


class ValidationError(RuntimeError):
    pass


def fail(message: str) -> None:
    raise ValidationError(message)


def load_inventory(root: Path) -> dict[str, Any]:
    path = root / INVENTORY
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"failed to load {INVENTORY}: {error}")


def exact_vocabulary(label: str, actual: Iterable[str], expected: Iterable[str]) -> None:
    actual_list = list(actual)
    expected_list = list(expected)
    if len(actual_list) != len(set(actual_list)):
        fail(f"{label} contains duplicate values")
    if set(actual_list) != set(expected_list):
        fail(f"{label} must equal {expected_list!r}, found {actual_list!r}")


def safe_repo_path(raw: str, *, generated: bool = False) -> PurePosixPath:
    path = PurePosixPath(raw)
    if path.is_absolute() or ".." in path.parts or "." in path.parts:
        fail(f"repository path must be normalized and relative: {raw!r}")
    if not path.parts:
        fail("repository path must not be empty")
    if generated:
        if path.parent != PurePosixPath("docs/project/status") or path.suffix != ".md":
            fail(
                "generated_status must be a Markdown file directly under "
                f"docs/project/status, found {raw!r}"
            )
    return path


def rust_tokens(source: str) -> list[str]:
    """Return Rust identifiers and punctuation outside comments and literals."""

    tokens: list[str] = []
    index = 0
    length = len(source)
    while index < length:
        char = source[index]
        next_char = source[index + 1] if index + 1 < length else ""

        if char.isspace():
            index += 1
            continue
        if char == "/" and next_char == "/":
            index = source.find("\n", index + 2)
            if index == -1:
                break
            continue
        if char == "/" and next_char == "*":
            depth = 1
            index += 2
            while index < length and depth:
                pair = source[index : index + 2]
                if pair == "/*":
                    depth += 1
                    index += 2
                elif pair == "*/":
                    depth -= 1
                    index += 2
                else:
                    index += 1
            continue

        raw_start = index
        if char == "b" and next_char == "r":
            raw_start += 1
        if source[raw_start : raw_start + 1] == "r":
            cursor = raw_start + 1
            hashes = 0
            while cursor < length and source[cursor] == "#":
                hashes += 1
                cursor += 1
            if cursor < length and source[cursor] == '"':
                terminator = '"' + ("#" * hashes)
                end = source.find(terminator, cursor + 1)
                index = length if end == -1 else end + len(terminator)
                continue

        string_start = index + 1 if char == "b" and next_char == '"' else index
        if source[string_start : string_start + 1] == '"':
            index = string_start + 1
            escaped = False
            while index < length:
                current = source[index]
                index += 1
                if escaped:
                    escaped = False
                elif current == "\\":
                    escaped = True
                elif current == '"':
                    break
            continue

        if char == "'":
            cursor = index + 1
            escaped = False
            while cursor < min(length, index + 8):
                current = source[cursor]
                cursor += 1
                if escaped:
                    escaped = False
                elif current == "\\":
                    escaped = True
                elif current == "'":
                    index = cursor
                    break
            else:
                tokens.append("'")
                index += 1
            continue

        if char.isalpha() or char == "_":
            cursor = index + 1
            while cursor < length and (source[cursor].isalnum() or source[cursor] == "_"):
                cursor += 1
            tokens.append(source[index:cursor])
            index = cursor
            continue

        if source[index : index + 2] == "::":
            tokens.append("::")
            index += 2
            continue

        tokens.append(char)
        index += 1

    return tokens


def anchor_count(tokens: list[str], kind: str, value: str) -> int:
    if kind == "rust_fn":
        return sum(
            1
            for index in range(len(tokens) - 1)
            if tokens[index] == "fn" and tokens[index + 1] == value
        )
    if kind == "rust_call":
        return sum(
            1
            for index in range(len(tokens) - 1)
            if tokens[index] == value
            and tokens[index + 1] == "("
            and (index == 0 or tokens[index - 1] != "fn")
        )
    fail(f"unsupported anchor kind {kind!r}")
    return 0


def validate_source(root: Path, read_id: str, source: dict[str, Any]) -> None:
    raw_path = source.get("path")
    if not isinstance(raw_path, str) or not raw_path.strip():
        fail(f"{read_id}: source path must be a non-empty string")
    relative = safe_repo_path(raw_path)
    path = root / relative
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        fail(f"{read_id}: failed to read {relative}: {error}")

    anchors = source.get("anchor")
    if not isinstance(anchors, list) or not anchors:
        fail(f"{read_id}: {relative} must declare at least one structural anchor")
    tokens = rust_tokens(text)
    seen: set[tuple[str, str]] = set()
    for anchor in anchors:
        if not isinstance(anchor, dict):
            fail(f"{read_id}: {relative} anchor must be a table")
        kind = anchor.get("kind")
        value = anchor.get("value")
        expected_count = anchor.get("count")
        if kind not in ALLOWED_ANCHOR_KINDS:
            fail(f"{read_id}: unsupported anchor kind {kind!r}")
        if not isinstance(value, str) or not IDENTIFIER.fullmatch(value):
            fail(f"{read_id}: invalid Rust anchor identifier {value!r}")
        if not isinstance(expected_count, int) or isinstance(expected_count, bool) or expected_count < 1:
            fail(f"{read_id}: anchor count must be a positive integer")
        identity = (kind, value)
        if identity in seen:
            fail(f"{read_id}: duplicate anchor {kind}:{value}")
        seen.add(identity)
        actual_count = anchor_count(tokens, kind, value)
        if actual_count != expected_count:
            fail(
                f"{read_id}: {relative} anchor {kind}:{value} expected "
                f"{expected_count}, found {actual_count}"
            )


def validate_inventory(root: Path, inventory: dict[str, Any]) -> None:
    if inventory.get("schema_version") != EXPECTED_SCHEMA:
        fail(
            f"schema_version must be {EXPECTED_SCHEMA}, "
            f"found {inventory.get('schema_version')!r}"
        )
    if inventory.get("policy") != EXPECTED_POLICY:
        fail(f"policy must be {EXPECTED_POLICY!r}")
    if inventory.get("owner_issue") != EXPECTED_OWNER:
        fail(f"owner_issue must be {EXPECTED_OWNER}")

    generated_raw = inventory.get("generated_status")
    if not isinstance(generated_raw, str):
        fail("generated_status must be a string")
    generated = safe_repo_path(generated_raw, generated=True)
    if generated != EXPECTED_STATUS:
        fail(f"generated_status must be {EXPECTED_STATUS}, found {generated}")

    exact_vocabulary(
        "required_providers", inventory.get("required_providers", []), EXPECTED_PROVIDERS
    )
    exact_vocabulary(
        "allowed_producers", inventory.get("allowed_producers", []), EXPECTED_PRODUCERS
    )
    exact_vocabulary(
        "allowed_proof_classes",
        inventory.get("allowed_proof_classes", []),
        EXPECTED_PROOF_CLASSES,
    )
    exact_vocabulary(
        "allowed_dispositions",
        inventory.get("allowed_dispositions", []),
        EXPECTED_DISPOSITIONS,
    )

    reads = inventory.get("read")
    if not isinstance(reads, list):
        fail("inventory must contain [[read]] rows")
    ids = [read.get("id") for read in reads if isinstance(read, dict)]
    exact_vocabulary("read IDs", ids, EXPECTED_READ_IDS)

    provider_counts: Counter[str] = Counter()
    for read in reads:
        if not isinstance(read, dict):
            fail("each [[read]] row must be a table")
        read_id = read.get("id")
        for field in REQUIRED_TEXT_FIELDS:
            value = read.get(field)
            if not isinstance(value, str) or not value.strip():
                fail(f"{read_id}: {field} must be a non-empty string")
        provider = read["provider"]
        if not read_id.startswith(provider + "."):
            fail(f"{read_id}: ID must start with provider prefix {provider!r}")
        if provider not in EXPECTED_PROVIDERS:
            fail(f"{read_id}: ungoverned provider {provider!r}")
        if read["producer"] not in EXPECTED_PRODUCERS:
            fail(f"{read_id}: unsupported producer {read['producer']!r}")
        if read["proof_class"] not in EXPECTED_PROOF_CLASSES:
            fail(f"{read_id}: unsupported proof class {read['proof_class']!r}")
        if read["migration_disposition"] not in EXPECTED_DISPOSITIONS:
            fail(
                f"{read_id}: unsupported disposition "
                f"{read['migration_disposition']!r}"
            )
        for owner in read["replacement_owner"].split("/"):
            if not OWNER_TOKEN.fullmatch(owner):
                fail(f"{read_id}: invalid replacement owner {owner!r}")

        sources = read.get("source")
        if not isinstance(sources, list) or not sources:
            fail(f"{read_id}: must declare at least one [[read.source]]")
        source_paths: list[str] = []
        for source in sources:
            if not isinstance(source, dict):
                fail(f"{read_id}: source must be a table")
            source_paths.append(str(source.get("path")))
            validate_source(root, read_id, source)
        if len(source_paths) != len(set(source_paths)):
            fail(f"{read_id}: duplicate source paths")
        provider_counts[provider] += 1

    missing = [provider for provider in EXPECTED_PROVIDERS if provider_counts[provider] == 0]
    if missing:
        fail(f"required providers without reads: {missing}")


def escape_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", "<br>")


def render_sources(sources: list[dict[str, Any]]) -> str:
    rendered: list[str] = []
    for source in sources:
        anchors = ", ".join(
            f"{anchor['kind']}:{anchor['value']}×{anchor['count']}"
            for anchor in source["anchor"]
        )
        rendered.append(f"`{escape_cell(source['path'])}`<br>{escape_cell(anchors)}")
    return "<br>".join(rendered)


def render_status(inventory: dict[str, Any]) -> str:
    reads = inventory["read"]
    counts = Counter(read["provider"] for read in reads)
    lines = [
        "# Provider Fact Read Inventory",
        "",
        "> Generated by `cargo xtask update-status --write --only provider-facts` from",
        f"> `{INVENTORY}`. This inventory records current provider fact reads,",
        "> ownership assumptions, and duplicate interpretation seams. It does not",
        "> change provider behavior, promote a producer, or authorize edits.",
        "",
        f"Owner: [#{inventory['owner_issue']}](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/{inventory['owner_issue']})",
        "",
        "## Coverage",
        "",
        "| Provider | Inventoried reads |",
        "| --- | ---: |",
    ]
    lines.extend(
        f"| `{escape_cell(provider)}` | {counts[provider]} |"
        for provider in inventory["required_providers"]
    )
    lines.extend(
        [
            "",
            "## Reads",
            "",
            "| ID | Provider | Request | Query / fact need | Current producer | Proof assumption | Readiness / freshness | Fallback / refusal | Executable seams | Duplicate interpretation seam | Migration |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    )
    for read in reads:
        lines.append(
            f"| `{escape_cell(read['id'])}` | `{escape_cell(read['provider'])}` | "
            f"`{escape_cell(read['request_class'])}` | {escape_cell(read['query'])} | "
            f"`{escape_cell(read['producer'])}` | `{escape_cell(read['proof_class'])}` | "
            f"{escape_cell(read['readiness_input'])} | {escape_cell(read['fallback'])} | "
            f"{render_sources(read['source'])} | {escape_cell(read['duplicate_interpretation'])} | "
            f"`{escape_cell(read['migration_disposition'])}` → "
            f"{escape_cell(read['replacement_owner'])} |"
        )
    lines.extend(
        [
            "",
            "## Claim boundary",
            "",
            "- A producer name is not a proof or safety class.",
            "- An inventory row is not a cutover decision.",
            "- Every row is anchored to executable Rust structure; comments and string literals do not satisfy the inventory.",
            "- `port_candidate` means the read should move behind the canonical provider port.",
            "- `intentional_provider_policy` means domain policy may remain provider-owned after shared facts arrive.",
            "- `retire_after_parity` requires request-bound comparison evidence before removal.",
            "- Generated, dynamic, stale, partial, ambiguous, or low-confidence facts do not gain edit authority from this inventory.",
            "",
        ]
    )
    return "\n".join(lines)


def validate_generated_status(root: Path, inventory: dict[str, Any]) -> None:
    expected = render_status(inventory)
    path = root / EXPECTED_STATUS
    try:
        actual = path.read_text(encoding="utf-8")
    except OSError as error:
        fail(f"failed to read {EXPECTED_STATUS}: {error}")
    if actual != expected:
        fail(
            f"{EXPECTED_STATUS} is stale; run "
            "`cargo xtask update-status --write --only provider-facts`"
        )


def self_test() -> None:
    sample = '''
fn real_target() { helper(); }
// fn comment_target() { helper(); }
const TEXT: &str = "fn string_target() { helper(); }";
/* helper(); fn block_target() {} */
'''
    tokens = rust_tokens(sample)
    assert anchor_count(tokens, "rust_fn", "real_target") == 1
    assert anchor_count(tokens, "rust_fn", "comment_target") == 0
    assert anchor_count(tokens, "rust_fn", "string_target") == 0
    assert anchor_count(tokens, "rust_fn", "block_target") == 0
    assert anchor_count(tokens, "rust_call", "helper") == 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    try:
        self_test()
        if not args.self_test:
            root = args.root.resolve()
            inventory = load_inventory(root)
            validate_inventory(root, inventory)
            validate_generated_status(root, inventory)
            print(
                f"provider fact-read inventory: {len(inventory['read'])} reads, "
                f"{len(inventory['required_providers'])} providers, structurally valid"
            )
        return 0
    except (AssertionError, ValidationError) as error:
        print(f"provider fact-read inventory validation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
