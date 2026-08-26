#!/usr/bin/env bash
# Static fail-closed checks for the staged Zed extension submission packet.
set -euo pipefail

# Toolchain guard (#12593): refuse a stale non-rustup cargo before any build work.
. "$(dirname -- "${BASH_SOURCE[0]}")/lib/cargo-toolchain-guard.sh" && cargo_toolchain_guard

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

bash -n "$REPO_ROOT/scripts/apply-zed-perl-upstream.sh"

# Behavioral proof for the staged candidate: its own unit suite carries the
# LSP/DAP identity-separation, schema acceptance/rejection, request-kind,
# precedence, projection, and cleanup-boundary falsifiers (#9485).
(
  cd "$REPO_ROOT/.ci/fixtures/zed-perl-upstream/zed-perl"
  cargo test --quiet
)

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

# ---- perl-dap debug-adapter authority (#9485) ----

debug_adapters = extension.get("debug_adapters", {})
require(
    set(debug_adapters) == {"perl-dap"},
    "extension.toml must declare exactly the `perl-dap` debug adapter",
)
require(
    debug_adapters["perl-dap"].get("schema_path")
    == "debug_adapter_schemas/perl-dap.json",
    "[debug_adapters.perl-dap] must bind the debugger configuration schema",
)
schema_path = candidate / "debug_adapter_schemas" / "perl-dap.json"
require(schema_path.is_file(), "perl-dap debugger configuration schema is missing")
import json as _json

schema = _json.loads(schema_path.read_text(encoding="utf-8"))
require(
    schema.get("required") == ["request", "program"],
    "perl-dap schema must require exactly `request` and `program`",
)
require(
    schema.get("properties", {}).get("request", {}).get("enum") == ["launch"],
    "perl-dap schema must admit only the `launch` request kind",
)
require(
    schema.get("additionalProperties") is True,
    "perl-dap schema must preserve forward-compatible pass-through keys",
)

require(
    set(servers) == {"perlnavigator-server", "perl-lsp", "perllsp"},
    "the DAP increment must not change the three LSP provider identities",
)
require(
    "perl-dap" not in servers,
    "no language-server ID may alias the perl-dap debug-adapter ID",
)
require(
    manifest.get("debug_adapter_id") == "perl-dap",
    "packet manifest must own the exact `perl-dap` adapter identity",
)
require(
    manifest.get("debug_binary") == "perl-dap",
    "packet manifest must name `perl-dap` as the debug binary",
)
require(
    manifest.get("debug_adapter_id") != manifest.get("server_id"),
    "adapter ID must never alias the LSP server ID",
)
require(
    "debug_adapter_schemas/perl-dap.json" in manifest.get("copied_files", []),
    "submission packet must carry the perl-dap schema",
)

require(
    'const PERL_DAP_ADAPTER_ID: &str = "perl-dap";' in source,
    "candidate source must own the dedicated perl-dap adapter ID",
)
require(
    'const PERL_DAP_REPO: &str = PERLLSP_REPO;' in source,
    "candidate source must consume the canonical shared release topology",
)
require(
    'const PERL_DAP_MANAGED_PREFIX: &str = "perl-dap-managed-";' in source,
    "candidate source must keep a debugger-specific managed cache boundary",
)
require(
    "unknown Perl debug adapter id" in source,
    "candidate DAP dispatcher must reject unknown adapter IDs",
)
require(
    "`attach` configurations are not supported" in source,
    "candidate DAP dispatcher must fail closed on unsupported request kinds",
)
require(
    "lacks the required `request` field" in source
    and "lacks the required `program` field" in source,
    "candidate DAP validation must name the exact missing field",
)
require(
    "worktree.which(PERL_DAP_BINARY_NAME)" in source,
    "candidate resolver must probe the exact `perl-dap` name on PATH",
)
require(
    "remove_old_downloads(PERL_DAP_MANAGED_PREFIX" in source,
    "cleanup must stay inside the perl-dap managed boundary",
)

print("Zed integration candidate checks passed.")
PY
