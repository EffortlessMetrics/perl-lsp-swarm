//! Immutable active position-encoding identity for one LSP session.
//!
//! Initialize still records a client's preferred encoding in the legacy
//! `ClientCapabilities` slot while every shipping coordinate producer and
//! consumer remains UTF-16. This module owns the separate canonical active
//! identity that generation, request, output, edit, receipt, and cache paths
//! can consume as they migrate. It deliberately contains no mapper and exposes
//! no per-document or per-provider override.

use super::super::LspServer;
use perl_position_tracking::PositionEncoding;

const SERVER_SUPPORTED_POSITION_ENCODINGS: [PositionEncoding; 2] =
    [PositionEncoding::Utf8, PositionEncoding::Utf16];

/// Why one position encoding is active for the initialized LSP session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivePositionEncodingSelectionReason {
    /// Compatibility phase: every shipping wire-coordinate path still uses UTF-16.
    CompatibilityPinnedUtf16,
}

/// Immutable coordinate identity shared by every position-bearing session path.
///
/// The encoding and its selection reason intentionally travel together. The
/// value is serializable and hashable so a cache or receipt can bind wire
/// coordinates to the active encoding without inventing another identity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub(crate) struct ActivePositionEncoding {
    encoding: PositionEncoding,
    selection_reason: ActivePositionEncodingSelectionReason,
}

impl ActivePositionEncoding {
    const COMPATIBILITY_PINNED_UTF16: Self = Self {
        encoding: PositionEncoding::Utf16,
        selection_reason: ActivePositionEncodingSelectionReason::CompatibilityPinnedUtf16,
    };

    /// Canonical encoding used for all wire coordinates in this session.
    #[must_use]
    pub(crate) const fn encoding(self) -> PositionEncoding {
        self.encoding
    }

    /// Typed reason the encoding was selected.
    #[must_use]
    pub(crate) const fn selection_reason(self) -> ActivePositionEncodingSelectionReason {
        self.selection_reason
    }
}

/// Position-encoding authority exposed by one initialized server session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PositionEncodingSessionContext {
    active: ActivePositionEncoding,
}

impl PositionEncodingSessionContext {
    const COMPATIBILITY_PINNED_UTF16: Self =
        Self { active: ActivePositionEncoding::COMPATIBILITY_PINNED_UTF16 };

    /// Encodings the server implementation can represent.
    ///
    /// Support is not activation: the active value remains independently pinned
    /// until the end-to-end cutover owned by #1690.
    #[must_use]
    pub(crate) const fn server_supported(self) -> &'static [PositionEncoding] {
        &SERVER_SUPPORTED_POSITION_ENCODINGS
    }

    /// Immutable active encoding and selection identity for this session.
    #[must_use]
    pub(crate) const fn active(self) -> ActivePositionEncoding {
        self.active
    }
}

impl LspServer {
    /// Return the active position-encoding context for a successful session.
    ///
    /// The client preference remains separate in `ClientCapabilities`; this
    /// accessor never derives the active value from that mutable compatibility
    /// record. Repeated initialize requests therefore cannot replace the active
    /// coordinate identity.
    #[must_use]
    pub(crate) fn position_encoding_session_context(
        &self,
    ) -> Option<PositionEncodingSessionContext> {
        *self.position_encoding_session_context.lock()
    }

    pub(super) fn publish_position_encoding_session_context(&self) {
        *self.position_encoding_session_context.lock() =
            Some(PositionEncodingSessionContext::COMPATIBILITY_PINNED_UTF16);
    }

    pub(super) fn clear_position_encoding_session_context(&self) {
        *self.position_encoding_session_context.lock() = None;
    }

