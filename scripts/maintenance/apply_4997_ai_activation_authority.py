#!/usr/bin/env python3
"""Apply the bounded #4997 remote-AI activation-authority cutover.

This one-shot branch script exists only to apply a count-checked multi-file edit
inside GitHub Actions. The workflow removes it before publishing the resulting
implementation commit. Every source transformation fails closed on drift.
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
        raise RuntimeError(f"{path}: expected {count} exact matches, found {actual}: {old[:100]!r}")
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


def replace_function_body(path: str, marker: str, new_body: str) -> None:
    text = read(path)
    start = text.find(marker)
    if start < 0:
        raise RuntimeError(f"{path}: function marker missing: {marker}")
    open_brace = text.find("{", start)
    if open_brace < 0:
        raise RuntimeError(f"{path}: function body open brace missing: {marker}")
    depth = 0
    close_brace = -1
    for idx in range(open_brace, len(text)):
        char = text[idx]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                close_brace = idx
                break
    if close_brace < 0:
        raise RuntimeError(f"{path}: function close brace missing: {marker}")
    write(path, text[: open_brace + 1] + "\n" + new_body.rstrip() + "\n    " + text[close_brace:])


def assert_absent(path: str, needle: str) -> None:
    if needle in read(path):
        raise RuntimeError(f"{path}: forbidden marker remains: {needle!r}")


# ---------------------------------------------------------------------------
# Generic configuration: arm/select fields are not accepted authority.
# ---------------------------------------------------------------------------

CONFIG = "crates/perl-lsp-rs-core/src/config/mod.rs"

replace_exact(
    CONFIG,
    "    /// Whether the user explicitly enabled AI completions via the LSP client\n"
    "    /// configuration channel. Default: false.\n",
    "    /// Whether a server-owned trusted user/operator adapter enabled AI completions.\n"
    "    /// Generic LSP configuration cannot set this field. Default: false.\n",
)
replace_exact(
    CONFIG,
    '    /// Provider type. Currently only "openai_compat" is supported.\n',
    '    /// Trusted provider type. Currently only "openai_compat" is supported.\n',
)
replace_exact(
    CONFIG,
    '    /// Model identifier (e.g., "gpt-4o-mini").\n',
    '    /// Trusted model identifier (e.g., "gpt-4o-mini").\n',
)

replace_exact(
    CONFIG,
    '            if let Some(enabled) = ai.get("enabled").and_then(|v| v.as_bool()) {\n'
    '                self.ai_completion.user_enabled = enabled;\n'
    '            }\n'
    '            if let Some(provider) = ai.get("provider").and_then(|v| v.as_str()) {\n'
    '                self.ai_completion.provider = provider.to_string();\n'
    '            }\n',
    '            if ai.get("enabled").is_some() {\n'
    '                tracing::warn!(\n'
    '                    target: "perl_lsp::config",\n'
    '                    setting = "aiCompletion.enabled",\n'
    '                    "ignoring AI activation from generic LSP settings; activation requires a server-owned trusted user/operator adapter (#4997)",\n'
    '                );\n'
    '            }\n'
    '            if ai.get("provider").is_some() {\n'
    '                tracing::warn!(\n'
    '                    target: "perl_lsp::config",\n'
    '                    setting = "aiCompletion.provider",\n'
    '                    "ignoring AI provider selection from generic LSP settings; selection requires trusted activation authority (#4997)",\n'
    '                );\n'
    '            }\n',
)
replace_exact(
    CONFIG,
    '            if let Some(model) = ai.get("model").and_then(|v| v.as_str()) {\n'
    '                self.ai_completion.model = model.to_string();\n'
    '            }\n',
    '            if ai.get("model").is_some() {\n'
    '                tracing::warn!(\n'
    '                    target: "perl_lsp::config",\n'
    '                    setting = "aiCompletion.model",\n'
    '                    "ignoring AI model selection from generic LSP settings; selection requires trusted activation authority (#4997)",\n'
    '                );\n'
    '            }\n',
)

# Replace the historical test that pinned generic arm/select behavior.
sub_exact(
    CONFIG,
    r'    #\[test\]\n    fn workspace_ai_completion_ignores_untrusted_endpoint_and_credential_settings\(\) \{.*?\n    \}\n(?=\n    #\[test\])',
    '''    #[test]
    fn generic_ai_completion_cannot_arm_or_select_but_safe_preferences_still_apply() {
        let mut config = ServerConfig::default();
        config.ai_completion.user_enabled = true;
        config.ai_completion.provider = "trusted-provider".to_string();
        config.ai_completion.model = "trusted-model".to_string();
        recompute_ai_completion_effective(&mut config.ai_completion);

        config.update_from_value(&serde_json::json!({
            "aiCompletion": {
                "enabled": false,
                "provider": "attacker-provider",
                "model": "attacker-model",
                "fallback": false,
                "streaming": { "updateDebounceMs": 91 }
            }
        }));

        assert!(config.ai_completion.user_enabled);
        assert!(config.ai_completion.enabled);
        assert_eq!(config.ai_completion.provider, "trusted-provider");
        assert_eq!(config.ai_completion.model, "trusted-model");
        assert!(!config.ai_completion.fallback);
        assert_eq!(config.ai_completion.streaming.update_debounce_ms, 91);
    }
''',
    flags=re.DOTALL,
)

# ---------------------------------------------------------------------------
# Declarative authority: generic sources may tune safe preferences, but may not
# arm/select the remote backend.
# ---------------------------------------------------------------------------

CATALOG = "crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs"
replace_exact(
    CATALOG,
    '/// User-channel AI inputs with the project file removed (issues #4955, #4997):\n'
    '/// `.perl-lsp.toml` may only opt out of AI completions; it may never arm the\n'
    '/// backend or select provider/model, so it is not an input source for any AI\n'
    '/// arm/select field. InitializationOptions/GlobalClientSettings remain listed\n'
    '/// because the current runtime still honours them via `update_from_value`;\n'
    '/// tightening that provenance is the remaining #4997 server-side slice.\n'
    'const AI_USER_CHANNEL: &[Source] =\n'
    '    &[Source::CompiledDefault, Source::InitializationOptions, Source::GlobalClientSettings];\n'
    '/// Sources of the DERIVED `ai.effective_enabled` value: the user channel plus\n'
    '/// `ProjectFile`, because `.perl-lsp.toml enabled = false` feeds\n'
    '/// `project_opt_out` and recomputes\n'
    '/// `enabled = user_enabled && !project_opt_out`. The project file contributes\n'
    '/// here only as a reducer into that derivation; it still has no arm/select\n'
    '/// authority over any direct AI activation row (issues #4955, #4997).\n'
    'const AI_EFFECTIVE_ENABLED_SOURCES: &[Source] = &[\n'
    '    Source::CompiledDefault,\n'
    '    Source::InitializationOptions,\n'
    '    Source::ProjectFile,\n'
    '    Source::GlobalClientSettings,\n'
    '];\n',
    '/// Generic AI preference/resource inputs. These may tune non-activating\n'
    '/// scheduling and presentation fields only; they carry no arm/select authority.\n'
    'const AI_USER_CHANNEL: &[Source] =\n'
    '    &[Source::CompiledDefault, Source::InitializationOptions, Source::GlobalClientSettings];\n'
    '/// Remote AI activation and request-identity selection require an independently\n'
    '/// admitted server-owned trusted-user/operator observation (#4997).\n'
    'const AI_TRUSTED_ARM_SELECT: &[Source] =\n'
    '    &[Source::CompiledDefault, Source::TrustedUserSettings];\n'
    '/// The effective flag derives from trusted activation plus the project-file\n'
    '/// opt-out reducer. Project input can only make the result less permissive.\n'
    'const AI_EFFECTIVE_ENABLED_SOURCES: &[Source] = &[\n'
    '    Source::CompiledDefault,\n'
    '    Source::ProjectFile,\n'
    '    Source::TrustedUserSettings,\n'
    '];\n',
)

for field_id in ["ai.user_enabled", "ai.provider", "ai.model"]:
    pattern = rf'(        "{re.escape(field_id)}",.*?\n        )AI_USER_CHANNEL,'
    sub_exact(CATALOG, pattern, r'\1AI_TRUSTED_ARM_SELECT,', flags=re.DOTALL)

# ---------------------------------------------------------------------------
# Generic schema: remove arm/select fields while retaining safe preferences.
# ---------------------------------------------------------------------------

SCHEMA = "schemas/perllsp-settings.schema.json"
replace_exact(
    SCHEMA,
    '          "description": "AI-powered inline completion configuration. Sensitive destination/credential fields are intentionally excluded from LSP client settings.",\n',
    '          "description": "Non-activating AI inline-completion preferences and resource requests. Remote activation, provider, model, destination, and credentials require a server-owned trusted user/operator adapter and are intentionally excluded from generic LSP settings.",\n',
)
for line in [
    '            "enabled": { "type": "boolean", "default": false },\n',
    '            "provider": { "type": "string", "default": "openai_compat" },\n',
    '            "model": { "type": "string", "default": "gpt-4o-mini" },\n',
]:
    replace_exact(SCHEMA, line, "")

SCHEMA_TEST = "crates/perl-lsp-rs-core/tests/perllsp_settings_schema_tests.rs"
replace_exact(
    SCHEMA_TEST,
    '            "enabled": true,\n'
    '            "provider": "openai_compat",\n'
    '            "model": "fixture-model",\n',
    "",
)
replace_exact(
    SCHEMA_TEST,
    '    assert!(server.ai_completion.user_enabled);\n'
    '    assert_eq!(server.ai_completion.provider, "openai_compat");\n'
    '    assert_eq!(server.ai_completion.model, "fixture-model");\n',
    '    assert!(!server.ai_completion.user_enabled);\n'
    '    assert!(!server.ai_completion.enabled);\n',
)
replace_exact(
    SCHEMA_TEST,
    '    for sensitive in ["endpoint", "apiKeyEnv", "apiKeyHeader", "apiKeyPrefix"] {\n',
    '    for sensitive in [\n'
    '        "enabled",\n'
    '        "provider",\n'
    '        "model",\n'
    '        "endpoint",\n'
    '        "apiKeyEnv",\n'
    '        "apiKeyHeader",\n'
    '        "apiKeyPrefix",\n'
    '    ] {\n',
)

# ---------------------------------------------------------------------------
# Runtime authority subject and authority-bound backend wrapper.
# ---------------------------------------------------------------------------

RUNTIME = "crates/perl-lsp-rs/src/runtime/mod.rs"
TYPE_MARKER = "/// LSP server that handles JSON-RPC communication\npub struct LspServer {"
TYPE_INSERT = r'''/// Server-owned authority for remote AI activation and request identity.
///
/// No production generic client channel can construct `TrustedUserOperator`.
/// The temporary generation token is deliberately replaceable by the accepted
/// configuration subject owned by #10817/#10387/#10909.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AiActivationAuthority {
    /// No independently admitted user/operator activation exists.
    #[default]
    Unavailable,
    /// An exact server-owned adapter admitted activation for one generation.
    TrustedUserOperator {
        adapter: &'static str,
        generation: u64,
    },
}

impl AiActivationAuthority {
    const fn is_trusted(self) -> bool {
        matches!(self, Self::TrustedUserOperator { .. })
    }

    const fn adapter(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::TrustedUserOperator { adapter, .. } => adapter,
        }
    }

    const fn generation(self) -> u64 {
        match self {
            Self::Unavailable => 0,
            Self::TrustedUserOperator { generation, .. } => generation,
        }
    }
}

/// Backend wrapper that rechecks activation immediately before transport work.
///
/// A caller may retain the returned `Arc`, but it cannot use that stale object
/// after authority replacement, project opt-out, or effective disablement.
struct AuthorityBoundAiBackend {
    inner: Arc<dyn perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend>,
    expected_authority: AiActivationAuthority,
    current_authority: Arc<Mutex<AiActivationAuthority>>,
    config: Arc<Mutex<ServerConfig>>,
}

impl perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend
    for AuthorityBoundAiBackend
{
    fn stream(
        &self,
        req: &perl_lsp_rs_core::providers::inline_completion::BackendRequest,
        sink: &mut dyn FnMut(
            perl_lsp_rs_core::providers::inline_completion::StreamChunk,
        ) -> perl_lsp_rs_core::providers::inline_completion::StreamControl,
    ) -> Result<(), perl_lsp_rs_core::providers::inline_completion::BackendError> {
        let current = *self.current_authority.lock();
        let effective_enabled = self.config.lock().ai_completion.enabled;
        if !self.expected_authority.is_trusted()
            || current != self.expected_authority
            || !effective_enabled
        {
            return Err(
                perl_lsp_rs_core::providers::inline_completion::BackendError::Cancelled,
            );
        }
        self.inner.stream(req, sink)
    }
}

'''
replace_exact(RUNTIME, TYPE_MARKER, TYPE_INSERT + TYPE_MARKER)

replace_exact(
    RUNTIME,
    '    /// Optional AI inline-completion backend.\n'
    '    ///\n'
    '    /// When `Some`, the `handle_inline_completion` handler will attempt\n'
    '    /// AI-backed completions before falling back to deterministic rules.\n'
    '    /// Set to `None` by default; a backend can be registered later.\n'
    '    pub(crate) ai_inline_backend: Mutex<\n'
    '        Option<Arc<dyn perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend>>,\n'
    '    >,\n',
    '    /// Current server-owned AI activation authority. Generic client payloads\n'
    '    /// cannot mutate this state.\n'
    '    pub(crate) ai_activation_authority: Arc<Mutex<AiActivationAuthority>>,\n'
    '    /// Optional authority-bound AI inline-completion backend.\n'
    '    ///\n'
    '    /// Every stored backend is wrapped so a retained `Arc` rechecks the exact\n'
    '    /// activation generation and effective opt-out state before transport work.\n'
    '    pub(crate) ai_inline_backend: Mutex<\n'
    '        Option<Arc<dyn perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend>>,\n'
    '    >,\n',
)

replace_exact(
    RUNTIME,
    '    /// Get the registered AI inline-completion backend, if any.\n'
    '    ///\n'
    '    /// Returns `None` when no backend has been registered (the default).\n'
    '    /// The returned `Arc` is a cheap clone suitable for use outside the lock.\n'
    '    pub(crate) fn ai_backend(\n'
    '        &self,\n'
    '    ) -> Option<Arc<dyn perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend>>\n'
    '    {\n'
    '        self.ai_inline_backend.lock().clone()\n'
    '    }\n',
    '    /// Get the registered authority-bound AI inline-completion backend.\n'
    '    ///\n'
    '    /// The returned object performs the final authority/currentness check in\n'
    '    /// its `stream` implementation immediately before delegating to transport.\n'
    '    pub(crate) fn ai_backend(\n'
    '        &self,\n'
    '    ) -> Option<Arc<dyn perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend>>\n'
    '    {\n'
    '        self.ai_inline_backend.lock().clone()\n'
    '    }\n'
    '\n'
    '    fn install_ai_backend_for_authority(\n'
    '        &self,\n'
    '        backend: Option<\n'
    '            Arc<dyn perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend>,\n'
    '        >,\n'
    '        authority: AiActivationAuthority,\n'
    '    ) {\n'
    '        let guarded = backend.map(|inner| {\n'
    '            Arc::new(AuthorityBoundAiBackend {\n'
    '                inner,\n'
    '                expected_authority: authority,\n'
    '                current_authority: Arc::clone(&self.ai_activation_authority),\n'
    '                config: Arc::clone(&self.config),\n'
    '            })\n'
    '                as Arc<\n'
    '                    dyn perl_lsp_rs_core::providers::inline_completion::InlineCompletionBackend,\n'
    '                >\n'
    '        });\n'
    '        *self.ai_inline_backend.lock() = guarded;\n'
    '    }\n',
)

sub_exact(
    RUNTIME,
    r'    /// Refresh the AI inline-completion backend based on current configuration\..*?\n    pub\(crate\) fn refresh_ai_backend\(&self\) \{.*?\n    \}\n(?=\n    /// Get the subprocess runtime)',
    r'''    /// Refresh the AI inline-completion backend from current configuration.
    ///
    /// Construction requires both effective enablement and an independently
    /// admitted server-owned authority. Generic LSP configuration can change
    /// non-activating preferences but cannot satisfy this gate (#4997).
    pub(crate) fn refresh_ai_backend(&self) {
        let ai_config = self.config.lock().ai_completion.clone();
        let authority = *self.ai_activation_authority.lock();

        if !ai_config.enabled || !authority.is_trusted() {
            *self.ai_inline_backend.lock() = None;
            return;
        }

        let Some(api_key) = Self::resolve_ai_api_key(&ai_config) else {
            tracing::warn!(
                env_var = %ai_config.api_key_env,
                "AI completion has trusted activation but the API key source is empty or unset"
            );
            *self.ai_inline_backend.lock() = None;
            return;
        };

        let mut provider_config = perl_lsp_rs_core::providers::ai::OpenAiConfig::new(
            ai_config.endpoint.clone(),
            ai_config.model.clone(),
            api_key,
            ai_config.timeout_ms,
        );
        provider_config.api_key_header = ai_config.api_key_header.clone();
        provider_config.api_key_prefix = ai_config.api_key_prefix.clone();
        provider_config.local_model_mode = ai_config.local_model_mode;

        let limiter = Arc::new(perl_lsp_rs_core::providers::ai::RateLimiter::new(
            ai_config.rate_limit_rps,
            ai_config.max_inflight,
        ));
        let provider =
            perl_lsp_rs_core::providers::ai::OpenAiProvider::new(provider_config, limiter);
        self.install_ai_backend_for_authority(Some(Arc::new(provider)), authority);

        tracing::info!(
            authority_adapter = authority.adapter(),
            authority_generation = authority.generation(),
            "AI inline completion backend configured under trusted authority"
        );
    }
''',
    flags=re.DOTALL,
)

CONSTRUCTORS = "crates/perl-lsp-rs/src/runtime/constructors.rs"
replace_exact(
    CONSTRUCTORS,
    '            ai_inline_backend: Mutex::new(None),\n',
    '            ai_activation_authority: Arc::new(Mutex::new(\n'
    '                super::AiActivationAuthority::Unavailable,\n'
    '            )),\n'
    '            ai_inline_backend: Mutex::new(None),\n',
    count=2,
)

# ---------------------------------------------------------------------------
# Test-only trusted adapter and generic-channel helpers.
# ---------------------------------------------------------------------------

TEST_API = "crates/perl-lsp-rs/src/runtime/test_api.rs"
replace_function_body(
    TEST_API,
    "    pub fn test_configure_ai_completion",
    '''        let mut authority = self.ai_activation_authority.lock();
        let next_generation = match *authority {
            super::AiActivationAuthority::Unavailable => 1,
            super::AiActivationAuthority::TrustedUserOperator { generation, .. } => {
                generation.saturating_add(1)
            }
        };
        *authority = if enabled {
            super::AiActivationAuthority::TrustedUserOperator {
                adapter: "expose_lsp_test_api",
                generation: next_generation,
            }
        } else {
            super::AiActivationAuthority::Unavailable
        };
        drop(authority);

        let mut config = self.config.lock();
        config.ai_completion.user_enabled = enabled;
        config.ai_completion.fallback = fallback;
        recompute_ai_completion_effective(&mut config.ai_completion);''',
)
replace_function_body(
    TEST_API,
    "    pub fn test_install_ai_backend",
    '''        let authority = *self.ai_activation_authority.lock();
        self.install_ai_backend_for_authority(backend, authority);''',
)

TEST_API_APPEND = r'''

    /// Apply an untrusted generic LSP `aiCompletion` object and run the same
    /// backend refresh performed by configuration notifications.
    pub fn test_apply_generic_ai_completion_settings(&self, settings: Value) {
        self.config
            .lock()
            .update_from_value(&serde_json::json!({ "aiCompletion": settings }));
        self.refresh_ai_backend();
    }

    /// Seed a valid-looking transport subject without granting activation.
    pub fn test_seed_ai_transport(
        &self,
        endpoint: &str,
        api_key_env: &str,
        timeout_ms: u64,
        local_model_mode: bool,
    ) {
        let mut config = self.config.lock();
        config.ai_completion.endpoint = endpoint.to_string();
        config.ai_completion.api_key_env = api_key_env.to_string();
        config.ai_completion.provider = "openai_compat".to_string();
        config.ai_completion.model = "authority-test-model".to_string();
        config.ai_completion.timeout_ms = timeout_ms;
        config.ai_completion.local_model_mode = local_model_mode;
    }

    /// Re-run the production backend construction choke point.
    pub fn test_refresh_ai_backend(&self) {
        self.refresh_ai_backend();
    }

    /// Whether production construction currently retains a backend wrapper.
    pub fn test_ai_backend_available(&self) -> bool {
        self.ai_inline_backend.lock().is_some()
    }

    /// Apply the project opt-out reducer without changing trusted authority.
    pub fn test_set_ai_project_opt_out(&self, opted_out: bool) {
        let mut config = self.config.lock();
        config.ai_completion.project_opt_out = opted_out;
        recompute_ai_completion_effective(&mut config.ai_completion);
    }
'''
test_api_text = read(TEST_API)
if "test_apply_generic_ai_completion_settings" in test_api_text:
    raise RuntimeError(f"{TEST_API}: one-shot helpers already present")
impl_end_marker = (
    "\n}\n\n/// Public snapshot of the installed parse worker's counters (test-only)."
)
impl_end = test_api_text.find(impl_end_marker)
if impl_end < 0:
    raise RuntimeError(f"{TEST_API}: public test-only LspServer impl close missing")
write(TEST_API, test_api_text[:impl_end] + TEST_API_APPEND + test_api_text[impl_end:])

# ---------------------------------------------------------------------------
# Runtime first-effect tests: generic, construction/network, stale authority,
# project reduction, and trusted positive control.
# ---------------------------------------------------------------------------

AI_TEST = "crates/perl-lsp-rs/tests/lsp_ai_inline_completion_tests.rs"
AI_TEST_APPEND = r'''

// ── #4997 activation-authority and first-egress controls ───────────────────

fn invoked_inline_completion_with_context(
    server: &LspServer,
    uri: &str,
    line: u32,
    character: u32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let request = JsonRpcRequest {
        _jsonrpc: "2.0".into(),
        id: Some(perl_lsp::protocol::JsonRpcId::Integer(4997_i64)),
        method: "textDocument/inlineCompletion".into(),
        params: Some(json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "triggerKind": 1 }
        })),
    };
    let response = server.handle_request(request).ok_or("inline completion response")?;
    response.result.ok_or("result field present".into())
}

#[test]
fn generic_ai_settings_cannot_call_an_injected_backend()
-> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_generic_authority.pl";
    open_doc(&server, uri, "use str");

    server.test_apply_generic_ai_completion_settings(json!({
        "enabled": true,
        "provider": "attacker-provider",
        "model": "attacker-model",
        "fallback": true,
        "streaming": { "enabled": true, "updateDebounceMs": 17 }
    }));

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    server
        .test_install_ai_backend(Some(Arc::new(CountingSlowBackend { calls: Arc::clone(&calls) })));

    let result = invoked_inline_completion_with_context(&server, uri, 0, 7)?;
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "generic LSP configuration must not authorize the backend"
    );
    let texts: Vec<&str> = result["items"]
        .as_array()
        .ok_or("items array")?
        .iter()
        .filter_map(|item| item["insertText"].as_str())
        .collect();
    assert!(texts.contains(&"strict;"), "deterministic fallback must remain available");
    Ok(())
}

#[test]
fn generic_ai_settings_construct_no_backend_and_open_no_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let endpoint = format!(
        "http://127.0.0.1:{}/v1/chat/completions",
        listener.local_addr()?.port()
    );

    let server = setup_server()?;
    let uri = "file:///ai_generic_zero_egress.pl";
    open_doc(&server, uri, "use str");
    server.test_seed_ai_transport(&endpoint, "PATH", 100, true);
    server.test_apply_generic_ai_completion_settings(json!({
        "enabled": true,
        "provider": "openai_compat",
        "model": "would-egress-without-authority",
        "fallback": true
    }));

    assert!(
        !server.test_ai_backend_available(),
        "generic activation must not leave an egress-authorized backend"
    );
    let _ = invoked_inline_completion_with_context(&server, uri, 0, 7)?;
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(_) => return Err("generic activation opened the network socket".into()),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[test]
fn stale_trusted_backend_cannot_run_after_authority_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_stale_authority.pl";
    open_doc(&server, uri, "use str");

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    server.test_configure_ai_completion(true, true);
    server
        .test_install_ai_backend(Some(Arc::new(CountingSlowBackend { calls: Arc::clone(&calls) })));

    server.test_configure_ai_completion(false, true);
    server.test_configure_ai_completion(true, true);
    let _ = invoked_inline_completion_with_context(&server, uri, 0, 7)?;
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a retained backend Arc must not survive authority-generation replacement"
    );
    Ok(())
}

#[test]
fn project_opt_out_blocks_a_retained_trusted_backend_at_first_effect()
-> Result<(), Box<dyn std::error::Error>> {
    let server = setup_server()?;
    let uri = "file:///ai_project_reducer.pl";
    open_doc(&server, uri, "use str");

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    server.test_configure_ai_completion(true, true);
    server
        .test_install_ai_backend(Some(Arc::new(CountingSlowBackend { calls: Arc::clone(&calls) })));
    server.test_set_ai_project_opt_out(true);

    let _ = invoked_inline_completion_with_context(&server, uri, 0, 7)?;
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "project opt-out must reduce a previously trusted capability"
    );
    Ok(())
}
'''
ai_test_text = read(AI_TEST)
if "generic_ai_settings_cannot_call_an_injected_backend" in ai_test_text:
    raise RuntimeError(f"{AI_TEST}: #4997 tests already present")
write(AI_TEST, ai_test_text.rstrip() + AI_TEST_APPEND + "\n")

# ---------------------------------------------------------------------------
# Structural recurrence: parser, catalog, schema, runtime and docs must agree.
# ---------------------------------------------------------------------------

RECURRENCE = "crates/perl-lsp-rs-core/tests/ai_activation_authority_contract.rs"
recurrence_path = ROOT / RECURRENCE
if recurrence_path.exists():
    raise RuntimeError(f"{RECURRENCE}: path already exists")
recurrence_path.write_text(
    r'''use serde_json::Value;
use std::{error::Error, fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn generic_lsp_cannot_regain_remote_ai_arm_or_selection_authority()
-> Result<(), Box<dyn Error>> {
    let root = root();
    let config = fs::read_to_string(root.join("crates/perl-lsp-rs-core/src/config/mod.rs"))?;
    let catalog = fs::read_to_string(
        root.join("crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs"),
    )?;
    let runtime = fs::read_to_string(root.join("crates/perl-lsp-rs/src/runtime/mod.rs"))?;
    let schema_text = fs::read_to_string(root.join("schemas/perllsp-settings.schema.json"))?;
    let schema: Value = serde_json::from_str(&schema_text)?;

    for forbidden in [
        "self.ai_completion.user_enabled = enabled",
        "self.ai_completion.provider = provider.to_string()",
        "self.ai_completion.model = model.to_string()",
    ] {
        assert!(!config.contains(forbidden), "generic parser authority returned: {forbidden}");
    }

    assert!(catalog.contains("const AI_TRUSTED_ARM_SELECT"));
    for id in ["ai.user_enabled", "ai.provider", "ai.model"] {
        let start = catalog.find(&format!("\"{id}\"")).ok_or("catalog row")?;
        let tail = &catalog[start..];
        let end = tail.find("    ),").ok_or("catalog row end")?;
        assert!(
            tail[..end].contains("AI_TRUSTED_ARM_SELECT"),
            "{id} must use trusted arm/select sources"
        );
    }

    let ai = &schema["properties"]["perl"]["properties"]["aiCompletion"]["properties"];
    for forbidden in [
        "enabled",
        "provider",
        "model",
        "endpoint",
        "apiKeyEnv",
        "apiKeyHeader",
        "apiKeyPrefix",
    ] {
        assert!(ai.get(forbidden).is_none(), "generic schema restored {forbidden}");
    }

    assert!(runtime.contains("enum AiActivationAuthority"));
    assert!(runtime.contains("struct AuthorityBoundAiBackend"));
    assert!(runtime.contains("current != self.expected_authority"));
    assert!(runtime.contains("!effective_enabled"));
    assert!(runtime.contains("!authority.is_trusted()"));

    let ai_doc = fs::read_to_string(root.join("docs/reference/AI_COMPLETION.md"))?;
    assert!(ai_doc.contains("unavailable until a trusted user/operator adapter"));
    assert!(!ai_doc.contains("Set `aiCompletion.enabled` to `true`"));

    let extension_readme = fs::read_to_string(root.join("vscode-extension/README.md"))?;
    assert!(!extension_readme.contains("To enable it, set `perl-lsp.aiCompletion.enabled`"));
    Ok(())
}
''',
    encoding="utf-8",
)

# ---------------------------------------------------------------------------
# Public/current documentation and extension-setting descriptions.
# ---------------------------------------------------------------------------

AI_DOC = "docs/reference/AI_COMPLETION.md"
write(
    AI_DOC,
    r'''# AI Inline Completion

## Current product state

The repository contains an OpenAI-compatible backend, streaming parser, rate
limiter, and deterministic fallback. **Production remote backend activation is
unavailable until a trusted user/operator adapter can give the server an
unforgeable activation subject.**

This is deliberate security containment for #4997. Generic LSP transports do not
preserve whether a value came from user, machine, workspace, folder, project, or
a mixed client merge. Therefore none of these generic fields can arm or select a
remote backend:

```text
aiCompletion.enabled
aiCompletion.provider
aiCompletion.model
aiCompletion.endpoint
aiCompletion.apiKeyEnv
aiCompletion.apiKeyHeader
aiCompletion.apiKeyPrefix
```

The generic settings schema exposes only non-activating preferences and resource
requests. Project `.perl-lsp.toml` may opt out with:

```toml
[ai_completion]
enabled = false
```

Project configuration cannot enable the backend or choose provider, model,
destination, or credentials.

## VS Code setting

`perl-lsp.aiCompletion.enabled` remains a machine-scoped client preference and a
reserved migration surface. It prevents committed workspace settings from
changing the client-side gate, but it is **not** server-side proof of trusted
activation and currently does not make remote completion available.

`perl-lsp.aiCompletion.streaming.enabled` is likewise a machine-scoped
presentation preference. Streaming preference never authorizes network access.

## What still works

`textDocument/inlineCompletion` continues to provide deterministic, local
suggestions. Automatic requests remain local-first. The test-only
`expose_lsp_test_api` adapter can admit a private trusted authority so provider,
fallback, cancellation, and streaming behavior remain directly tested; that seam
is not production configuration.

## Future activation contract

#10817/#10387 will carry typed source observations and accepted configuration
subjects. #10909 will bind backend construction, session/cache identity, retries,
streams, and the last pre-network check to that accepted generation. A future
trusted adapter must prove all of the following before remote activation becomes
available:

```text
server-owned adapter identity
accepted user/operator source
exact configuration generation
project/root eligibility
trusted provider/model/destination/credential source
hard network and resource envelope
final currentness immediately before first egress
```

Client name, configuration-array position, manifest scope text, URI, or a
payload-supplied `trusted` flag cannot satisfy this contract.

## Security boundaries

Destination validation, redirect credential binding, and response/concurrency
limits remain with #5004/#4955 after legitimate activation. Consent UX remains
#5049. This document does not claim those programmes complete.
''',
)

README = "vscode-extension/README.md"
sub_exact(
    README,
    r'### AI Completion\n.*?(?=\n### Quick Start: Demo Project)',
    '''### AI Completion

The extension retains machine-scoped AI and streaming preferences so a committed
workspace cannot arm them. Remote AI activation is currently unavailable while
the server-side trusted user/operator adapter is being built. The setting alone
does not authorize network access; local deterministic inline completion remains
available. See the [AI completion contract](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/reference/AI_COMPLETION.md).
''',
    flags=re.DOTALL,
)

EXTENSION_DOC = "docs/EXTENSION.md"
replace_exact(
    EXTENSION_DOC,
    '| `perl-lsp.aiCompletion.enabled` | `false` | Enable AI-assisted inline completion. |\n',
    '| `perl-lsp.aiCompletion.enabled` | `false` | Reserved machine-scoped client preference; remote server activation remains unavailable until a trusted user/operator adapter lands. |\n',
)

PACKAGE = "vscode-extension/package.json"
replace_exact(
    PACKAGE,
    '            "description": "Enable AI-powered inline code suggestions. Off by default; requires a language server that advertises inline-completion support. See the README for configuration.",\n'
    '            "markdownDescription": "Enable AI-powered inline code suggestions. Off by default; requires a language server that advertises inline-completion support. See the [README](https://github.com/EffortlessMetrics/perl-lsp/tree/master/vscode-extension#ai-completion) for configuration."\n',
    '            "description": "Reserved machine-scoped AI preference. Remote server activation is currently unavailable until a trusted user/operator adapter lands; deterministic inline completion remains available.",\n'
    '            "markdownDescription": "Reserved machine-scoped AI preference. Remote server activation is currently unavailable until a trusted user/operator adapter lands; deterministic inline completion remains available. See the [AI completion contract](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/reference/AI_COMPLETION.md)."\n',
)
replace_exact(
    PACKAGE,
    '            "description": "Enable progressive streaming for AI completions so results appear as they are generated. Requires aiCompletion.enabled to be true.",\n'
    '            "markdownDescription": "Enable progressive streaming for AI completions so results appear as they are generated. Requires `aiCompletion.enabled` to be true."\n',
    '            "description": "Reserved machine-scoped streaming preference. It never authorizes remote backend activation.",\n'
    '            "markdownDescription": "Reserved machine-scoped streaming preference. It never authorizes remote backend activation; see the [AI completion contract](https://github.com/EffortlessMetrics/perl-lsp/blob/master/docs/reference/AI_COMPLETION.md)."\n',
)

INLINE_DOC = "docs/reference/INLINE_COMPLETION_CONTRACTS.md"
replace_exact(
    INLINE_DOC,
    'See [AI_COMPLETION.md](AI_COMPLETION.md): `perl-lsp.aiCompletion.enabled`\n'
    '(default `false`), timeout, and token controls.\n',
    'See [AI_COMPLETION.md](AI_COMPLETION.md). Production remote activation is\n'
    'currently unavailable until a server-owned trusted user/operator adapter\n'
    'lands; generic LSP settings expose only non-activating preferences and\n'
    'resource requests. Deterministic inline completion remains available.\n',
)

# ---------------------------------------------------------------------------
# Changelog and final invariants.
# ---------------------------------------------------------------------------

CHANGE = ".changes/unreleased/product-11951-Security-230900.yaml"
change_path = ROOT / CHANGE
if change_path.exists():
    raise RuntimeError(f"{CHANGE}: path already exists")
change_path.write_text(
    'project: product\n'
    'component: Language intelligence\n'
    'kind: Security\n'
    'body: "Generic LSP configuration can no longer activate a remote AI backend or select its provider/model identity. Every backend is bound to a server-owned authority generation and rechecks that generation plus project opt-out state immediately before transport work. Remote AI activation remains unavailable until a trusted user/operator adapter lands; deterministic inline completion remains available."\n'
    'time: 2026-08-22T23:09:00Z\n'
    'custom:\n'
    '  PR: "11951"\n'
    '  Slug: ai-activation-authority\n'
    '  Breaking: "yes"\n',
    encoding="utf-8",
)

for path in [CONFIG, CATALOG, RUNTIME, CONSTRUCTORS, TEST_API, AI_TEST, SCHEMA_TEST]:
    if "\r\n" in read(path):
        raise RuntimeError(f"{path}: unexpected CRLF rewrite")

assert_absent(CONFIG, "self.ai_completion.user_enabled = enabled")
assert_absent(CONFIG, "self.ai_completion.provider = provider.to_string()")
assert_absent(CONFIG, "self.ai_completion.model = model.to_string()")
assert_absent(SCHEMA, '"enabled": { "type": "boolean", "default": false }')
assert_absent(SCHEMA, '"provider": { "type": "string", "default": "openai_compat" }')
assert_absent(SCHEMA, '"model": { "type": "string", "default": "gpt-4o-mini" }')

print("#4997 remote-AI activation-authority cutover prepared")
