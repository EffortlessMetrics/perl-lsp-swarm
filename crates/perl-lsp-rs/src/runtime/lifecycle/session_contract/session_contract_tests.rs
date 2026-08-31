//! Model-level proof for the immutable text-sync session contract (#9378).
//!
//! Covers the closed offer classification, selection reasons, typed
//! malformed-input rejection, digests, evidence projection, and the
//! response/contract divergence check. Runtime-seam proof lives in
//! `runtime/lifecycle/capabilities.rs` tests and the lifecycle integration
//! tests.

use serde_json::json;

use super::{
    AcceptedPositionEncoding, AcceptedSyncKind, OfferedPositionEncoding, PositionEncodingOffer,
    SessionContractRejection, TextSyncSessionContract, Utf16SelectionReason,
    classify_position_encoding_offer, verify_response_matches_contract,
};

fn accepted_with_offer(offer: serde_json::Value) -> TextSyncSessionContract {
    let params = json!({ "capabilities": { "general": { "positionEncodings": offer } } });
    TextSyncSessionContract::accept(Some(&params), "s-1".to_string())
        .unwrap_or_else(|rejection| unreachable!("valid string-list offer must be accepted: {rejection:?}"))
}

#[test]
fn absent_offer_selects_utf16_by_protocol_default() {
    let classified = classify_position_encoding_offer(None)
        .unwrap_or_else(|rejection| unreachable!("absent offer must classify: {rejection:?}"));
    assert_eq!(classified, PositionEncodingOffer::Absent);

    let contract = TextSyncSessionContract::accept(None, "s-1".to_string())
        .unwrap_or_else(|rejection| unreachable!("absent offer must be accepted: {rejection:?}"));
    assert_eq!(contract.sync_kind(), AcceptedSyncKind::Full);
    assert_eq!(contract.position_encoding(), AcceptedPositionEncoding::Utf16);
    assert_eq!(contract.selection_reason(), Utf16SelectionReason::OfferAbsent);
}

#[test]
fn json_null_offer_is_recorded_distinctly_from_absence() {
    let params = json!({ "capabilities": { "general": { "positionEncodings": null } } });
    let contract = TextSyncSessionContract::accept(Some(&params), "s-2".to_string())
        .unwrap_or_else(|rejection| unreachable!("null offer must be accepted: {rejection:?}"));
    assert_eq!(contract.client_offer(), &PositionEncodingOffer::Null);
    assert_eq!(contract.selection_reason(), Utf16SelectionReason::OfferAbsent);
}

#[test]
fn valid_offer_matrix_always_selects_full_utf16() {
    for (offer, expected_reason) in [
        (json!(["utf-16"]), Utf16SelectionReason::ClientOfferedUtf16),
        (json!(["utf-8", "utf-16"]), Utf16SelectionReason::ClientOfferedUtf16),
        (json!(["utf-32", "utf-16"]), Utf16SelectionReason::ClientOfferedUtf16),
        (json!(["utf-16", "utf-16"]), Utf16SelectionReason::ClientOfferedUtf16),
        (json!(["utf-7", "utf-16"]), Utf16SelectionReason::ClientOfferedUtf16),
        (json!(["utf-8"]), Utf16SelectionReason::MandatoryUtf16Fallback),
        (json!(["utf-32"]), Utf16SelectionReason::MandatoryUtf16Fallback),
        (json!(["utf-32", "utf-7"]), Utf16SelectionReason::MandatoryUtf16Fallback),
        (json!(["future-encoding"]), Utf16SelectionReason::MandatoryUtf16Fallback),
    ] {
        let contract = accepted_with_offer(offer.clone());
        assert_eq!(
            contract.position_encoding(),
            AcceptedPositionEncoding::Utf16,
            "offer {offer} must select utf-16"
        );
        assert_eq!(contract.sync_kind(), AcceptedSyncKind::Full);
        assert_eq!(
            contract.selection_reason(),
            expected_reason,
            "offer {offer} must retain the correct selection reason"
        );
    }
}

#[test]
fn unknown_entries_are_retained_without_changing_mandatory_selection() {
    let contract = accepted_with_offer(json!(["utf-7", "utf-32"]));
    let PositionEncodingOffer::Present(receipt) = contract.client_offer() else {
        unreachable!("present offer must be retained");
    };
    assert_eq!(receipt.total_entries, 2);
    assert_eq!(receipt.entries.len(), 2);
    assert_eq!(receipt.entries[0].recognized, None);
    assert_eq!(receipt.entries[1].recognized, Some(OfferedPositionEncoding::Utf32));
    assert_eq!(contract.selection_reason(), Utf16SelectionReason::MandatoryUtf16Fallback);
}

#[test]
fn empty_offer_has_its_own_explicit_disposition() {
    let contract = accepted_with_offer(json!([]));
    assert_eq!(contract.selection_reason(), Utf16SelectionReason::OfferEmpty);
    assert_eq!(contract.position_encoding(), AcceptedPositionEncoding::Utf16);
}

