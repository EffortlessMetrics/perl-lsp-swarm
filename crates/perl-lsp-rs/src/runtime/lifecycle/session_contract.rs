//! Immutable text-sync session contract (#9378).
//!
//! One typed authority owns the accepted initialize/session wire contract:
//! `sync_kind = full` and `position_encoding = utf-16` are accepted
//! together, derived from a closed classification of the client's
//! `general.positionEncodings` offer. The `InitializeResult`, the stored
//! session state, and the bounded evidence projection are all built from
//! the same accepted value, so they cannot diverge silently.
//!
//! Branch authority: #8129 selected `full_document_utf16` for the v0.18
//! release envelope. Negotiated UTF-8, incremental sync, and provider
//! migration are NOT claimed here (#9380/#9383/#9386 own those leaves).

use serde::Serialize;
use serde_json::{Value, json};

use crate::protocol::JsonRpcError;
use perl_lsp_rs_core::protocol::{INTERNAL_ERROR, INVALID_PARAMS};

/// Schema marker carried by the contract and every bounded projection.
pub(crate) const TEXT_SYNC_CONTRACT_SCHEMA: &str = "perl-lsp.text-sync-session/1";

/// Maximum offer entries retained in the bounded receipt. Selection always
/// scans the complete offer; only the retained receipt is capped.
const OFFER_RECEIPT_CAP: usize = 16;

/// Maximum retained client-name length in the session identity.
const CLIENT_NAME_CAP: usize = 128;

/// Wire position encodings a client can name (LSP 3.17+ PositionEncodingKind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum OfferedPositionEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-16")]
    Utf16,
    #[serde(rename = "utf-32")]
    Utf32,
}

impl OfferedPositionEncoding {
    fn parse(entry: &str) -> Option<Self> {
        match entry {
            "utf-8" => Some(Self::Utf8),
            "utf-16" => Some(Self::Utf16),
            "utf-32" => Some(Self::Utf32),
            _ => None,
        }
    }
}

/// One retained offer entry: the bounded raw string plus whether it named a
/// known LSP position encoding. Unknown entries are retained on purpose —
/// the receipt records what was offered, not just what was understood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct OfferEntryReceipt {
    entry: String,
    recognized: Option<OfferedPositionEncoding>,
}

/// Bounded receipt of a present offer. `contains_utf16` reflects the FULL
/// offer (including entries beyond the retained cap).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct OfferEntriesReceipt {
    entries: Vec<OfferEntryReceipt>,
    total_entries: usize,
    contains_utf16: bool,
}

impl OfferEntriesReceipt {
    fn from_raw_list(list: &[Value]) -> Result<Self, SessionContractRejection> {
        let mut entries = Vec::new();
        for (index, entry) in list.iter().enumerate() {
            let Some(text) = entry.as_str() else {
                return Err(SessionContractRejection::MalformedOffer {
                    detail: format!(
                        "general.positionEncodings entry {index} must be a string, got {}",
                        json_type_name(entry)
                    ),
                });
            };
            if entries.len() < OFFER_RECEIPT_CAP {
                entries.push(OfferEntryReceipt {
                    entry: bound_text(text, CLIENT_NAME_CAP),
                    recognized: OfferedPositionEncoding::parse(text),
                });
            }
        }
        Ok(Self {
            total_entries: list.len(),
            contains_utf16: list.iter().filter_map(Value::as_str).any(|s| s == "utf-16"),
            entries,
        })
    }

    fn retained_entries(&self) -> Vec<String> {
        self.entries.iter().map(|receipt| receipt.entry.clone()).collect()
    }
}

/// Closed classification of the client's `general.positionEncodings` offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "offer_class")]
pub(crate) enum PositionEncodingOffer {
    /// Key absent from the initialize params.
    Absent,
    /// Key present with JSON `null` — the absent spelling for an optional
    /// array, recorded distinctly from a malformed value.
    Null,
    /// Present array (possibly empty).
    Present(OfferEntriesReceipt),
}

/// Why the accepted contract selected UTF-16. Every accepted session names
/// exactly one reason; there is no unreasoned selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Utf16SelectionReason {
    /// No `general.positionEncodings` offer (absent key or JSON null).
    OfferAbsent,
    /// Present but empty list — no constraint expressed; protocol default.
    OfferEmpty,
    /// The offer contained `utf-16`.
    ClientOfferedUtf16,
}

