#![warn(missing_docs)]
//! Configuration models for perl-lsp server runtime state.
//!
//! Absorbed from `perl-lsp-config` crate into `perl-lsp-rs-core`
//! as part of Wave Final PR B (#4541). This module isolates configuration
//! parsing and defaults from the main server crate so they can evolve
//! independently and be reused by tooling.

#[cfg(all(test, not(target_arch = "wasm32")))]
use crate::platform::resolve_perl_path_with_toolchain;
use perl_parser_core::path_security::{WorkspacePathError, validate_workspace_path};
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::process::{Command, Output, Stdio};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
use std::{fs::File, io::Read};

use crate::tooling::perl_critic::NativeCriticProfile;

mod critic_state;
mod dependency_detection;
mod metadata_dependencies;
mod native_build_hints;
pub mod perl_oracle_env;
pub mod toolchain_profile;

pub(crate) use critic_state::CriticSettingsCandidate;
pub use critic_state::{EffectiveCriticState, EffectiveNativeCriticConfig};
pub use dependency_detection::detect_dependency_include_paths;
pub use metadata_dependencies::{
    DeclaredDependency, DeclaredDependencySource, detect_declared_dependencies,
    extract_build_pl_requirements, extract_cpanfile_requirements, extract_dist_ini_requirements,
    extract_makefile_pl_requirements, extract_meta_json_requirements,
    extract_meta_yml_requirements,
};
pub use native_build_hints::{NativeBuildHints, detect_native_build_hints};
pub use perl_lsp_perltidy::FormatterMode;
#[cfg(not(target_arch = "wasm32"))]
pub use perl_oracle_env::PerlOracleEnv;
pub use toolchain_profile::PerlToolchainProfile;

/// Critic diagnostic engine used for LSP policy diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CriticEngine {
    /// Existing built-in/external Perl::Critic-compatible path.
    Legacy,
    /// Rust-native critic rule registry.
    ///
    /// This is the default: native diagnostics are always on and require no
    /// external `perlcritic`. The legacy/external engine is opt-in via
    /// `.perl-lsp.toml`.
    #[default]
    Native,
}

/// Server configuration
///
/// Runtime configuration for LSP server features. Updated dynamically via
/// `didChangeConfiguration`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerConfig {
    /// Whether inlay hints are globally enabled.
    pub inlay_hints_enabled: bool,
    /// Show parameter name hints at call sites.
    pub inlay_hints_parameter_hints: bool,
    /// Show inferred type hints for variables.
    pub inlay_hints_type_hints: bool,
    /// Show hints for method chains.
    pub inlay_hints_chained_hints: bool,
    /// Maximum character length for hint labels before truncation.
    pub inlay_hints_max_length: usize,

    /// Whether telemetry events are enabled.
    pub telemetry_enabled: bool,

    /// Whether critic diagnostics are enabled.
    ///
    /// The default engine is native and does not require `perlcritic`. Projects
    /// can select the legacy/external engine when exact Perl::Critic behavior is
    /// required.
    pub perlcritic_enabled: bool,

    /// Minimum Perl::Critic severity level to report (1-5, where 5 = most severe).
    ///
    /// `perlcritic --severity N` reports violations at or above `N`.
    /// With this scale, `1` reports everything while `5` reports only the
    /// highest-severity violations. Default is 3 (Harsh).
    /// Equivalent to `perlcritic --severity`.
    pub perlcritic_severity: u8,

    /// Path to a `.perlcriticrc` profile file.
    ///
    /// When `Some`, passes `--profile=<path>` to perlcritic. When `None`,
    /// the auto-discovery logic looks for `.perlcriticrc` in the workspace root.
    pub perlcritic_profile: Option<String>,

    /// Optional Perl::Critic theme expression.
    ///
    /// When `Some`, passes `--theme=<expr>` to perlcritic.
    pub perlcritic_theme: Option<String>,

    /// Critic engine used for LSP policy diagnostics.
    pub critic_engine: CriticEngine,

    /// Native critic profile used when `critic_engine` is [`CriticEngine::Native`].
    ///
    /// Defaults to `recommended` for the lower-noise native rule bundle. Projects
    /// can select `strict` for the full native rule surface.
    pub native_critic_profile: String,

    /// Native critic rule IDs to include. Empty means use the selected profile.
    pub native_critic_include: Vec<String>,

    /// Native critic rule IDs to exclude from the selected profile.
    pub native_critic_exclude: Vec<String>,

    /// Whether LSP formatting is enabled.
    ///
    /// Kept as `perltidy_enabled` for compatibility with older internal call sites
    /// and configuration names; the actual engine is selected by `formatting_engine`.
    pub perltidy_enabled: bool,

    /// Formatter engine used for LSP formatting requests.
    pub formatting_engine: FormatterMode,

    /// Whether to format on save (willSaveWaitUntil). Default `true` for
    /// backward compatibility. When `false`, manual formatting via
    /// `textDocument/formatting` still works — only the automatic save
    /// trigger is disabled (#5678).
    pub format_on_save: bool,

    /// Path to a `.perltidyrc` profile file.
    ///
    /// When `Some`, passes `--profile=<path>` to perltidy. When `None`,
    /// perltidy uses its default behavior or auto-discovers a profile.
    pub perltidy_profile: Option<String>,

    /// Maximum line length for perltidy.
    pub perltidy_maximum_line_length: Option<u32>,

    /// Indent size in spaces for perltidy.
    ///
    /// `None` means the workspace has not configured an indent width, in which
    /// case formatting falls back to the editor-supplied `tabSize`. When set —
    /// from `.perl-lsp.toml`, a discovered `.perltidyrc`, or
    /// `didChangeConfiguration` — the configured width wins over `tabSize` on
    /// both the native and external formatting paths.
    pub perltidy_indent_columns: Option<u32>,

    /// Use tabs instead of spaces for perltidy.
    ///
    /// `None` means unconfigured; formatting falls back to the editor-supplied
    /// `insertSpaces`. See [`ServerConfig::perltidy_indent_columns`] for the
    /// precedence rule.
    pub perltidy_tabs: Option<bool>,

    /// Opening brace on new line for perltidy.
    pub perltidy_opening_brace_on_new_line: Option<bool>,

    /// Cuddled else style for perltidy.
    pub perltidy_cuddled_else: Option<bool>,

    /// Space after keyword for perltidy.
    pub perltidy_space_after_keyword: Option<bool>,

    /// Add trailing commas for perltidy.
    pub perltidy_add_trailing_commas: Option<bool>,

    /// Vertical alignment for perltidy.
    pub perltidy_vertical_alignment: Option<bool>,

    /// Block comment indentation for perltidy.
    pub perltidy_block_comment_indentation: Option<u32>,

    /// Extra perltidy arguments.
    pub perltidy_extra_args: Vec<String>,

    /// Timeout in seconds for perltidy.
    pub perltidy_timeout_secs: u64,

    /// Feature gate for future next-edit suggestions.
    pub next_edit: NextEditConfig,

    /// AI-powered inline completion configuration.
    pub ai_completion: AiCompletionConfig,
}

/// Configuration for gated next-edit suggestions.
///
/// Disabled by default. Enabling this only opens the runtime boundary; no
/// editor-visible next-edit provider is registered yet.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NextEditConfig {
    /// Whether the future next-edit runtime boundary is explicitly enabled.
    pub enabled: bool,
}

/// Configuration for AI-powered inline completions.
///
/// Disabled by default. When enabled, the server calls an external AI provider
/// for inline completion suggestions, falling back to deterministic rules on
/// timeout, error, or when AI is disabled.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiCompletionConfig {
    /// Whether the user explicitly enabled AI completions via the LSP client
    /// configuration channel. Default: false.
    pub user_enabled: bool,
    /// Whether a workspace/project `.perl-lsp.toml` opted out (`enabled = false`).
    /// Project config may only disable AI, never enable it (issue #4997).
    pub project_opt_out: bool,
    /// Effective runtime flag: `user_enabled && !project_opt_out`.
    pub enabled: bool,
    /// Provider type. Currently only "openai_compat" is supported.
    pub provider: String,
    /// API endpoint URL.
    pub endpoint: String,
    /// Model identifier (e.g., "gpt-4o-mini").
    pub model: String,
    /// Environment variable name containing the API key.
    pub api_key_env: String,
    /// HTTP header used to send the API key. Default: Authorization.
    pub api_key_header: String,
    /// Optional auth scheme prepended before the API key. Default: Bearer.
    pub api_key_prefix: Option<String>,
    /// Request timeout in milliseconds. Default: 1800.
    pub timeout_ms: u64,
    /// Maximum output tokens per request. Default: 64.
    pub max_output_tokens: u32,
    /// Maximum requests per second. Default: 1.
    pub rate_limit_rps: f64,
    /// Maximum concurrent in-flight requests. Default: 1.
    pub max_inflight: u32,
    /// Whether to fall back to deterministic completions on AI failure. Default: true.
    pub fallback: bool,
    /// Allow plain HTTP to loopback-only local model endpoints. Default: false.
    pub local_model_mode: bool,
    /// Streaming-specific configuration.
    pub streaming: AiStreamingConfig,
}

pub(crate) const DEFAULT_AI_API_KEY_HEADER: &str = "Authorization";
pub(crate) const DEFAULT_AI_API_KEY_PREFIX: &str = "Bearer";

pub(crate) fn normalize_ai_api_key_header(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed.bytes().all(is_http_header_name_byte))
        .then(|| trimmed.to_string())
}

pub(crate) fn normalize_ai_api_key_prefix(value: &str) -> Option<Option<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(None);
    }

    is_safe_http_header_value_part(trimmed).then(|| Some(trimmed.to_string()))
}

pub(crate) fn is_safe_http_header_value_part(value: &str) -> bool {
    !value.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
}

fn is_http_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Streaming sub-configuration for AI completions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiStreamingConfig {
    /// Whether the user enabled streaming via the LSP client configuration channel.
    /// Default: true (streaming is on when AI completions are enabled).
    pub user_enabled: bool,
    /// Effective runtime flag, currently mirrors `user_enabled`.
    pub enabled: bool,
    /// Minimum milliseconds between emitted updates. Default: 60.
    pub update_debounce_ms: u64,
}

/// Recompute effective AI completion flags from user/project inputs.
///
/// `enabled` is true only when the user enabled AI and the project did not
/// opt out. Streaming `enabled` currently mirrors streaming `user_enabled`.
pub fn recompute_ai_completion_effective(ai: &mut AiCompletionConfig) {
    ai.enabled = ai.user_enabled && !ai.project_opt_out;
    ai.streaming.enabled = ai.streaming.user_enabled;
}

impl Default for AiCompletionConfig {
    fn default() -> Self {
        Self {
            user_enabled: false,
            project_opt_out: false,
            enabled: false,
            provider: "openai_compat".to_string(),
            endpoint: String::new(),
            model: "gpt-4o-mini".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            api_key_header: DEFAULT_AI_API_KEY_HEADER.to_string(),
            api_key_prefix: Some(DEFAULT_AI_API_KEY_PREFIX.to_string()),
            timeout_ms: 1800,
            max_output_tokens: 64,
            rate_limit_rps: 1.0,
            max_inflight: 1,
            fallback: true,
            local_model_mode: false,
            streaming: AiStreamingConfig::default(),
        }
    }
}

impl Default for AiStreamingConfig {
    fn default() -> Self {
        Self { user_enabled: true, enabled: true, update_debounce_ms: 60 }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            inlay_hints_enabled: true,
            inlay_hints_parameter_hints: true,
            inlay_hints_type_hints: true,
            inlay_hints_chained_hints: false,
            inlay_hints_max_length: 30,
            telemetry_enabled: false,
            perlcritic_enabled: true,
            perlcritic_severity: 3,
            perlcritic_profile: None,
            perlcritic_theme: None,
            critic_engine: CriticEngine::Native,
            native_critic_profile: "recommended".to_string(),
            native_critic_include: Vec::new(),
            native_critic_exclude: Vec::new(),
            perltidy_enabled: true,
            formatting_engine: FormatterMode::Native,
            format_on_save: true,
            perltidy_profile: None,
            perltidy_maximum_line_length: Some(80),
            // Unset by default: unconfigured workspaces defer to the editor's
            // `tabSize` / `insertSpaces`.
            perltidy_indent_columns: None,
            perltidy_tabs: None,
            perltidy_opening_brace_on_new_line: Some(false),
            perltidy_cuddled_else: Some(true),
            perltidy_space_after_keyword: Some(true),
            perltidy_add_trailing_commas: Some(false),
            perltidy_vertical_alignment: Some(true),
            perltidy_block_comment_indentation: Some(0),
            perltidy_extra_args: Vec::new(),
            perltidy_timeout_secs: 10,
            next_edit: NextEditConfig::default(),
            ai_completion: AiCompletionConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Update configuration from LSP settings
    pub fn update_from_value(&mut self, settings: &serde_json::Value) {
        if let Some(inlay) = settings.get("inlayHints") {
            if let Some(enabled) = inlay.get("enabled").and_then(|v| v.as_bool()) {
                self.inlay_hints_enabled = enabled;
            }
            if let Some(param) = inlay.get("parameterHints").and_then(|v| v.as_bool()) {
                self.inlay_hints_parameter_hints = param;
            }
            if let Some(type_hints) = inlay.get("typeHints").and_then(|v| v.as_bool()) {
                self.inlay_hints_type_hints = type_hints;
            }
            if let Some(chained) = inlay.get("chainedHints").and_then(|v| v.as_bool()) {
                self.inlay_hints_chained_hints = chained;
            }
            if let Some(max_len) = inlay.get("maxLength").and_then(as_config_u64) {
                self.inlay_hints_max_length = max_len as usize;
            }
        }

        if let Some(telemetry) = settings.get("telemetry")
            && let Some(enabled) = telemetry.get("enabled").and_then(|v| v.as_bool())
        {
            self.telemetry_enabled = enabled;
        }

        if let Some(next_edit) = settings.get("nextEdit")
            && let Some(enabled) = next_edit.get("enabled").and_then(|v| v.as_bool())
        {
            self.next_edit.enabled = enabled;
        }

        // Critic settings advance as ONE accepted transaction (#8253): the
        // legacy `perlcritic.*` keys seed shared enablement/severity and the
        // native `critic.*` keys override them (#3276), but both blocks are
        // validated together before any sibling mutates. A payload with any
        // invalid critic sibling is rejected whole, retaining the complete
        // prior accepted state and emitting exactly one deduplicated condition.
        match CriticSettingsCandidate::parse_lsp_update(settings) {
            Ok(candidate) => {
                if !candidate.is_empty() {
                    candidate.apply_to(self);
                }
            }
            Err(rejection) => rejection.emit_single_condition(),
        }

        if let Some(formatting) = settings.get("formatting") {
            if let Some(enabled) = formatting.get("enabled").and_then(|v| v.as_bool()) {
                self.perltidy_enabled = enabled;
            }
            if let Some(format_on_save) = formatting.get("formatOnSave").and_then(|v| v.as_bool()) {
                self.format_on_save = format_on_save;
            }
            if let Some(engine) = formatting.get("engine").and_then(|v| v.as_str()) {
                match parse_client_formatter_mode(engine) {
                    Some(mode) => self.formatting_engine = mode,
                    None => tracing::warn!(
                        target: "perl_lsp::config",
                        setting = "formatting.engine",
                        value = %engine,
                        valid = CLIENT_FORMATTER_MODE_VALID_OPTIONS,
                        "unrecognized formatting.engine value; keeping current setting",
                    ),
                }
            }
            // Security: do NOT honour LSP-channel formatting.profile / extraArgs (issue #5001).
            if let Some(len) = formatting.get("maximumLineLength").and_then(as_config_u64) {
                self.perltidy_maximum_line_length = Some(len as u32);
            }
            if let Some(indent) = formatting.get("indentColumns").and_then(as_config_u64) {
                self.perltidy_indent_columns = Some(indent as u32);
            }
            if let Some(tabs) = formatting.get("tabs").and_then(|v| v.as_bool()) {
                self.perltidy_tabs = Some(tabs);
            }
            if let Some(brace) = formatting.get("openingBraceOnNewLine").and_then(|v| v.as_bool()) {
                self.perltidy_opening_brace_on_new_line = Some(brace);
            }
            if let Some(cuddle) = formatting.get("cuddledElse").and_then(|v| v.as_bool()) {
                self.perltidy_cuddled_else = Some(cuddle);
            }
            if let Some(space) = formatting.get("spaceAfterKeyword").and_then(|v| v.as_bool()) {
                self.perltidy_space_after_keyword = Some(space);
            }
            if let Some(comma) = formatting.get("addTrailingCommas").and_then(|v| v.as_bool()) {
                self.perltidy_add_trailing_commas = Some(comma);
            }
            if let Some(align) = formatting.get("verticalAlignment").and_then(|v| v.as_bool()) {
                self.perltidy_vertical_alignment = Some(align);
            }
            if let Some(block) = formatting.get("blockCommentIndentation").and_then(as_config_u64) {
                self.perltidy_block_comment_indentation = Some(block as u32);
            }
            if let Some(timeout) = formatting.get("timeoutSecs").and_then(as_config_u64) {
                self.perltidy_timeout_secs = timeout;
            }
        }

        if let Some(ai) = settings.get("aiCompletion") {
            if let Some(enabled) = ai.get("enabled").and_then(|v| v.as_bool()) {
                self.ai_completion.user_enabled = enabled;
            }
            if let Some(provider) = ai.get("provider").and_then(|v| v.as_str()) {
                self.ai_completion.provider = provider.to_string();
            }
            // Security (#5684): do NOT honour LSP-channel endpoint, apiKeyEnv,
            // apiKeyHeader, or apiKeyPrefix. A hostile workspace could redirect
            // AI completion requests to an attacker-controlled endpoint and
            // exfiltrate source code, or change the env var name to read an
            // arbitrary secret. No configuration path currently sets them:
            // `.perl-lsp.toml` does not carry these fields at all (#4955),
            // no client channel may supply them (#5684), and the primary VS
            // Code extension exposes no endpoint/credential surface (known
            // gap documented in docs/reference/AI_COMPLETION.md and #4997).
            if let Some(_endpoint) = ai.get("endpoint").and_then(|v| v.as_str()) {
                tracing::warn!(
                    target: "perl_lsp::config",
                    "ignoring aiCompletion.endpoint from didChangeConfiguration (security: #5684)"
                );
            }
            if let Some(model) = ai.get("model").and_then(|v| v.as_str()) {
                self.ai_completion.model = model.to_string();
            }
            if let Some(_key_env) = ai.get("apiKeyEnv").and_then(|v| v.as_str()) {
                tracing::warn!(
                    target: "perl_lsp::config",
                    "ignoring aiCompletion.apiKeyEnv from didChangeConfiguration (security: #5684)"
                );
            }
            if let Some(_key_header) = ai.get("apiKeyHeader").and_then(|v| v.as_str()) {
                tracing::warn!(
                    target: "perl_lsp::config",
                    "ignoring aiCompletion.apiKeyHeader from didChangeConfiguration (security: #5684)"
                );
            }
            if let Some(_key_prefix) = ai.get("apiKeyPrefix") {
                tracing::warn!(
                    target: "perl_lsp::config",
                    "ignoring aiCompletion.apiKeyPrefix from didChangeConfiguration (security: #5684)"
                );
            }
            if let Some(timeout) = ai.get("timeoutMs").and_then(|v| v.as_u64()) {
                self.ai_completion.timeout_ms = timeout;
            }
            if let Some(tokens) = ai.get("maxOutputTokens").and_then(|v| v.as_u64()) {
                self.ai_completion.max_output_tokens = tokens as u32;
            }
            if let Some(rps) = ai.get("rateLimitRps").and_then(|v| v.as_f64()) {
                self.ai_completion.rate_limit_rps = rps;
            }
            if let Some(inflight) = ai.get("maxInflight").and_then(|v| v.as_u64()) {
                self.ai_completion.max_inflight = inflight as u32;
            }
            if let Some(fallback) = ai.get("fallback").and_then(|v| v.as_bool()) {
                self.ai_completion.fallback = fallback;
            }
            if let Some(local_model_mode) = ai.get("localModelMode").and_then(|v| v.as_bool()) {
                self.ai_completion.local_model_mode = local_model_mode;
            }
            if let Some(streaming) = ai.get("streaming") {
                if let Some(enabled) = streaming.get("enabled").and_then(|v| v.as_bool()) {
                    self.ai_completion.streaming.user_enabled = enabled;
                }
                if let Some(debounce) = streaming.get("updateDebounceMs").and_then(|v| v.as_u64()) {
                    self.ai_completion.streaming.update_debounce_ms = debounce;
                }
            }
            recompute_ai_completion_effective(&mut self.ai_completion);
        }

        // Warn on wrong-type values for well-known settings. (#5093)
        // The and_then(|v| v.as_*) guards above silently ignore type mismatches;
        // this pass surfaces them so users know their config is being ignored.
        warn_on_type_mismatch(settings, "inlayHints", "enabled", "boolean");
        warn_on_type_mismatch(settings, "inlayHints", "parameterHints", "boolean");
        warn_on_type_mismatch(settings, "inlayHints", "typeHints", "boolean");
        warn_on_type_mismatch(settings, "diagnostics", "enabled", "boolean");
        warn_on_type_mismatch(settings, "formatting", "enabled", "boolean");
    }

    /// Return invalid enum values supplied through the LSP client-settings channel.
    ///
    /// The normal update path deliberately keeps an invalid value from changing
    /// the active configuration and emits a tracing warning. Runtime callers can
    /// use this companion inspection before applying the same payload when they
    /// need to surface an actionable `window/showMessage` to the editor user.
    pub fn invalid_client_setting_values(
        settings: &serde_json::Value,
    ) -> Vec<InvalidClientSetting> {
        let mut invalid = Vec::new();

        if let Some(critic) = settings.get("critic") {
            if let Some(engine) = critic.get("engine") {
                let invalid_engine = engine
                    .as_str()
                    .map(|value| parse_lsp_critic_engine(value).is_none())
                    .unwrap_or(true);
                if invalid_engine {
                    invalid.push(InvalidClientSetting {
                        setting: "critic.engine",
                        value: client_setting_display_value(engine),
                        value_type: client_setting_value_type(engine),
                        valid_options: CLIENT_CRITIC_ENGINE_VALID_OPTIONS,
                    });
                }
            }
            if let Some(profile) = critic.get("profile") {
                let invalid_profile = profile
                    .as_str()
                    .map(|value| NativeCriticProfile::parse(value).is_none())
                    .unwrap_or(true);
                if invalid_profile {
                    invalid.push(InvalidClientSetting {
                        setting: "critic.profile",
                        value: client_setting_display_value(profile),
                        value_type: client_setting_value_type(profile),
                        valid_options: NativeCriticProfile::VALID_OPTIONS,
                    });
                }
            }
        }

        if let Some(formatting) = settings.get("formatting")
            && let Some(engine) = formatting.get("engine")
        {
            let invalid_engine = engine
                .as_str()
                .map(|value| parse_client_formatter_mode(value).is_none())
                .unwrap_or(true);
            if invalid_engine {
                invalid.push(InvalidClientSetting {
                    setting: "formatting.engine",
                    value: client_setting_display_value(engine),
                    value_type: client_setting_value_type(engine),
                    valid_options: CLIENT_FORMATTER_MODE_VALID_OPTIONS,
                });
            }
        }

        invalid
    }
}

fn client_setting_display_value(value: &serde_json::Value) -> String {
    value.as_str().map(ToOwned::to_owned).unwrap_or_else(|| value.to_string())
}

fn client_setting_value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// An invalid enum value found in editor-provided LSP settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidClientSetting {
    /// Dotted setting path used by the client configuration surface.
    pub setting: &'static str,
    /// Value supplied by the client.
    pub value: String,
    /// JSON type supplied by the client, used to distinguish values that render identically.
    pub value_type: &'static str,
    /// Human-readable accepted values for the setting.
    pub valid_options: &'static str,
}

/// Log a warning when a config value has the wrong type. (#5093)
fn warn_on_type_mismatch(settings: &serde_json::Value, section: &str, field: &str, expected: &str) {
    if let Some(section_val) = settings.get(section)
        && let Some(field_val) = section_val.get(field)
    {
        let is_correct = match expected {
            "boolean" => field_val.is_boolean(),
            "string" => field_val.is_string(),
            "number" => field_val.is_u64() || field_val.is_i64() || field_val.is_f64(),
            _ => true,
        };
        if !is_correct {
            tracing::warn!(
                "Config {section}.{field} has wrong type (expected {expected}), ignoring value: {field_val}"
            );
        }
    }
}

/// Normalize a formatter mode for comparison and warning deduplication.
///
/// This is shared by the parser and the client-setting warning path so aliases
/// differing only by case, surrounding whitespace, or underscore/hyphen spelling
/// receive the same semantic treatment.
pub fn normalize_formatter_mode_value(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('_', "-")
}

fn parse_formatter_mode(value: &str) -> Option<FormatterMode> {
    match normalize_formatter_mode_value(value).as_str() {
        "native" => Some(FormatterMode::Native),
        "compat" | "perltidy-compat" => Some(FormatterMode::Compat),
        "external-legacy" | "external-perltidy" | "perltidy" => Some(FormatterMode::ExternalLegacy),
        "off" | "disabled" | "none" => Some(FormatterMode::Off),
        _ => None,
    }
}

fn parse_client_formatter_mode(value: &str) -> Option<FormatterMode> {
    match normalize_formatter_mode_value(value).as_str() {
        "native" => Some(FormatterMode::Native),
        "compat" | "perltidy-compat" => Some(FormatterMode::Compat),
        "off" | "disabled" | "none" => Some(FormatterMode::Off),
        _ => None,
    }
}

fn parse_critic_engine(value: &str) -> Option<CriticEngine> {
    match value.trim().to_ascii_lowercase().as_str() {
        "legacy" | "external" | "perlcritic" => Some(CriticEngine::Legacy),
        "native" => Some(CriticEngine::Native),
        _ => None,
    }
}

