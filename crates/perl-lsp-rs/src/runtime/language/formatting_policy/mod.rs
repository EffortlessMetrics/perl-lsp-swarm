//! Shared runtime policy for every formatting request surface.
//!
//! The formatter libraries decide whether a result was applied, unchanged,
//! refused, or not proven. This module admits one current source/configuration
//! snapshot, binds cancellation, and projects that typed result onto LSP.
//! Multi-range plan geometry and live `textDocument/rangesFormatting`
//! wiring share this module; compose unit-test packet remains a follow-up.

use super::super::{
    GLOBAL_CANCELLATION_REGISTRY, INVALID_REQUEST, JsonRpcError, JsonRpcId, LspServer,
    PerlLspCancellationToken, Value, json,
};
use crate::cancellation::RequestCleanupGuard;
use crate::convert::{WirePosition, WireRange};
use crate::features::formatting::{
    CodeFormatter, FormatContext, FormatTextEdit, FormattingDecision, FormattingError,
    FormattingOptions, PerlTidyConfig,
};
use crate::protocol::{CONTENT_MODIFIED, REQUEST_CANCELLED, invalid_params, req_position, req_uri};
use perl_lsp_rs_core::config::FormatterMode;
use perl_lsp_rs_core::features::ids::{
    LSP_FORMATTING, LSP_ON_TYPE_FORMATTING, LSP_RANGE_FORMATTING, LSP_RANGES_FORMATTING,
};
use perl_lsp_rs_core::tooling::perltidy::native::FormatDisposition;
use serde::Serialize;

const PROVIDER: &str = "formatting";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    Document,
    Range,
    Ranges,
    OnType,
}

impl Surface {
    const fn method(self) -> &'static str {
        match self {
            Self::Document => "textDocument/formatting",
            Self::Range => "textDocument/rangeFormatting",
            Self::Ranges => "textDocument/rangesFormatting",
            Self::OnType => "textDocument/onTypeFormatting",
        }
    }

    const fn feature_id(self) -> &'static str {
        match self {
            Self::Document => LSP_FORMATTING,
            Self::Range => LSP_RANGE_FORMATTING,
            Self::Ranges => LSP_RANGES_FORMATTING,
            Self::OnType => LSP_ON_TYPE_FORMATTING,
        }
    }
}

#[derive(Debug, Clone)]
struct EffectiveConfig {
    configured_enabled: bool,
    configured_mode: FormatterMode,
    mode: FormatterMode,
    perltidy: PerlTidyConfig,
    fingerprint: String,
}

#[derive(Serialize)]
struct ConfigIdentity<'a> {
    configured_enabled: bool,
    configured_mode: FormatterMode,
    mode: FormatterMode,
    perltidy: &'a PerlTidyConfig,
}

#[derive(Debug, Clone)]
struct Snapshot {
    surface: Surface,
    uri: String,
    uri_hash: String,
    text: String,
    version: i32,
    generation: u64,
    options: FormattingOptions,
    config: EffectiveConfig,
}

fn digest(text: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn value<T: Serialize>(item: &T) -> Value {
    serde_json::to_value(item).unwrap_or_else(|error| {
        json!({
            "serialization_error": error.to_string(),
        })
    })
}

fn default_options() -> FormattingOptions {
    FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    }
}

fn options(params: &Value) -> Result<FormattingOptions, JsonRpcError> {
    match params.get("options") {
        None => Ok(default_options()),
        Some(options) => serde_json::from_value(options.clone())
            .map_err(|_| invalid_params("Invalid formatting options")),
    }
}

fn request_version(params: &Value) -> Option<i32> {
    params["textDocument"]["version"].as_i64().and_then(|number| i32::try_from(number).ok())
}

fn parse_range(value: &Value, label: &str) -> Result<WireRange, JsonRpcError> {
    let field = |pointer: &str, name: &str| -> Result<u32, JsonRpcError> {
        let raw = value
            .pointer(pointer)
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid_params(&format!("Missing {label}.{name}")))?;
        u32::try_from(raw).map_err(|_| invalid_params(&format!("{label}.{name} exceeds u32::MAX")))
    };

    Ok(WireRange::new(
        WirePosition::new(
            field("/start/line", "start.line")?,
            field("/start/character", "start.character")?,
        ),
        WirePosition::new(
            field("/end/line", "end.line")?,
            field("/end/character", "end.character")?,
        ),
    ))
}

fn document_not_open(uri: &str) -> JsonRpcError {
    JsonRpcError { code: INVALID_REQUEST, message: format!("Document not open: {uri}"), data: None }
}

fn formatting_error_reason(error: &FormattingError) -> Value {
    match error {
        FormattingError::NativeNotProven(reason) => value(reason),
        FormattingError::PerltidyNotFound(_) => json!("perltidy_not_found"),
        FormattingError::PerltidyError(_) => json!("perltidy_error"),
        FormattingError::InvalidOutputEncoding => json!("invalid_output_encoding"),
        FormattingError::IoError(_) => json!("io_error"),
    }
}

fn actual_engine_for_mode(mode: FormatterMode) -> &'static str {
    match mode {
        FormatterMode::Native | FormatterMode::Compat => "native",
        FormatterMode::ExternalLegacy => "external_legacy",
        FormatterMode::Off => "disabled",
    }
}