/// Typed initialize failure for offers the v0.18 envelope cannot accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "reason")]
pub(crate) enum SessionContractRejection {
    /// Present nonempty offer without `utf-16`. Fail-closed: a client that
    /// explicitly excludes UTF-16 is unsupported on this branch.
    NoCommonEncoding {
        /// Bounded view of what was offered.
        offered: Vec<String>,
    },
    /// Offer present but not a string array.
    MalformedOffer {
        /// What was wrong with the value.
        detail: String,
    },
}

impl SessionContractRejection {
    /// Typed -32602 initialize failure carrying the bounded classification.
    pub(crate) fn to_jsonrpc_error(&self) -> JsonRpcError {
        let message = match self {
            Self::NoCommonEncoding { .. } => {
                "client position encoding offer does not include the required utf-16 encoding"
            }
            Self::MalformedOffer { .. } => {
                "general.positionEncodings must be an array of position encoding strings"
            }
        };
        JsonRpcError::with_data(
            INVALID_PARAMS,
            message,
            json!({
                "schema": TEXT_SYNC_CONTRACT_SCHEMA,
                "rejection": self,
            }),
        )
    }
}

/// Closed accepted sync kind. `Full` is the only constructible value; no
/// caller can advertise incremental sync through this contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AcceptedSyncKind {
    Full,
}

impl AcceptedSyncKind {
    /// `TextDocumentSyncKind::FULL` on the wire.
    pub(crate) const fn wire_value(self) -> i32 {
        1
    }

    /// Canonical token for evidence projections.
    pub(crate) const fn token(self) -> &'static str {
        "full"
    }
}

/// Closed accepted wire position encoding. `Utf16` is the only constructible
/// value; the contract cannot hold a divergent selected encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AcceptedPositionEncoding {
    /// Serialized as the exact LSP wire value.
    #[serde(rename = "utf-16")]
    Utf16,
}

impl AcceptedPositionEncoding {
    /// Exact `InitializeResult.positionEncoding` wire value.
    pub(crate) const fn wire_name(self) -> &'static str {
        "utf-16"
    }

    /// Canonical token for evidence projections.
    pub(crate) const fn token(self) -> &'static str {
        "utf-16"
    }
}

/// Bounded session/run identity retained with the accepted contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct TextSyncSessionIdentity {
    session_id: String,
    process_id: Option<i64>,
    client_name: Option<String>,
    protocol_version: &'static str,
}

/// The accepted initialize/session wire contract. Constructed exactly once
/// per process through [`TextSyncSessionContract::accept`]; fields are
/// private and there are no setters, so neither `FULL`, `Utf16`, nor any
/// other encoding can be set independently after acceptance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct TextSyncSessionContract {
    schema: &'static str,
    identity: TextSyncSessionIdentity,
    sync_kind: AcceptedSyncKind,
    position_encoding: AcceptedPositionEncoding,
    client_offer: PositionEncodingOffer,
    selection_reason: Utf16SelectionReason,
}

impl TextSyncSessionContract {
    /// Classify the client offer and construct the complete contract
    /// candidate. Pure with respect to server state: a rejection here means
    /// initialize fails before any capability/session mutation.
    pub(crate) fn accept(
        params: Option<&Value>,
        session_id: String,
    ) -> Result<Self, SessionContractRejection> {
        let client_offer = classify_position_encoding_offer(params)?;
        let selection_reason = match &client_offer {
            PositionEncodingOffer::Absent | PositionEncodingOffer::Null => {
                Utf16SelectionReason::OfferAbsent
            }
            PositionEncodingOffer::Present(receipt) => {
                if receipt.total_entries == 0 {
                    Utf16SelectionReason::OfferEmpty
                } else if receipt.contains_utf16 {
                    Utf16SelectionReason::ClientOfferedUtf16
                } else {
                    // Present nonempty without utf-16 — the v0.18 envelope is
                    // fail-closed (#8129 branch `full_document_utf16`): a
                    // client that explicitly excludes UTF-16 is unsupported,
                    // and silence here would advertise an encoding the
                    // client cannot parse. Red-then-green: this branch was
                    // flipped from the old main fallback after the focused
                    // runtime gate observed the fallback RED.
                    return Err(SessionContractRejection::NoCommonEncoding {
                        offered: receipt.retained_entries(),
                    });
                }
            }
        };
        Ok(Self {
            schema: TEXT_SYNC_CONTRACT_SCHEMA,
            identity: session_identity(params, session_id),
            sync_kind: AcceptedSyncKind::Full,
            position_encoding: AcceptedPositionEncoding::Utf16,
            client_offer,
            selection_reason,
        })
    }

