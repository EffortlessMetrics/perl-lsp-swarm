#!/usr/bin/env python3
"""Apply the bounded review repairs for PR #11976.

Every replacement is count-checked. The script is temporary branch machinery and
is removed by the rebuild workflow after validation succeeds.
"""

from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")


def replace(path: str, old: str, new: str, count: int = 1) -> None:
    text = read(path)
    actual = text.count(old)
    if actual != count:
        raise RuntimeError(
            f"{path}: expected {count} matches, found {actual}: {old[:120]!r}"
        )
    write(path, text.replace(old, new))


CONFIG = "crates/perl-lsp-rs-core/src/config/mod.rs"
replace(
    CONFIG,
    "        // These settings arrive only via the LSP client/server configuration\n"
    "        // channel. Project activation is closed here (#4997); VS Code declares\n"
    "        // AI toggles `scope: machine`. Non-VS Code clients that forward\n"
    "        // workspace settings into `didChangeConfiguration` remain a residual\n"
    "        // provenance gap (documented in AI_COMPLETION.md).\n",
    "        // Endpoint and credential-routing fields remain excluded from project and\n"
    "        // generic client channels (#5684). Activation, provider/model selection,\n"
    "        // and streaming activation are rejected by `ServerConfig::update_from_value`\n"
    "        // regardless of client shape (#4997). A future server-owned adapter (#10817)\n"
    "        // must admit trusted user/operator authority explicitly.\n",
)
replace(
    CONFIG,
    '        config.ai_completion.provider = "openai_compat".to_string();\n'
    '        config.ai_completion.model = "user-chosen-model".to_string();\n',
    '        config.ai_completion.provider = "user-chosen-provider".to_string();\n'
    '        config.ai_completion.model = "user-chosen-model".to_string();\n',
)
replace(
    CONFIG,
    '        assert_eq!(config.ai_completion.model, "user-chosen-model");\n\n'
    '        // Malformed values are ignored and reset nothing.\n',
    '        assert_eq!(config.ai_completion.provider, "user-chosen-provider");\n'
    '        assert_eq!(config.ai_completion.model, "user-chosen-model");\n\n'
    '        // Malformed values are ignored and reset nothing.\n',
)

SCHEMA = "schemas/perllsp-settings.schema.json"
replace(
    SCHEMA,
    '          "description": "Client-controlled AI completion behavior. Activation, provider/model selection, and endpoint/credential-routing fields are intentionally excluded because no generic LSP settings channel can prove user/machine provenance (#4997, #5684); arrivals are rejected and previously accepted state is preserved.",\n',
    '          "description": "Client-controlled AI completion behavior. Endpoint and credential-routing fields are intentionally excluded from this schema (#5684). Activation and provider/model selection fields remain documented but advertise no client transports: no generic LSP settings channel can prove user/machine provenance (#4997), so arrivals are rejected and previously accepted state is preserved.",\n',
)

ZED = ".ci/fixtures/zed-perl-upstream/settings-behavior.v1.json"
replace(
    ZED,
    '      "observable": "The server configuration trace identifies the nested envelope value and its winning authority; AI activation/selection keys are authority-rejected on this channel (#4997) and client-visible streaming remains a separate feature cell."\n',
    '      "observable": "The server configuration trace identifies the nested envelope value and its winning authority. Separate authority tests cover rejection of AI activation and selection keys (#4997)."\n',
)

ACCEPTANCE = ".spec/4997-ai-egress-authority/acceptance.md"
replace(
    ACCEPTANCE,
    "| AI-AUTH-006 | Client-supplied scope/trust/client labels confer no authority | authority is set only by server-side constructor; parser never reads trust labels from payloads | payload field promotes authority |\n",
    "| AI-AUTH-006 | Client-supplied scope/trust/client labels confer no authority | authority is admitted only through `AiCompletionConfig::admit_trusted_user_operator_activation()`; parser never reads trust labels from payloads | payload field promotes authority |\n",
)

