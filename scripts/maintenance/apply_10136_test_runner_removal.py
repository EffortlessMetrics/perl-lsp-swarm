#!/usr/bin/env python3
"""One-shot branch edit for #10136.

This script is committed only to drive the branch-local edit from GitHub Actions.
The workflow removes it before publishing the resulting implementation commit.
Every transformation is count-checked so current-main drift fails closed.
"""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")


def replace_exact(path: str, old: str, new: str, count: int = 1) -> None:
    text = read(path)
    actual = text.count(old)
    if actual != count:
        raise RuntimeError(f"{path}: expected {count} exact matches, found {actual}: {old[:80]!r}")
    write(path, text.replace(old, new))


def sub_exact(
    path: str,
    pattern: str,
    replacement: str,
    count: int = 1,
    flags: int = 0,
) -> None:
    text = read(path)
    updated, actual = re.subn(pattern, replacement, text, count=count, flags=flags)
    if actual != count:
        raise RuntimeError(f"{path}: expected {count} regex matches, found {actual}: {pattern!r}")
    write(path, updated)


def sub_at_least(
    path: str,
    pattern: str,
    replacement: str,
    minimum: int,
    flags: int = 0,
) -> int:
    text = read(path)
    updated, actual = re.subn(pattern, replacement, text, flags=flags)
    if actual < minimum:
        raise RuntimeError(
            f"{path}: expected at least {minimum} regex matches, found {actual}: {pattern!r}"
        )
    write(path, updated)
    return actual


def assert_absent(path: str, needle: str) -> None:
    if needle in read(path):
        raise RuntimeError(f"{path}: forbidden current-surface marker remains: {needle!r}")


# ---------------------------------------------------------------------------
# Rust accepted state and parser
# ---------------------------------------------------------------------------

CONFIG = "crates/perl-lsp-rs-core/src/config/mod.rs"

replace_exact(
    CONFIG,
    "/// Runtime configuration for the LSP server features including inlay hints\n"
    "/// and test runner integration. Updated dynamically via `didChangeConfiguration`.\n",
    "/// Runtime configuration for LSP server features including inlay hints, diagnostics,\n"
    "/// formatting, and AI completion. Updated dynamically via `didChangeConfiguration`.\n",
)

replace_exact(
    CONFIG,
    '    /// Whether the integrated test runner is enabled.\n'
    '    pub test_runner_enabled: bool,\n'
    '    /// Command to execute tests (e.g., "perl", "prove").\n'
    '    pub test_runner_command: String,\n'
    '    /// Additional arguments passed to the test command.\n'
    '    pub test_runner_args: Vec<String>,\n'
    '    /// Test execution timeout in milliseconds.\n'
    '    pub test_runner_timeout: u64,\n\n',
    "",
)

replace_exact(
    CONFIG,
    '            test_runner_enabled: true,\n'
    '            test_runner_command: "perl".to_string(),\n'
    '            test_runner_args: vec![],\n'
    '            test_runner_timeout: 60000,\n',
    "",
)

replace_exact(
    CONFIG,
    '        if let Some(test) = settings.get("testRunner") {\n'
    '            if let Some(enabled) = test.get("enabled").and_then(|v| v.as_bool()) {\n'
    '                self.test_runner_enabled = enabled;\n'
    '            }\n'
    '            if let Some(cmd) = test.get("command").and_then(|v| v.as_str()) {\n'
    '                self.test_runner_command = cmd.to_string();\n'
    '            }\n'
    '            if let Some(args) = test.get("args").and_then(|v| v.as_array()) {\n'
    '                self.test_runner_args =\n'
    '                    args.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();\n'
    '            }\n'
    '            if let Some(timeout) = test.get("timeout").and_then(|v| v.as_u64()) {\n'
    '                self.test_runner_timeout = timeout;\n'
    '            }\n'
    '        }\n',
    '        if settings.get("testRunner").is_some() {\n'
    '            tracing::warn!(\n'
    '                target: "perl_lsp::config",\n'
    '                setting = "testRunner",\n'
    '                "ignoring removed LSP client testRunner configuration; test execution policy is server-owned",\n'
    '            );\n'
    '        }\n',
)

replace_exact(
    CONFIG,
    '        warn_on_type_mismatch(settings, "testRunner", "enabled", "boolean");\n',
    "",
)