    /// Return the active coordinate encoding.
    ///
    /// Coordinate production is not valid until initialize has published the
    /// session context. Do not silently fall back to the legacy client record
    /// or to UTF-16: doing so would hide a lifecycle violation.
    #[must_use]
    pub(crate) fn position_encoding_for_coordinates(
        &self,
    ) -> Result<PositionEncoding, crate::protocol::JsonRpcError> {
        self.position_encoding_session_context()
            .map(|context| context.active().encoding())
            .ok_or_else(|| {
                crate::protocol::JsonRpcError::new(
                    crate::protocol::INVALID_REQUEST,
                    "position encoding is unavailable before initialize or after shutdown",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textdoc::PosEnc;
    use serde_json::{Value, json};
    use std::collections::HashSet;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn initialize(server: &LspServer, params: Value) -> TestResult<Value> {
        let response =
            server.handle_initialize(Some(params))?.ok_or("initialize should return payload")?;
        Ok(response)
    }

    #[test]
    fn active_context_is_absent_before_initialize() {
        let server = LspServer::new();
        assert!(server.position_encoding_session_context().is_none());
    }

    #[test]
    fn coordinate_encoding_is_rejected_before_initialize() {
        let server = LspServer::new();
        let error = server.position_encoding_for_coordinates().unwrap_err();

        assert_eq!(error.code, crate::protocol::INVALID_REQUEST);
        assert!(error.message.contains("before initialize"));

        server.publish_position_encoding_session_context();
        server.clear_position_encoding_session_context();
        let error = server.position_encoding_for_coordinates().unwrap_err();
        assert_eq!(error.code, crate::protocol::INVALID_REQUEST);
        assert!(error.message.contains("after shutdown"));
    }

    #[test]
    fn utf8_preference_remains_distinct_from_active_utf16() -> TestResult {
        let server = LspServer::new();
        let response = initialize(
            &server,
            json!({
                "capabilities": {
                    "general": {
                        "positionEncodings": ["utf-8", "utf-16"]
                    }
                }
            }),
        )?;

        assert!(matches!(server.client_capabilities.lock().position_encoding, PosEnc::Utf8));

        let context = server
            .position_encoding_session_context()
            .ok_or("initialized session should expose an active encoding")?;
        assert_eq!(context.server_supported(), &[PositionEncoding::Utf8, PositionEncoding::Utf16]);
        assert_eq!(context.active().encoding(), PositionEncoding::Utf16);
        assert_eq!(
            context.active().selection_reason(),
            ActivePositionEncodingSelectionReason::CompatibilityPinnedUtf16
        );
        assert_eq!(
            response.pointer("/capabilities/positionEncoding").and_then(Value::as_str),
            Some("utf-16")
        );
        Ok(())
    }

    #[test]
    fn omitted_and_utf16_offers_keep_the_same_active_identity() -> TestResult {
        for params in [
            json!({ "capabilities": {} }),
            json!({
                "capabilities": {
                    "general": {
                        "positionEncodings": ["utf-16", "utf-8"]
                    }
                }
            }),
        ] {
            let server = LspServer::new();
            let response = initialize(&server, params)?;
            let context = server
                .position_encoding_session_context()
                .ok_or("initialized session should expose an active encoding")?;

            assert_eq!(context.active(), ActivePositionEncoding::COMPATIBILITY_PINNED_UTF16);
            assert_eq!(
                response.pointer("/capabilities/positionEncoding").and_then(Value::as_str),
                Some("utf-16")
            );
        }
        Ok(())
    }

    #[test]
    fn duplicate_initialize_cannot_mutate_active_identity() -> TestResult {
        let server = LspServer::new();
        let _initial_response = initialize(
            &server,
            json!({
                "capabilities": {
                    "general": {
                        "positionEncodings": ["utf-8", "utf-16"]
                    }
                }
            }),
        )?;
        let before = server
            .position_encoding_session_context()
            .ok_or("initialized session should expose an active encoding")?;

        let Err(error) = server.handle_initialize(Some(json!({
            "capabilities": {
                "general": {
                    "positionEncodings": ["utf-16"]
                }
            }
        }))) else {
            return Err("duplicate initialize should fail".into());
        };

        let after = server
            .position_encoding_session_context()
            .ok_or("active encoding should survive rejected initialize")?;
        assert_eq!(error.code, -32600);
        assert_eq!(before, after);
        assert!(matches!(server.client_capabilities.lock().position_encoding, PosEnc::Utf8));
        Ok(())
    }

    #[test]
    fn active_identity_binds_encoding_and_selection_reason() -> TestResult {
        let pinned = ActivePositionEncoding::COMPATIBILITY_PINNED_UTF16;
        assert_eq!(
            serde_json::to_value(pinned)?,
            json!({
                "encoding": "utf-16",
                "selection_reason": "compatibility_pinned_utf16"
            })
        );

        let encoding_only_utf8 = ActivePositionEncoding {
            encoding: PositionEncoding::Utf8,
            selection_reason: pinned.selection_reason(),
        };
        assert_ne!(pinned, encoding_only_utf8);

        let mut identities = HashSet::new();
        assert!(identities.insert(pinned));
        assert!(identities.insert(encoding_only_utf8));
        assert_eq!(identities.len(), 2);
        Ok(())
    }

    #[test]
    fn independent_servers_do_not_share_active_context() -> TestResult {
        let first = LspServer::new();
        let second = LspServer::new();
        initialize(&first, json!({"capabilities": {}}))?;

        assert!(first.position_encoding_session_context().is_some());
        assert!(second.position_encoding_session_context().is_none());
        Ok(())
    }

    #[test]
    fn legacy_preference_mutation_cannot_change_active_encoding() -> TestResult {
        let server = LspServer::new();
        initialize(
            &server,
            json!({
                "capabilities": {"general": {"positionEncodings": ["utf-8"]}}
            }),
        )?;
        server.client_capabilities.lock().position_encoding = PosEnc::Utf8;

        assert_eq!(server.position_encoding_for_coordinates()?, PositionEncoding::Utf16);
        Ok(())
    }
}