fn parse_lsp_critic_engine(value: &str) -> Option<CriticEngine> {
    match parse_critic_engine(value) {
        Some(CriticEngine::Native) => Some(CriticEngine::Native),
        Some(CriticEngine::Legacy) | None => None,
    }
}

/// Human-readable list of accepted `formatting.engine` values, used in
/// `tracing::warn!` messages when a user supplies an unrecognized value.
/// Kept in sync with [`parse_formatter_mode`].
const FORMATTER_MODE_VALID_OPTIONS: &str = "native, compat (perltidy-compat), external-legacy (external-perltidy, perltidy), \
     off (disabled, none)";

/// Human-readable values accepted for `formatting.engine` on the LSP
/// client-settings channel. External process selection remains project-owned.
const CLIENT_FORMATTER_MODE_VALID_OPTIONS: &str =
    "native, compat (perltidy-compat), off (disabled, none)";

/// Human-readable values accepted for `critic.engine` on the LSP client-settings
/// channel. Legacy subprocess aliases remain available only through trusted
/// project configuration.
const CLIENT_CRITIC_ENGINE_VALID_OPTIONS: &str = "native";

/// Which config channel supplied a critic rule-ID list.
///
/// The warning names this so the user knows *which* of the two places that can
/// set `critic.include` / `critic.exclude` they actually have to go and edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CriticRuleIdSource {
    /// Settings delivered by the editor over LSP — `initializationOptions`,
    /// `workspace/didChangeConfiguration`, or a `workspace/configuration`
    /// response. Deliberately labelled as one channel: [`ServerConfig::update_from_value`]
    /// serves all three and cannot tell them apart.
    ClientSettings,
    /// The project's `.perl-lsp.toml` file.
    ProjectFile,
}

impl CriticRuleIdSource {
    /// Label naming the concrete place the user edits to fix the entry.
    const fn as_str(self) -> &'static str {
        match self {
            Self::ClientSettings => "LSP client settings",
            Self::ProjectFile => ".perl-lsp.toml",
        }
    }
}

/// Warn for every entry in `ids` that is not a known native critic rule ID.
///
/// The full rule catalog is derived from the strict profile at call time so
/// warnings stay current when rules are added or removed from the registry.
/// Strict is a superset of recommended, so an ID missing from it is unknown
/// under every profile — which is why the warning points at the catalog and
/// spelling rather than suggesting a profile change.
/// Values are stored as-is even when unknown — the rule simply never matches.
///
/// Each warning names the offending ID, the setting key, the config channel it
/// arrived on, and — when one can be identified honestly — the closest valid
/// rule ID.
fn warn_unknown_rule_ids(source: CriticRuleIdSource, setting: &str, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let known = crate::tooling::perl_critic::NativeCriticRegistry::for_profile(
        crate::tooling::perl_critic::NativeCriticProfile::Strict,
    )
    .rule_ids();
    let origin = source.as_str();
    for id in ids {
        if known.contains(&id.as_str()) {
            continue;
        }
        match suggest_rule_id(id, &known) {
            Some(suggestion) => tracing::warn!(
                target: "perl_lsp::config",
                source = %origin,
                setting = %setting,
                value = %id,
                suggestion = %suggestion,
                "unrecognized native critic rule ID '{id}' in `{setting}` from {origin}; \
                 did you mean '{suggestion}'? until it is corrected this entry \
                 matches no finding",
            ),
            None => tracing::warn!(
                target: "perl_lsp::config",
                source = %origin,
                setting = %setting,
                value = %id,
                "unrecognized native critic rule ID '{id}' in `{setting}` from {origin}; \
                 no close match found — check the spelling against the native critic \
                 rule catalog; until it is corrected this entry matches no finding",
            ),
        }
    }
}

/// Final dotted segment of a rule ID — the bare rule name without its namespace.
fn rule_id_leaf(id: &str) -> &str {
    id.rsplit('.').next().unwrap_or(id)
}

/// Best-guess replacement for an unrecognized rule ID, or `None` when nothing is
/// close enough to suggest honestly.
///
/// Three cheap passes, in decreasing confidence:
/// 1. exact match ignoring ASCII case (`Native.IO.Pipe_Open`);
/// 2. a unique match on the bare rule name, which catches a dropped or wrong
///    namespace (`unused_lexical`, `native.vars.unused_lexical`);
/// 3. nearest edit distance within a length-scaled threshold (`...conditon`).
///
/// Returning `None` is deliberate. A confidently wrong suggestion sends the user
/// to edit the wrong rule, which costs more than offering no suggestion at all.
fn suggest_rule_id(unknown: &str, known: &[&'static str]) -> Option<&'static str> {
    if let Some(same_ignoring_case) =
        known.iter().copied().find(|candidate| candidate.eq_ignore_ascii_case(unknown))
    {
        return Some(same_ignoring_case);
    }

    let unknown_leaf = rule_id_leaf(unknown);
    let mut leaf_matches = known
        .iter()
        .copied()
        .filter(|candidate| rule_id_leaf(candidate).eq_ignore_ascii_case(unknown_leaf));
    if let Some(only_leaf_match) = leaf_matches.next()
        && leaf_matches.next().is_none()
    {
        return Some(only_leaf_match);
    }

    known
        .iter()
        .copied()
        .filter_map(|candidate| {
            let threshold = rule_id_suggestion_threshold(unknown, candidate);
            // Levenshtein distance is never smaller than the length difference,
            // so a candidate this far off in length cannot clear the threshold.
            // Skipping it here is exactly equivalent to computing the distance
            // and rejecting it, and it keeps a pathological `.perl-lsp.toml`
            // entry from paying the quadratic cost 28 times over.
            if unknown.len().abs_diff(candidate.len()) > threshold {
                return None;
            }
            let distance = rule_id_edit_distance(unknown, candidate);
            (distance <= threshold).then_some((candidate, distance))
        })
        .min_by_key(|(candidate, distance)| (*distance, candidate.len()))
        .map(|(candidate, _)| candidate)
}

/// How far apart two rule IDs may be and still be offered as a suggestion.
///
/// Scaled by the longer ID so a short bare name is held to a tighter budget than
/// a 38-character fully-qualified ID, and capped so an unrelated string never
/// drifts into range.
fn rule_id_suggestion_threshold(unknown: &str, known: &str) -> usize {
    unknown.len().max(known.len()).saturating_div(4).clamp(1, 4)
}

/// Levenshtein distance between two rule IDs, compared case-insensitively.
///
/// Rule IDs are ASCII (`native.<area>.<rule_name>`), so byte-wise comparison is
/// exact here and avoids the cost of building char vectors.
fn rule_id_edit_distance(left: &str, right: &str) -> usize {
    let left = left.to_ascii_lowercase();
    let right = right.to_ascii_lowercase();
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();

    let mut previous: Vec<usize> = (0..=right_bytes.len()).collect();
    let mut current = vec![0; right_bytes.len() + 1];

    for (left_index, left_byte) in left_bytes.iter().enumerate() {
        current[0] = left_index + 1;

        for (right_index, right_byte) in right_bytes.iter().enumerate() {
            let substitution_cost = usize::from(left_byte != right_byte);
            let deletion = previous[right_index + 1] + 1;
            let insertion = current[right_index] + 1;
            let substitution = previous[right_index] + substitution_cost;
            current[right_index + 1] = deletion.min(insertion).min(substitution);
        }

        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_bytes.len()]
}

/// Controls whether PERL5LIB paths are prepended or appended to `include_paths`.
///
/// `Prepend` (the default) mirrors Perl's own behaviour: paths earlier in the
/// search order shadow later ones, so PERL5LIB paths take priority over any
/// project-level `include_paths`.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Perl5LibPrecedence {
    /// PERL5LIB entries are placed *before* `include_paths` (default).
    #[default]
    Prepend,
    /// PERL5LIB entries are placed *after* `include_paths`.
    Append,
}

/// Outcome of a system `@INC` probe.
///
/// The outcome is retained separately from the fail-closed path returned
/// by [`WorkspaceConfig::get_system_inc`], so callers that need to decide
/// whether a retry is safe do not have to infer process failures from an empty
/// include-path list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemIncProbeOutcome {
    /// System include probing is disabled for this configuration.
    Disabled,
    /// No Perl oracle could be constructed for the requested probe.
    Unavailable,
    /// The Perl probe timed out before producing a result.
    TimedOut,
    /// The probe process failed at the I/O level — the spawn itself or a
    /// later `try_wait`/pipe error. The underlying helper reports these
    /// identically, so the stage is deliberately NOT distinguished here;
    /// callers must not read this as "spawn-only" (#11840 review).
    IoFailed,
    /// The Perl process exited unsuccessfully.
    NonZeroExit,
    /// The Perl process succeeded but produced no usable `@INC` paths.
    SuccessfulEmpty,
    /// The Perl process succeeded and produced usable `@INC` paths.
    Paths(Vec<PathBuf>),
}

/// Workspace configuration for module resolution
///
/// Controls how the LSP server resolves module imports and finds
/// Perl module files across the workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    /// Workspace-root-relative include paths for module resolution.
    ///
    /// Only relative entries that remain inside the workspace after
    /// normalization are accepted from the resource-scoped client-settings
    /// channel (`perl.workspace.includePaths`). Absolute external roots
    /// belong in [`Self::external_include_paths`].
    /// Default: `["lib", ".", "local/lib/perl5"]`
    pub include_paths: Vec<String>,

    /// Machine-scoped external include roots (absolute paths).
    ///
    /// Admitted only through
    /// [`ExternalIncludePathAuthority::TrustedUserOperator`] (a future
    /// server-owned operator adapter; #10817). Every current client channel is
    /// unauthorized (#4998), so this stays empty in production until that
    /// adapter lands. The VS Code `perl-lsp.externalIncludePaths` setting
    /// (`scope: machine`) remains client-side defence in depth only.
    pub external_include_paths: Vec<String>,

    /// Additional file extensions accepted during workspace discovery.
    pub discovery_extra_extensions: Vec<String>,

    /// Additional directory names skipped during workspace discovery.
    pub discovery_extra_skipped_dirs: Vec<String>,

    /// Whether to include system @INC paths in module resolution
    /// Default: false (avoids blocking on network filesystems)
    pub use_system_inc: bool,

    /// Cached system @INC probe outcome (populated lazily when use_system_inc is true).
    system_inc_cache: Option<SystemIncProbeOutcome>,

    /// Perl interpreter used for startup `@INC` probing.
    ///
    /// When unset, falls back to `perl` on `PATH`.
    pub perl_path: Option<String>,

    /// Extra arguments passed to the Perl interpreter for startup `@INC` probing.
    pub perl_args: Vec<String>,

    /// Native build hints derived from workspace-root `Makefile.PL` / `Build.PL`.
    ///
    /// These are cached once at workspace initialization and kept separate from
    /// Perl module search paths.
    pub native_build_hints: NativeBuildHints,

    /// Declared dependency facts derived from workspace-root project metadata.
    ///
    /// These facts are advisory only. They do not mutate module search paths or
    /// imply that a dependency is installed/indexed.
    pub declared_dependencies: Vec<DeclaredDependency>,

    /// Resolution timeout in milliseconds
    /// Default: 50ms
    pub resolution_timeout_ms: u64,

    /// Whether the `PERL5LIB` environment variable is read and merged into
    /// the module search path.  Default: `true`.
    pub use_perl5lib: bool,

    /// Controls whether PERL5LIB entries come before or after `include_paths`.
    /// Default: `Prepend` (mirrors Perl's own search order).
    pub perl5lib_precedence: Perl5LibPrecedence,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            include_paths: vec!["lib".to_string(), ".".to_string(), "local/lib/perl5".to_string()],
            external_include_paths: Vec::new(),
            discovery_extra_extensions: Vec::new(),
            discovery_extra_skipped_dirs: Vec::new(),
            use_system_inc: false,
            system_inc_cache: None,
            perl_path: None,
            perl_args: Vec::new(),
            native_build_hints: NativeBuildHints::default(),
            declared_dependencies: Vec::new(),
            resolution_timeout_ms: 50,
            use_perl5lib: true,
            perl5lib_precedence: Perl5LibPrecedence::Prepend,
        }
    }
}

/// A configuration channel that cannot prove user/machine provenance.
///
/// The label only names the channel for diagnostics; it never grants
/// authority (#4998).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnauthorizedExternalIncludePathSource {
    /// Client-supplied `initializationOptions`.
    InitializationOptions,
    /// `workspace/didChangeConfiguration` notification payload.
    DidChangeConfiguration,
    /// The unscoped (first) item of a `workspace/configuration` response.
    GenericUnscopedConfiguration,
    /// A per-folder item of a `workspace/configuration` response.
    FolderConfiguration,
    /// Project/resource configuration (`.perl-lsp.toml`).
    ProjectResource,
    /// Unclassified or unknown channel; fails closed like every other source.
    Unknown,
}

impl UnauthorizedExternalIncludePathSource {
    /// Human-readable channel name for rejection messages and logs.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::InitializationOptions => "initialization options",
            Self::DidChangeConfiguration => "workspace/didChangeConfiguration",
            Self::GenericUnscopedConfiguration => "unscoped workspace/configuration",
            Self::FolderConfiguration => "folder-scoped workspace/configuration",
            Self::ProjectResource => "project configuration",
            Self::Unknown => "unclassified",
        }
    }
}

/// Server-owned authority disposition for machine-scoped external include
/// roots (`workspace.externalIncludePaths`).
///
/// Transport position (result-array slot), key spelling, client names, and
/// client-supplied scope labels confer no authority (#4998): any LSP client
/// can forge them. No production channel currently carries independently
/// verified user/machine provenance, so every current call site passes
/// [`ExternalIncludePathAuthority::Untrusted`] and `externalIncludePaths`
/// entries are rejected without clearing previously accepted values. The
/// [`ExternalIncludePathAuthority::TrustedUserOperator`] variant is reserved
/// for a future server-owned operator adapter and for tests proving the rule
/// is not "absolute paths are impossible"; constructing it from client input
/// is a security defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalIncludePathAuthority {
    /// The channel cannot prove user/machine provenance.
    Untrusted(UnauthorizedExternalIncludePathSource),
    /// An explicitly trusted user/operator adapter. Entries still pass the
    /// existing absolute-path validation.
    TrustedUserOperator,
}

impl Default for ExternalIncludePathAuthority {
    fn default() -> Self {
        Self::Untrusted(UnauthorizedExternalIncludePathSource::Unknown)
    }
}

/// Context for [`WorkspaceConfig::update_from_value_with_context`].
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceConfigUpdateContext<'a> {
    /// Workspace root used to reject traversal in resource-scoped `includePaths`.
    pub workspace_root: Option<&'a Path>,
    /// Authority disposition for machine-scoped `externalIncludePaths`.
    ///
    /// Defaults to [`ExternalIncludePathAuthority::Untrusted`] with an
    /// unclassified source, so omitted context fails closed.
    pub external_include_paths: ExternalIncludePathAuthority,
}

/// A resource-scoped `includePaths` entry rejected during client-settings validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedClientIncludePath {
    /// The raw, as-configured entry string.
    pub entry: String,
    /// Why it was rejected.
    pub reason: RejectedClientIncludePathReason,
}

/// Why a client-settings `includePaths` / `externalIncludePaths` entry was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectedClientIncludePathReason {
    /// Absolute paths must use `externalIncludePaths` (machine scope) instead.
    Absolute,
    /// Relative entry failed workspace containment validation.
    EscapesWorkspace(String),
    /// `externalIncludePaths` entries must be absolute filesystem roots.
    ExternalRelative,
    /// `externalIncludePaths` entries must not contain null/control characters.
    ExternalInvalidCharacters,
    /// Machine-scoped external roots arrived on a channel that cannot prove
    /// user/machine provenance (#4998). The stored value is preserved.
    ExternalUnauthorized(UnauthorizedExternalIncludePathSource),
}

impl RejectedClientIncludePath {
    /// Render a single human-readable line for logs and editor notifications.
    #[must_use]
    pub fn render(&self) -> String {
        match &self.reason {
            RejectedClientIncludePathReason::Absolute => format!(
                "'{}': absolute paths are not allowed in `perl.workspace.includePaths` \
                 (workspace-supplied). Move this entry to `perl-lsp.externalIncludePaths` \
                 in your user settings instead.",
                self.entry
            ),
            RejectedClientIncludePathReason::EscapesWorkspace(detail) => {
                format!("'{}': escapes the workspace root ({detail})", self.entry)
            }
            RejectedClientIncludePathReason::ExternalRelative => format!(
                "'{}': relative paths are not allowed in `perl.workspace.externalIncludePaths`; \
                 use `perl.workspace.includePaths` for workspace-relative roots instead.",
                self.entry
            ),
            RejectedClientIncludePathReason::ExternalInvalidCharacters => format!(
                "'{}': contains null bytes or disallowed control characters",
                escape_for_display(&self.entry)
            ),
            RejectedClientIncludePathReason::ExternalUnauthorized(source) => format!(
                "'{}': `perl.workspace.externalIncludePaths` was ignored because the {} \
                 channel cannot prove user/machine provenance. External include roots are \
                 not applied yet; use workspace-relative paths in `includePaths` instead.",
                self.entry,
                source.label()
            ),
        }
    }
}

fn validate_resource_include_path_entry(
    entry: &str,
    workspace_root: Option<&Path>,
) -> Result<(), RejectedClientIncludePathReason> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return Err(RejectedClientIncludePathReason::EscapesWorkspace(
            "empty include path".to_string(),
        ));
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return Err(RejectedClientIncludePathReason::Absolute);
    }

    if let Some(root) = workspace_root {
        if let Err(err) = validate_workspace_path(candidate, root) {
            return Err(RejectedClientIncludePathReason::EscapesWorkspace(err.to_string()));
        }
    } else {
        // No workspace root configured (e.g. single-file mode). Fail-closed:
        // reject relative paths containing `..` components, which could
        // escape to arbitrary filesystem locations. Without this, a hostile
        // client could set includePaths to `../../etc/passwd` and probe
        // files outside any workspace boundary. (#5345)
        if candidate.components().any(|c| c == std::path::Component::ParentDir) {
            tracing::warn!(
                entry = trimmed,
                "rejecting relative include path with '..' because no workspace root is configured"
            );
            return Err(RejectedClientIncludePathReason::EscapesWorkspace(
                "relative path with '..' cannot be validated without a workspace root".to_string(),
            ));
        }
    }

    Ok(())
}

fn parse_external_include_paths(
    paths: &[serde_json::Value],
) -> (Vec<String>, Vec<RejectedClientIncludePath>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for value in paths {
        let Some(entry) = value.as_str() else {
            continue;
        };
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        if external_include_path_has_invalid_characters(trimmed) {
            tracing::warn!(
                target: "perl_lsp::config",
                entry = %escape_for_display(trimmed),
                "rejected perl.workspace.externalIncludePaths entry with null/control characters"
            );
            rejected.push(RejectedClientIncludePath {
                entry: trimmed.to_string(),
                reason: RejectedClientIncludePathReason::ExternalInvalidCharacters,
            });
            continue;
        }
        // Machine-scoped external roots must be absolute. Relative entries belong in
        // resource-scoped `includePaths` (validated against the workspace root).
        if !Path::new(trimmed).is_absolute() {
            tracing::warn!(
                target: "perl_lsp::config",
                entry = %escape_for_display(trimmed),
                "rejected relative perl.workspace.externalIncludePaths entry; use includePaths for workspace-relative roots"
            );
            rejected.push(RejectedClientIncludePath {
                entry: trimmed.to_string(),
                reason: RejectedClientIncludePathReason::ExternalRelative,
            });
            continue;
        }
        accepted.push(trimmed.to_string());
    }
    (accepted, rejected)
}

fn external_include_path_has_invalid_characters(entry: &str) -> bool {
    entry.chars().any(|c| c == '\0' || (c.is_control() && c != '\t'))
}

fn normalize_include_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = Path::new(trimmed).components().fold(PathBuf::new(), |mut acc, comp| {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::RootDir
            | std::path::Component::Prefix(_)
            | std::path::Component::ParentDir
            | std::path::Component::Normal(_) => acc.push(comp.as_os_str()),
        }
        acc
    });

    if normalized.as_os_str().is_empty() {
        return Some(".".to_string());
    }

    Some(normalized.to_string_lossy().into_owned())
}

fn dedupe_preserve_order<'a>(paths: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut result = Vec::new();
    for path in paths {
        let Some(normalized) = normalize_include_path(path) else {
            continue;
        };
        if !result.iter().any(|existing| existing == &normalized) {
            result.push(normalized);
        }
    }
    result
}

fn normalize_string_list(values: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || result.iter().any(|existing| existing == trimmed) {
            continue;
        }
        result.push(trimmed.to_string());
    }
    result
}

fn string_array(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    value.and_then(|value| value.as_array()).map(|values| {
        normalize_string_list(
            &values
                .iter()
                .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>(),
        )
    })
}

/// Parse a JSON numeric value into `u64`, accepting both integer and float forms.
///
/// `serde_json::Value::as_u64()` rejects JSON floats (e.g. `4.0`), which some
/// configuration generators emit. This helper accepts both `4` and `4.0` and
/// also rejects negative values (all config numeric fields are non-negative).
fn as_config_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().filter(|f| *f >= 0.0 && f.is_finite()).map(|f| f as u64))
}

impl WorkspaceConfig {
    /// Parse a `PERL5LIB` environment variable value into a list of paths.
    ///
    /// Uses `:` as the separator on Unix and `;` on Windows, matching Perl's
    /// own behaviour.  Empty components (produced by leading, trailing, or
    /// consecutive separators) are silently dropped.
    pub fn parse_perl5lib(value: &str) -> Vec<String> {
        #[cfg(windows)]
        const SEP: char = ';';
        #[cfg(not(windows))]
        const SEP: char = ':';
        dedupe_preserve_order(value.split(SEP))
    }

    /// Read Perl library paths from the environment.
    ///
    /// Checks `PERL5LIB` first, falling back to `PERLLIB` (which Perl itself
    /// treats as the fallback when `PERL5LIB` is unset). Returns an empty vec
    /// if neither is set. Uses the platform-appropriate path separator.
    pub fn env_perl_lib_paths() -> Vec<String> {
        #[cfg(windows)]
        const SEP: char = ';';
        #[cfg(not(windows))]
        const SEP: char = ':';
        let value =
            std::env::var("PERL5LIB").or_else(|_| std::env::var("PERLLIB")).unwrap_or_default();
        if value.is_empty() { Vec::new() } else { dedupe_preserve_order(value.split(SEP)) }
    }

    /// Return the effective module-search-path, merging `PERL5LIB` paths with
    /// `self.include_paths` according to `self.perl5lib_precedence`.
    ///
    /// If `self.use_perl5lib` is `false`, or `perl5lib_paths` is empty, the
    /// returned list contains only `self.include_paths` entries (trimmed and deduplicated).
    pub fn effective_include_paths(&self, perl5lib_paths: &[String]) -> Vec<String> {
        let configured = dedupe_preserve_order(
            self.include_paths
                .iter()
                .map(String::as_str)
                .chain(self.external_include_paths.iter().map(String::as_str)),
        );
        if !self.use_perl5lib || perl5lib_paths.is_empty() {
            return configured;
        }
        match self.perl5lib_precedence {
            Perl5LibPrecedence::Prepend => dedupe_preserve_order(
                perl5lib_paths
                    .iter()
                    .map(String::as_str)
                    .chain(configured.iter().map(String::as_str)),
            ),
            Perl5LibPrecedence::Append => dedupe_preserve_order(
                configured
                    .iter()
                    .map(String::as_str)
                    .chain(perl5lib_paths.iter().map(String::as_str)),
            ),
        }
    }

    /// Refresh workspace-native build hints from the selected workspace root.
    ///
    /// This is a workspace-initialization cache step only; it does not mutate
    /// module-resolution include paths.
    pub fn refresh_native_build_hints(&mut self, workspace_root: &Path) {
        self.native_build_hints = detect_native_build_hints(workspace_root);
    }

    /// Refresh declared dependency facts from the selected workspace root.
    ///
    /// This is a workspace-initialization cache step only; it does not mutate
    /// module-resolution include paths.
    pub fn refresh_declared_dependencies(&mut self, workspace_root: &Path) {
        self.declared_dependencies = detect_declared_dependencies(workspace_root);
    }

    /// Append marker-detected Carton/Carmel roots to module-resolution paths.
    ///
    /// Existing configured paths are preserved in order, and equivalent paths
    /// are not added twice. `PERL5LIB` is merged later by
    /// [`Self::effective_include_paths`], so its configured precedence remains
    /// unchanged.
    pub fn refresh_dependency_include_paths(&mut self, workspace_root: &Path) {
        for detected in detect_dependency_include_paths(workspace_root) {
            let Some(normalized_detected) = normalize_include_path(&detected) else {
                continue;
            };
            let already_present = self
                .include_paths
                .iter()
                .filter_map(|path| normalize_include_path(path))
                .any(|path| path == normalized_detected);
            if !already_present {
                self.include_paths.push(detected);
            }
        }
    }

    /// Update workspace configuration from LSP settings.
    ///
    /// Fail-closed for `externalIncludePaths`: the default context carries no
    /// external-root authority, so entries are rejected and the stored value
    /// is preserved. Callers must name their channel through
    /// [`Self::update_from_value_with_context`].
    pub fn update_from_value(
        &mut self,
        settings: &serde_json::Value,
    ) -> Vec<RejectedClientIncludePath> {
        self.update_from_value_with_context(settings, WorkspaceConfigUpdateContext::default())
    }