replace_exact(
    CONFIG,
    '        assert!(!config.test_runner_enabled);\n'
    '        assert_eq!(config.test_runner_command, "prove");\n'
    '        assert_eq!(config.test_runner_args, vec!["-lv".to_string(), "t/unit.t".to_string()]);\n'
    '        assert_eq!(config.test_runner_timeout, 12_345);\n',
    "",
)

config_text = read(CONFIG)
new_config_test = r'''

    #[test]
    fn server_config_ignores_removed_test_runner_client_authority() {
        let mut config = ServerConfig::default();
        let before = serde_json::to_string(&config);

        config.update_from_value(&serde_json::json!({
            "testRunner": {
                "enabled": true,
                "command": "/tmp/attacker-wrapper",
                "args": ["--shell", "$(touch should-not-run)"],
                "timeout": u64::MAX
            }
        }));

        let after = serde_json::to_string(&config);
        assert!(before.is_ok(), "default ServerConfig should serialize");
        assert!(after.is_ok(), "updated ServerConfig should serialize");
        assert_eq!(before.ok(), after.ok(), "removed testRunner input must have zero state effect");
    }
'''
if "server_config_ignores_removed_test_runner_client_authority" in config_text:
    raise RuntimeError(f"{CONFIG}: one-shot test already present")
last_close = config_text.rfind("\n}")
if last_close < 0 or "#[cfg(test)]" not in config_text:
    raise RuntimeError(f"{CONFIG}: could not locate final test-module close")
write(CONFIG, config_text[:last_close] + new_config_test + config_text[last_close:])

# ---------------------------------------------------------------------------
# Configuration authority catalog and vocabulary
# ---------------------------------------------------------------------------

CATALOG = "crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs"
sub_exact(
    CATALOG,
    r'    authority!\(\n        "test\.(?:args|command|enabled|timeout_ms)",.*?\n    \),\n',
    "",
    count=4,
    flags=re.DOTALL,
)

AUTHORITY = "crates/perl-lsp-rs-core/src/configuration_authority/mod.rs"
replace_exact(AUTHORITY, "    ExecutableAndArgs,\n", "")
replace_exact(AUTHORITY, "    TestRunner,\n", "", count=2)
replace_exact(
    AUTHORITY,
    '            "test.command",\n            "test.args",\n',
    "",
)

# ---------------------------------------------------------------------------
# Runtime reflection
# ---------------------------------------------------------------------------

WORKSPACE = "crates/perl-lsp-rs/src/runtime/workspace.rs"
sub_exact(
    WORKSPACE,
    r'^\s+"perl\.testRunner\.(?:enabled|testCommand|testArgs|testTimeout)" => .*\n',
    "",
    count=4,
    flags=re.MULTILINE,
)
replace_exact(
    "crates/perl-lsp-rs/tests/lsp_smoke_e2e.rs",
    "    // Under default config (test_runner_enabled: true) the formatter runs, so edits\n",
    "    // Under the default formatting configuration the formatter runs, so edits\n",
)

# ---------------------------------------------------------------------------
# Generic schema
# ---------------------------------------------------------------------------

SCHEMA = "schemas/perllsp-settings.schema.json"
sub_exact(
    SCHEMA,
    r'\n        "testRunner": \{.*?\n        \},(?=\n        "limits":)',
    "",
    flags=re.DOTALL,
)

# ---------------------------------------------------------------------------
# Focused and collateral tests
# ---------------------------------------------------------------------------

SCHEMA_TEST = "crates/perl-lsp-rs-core/tests/perllsp_settings_schema_tests.rs"
replace_exact(SCHEMA_TEST, '        "testRunner",\n', "")
replace_exact(
    SCHEMA_TEST,
    '    assert_eq!(schema["properties"].get("perl-lsp").is_none(), true);\n',
    '    assert_eq!(properties.get("testRunner").is_none(), true);\n'
    '    assert_eq!(schema["properties"].get("perl-lsp").is_none(), true);\n',
)
replace_exact(
    SCHEMA_TEST,
    '    assert_eq!(server.test_runner_enabled, false);\n'
    '    assert_eq!(server.test_runner_command, "prove");\n'
    '    assert_eq!(server.test_runner_args, ["-lr", "t"]);\n'
    '    assert_eq!(server.test_runner_timeout, 90000);\n',
    "",
)

