#!/usr/bin/env python3
"""Adapt the one-shot #4997 editor to the exact current branch substrate.

This file is temporary branch machinery. It makes only count-checked edits to
`apply_4997_ai_activation_authority.py`; the apply workflow deletes both files
before publishing the real implementation commit.
"""

from pathlib import Path

path = Path("scripts/maintenance/apply_4997_ai_activation_authority.py")
text = path.read_text(encoding="utf-8")

signature_old = (
    r"fn workspace_ai_completion_ignores_untrusted_endpoint_and_credential_settings\(\) \{"
)
signature_new = (
    r"fn workspace_ai_completion_ignores_untrusted_endpoint_and_credential_settings\(\)(?: -> TestResult)? \{"
)
assert text.count(signature_old) == 1, text.count(signature_old)
text = text.replace(signature_old, signature_new)

description_old = (
    "AI-powered inline completion configuration. Sensitive destination/credential fields are intentionally excluded from LSP client settings."
)
description_new = (
    "Client-controlled AI completion behavior. Endpoint and credential-routing fields are intentionally excluded because workspace-delivered LSP settings may not redirect source/secret-bearing requests."
)
assert text.count(description_old) == 1, text.count(description_old)
text = text.replace(description_old, description_new)

provider_old = '"provider": { "type": "string", "default": "openai_compat" }'
provider_new = (
    '"provider": { "type": "string", "enum": ["openai_compat"], '
    '"default": "openai_compat" }'
)
block_start = text.index("for line in [")
block_end = text.index(']:\n    replace_exact(SCHEMA, line, "")', block_start)
block = text[block_start:block_end]
assert block.count(provider_old) == 1, block.count(provider_old)
block = block.replace(provider_old, provider_new)
text = text[:block_start] + block + text[block_end:]

section = text.index(
    'SCHEMA_TEST = "crates/perl-lsp-rs-core/tests/perllsp_settings_schema_tests.rs"'
)
call_starts = []
cursor = section
for _ in range(3):
    start = text.index("replace_exact(\n    SCHEMA_TEST,\n", cursor)
    call_starts.append(start)
    cursor = text.index("\n)\n", start) + 3

assertion_start = call_starts[1]
assertion_end = text.index("\n)\n", assertion_start) + 3
sensitive_start = call_starts[2]
sensitive_end = text.index("\n)\n", sensitive_start) + 3

assertion_call = r'''replace_exact(
    SCHEMA_TEST,
    '    assert_eq!(server.ai_completion.user_enabled, true);\n'
    '    assert_eq!(server.ai_completion.model, "fixture-model");\n',
    '    assert_eq!(server.ai_completion.user_enabled, false);\n'
    '    assert_eq!(server.ai_completion.enabled, false);\n'
    '    assert_eq!(server.ai_completion.model, "gpt-4o-mini");\n',
)
'''
sensitive_call = r'''replace_exact(
    SCHEMA_TEST,
    '    assert_eq!(ai.get("endpoint").is_none(), true);\n'
    '    assert_eq!(ai.get("apiKeyEnv").is_none(), true);\n'
    '    assert_eq!(ai.get("apiKeyHeader").is_none(), true);\n'
    '    assert_eq!(ai.get("apiKeyPrefix").is_none(), true);\n',
    '    assert_eq!(ai.get("enabled").is_none(), true);\n'
    '    assert_eq!(ai.get("provider").is_none(), true);\n'
    '    assert_eq!(ai.get("model").is_none(), true);\n'
    '    assert_eq!(ai.get("endpoint").is_none(), true);\n'
    '    assert_eq!(ai.get("apiKeyEnv").is_none(), true);\n'
    '    assert_eq!(ai.get("apiKeyHeader").is_none(), true);\n'
    '    assert_eq!(ai.get("apiKeyPrefix").is_none(), true);\n',
)
'''
text = text[:sensitive_start] + sensitive_call + text[sensitive_end:]
text = text[:assertion_start] + assertion_call + text[assertion_end:]

constructors = text.index(
    'CONSTRUCTORS = "crates/perl-lsp-rs/src/runtime/constructors.rs"'
)
count_start = text.index("    count=2,\n)", constructors)
text = (
    text[:count_start]
    + "    count=3,\n)"
    + text[count_start + len("    count=2,\n)") :]
)

final_old = '''assert_absent(SCHEMA, '\"enabled\": { \"type\": \"boolean\", \"default\": false }')
assert_absent(SCHEMA, '\"provider\": { \"type\": \"string\", \"default\": \"openai_compat\" }')
assert_absent(SCHEMA, '\"model\": { \"type\": \"string\", \"default\": \"gpt-4o-mini\" }')
'''
final_new = '''schema_value = __import__("json").loads(read(SCHEMA))
ai_properties = schema_value["properties"]["perl"]["properties"]["aiCompletion"]["properties"]
for forbidden in [
    "enabled",
    "provider",
    "model",
    "endpoint",
    "apiKeyEnv",
    "apiKeyHeader",
    "apiKeyPrefix",
]:
    if forbidden in ai_properties:
        raise RuntimeError(f"{SCHEMA}: generic AI authority returned: {forbidden}")
'''
assert text.count(final_old) == 1, text.count(final_old)
text = text.replace(final_old, final_new)

marker = '''# ---------------------------------------------------------------------------
# Declarative authority: generic sources may tune safe preferences, but may not
# arm/select the remote backend.
# ---------------------------------------------------------------------------
'''
insertion = r'''replace_exact(
    CONFIG,
    '        let mut config = ServerConfig::default();\n'
    '        config.update_from_value(&serde_json::json!({\n'
    '            "aiCompletion": { "enabled": true }\n'
    '        }));\n',
    '        let mut config = ServerConfig::default();\n'
    '        config.ai_completion.user_enabled = true;\n'
    '        recompute_ai_completion_effective(&mut config.ai_completion);\n',
)

'''
assert text.count(marker) == 1, text.count(marker)
text = text.replace(marker, insertion + marker)

path.write_text(text, encoding="utf-8")