    /// Update workspace configuration from LSP settings with validation context.
    ///
    /// Resource-scoped `includePaths` reject absolute entries and any relative
    /// entry that escapes `workspace_root` when provided. `externalIncludePaths`
    /// are accepted only from
    /// [`ExternalIncludePathAuthority::TrustedUserOperator`]; every untrusted
    /// channel (initialization options, didChangeConfiguration, unscoped and
    /// folder-scoped configuration results, project config, unknown) gets its
    /// non-empty entries rejected without clearing previously accepted values.
    pub fn update_from_value_with_context(
        &mut self,
        settings: &serde_json::Value,
        context: WorkspaceConfigUpdateContext<'_>,
    ) -> Vec<RejectedClientIncludePath> {
        let mut rejected = Vec::new();
        if let Some(workspace) = settings.get("workspace") {
            if let Some(paths) = workspace.get("includePaths").and_then(|v| v.as_array()) {
                let mut valid = Vec::with_capacity(paths.len());
                for value in paths {
                    let Some(entry) = value.as_str() else {
                        continue;
                    };
                    match validate_resource_include_path_entry(entry, context.workspace_root) {
                        Ok(()) => valid.push(entry.to_string()),
                        Err(reason) => rejected
                            .push(RejectedClientIncludePath { entry: entry.to_string(), reason }),
                    }
                }
                self.include_paths = valid;
            }
            if let Some(paths) = workspace.get("externalIncludePaths").and_then(|v| v.as_array()) {
                match context.external_include_paths {
                    ExternalIncludePathAuthority::TrustedUserOperator => {
                        let (accepted, external_rejected) = parse_external_include_paths(paths);
                        if external_rejected.is_empty() {
                            self.external_include_paths = accepted;
                        } else {
                            // Atomic admission (catalog fallback `RejectSource`):
                            // a candidate with any invalid entry is rejected as
                            // a whole, so a malformed or partially valid
                            // payload never clears or partially replaces the
                            // previously accepted complete set (INC-AUTH-007).
                            rejected.extend(external_rejected);
                        }
                    }
                    ExternalIncludePathAuthority::Untrusted(source) => {
                        // Fail closed (#4998): reject non-empty unauthorized
                        // arrivals without clearing previously accepted values.
                        let entries: Vec<&str> = paths
                            .iter()
                            .filter_map(|value| value.as_str())
                            .map(str::trim)
                            .filter(|entry| !entry.is_empty())
                            .collect();
                        if !entries.is_empty() {
                            tracing::warn!(
                                target: "perl_lsp::config",
                                source = source.label(),
                                count = entries.len(),
                                "ignored unauthorized externalIncludePaths entries"
                            );
                            rejected.extend(entries.into_iter().map(|entry| {
                                RejectedClientIncludePath {
                                    entry: entry.to_string(),
                                    reason: RejectedClientIncludePathReason::ExternalUnauthorized(
                                        source,
                                    ),
                                }
                            }));
                        }
                    }
                }
            }
            if let Some(extensions) = string_array(workspace.get("discoveryExtensions")) {
                self.discovery_extra_extensions = extensions;
            }
            if let Some(skipped_dirs) = string_array(workspace.get("discoverySkippedDirs")) {
                self.discovery_extra_skipped_dirs = skipped_dirs;
            }
            if let Some(use_inc) = workspace.get("useSystemInc").and_then(|v| v.as_bool()) {
                if use_inc != self.use_system_inc {
                    self.system_inc_cache = None;
                }
                self.use_system_inc = use_inc;
            }
            // Security: do NOT honour workspace-supplied perlPath / perlArgs.
            // Allowing arbitrary Perl interpreter / argv from workspace settings
            // would let a hostile project execute arbitrary code via the @INC
            // probe (issue #3729). The interpreter / args remain whatever the
            // user (not the workspace) configured globally.
            if let Some(timeout) = workspace.get("resolutionTimeout").and_then(as_config_u64) {
                self.resolution_timeout_ms = timeout;
            }
            if let Some(use_p5l) = workspace.get("usePerl5lib").and_then(|v| v.as_bool()) {
                // Invalidate the lazy startup-@INC cache when usePerl5lib toggles, because
                // `fetch_perl_inc` strips PERL5LIB from the probe environment when
                // usePerl5lib is false (and inherits it when true). A cache built under
                // the old setting may include or exclude PERL5LIB paths incorrectly.
                if use_p5l != self.use_perl5lib {
                    self.system_inc_cache = None;
                }
                self.use_perl5lib = use_p5l;
            }
            if let Some(prec) = workspace.get("perl5libPrecedence").and_then(|v| v.as_str()) {
                // Only update on recognised values; leave the current setting unchanged for
                // unknown strings so a typo does not silently reset an explicitly-set Append.
                match prec {
                    "append" => self.perl5lib_precedence = Perl5LibPrecedence::Append,
                    "prepend" => self.perl5lib_precedence = Perl5LibPrecedence::Prepend,
                    other => tracing::warn!(
                        target: "perl_lsp::config",
                        setting = "workspace.perl5libPrecedence",
                        value = %other,
                        valid = "prepend, append",
                        "unrecognized perl5libPrecedence value; keeping current setting",
                    ),
                }
            }
        }
        rejected
    }

    fn ensure_system_inc_probe(&mut self) {
        if self.system_inc_cache.is_some() {
            return;
        }

        // Snapshot the fields needed by the oracle constructor before the
        // mutable borrow below.
        let perl_args = self.perl_args.clone();
        let result = Self::fetch_perl_inc(self, &perl_args);
        self.system_inc_cache = Some(result);
    }

    /// Get the typed system `@INC` probe outcome (lazily populated).
    ///
    /// The probe (`perl -e 'print join("\n", @INC)'`) is bounded by
    /// `SYSTEM_INC_PROBE_TIMEOUT`. The typed result is cached so callers can
    /// distinguish a transient timeout from a spawn failure, nonzero exit,
    /// unavailable oracle, or a successful empty output without changing the
    /// fail-closed behaviour of [`Self::get_system_inc`].
    pub fn get_system_inc_probe_outcome(&mut self) -> SystemIncProbeOutcome {
        if !self.use_system_inc {
            return SystemIncProbeOutcome::Disabled;
        }

        self.ensure_system_inc_probe();
        match self.system_inc_cache.as_ref() {
            Some(outcome) => outcome.clone(),
            None => SystemIncProbeOutcome::Unavailable,
        }
    }

    /// Get system @INC paths (lazily populated).
    ///
    /// Any unavailable or failed probe remains fail-closed as an empty slice;
    /// use [`Self::get_system_inc_probe_outcome`] when the caller needs to
    /// distinguish the failure class. The user can re-trigger probing by
    /// toggling `useSystemInc`, which invalidates the cache.
    ///
    /// The PERL5LIB environment variable is stripped from the probe subprocess
    /// when `use_perl5lib` is false, so interpreter startup `@INC` does not
    /// silently reintroduce PERL5LIB paths that the user has disabled. The two
    /// settings remain independent: PERL5LIB visibility is controlled by
    /// `use_perl5lib`, and startup `@INC` (everything except PERL5LIB) is
    /// controlled by `use_system_inc`.
    pub fn get_system_inc(&mut self) -> &[PathBuf] {
        if !self.use_system_inc {
            return &[];
        }

        self.ensure_system_inc_probe();

        match self.system_inc_cache.as_ref() {
            Some(SystemIncProbeOutcome::Paths(paths)) => paths.as_slice(),
            _ => &[],
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn fetch_perl_inc(config: &WorkspaceConfig, perl_args: &[String]) -> SystemIncProbeOutcome {
        let oracle = match PerlOracleEnv::for_module_resolution(config) {
            Some(o) => o,
            None => {
                tracing::warn!(
                    target: "perl_lsp::config::system_inc",
                    "startup @INC probe unavailable; caching an unavailable outcome"
                );
                return SystemIncProbeOutcome::Unavailable;
            }
        };
        let timeout = oracle.timeout;
        let mut command = oracle.into_command();
        command.args(perl_args);
        command.args(["-e", "print join(\"\\n\", @INC)"]);
        let output = output_with_timeout(command, timeout);

        match &output {
            Ok(out) if !out.status.success() => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!(
                    target: "perl_lsp::config::system_inc",
                    status = ?out.status,
                    stderr = %stderr.trim(),
                    "startup @INC probe exited non-zero; caching empty result"
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                tracing::warn!(
                    target: "perl_lsp::config::system_inc",
                    timeout_ms = timeout.as_millis() as u64,
                    "startup @INC probe timed out; caching empty result. \
                     Set perl.workspace.useSystemInc=false to disable probing, \
                     or pin a faster perl interpreter."
                );
            }
            Err(err) => {
                tracing::warn!(
                    target: "perl_lsp::config::system_inc",
                    error = %err,
                    "startup @INC probe failed to spawn perl; caching empty result"
                );
            }
            _ => {}
        }

        Self::classify_perl_inc_output(output)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn classify_perl_inc_output(output: std::io::Result<Output>) -> SystemIncProbeOutcome {
        match output {
            Ok(out) if out.status.success() => {
                let paths = Self::parse_perl_inc_output(&String::from_utf8_lossy(&out.stdout));
                if paths.is_empty() {
                    SystemIncProbeOutcome::SuccessfulEmpty
                } else {
                    SystemIncProbeOutcome::Paths(paths)
                }
            }
            Ok(_) => SystemIncProbeOutcome::NonZeroExit,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                SystemIncProbeOutcome::TimedOut
            }
            Err(_) => SystemIncProbeOutcome::IoFailed,
        }
    }

    fn parse_perl_inc_output(stdout: &str) -> Vec<PathBuf> {
        stdout
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty()
                    && *line != "."
                    && !line.starts_with("CODE(")
                    && !line.starts_with("ARRAY(")
                    && !line.starts_with("HASH(")
                    && !line.starts_with("SCALAR(")
                    && !line.starts_with("REF(")
                    && !line.starts_with("GLOB(")
                    && !line.starts_with("IO::")
            })
            .map(PathBuf::from)
            .fold(Vec::new(), |mut acc, path| {
                if !acc.contains(&path) {
                    acc.push(path);
                }
                acc
            })
    }

    #[cfg(target_arch = "wasm32")]
    fn fetch_perl_inc(_config: &WorkspaceConfig, _perl_args: &[String]) -> SystemIncProbeOutcome {
        SystemIncProbeOutcome::Unavailable
    }
}

/// Bounded interpreter startup `@INC` probe.
///
/// The probe is intentionally a separate constant from
/// `WorkspaceConfig::resolution_timeout_ms` (50 ms default). 50 ms is well
/// under Perl interpreter startup on most platforms — a perlbrew shim,
/// remote filesystem, or even a cold cache can comfortably exceed it.
/// 1000 ms is short enough that a stalled probe does not noticeably block
/// the LSP and long enough that healthy probes succeed reliably.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SYSTEM_INC_PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// Run `command` with a wall-clock timeout, killing the child if it exceeds
/// `timeout`. Returns `io::Error` with kind `TimedOut` on timeout. Used by
/// `fetch_perl_inc` so a hanging or slow `perl` interpreter cannot stall
/// the LSP indefinitely.
///
/// Polls `try_wait` every 20 ms. The 20 ms granularity is acceptable for the
/// startup-`@INC` probe — total overhead at the bound is at most one extra
/// poll tick.
#[cfg(not(target_arch = "wasm32"))]
fn output_with_timeout(mut command: Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let start = Instant::now();
    let poll_interval = Duration::from_millis(20);

    loop {
        match child.try_wait()? {
            Some(_) => return child.wait_with_output(),
            None => {
                if start.elapsed() >= timeout {
                    // Best-effort kill; ignore errors from already-exited processes.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("subprocess exceeded {timeout:?}"),
                    ));
                }
                std::thread::sleep(poll_interval);
            }
        }
    }
}

// ── ProjectConfig ─────────────────────────────────────────────────────────────

/// Project configuration loaded from `.perl-lsp.toml` in the workspace root.
///
/// Committed to the repo; provides editor-agnostic, team-wide defaults.
/// LSP `initializationOptions` / `didChangeConfiguration` always win over this file.
///
/// Unknown TOML keys are silently ignored for forward compatibility.
#[non_exhaustive]
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    /// `[perl]` section: module resolution settings.
    pub perl: ProjectPerlConfig,
    /// `[diagnostics]` section: linting settings.
    pub diagnostics: ProjectDiagnosticsConfig,
    /// `[features]` section: LSP feature toggles.
    pub features: ProjectFeaturesConfig,
    /// `[ai_completion]` section: AI completion settings.
    pub ai_completion: ProjectAiCompletionConfig,
    /// `[next_edit]` section: gated next-edit settings.
    pub next_edit: ProjectNextEditConfig,
    /// `[formatting]` section: native formatter and legacy adapter configuration.
    pub formatting: ProjectFormattingConfig,
    /// `[critic]` section: native critic and legacy adapter configuration.
    pub critic: ProjectCriticConfig,
}

/// `[perl]` section of `.perl-lsp.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectPerlConfig {
    /// Additional include paths for module resolution.
    ///
    /// Relative entries are resolved against the workspace root. Absolute
    /// entries are honored literally as external include roots.
    pub include_paths: Vec<String>,
    /// Additional file extensions accepted during workspace discovery.
    pub discovery_extensions: Vec<String>,
    /// Additional directory names skipped during workspace discovery.
    pub discovery_skipped_dirs: Vec<String>,
    /// Perl version string (e.g. "5.38") — parsed but not yet wired to diagnostics.
    /// Reserved for future use; ignored in this implementation.
    pub version: Option<String>,
    /// Whether to read `PERL5LIB` from the environment and include it in the
    /// module search path.  Unset means "leave the server default unchanged".
    pub use_perl5lib: Option<bool>,
    /// Whether PERL5LIB paths come before or after `include_paths`.
    /// Unset means "leave the server default unchanged".
    pub perl5lib_precedence: Option<Perl5LibPrecedence>,
}

/// `[diagnostics]` section of `.perl-lsp.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectDiagnosticsConfig {
    /// Whether perlcritic is enabled. Maps to `ServerConfig.perlcritic_enabled`.
    pub perlcritic: Option<bool>,
    /// Minimum perlcritic severity (1-5). Maps to `ServerConfig.perlcritic_severity`.
    pub perlcritic_severity: Option<u8>,
}

/// `[critic]` section of `.perl-lsp.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectCriticConfig {
    /// Critic engine (`legacy`, `perlcritic`, or `native`).
    pub engine: Option<String>,
    /// Native critic profile (`recommended` or `strict`).
    pub profile: Option<String>,
    /// Native critic rule IDs to include. Empty means use the selected profile.
    pub include: Option<Vec<String>>,
    /// Native critic rule IDs to exclude from the selected profile.
    pub exclude: Option<Vec<String>>,
}

/// `[features]` section of `.perl-lsp.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectFeaturesConfig {
    /// Whether inlay hints are enabled globally. Maps to `ServerConfig.inlay_hints_enabled`.
    pub inlay_hints: Option<bool>,
}

/// `[ai_completion]` section of `.perl-lsp.toml`.
///
/// Security: this struct intentionally does NOT carry `endpoint`,
/// `api_key_env`, `api_key_header`, or `api_key_prefix`. Those four fields
/// select a network destination and a process-environment credential; if a
/// workspace-supplied `.perl-lsp.toml` could set them, a hostile cloned
/// repository could name an arbitrary environment variable (e.g.
/// `AWS_SECRET_ACCESS_KEY`) and have its value POSTed to an attacker-chosen
/// endpoint on the first inline-completion request (issue #4955). See the
/// analogous `perlPath`/`perlArgs` precedent below (issue #3729).
///
/// `enabled`, `provider`, and `model` are also not workspace-authoritative
/// (issue #4997): project config may only opt out (`enabled = false`), never
/// activate a remote AI backend or override user-owned provider/model choice.
/// Those settings arrive only through the LSP client configuration channel
/// (`ServerConfig::update_from_value`'s `aiCompletion` block).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectAiCompletionConfig {
    /// Opt-out only: when `false`, disables AI completions for this workspace.
    /// `true` is ignored — a repository cannot turn AI on.
    pub enabled: Option<bool>,
}

/// `[next_edit]` section of `.perl-lsp.toml`.
#[non_exhaustive]
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectNextEditConfig {
    /// Whether the future next-edit runtime boundary is explicitly enabled.
    pub enabled: Option<bool>,
}

/// `[formatting]` section of `.perl-lsp.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct ProjectFormattingConfig {
    /// Whether LSP formatting is enabled.
    pub enabled: Option<bool>,
    /// Whether to format on save (willSaveWaitUntil). Default `true`.
    pub format_on_save: Option<bool>,
    /// Formatter engine (`native`, `compat`, `external-perltidy`, or `off`).
    pub engine: Option<String>,
    /// Path to a `.perltidyrc` profile file.
    pub perltidy_profile: Option<String>,
    /// Maximum line length.
    pub perltidy_maximum_line_length: Option<u32>,
    /// Indent size in spaces.
    pub perltidy_indent_columns: Option<u32>,
    /// Use tabs instead of spaces.
    pub perltidy_tabs: Option<bool>,
    /// Opening brace on new line.
    pub perltidy_opening_brace_on_new_line: Option<bool>,
    /// Cuddled else style.
    pub perltidy_cuddled_else: Option<bool>,
    /// Space after keyword.
    pub perltidy_space_after_keyword: Option<bool>,
    /// Add trailing commas.
    pub perltidy_add_trailing_commas: Option<bool>,
    /// Vertical alignment.
    pub perltidy_vertical_alignment: Option<bool>,
    /// Block comment indentation.
    pub perltidy_block_comment_indentation: Option<u32>,
    /// Extra perltidy arguments.
    pub perltidy_extra_args: Vec<String>,
    /// Timeout in seconds.
    pub perltidy_timeout_secs: Option<u64>,
}

/// Walk parent directories from `start_dir` upward looking for `.perl-lsp.toml`.
///
/// This mirrors the parent-walk strategy used by `.perlcriticrc` discovery
/// (`find_workspace_perlcritic_profile`) so that a monorepo opened at a
/// subdirectory still finds a root-level `.perl-lsp.toml`. The search starts
/// at `start_dir` itself and proceeds upward to the filesystem root.
///
/// Returns the path to the first `.perl-lsp.toml` found, or `None` if no
/// candidate exists in any ancestor directory.
fn discover_project_config_path(start_dir: &Path) -> Option<PathBuf> {
    let mut current = Some(start_dir);
    while let Some(dir) = current {
        let candidate = dir.join(".perl-lsp.toml");
        // Use `exists()` (not `is_file()`) so that a non-regular entry (e.g. a
        // directory named `.perl-lsp.toml`) is still surfaced to the caller —
        // the metadata checks in `load_project_config` will reject it with a
        // descriptive error rather than silently skipping it.
        if candidate.exists() {
            return Some(candidate);
        }
        current = dir.parent();
    }
    None
}

/// Load project config from `.perl-lsp.toml`, searching upward from
/// `workspace_root`.
///
/// The search walks parent directories from `workspace_root` to the filesystem
/// root, returning the first `.perl-lsp.toml` found. This matches the
/// parent-walk behavior of `.perlcriticrc` discovery so that a monorepo
/// opened at a subdirectory discovers a root-level config.
///
/// Returns `None` if no `.perl-lsp.toml` exists in any ancestor (normal case —
/// most projects won't have one). Returns `Err` on TOML parse failure, I/O
/// errors, oversized files, or non-regular paths; caller should emit a
/// `window/showMessage` warning and continue with defaults.
pub fn load_project_config(
    workspace_root: &std::path::Path,
) -> Result<Option<ProjectConfig>, String> {
    const MAX_PROJECT_CONFIG_BYTES: u64 = 1024 * 1024; // 1 MiB

    let path = match discover_project_config_path(workspace_root) {
        Some(p) => p,
        None => return Ok(None),
    };
    let metadata = match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "Could not read .perl-lsp.toml: {}. \
                 Check that the file is readable and not locked by another process.",
                e
            ));
        }
        Ok(metadata) => metadata,
    };

    if !metadata.file_type().is_file() {
        return Err(
            "Could not read .perl-lsp.toml: path must be a regular file (not a directory, pipe, \
             or device)."
                .to_string(),
        );
    }

    if metadata.len() > MAX_PROJECT_CONFIG_BYTES {
        return Err(format!(
            "Could not read .perl-lsp.toml: file is too large ({} bytes, max {} bytes).",
            metadata.len(),
            MAX_PROJECT_CONFIG_BYTES
        ));
    }

    // Open the file; guard against a TOCTOU race where the file is removed after
    // the metadata check succeeds — treat a vanished file the same as not-found.
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "Could not read .perl-lsp.toml: {}. \
                 Check that the file is readable and not locked by another process.",
                e
            ));
        }
    };
    let mut content = String::new();
    file.take(MAX_PROJECT_CONFIG_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(|e| format!("Could not read .perl-lsp.toml: {}", e))?;

    if content.len() as u64 > MAX_PROJECT_CONFIG_BYTES {
        return Err(format!(
            "Could not read .perl-lsp.toml: file is too large ({} bytes, max {} bytes). \
             The file may have grown between the size check and the read.",
            content.len(),
            MAX_PROJECT_CONFIG_BYTES
        ));
    }

    // Strip BOM if present — some Windows editors save .perl-lsp.toml with a
    // UTF-8 BOM, which causes toml::from_str to report a syntax error at byte 0.
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);

    toml::from_str::<ProjectConfig>(content)
        .map(Some)
        .map_err(|e| format!(".perl-lsp.toml has a syntax error: {}", e))
}