schema_test_text = read(SCHEMA_TEST)
new_schema_test = r'''

#[test]
fn generic_schema_withdraws_test_runner_client_authority() -> Result<(), Box<dyn Error>> {
    let schema = load_schema()?;
    let properties = &schema["properties"]["perl"]["properties"];
    assert!(properties.get("testRunner").is_none());

    let mut server = ServerConfig::default();
    let before = serde_json::to_string(&server)?;
    server.update_from_value(&json!({
        "testRunner": {
            "enabled": true,
            "command": "attacker-wrapper",
            "args": ["--arbitrary"],
            "timeout": u64::MAX
        }
    }));
    assert_eq!(serde_json::to_string(&server)?, before);
    Ok(())
}
'''
insert_before = "\n#[test]\nfn generic_schema_fields_are_behavior_backed_by_runtime_config()"
if insert_before not in schema_test_text:
    raise RuntimeError(f"{SCHEMA_TEST}: insertion marker missing")
write(SCHEMA_TEST, schema_test_text.replace(insert_before, new_schema_test + insert_before, 1))

replace_exact(
    "crates/perl-lsp-rs-core/tests/wave_final_absorption_tests.rs",
    '    assert!(config.test_runner_enabled);\n'
    '    assert_eq!(config.test_runner_command, "perl");\n',
    "",
)

RECURRENCE = "crates/perl-lsp-rs-core/tests/test_runner_client_authority_removed.rs"
recurrence_path = ROOT / RECURRENCE
if recurrence_path.exists():
    raise RuntimeError(f"{RECURRENCE}: path already exists")
recurrence_path.write_text(
    r'''use serde_json::Value;
use std::{error::Error, fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn generic_test_runner_process_authority_is_absent() -> Result<(), Box<dyn Error>> {
    let root = workspace_root();
    let config = fs::read_to_string(root.join("crates/perl-lsp-rs-core/src/config/mod.rs"))?;
    let reflection = fs::read_to_string(root.join("crates/perl-lsp-rs/src/runtime/workspace.rs"))?;
    let catalog = fs::read_to_string(
        root.join("crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs"),
    )?;
    let authority = fs::read_to_string(
        root.join("crates/perl-lsp-rs-core/src/configuration_authority/mod.rs"),
    )?;
    let schema_text = fs::read_to_string(root.join("schemas/perllsp-settings.schema.json"))?;
    let schema: Value = serde_json::from_str(&schema_text)?;

    for forbidden in [
        "pub test_runner_",
        "self.test_runner_",
        "test_runner_enabled:",
        "test_runner_command:",
        "test_runner_args:",
        "test_runner_timeout:",
    ] {
        assert!(!config.contains(forbidden), "removed config authority returned: {forbidden}");
    }

    assert!(!reflection.contains("perl.testRunner"));
    assert!(!catalog.contains("\"test."));
    assert!(!catalog.contains("Server.test_runner_"));
    assert!(!authority.contains("ExecutableAndArgs"));
    assert!(!authority.lines().any(|line| line.trim() == "TestRunner,"));
    assert!(schema["properties"]["perl"]["properties"].get("testRunner").is_none());

    for path in [
        "docs/reference/CONFIG.md",
        "docs/reference/CONFIGURATION.md",
        "docs/reference/CONFIGURATION_SCHEMA.md",
        "docs/how-to/PERFORMANCE_TUNING.md",
    ] {
        let current_doc = fs::read_to_string(root.join(path))?;
        assert!(!current_doc.contains("testRunner"), "current documentation restored {path}");
    }

    Ok(())
}
''',
    encoding="utf-8",
)

# ---------------------------------------------------------------------------
# Current documentation
# ---------------------------------------------------------------------------

CONFIG_DOC = "docs/reference/CONFIG.md"
replace_exact(CONFIG_DOC, "  - [perl.testRunner](#perltestrunner)\n", "")
replace_exact(CONFIG_DOC, '    "testRunner": { "command": "prove" },\n', "")
sub_exact(
    CONFIG_DOC,
    r'\n### perl\.testRunner\n.*?(?=\n### perl\.formatting\n)',
    "",
    flags=re.DOTALL,
)

SCHEMA_DOC = "docs/reference/CONFIGURATION_SCHEMA.md"
replace_exact(SCHEMA_DOC, "  - [Test Runner](#test-runner)\n", "")
replace_exact(SCHEMA_DOC, '    "testRunner": { ... },\n', "")
replace_exact(
    SCHEMA_DOC,
    '        "testRunner": {\n          "$ref": "#/definitions/testRunner"\n        },\n',
    "",
)
sub_exact(
    SCHEMA_DOC,
    r'\n    "testRunner": \{.*?\n    \},(?=\n    "formatting": \{)',
    "",
    flags=re.DOTALL,
)
sub_exact(
    SCHEMA_DOC,
    r'\n### Test Runner\n.*?(?=\n### Resource Limits\n)',
    "",
    flags=re.DOTALL,
)