fn cancellation_token(
    id: Option<&JsonRpcId>,
    surface: Surface,
) -> Option<PerlLspCancellationToken> {
    let id = id?;
    if let Some(token) = GLOBAL_CANCELLATION_REGISTRY.get_token(id) {
        return Some(token);
    }
    let token = PerlLspCancellationToken::new(id.clone(), surface.method().to_string());
    let _ = GLOBAL_CANCELLATION_REGISTRY.register_token(token.clone());
    Some(token)
}

fn sanitized_outcome(decision: &FormattingDecision) -> Value {
    let mut outcome = value(&decision.outcome);
    if let Some(identity) = outcome.get_mut("identity").and_then(Value::as_object_mut)
        && let Some(Value::String(source_id)) = identity.remove("source_id")
    {
        identity.insert("source_id_hash".to_string(), json!(digest(&source_id)));
    }
    outcome
}

impl LspServer {
    fn surface_advertised(&self, surface: Surface) -> bool {
        let ids = self.advertised_feature_ids.lock();
        if !ids.is_empty() {
            return ids.contains(&surface.feature_id());
        }
        drop(ids);

        let advertised = self.advertised_features.lock();
        match surface {
            Surface::Document | Surface::OnType => advertised.formatting,
            Surface::Range | Surface::Ranges => advertised.range_formatting,
        }
    }

    /// Fail closed with method-not-advertised before parameter validation.
    fn ensure_surface_advertised(&self, surface: Surface) -> Result<(), JsonRpcError> {
        if self.surface_advertised(surface) {
            return Ok(());
        }
        Err(crate::protocol::method_not_advertised())
    }

    fn effective_formatting_config(&self) -> Result<EffectiveConfig, JsonRpcError> {
        let discovered_profile = self.discovered_perltidy_profile.lock().clone();
        let config = self.config.lock();
        let configured_enabled = config.perltidy_enabled;
        let configured_mode = config.formatting_engine;
        let mode = if configured_enabled { configured_mode } else { FormatterMode::Off };
        let perltidy = PerlTidyConfig {
            maximum_line_length: config.perltidy_maximum_line_length,
            indent_columns: config.perltidy_indent_columns,
            tabs: config.perltidy_tabs,
            opening_brace_on_new_line: config.perltidy_opening_brace_on_new_line,
            cuddled_else: config.perltidy_cuddled_else,
            space_after_keyword: config.perltidy_space_after_keyword,
            add_trailing_commas: config.perltidy_add_trailing_commas,
            vertical_alignment: config.perltidy_vertical_alignment,
            block_comment_indentation: config.perltidy_block_comment_indentation,
            profile: config.perltidy_profile.clone().or(discovered_profile),
            extra_args: config.perltidy_extra_args.clone(),
            timeout_secs: config.perltidy_timeout_secs,
        };
        let identity = serde_json::to_string(&ConfigIdentity {
            configured_enabled,
            configured_mode,
            mode,
            perltidy: &perltidy,
        })
        .map_err(|error| JsonRpcError {
            code: -32603,
            message: format!("Failed to capture formatting configuration: {error}"),
            data: Some(json!({
                "error_kind": "invalid_configuration",
                "reason": "invalid_configuration",
            })),
        })?;

        Ok(EffectiveConfig {
            configured_enabled,
            configured_mode,
            mode,
            perltidy,
            fingerprint: digest(&identity),
        })
    }

    fn admit(&self, surface: Surface, params: &Value) -> Result<Snapshot, JsonRpcError> {
        self.ensure_surface_advertised(surface)?;

        let uri = req_uri(params)?.to_string();
        let (text, version, generation) = {
            let documents = self.documents_guard();
            let document =
                self.get_document(&documents, &uri).ok_or_else(|| document_not_open(&uri))?;
            (
                document.text_arc.to_string(),
                document.version,
                u64::from(document.current_generation()),
            )
        };
        let snapshot = Snapshot {
            surface,
            uri_hash: digest(&uri),
            uri,
            text,
            version,
            generation,
            options: options(params)?,
            config: self.effective_formatting_config()?,
        };

        if request_version(params).is_some_and(|requested| requested != snapshot.version) {
            return Err(self.stale_error(
                &snapshot,
                "stale_source",
                "Formatting request version is stale.",
            ));
        }
        Ok(snapshot)
    }

    fn ensure_current(&self, snapshot: &Snapshot) -> Result<(), JsonRpcError> {
        let current = {
            let documents = self.documents_guard();
            self.get_document(&documents, &snapshot.uri).is_some_and(|document| {
                document.version == snapshot.version
                    && u64::from(document.current_generation()) == snapshot.generation
            })
        };
        if !current {
            return Err(self.stale_error(
                snapshot,
                "stale_source",
                "Document changed while formatting was running; no edits were returned.",
            ));
        }
        if self.effective_formatting_config()?.fingerprint != snapshot.config.fingerprint {
            return Err(self.stale_error(
                snapshot,
                "stale_configuration",
                "Formatting configuration changed while the request was running; no edits were returned.",
            ));
        }
        Ok(())
    }
}

mod handlers;
mod receipt;

#[cfg(test)]
mod tests;