    pub(crate) const fn schema(&self) -> &'static str {
        TEXT_SYNC_CONTRACT_SCHEMA
    }

    pub(crate) const fn sync_kind(&self) -> AcceptedSyncKind {
        self.sync_kind
    }

    pub(crate) const fn position_encoding(&self) -> AcceptedPositionEncoding {
        self.position_encoding
    }

    pub(crate) const fn selection_reason(&self) -> Utf16SelectionReason {
        self.selection_reason
    }

    pub(crate) const fn client_offer(&self) -> &PositionEncodingOffer {
        &self.client_offer
    }

    pub(crate) const fn identity(&self) -> &TextSyncSessionIdentity {
        &self.identity
    }

    /// Stable digest over the canonical contract form. Detects any stored
    /// contract change between acceptance and evidence consumption.
    pub(crate) fn digest(&self) -> String {
        digest_bytes(self.canonical_form().as_bytes())
    }

    fn canonical_form(&self) -> String {
        let offer = match &self.client_offer {
            PositionEncodingOffer::Absent => "absent".to_string(),
            PositionEncodingOffer::Null => "null".to_string(),
            PositionEncodingOffer::Present(receipt) => {
                let entries = receipt
                    .entries
                    .iter()
                    .map(|entry| entry.entry.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "present(total={},utf16={},[{}])",
                    receipt.total_entries, receipt.contains_utf16, entries
                )
            }
        };
        let identity = &self.identity;
        format!(
            "v1|session={}|process={:?}|client={:?}|protocol={}|sync={}|encoding={}|offer={}|reason={:?}",
            identity.session_id,
            identity.process_id,
            identity.client_name,
            identity.protocol_version,
            self.sync_kind.token(),
            self.position_encoding.token(),
            offer,
            self.selection_reason,
        )
    }
}

/// A classify-only view used by the initialize transaction before any state
/// mutation. Kept separate from [`TextSyncSessionContract::accept`] so the
/// failure classification is the single source for both the typed error and
/// the accepted contract.
pub(crate) fn classify_position_encoding_offer(
    params: Option<&Value>,
) -> Result<PositionEncodingOffer, SessionContractRejection> {
    let Some(raw) =
        params.and_then(|params| params.pointer("/capabilities/general/positionEncodings"))
    else {
        return Ok(PositionEncodingOffer::Absent);
    };
    if raw.is_null() {
        return Ok(PositionEncodingOffer::Null);
    }
    let Some(list) = raw.as_array() else {
        return Err(SessionContractRejection::MalformedOffer {
            detail: format!(
                "general.positionEncodings must be an array of strings, got {}",
                json_type_name(raw)
            ),
        });
    };
    Ok(PositionEncodingOffer::Present(OfferEntriesReceipt::from_raw_list(list)?))
}

/// Accepted session: the immutable contract plus the digest of the exact
/// `InitializeResult` built from it. Stored together so response/state
/// agreement is checkable after acceptance.
#[derive(Debug, Clone)]
pub(crate) struct AcceptedTextSyncSession {
    contract: std::sync::Arc<TextSyncSessionContract>,
    response_digest: String,
}

impl AcceptedTextSyncSession {
    pub(crate) fn new(contract: TextSyncSessionContract, response_digest: String) -> Self {
        Self { contract: std::sync::Arc::new(contract), response_digest }
    }

    pub(crate) fn contract(&self) -> &TextSyncSessionContract {
        &self.contract
    }

    /// Bounded doctor/receipt projection. The evidence proves negotiation
    /// and response/state agreement only — it proves nothing about document
    /// mutation, provider conversion, process behavior, or installed editor
    /// support.
    pub(crate) fn evidence(&self) -> TextSyncSessionEvidence {
        let contract = self.contract();
        let identity = contract.identity();
        let (offer_class, offered_entries, offered_entries_total) = match contract.client_offer() {
            PositionEncodingOffer::Absent => ("absent", Vec::new(), 0usize),
            PositionEncodingOffer::Null => ("null", Vec::new(), 0usize),
            PositionEncodingOffer::Present(receipt) => {
                ("present", receipt.retained_entries(), receipt.total_entries)
            }
        };
        TextSyncSessionEvidence {
            schema: contract.schema(),
            session_id: identity.session_id.clone(),
            process_id: identity.process_id,
            client_name: identity.client_name.clone(),
            protocol_version: identity.protocol_version,
            sync_kind: contract.sync_kind(),
            position_encoding: contract.position_encoding(),
            offer_class,
            offered_entries,
            offered_entries_total,
            selection_reason: contract.selection_reason(),
            contract_digest: contract.digest(),
            response_digest: self.response_digest.clone(),
            terminal_outcome: "accepted",
        }
    }
}