USER_DOC = "docs/reference/CONFIGURATION.md"
replace_exact(
    USER_DOC,
    "You need to use a specific Perl binary (perlbrew, plenv, system Perl at a non-standard path) for running tests or the debugger.\n",
    "You need to use a specific Perl binary (perlbrew, plenv, system Perl at a non-standard path) for the debugger or the LSP process environment.\n",
)
sub_exact(
    USER_DOC,
    r'\n\*\*Test runner\*\*.*?(?=\n\*\*Shell approach\*\*)',
    "",
    flags=re.DOTALL,
)
removed_user_blocks = sub_at_least(
    USER_DOC,
    r'^    "testRunner": \{\n(?:      .*\n)+?    \},\n',
    "",
    minimum=2,
    flags=re.MULTILINE,
)
replace_exact(
    USER_DOC,
    "For a project that also uses critic checks in CI, use the `perl.perlcritic`\n"
    "settings together with the test runner. Add `perl.critic.engine = \"native\"` when\n"
    "the project is ready for native critic diagnostics:\n",
    "For a project that also uses critic checks in CI, use the `perl.perlcritic`\n"
    "settings. Add `perl.critic.engine = \"native\"` when the project is ready for\n"
    "native critic diagnostics:\n",
)
user_text = read(USER_DOC)
user_text = re.sub(r'^    \},\n(?=  \}\n\})', '    }\n', user_text, flags=re.MULTILINE)
write(USER_DOC, user_text)

PERF_DOC = "docs/how-to/PERFORMANCE_TUNING.md"
removed_perf_blocks = sub_at_least(
    PERF_DOC,
    r'^    "testRunner": \{\n(?:      .*\n)+?    \},\n',
    "",
    minimum=5,
    flags=re.MULTILINE,
)
perf_text = read(PERF_DOC)
perf_text = re.sub(r'^    \},\n(?=  \}\n\})', '    }\n', perf_text, flags=re.MULTILINE)
write(PERF_DOC, perf_text)

for current_doc in [CONFIG_DOC, SCHEMA_DOC, USER_DOC, PERF_DOC]:
    assert_absent(current_doc, "testRunner")

# Fix a zero-width typo in the newly compiled spec packet.
context_path = ".spec/10136-test-runner-client-authority/context.md"
context_text = read(context_path).replace("s\u200b\u200bchemas", "schemas")
write(context_path, context_text)

# ---------------------------------------------------------------------------
# Changelog
# ---------------------------------------------------------------------------

CHANGE = ".changes/unreleased/product-11845-Security-024800.yaml"
change_path = ROOT / CHANGE
if change_path.exists():
    raise RuntimeError(f"{CHANGE}: path already exists")
change_path.write_text(
    'project: product\n'
    'component: Language intelligence\n'
    'kind: Security\n'
    'body: "Removed the inert generic LSP testRunner configuration block so client, workspace, and project payloads can no longer store or reflect test executable, argument, enablement, or timeout authority. Future testing policy must bind a server-owned RunnerPlan to accepted configuration and hard resource envelopes."\n'
    'time: 2026-08-21T02:48:00Z\n'
    'custom:\n'
    '  PR: "11845"\n'
    '  Slug: remove-client-test-runner-authority\n'
    '  Breaking: "yes"\n',
    encoding="utf-8",
)

# ---------------------------------------------------------------------------
# Final source-level invariants before compilation
# ---------------------------------------------------------------------------

for path in [CONFIG, CATALOG, AUTHORITY, WORKSPACE, SCHEMA, SCHEMA_TEST]:
    assert "\r\n" not in read(path), f"{path}: unexpected CRLF rewrite"

assert_absent(CONFIG, "pub test_runner_")
assert_absent(CONFIG, "self.test_runner_")
assert_absent(CATALOG, '"test.')
assert_absent(WORKSPACE, "perl.testRunner")
assert_absent(SCHEMA, '"testRunner"')

print(
    "#10136 branch edit prepared:",
    f"removed {removed_user_blocks} user-guide blocks and {removed_perf_blocks} performance-guide blocks",
)