#[test]
fn malformed_offers_fail_typed() {
    for offer in [json!("utf-16"), json!(42), json!({}), json!(["utf-16", 42]), json!([true])] {
        let params = json!({ "capabilities": { "general": { "positionEncodings": offer } } });
        let rejection = TextSyncSessionContract::accept(Some(&params), "s-4".to_string())
            .err()
            .unwrap_or_else(|| unreachable!("malformed offer must fail: {offer}"));
        assert!(
            matches!(rejection, SessionContractRejection::MalformedOffer { .. }),
            "offer {offer} must fail as malformed, got {rejection:?}"
        );
    }
}

#[test]
fn malformed_rejection_maps_to_typed_invalid_params_error() {
    let params = json!({ "capabilities": { "general": { "positionEncodings": ["utf-16", 7] } } });
    let rejection = TextSyncSessionContract::accept(Some(&params), "s-5".to_string())
        .err()
        .unwrap_or_else(|| unreachable!("malformed offer must fail"));
    let error = rejection.to_jsonrpc_error();
    assert_eq!(error.code, -32602, "malformed offer must be typed InvalidParams");
    let data = error.data.unwrap_or_else(|| unreachable!("rejection error must carry data"));
    assert_eq!(
        data.pointer("/schema").and_then(serde_json::Value::as_str),
        Some(super::TEXT_SYNC_CONTRACT_SCHEMA)
    );
    assert_eq!(
        data.pointer("/rejection/reason").and_then(serde_json::Value::as_str),
        Some("malformed-offer")
    );
}

#[test]
fn response_must_match_the_accepted_contract() {
    let contract = accepted_with_offer(json!(["utf-8"]));
    let agreeing = json!({
        "capabilities": {
            "positionEncoding": "utf-16",
            "textDocumentSync": { "openClose": true, "change": 1 }
        }
    });
    assert!(verify_response_matches_contract(&contract, &agreeing).is_ok());

    let diverged_encoding = json!({
        "capabilities": {
            "positionEncoding": "utf-8",
            "textDocumentSync": { "change": 1 }
        }
    });
    let error = verify_response_matches_contract(&contract, &diverged_encoding)
        .err()
        .unwrap_or_else(|| unreachable!("encoding divergence must fail"));
    assert_eq!(error.code, -32603, "divergence is an internal-invariant failure");

    let diverged_sync = json!({
        "capabilities": {
            "positionEncoding": "utf-16",
            "textDocumentSync": { "change": 2 }
        }
    });
    assert!(
        verify_response_matches_contract(&contract, &diverged_sync).is_err(),
        "incremental sync advertisement must fail against the accepted contract"
    );
}

#[test]
fn evidence_projection_agrees_with_contract_and_response_digest() {
    let contract = accepted_with_offer(json!(["utf-32"]));
    let session = super::AcceptedTextSyncSession::new(contract.clone(), "resp-digest".to_string());
    let evidence = session.evidence();
    let serialized = serde_json::to_string(&evidence)
        .unwrap_or_else(|error| unreachable!("evidence must serialize: {error}"));
    for needle in [
        "\"sync-kind\":\"full\"",
        "\"position-encoding\":\"utf-16\"",
        "\"offer-class\":\"present\"",
        "\"selection-reason\":\"mandatory-utf16-fallback\"",
        "\"contract-digest\":",
        "\"response-digest\":\"resp-digest\"",
        "\"terminal-outcome\":\"accepted\"",
    ] {
        assert!(serialized.contains(needle), "evidence must contain {needle}: {serialized}");
    }
    assert_eq!(evidence.contract_digest, contract.digest());
}

#[test]
fn contract_digest_is_stable_and_identity_sensitive() {
    let first = accepted_with_offer(json!(["utf-8"]));
    let second = accepted_with_offer(json!(["utf-8"]));
    assert_eq!(first.digest(), second.digest(), "same inputs must give one digest");

    let different_session = {
        let params = json!({ "capabilities": { "general": { "positionEncodings": ["utf-8"] } } });
        TextSyncSessionContract::accept(Some(&params), "s-other".to_string())
            .unwrap_or_else(|rejection| unreachable!("offer must be accepted: {rejection:?}"))
    };
    assert_ne!(
        first.digest(),
        different_session.digest(),
        "session identity must be part of the digest"
    );
}

#[test]
fn accepted_values_are_the_only_constructible_contract_members() {
    assert_eq!(AcceptedSyncKind::Full.wire_value(), 1);
    assert_eq!(AcceptedSyncKind::Full.token(), "full");
    assert_eq!(AcceptedPositionEncoding::Utf16.wire_name(), "utf-16");
    assert_eq!(AcceptedPositionEncoding::Utf16.token(), "utf-16");
}