/// Bounded initialize/doctor evidence projection (LSP-FS16-010).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct TextSyncSessionEvidence {
    schema: &'static str,
    session_id: String,
    process_id: Option<i64>,
    client_name: Option<String>,
    protocol_version: &'static str,
    sync_kind: AcceptedSyncKind,
    position_encoding: AcceptedPositionEncoding,
    offer_class: &'static str,
    offered_entries: Vec<String>,
    offered_entries_total: usize,
    selection_reason: Utf16SelectionReason,
    /// Digest of the stored contract; pub(crate) so runtime-seam tests can
    /// assert response/state agreement without widening the projection API.
    pub(crate) contract_digest: String,
    /// Digest of the exact published InitializeResult.
    pub(crate) response_digest: String,
    terminal_outcome: &'static str,
}

/// Typed initialize failure when the built `InitializeResult` diverges from
/// the accepted contract. This is an internal-invariant failure (-32603),
/// not a client error: response and stored state must derive from one value.
pub(crate) fn verify_response_matches_contract(
    contract: &TextSyncSessionContract,
    result: &Value,
) -> Result<(), JsonRpcError> {
    let advertised_encoding =
        result.pointer("/capabilities/positionEncoding").and_then(Value::as_str);
    let advertised_sync =
        result.pointer("/capabilities/textDocumentSync/change").and_then(Value::as_i64);
    let expected_encoding = contract.position_encoding().wire_name();
    let expected_sync = i64::from(contract.sync_kind().wire_value());
    if advertised_encoding != Some(expected_encoding) || advertised_sync != Some(expected_sync) {
        return Err(JsonRpcError::with_data(
            INTERNAL_ERROR,
            "initialize response diverges from the accepted text-sync session contract",
            json!({
                "schema": TEXT_SYNC_CONTRACT_SCHEMA,
                "expected": {
                    "positionEncoding": expected_encoding,
                    "textDocumentSync.change": expected_sync,
                },
                "advertised": {
                    "positionEncoding": advertised_encoding,
                    "textDocumentSync.change": advertised_sync,
                },
            }),
        ));
    }
    Ok(())
}

fn session_identity(params: Option<&Value>, session_id: String) -> TextSyncSessionIdentity {
    let process_id = params.and_then(|params| params.get("processId")).and_then(Value::as_i64);
    let client_name = params
        .and_then(|params| params.pointer("/clientInfo/name"))
        .and_then(Value::as_str)
        .map(|name| bound_text(name, CLIENT_NAME_CAP));
    TextSyncSessionIdentity { session_id, process_id, client_name, protocol_version: "3.18" }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn bound_text(text: &str, cap: usize) -> String {
    text.chars().take(cap).collect()
}

/// FNV-1a 64-bit over the given bytes; dependency-free bounded digest.
pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

/// Process-local session sequence. Session identity is exact per accepted
/// initialize within the process; it is not a durable or cross-process id.
pub(crate) fn next_session_id() -> String {
    static SESSION_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = SESSION_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("s-{sequence}")
}

impl super::super::LspServer {
    /// The accepted text-sync session, if initialize completed acceptance.
    pub(crate) fn accepted_text_sync_session(&self) -> Option<AcceptedTextSyncSession> {
        self.text_sync_session.lock().clone()
    }

    /// Single derived serving gate: initialize has ACCEPTED a text-sync
    /// session contract on this connection. Lifecycle completion, the
    /// router's ServerNotInitialized (-32002) arm, and the formatting
    /// intercept all consult this one predicate — the same stored-contract
    /// authority — so a consumed one-shot guard without acceptance (the
    /// failed-classification/failed-acceptance window, review 5061915323)
    /// can neither serve requests nor complete the lifecycle. Do not add an
    /// independent readiness truth beside it.
    pub(crate) fn initialization_accepted(&self) -> bool {
        self.accepted_text_sync_session().is_some()
    }

    /// Accept the session contract exactly once. A second acceptance attempt
    /// is a typed internal failure — the accepted contract is never replaced.
    pub(crate) fn accept_text_sync_session(
        &self,
        contract: TextSyncSessionContract,
        response_digest: String,
    ) -> Result<(), JsonRpcError> {
        let mut session = self.text_sync_session.lock();
        if session.is_some() {
            return Err(JsonRpcError::new(
                INTERNAL_ERROR,
                "text-sync session contract already accepted for this connection",
            ));
        }
        *session = Some(AcceptedTextSyncSession::new(contract, response_digest));
        Ok(())
    }
}

#[cfg(test)]
mod session_contract_tests;