/// Discover a `.perltidyrc` profile for a workspace following perltidy's
/// conventional search order.
///
/// Standard Perl tooling auto-discovers a profile so that a project-local
/// `.perltidyrc` applies without any editor configuration. This mirrors that
/// behavior while keeping the project-local profile first (the LSP-appropriate
/// priority). The search order is:
///
/// 1. `<workspace_root>/.perltidyrc`, then `<workspace_root>/perltidyrc`
/// 2. The file named by the `PERLTIDY` environment variable (perltidy's
///    documented override, searched before the home profile)
/// 3. `$HOME/.perltidyrc`
///
/// Returns the first existing profile path as a string, or `None` to let
/// perltidy fall back to its own defaults. This function only consults the
/// filesystem; callers are responsible for preferring an explicitly configured
/// profile over the discovered one.
pub fn discover_perltidy_profile(workspace_root: &Path) -> Option<String> {
    discover_perltidy_profile_from(
        workspace_root,
        std::env::var_os("PERLTIDY").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// Pure, dependency-injected core of [`discover_perltidy_profile`].
///
/// Separated so tests can exercise the search order deterministically without
/// mutating process-global environment variables. `env_profile` is the file
/// named by perltidy's `PERLTIDY` environment variable; `home` is the user's
/// home directory. Per perltidy's documented convention the environment
/// override is searched before the home profile.
fn discover_perltidy_profile_from(
    workspace_root: &Path,
    env_profile: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<String> {
    for name in [".perltidyrc", "perltidyrc"] {
        let candidate = workspace_root.join(name);
        if candidate.is_file() {
            return candidate.to_str().map(ToOwned::to_owned);
        }
    }

    if let Some(env_profile) = env_profile
        && env_profile.is_file()
    {
        return env_profile.to_str().map(ToOwned::to_owned);
    }

    if let Some(home) = home {
        let candidate = home.join(".perltidyrc");
        if candidate.is_file() {
            return candidate.to_str().map(ToOwned::to_owned);
        }
    }

    None
}

impl ServerConfig {
    /// Apply native-formatter scalar options parsed from a `.perltidyrc` profile
    /// as a base layer, overwriting only the fields the profile actually sets.
    ///
    /// This is intended to run **after** the built-in defaults but **before**
    /// user configuration (`.perl-lsp.toml` / `didChangeConfiguration`), so a
    /// project-local profile beats the built-in defaults while an explicitly
    /// configured field still wins. It must not be applied at format time:
    /// because the built-in defaults are `Some(..)`, a per-request `.or()` merge
    /// can never reach the profile value.
    pub fn apply_perltidy_native_options(
        &mut self,
        options: &crate::tooling::native_compat::PerltidyNativeConfigSuggestion,
    ) {
        if let Some(value) = options.perltidy_maximum_line_length {
            self.perltidy_maximum_line_length = Some(value);
        }
        if let Some(value) = options.perltidy_indent_columns {
            self.perltidy_indent_columns = Some(value);
        }
        if let Some(value) = options.perltidy_tabs {
            self.perltidy_tabs = Some(value);
        }
        if let Some(value) = options.perltidy_opening_brace_on_new_line {
            self.perltidy_opening_brace_on_new_line = Some(value);
        }
        if let Some(value) = options.perltidy_cuddled_else {
            self.perltidy_cuddled_else = Some(value);
        }
        if let Some(value) = options.perltidy_space_after_keyword {
            self.perltidy_space_after_keyword = Some(value);
        }
        if let Some(value) = options.perltidy_add_trailing_commas {
            self.perltidy_add_trailing_commas = Some(value);
        }
    }
}

impl ProjectConfig {
    /// Apply project config to `ServerConfig` as the base layer.
    ///
    /// Only fields explicitly set in the TOML override defaults; unset fields are untouched.
    /// LSP `didChangeConfiguration` is expected to run after this, overriding any values here.
    pub fn apply_to_server_config(&self, config: &mut ServerConfig) {
        if let Some(hints) = self.features.inlay_hints {
            config.inlay_hints_enabled = hints;
        }
        // Security: project config may only opt out of AI completions, never
        // enable them or override user-owned provider/model (issue #4997).
        if self.ai_completion.enabled == Some(true) {
            tracing::warn!(
                target: "perl_lsp::config",
                setting = "ai_completion.enabled",
                "workspace-supplied ai_completion.enabled=true is ignored; \
                 AI completions require user-level configuration",
            );
        }
        config.ai_completion.project_opt_out = self.ai_completion.enabled == Some(false);
        // Security: do NOT honour workspace-supplied endpoint / api_key_env /
        // api_key_header / api_key_prefix. Allowing a hostile project to pick
        // both the destination and the process-environment credential name
        // would let it exfiltrate an arbitrary named secret (issue #4955).
        // These settings arrive only via the LSP client/server configuration
        // channel. Project activation is closed here (#4997); VS Code declares
        // AI toggles `scope: machine`. Non-VS Code clients that forward
        // workspace settings into `didChangeConfiguration` remain a residual
        // provenance gap (documented in AI_COMPLETION.md).
        recompute_ai_completion_effective(&mut config.ai_completion);
        if let Some(enabled) = self.next_edit.enabled {
            config.next_edit.enabled = enabled;
        }

        // Apply formatting configuration
        if let Some(enabled) = self.formatting.enabled {
            config.perltidy_enabled = enabled;
        }
        if let Some(format_on_save) = self.formatting.format_on_save {
            config.format_on_save = format_on_save;
        }
        if let Some(ref engine) = self.formatting.engine {
            match parse_formatter_mode(engine) {
                Some(mode) => config.formatting_engine = mode,
                None => tracing::warn!(
                    target: "perl_lsp::config",
                    setting = "formatting.engine",
                    value = %engine,
                    valid = FORMATTER_MODE_VALID_OPTIONS,
                    "unrecognized formatting.engine value in .perl-lsp.toml; \
                     keeping current setting",
                ),
            }
        }
        // Critic initialization from the trusted project file also advances as
        // ONE accepted transaction (#8253): `[diagnostics]` enablement/severity
        // and `[critic]` engine/profile/include/exclude are validated together,
        // and an invalid sibling rejects the whole candidate while the complete
        // prior accepted state is retained with one deduplicated condition.
        match CriticSettingsCandidate::parse_project_config(&self.diagnostics, &self.critic) {
            Ok(candidate) => {
                if !candidate.is_empty() {
                    candidate.apply_to(config);
                }
            }
            Err(rejection) => rejection.emit_single_condition(),
        }
        if let Some(ref profile) = self.formatting.perltidy_profile {
            config.perltidy_profile = Some(profile.clone());
        }
        if let Some(len) = self.formatting.perltidy_maximum_line_length {
            config.perltidy_maximum_line_length = Some(len);
        }
        if let Some(indent) = self.formatting.perltidy_indent_columns {
            config.perltidy_indent_columns = Some(indent);
        }
        if let Some(tabs) = self.formatting.perltidy_tabs {
            config.perltidy_tabs = Some(tabs);
        }
        if let Some(brace) = self.formatting.perltidy_opening_brace_on_new_line {
            config.perltidy_opening_brace_on_new_line = Some(brace);
        }
        if let Some(cuddle) = self.formatting.perltidy_cuddled_else {
            config.perltidy_cuddled_else = Some(cuddle);
        }
        if let Some(space) = self.formatting.perltidy_space_after_keyword {
            config.perltidy_space_after_keyword = Some(space);
        }
        if let Some(comma) = self.formatting.perltidy_add_trailing_commas {
            config.perltidy_add_trailing_commas = Some(comma);
        }
        if let Some(align) = self.formatting.perltidy_vertical_alignment {
            config.perltidy_vertical_alignment = Some(align);
        }
        if let Some(block) = self.formatting.perltidy_block_comment_indentation {
            config.perltidy_block_comment_indentation = Some(block);
        }
        if !self.formatting.perltidy_extra_args.is_empty() {
            config.perltidy_extra_args = self.formatting.perltidy_extra_args.clone();
        }
        if let Some(timeout) = self.formatting.perltidy_timeout_secs {
            config.perltidy_timeout_secs = timeout;
        }
    }

    /// Apply project config to `WorkspaceConfig` as the base layer.
    ///
    /// Only applies list settings when their TOML lists are non-empty, so that
    /// absent keys leave defaults unchanged (distinct from explicit `[]`).
    ///
    /// # Security
    ///
    /// `include_paths` entries come from `.perl-lsp.toml`, a file checked into
    /// the (possibly hostile) cloned workspace — an **untrusted** channel.
    /// The LSP client-settings channel applied via
    /// [`WorkspaceConfig::update_from_value`] is NOT validated here; its
    /// provenance is not distinguished in this slice (see issue #4998 — in
    /// VS Code `perl-lsp.includePaths` is `scope: resource`, so workspace and
    /// folder values reach it too). Mirroring the `perlPath` /
    /// `perlArgs` precedent (issue #3729, see the comment at
    /// `update_from_value`), this rejects entries that could let a hostile
    /// project read outside the workspace:
    ///
    /// - Absolute entries are always rejected. Legitimate external lib roots
    ///   must be configured through the LSP client-settings channel, not via
    ///   workspace-supplied `.perl-lsp.toml`. Note that channel is itself
    ///   pending provenance hardening (issue #4998); this slice only closes
    ///   the `.perl-lsp.toml` route.
    /// - Relative entries that escape the workspace root after normalization
    ///   (lexical `..` traversal, or a symlink that resolves outside the
    ///   workspace) are rejected.
    ///
    /// Rejected entries are dropped from `config.include_paths` (never
    /// silently applied) and returned so the caller can surface an actionable
    /// warning — a bad entry must be debuggable, not just silently ignored.
    pub fn apply_to_workspace_config(
        &self,
        config: &mut WorkspaceConfig,
        workspace_root: &Path,
    ) -> Vec<RejectedIncludePath> {
        let mut rejected = Vec::new();
        let mut skip_include_paths = false;
        if !self.perl.include_paths.is_empty() {
            // Fail closed: if the workspace root itself cannot be canonicalized
            // there is nothing to validate containment against, so no entry can
            // be trusted. Detected once here rather than inferred per-entry, so
            // the reported reason names the actual cause instead of blaming
            // each path for "escaping" a root that could not be read.
            if let Err(err) = std::fs::canonicalize(workspace_root) {
                // The failure belongs to the workspace root, not to any single
                // entry, so record it once instead of repeating an identical
                // warning per configured path.
                rejected.push(RejectedIncludePath {
                    entry: workspace_root.display().to_string(),
                    reason: RejectedIncludePathReason::WorkspaceRootUnavailable(err.to_string()),
                });
                // Fail closed for include roots: entries are dropped, not merely
                // reported, since leaving a previously-set list in place would
                // let an unvalidated path stay live.
                //
                // Scoped deliberately: this is a path-validation failure, not a
                // failure of the whole project config, so discovery extensions,
                // skip lists and the perl5lib settings below still apply.
                config.include_paths.clear();
                skip_include_paths = true;
            }
            let mut valid = Vec::with_capacity(self.perl.include_paths.len());
            for entry in self.perl.include_paths.iter().filter(|_| !skip_include_paths) {
                let candidate = Path::new(entry);
                if candidate.is_absolute() {
                    rejected.push(RejectedIncludePath {
                        entry: entry.clone(),
                        reason: RejectedIncludePathReason::Absolute,
                    });
                    continue;
                }
                if let Err(err) = validate_workspace_path(candidate, workspace_root) {
                    rejected.push(RejectedIncludePath {
                        entry: entry.clone(),
                        reason: RejectedIncludePathReason::from_path_error(&err),
                    });
                    continue;
                }
                valid.push(entry.clone());
            }
            if !skip_include_paths {
                config.include_paths = valid;
            }
        }
        if !self.perl.discovery_extensions.is_empty() {
            config.discovery_extra_extensions =
                normalize_string_list(&self.perl.discovery_extensions);
        }
        if !self.perl.discovery_skipped_dirs.is_empty() {
            config.discovery_extra_skipped_dirs =
                normalize_string_list(&self.perl.discovery_skipped_dirs);
        }
        if let Some(use_p5l) = self.perl.use_perl5lib {
            config.use_perl5lib = use_p5l;
        }
        if let Some(ref prec) = self.perl.perl5lib_precedence {
            config.perl5lib_precedence = prec.clone();
        }
        rejected
    }
}

/// A `.perl-lsp.toml` `[perl].include_paths` entry rejected during validation.
///
/// Rejection happens in [`ProjectConfig::apply_to_workspace_config`]; see its
/// `# Security` doc comment for the trust-boundary rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedIncludePath {
    /// The raw, as-configured entry string.
    pub entry: String,
    /// Why it was rejected.
    pub reason: RejectedIncludePathReason,
}

/// Why a `.perl-lsp.toml` `include_paths` entry was rejected.
///
/// These categories mirror [`WorkspacePathError`] rather than collapsing into
/// one bucket: this is a public operational surface (doctor output,
/// `window/showMessage`), and "escapes the workspace root" is simply false for
/// an entry containing null bytes or for an unreadable workspace root.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectedIncludePathReason {
    /// Absolute paths are never honoured from workspace-supplied
    /// `.perl-lsp.toml` (mirrors the perlPath/perlArgs precedent, issue
    /// #3729). External lib roots must come through the LSP client-settings
    /// channel instead — which is itself pending provenance hardening under
    /// issue #4998, so this is a narrower statement than "trusted".
    Absolute,
    /// Lexical `..` traversal out of the workspace.
    Traversal(String),
    /// Normalizes to a location outside the workspace root.
    OutsideWorkspace(String),
    /// A symlink component resolves to a target outside the workspace root.
    SymlinkOutsideWorkspace(String),
    /// Null bytes or disallowed control characters. Checked before any
    /// containment logic, so this is not a containment failure.
    InvalidCharacters,
    /// The workspace root itself could not be canonicalized (missing,
    /// unreadable, or not a directory), so no entry could be validated
    /// against it. Every entry is rejected — this is the fail-closed case.
    WorkspaceRootUnavailable(String),
}

/// Escape control characters in a workspace-controlled string before it reaches
/// a terminal or an editor message.
///
/// `include_paths` entries come from `.perl-lsp.toml`, which a hostile cloned
/// repository controls. Rendering them raw lets an entry inject ANSI/OSC escape
/// sequences into `perl-lsp doctor` output and `window/showMessage` — so the
/// warning *about* a malicious path would itself be the injection vector
/// (CWE-150).
///
/// Note the absolute-path branch of `apply_to_workspace_config` rejects before
/// `validate_workspace_path` runs, so its `InvalidPathCharacters` check never
/// sees those entries. Escaping centrally here covers every rejection reason
/// regardless of which branch produced it.
fn escape_for_display(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| {
            if c == '\t' || !needs_display_escape(c) {
                vec![c]
            } else {
                format!("\\u{{{:04x}}}", c as u32).chars().collect()
            }
        })
        .collect()
}

/// Characters that must not reach a terminal or editor message verbatim.
///
/// `char::is_control` covers C0, DEL and C1 — which handles ANSI/OSC injection.
/// It does NOT cover the Unicode bidirectional formatting characters, which are
/// category `Cf` rather than `Cc`. Those visually reorder surrounding text
/// without being control codes (the "Trojan Source" class, CVE-2021-42574), so
/// a rejected entry could still render as a path other than the one configured.
/// Zero-width characters are included for the same reason: they let two
/// different entries display identically.
fn needs_display_escape(c: char) -> bool {
    if c.is_control() {
        return true;
    }
    matches!(
        c,
        // zero-width space/ZWNJ/ZWJ, LRM/RLM
        '\u{200b}'..='\u{200f}'
        // LRE, RLE, PDF, LRO, RLO
        | '\u{202a}'..='\u{202e}'
        // LRI, RLI, FSI, PDI
        | '\u{2066}'..='\u{2069}'
        // zero-width no-break space / BOM
        | '\u{feff}'
    )
}

impl RejectedIncludePath {
    /// Render a single human-readable line for `window/showMessage` / doctor reports.
    ///
    /// The entry is escaped via [`escape_for_display`]; it is workspace-controlled.
    #[must_use]
    pub fn render(&self) -> String {
        let entry = escape_for_display(&self.entry);
        match &self.reason {
            RejectedIncludePathReason::Absolute => format!(
                "'{}': absolute include_paths are not allowed in .perl-lsp.toml \
                 (workspace-supplied). Configure external lib roots in your own \
                 editor settings instead; not in a file checked into the repository.",
                entry
            ),
            RejectedIncludePathReason::Traversal(detail) => {
                format!("'{}': traverses out of the workspace root ({detail})", entry)
            }
            RejectedIncludePathReason::OutsideWorkspace(detail) => {
                format!("'{}': resolves outside the workspace root ({detail})", entry)
            }
            RejectedIncludePathReason::SymlinkOutsideWorkspace(detail) => {
                format!(
                    "'{}': a symlink in this path resolves outside the workspace root ({detail})",
                    entry
                )
            }
            RejectedIncludePathReason::InvalidCharacters => {
                format!("'{}': contains null bytes or disallowed control characters", entry)
            }
            RejectedIncludePathReason::WorkspaceRootUnavailable(detail) => {
                format!(
                    "'{}': the workspace root could not be resolved, so no include_paths \
                     entry could be validated ({detail})",
                    entry
                )
            }
        }
    }
}

impl RejectedIncludePathReason {
    /// Map a [`WorkspacePathError`] onto the matching rejection category.
    ///
    /// Kept exhaustive on purpose: a new `WorkspacePathError` variant must be
    /// classified here rather than silently inheriting a generic bucket.
    fn from_path_error(err: &WorkspacePathError) -> Self {
        match err {
            WorkspacePathError::PathTraversalAttempt(d) => Self::Traversal(d.clone()),
            WorkspacePathError::PathOutsideWorkspace(d) => Self::OutsideWorkspace(d.clone()),
            WorkspacePathError::SymlinkOutsideWorkspace(d) => {
                Self::SymlinkOutsideWorkspace(d.clone())
            }
            WorkspacePathError::InvalidPathCharacters => Self::InvalidCharacters,
        }
    }
}

/// A conflict between two or more workspace folders over a shared (server-global)
/// setting sourced from `.perl-lsp.toml`.
///
/// Produced by [`merge_project_configs_for_server`]. The `[perl]` section is
/// intentionally excluded because it is already scoped per-folder via
/// `WorkspaceConfig`; only the six server-global sections (`[diagnostics]`,
/// `[critic]`, `[features]`, `[formatting]`, `[ai_completion]`, `[next_edit]`)
/// participate in the merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiRootConfigConflict {
    /// Dotted path of the conflicting key, e.g. `"diagnostics.perlcritic"`.
    pub key: &'static str,
    /// Display names of the folders that set this key, in iteration order.
    pub folders: Vec<String>,
    /// Human-readable rendering of the differing values, parallel to `folders`.
    pub values: Vec<String>,
}