CHECKLIST = ".spec/4997-ai-egress-authority/checklist.md"
replace(
    CHECKLIST,
    "- [x] `cargo test ... --test lsp_ai_inline_completion_tests` (feature) — 14 passed\n",
    "- [x] `RUST_TEST_THREADS=2 cargo test -p perl-lsp-rs --features expose_lsp_test_api --test lsp_ai_inline_completion_tests -- --test-threads=2` — 14 passed\n",
)
replace(
    CHECKLIST,
    "- [x] `cargo fmt --all -- --check` (per-package `cargo fmt -p <pkg> -- --check`\n"
    "      for perl-lsp-rs-core / perl-lsp-rs / xtask; `--all` trips a Windows\n"
    "      command-length limit on this box, exit 206)\n",
    "- [x] Per-package `cargo fmt -p <pkg> -- --check` for `perl-lsp-rs-core`, `perl-lsp-rs`, and `xtask` — passed on the original Windows review host\n"
    "- [x] `cargo fmt --all -- --check` — passed in hosted Linux review-repair validation; the earlier Windows exit 206 was not treated as proof\n",
)

DOC = "docs/reference/AI_COMPLETION.md"
replace(
    DOC,
    '| `perl-lsp.aiCompletion.enabled` | boolean | `false` | Enable AI-powered inline completions. **Machine scope** — cannot be set per-workspace in `.vscode/settings.json`. |\n'
    '| `perl-lsp.aiCompletion.streaming.enabled` | boolean | `true` | Enable progressive streaming (ghost text updates as tokens arrive). Requires `aiCompletion.enabled`. **Machine scope.** |\n',
    '| `perl-lsp.aiCompletion.enabled` | boolean | `false` | Reserved machine-scoped extension preference. The extension does not forward trusted activation and generic server arrivals are rejected, so this key cannot currently enable remote AI. |\n'
    '| `perl-lsp.aiCompletion.streaming.enabled` | boolean | `true` | Reserved machine-scoped extension preference for a future trusted adapter. It does not currently authorize server streaming. |\n',
)
replace(
    DOC,
    "Enabling AI or choosing provider/model requires\nuser/machine client settings.\n",
    "Enabling AI or choosing provider/model requires the future server-owned trusted\nuser/operator adapter; no accepted activation channel exists today.\n",
)
replace(
    DOC,
    "# enabled = true is ignored — AI must be turned on in user/machine settings.\n",
    "# enabled = true is ignored — no accepted activation channel exists today.\n",
)
replace(
    DOC,
    "> **Activation is user/machine-scoped — and server-enforced.**\n"
    "> `perl-lsp.aiCompletion.enabled` and\n"
    "> `perl-lsp.aiCompletion.streaming.enabled` are declared `scope: machine` in\n"
    "> the VS Code extension, the server ignores project attempts to enable AI,\n"
    "> and no generic LSP channel can arm the backend either (issue #4997).\n"
    "> A repository can still opt out with `[ai_completion] enabled =\n"
    "> false` in `.perl-lsp.toml`. Issue #4998 covers the same provenance gap for\n"
    "> include paths.\n",
    "> **Activation authority is server-owned and fails closed.**\n"
    "> The VS Code keys are machine-scoped defense in depth, but the extension does\n"
    "> not forward trusted activation and no generic LSP channel can arm the backend\n"
    "> (issue #4997). A repository can still opt out with `[ai_completion]\n"
    "> enabled = false` in `.perl-lsp.toml`. Issue #4998 covers the corresponding\n"
    "> provenance boundary for include paths.\n",
)
replace(
    DOC,
    "## Example Configuration (VS Code `settings.json`)\n\n"
    "```jsonc\n"
    "{\n"
    "  // Enable the feature\n"
    '  "perl-lsp.aiCompletion.enabled": true\n'
    "}\n"
    "```\n\n",
    "## VS Code activation status\n\n"
    "There is intentionally no `settings.json` example that enables remote AI. The\n"
    "machine-scoped extension toggle is not an accepted server activation channel;\n"
    "until #10817 supplies a trusted operator adapter, remote construction fails\n"
    "closed.\n\n",
)
replace(
    DOC,
    "## Streaming vs Buffered Behavior\n\n"
    "The server supports two completion delivery modes:\n",
    "## Streaming vs Buffered Behavior\n\n"
    "The following modes describe runtime behavior after a trusted activation adapter\n"
    "exists. Generic `streaming.enabled` arrivals are currently rejected and cannot\n"
    "authorize either mode.\n\n"
    "The server supports two completion delivery modes:\n",
)
replace(
    DOC,
    "- Enable streaming (`streaming.enabled: true`) for lower perceived latency.\n",
    "- After the trusted activation adapter lands, streaming may lower perceived\n"
    "  latency. Generic `streaming.enabled` settings are currently rejected.\n",
)

print("PR #11976 bounded review repairs applied")
