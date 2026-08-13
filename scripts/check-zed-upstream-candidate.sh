#!/usr/bin/env bash
# Static fail-closed checks for the staged Zed extension submission packet.
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

bash -n "$REPO_ROOT/scripts/apply-zed-perl-upstream.sh"

python3 - "$REPO_ROOT" <<'PY'
from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path

root = Path(sys.argv[1])
packet = root / ".ci" / "fixtures" / "zed-perl-upstream"
candidate = packet / "zed-perl"


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise SystemExit(f"error: cannot read {path.relative_to(root)}: {error}") from error


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"error: {message}")


with (packet / "manifest.toml").open("rb") as handle:
    manifest = tomllib.load(handle)
with (candidate / "extension.toml").open("rb") as handle:
    extension = tomllib.load(handle)
with (candidate / "languages" / "perl" / "config.toml").open("rb") as handle:
    language = tomllib.load(handle)
with (root / "docs" / "reference" / "downstream-dap-integrations.json").open(
    encoding="utf-8"
) as handle:
    release_contract = json.load(handle)
with (packet / "zed-defaults.json").open(encoding="utf-8") as handle:
    defaults = json.load(handle)
with (candidate / "languages" / "perl" / "semantic_token_rules.json").open(
    encoding="utf-8"
) as handle:
    semantic_rules = json.load(handle)

source = read(candidate / "src" / "perl.rs")
readme = read(root / "README.md")
faq = read(root / "docs" / "reference" / "FAQ.md")
setup = read(root / "docs" / "EDITORS" / "ZED_SETUP.md")
combined_setup = read(root / "docs" / "how-to" / "EDITOR_SETUP.md")
book_setup = read(root / "book" / "src" / "reference" / "editor-setup-canonical.md")
troubleshooting = read(root / "docs" / "how-to" / "TROUBLESHOOTING.md")
steering = read(root / ".kiro" / "steering" / "product.md")

servers = extension.get("language_servers", {})
require(
    set(servers) == {"perlnavigator-server", "perl-lsp", "perllsp"},
    "extension.toml must register exactly the three distinct Perl server IDs",
)
require(
    'const PERLLSP_SERVER_ID: &str = "perllsp";' in source,
    "candidate source must own the dedicated perllsp server ID",
)
require(
    'const PERLLSP_REPO: &str = "EffortlessMetrics/perl-lsp";' in source,
    "candidate source must download from EffortlessMetrics/perl-lsp",
)
require(
    "normalize_perllsp_args" in source and 'normalized.push("--stdio".to_string())' in source,
    "candidate source must normalize to exactly one explicit --stdio argument",
)
require(
    'argument == "--stdio" || argument == "--mcp" || argument == "mcp"' in source,
    "candidate source must treat mcp/--mcp as stdio launcher aliases",
)
require(
    "is_non_lsp_argument" in source and '"--socket"' in source,
    "candidate source must reject non-LSP transport routes such as --socket",
)
require(
    "LspSettings::for_worktree(PERLLSP_SERVER_ID, worktree)" in source,
    "candidate source must consume standard perllsp binary settings",
)
require(
    "worktree.shell_env()" in source and "shell_env.extend(overrides)" in source,
    "candidate source must use the worktree shell environment with explicit overrides",
)
require(
    '"lsp.perllsp.binary.path must not be empty"' in source,
    "candidate source must fail closed on an empty explicit binary override",
)
require(
    "unknown Perl language server id" in source,
    "candidate dispatcher must reject unknown server IDs",
)
require(
    'const PERL_LSP_REPO: &str = "tree-sitter-perl/perl-tree-sitter-lsp";' in source,
    "candidate must preserve the independent tree-sitter-perl server identity",
)

suffixes = set(language.get("path_suffixes", []))
require(
    {"pl", "PL", "pm", "t", "psgi", "cgi", "fcgi"}.issubset(suffixes),
    "Perl activation is missing a required staged suffix",
)
require("pod" not in {value.lower() for value in suffixes}, ".pod must remain the POD language")

rule_types = {rule.get("token_type") for rule in semantic_rules}
require(
    rule_types == {"sql_string", "sql_heredoc_keyword", "json_heredoc_key"},
    "semantic-token defaults must cover the three perllsp custom token types exactly",
)

released_targets = {entry["triple"] for entry in release_contract["targets"]}
managed_targets = set(manifest["managed_targets"])
require(
    managed_targets <= released_targets,
    f"managed target(s) absent from release contract: {sorted(managed_targets - released_targets)}",
)
require(
    "aarch64-pc-windows-msvc" in manifest["unsupported_managed_targets"],
    "Windows ARM64 must remain explicitly unclaimed until the release contract promotes it",
)

server_defaults = defaults["languages"]["Perl"]["language_servers"]
require(
    server_defaults
    == ["perlnavigator-server", "!perl-lsp", "!perllsp", "..."],
    "Zed defaults must preserve Perl Navigator and disable both alternatives",
)

require("Zed integration: planned / not proven" in readme, "README Zed boundary is missing")
require("Zed is **planned / not proven**" in faq, "FAQ Zed boundary is missing")
require("**Status: planned / not proven.**" in setup, "Zed guide status is missing")
require("Planned / not proven" in combined_setup, "combined editor table must bound Zed")
require(book_setup == combined_setup, "committed mdBook editor projection must match the canonical guide")
require("public Perl extension does not register `perllsp`" in troubleshooting, "troubleshooting boundary is missing")
require("Zed integration: planned / not proven" in steering, "agent steering still overclaims Zed")


def markdown_section(text: str, heading: str, next_heading_prefix: str) -> str:
    marker = f"{heading}\n"
    start = text.find(marker)
    require(start >= 0, f"missing Markdown section `{heading}`")
    body_start = start + len(marker)
    end = text.find(f"\n{next_heading_prefix}", body_start)
    return text[body_start:] if end < 0 else text[body_start:end]


zed_sections = {
    "docs/EDITORS/ZED_SETUP.md": setup,
    "docs/how-to/EDITOR_SETUP.md": markdown_section(combined_setup, "### Zed", "### "),
    "docs/how-to/TROUBLESHOOTING.md": markdown_section(
        troubleshooting, "## Zed Does Not Start `perllsp`", "## "
    ),
}
for path, text in zed_sections.items():
    require(
        '\"perl-lsp\": {' not in text,
        f"{path} still contains a runnable Zed perl-lsp configuration block",
    )

require(
    "`perl-lsp` | `tree-sitter-perl/perl-tree-sitter-lsp`" in setup,
    "Zed guide must identify the existing perl-lsp product",
)
require(
    "does **not** register `perllsp`" in setup,
    "Zed guide must state the public extension gap",
)

print("Zed integration candidate checks passed.")
PY