impl MultiRootConfigConflict {
    /// Render this conflict as `"key (folderA=vA, folderB=vB)"`.
    #[must_use]
    pub fn render(&self) -> String {
        let pairs = self
            .folders
            .iter()
            .zip(self.values.iter())
            .map(|(folder, value)| format!("{folder}={value}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{} ({})", self.key, pairs)
    }
}

/// Merge the server-global sections of multiple folders' `ProjectConfig`s into a
/// single `ProjectConfig`, using **first-set-wins** per field.
///
/// In a multi-root workspace, each folder's `.perl-lsp.toml` is loaded
/// independently. The `[perl]` section is correctly scoped per-folder through
/// `WorkspaceConfig`, but the other six sections (`[diagnostics]`, `[critic]`,
/// `[features]`, `[formatting]`, `[ai_completion]`, `[next_edit]`) target the
/// single shared `ServerConfig`. Applying every folder's config in a loop would
/// silently let the last folder win for any field set by more than one folder.
///
/// This function instead produces a merged `ProjectConfig` where each field
/// takes the value from the **first** folder (in iteration order) that sets it,
/// so a single subsequent `apply_to_server_config` call cannot clobber earlier
/// folders. Fields that two or more folders set to *different* values are
/// reported as conflicts so the caller can emit a user-visible warning instead
/// of silently discarding a folder's setting.
///
/// Folders that have no `.perl-lsp.toml` (a `None` project config) are skipped
/// by the caller before invoking this function.
///
/// Returns `(merged_config, conflicts)`.
#[must_use]
pub fn merge_project_configs_for_server(
    folders: &[(&str, &ProjectConfig)],
) -> (ProjectConfig, Vec<MultiRootConfigConflict>) {
    let mut merged = ProjectConfig::default();
    let mut conflicts: Vec<MultiRootConfigConflict> = Vec::new();

    // `[diagnostics]`
    merge_opt_field(
        &mut merged.diagnostics.perlcritic,
        &mut conflicts,
        "diagnostics.perlcritic",
        folders,
        |c| c.diagnostics.perlcritic,
    );
    merge_opt_field(
        &mut merged.diagnostics.perlcritic_severity,
        &mut conflicts,
        "diagnostics.perlcritic_severity",
        folders,
        |c| c.diagnostics.perlcritic_severity,
    );

    // `[features]`
    merge_opt_field(
        &mut merged.features.inlay_hints,
        &mut conflicts,
        "features.inlay_hints",
        folders,
        |c| c.features.inlay_hints,
    );

    // `[ai_completion]` — project may only opt out (`enabled = false`); never merge
    // `enabled = true`, `provider`, or `model` (issue #4997).
    merge_ai_completion_project_opt_out(&mut merged.ai_completion, &mut conflicts, folders);
    // Security: `endpoint` / `api_key_env` / `api_key_header` / `api_key_prefix`
    // are intentionally absent from `ProjectAiCompletionConfig` (issue #4955)
    // and therefore have nothing to merge here.

    // `[next_edit]`
    merge_opt_field(
        &mut merged.next_edit.enabled,
        &mut conflicts,
        "next_edit.enabled",
        folders,
        |c| c.next_edit.enabled,
    );

    // `[formatting]`
    merge_opt_field(
        &mut merged.formatting.enabled,
        &mut conflicts,
        "formatting.enabled",
        folders,
        |c| c.formatting.enabled,
    );
    merge_opt_field(
        &mut merged.formatting.engine,
        &mut conflicts,
        "formatting.engine",
        folders,
        |c| c.formatting.engine.clone(),
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_profile,
        &mut conflicts,
        "formatting.perltidy_profile",
        folders,
        |c| c.formatting.perltidy_profile.clone(),
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_maximum_line_length,
        &mut conflicts,
        "formatting.perltidy_maximum_line_length",
        folders,
        |c| c.formatting.perltidy_maximum_line_length,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_indent_columns,
        &mut conflicts,
        "formatting.perltidy_indent_columns",
        folders,
        |c| c.formatting.perltidy_indent_columns,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_tabs,
        &mut conflicts,
        "formatting.perltidy_tabs",
        folders,
        |c| c.formatting.perltidy_tabs,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_opening_brace_on_new_line,
        &mut conflicts,
        "formatting.perltidy_opening_brace_on_new_line",
        folders,
        |c| c.formatting.perltidy_opening_brace_on_new_line,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_cuddled_else,
        &mut conflicts,
        "formatting.perltidy_cuddled_else",
        folders,
        |c| c.formatting.perltidy_cuddled_else,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_space_after_keyword,
        &mut conflicts,
        "formatting.perltidy_space_after_keyword",
        folders,
        |c| c.formatting.perltidy_space_after_keyword,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_add_trailing_commas,
        &mut conflicts,
        "formatting.perltidy_add_trailing_commas",
        folders,
        |c| c.formatting.perltidy_add_trailing_commas,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_vertical_alignment,
        &mut conflicts,
        "formatting.perltidy_vertical_alignment",
        folders,
        |c| c.formatting.perltidy_vertical_alignment,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_block_comment_indentation,
        &mut conflicts,
        "formatting.perltidy_block_comment_indentation",
        folders,
        |c| c.formatting.perltidy_block_comment_indentation,
    );
    merge_opt_field(
        &mut merged.formatting.perltidy_timeout_secs,
        &mut conflicts,
        "formatting.perltidy_timeout_secs",
        folders,
        |c| c.formatting.perltidy_timeout_secs,
    );
    merge_vec_field(
        &mut merged.formatting.perltidy_extra_args,
        &mut conflicts,
        "formatting.perltidy_extra_args",
        folders,
        |c| &c.formatting.perltidy_extra_args,
    );

    // `[critic]`
    merge_opt_field(&mut merged.critic.engine, &mut conflicts, "critic.engine", folders, |c| {
        c.critic.engine.clone()
    });
    merge_opt_field(&mut merged.critic.profile, &mut conflicts, "critic.profile", folders, |c| {
        c.critic.profile.clone()
    });
    merge_opt_field(&mut merged.critic.include, &mut conflicts, "critic.include", folders, |c| {
        c.critic.include.clone()
    });
    merge_opt_field(&mut merged.critic.exclude, &mut conflicts, "critic.exclude", folders, |c| {
        c.critic.exclude.clone()
    });

    (merged, conflicts)
}

/// Merge project AI-completion opt-outs across folders.
///
/// Workspace/project config may only disable AI (`enabled = false`). Values of
/// `enabled = true` are ignored and never merged (issue #4997).
fn merge_ai_completion_project_opt_out(
    merged: &mut ProjectAiCompletionConfig,
    conflicts: &mut Vec<MultiRootConfigConflict>,
    folders: &[(&str, &ProjectConfig)],
) {
    let mut saw_false: Vec<(String, String)> = Vec::new();
    let mut saw_true: Vec<(String, String)> = Vec::new();

    for (name, cfg) in folders {
        match cfg.ai_completion.enabled {
            Some(false) => {
                saw_false.push((name.to_string(), "false".to_string()));
            }
            Some(true) => {
                saw_true.push((name.to_string(), "true".to_string()));
            }
            None => {}
        }
    }

    if !saw_false.is_empty() && !saw_true.is_empty() {
        let folders: Vec<String> =
            saw_false.iter().chain(saw_true.iter()).map(|(folder, _)| folder.clone()).collect();
        let values: Vec<String> =
            saw_false.iter().chain(saw_true.iter()).map(|(_, value)| value.clone()).collect();
        conflicts.push(MultiRootConfigConflict { key: "ai_completion.enabled", folders, values });
    }

    if !saw_false.is_empty() {
        merged.enabled = Some(false);
    }
}

/// First-set-wins merge for a single `Option<T>` field across folders, recording
/// a conflict when two or more folders set the field to different values.
fn merge_opt_field<T: Clone + PartialEq + std::fmt::Debug>(
    merged: &mut Option<T>,
    conflicts: &mut Vec<MultiRootConfigConflict>,
    key: &'static str,
    folders: &[(&str, &ProjectConfig)],
    extract: impl Fn(&ProjectConfig) -> Option<T>,
) {
    let mut seen: Vec<(String, String)> = Vec::new();
    for (name, cfg) in folders {
        let Some(value) = extract(cfg) else { continue };
        if merged.is_none() {
            *merged = Some(value.clone());
        }
        let value_str = value_to_string(&value);
        if !seen.iter().any(|(_, v)| v == &value_str) {
            seen.push((name.to_string(), value_str));
        }
    }
    push_conflict(conflicts, key, seen);
}

/// First-set-wins merge for a single `Vec<String>` field across folders (treated
/// as unset when empty), recording a conflict when two or more folders set the
/// field to different values.
fn merge_vec_field(
    merged: &mut Vec<String>,
    conflicts: &mut Vec<MultiRootConfigConflict>,
    key: &'static str,
    folders: &[(&str, &ProjectConfig)],
    extract: impl Fn(&ProjectConfig) -> &[String],
) {
    let mut seen: Vec<(String, String)> = Vec::new();
    for (name, cfg) in folders {
        let value = extract(cfg);
        if value.is_empty() {
            continue;
        }
        if merged.is_empty() {
            *merged = value.to_vec();
        }
        let value_str = value.join(",");
        if !seen.iter().any(|(_, v)| v == &value_str) {
            seen.push((name.to_string(), value_str));
        }
    }
    push_conflict(conflicts, key, seen);
}

/// Record a conflict only when more than one distinct value was seen for a key.
fn push_conflict(
    conflicts: &mut Vec<MultiRootConfigConflict>,
    key: &'static str,
    seen: Vec<(String, String)>,
) {
    if seen.len() > 1 {
        conflicts.push(MultiRootConfigConflict {
            key,
            folders: seen.iter().map(|(f, _)| f.clone()).collect(),
            values: seen.iter().map(|(_, v)| v.clone()).collect(),
        });
    }
}

/// Render an arbitrary merge value to a stable string for conflict comparison.
fn value_to_string<T: std::fmt::Debug>(value: &T) -> String {
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt as _;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn merge_project_configs_first_folder_wins_on_conflict() {
        // Two folders set [diagnostics].perlcritic to different values.
        let mut a = ProjectConfig::default();
        a.diagnostics.perlcritic = Some(true);
        let mut b = ProjectConfig::default();
        b.diagnostics.perlcritic = Some(false);

        let inputs: Vec<(&str, &ProjectConfig)> = vec![("folderA", &a), ("folderB", &b)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        // First folder wins for the conflicting key.
        assert_eq!(merged.diagnostics.perlcritic, Some(true));

        // A conflict is reported naming both folders and values.
        assert_eq!(conflicts.len(), 1);
        let conflict = &conflicts[0];
        assert_eq!(conflict.key, "diagnostics.perlcritic");
        assert_eq!(conflict.folders, vec!["folderA", "folderB"]);
        assert_eq!(conflict.values, vec!["true", "false"]);
        assert_eq!(conflict.render(), "diagnostics.perlcritic (folderA=true, folderB=false)");
    }

    #[test]
    fn merge_project_configs_non_conflicting_fields_all_apply() {
        // folderA sets perlcritic; folderB sets inlay_hints. No conflict.
        let mut a = ProjectConfig::default();
        a.diagnostics.perlcritic = Some(true);
        let mut b = ProjectConfig::default();
        b.features.inlay_hints = Some(false);

        let inputs: Vec<(&str, &ProjectConfig)> = vec![("folderA", &a), ("folderB", &b)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        assert_eq!(merged.diagnostics.perlcritic, Some(true));
        assert_eq!(merged.features.inlay_hints, Some(false));
        assert!(conflicts.is_empty(), "non-conflicting fields must not warn: {conflicts:?}");
    }

    #[test]
    fn merge_project_configs_same_value_is_not_a_conflict() {
        // Both folders set perlcritic=true: not a conflict, value applies once.
        let mut a = ProjectConfig::default();
        a.diagnostics.perlcritic = Some(true);
        let mut b = ProjectConfig::default();
        b.diagnostics.perlcritic = Some(true);

        let inputs: Vec<(&str, &ProjectConfig)> = vec![("folderA", &a), ("folderB", &b)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        assert_eq!(merged.diagnostics.perlcritic, Some(true));
        assert!(conflicts.is_empty(), "identical values must not warn: {conflicts:?}");
    }

    #[test]
    fn merge_project_configs_unset_fields_stay_none() {
        // No folder sets any global field.
        let a = ProjectConfig::default();
        let b = ProjectConfig::default();
        let inputs: Vec<(&str, &ProjectConfig)> = vec![("folderA", &a), ("folderB", &b)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        assert_eq!(merged.diagnostics.perlcritic, None);
        assert_eq!(merged.features.inlay_hints, None);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn merge_project_configs_excludes_perl_section() {
        // The [perl] section is per-folder; it must NOT participate in the merge.
        let mut a = ProjectConfig::default();
        a.perl.include_paths = vec!["a_lib".to_string()];
        let mut b = ProjectConfig::default();
        b.perl.include_paths = vec!["b_lib".to_string()];

        let inputs: Vec<(&str, &ProjectConfig)> = vec![("folderA", &a), ("folderB", &b)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        assert!(merged.perl.include_paths.is_empty(), "[perl] must not be merged");
        assert!(conflicts.is_empty(), "[perl] differences must not be conflicts");
    }

    #[test]
    fn merge_project_configs_three_folders_first_wins() {
        let mut a = ProjectConfig::default();
        a.diagnostics.perlcritic_severity = Some(3);
        let mut b = ProjectConfig::default();
        b.diagnostics.perlcritic_severity = Some(1);
        let mut c = ProjectConfig::default();
        c.diagnostics.perlcritic_severity = Some(5);

        let inputs: Vec<(&str, &ProjectConfig)> =
            vec![("folderA", &a), ("folderB", &b), ("folderC", &c)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        assert_eq!(merged.diagnostics.perlcritic_severity, Some(3));
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].key, "diagnostics.perlcritic_severity");
        assert_eq!(conflicts[0].folders, vec!["folderA", "folderB", "folderC"]);
    }

    #[test]
    fn merge_project_configs_ai_conflict_lists_all_folders_per_value() {
        let mut a = ProjectConfig::default();
        a.ai_completion.enabled = Some(false);
        let mut b = ProjectConfig::default();
        b.ai_completion.enabled = Some(false);
        let mut c = ProjectConfig::default();
        c.ai_completion.enabled = Some(true);

        let inputs: Vec<(&str, &ProjectConfig)> =
            vec![("folderA", &a), ("folderB", &b), ("folderC", &c)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        assert_eq!(merged.ai_completion.enabled, Some(false));
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].key, "ai_completion.enabled");
        assert_eq!(conflicts[0].folders, vec!["folderA", "folderB", "folderC"]);
        assert_eq!(conflicts[0].values, vec!["false", "false", "true"]);
    }

    #[test]
    fn merge_project_configs_vec_field_conflict() {
        let mut a = ProjectConfig::default();
        a.critic.include = Some(vec!["ProhibitGrep".to_string()]);
        let mut b = ProjectConfig::default();
        b.critic.include = Some(vec!["ProhibitMap".to_string()]);

        let inputs: Vec<(&str, &ProjectConfig)> = vec![("folderA", &a), ("folderB", &b)];
        let (merged, conflicts) = merge_project_configs_for_server(&inputs);

        assert_eq!(
            merged.critic.include.as_ref().map(Vec::as_slice),
            Some(&["ProhibitGrep".to_string()][..])
        );
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].key, "critic.include");
    }

    #[test]
    fn load_project_config_returns_none_when_missing() -> TestResult {
        let temp = tempfile::tempdir()?;
        let config = load_project_config(temp.path())?;
        assert!(config.is_none());
        Ok(())
    }

    #[test]
    fn load_project_config_finds_toml_in_parent_directory() -> TestResult {
        // Simulates a monorepo where the workspace folder is a subdirectory
        // but .perl-lsp.toml lives at the parent (monorepo root).
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join(".perl-lsp.toml"),
            r#"
[diagnostics]
perlcritic = true
"#,
        )?;
        let subdir = root.path().join("services").join("web");
        std::fs::create_dir_all(&subdir)?;

        let config = load_project_config(&subdir)?;
        let parsed = config.ok_or("expected config from parent .perl-lsp.toml")?;
        assert_eq!(parsed.diagnostics.perlcritic, Some(true));
        Ok(())
    }

    #[test]
    fn load_project_config_prefers_nearest_toml_in_parent_walk() -> TestResult {
        // When .perl-lsp.toml exists in both a parent and a nearer directory,
        // the nearer one wins (first match in the upward walk).
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join(".perl-lsp.toml"),
            r#"
[diagnostics]
perlcritic_severity = 5
"#,
        )?;
        let mid = root.path().join("services");
        std::fs::create_dir_all(&mid)?;
        std::fs::write(
            mid.join(".perl-lsp.toml"),
            r#"
[diagnostics]
perlcritic_severity = 2
"#,
        )?;
        let leaf = mid.join("web");
        std::fs::create_dir_all(&leaf)?;

        let config = load_project_config(&leaf)?;
        let parsed = config.ok_or("expected config from nearest .perl-lsp.toml")?;
        assert_eq!(parsed.diagnostics.perlcritic_severity, Some(2));
        Ok(())
    }

    #[test]
    fn load_project_config_returns_none_when_no_ancestor_has_toml() -> TestResult {
        let root = tempfile::tempdir()?;
        let subdir = root.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&subdir)?;

        let config = load_project_config(&subdir)?;
        assert!(config.is_none());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_finds_workspace_root_profile() -> TestResult {
        let temp = tempfile::tempdir()?;
        let profile = temp.path().join(".perltidyrc");
        std::fs::write(&profile, "-l=100\n")?;

        let discovered = discover_perltidy_profile_from(temp.path(), None, None);

        assert_eq!(discovered.as_deref(), profile.to_str());
        Ok(())
    }

    #[test]
    fn apply_perltidy_native_options_overrides_only_specified_fields() {
        let mut config = ServerConfig::default();
        // A built-in default the profile must be able to override.
        assert_eq!(config.perltidy_maximum_line_length, Some(80));
        // Indentation ships unset so unconfigured workspaces keep deferring to
        // the editor's tabSize.
        assert_eq!(config.perltidy_indent_columns, None);

        // Profile sets only the line width.
        let options =
            crate::tooling::native_compat::classify_perltidy_profile("-l=120\n").suggested_config;
        config.apply_perltidy_native_options(&options);

        assert_eq!(
            config.perltidy_maximum_line_length,
            Some(120),
            "the profile's line width must override the built-in default"
        );
        assert_eq!(
            config.perltidy_indent_columns, None,
            "fields the profile does not set must be left unchanged"
        );
    }

    #[test]
    fn perltidy_profile_indent_columns_reach_the_server_config() {
        // The discovered-profile layer is one of the sources that turns
        // indentation from "unset" into an explicit value the formatter must
        // honour over the editor's tabSize.
        let mut config = ServerConfig::default();
        let options =
            crate::tooling::native_compat::classify_perltidy_profile("-i=2\n-t\n").suggested_config;
        config.apply_perltidy_native_options(&options);

        assert_eq!(config.perltidy_indent_columns, Some(2));
        assert_eq!(config.perltidy_tabs, Some(true));
    }

    #[test]
    fn discover_perltidy_profile_accepts_unprefixed_workspace_profile() -> TestResult {
        let temp = tempfile::tempdir()?;
        let profile = temp.path().join("perltidyrc");
        std::fs::write(&profile, "-l=100\n")?;

        let discovered = discover_perltidy_profile_from(temp.path(), None, None);

        assert_eq!(discovered.as_deref(), profile.to_str());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_prefers_workspace_over_home_and_env() -> TestResult {
        let workspace = tempfile::tempdir()?;
        let home = tempfile::tempdir()?;
        let env_dir = tempfile::tempdir()?;
        let workspace_profile = workspace.path().join(".perltidyrc");
        std::fs::write(&workspace_profile, "-l=100\n")?;
        std::fs::write(home.path().join(".perltidyrc"), "-l=80\n")?;
        let env_profile = env_dir.path().join("custom.perltidyrc");
        std::fs::write(&env_profile, "-l=72\n")?;

        let discovered = discover_perltidy_profile_from(
            workspace.path(),
            Some(env_profile),
            Some(home.path().to_path_buf()),
        );

        assert_eq!(discovered.as_deref(), workspace_profile.to_str());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_prefers_env_over_home() -> TestResult {
        // Per perltidy's documented convention, the `PERLTIDY` environment
        // override is searched before `$HOME/.perltidyrc`.
        let workspace = tempfile::tempdir()?;
        let home = tempfile::tempdir()?;
        let env_dir = tempfile::tempdir()?;
        std::fs::write(home.path().join(".perltidyrc"), "-l=80\n")?;
        let env_profile = env_dir.path().join("custom.perltidyrc");
        std::fs::write(&env_profile, "-l=72\n")?;

        let discovered = discover_perltidy_profile_from(
            workspace.path(),
            Some(env_profile.clone()),
            Some(home.path().to_path_buf()),
        );

        assert_eq!(discovered.as_deref(), env_profile.to_str());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_falls_back_to_home() -> TestResult {
        let workspace = tempfile::tempdir()?;
        let home = tempfile::tempdir()?;
        let home_profile = home.path().join(".perltidyrc");
        std::fs::write(&home_profile, "-l=80\n")?;

        let discovered =
            discover_perltidy_profile_from(workspace.path(), None, Some(home.path().to_path_buf()));

        assert_eq!(discovered.as_deref(), home_profile.to_str());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_falls_back_to_env_var() -> TestResult {
        let workspace = tempfile::tempdir()?;
        let env_dir = tempfile::tempdir()?;
        let env_profile = env_dir.path().join("custom.perltidyrc");
        std::fs::write(&env_profile, "-l=72\n")?;

        let discovered =
            discover_perltidy_profile_from(workspace.path(), Some(env_profile.clone()), None);

        assert_eq!(discovered.as_deref(), env_profile.to_str());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_returns_none_when_absent() -> TestResult {
        let workspace = tempfile::tempdir()?;
        let home = tempfile::tempdir()?;

        let discovered = discover_perltidy_profile_from(
            workspace.path(),
            Some(home.path().join("missing.perltidyrc")),
            Some(home.path().to_path_buf()),
        );

        assert!(discovered.is_none());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_ignores_directory_named_profile() -> TestResult {
        let workspace = tempfile::tempdir()?;
        // A directory named `.perltidyrc` must not be treated as a profile file.
        std::fs::create_dir(workspace.path().join(".perltidyrc"))?;

        let discovered = discover_perltidy_profile_from(workspace.path(), None, None);

        assert!(discovered.is_none());
        Ok(())
    }

    #[test]
    fn discover_perltidy_profile_ignores_env_var_pointing_to_directory() -> TestResult {
        let workspace = tempfile::tempdir()?;
        // $PERLTIDY is sometimes mis-configured to point to a directory rather
        // than a file (e.g. `PERLTIDY=/home/user/` instead of
        // `PERLTIDY=/home/user/.perltidyrc`). The env-var candidate must not be
        // treated as a profile when it resolves to a directory.
        let env_dir = tempfile::tempdir()?;

        let discovered = discover_perltidy_profile_from(
            workspace.path(),
            Some(env_dir.path().to_path_buf()),
            None,
        );

        assert!(
            discovered.is_none(),
            "a directory passed via PERLTIDY must not be returned as a profile"
        );
        Ok(())
    }

    #[test]
    fn load_project_config_returns_parse_error_for_invalid_toml() -> TestResult {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join(".perl-lsp.toml"), "[perl\ninclude_paths = [\"lib\"]")?;

        let err = load_project_config(temp.path())
            .err()
            .ok_or("expected invalid TOML to return an error")?;
        assert!(err.contains("syntax error"));
        Ok(())
    }

    #[test]
    fn load_project_config_parses_known_sections() -> TestResult {
        let temp = tempfile::tempdir()?;
        std::fs::write(
            temp.path().join(".perl-lsp.toml"),
            r#"
[perl]
include_paths = ["lib", "t/lib"]
use_perl5lib = true
perl5lib_precedence = "prepend"

[diagnostics]
perlcritic = true
perlcritic_severity = 4

[features]
inlay_hints = false

[formatting]
enabled = true
engine = "external-perltidy"
perltidy_maximum_line_length = 100
perltidy_opening_brace_on_new_line = true
perltidy_cuddled_else = false
perltidy_space_after_keyword = false
perltidy_add_trailing_commas = true
perltidy_extra_args = ["-noll"]

[critic]
engine = "native"
profile = "recommended"
"#,
        )?;

        let config = load_project_config(temp.path())?.ok_or("expected parsed project config")?;

        assert_eq!(config.perl.include_paths, vec!["lib", "t/lib"]);
        assert_eq!(config.perl.use_perl5lib, Some(true));
        assert!(matches!(config.perl.perl5lib_precedence, Some(Perl5LibPrecedence::Prepend)));
        assert_eq!(config.diagnostics.perlcritic, Some(true));
        assert_eq!(config.diagnostics.perlcritic_severity, Some(4));
        assert_eq!(config.features.inlay_hints, Some(false));
        assert_eq!(config.formatting.enabled, Some(true));
        assert_eq!(config.formatting.engine.as_deref(), Some("external-perltidy"));
        assert_eq!(config.formatting.perltidy_maximum_line_length, Some(100));
        assert_eq!(config.formatting.perltidy_opening_brace_on_new_line, Some(true));
        assert_eq!(config.formatting.perltidy_cuddled_else, Some(false));
        assert_eq!(config.formatting.perltidy_space_after_keyword, Some(false));
        assert_eq!(config.formatting.perltidy_add_trailing_commas, Some(true));
        assert_eq!(config.formatting.perltidy_extra_args, vec!["-noll"]);
        assert_eq!(config.critic.engine.as_deref(), Some("native"));
        assert_eq!(config.critic.profile.as_deref(), Some("recommended"));
        Ok(())
    }

    #[test]
    fn load_project_config_rejects_non_regular_file() -> TestResult {
        let temp = tempfile::tempdir()?;
        std::fs::create_dir(temp.path().join(".perl-lsp.toml"))?;

        let err = load_project_config(temp.path())
            .err()
            .ok_or("expected non-regular config path to return an error")?;
        assert!(err.contains("regular file"));
        Ok(())
    }

    #[test]
    fn load_project_config_rejects_oversized_file() -> TestResult {
        let temp = tempfile::tempdir()?;
        let oversized = vec![b'a'; (1024 * 1024) + 1];
        std::fs::write(temp.path().join(".perl-lsp.toml"), oversized)?;

        let err = load_project_config(temp.path())
            .err()
            .ok_or("expected oversized config file to return an error")?;
        assert!(err.contains("too large"));
        Ok(())
    }

    /// A 1 MiB file (exactly at the cap) must be accepted without error.
    #[test]
    fn load_project_config_accepts_file_at_size_limit() -> TestResult {
        let temp = tempfile::tempdir()?;
        // Write a 1 MiB file containing only '#' comment chars — valid TOML, empty config.
        let exactly_at_limit = vec![b'#'; 1024 * 1024];
        std::fs::write(temp.path().join(".perl-lsp.toml"), exactly_at_limit)?;
        // Should parse successfully and yield a default ProjectConfig (no sections set).
        let config = load_project_config(temp.path())?;
        assert!(config.is_some(), "1 MiB file at the limit must be accepted");
        Ok(())
    }

    /// An error from File::open when the file is not found (TOCTOU: file removed after metadata
    /// check) must be treated as absent, not as a hard error.
    #[test]
    fn load_project_config_returns_none_for_vanished_file() -> TestResult {
        // We can't easily reproduce a true TOCTOU race, but we can verify the code path by
        // directly exercising the condition: call load_project_config on a path where no file
        // exists (metadata returns NotFound at the first check, so this also confirms the
        // original not-found path still works under the restructured code).
        let temp = tempfile::tempdir()?;
        let result = load_project_config(temp.path())?;
        assert!(result.is_none(), "missing file must yield None, not Err");
        Ok(())
    }

    #[test]
    fn apply_to_server_config_clamps_perlcritic_severity() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.diagnostics.perlcritic_severity = Some(99);

        project.apply_to_server_config(&mut config);

        assert_eq!(config.perlcritic_severity, 5);
    }

    #[test]
    fn server_config_update_from_value_ignores_removed_test_runner_authority() -> TestResult {
        let mut config = ServerConfig::default();

        config.update_from_value(&serde_json::json!({
            "inlayHints": {
                "enabled": false,
                "parameterHints": false,
                "typeHints": false,
                "chainedHints": true,
                "maxLength": 12
            },
            "testRunner": {
                "enabled": false,
                "command": "CANARY-EXECUTABLE",
                "args": ["CANARY-ARG"],
                "cwd": "CANARY-CWD",
                "env": {"CANARY": "CANARY-VALUE"},
                "timeout": 12_345
            },
            "telemetry": {
                "enabled": true
            },
            "perlcritic": {
                "enabled": false,
                "severity": 99,
                "profile": "  .perlcriticrc  ",
                "theme": "  core && !pbp  "
            }
        }));

        assert!(!config.inlay_hints_enabled);
        assert!(!config.inlay_hints_parameter_hints);
        assert!(!config.inlay_hints_type_hints);
        assert!(config.inlay_hints_chained_hints);
        assert_eq!(config.inlay_hints_max_length, 12);
        assert!(config.telemetry_enabled);
        assert!(!config.perlcritic_enabled);
        assert_eq!(config.perlcritic_severity, 5);
        assert!(config.perlcritic_profile.is_none());
        assert!(config.perlcritic_theme.is_none());

        config.update_from_value(&serde_json::json!({
            "perlcritic": {
                "severity": 0,
                "profile": "   ",
                "theme": ""
            }
        }));

        assert_eq!(config.perlcritic_severity, 1);
        assert!(config.perlcritic_profile.is_none());
        assert!(config.perlcritic_theme.is_none());
        Ok(())
    }

    #[test]
    fn server_config_update_from_value_applies_next_edit_gate() -> TestResult {
        let mut config = ServerConfig::default();
        assert!(!config.next_edit.enabled);

        config.update_from_value(&serde_json::json!({
            "nextEdit": {
                "enabled": true
            }
        }));

        assert!(config.next_edit.enabled);

        config.update_from_value(&serde_json::json!({
            "nextEdit": {
                "enabled": false
            }
        }));

        assert!(!config.next_edit.enabled);
        Ok(())
    }

    #[test]
    fn server_config_update_from_value_applies_formatting_and_ai_settings() -> TestResult {
        let mut config = ServerConfig::default();

        config.update_from_value(&serde_json::json!({
            "formatting": {
                "enabled": false,
                "engine": "perltidy_compat",
                "profile": "  .perltidyrc  ",
                "maximumLineLength": 120,
                "indentColumns": 2,
                "tabs": true,
                "openingBraceOnNewLine": true,
                "cuddledElse": false,
                "spaceAfterKeyword": false,
                "addTrailingCommas": true,
                "verticalAlignment": false,
                "blockCommentIndentation": 1,
                "extraArgs": ["-noll", false, "-bar"],
                "timeoutSecs": 7
            },
            "aiCompletion": {
                "enabled": true,
                "provider": "local",
                "endpoint": "http://127.0.0.1:11434/v1",
                "model": "codellama",
                "apiKeyEnv": "LOCAL_AI_KEY",
                "apiKeyHeader": "x-api-key",
                "apiKeyPrefix": "",
                "timeoutMs": 2500,
                "maxOutputTokens": 128,
                "rateLimitRps": 2.5,
                "maxInflight": 3,
                "fallback": false,
                "streaming": {
                    "enabled": false,
                    "updateDebounceMs": 125
                }
            }
        }));

        assert!(!config.perltidy_enabled);
        assert_eq!(config.formatting_engine, FormatterMode::Compat);
        assert!(config.perltidy_profile.is_none());
        assert_eq!(config.perltidy_maximum_line_length, Some(120));
        assert_eq!(config.perltidy_indent_columns, Some(2));
        assert_eq!(config.perltidy_tabs, Some(true));
        assert_eq!(config.perltidy_opening_brace_on_new_line, Some(true));
        assert_eq!(config.perltidy_cuddled_else, Some(false));
        assert_eq!(config.perltidy_space_after_keyword, Some(false));
        assert_eq!(config.perltidy_add_trailing_commas, Some(true));
        assert_eq!(config.perltidy_vertical_alignment, Some(false));
        assert_eq!(config.perltidy_block_comment_indentation, Some(1));
        assert!(config.perltidy_extra_args.is_empty());
        assert_eq!(config.perltidy_timeout_secs, 7);
        assert!(config.ai_completion.enabled);
        assert!(config.ai_completion.user_enabled);
        assert_eq!(config.ai_completion.provider, "local");
        // #5684: endpoint, apiKeyEnv, apiKeyHeader, apiKeyPrefix are NOT
        // settable via didChangeConfiguration. They remain at defaults.
        assert_eq!(config.ai_completion.endpoint, "");
        assert_eq!(config.ai_completion.model, "codellama");
        assert_eq!(config.ai_completion.api_key_env, "OPENAI_API_KEY");
        assert_eq!(config.ai_completion.api_key_header, "Authorization");
        assert_eq!(config.ai_completion.api_key_prefix, Some("Bearer".to_string()));
        assert_eq!(config.ai_completion.timeout_ms, 2500);
        assert_eq!(config.ai_completion.max_output_tokens, 128);
        assert_eq!(config.ai_completion.rate_limit_rps, 2.5);
        assert_eq!(config.ai_completion.max_inflight, 3);
        assert!(!config.ai_completion.fallback);
        assert!(!config.ai_completion.streaming.enabled);
        assert_eq!(config.ai_completion.streaming.update_debounce_ms, 125);

        config.update_from_value(&serde_json::json!({
            "formatting": {
                "profile": ""
            }
        }));
        assert!(config.perltidy_profile.is_none());
        Ok(())
    }

    #[test]
    fn server_config_rejects_external_formatter_engine_from_client_settings() {
        let mut config = ServerConfig::default();

        config.update_from_value(&serde_json::json!({
            "formatting": {
                "engine": "external-perltidy"
            }
        }));
        assert_eq!(config.formatting_engine, FormatterMode::Native);

        config.update_from_value(&serde_json::json!({
            "formatting": {
                "engine": "native"
            }
        }));
        assert_eq!(config.formatting_engine, FormatterMode::Native);
    }

    // Native-first formatter guards. The formatter engine must default to
    // native and only ever select external `perltidy` when explicitly
    // configured — merely having `perltidy` on PATH must not change the
    // default. These tests lock that contract against future regressions
    // ("auto-use perltidy if present").

    #[test]
    fn default_formatter_engine_is_native() {
        let config = ServerConfig::default();
        assert_eq!(config.formatting_engine, FormatterMode::Native);
        assert!(config.perltidy_enabled, "formatting is enabled by default via the native engine");
    }

    #[test]
    fn external_perltidy_is_selected_only_by_explicit_engine() {
        // `parse_formatter_mode` is a pure mapping with no environment/PATH
        // probe: the external engine is reachable only through explicit config.
        assert_eq!(parse_formatter_mode("external-perltidy"), Some(FormatterMode::ExternalLegacy));
        assert_eq!(parse_formatter_mode("external-legacy"), Some(FormatterMode::ExternalLegacy));
        assert_eq!(parse_formatter_mode("perltidy"), Some(FormatterMode::ExternalLegacy));
        assert_eq!(parse_formatter_mode("native"), Some(FormatterMode::Native));
        // Unknown values do not silently select external; the caller keeps its
        // current value (native by default).
        assert_eq!(parse_formatter_mode("definitely-not-an-engine"), None);
        assert_eq!(parse_formatter_mode(""), None);
    }

    #[test]
    fn perltidy_on_path_does_not_change_default_formatter_engine() {
        // The default engine is a fixed value, not derived from whether
        // `perltidy` exists on PATH. Applying config that does not name an
        // engine leaves the native default intact.
        let mut config = ServerConfig::default();
        assert_eq!(config.formatting_engine, FormatterMode::Native);
        config.update_from_value(&serde_json::json!({
            "formatting": { "enabled": true }
        }));
        assert_eq!(config.formatting_engine, FormatterMode::Native);
    }

    #[test]
    #[allow(unsafe_code)] // transient PATH mutation, serialized + restored (see below)
    fn perltidy_discoverable_on_path_still_yields_native_default()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        // Stronger form of the guard above. The previous test only proves the
        // default holds when `perltidy` is ABSENT from PATH (the usual CI
        // condition), so it would not catch a regression that auto-selects the
        // external engine only when a PATH probe (`which perltidy`) succeeds.
        // Here we make a real `perltidy` executable discoverable on PATH and
        // assert the default engine is STILL native — locking the "installed
        // external tools must not change default behavior merely by existing on
        // PATH" contract behaviorally, not just structurally.
        //
        // PATH is process-global, so serialize against any other PATH-touching
        // test and restore it before asserting (a leaked mutation would poison
        // sibling tests). The lock is crate-shared (`crate::test_support`), not
        // function-local, so every PATH-mutating test acquires the SAME guard.
        use std::io::Write as _;
        let _lock = crate::test_support::PATH_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let dir = tempfile::tempdir()?;
        let bin_name = if cfg!(windows) { "perltidy.exe" } else { "perltidy" };
        let bin_path = dir.path().join(bin_name);
        std::fs::File::create(&bin_path)?.write_all(b"#!/bin/sh\nexit 0\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = std::fs::metadata(&bin_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perms)?;
        }

        let original_path = std::env::var_os("PATH");
        let probe_path = {
            let mut parts = vec![dir.path().to_path_buf()];
            if let Some(existing) = &original_path {
                parts.extend(std::env::split_paths(existing));
            }
            std::env::join_paths(parts)?
        };
        // SAFETY: serialized by PATH_ENV_LOCK; PATH is restored below before any
        // assertion can unwind the thread. Mirrors the crate's existing
        // `EnvVarGuard` pattern (runtime/launcher/mod.rs).
        unsafe { std::env::set_var("PATH", &probe_path) };

        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "formatting": { "enabled": true }
        }));
        let engine_with_perltidy_on_path = config.formatting_engine;

        // Restore PATH before asserting so a failing assert cannot leak the
        // mutated PATH into sibling tests. SAFETY: still under PATH_ENV_LOCK.
        match original_path {
            Some(prev) => unsafe { std::env::set_var("PATH", prev) },
            None => unsafe { std::env::remove_var("PATH") },
        }

        assert_eq!(
            engine_with_perltidy_on_path,
            FormatterMode::Native,
            "a `perltidy` discoverable on PATH must not flip the default formatter engine"
        );
        Ok(())
    }

    #[test]
    fn perltidyrc_profile_does_not_force_external_formatting() {
        // A `.perltidyrc` profile is usable for compatibility reporting or an
        // explicit external mode, but setting it via the LSP settings channel
        // must NOT arm subprocess profile paths (issue #5001).
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "formatting": { "profile": "/path/to/.perltidyrc" }
        }));
        assert!(config.perltidy_profile.is_none());
        assert_eq!(config.formatting_engine, FormatterMode::Native);
    }

    #[test]
    fn server_config_accepts_native_critic_engine() {
        let mut config = ServerConfig::default();
        assert_eq!(config.critic_engine, CriticEngine::Native);
        assert_eq!(config.native_critic_profile, "recommended");

        config.update_from_value(&serde_json::json!({
            "critic": {
                "engine": "native",
                "profile": "recommended"
            }
        }));
        assert_eq!(config.critic_engine, CriticEngine::Native);
        assert_eq!(config.native_critic_profile, "recommended");

        config.update_from_value(&serde_json::json!({
            "critic": {
                "engine": "perlcritic",
                "profile": "strict"
            }
        }));
        assert_eq!(config.critic_engine, CriticEngine::Native);
        assert_eq!(config.native_critic_profile, "strict");

        config.update_from_value(&serde_json::json!({
            "critic": {
                "profile": "unknown"
            }
        }));
        assert_eq!(config.native_critic_profile, "strict");
    }

    #[test]
    fn native_critic_config_boundary_agrees_with_profile_authority() {
        for raw in ["recommended", " RECOMMENDED ", "strict", " STRICT "] {
            let expected = NativeCriticProfile::parse(raw)
                .expect("boundary fixture must be accepted by the profile authority");
            let mut config = ServerConfig::default();
            config.update_from_value(&serde_json::json!({
                "critic": { "profile": raw }
            }));

            assert_eq!(config.native_critic_profile, expected.as_str(), "profile token: {raw:?}");
        }

        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "critic": { "profile": "recomended" }
        }));
        assert_eq!(config.native_critic_profile, NativeCriticProfile::default().as_str());
    }

    #[test]
    fn server_config_accepts_native_critic_include_and_exclude_filters() {
        let mut config = ServerConfig::default();

        config.update_from_value(&serde_json::json!({
            "critic": {
                "include": [
                    "native.testing.require_use_strict",
                    "",
                    "native.testing.require_use_warnings"
                ],
                "exclude": [
                    "native.common.assignment_in_condition"
                ]
            }
        }));

        assert_eq!(
            config.native_critic_include,
            vec![
                "native.testing.require_use_strict".to_string(),
                "native.testing.require_use_warnings".to_string()
            ]
        );
        assert_eq!(
            config.native_critic_exclude,
            vec!["native.common.assignment_in_condition".to_string()]
        );

        config.update_from_value(&serde_json::json!({
            "critic": {
                "include": []
            }
        }));
        assert!(config.native_critic_include.is_empty());
        assert_eq!(
            config.native_critic_exclude,
            vec!["native.common.assignment_in_condition".to_string()]
        );
    }

    #[test]
    fn server_config_native_critic_enabled_and_severity() {
        let mut config = ServerConfig::default();

        config.update_from_value(&serde_json::json!({
            "critic": {
                "enabled": false,
                "severity": 5
            }
        }));
        assert!(!config.perlcritic_enabled);
        assert_eq!(config.perlcritic_severity, 5);

        // Out-of-range severities clamp into 1..=5.
        config.update_from_value(&serde_json::json!({
            "critic": { "severity": 9 }
        }));
        assert_eq!(config.perlcritic_severity, 5);
        config.update_from_value(&serde_json::json!({
            "critic": { "severity": 0 }
        }));
        assert_eq!(config.perlcritic_severity, 1);
    }

    #[test]
    fn native_critic_settings_win_over_legacy_perlcritic() {
        let mut config = ServerConfig::default();

        // When both `perlcritic.*` and `critic.*` are present in the same
        // payload, the native `critic.*` block is parsed second and wins.
        config.update_from_value(&serde_json::json!({
            "perlcritic": {
                "enabled": true,
                "severity": 2,
                "profile": "legacy-profile"
            },
            "critic": {
                "enabled": false,
                "severity": 4,
                "profile": "strict"
            }
        }));

        assert!(!config.perlcritic_enabled);
        assert_eq!(config.perlcritic_severity, 4);
        assert_eq!(config.native_critic_profile, "strict");
    }

    #[test]
    fn project_config_applies_formatter_engine() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.formatting.engine = Some("off".to_string());

        project.apply_to_server_config(&mut config);

        assert_eq!(config.formatting_engine, FormatterMode::Off);
    }

    #[test]
    fn project_config_applies_native_formatting_policy_fields() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.formatting.engine = Some("native".to_string());
        project.formatting.perltidy_opening_brace_on_new_line = Some(true);
        project.formatting.perltidy_cuddled_else = Some(false);
        project.formatting.perltidy_space_after_keyword = Some(false);
        project.formatting.perltidy_add_trailing_commas = Some(true);

        project.apply_to_server_config(&mut config);

        assert_eq!(config.formatting_engine, FormatterMode::Native);
        assert_eq!(config.perltidy_opening_brace_on_new_line, Some(true));
        assert_eq!(config.perltidy_cuddled_else, Some(false));
        assert_eq!(config.perltidy_space_after_keyword, Some(false));
        assert_eq!(config.perltidy_add_trailing_commas, Some(true));
    }

    #[test]
    fn project_config_applies_native_critic_engine() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.critic.engine = Some("native".to_string());
        project.critic.profile = Some("recommended".to_string());
        project.critic.include = Some(vec![
            "native.testing.require_use_strict".to_string(),
            " ".to_string(),
            "native.testing.require_use_strict".to_string(),
        ]);
        project.critic.exclude = Some(vec!["native.common.assignment_in_condition".to_string()]);

        project.apply_to_server_config(&mut config);

        assert_eq!(config.critic_engine, CriticEngine::Native);
        assert_eq!(config.native_critic_profile, "recommended");
        assert_eq!(
            config.native_critic_include,
            vec!["native.testing.require_use_strict".to_string()]
        );
        assert_eq!(
            config.native_critic_exclude,
            vec!["native.common.assignment_in_condition".to_string()]
        );
    }

    // --- tracing::warn! capture helper for unrecognized-config tests (#4622) ---
    // A minimal `tracing_subscriber` layer that records the `message` field of
    // every WARN (and higher) event into a shared buffer. Installed scoped via
    // `tracing::dispatcher::with_default` so parallel tests do not clobber the
    // global subscriber.

    /// `tracing::field::Visit` implementation that collects the value of every
    /// field on a single event (the `message` literal as well as structured
    /// fields like `setting`, `value`, `valid`) into a shared buffer.
    struct MessageVisitor(Arc<Mutex<Vec<String>>>);
    impl tracing::field::Visit for MessageVisitor {
        fn record_debug(&mut self, _field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.lock().unwrap_or_else(|p| p.into_inner()).push(format!("{:?}", value));
        }
        fn record_str(&mut self, _field: &tracing::field::Field, value: &str) {
            self.0.lock().unwrap_or_else(|p| p.into_inner()).push(value.to_string());
        }
    }

    /// `tracing_subscriber` layer that records WARN+ events' message text.
    struct CapturingLayer {
        messages: Arc<Mutex<Vec<String>>>,
    }
    impl<S> tracing_subscriber::layer::Layer<S> for CapturingLayer
    where
        S: tracing::Subscriber,
    {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            // Only capture WARN and above (the config code emits at warn!).
            if event.metadata().level() <= &tracing::Level::WARN {
                let mut visitor = MessageVisitor(Arc::clone(&self.messages));
                event.record(&mut visitor);
            }
        }
    }

    /// Run `body` under a scoped tracing subscriber that captures WARN+ messages,
    /// and return the captured message strings.
    fn capture_warnings(body: impl FnOnce()) -> Vec<String> {
        let messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let dispatch = tracing_subscriber::registry()
            .with(CapturingLayer { messages: Arc::clone(&messages) })
            .into();
        tracing::dispatcher::with_default(&dispatch, body);
        drop(dispatch);
        // Lock-and-clone instead of Arc::try_unwrap to avoid the race where
        // the subscriber's Arc clone is still alive (thread-local retention).
        messages.lock().map(|m| m.clone()).unwrap_or_default()
    }

    /// Assert that at least one captured warning mentions every `needle`.
    fn assert_warned_contains(captured: &[String], needles: &[&str]) {
        let combined: String = captured.join("\n");
        for needle in needles {
            assert!(
                combined.contains(needle),
                "expected a warning containing {:?}; captured warnings:\n{}",
                needle,
                combined,
            );
        }
    }

    #[test]
    fn json_invalid_critic_engine_warns_and_keeps_default() {
        let mut config = ServerConfig::default();
        let prior = config.critic_engine;
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "engine": "nativ" }
            }));
        });
        // Behaviour: typo is ignored, default/current setting kept.
        assert_eq!(config.critic_engine, prior);
        // Signal: a warning was emitted naming the setting and value.
        assert_warned_contains(&captured, &["critic.engine", "nativ"]);
    }

    #[test]
    fn json_invalid_critic_profile_warns_and_keeps_current() {
        let mut config =
            ServerConfig { native_critic_profile: "strict".to_string(), ..ServerConfig::default() };
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "profile": "recomended" }
            }));
        });
        assert_eq!(config.native_critic_profile, "strict");
        assert_warned_contains(&captured, &["critic.profile", "recomended"]);
    }

    #[test]
    fn json_invalid_formatting_engine_warns_and_keeps_default() {
        let mut config = ServerConfig::default();
        let prior = config.formatting_engine;
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "formatting": { "engine": "perltide" }
            }));
        });
        assert_eq!(config.formatting_engine, prior);
        assert_warned_contains(&captured, &["formatting.engine", "perltide"]);
    }

    #[test]
    fn client_invalid_enum_inspection_covers_all_user_visible_engine_settings() {
        let invalid = ServerConfig::invalid_client_setting_values(&serde_json::json!({
            "critic": {
                "engine": "nativ",
                "profile": "recomended"
            },
            "formatting": { "engine": "perltide" }
        }));

        assert_eq!(
            invalid,
            vec![
                InvalidClientSetting {
                    setting: "critic.engine",
                    value: "nativ".to_string(),
                    value_type: "string",
                    valid_options: CLIENT_CRITIC_ENGINE_VALID_OPTIONS,
                },
                InvalidClientSetting {
                    setting: "critic.profile",
                    value: "recomended".to_string(),
                    value_type: "string",
                    valid_options: NativeCriticProfile::VALID_OPTIONS,
                },
                InvalidClientSetting {
                    setting: "formatting.engine",
                    value: "perltide".to_string(),
                    value_type: "string",
                    valid_options: CLIENT_FORMATTER_MODE_VALID_OPTIONS,
                },
            ]
        );

        let legacy = ServerConfig::invalid_client_setting_values(&serde_json::json!({
            "critic": { "engine": " LEGACY ", "profile": "STRICT" },
            "formatting": { "engine": "external_perltidy" }
        }));
        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].setting, "critic.engine");
        assert_eq!(legacy[0].valid_options, CLIENT_CRITIC_ENGINE_VALID_OPTIONS);
        assert_eq!(legacy[1].setting, "formatting.engine");
        assert_eq!(legacy[1].valid_options, CLIENT_FORMATTER_MODE_VALID_OPTIONS);
    }

    #[test]
    fn client_formatter_external_aliases_are_rejected_but_project_alias_remains_supported() {
        for value in ["external-legacy", "external_perltidy", "perltidy"] {
            let mut config = ServerConfig::default();
            config.update_from_value(&serde_json::json!({
                "formatting": { "engine": value }
            }));
            assert_eq!(config.formatting_engine, FormatterMode::Native, "client value {value}");
            assert_eq!(
                ServerConfig::invalid_client_setting_values(&serde_json::json!({
                    "formatting": { "engine": value }
                }))[0]
                    .valid_options,
                CLIENT_FORMATTER_MODE_VALID_OPTIONS
            );
        }

        let mut config = ServerConfig::default();
        let project = ProjectConfig {
            formatting: ProjectFormattingConfig {
                engine: Some("external-legacy".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        project.apply_to_server_config(&mut config);
        assert_eq!(config.formatting_engine, FormatterMode::ExternalLegacy);
    }

    #[test]
    fn client_invalid_enum_inspection_reports_wrong_value_types() {
        let invalid = ServerConfig::invalid_client_setting_values(&serde_json::json!({
            "critic": {
                "engine": false,
                "profile": ["recommended"]
            },
            "formatting": { "engine": null }
        }));

        assert_eq!(invalid.len(), 3);
        assert_eq!(invalid[0].setting, "critic.engine");
        assert_eq!(invalid[0].value, "false");
        assert_eq!(invalid[1].setting, "critic.profile");
        assert_eq!(invalid[1].value, "[\"recommended\"]");
        assert_eq!(invalid[2].setting, "formatting.engine");
        assert_eq!(invalid[2].value, "null");
    }

    #[test]
    fn json_invalid_perl5lib_precedence_warns_and_keeps_current() {
        let mut config = WorkspaceConfig {
            perl5lib_precedence: Perl5LibPrecedence::Append,
            ..WorkspaceConfig::default()
        };
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "workspace": { "perl5libPrecedence": "badvalue" }
            }));
        });
        assert_eq!(config.perl5lib_precedence, Perl5LibPrecedence::Append);
        assert_warned_contains(&captured, &["perl5libPrecedence", "badvalue"]);
    }

    #[test]
    fn json_severity_out_of_range_warns_and_clamps() {
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "severity": 99 }
            }));
        });
        assert_eq!(config.perlcritic_severity, 5);
        assert_warned_contains(&captured, &["critic.severity", "99"]);
    }

    #[test]
    fn toml_invalid_critic_engine_warns_and_keeps_default() {
        let mut config = ServerConfig::default();
        let prior = config.critic_engine;
        let mut project = ProjectConfig::default();
        project.critic.engine = Some("nativ".to_string());
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert_eq!(config.critic_engine, prior);
        assert_warned_contains(&captured, &["critic.engine", "nativ"]);
    }

    #[test]
    fn toml_invalid_critic_profile_warns_and_keeps_current() {
        let mut config =
            ServerConfig { native_critic_profile: "strict".to_string(), ..ServerConfig::default() };
        let mut project = ProjectConfig::default();
        project.critic.profile = Some("recomended".to_string());
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert_eq!(config.native_critic_profile, "strict");
        assert_warned_contains(&captured, &["critic.profile", "recomended"]);
    }

    #[test]
    fn toml_invalid_formatting_engine_warns_and_keeps_default() {
        let mut config = ServerConfig::default();
        let prior = config.formatting_engine;
        let mut project = ProjectConfig::default();
        project.formatting.engine = Some("perltide".to_string());
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert_eq!(config.formatting_engine, prior);
        assert_warned_contains(&captured, &["formatting.engine", "perltide"]);
    }

    #[test]
    fn toml_severity_out_of_range_warns_and_clamps() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.diagnostics.perlcritic_severity = Some(99);
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert_eq!(config.perlcritic_severity, 5);
        assert_warned_contains(&captured, &["perlcritic_severity", "99"]);
    }

    #[test]
    fn project_config_applies_next_edit_gate() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.next_edit.enabled = Some(true);

        project.apply_to_server_config(&mut config);

        assert!(config.next_edit.enabled);
    }

    #[test]
    fn project_config_can_disable_next_edit_gate() {
        let mut config = ServerConfig::default();
        config.next_edit.enabled = true;
        let mut project = ProjectConfig::default();
        project.next_edit.enabled = Some(false);

        project.apply_to_server_config(&mut config);

        assert!(!config.next_edit.enabled);
    }

    #[test]
    fn apply_to_server_config_does_not_overwrite_unset_values() {
        let mut config = ServerConfig {
            perlcritic_enabled: true,
            inlay_hints_enabled: true,
            next_edit: NextEditConfig { enabled: true },
            ..ServerConfig::default()
        };
        let project = ProjectConfig::default();

        project.apply_to_server_config(&mut config);

        assert!(config.perlcritic_enabled);
        assert!(config.inlay_hints_enabled);
        assert!(config.next_edit.enabled);
    }

    #[test]
    fn apply_to_workspace_config_only_overrides_non_empty_include_paths() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut workspace = WorkspaceConfig::default();
        let baseline_include_paths = workspace.include_paths.clone();

        let mut project = ProjectConfig::default();
        project.apply_to_workspace_config(&mut workspace, temp.path());
        assert_eq!(workspace.include_paths, baseline_include_paths);

        project.perl.include_paths = vec!["custom/lib".to_string()];
        project.apply_to_workspace_config(&mut workspace, temp.path());
        assert_eq!(workspace.include_paths, vec!["custom/lib"]);
        Ok(())
    }

    #[test]
    fn apply_to_workspace_config_rejects_absolute_include_paths() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut workspace = WorkspaceConfig::default();
        let mut project = ProjectConfig::default();
        // "/etc" is not absolute on Windows, so the assertion would pass for
        // the wrong reason there (rejected as a bad relative path, not as an
        // absolute one). Pick a genuinely absolute path per platform.
        let absolute = if cfg!(windows) { "C:\\Windows" } else { "/etc" };
        project.perl.include_paths = vec![absolute.to_string(), "relative/lib".to_string()];

        let rejected = project.apply_to_workspace_config(&mut workspace, temp.path());

        assert_eq!(workspace.include_paths, vec!["relative/lib".to_string()]);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entry, absolute);
        assert_eq!(rejected[0].reason, RejectedIncludePathReason::Absolute);
        Ok(())
    }

    /// Rejected entries are workspace-controlled, so `render` must not emit raw
    /// control characters into a terminal or an editor message.
    ///
    /// The absolute-path branch rejects before `validate_workspace_path` runs,
    /// so its `InvalidPathCharacters` check never sees these entries — without
    /// central escaping, a hostile `.perl-lsp.toml` could inject ANSI/OSC
    /// sequences through the very warning that reports it (CWE-150).
    #[test]
    fn render_escapes_control_characters_in_workspace_controlled_entries() {
        let rejected = RejectedIncludePath {
            entry: "/etc\u{1b}]0;pwned\u{7}".to_string(),
            reason: RejectedIncludePathReason::Absolute,
        };

        let rendered = rejected.render();
        assert!(
            !rendered.chars().any(|c| c.is_control() && c != '\t'),
            "render must not emit raw control characters; got {rendered:?}"
        );
        assert!(
            rendered.contains("\\u{001b}"),
            "the escape sequence should be shown in printable form; got {rendered:?}"
        );

        // C1 controls (U+0080-U+009F) are also ANSI-capable, and newline lets a
        // single entry forge an extra line of output.
        for (label, raw) in [("C1 CSI", "\u{9b}31m"), ("newline", "a\nFAKE: all good")] {
            let out = RejectedIncludePath {
                entry: raw.to_string(),
                reason: RejectedIncludePathReason::Absolute,
            }
            .render();
            assert!(
                !out.chars().any(|c| c.is_control() && c != '\t'),
                "{label} must be escaped; got {out:?}"
            );
        }

        // Bidi overrides are NOT control characters by Unicode category but
        // visually reorder text, so an entry could display as a different path
        // than the one configured (Trojan Source, CVE-2021-42574).
        let bidi = RejectedIncludePath {
            entry: "safe\u{202e}gnp.exe".to_string(),
            reason: RejectedIncludePathReason::Absolute,
        }
        .render();
        assert!(
            !bidi.contains('\u{202e}'),
            "bidi override must not survive rendering; got {bidi:?}"
        );
    }

    /// Pin the exact `WorkspacePathError` -> `RejectedIncludePathReason` routing.
    ///
    /// `from_path_error` has no wildcard arm, so a new upstream variant is a
    /// compile error rather than a silent demotion into a generic bucket. This
    /// test guards the other direction: that the existing variants keep routing
    /// where they should, which a compile check cannot catch.
    #[test]
    fn rejection_reason_maps_each_workspace_path_error_exactly() {
        use perl_parser_core::path_security::WorkspacePathError;

        assert!(matches!(
            RejectedIncludePathReason::from_path_error(&WorkspacePathError::PathTraversalAttempt(
                "d".into()
            )),
            RejectedIncludePathReason::Traversal(_)
        ));
        assert!(matches!(
            RejectedIncludePathReason::from_path_error(&WorkspacePathError::PathOutsideWorkspace(
                "d".into()
            )),
            RejectedIncludePathReason::OutsideWorkspace(_)
        ));
        assert!(matches!(
            RejectedIncludePathReason::from_path_error(
                &WorkspacePathError::SymlinkOutsideWorkspace("d".into())
            ),
            RejectedIncludePathReason::SymlinkOutsideWorkspace(_)
        ));
        assert!(matches!(
            RejectedIncludePathReason::from_path_error(&WorkspacePathError::InvalidPathCharacters),
            RejectedIncludePathReason::InvalidCharacters
        ));
    }

    #[test]
    fn apply_to_workspace_config_rejects_traversal_include_paths() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut workspace = WorkspaceConfig::default();
        let mut project = ProjectConfig::default();
        project.perl.include_paths = vec!["../../../../etc".to_string(), "vendor/lib".to_string()];

        let rejected = project.apply_to_workspace_config(&mut workspace, temp.path());

        assert_eq!(workspace.include_paths, vec!["vendor/lib".to_string()]);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entry, "../../../../etc");
        // "../../../../etc" exists, so it canonicalizes and is correctly
        // categorised as resolving outside the workspace. `Traversal` is for
        // lexically-escaping paths that cannot be canonicalized. Exact routing
        // of every WorkspacePathError variant is pinned by
        // `rejection_reason_maps_each_workspace_path_error_exactly`.
        assert!(
            matches!(rejected[0].reason, RejectedIncludePathReason::OutsideWorkspace(_)),
            "an existing outside path must route to OutsideWorkspace; got {:?}",
            rejected[0].reason
        );
        Ok(())
    }

    #[test]
    fn apply_to_workspace_config_allows_internal_dotdot_that_stays_in_workspace() -> TestResult {
        // `lib/../lib2` lexically escapes and re-enters, but the net result
        // stays inside the workspace — must be kept, not over-rejected.
        let temp = tempfile::tempdir()?;
        std::fs::create_dir_all(temp.path().join("lib2"))?;
        let mut workspace = WorkspaceConfig::default();
        let mut project = ProjectConfig::default();
        project.perl.include_paths = vec!["lib/../lib2".to_string()];

        let rejected = project.apply_to_workspace_config(&mut workspace, temp.path());

        assert_eq!(workspace.include_paths, vec!["lib/../lib2".to_string()]);
        assert!(rejected.is_empty());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn apply_to_workspace_config_rejects_symlink_escaping_workspace() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace_root = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&workspace_root)?;
        std::fs::create_dir_all(&outside)?;
        std::os::unix::fs::symlink(&outside, workspace_root.join("escape_link"))?;

        let mut workspace = WorkspaceConfig::default();
        let mut project = ProjectConfig::default();
        project.perl.include_paths = vec!["escape_link".to_string()];

        let rejected = project.apply_to_workspace_config(&mut workspace, &workspace_root);

        assert!(workspace.include_paths.is_empty());
        assert_eq!(rejected.len(), 1);
        assert!(
            matches!(rejected[0].reason, RejectedIncludePathReason::SymlinkOutsideWorkspace(_)),
            "symlink escape must route to SymlinkOutsideWorkspace; got {:?}",
            rejected[0].reason
        );
        Ok(())
    }

    #[test]
    fn apply_to_workspace_config_fails_closed_when_workspace_root_missing() -> TestResult {
        let missing_root = if cfg!(windows) {
            Path::new("C:\\nonexistent\\perl-lsp-swarm-4957-workspace-root")
        } else {
            Path::new("/nonexistent/perl-lsp-swarm-4957-workspace-root")
        };
        let absolute = if cfg!(windows) { "C:\\Windows" } else { "/etc" };
        let mut workspace = WorkspaceConfig::default();
        let mut project = ProjectConfig::default();
        project.perl.include_paths = vec!["relative/lib".to_string(), absolute.to_string()];

        project.perl.discovery_extensions = vec!["pl".to_string()];

        let rejected = project.apply_to_workspace_config(&mut workspace, missing_root);

        // Fail closed for include roots: every entry is dropped, including the
        // otherwise-safe relative one.
        assert!(workspace.include_paths.is_empty());

        // Reported once against the root, not repeated per configured entry.
        assert_eq!(rejected.len(), 1, "root failure is one rejection, got {rejected:?}");
        assert!(matches!(
            rejected[0].reason,
            RejectedIncludePathReason::WorkspaceRootUnavailable(_)
        ));

        // Scoped: a path-validation failure must not silently discard unrelated
        // project configuration.
        assert_eq!(
            workspace.discovery_extra_extensions,
            vec!["pl".to_string()],
            "unrelated project config must still apply when the root is unusable"
        );
        Ok(())
    }

    #[test]
    fn update_from_value_rejects_absolute_include_paths() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut workspace = WorkspaceConfig::default();
        let absolute = if cfg!(windows) { "C:\\Windows" } else { "/etc" };

        let rejected = workspace.update_from_value_with_context(
            &serde_json::json!({ "workspace": { "includePaths": [absolute, "lib"] } }),
            WorkspaceConfigUpdateContext {
                workspace_root: Some(temp.path()),
                external_include_paths: ExternalIncludePathAuthority::Untrusted(
                    UnauthorizedExternalIncludePathSource::Unknown,
                ),
            },
        );

        assert_eq!(workspace.include_paths, vec!["lib".to_string()]);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entry, absolute);
        assert_eq!(rejected[0].reason, RejectedClientIncludePathReason::Absolute);
        assert!(
            rejected[0].render().contains("externalIncludePaths"),
            "rejection message should name externalIncludePaths: {}",
            rejected[0].render()
        );
        Ok(())
    }

    #[test]
    fn update_from_value_rejects_traversal_include_paths() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut workspace = WorkspaceConfig::default();

        let rejected = workspace.update_from_value_with_context(
            &serde_json::json!({ "workspace": { "includePaths": ["../../../../etc", "vendor/lib"] } }),
            WorkspaceConfigUpdateContext {
                workspace_root: Some(temp.path()),
                external_include_paths: ExternalIncludePathAuthority::Untrusted(
                    UnauthorizedExternalIncludePathSource::Unknown,
                ),
            },
        );

        assert_eq!(workspace.include_paths, vec!["vendor/lib".to_string()]);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entry, "../../../../etc");
        assert!(matches!(rejected[0].reason, RejectedClientIncludePathReason::EscapesWorkspace(_)));
        Ok(())
    }

    #[test]
    fn update_from_value_accepts_external_include_paths_from_trusted_operator() {
        let mut workspace = WorkspaceConfig::default();
        let absolute = if cfg!(windows) { "C:\\perl\\lib" } else { "/opt/perl/lib" };

        let rejected = workspace.update_from_value_with_context(
            &serde_json::json!({
                "workspace": {
                    "includePaths": ["lib"],
                    "externalIncludePaths": [absolute]
                }
            }),
            WorkspaceConfigUpdateContext {
                workspace_root: None,
                external_include_paths: ExternalIncludePathAuthority::TrustedUserOperator,
            },
        );

        assert!(rejected.is_empty());
        assert_eq!(workspace.include_paths, vec!["lib".to_string()]);
        assert_eq!(workspace.external_include_paths, vec![absolute.to_string()]);
        assert_eq!(
            workspace.effective_include_paths(&[]),
            vec!["lib".to_string(), absolute.to_string()]
        );
    }

    #[test]
    fn trusted_channel_rejects_mixed_external_candidate_atomically() {
        let mut workspace = WorkspaceConfig::default();
        let absolute = if cfg!(windows) { "C:\\perl\\lib" } else { "/opt/perl/lib" };

        // Seed an accepted complete set first.
        workspace
            .update_from_value_with_context(
                &serde_json::json!({
                    "workspace": { "externalIncludePaths": [absolute] }
                }),
                WorkspaceConfigUpdateContext {
                    workspace_root: None,
                    external_include_paths: ExternalIncludePathAuthority::TrustedUserOperator,
                },
            )
            .into_iter()
            .for_each(|_| {});

        // A later mixed candidate (one valid, one relative) is rejected as a
        // whole; the previously accepted complete set survives untouched.
        let rejected = workspace.update_from_value_with_context(
            &serde_json::json!({
                "workspace": { "externalIncludePaths": ["lib", absolute] }
            }),
            WorkspaceConfigUpdateContext {
                workspace_root: None,
                external_include_paths: ExternalIncludePathAuthority::TrustedUserOperator,
            },
        );

        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entry, "lib");
        assert!(matches!(rejected[0].reason, RejectedClientIncludePathReason::ExternalRelative));
        assert_eq!(
            workspace.external_include_paths,
            vec![absolute.to_string()],
            "mixed candidate must not partially replace the accepted set"
        );
    }

    #[test]
    fn update_from_value_default_rejects_external_include_paths_as_unclassified() {
        let mut workspace = WorkspaceConfig::default();
        let absolute = if cfg!(windows) { "C:\\perl\\lib" } else { "/opt/perl/lib" };

        let rejected = workspace.update_from_value(&serde_json::json!({
            "workspace": {
                "externalIncludePaths": [absolute]
            }
        }));

        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entry, absolute);
        assert_eq!(
            rejected[0].reason,
            RejectedClientIncludePathReason::ExternalUnauthorized(
                UnauthorizedExternalIncludePathSource::Unknown
            )
        );
        assert!(
            rejected[0].render().contains("includePaths"),
            "rejection message should name the supported alternative: {}",
            rejected[0].render()
        );
        assert!(workspace.external_include_paths.is_empty());
    }

    #[test]
    fn update_from_value_rejects_external_include_paths_from_folder_channel() {
        let mut workspace = WorkspaceConfig::default();
        let absolute = if cfg!(windows) { "C:\\perl\\lib" } else { "/opt/perl/lib" };

        let rejected = workspace.update_from_value_with_context(
            &serde_json::json!({
                "workspace": {
                    "externalIncludePaths": [absolute]
                }
            }),
            WorkspaceConfigUpdateContext {
                workspace_root: None,
                external_include_paths: ExternalIncludePathAuthority::Untrusted(
                    UnauthorizedExternalIncludePathSource::FolderConfiguration,
                ),
            },
        );

        assert_eq!(rejected.len(), 1);
        assert_eq!(
            rejected[0].reason,
            RejectedClientIncludePathReason::ExternalUnauthorized(
                UnauthorizedExternalIncludePathSource::FolderConfiguration
            )
        );
        assert!(
            rejected[0].render().contains("folder-scoped"),
            "rejection should name the channel: {}",
            rejected[0].render()
        );
        assert!(workspace.external_include_paths.is_empty());
    }

    #[test]
    fn unauthorized_external_candidate_preserves_accepted_trusted_values() {
        let mut workspace = WorkspaceConfig::default();
        let trusted_root = if cfg!(windows) { "C:\\perl\\lib" } else { "/opt/perl/lib" };
        let hostile_root = if cfg!(windows) { "C:\\Windows" } else { "/etc" };

        workspace
            .update_from_value_with_context(
                &serde_json::json!({ "workspace": { "externalIncludePaths": [trusted_root] } }),
                WorkspaceConfigUpdateContext {
                    workspace_root: None,
                    external_include_paths: ExternalIncludePathAuthority::TrustedUserOperator,
                },
            )
            .into_iter()
            .for_each(|_| {});

        // A later unauthorized candidate (hostile or stale) must not clear or
        // partially replace the previously accepted trusted set.
        let rejected = workspace.update_from_value_with_context(
            &serde_json::json!({ "workspace": { "externalIncludePaths": [hostile_root] } }),
            WorkspaceConfigUpdateContext {
                workspace_root: None,
                external_include_paths: ExternalIncludePathAuthority::Untrusted(
                    UnauthorizedExternalIncludePathSource::DidChangeConfiguration,
                ),
            },
        );

        assert_eq!(rejected.len(), 1);
        assert_eq!(
            rejected[0].reason,
            RejectedClientIncludePathReason::ExternalUnauthorized(
                UnauthorizedExternalIncludePathSource::DidChangeConfiguration
            )
        );
        assert_eq!(workspace.external_include_paths, vec![trusted_root.to_string()]);
        assert!(!workspace.effective_include_paths(&[]).iter().any(|p| p == hostile_root));
    }

    #[test]
    fn empty_unauthorized_external_include_paths_produce_no_noise() {
        let mut workspace = WorkspaceConfig::default();

        let rejected = workspace.update_from_value_with_context(
            &serde_json::json!({ "workspace": { "externalIncludePaths": [] } }),
            WorkspaceConfigUpdateContext {
                workspace_root: None,
                external_include_paths: ExternalIncludePathAuthority::Untrusted(
                    UnauthorizedExternalIncludePathSource::GenericUnscopedConfiguration,
                ),
            },
        );

        assert!(rejected.is_empty());
        assert!(workspace.external_include_paths.is_empty());
    }

    #[test]
    fn apply_to_workspace_config_sets_perl5lib_toggles() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut workspace = WorkspaceConfig::default();
        let mut project = ProjectConfig::default();
        project.perl.use_perl5lib = Some(false);
        project.perl.perl5lib_precedence = Some(Perl5LibPrecedence::Append);

        project.apply_to_workspace_config(&mut workspace, temp.path());

        assert!(!workspace.use_perl5lib);
        assert!(matches!(workspace.perl5lib_precedence, Perl5LibPrecedence::Append));
        Ok(())
    }

    #[test]
    fn project_and_client_config_apply_discovery_policy() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut workspace = WorkspaceConfig::default();
        let mut project = ProjectConfig::default();
        project.perl.discovery_extensions = vec![".foo".to_string()];
        project.perl.discovery_skipped_dirs = vec!["generated".to_string()];

        project.apply_to_workspace_config(&mut workspace, temp.path());
        assert_eq!(workspace.discovery_extra_extensions, vec![".foo"]);
        assert_eq!(workspace.discovery_extra_skipped_dirs, vec!["generated"]);

        workspace.update_from_value(&serde_json::json!({
            "workspace": {
                "discoveryExtensions": [" .bar ", ".bar", "BAR"],
                "discoverySkippedDirs": [" cache ", "cache"]
            }
        }));
        assert_eq!(workspace.discovery_extra_extensions, vec![".bar", "BAR"]);
        assert_eq!(workspace.discovery_extra_skipped_dirs, vec!["cache"]);
        Ok(())
    }

    #[test]
    fn parse_perl5lib_trims_and_dedupes_entries() {
        // Use the platform separator so the test works on both Unix and Windows.
        #[cfg(windows)]
        let input = " lib ;local/lib;;lib; ";
        #[cfg(not(windows))]
        let input = " lib :local/lib::lib: ";
        let parsed = WorkspaceConfig::parse_perl5lib(input);
        // normalize_include_path round-trips through PathBuf, which emits the
        // platform-native separator.  Gate the expected value accordingly.
        #[cfg(windows)]
        assert_eq!(parsed, vec!["lib", "local\\lib"]);
        #[cfg(not(windows))]
        assert_eq!(parsed, vec!["lib", "local/lib"]);
    }

    #[test]
    fn effective_include_paths_dedupes_with_prepend_precedence() {
        let config = WorkspaceConfig {
            include_paths: vec!["lib".to_string(), "local/lib".to_string(), "lib".to_string()],
            perl5lib_precedence: Perl5LibPrecedence::Prepend,
            ..WorkspaceConfig::default()
        };

        let paths = config.effective_include_paths(&[
            "local/lib".to_string(),
            "vendor/lib".to_string(),
            "vendor/lib".to_string(),
        ]);

        // normalize_include_path uses PathBuf internally, so separators are platform-native.
        #[cfg(windows)]
        assert_eq!(paths, vec!["local\\lib", "vendor\\lib", "lib"]);
        #[cfg(not(windows))]
        assert_eq!(paths, vec!["local/lib", "vendor/lib", "lib"]);
    }

    /// Precedence audit: when the SAME path appears in both `PERL5LIB` and
    /// `include_paths`, `Prepend` resolves it from PERL5LIB's slot (early in
    /// the search list) and `Append` resolves it from `include_paths`' slot.
    ///
    /// This pins the policy decision flagged in the 2026-05-11 @INC
    /// checkpoint: `perl5libPrecedence=prepend` is **Perl-faithful**
    /// (PERL5LIB shadows workspace `includePaths` for overlapping modules).
    /// `Append` makes workspace `includePaths` win over PERL5LIB. The
    /// resolver iterates in the returned order and picks the first hit, so
    /// the asserted index is the resolution outcome.
    #[test]
    fn perl5lib_precedence_pins_shadow_order_for_overlapping_paths() {
        let shared = "shared/lib".to_string();
        let perl5lib = vec![shared.clone()];
        let workspace = vec![shared.clone()];

        let prepend_config = WorkspaceConfig {
            include_paths: workspace.clone(),
            perl5lib_precedence: Perl5LibPrecedence::Prepend,
            ..WorkspaceConfig::default()
        };
        let prepend = prepend_config.effective_include_paths(&perl5lib);
        assert_eq!(prepend.len(), 1, "dedup must collapse to one entry");
        // With prepend, the overlapping path is contributed by PERL5LIB
        // first and the workspace copy is deduped. The kept entry is
        // conceptually the PERL5LIB one — which matches Perl's runtime
        // behaviour where `-I` / PERL5LIB precedes the default `@INC`.

        let append_config = WorkspaceConfig {
            include_paths: workspace,
            perl5lib_precedence: Perl5LibPrecedence::Append,
            ..WorkspaceConfig::default()
        };
        let append = append_config.effective_include_paths(&perl5lib);
        assert_eq!(append.len(), 1, "dedup must collapse to one entry");
        // Symmetric: with append, the workspace `includePaths` entry comes
        // first; the PERL5LIB duplicate is deduped after it.

        // Multi-path overlap: shared appears in both lists, plus distinct
        // entries on each side. Prepend interleaves PERL5LIB-first.
        let prepend_multi_config = WorkspaceConfig {
            include_paths: vec!["workspace_only".into(), shared.clone()],
            perl5lib_precedence: Perl5LibPrecedence::Prepend,
            ..WorkspaceConfig::default()
        };
        let prepend_multi =
            prepend_multi_config.effective_include_paths(&["env_only".into(), shared.clone()]);
        // PERL5LIB entries first → env_only then shared (from env). The
        // workspace's `shared` is deduped. workspace_only follows.
        #[cfg(windows)]
        assert_eq!(prepend_multi, vec!["env_only", "shared\\lib", "workspace_only"]);
        #[cfg(not(windows))]
        assert_eq!(prepend_multi, vec!["env_only", "shared/lib", "workspace_only"]);

        // And append flips it: workspace entries first, then env-only.
        let append_multi_config = WorkspaceConfig {
            include_paths: vec!["workspace_only".into(), shared.clone()],
            perl5lib_precedence: Perl5LibPrecedence::Append,
            ..WorkspaceConfig::default()
        };
        let append_multi =
            append_multi_config.effective_include_paths(&["env_only".into(), shared.clone()]);
        #[cfg(windows)]
        assert_eq!(append_multi, vec!["workspace_only", "shared\\lib", "env_only"]);
        #[cfg(not(windows))]
        assert_eq!(append_multi, vec!["workspace_only", "shared/lib", "env_only"]);
    }

    #[test]
    fn effective_include_paths_dedupes_with_append_precedence() {
        let config = WorkspaceConfig {
            include_paths: vec!["lib".to_string(), "local/lib".to_string()],
            perl5lib_precedence: Perl5LibPrecedence::Append,
            ..WorkspaceConfig::default()
        };

        let paths = config.effective_include_paths(&[
            "local/lib".to_string(),
            "vendor/lib".to_string(),
            "lib".to_string(),
        ]);

        // normalize_include_path uses PathBuf internally, so separators are platform-native.
        #[cfg(windows)]
        assert_eq!(paths, vec!["lib", "local\\lib", "vendor\\lib"]);
        #[cfg(not(windows))]
        assert_eq!(paths, vec!["lib", "local/lib", "vendor/lib"]);
    }

    #[test]
    fn effective_include_paths_filters_whitespace_only_entries() {
        // Whitespace-only entries in include_paths must be silently dropped.
        let config = WorkspaceConfig {
            include_paths: vec![
                "lib".to_string(),
                "  ".to_string(),
                "".to_string(),
                "lib".to_string(),
            ],
            perl5lib_precedence: Perl5LibPrecedence::Prepend,
            ..WorkspaceConfig::default()
        };
        // use_perl5lib is true by default but perl5lib_paths is empty → takes the
        // early-return branch that also dedupes and trims include_paths.
        let paths = config.effective_include_paths(&[]);
        assert_eq!(paths, vec!["lib"]);
    }

    #[test]
    fn parse_perl5lib_normalizes_dot_and_trailing_slash_entries() {
        #[cfg(windows)]
        let input = ".;./lib;lib\\;./lib\\";
        #[cfg(not(windows))]
        let input = ".:./lib:lib/:./lib/";

        let parsed = WorkspaceConfig::parse_perl5lib(input);
        assert_eq!(parsed, vec![".", "lib"]);
    }

    #[test]
    fn effective_include_paths_normalizes_equivalent_entries_before_dedupe() {
        let config = WorkspaceConfig {
            include_paths: vec!["./lib".to_string(), "lib/".to_string(), ".".to_string()],
            perl5lib_precedence: Perl5LibPrecedence::Prepend,
            ..WorkspaceConfig::default()
        };

        let paths = config.effective_include_paths(&["./lib/".to_string(), " ./ ".to_string()]);
        assert_eq!(paths, vec!["lib", "."]);
    }

    /// Security regression: workspace settings must not be able to redirect the
    /// Perl interpreter / argv used for the @INC probe (issue #3729). A hostile
    /// project could otherwise execute arbitrary code at config-load time.
    #[test]
    fn workspace_config_ignores_untrusted_perl_probe_settings() {
        let mut config = WorkspaceConfig::default();
        config.update_from_value(&serde_json::json!({
            "workspace": {
                "perlPath": "/opt/custom/perl",
                "perlArgs": ["-I", "/tmp/custom/lib"]
            }
        }));
        assert!(config.perl_path.is_none(), "perlPath from workspace must be ignored");
        assert!(config.perl_args.is_empty(), "perlArgs from workspace must be ignored");
    }

    /// Security regression for the resource-scoped client-settings channel:
    /// absolute paths must be rejected even when no workspace root is known.
    /// Legitimate workspace-relative entries remain accepted.
    #[test]
    fn update_from_value_rejects_absolute_include_paths_without_workspace_root() {
        let mut config = WorkspaceConfig::default();
        let absolute = if cfg!(windows) { r"C:\Windows" } else { "/opt/company-perl-libs" };
        let rejected = config.update_from_value(&serde_json::json!({
            "workspace": {
                "includePaths": [absolute, "relative/lib"]
            }
        }));

        assert_eq!(config.include_paths, vec!["relative/lib".to_string()]);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].entry, absolute);
        assert_eq!(rejected[0].reason, RejectedClientIncludePathReason::Absolute);
    }

    /// Security regression: the LSP settings channel must not arm legacy subprocess
    /// profile paths or external formatter argv (issue #5001). Trusted project
    /// config (`.perl-lsp.toml`) still applies via `apply_to_server_config`.
    #[test]
    fn server_config_update_from_value_ignores_untrusted_subprocess_capabilities() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "perlcritic": {
                "profile": "/tmp/hostile/.perlcriticrc",
                "theme": "core && !pbp"
            },
            "critic": {
                "engine": "legacy"
            },
            "formatting": {
                "profile": "/tmp/hostile/.perltidyrc",
                "extraArgs": ["--logfile=/tmp/evil.log"]
            }
        }));

        assert!(config.perlcritic_profile.is_none());
        assert!(config.perlcritic_theme.is_none());
        assert_eq!(config.critic_engine, CriticEngine::Native);
        assert!(config.perltidy_profile.is_none());
        assert!(config.perltidy_extra_args.is_empty());
    }
    #[test]
    fn parse_perl_inc_output_filters_dynamic_hook_entries() {
        let parsed = WorkspaceConfig::parse_perl_inc_output(
            "lib\nCODE(0x123)\nARRAY(0xabc)\nHASH(0xdef)\n/usr/lib/perl5\n",
        );
        assert_eq!(parsed, vec![PathBuf::from("lib"), PathBuf::from("/usr/lib/perl5")]);
    }

    #[test]
    fn parse_perl_inc_output_dedupes_and_drops_dot() {
        let parsed =
            WorkspaceConfig::parse_perl_inc_output("lib\n.\nlib\n/usr/lib/perl5\n/usr/lib/perl5\n");
        assert_eq!(parsed, vec![PathBuf::from("lib"), PathBuf::from("/usr/lib/perl5")]);
    }

    /// Regression guard for the unbounded `Command::output()` stall: a long-running
    /// probe must be killed within roughly `timeout + poll_interval`. We use perl
    /// itself (via the same toolchain resolver that `fetch_perl_inc` uses) to
    /// guarantee an interpreter is present; the test skips when no perl is
    /// available.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn output_with_timeout_kills_long_running_subprocess() -> TestResult {
        let perl_path = match resolve_perl_path_with_toolchain() {
            Ok(path) => path,
            Err(_) => return Ok(()),
        };

        let mut command = Command::new(perl_path);
        command.args(["-e", "sleep 10; print 'should not reach'"]);

        let start = Instant::now();
        let result = output_with_timeout(command, Duration::from_millis(250));
        let elapsed = start.elapsed();

        let err = match result {
            Ok(out) => return Err(format!("expected timeout, got success: {out:?}").into()),
            Err(e) => e,
        };
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::TimedOut,
            "expected ErrorKind::TimedOut, got {err:?}",
        );
        // Allow generous overhead (slow CI cold start, antivirus, etc.).
        assert!(
            elapsed < Duration::from_secs(3),
            "timeout should fire within reasonable overhead, took {elapsed:?}",
        );
        Ok(())
    }

    /// `get_system_inc` must respect `SYSTEM_INC_PROBE_TIMEOUT` so a hung
    /// interpreter cannot block the LSP request thread. Verifies the full
    /// path: `use_system_inc=true`, slow `perl_path`, the lazy probe lands
    /// in a bounded failure outcome, the returned slice is empty, and the
    /// typed cache holds that outcome for reuse.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn get_system_inc_does_not_stall_on_slow_interpreter() -> TestResult {
        let perl_path = match resolve_perl_path_with_toolchain() {
            Ok(path) => path,
            Err(_) => return Ok(()),
        };

        let mut config = WorkspaceConfig {
            use_system_inc: true,
            perl_path: Some(perl_path.to_string_lossy().into_owned()),
            // perl_args runs BEFORE -e 'print @INC', so we make perl sleep up front.
            // The sleep is much longer than SYSTEM_INC_PROBE_TIMEOUT (1s).
            perl_args: vec!["-e".into(), "sleep 10".into()],
            ..WorkspaceConfig::default()
        };

        let start = Instant::now();
        let outcome = config.get_system_inc_probe_outcome();
        let paths = config.get_system_inc().to_vec();
        let elapsed = start.elapsed();

        // The contract under test is bounded, empty, and cached — NOT which
        // failure class the runner's perl produces. The resolved interpreter
        // varies by environment (msys-vs-Strawberry on Windows, shimmed
        // perls on CI) and a `-e "sleep 10"` program can exit nonzero or
        // fail to spawn before the 1s timeout fires; both were observed
        // (CI red with NonZeroExit where the author saw TimedOut locally).
        // SuccessfulEmpty/Paths WOULD be failures here: the sleep program
        // must never produce paths.
        assert!(
            matches!(
                outcome,
                SystemIncProbeOutcome::TimedOut
                    | SystemIncProbeOutcome::NonZeroExit
                    | SystemIncProbeOutcome::IoFailed
                    | SystemIncProbeOutcome::Unavailable
            ),
            "expected a bounded failure outcome, got {outcome:?}"
        );
        assert!(paths.is_empty(), "expected empty @INC on timeout, got {paths:?}");
        // Generous bound: SYSTEM_INC_PROBE_TIMEOUT (1s) + spawn + poll overhead.
        assert!(
            elapsed < Duration::from_secs(4),
            "get_system_inc must return within timeout+overhead, took {elapsed:?}",
        );

        // Cached empty result — second call does not respawn perl.
        let start2 = Instant::now();
        let paths2 = config.get_system_inc().to_vec();
        let elapsed2 = start2.elapsed();
        assert!(paths2.is_empty());
        assert!(
            elapsed2 < Duration::from_millis(50),
            "cached lookup should be fast, took {elapsed2:?}",
        );
        Ok(())
    }

    /// A second lookup must reuse the first probe result rather than launch a
    /// new interpreter. The missing path makes a reprobe observably fail with
    /// `IoFailed`, so a fast second process cannot satisfy this oracle.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn get_system_inc_reuses_cached_probe_without_relaunching() -> TestResult {
        let perl_path = match resolve_perl_path_with_toolchain() {
            Ok(path) => path,
            Err(_) => return Ok(()),
        };
        let missing_perl = tempfile::tempdir()?.path().join("missing-perl");
        let mut config = WorkspaceConfig {
            use_system_inc: true,
            perl_path: Some(perl_path.to_string_lossy().into_owned()),
            // Quote-, space-, and backslash-free program: the sentinel arg
            // passes through the oracle's env-stripped command, where
            // embedded double quotes broke arg quoting into a NonZeroExit,
            // and msys perl on Windows ate the backslash of a qq newline
            // escape. The trailing semicolon is load-bearing —
            // `fetch_perl_inc` appends its own `-e` block and perl
            // concatenates -e programs into ONE script, so without it the
            // concatenation is a syntax error — and the chr(10) keeps the
            // sentinel on its own line so it never fuses with the first
            // @INC entry.
            perl_args: vec!["-e".into(), "print(qq(cache-sentinel).chr(10));".into()],
            ..WorkspaceConfig::default()
        };

        let cached = config.get_system_inc_probe_outcome();
        let cached_paths = match &cached {
            SystemIncProbeOutcome::Paths(paths)
                if paths.iter().any(|path| path == Path::new("cache-sentinel")) =>
            {
                paths.clone()
            }
            other => {
                return Err(
                    format!("expected the sentinel probe to produce Paths, got {other:?}").into()
                );
            }
        };

        // Changing the public probe input after the first lookup is only a
        // test discriminator; normal settings updates invalidate the cache.
        config.perl_path = Some(missing_perl.to_string_lossy().into_owned());
        let reused = config.get_system_inc_probe_outcome();
        assert_eq!(reused, cached, "second lookup must reuse the cached outcome");
        assert_eq!(config.get_system_inc().to_vec(), cached_paths);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn synthetic_exit_status(code: i32) -> std::process::ExitStatus {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            ExitStatusExt::from_raw(code)
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            ExitStatusExt::from_raw(code as u32)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn synthetic_output(code: i32, stdout: &str) -> std::process::Output {
        std::process::Output {
            status: synthetic_exit_status(code),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn system_inc_probe_outcome_preserves_execution_classes() {
        assert_eq!(
            WorkspaceConfig::classify_perl_inc_output(Ok(synthetic_output(0, ".\n"))),
            SystemIncProbeOutcome::SuccessfulEmpty,
        );
        assert_eq!(
            WorkspaceConfig::classify_perl_inc_output(Ok(synthetic_output(0, "lib\n"))),
            SystemIncProbeOutcome::Paths(vec![PathBuf::from("lib")]),
        );
        assert_eq!(
            WorkspaceConfig::classify_perl_inc_output(Ok(synthetic_output(1, ""))),
            SystemIncProbeOutcome::NonZeroExit,
        );
        assert_eq!(
            WorkspaceConfig::classify_perl_inc_output(Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "synthetic timeout",
            ))),
            SystemIncProbeOutcome::TimedOut,
        );
        assert_eq!(
            WorkspaceConfig::classify_perl_inc_output(Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "synthetic spawn-or-io failure",
            ))),
            SystemIncProbeOutcome::IoFailed,
        );

        let disabled = WorkspaceConfig::default().get_system_inc_probe_outcome();
        assert_eq!(disabled, SystemIncProbeOutcome::Disabled);
    }

    /// `usePerl5lib` and `useSystemInc` must produce independent startup-`@INC`
    /// caches. When `usePerl5lib` toggles, the cache must be invalidated so the
    /// next `get_system_inc` call re-probes Perl with the correct PERL5LIB
    /// environment (stripped or inherited based on `use_perl5lib`).
    #[test]
    fn use_perl5lib_toggle_invalidates_system_inc_cache() {
        let mut config = WorkspaceConfig { use_system_inc: true, ..WorkspaceConfig::default() };
        assert!(config.use_perl5lib, "default usePerl5lib should be true");

        // Pre-populate the cache; flipping usePerl5lib must clear it.
        config.system_inc_cache =
            Some(SystemIncProbeOutcome::Paths(vec![PathBuf::from("/sentinel/cached")]));
        config.update_from_value(&serde_json::json!({
            "workspace": { "usePerl5lib": false }
        }));
        assert!(!config.use_perl5lib);
        assert!(
            config.system_inc_cache.is_none(),
            "system_inc_cache must invalidate when usePerl5lib changes (true -> false); \
             got {:?}",
            config.system_inc_cache
        );

        // Flip back the other direction.
        config.system_inc_cache =
            Some(SystemIncProbeOutcome::Paths(vec![PathBuf::from("/sentinel/cached2")]));
        config.update_from_value(&serde_json::json!({
            "workspace": { "usePerl5lib": true }
        }));
        assert!(config.use_perl5lib);
        assert!(
            config.system_inc_cache.is_none(),
            "system_inc_cache must invalidate when usePerl5lib changes (false -> true); \
             got {:?}",
            config.system_inc_cache
        );

        // No-op (same value) must NOT clear the cache.
        let stable = vec![PathBuf::from("/sentinel/stable")];
        config.system_inc_cache = Some(SystemIncProbeOutcome::Paths(stable.clone()));
        config.update_from_value(&serde_json::json!({
            "workspace": { "usePerl5lib": true }
        }));
        assert_eq!(
            config.system_inc_cache,
            Some(SystemIncProbeOutcome::Paths(stable)),
            "cache must survive when usePerl5lib value does not change",
        );
    }

    /// An unrecognised `perl5libPrecedence` string must leave the current
    /// setting unchanged.  This guards the `_ => {}` arm in `update_from_value`
    /// so that a typo in workspace settings does not silently reset an
    /// explicitly-configured `Append` back to the default `Prepend`.
    #[test]
    fn update_from_value_keeps_existing_perl5lib_precedence_on_unknown_value() {
        let mut config = WorkspaceConfig {
            perl5lib_precedence: Perl5LibPrecedence::Append,
            ..WorkspaceConfig::default()
        };

        // An unrecognised value must not change the setting.
        config.update_from_value(&serde_json::json!({
            "workspace": { "perl5libPrecedence": "badvalue" }
        }));
        assert!(
            matches!(config.perl5lib_precedence, Perl5LibPrecedence::Append),
            "unknown perl5libPrecedence string must leave Append unchanged; got {:?}",
            config.perl5lib_precedence,
        );

        // A recognised value must still take effect.
        config.update_from_value(&serde_json::json!({
            "workspace": { "perl5libPrecedence": "prepend" }
        }));
        assert!(
            matches!(config.perl5lib_precedence, Perl5LibPrecedence::Prepend),
            "recognised 'prepend' must update perl5lib_precedence",
        );

        // Round-trip back to append.
        config.update_from_value(&serde_json::json!({
            "workspace": { "perl5libPrecedence": "append" }
        }));
        assert!(
            matches!(config.perl5lib_precedence, Perl5LibPrecedence::Append),
            "recognised 'append' must update perl5lib_precedence",
        );

        // A second unknown value after a successful round-trip must also be a no-op.
        config.update_from_value(&serde_json::json!({
            "workspace": { "perl5libPrecedence": "" }
        }));
        assert!(
            matches!(config.perl5lib_precedence, Perl5LibPrecedence::Append),
            "empty string perl5libPrecedence must also leave Append unchanged",
        );
    }

    /// Toggling `useSystemInc` must invalidate `system_inc_cache` so the next
    /// `get_system_inc` call re-probes Perl under the new setting.  This is
    /// symmetric with `use_perl5lib_toggle_invalidates_system_inc_cache` which
    /// covers the `usePerl5lib` branch of the same cache-invalidation logic.
    #[test]
    fn update_from_value_clears_system_inc_cache_when_use_system_inc_changes() {
        let mut config = WorkspaceConfig::default();
        // Default is false; toggle to true so we can test both directions.
        assert!(!config.use_system_inc, "default useSystemInc should be false");

        // Pre-populate the cache; enabling useSystemInc must clear it.
        config.system_inc_cache =
            Some(SystemIncProbeOutcome::Paths(vec![PathBuf::from("/sentinel/cached")]));
        config.update_from_value(&serde_json::json!({
            "workspace": { "useSystemInc": true }
        }));
        assert!(config.use_system_inc);
        assert!(
            config.system_inc_cache.is_none(),
            "system_inc_cache must invalidate when useSystemInc changes (false -> true); \
             got {:?}",
            config.system_inc_cache,
        );

        // Flip back the other direction.
        config.system_inc_cache =
            Some(SystemIncProbeOutcome::Paths(vec![PathBuf::from("/sentinel/cached2")]));
        config.update_from_value(&serde_json::json!({
            "workspace": { "useSystemInc": false }
        }));
        assert!(!config.use_system_inc);
        assert!(
            config.system_inc_cache.is_none(),
            "system_inc_cache must invalidate when useSystemInc changes (true -> false); \
             got {:?}",
            config.system_inc_cache,
        );

        // No-op (same value) must NOT clear the cache.
        let stable = vec![PathBuf::from("/sentinel/stable")];
        config.system_inc_cache = Some(SystemIncProbeOutcome::Paths(stable.clone()));
        config.update_from_value(&serde_json::json!({
            "workspace": { "useSystemInc": false }
        }));
        assert_eq!(
            config.system_inc_cache,
            Some(SystemIncProbeOutcome::Paths(stable)),
            "cache must survive when useSystemInc value does not change",
        );
    }

    /// JSON `null` for `apiKeyPrefix` must clear the prefix to `None` (raw key, no scheme).
    /// Empty string was already covered by `server_config_update_from_value_applies_formatting_and_ai_settings`;
    /// this guards the `Value::Null` code path which follows a distinct branch in `as_str()`.
    #[test]
    fn update_from_value_clears_api_key_prefix_on_null() {
        let mut config = ServerConfig::default();
        // Default is Some("Bearer") — must be cleared by explicit null.
        assert_eq!(config.ai_completion.api_key_prefix, Some("Bearer".to_string()));

        config.update_from_value(&serde_json::json!({
            "aiCompletion": { "apiKeyPrefix": null }
        }));
        assert_eq!(
            config.ai_completion.api_key_prefix,
            Some("Bearer".to_string()),
            "explicit JSON null must produce None (raw key, no scheme)",
        );
    }

    /// A non-empty `apiKeyPrefix` value must be ignored from didChangeConfiguration (#5684).
    #[test]
    fn update_from_value_ignores_non_empty_api_key_prefix() {
        let mut config = ServerConfig::default();

        config.update_from_value(&serde_json::json!({
            "aiCompletion": { "apiKeyPrefix": "Token" }
        }));
        assert_eq!(
            config.ai_completion.api_key_prefix,
            Some("Bearer".to_string()),
            "apiKeyPrefix from didChangeConfiguration must be ignored (stays at default, security: #5684)",
        );
    }

    /// Malformed auth header settings must not flow into outbound HTTP header construction.
    #[test]
    fn update_from_value_rejects_malformed_ai_auth_header_settings() {
        let mut config = ServerConfig::default();

        config.update_from_value(&serde_json::json!({
            "aiCompletion": {
                "apiKeyHeader": "x-api-key\r\nX-Injected",
                "apiKeyPrefix": "Token\r\nX-Injected"
            }
        }));

        assert_eq!(config.ai_completion.api_key_header, "Authorization");
        assert_eq!(config.ai_completion.api_key_prefix, Some("Bearer".to_string()));
    }

    // NOTE: `project_config_applies_ai_auth_header_and_prefix` and
    // `project_config_ignores_malformed_ai_auth_header_settings` used to live
    // here. They asserted that `ProjectConfig` (workspace-supplied
    // `.perl-lsp.toml`) could set `api_key_header` / `api_key_prefix` and have
    // it flow into `ServerConfig` — exactly the trust-boundary violation
    // fixed for issue #4955. `ProjectAiCompletionConfig` no longer has those
    // fields at all, so there is nothing left to thread through; the
    // CRLF-injection normalization behaviour they also covered remains
    // exercised for the (unaffected) user-settings path by
    // `update_from_value_rejects_malformed_ai_auth_header_settings` above.
    // See `workspace_ai_completion_ignores_untrusted_endpoint_and_credential_settings`
    // below for the regression coverage that replaced them.

    /// Security regression: a workspace-supplied `.perl-lsp.toml` must not be
    /// able to redirect the AI-completion endpoint or select which process
    /// environment variable is read as a credential (issue #4955), and must
    /// not activate AI or override user-owned `provider` / `model` (issue
    /// #4997). A hostile cloned repository could otherwise name an arbitrary
    /// secret (e.g. `AWS_SECRET_ACCESS_KEY`) and have its value POSTed to an
    /// attacker-chosen endpoint on the first inline-completion request.
    #[test]
    fn workspace_ai_completion_ignores_untrusted_endpoint_and_credential_settings() -> TestResult {
        let temp = tempfile::tempdir()?;
        std::fs::write(
            temp.path().join(".perl-lsp.toml"),
            r#"
[ai_completion]
enabled = true
provider = "openai"
model = "gpt-4"
endpoint = "http://attacker.example/v1/chat/completions"
api_key_env = "AWS_SECRET_ACCESS_KEY"
api_key_header = "X-Attacker-Header"
api_key_prefix = "Attacker "
"#,
        )?;
        let project = load_project_config(temp.path())?.ok_or("expected parsed project config")?;

        let default_config = ServerConfig::default();
        let mut config = ServerConfig::default();
        project.apply_to_server_config(&mut config);

        // The four credential/destination fields must be untouched by the
        // workspace-supplied TOML — assert per-field, not in aggregate.
        assert_eq!(
            config.ai_completion.endpoint, default_config.ai_completion.endpoint,
            "workspace-supplied endpoint must not change the effective config",
        );
        assert_eq!(
            config.ai_completion.api_key_env, default_config.ai_completion.api_key_env,
            "workspace-supplied api_key_env must not change the effective config",
        );
        assert_eq!(
            config.ai_completion.api_key_header, default_config.ai_completion.api_key_header,
            "workspace-supplied api_key_header must not change the effective config",
        );
        assert_eq!(
            config.ai_completion.api_key_prefix, default_config.ai_completion.api_key_prefix,
            "workspace-supplied api_key_prefix must not change the effective config",
        );

        // Workspace cannot activate AI or override user-owned provider/model (#4997).
        assert!(
            !config.ai_completion.enabled,
            "workspace-supplied enabled=true must not activate AI completions",
        );
        assert!(
            !config.ai_completion.user_enabled,
            "workspace-supplied enabled=true must not set user_enabled",
        );
        assert_eq!(
            config.ai_completion.provider, default_config.ai_completion.provider,
            "workspace-supplied provider must not change the effective config",
        );
        assert_eq!(
            config.ai_completion.model, default_config.ai_completion.model,
            "workspace-supplied model must not change the effective config",
        );
        Ok(())
    }

    /// Project config may opt out of AI completions when the user enabled them.
    #[test]
    fn project_config_can_opt_out_of_user_enabled_ai_completions() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "aiCompletion": { "enabled": true }
        }));
        assert!(config.ai_completion.user_enabled);
        assert!(config.ai_completion.enabled);

        let mut project = ProjectConfig::default();
        project.ai_completion.enabled = Some(false);
        project.apply_to_server_config(&mut config);

        assert!(config.ai_completion.project_opt_out);
        assert!(!config.ai_completion.enabled, "project opt-out must disable effective AI");
    }

    #[test]
    fn project_opt_out_clears_when_ai_completion_section_removed() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "aiCompletion": { "enabled": true }
        }));

        let mut project = ProjectConfig::default();
        project.ai_completion.enabled = Some(false);
        project.apply_to_server_config(&mut config);
        assert!(config.ai_completion.project_opt_out);
        assert!(!config.ai_completion.enabled);

        let project_cleared = ProjectConfig::default();
        project_cleared.apply_to_server_config(&mut config);

        assert!(!config.ai_completion.project_opt_out);
        assert!(config.ai_completion.enabled);
    }

    /// Companion to the regression above: the LSP client/server configuration
    /// channel (the `aiCompletion` block of `ServerConfig::update_from_value`)
    /// can still set endpoint/credential fields. This proves the fix closes the
    /// `.perl-lsp.toml` route rather than disabling the feature.
    ///
    /// It asserts nothing about that channel's authority for non-VS Code
    /// clients. `update_from_value` cannot tell machine, user, workspace, or
    /// folder settings apart; the VS Code extension closes activation via
    /// `scope: machine` (#4997), while endpoint/credential user UI remains a
    /// documented gap.
    #[test]
    fn client_configuration_ignores_ai_endpoint_and_credential_fields_from_didChange() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "aiCompletion": {
                "endpoint": "https://evil.example.com/exfil",
                "apiKeyEnv": "AWS_SECRET_ACCESS_KEY",
                "apiKeyHeader": "X-Evil",
                "apiKeyPrefix": "EvilToken"
            }
        }));

        // #5684: all sensitive fields must remain at defaults (not changed by didChangeConfiguration)
        assert_eq!(config.ai_completion.endpoint, "");
        assert_eq!(config.ai_completion.api_key_env, "OPENAI_API_KEY");
        assert_eq!(config.ai_completion.api_key_header, "Authorization");
        assert_eq!(config.ai_completion.api_key_prefix, Some("Bearer".to_string()));
    }

    // ── critic include/exclude rule-ID validation ──────────────────────────

    #[test]
    fn json_unknown_include_rule_id_warns_keeps_value() {
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "include": ["native.common.typo_rule", "native.testing.require_use_strict"] }
            }));
        });
        // Unknown ID stored, valid ID stored.
        assert_eq!(
            config.native_critic_include,
            vec![
                "native.common.typo_rule".to_string(),
                "native.testing.require_use_strict".to_string(),
            ]
        );
        // One warning for the unknown ID, naming the setting and the bad value.
        assert_warned_contains(&captured, &["critic.include", "native.common.typo_rule"]);
        // No warning for the valid ID.
        let combined = captured.join("\n");
        assert!(
            !combined.contains("native.testing.require_use_strict"),
            "unexpected warning for a valid rule ID; captured:\n{combined}"
        );
    }

    #[test]
    fn json_unknown_exclude_rule_id_warns() {
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "exclude": ["native.common.misspelled_rule"] }
            }));
        });
        assert_warned_contains(&captured, &["critic.exclude", "native.common.misspelled_rule"]);
        // Value is still stored — we only warn, we don't reject.
        assert_eq!(config.native_critic_exclude, vec!["native.common.misspelled_rule".to_string()]);
    }

    #[test]
    fn json_valid_include_rule_ids_produce_no_warnings() {
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": {
                    "include": ["native.testing.require_use_strict", "native.variables.unused_lexical"],
                    "exclude": ["native.common.assignment_in_condition"]
                }
            }));
        });
        assert!(
            captured.is_empty(),
            "expected no warnings for valid rule IDs; got:\n{}",
            captured.join("\n")
        );
    }

    #[test]
    fn toml_unknown_include_rule_id_warns_keeps_value() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.critic.include = Some(vec![
            "native.variables.typo_rule".to_string(),
            "native.testing.require_use_strict".to_string(),
        ]);
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert_eq!(
            config.native_critic_include,
            vec![
                "native.variables.typo_rule".to_string(),
                "native.testing.require_use_strict".to_string(),
            ]
        );
        assert_warned_contains(&captured, &["critic.include", "native.variables.typo_rule"]);
    }

    #[test]
    fn toml_unknown_exclude_rule_id_warns() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.critic.exclude = Some(vec!["native.io.bad_rule_name".to_string()]);
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert_warned_contains(&captured, &["critic.exclude", "native.io.bad_rule_name"]);
        assert_eq!(config.native_critic_exclude, vec!["native.io.bad_rule_name".to_string()]);
    }

    #[test]
    fn toml_valid_rule_ids_produce_no_warnings() {
        // Negative control for the `.perl-lsp.toml` channel: the unknown-ID
        // tests above prove the warning fires, this proves it stays silent for
        // valid IDs on the same code path.
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.critic.include = Some(vec![
            "native.testing.require_use_strict".to_string(),
            "native.variables.unused_lexical".to_string(),
        ]);
        project.critic.exclude = Some(vec!["native.common.assignment_in_condition".to_string()]);
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert!(
            captured.is_empty(),
            "expected no warnings for valid TOML rule IDs; got:\n{}",
            captured.join("\n")
        );
    }

    #[test]
    fn unknown_rule_id_warning_does_not_suggest_a_profile_change() {
        // The catalog is the strict profile, so no profile change can make an
        // unknown ID valid. The warning must not send users down that path.
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "include": ["native.common.typo_rule"] }
            }));
        });
        let combined = captured.join("\n");
        assert!(
            !combined.contains("critic.profile"),
            "warning must not suggest a profile change as remediation; got:\n{combined}"
        );
        assert!(
            combined.contains("spelling"),
            "warning should point at spelling/catalog as the actionable fix; got:\n{combined}"
        );
    }

    #[test]
    fn warn_unknown_rule_ids_is_silent_for_empty_list() {
        // `include`/`exclude` set to an empty list must not warn.
        let captured = capture_warnings(|| {
            warn_unknown_rule_ids(CriticRuleIdSource::ClientSettings, "critic.include", &[])
        });
        assert!(captured.is_empty(), "empty ID list must not warn; got:\n{}", captured.join("\n"));
    }

    // ── warning is actionable: names the source and the nearest valid ID ───

    #[test]
    fn json_unknown_rule_id_warning_names_the_client_settings_source() {
        // A user reading the log has to know which of the two config channels
        // to go and edit; "critic.include" alone does not tell them.
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "include": ["native.common.typo_rule"] }
            }));
        });
        assert_warned_contains(&captured, &["LSP client settings"]);
        let combined = captured.join("\n");
        assert!(
            !combined.contains(".perl-lsp.toml"),
            "client-settings warning must not blame the project file; got:\n{combined}"
        );
    }

    #[test]
    fn toml_unknown_rule_id_warning_names_the_project_file_source() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.critic.include = Some(vec!["native.variables.typo_rule".to_string()]);
        let captured = capture_warnings(|| project.apply_to_server_config(&mut config));
        assert_warned_contains(&captured, &[".perl-lsp.toml"]);
        let combined = captured.join("\n");
        assert!(
            !combined.contains("LSP client settings"),
            "project-file warning must not blame client settings; got:\n{combined}"
        );
    }

    #[test]
    fn unknown_rule_id_warning_suggests_the_closest_valid_id_for_a_typo() {
        // One transposed/dropped letter is the overwhelmingly common mistake.
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "exclude": ["native.common.assignment_in_conditon"] }
            }));
        });
        assert_warned_contains(
            &captured,
            &["did you mean", "native.common.assignment_in_condition"],
        );
    }

    #[test]
    fn unknown_rule_id_warning_suggests_the_qualified_id_for_a_bare_rule_name() {
        // Writing the rule name without its `native.<area>.` namespace is a
        // realistic mistake that plain edit distance is far too coarse to catch.
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "include": ["unused_lexical"] }
            }));
        });
        assert_warned_contains(&captured, &["did you mean", "native.variables.unused_lexical"]);
    }

    #[test]
    fn unknown_rule_id_warning_suggests_the_canonical_casing() {
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "include": ["Native.IO.Pipe_Open"] }
            }));
        });
        assert_warned_contains(&captured, &["did you mean", "native.io.pipe_open"]);
    }

    #[test]
    fn unrelated_rule_id_gets_no_invented_suggestion() {
        // An honest "no close match" beats confidently sending the user to edit
        // the wrong rule.
        let mut config = ServerConfig::default();
        let captured = capture_warnings(|| {
            config.update_from_value(&serde_json::json!({
                "critic": { "include": ["completely.made.up.thing"] }
            }));
        });
        let combined = captured.join("\n");
        assert!(
            combined.contains("no close match"),
            "expected an explicit no-close-match warning; got:\n{combined}"
        );
        assert!(
            !combined.contains("did you mean"),
            "must not invent a suggestion for an unrelated ID; got:\n{combined}"
        );
    }

    #[test]
    fn suggest_rule_id_returns_none_when_nothing_is_close() {
        let known = crate::tooling::perl_critic::NativeCriticRegistry::for_profile(
            crate::tooling::perl_critic::NativeCriticProfile::Strict,
        )
        .rule_ids();
        // Shares the `native.` prefix but nothing else — the length-scaled
        // threshold must still reject it.
        assert_eq!(suggest_rule_id("native.common.typo_rule", &known), None);
        assert_eq!(suggest_rule_id("", &known), None);
    }

    #[test]
    fn suggest_rule_id_length_guard_matches_unguarded_distance_result() {
        // The length short-circuit must be a pure optimization. For every rule
        // ID plus a spread of near-misses and junk, the guarded result has to
        // equal what the plain threshold check alone would return.
        let known = crate::tooling::perl_critic::NativeCriticRegistry::for_profile(
            crate::tooling::perl_critic::NativeCriticProfile::Strict,
        )
        .rule_ids();

        let unguarded = |unknown: &str| -> Option<&'static str> {
            known
                .iter()
                .copied()
                .filter_map(|candidate| {
                    let distance = rule_id_edit_distance(unknown, candidate);
                    (distance <= rule_id_suggestion_threshold(unknown, candidate))
                        .then_some((candidate, distance))
                })
                .min_by_key(|(candidate, distance)| (*distance, candidate.len()))
                .map(|(candidate, _)| candidate)
        };

        let mut probes: Vec<String> = vec![
            String::new(),
            "x".to_string(),
            "native".to_string(),
            "native.common.typo_rule".to_string(),
            "completely.made.up.thing".to_string(),
            "native.common.assignment_in_conditon".to_string(),
            "native.io.pipe_opennnnn".to_string(),
        ];
        for id in &known {
            probes.push((*id).to_string());
            probes.push(format!("{id}x"));
            probes.push(id.replace('_', "-"));
            probes.push(id.chars().rev().collect());
        }

        for probe in &probes {
            // Only the edit-distance pass is under test, so compare against the
            // unguarded form of that same pass.
            let guarded = known
                .iter()
                .copied()
                .filter_map(|candidate| {
                    let threshold = rule_id_suggestion_threshold(probe, candidate);
                    if probe.len().abs_diff(candidate.len()) > threshold {
                        return None;
                    }
                    let distance = rule_id_edit_distance(probe, candidate);
                    (distance <= threshold).then_some((candidate, distance))
                })
                .min_by_key(|(candidate, distance)| (*distance, candidate.len()))
                .map(|(candidate, _)| candidate);
            assert_eq!(guarded, unguarded(probe), "length guard changed the result for {probe:?}");
        }
    }

    #[test]
    fn suggest_rule_id_stays_bounded_for_a_pathological_entry() {
        // `.perl-lsp.toml` is workspace-controlled, so a hostile or simply
        // broken project file must not be able to stall config application in
        // the edit-distance pass.
        let known = crate::tooling::perl_critic::NativeCriticRegistry::for_profile(
            crate::tooling::perl_critic::NativeCriticProfile::Strict,
        )
        .rule_ids();
        let huge = "a".repeat(2_000_000);
        let started = std::time::Instant::now();
        assert_eq!(suggest_rule_id(&huge, &known), None);
        // Generous bound — the point is that it is bounded at all. Without the
        // length guard this runs ~28 full 2M-cell Levenshtein matrices.
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "suggestion pass must short-circuit on absurd input; took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn suggest_rule_id_prefers_the_nearest_candidate() {
        let known = crate::tooling::perl_critic::NativeCriticRegistry::for_profile(
            crate::tooling::perl_critic::NativeCriticProfile::Strict,
        )
        .rule_ids();
        // `unused_lexical` and `unused_parameter` are both near; the exact leaf
        // match must win rather than whichever the edit-distance pass reaches.
        assert_eq!(
            suggest_rule_id("native.vars.unused_parameter", &known),
            Some("native.variables.unused_parameter")
        );
    }

    #[test]
    fn include_path_with_parent_dir_rejected_when_no_workspace_root() {
        // #5345: when no workspace root is configured (single-file mode),
        // relative paths with `..` must be rejected fail-closed instead of
        // passing through unvalidated.
        let result = validate_resource_include_path_entry("../../etc/passwd", None);
        assert!(
            matches!(result, Err(RejectedClientIncludePathReason::EscapesWorkspace(_))),
            "relative path with '..' must be rejected when no workspace root is set, got: {result:?}"
        );
    }

    #[test]
    fn include_path_without_parent_dir_accepted_when_no_workspace_root() {
        // A simple relative path like "lib" is safe even without a workspace
        // root — it has no `..` component to escape.
        let result = validate_resource_include_path_entry("lib", None);
        assert!(result.is_ok(), "simple relative path must be accepted, got: {result:?}");
    }
}
