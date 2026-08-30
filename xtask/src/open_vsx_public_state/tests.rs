//! Behavioral proof for the Open VSX public-state classifier.
//!
//! The negative controls carry the weight here. Each one describes a wrong
//! implementation that would look correct on the happy path — collapsing a
//! provider failure into absence, letting a stale publish record stand in for
//! current availability, or accepting bytes that were never compared — and
//! fails if this classifier drifts into it.

use super::model::{CellObservation, PublicState};
use super::test_support::{
    AVAILABLE_EXACT, DIGEST_MISMATCH, INCIDENT, INSTRUMENT_INCOMPLETE, LISTING_MISSING,
    NAMESPACE_ABSENT, RATE_LIMITED, UNPLANNED_URL, expect_blocker, expect_state, receipt,
    receipt_with,
};
use color_eyre::eyre::{Result, bail};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// The seven states
// ---------------------------------------------------------------------------

#[test]
fn an_intact_identity_with_matching_public_bytes_is_available_exact() -> Result<()> {
    let receipt = receipt(AVAILABLE_EXACT)?;
    expect_state(&receipt, PublicState::AvailableExact, "intact identity")?;
    if !receipt.blockers.is_empty() {
        bail!("available_exact must carry no blockers: {:?}", receipt.blockers);
    }
    let Some(bytes) = &receipt.public_bytes else {
        bail!("available_exact must record the exact public bytes it proved");
    };
    if bytes.version != "0.17.0" {
        bail!("public bytes recorded the wrong version: {}", bytes.version);
    }
    Ok(())
}

#[test]
fn the_incident_shape_is_extension_missing_on_three_independent_absences() -> Result<()> {
    let receipt = receipt(INCIDENT)?;
    expect_state(&receipt, PublicState::ExtensionMissing, "incident shape")?;
    expect_blocker(&receipt, "extension_absent_on_every_surface")?;
    if receipt.public_bytes.is_some() {
        bail!("a missing extension cannot have retrievable public bytes");
    }
    Ok(())
}

#[test]
fn an_absent_listing_with_a_retrievable_package_keeps_both_facts() -> Result<()> {
    let receipt = receipt(LISTING_MISSING)?;
    expect_state(&receipt, PublicState::ListingMissingVersionRetrievable, "hidden gallery")?;
    if receipt.public_bytes.is_none() {
        bail!("a retrievable package must still record its public bytes");
    }
    Ok(())
}

#[test]
fn an_unresolvable_namespace_is_the_narrower_diagnosis() -> Result<()> {
    let receipt = receipt(NAMESPACE_ABSENT)?;
    expect_state(&receipt, PublicState::NamespaceOrPublisherProblem, "absent namespace")?;
    expect_blocker(&receipt, "namespace_absent")?;
    Ok(())
}

#[test]
fn a_public_package_that_is_not_the_approved_build_is_not_available_exact() -> Result<()> {
    let receipt = receipt(DIGEST_MISMATCH)?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "digest mismatch")?;
    expect_blocker(&receipt, "public_digest_mismatch")?;
    Ok(())
}

#[test]
fn an_observation_that_left_the_planned_request_set_is_invalid() -> Result<()> {
    let receipt = receipt(UNPLANNED_URL)?;
    expect_state(&receipt, PublicState::Invalid, "unplanned URL")?;
    expect_blocker(&receipt, "unplanned_request_url")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: provider failure is never absence
// ---------------------------------------------------------------------------

#[test]
fn a_rate_limited_package_endpoint_cannot_become_extension_missing() -> Result<()> {
    let receipt = receipt(RATE_LIMITED)?;
    expect_state(&receipt, PublicState::ProviderNotProven, "rate limited")?;
    expect_blocker(&receipt, "provider_evidence_missing")?;
    Ok(())
}

#[test]
fn every_non_404_failure_class_resolves_to_provider_not_proven() -> Result<()> {
    // Each of these would produce `extension_missing` under an implementation
    // that treats "did not get the file" as "the file is gone".
    let failures: Vec<(&str, Value)> = vec![
        ("server error", json!({"outcome": "http_response", "status": 500})),
        ("gateway timeout", json!({"outcome": "http_response", "status": 504})),
        ("forbidden", json!({"outcome": "http_response", "status": 403})),
        ("unauthorized", json!({"outcome": "http_response", "status": 401})),
        ("gone", json!({"outcome": "http_response", "status": 410})),
        ("connect failure", json!({"outcome": "transport_error", "error_kind": "connect"})),
        ("dns failure", json!({"outcome": "transport_error", "error_kind": "dns"})),
        ("tls failure", json!({"outcome": "transport_error", "error_kind": "tls"})),
        ("timeout", json!({"outcome": "transport_error", "error_kind": "timeout"})),
        ("schema drift", json!({"outcome": "transport_error", "error_kind": "schema_drift"})),
    ];

    for (label, patch) in failures {
        let receipt = receipt_with(INCIDENT, |document| {
            let transport = &mut document["cells"]["versioned_file"]["transport"];
            transport["status"] = Value::Null;
            transport["error_kind"] = Value::Null;
            for (key, value) in patch.as_object().into_iter().flatten() {
                transport[key.as_str()] = value.clone();
            }
        })?;
        expect_state(&receipt, PublicState::ProviderNotProven, label)?;
    }
    Ok(())
}

#[test]
fn a_truncated_response_cannot_prove_presence_or_absence() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["versioned_file"]["transport"]["truncated"] = json!(true);
    })?;
    expect_state(&receipt, PublicState::ProviderNotProven, "truncated package read")?;
    if receipt.public_bytes.is_some() {
        bail!("a truncated read must not be recorded as proven public bytes");
    }
    Ok(())
}

#[test]
fn an_incomplete_instrument_cannot_prove_absence() -> Result<()> {
    let receipt = receipt(INSTRUMENT_INCOMPLETE)?;
    expect_state(&receipt, PublicState::ProviderNotProven, "unattempted cell")?;
    expect_blocker(&receipt, "cell_not_attempted")?;
    if receipt.instrument_complete {
        bail!("a run with an unattempted cell must not report a complete instrument");
    }
    Ok(())
}

#[test]
fn skipping_even_a_non_decisive_surface_blocks_a_missing_verdict() -> Result<()> {
    // Search never decides availability on its own, so nothing in the
    // present/absent logic stops it being skipped. Absence still must not be
    // concluded from a run that did not finish.
    let receipt = receipt_with(INCIDENT, |document| {
        let transport = &mut document["cells"]["search"]["transport"];
        transport["outcome"] = json!("not_attempted");
        transport["status"] = Value::Null;
        transport["response_bytes"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::ProviderNotProven, "skipped search cell")?;
    expect_blocker(&receipt, "incomplete_instrument_cannot_prove_absence")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: historical evidence is never current availability
// ---------------------------------------------------------------------------

#[test]
fn historical_publication_evidence_cannot_raise_the_state() -> Result<()> {
    // The incident's whole difficulty is that a successful June publish sits
    // beside an unreachable August listing. Adding more historical references
    // must not move the answer by one bit.
    let baseline = receipt(INCIDENT)?;
    let embellished = receipt_with(INCIDENT, |document| {
        document["expected"]["publication_refs"] = json!([
            "Publish Open VSX reported success",
            "published extension smoke passed",
            "release closeout marked v0.17.0 distributed",
        ]);
    })?;
    expect_state(&embellished, PublicState::ExtensionMissing, "embellished history")?;
    if embellished.state != baseline.state || embellished.blockers != baseline.blockers {
        bail!("historical publication references changed the classification");
    }

    // Stripping them must not move it either.
    let stripped = receipt_with(INCIDENT, |document| {
        document["expected"]["publication_refs"] = json!([]);
    })?;
    expect_state(&stripped, PublicState::ExtensionMissing, "stripped history")?;
    Ok(())
}

#[test]
fn search_discoverability_alone_cannot_establish_availability() -> Result<()> {
    let receipt = receipt_with(INCIDENT, |document| {
        document["cells"]["search"]["matched_identity"] = json!(true);
    })?;
    expect_state(&receipt, PublicState::ExtensionMissing, "search claims a match")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: identity is exact
// ---------------------------------------------------------------------------

#[test]
fn a_record_that_does_not_affirm_this_identity_cannot_be_available_exact() -> Result<()> {
    for claim in [json!(false), Value::Null] {
        let receipt = receipt_with(AVAILABLE_EXACT, |document| {
            document["cells"]["extension_metadata"]["identity_matches"] = claim.clone();
        })?;
        expect_state(&receipt, PublicState::AvailableIdentityNotProven, "unaffirmed identity")?;
        expect_blocker(&receipt, "metadata_identity_mismatch")?;
    }
    Ok(())
}

#[test]
fn a_subject_absent_from_the_published_rows_cannot_be_available_exact() -> Result<()> {
    // Retrieving bytes from a version the registry does not list is exactly the
    // shape a partial restoration would produce.
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["version_rows"]["versions"] = json!(["0.16.0", "0.18.0"]);
    })?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "unlisted subject version")?;
    expect_blocker(&receipt, "subject_version_not_published")?;
    Ok(())
}

#[test]
fn the_receipt_keeps_published_rows_with_the_surface_that_reported_them() -> Result<()> {
    let receipt = receipt(AVAILABLE_EXACT)?;
    for (cell, expected) in [
        ("extension_metadata", Some(vec!["0.17.0".to_owned()])),
        ("version_rows", Some(vec!["0.17.0".to_owned()])),
        ("listing", None),
        ("search", None),
        ("namespace_metadata", None),
        ("versioned_file", None),
    ] {
        let Some(result) = receipt.cells.iter().find(|entry| entry.cell == cell) else {
            bail!("{cell} must be reported");
        };
        if result.versions != expected {
            bail!("{cell} reported versions {:?}, expected {expected:?}", result.versions);
        }
    }
    Ok(())
}

#[test]
fn a_package_reporting_another_version_cannot_satisfy_the_subject() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["versioned_file"]["version"] = json!("0.16.0");
    })?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "version mismatch")?;
    expect_blocker(&receipt, "public_version_mismatch")?;
    Ok(())
}

#[test]
fn public_bytes_without_an_expected_digest_are_never_proven_exact() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["versions"][0]["vsix_sha256"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "no expected digest")?;
    expect_blocker(&receipt, "expected_digest_absent")?;
    Ok(())
}

#[test]
fn a_listing_that_resolves_without_retrievable_bytes_is_not_exact() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        let file = &mut document["cells"]["versioned_file"];
        file["transport"]["status"] = json!(404);
        file["version"] = Value::Null;
        file["sha256"] = Value::Null;
        file["byte_length"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "package absent")?;
    expect_blocker(&receipt, "public_package_not_retrieved")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Structural refusals
// ---------------------------------------------------------------------------

#[test]
fn a_non_read_only_request_invalidates_the_observation() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["listing"]["transport"]["method"] = json!("DELETE");
    })?;
    expect_state(&receipt, PublicState::Invalid, "mutating method")?;
    expect_blocker(&receipt, "non_read_only_request")?;
    Ok(())
}

#[test]
fn a_budget_overrun_must_be_reported_rather_than_absorbed() -> Result<()> {
    let redirects = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["listing"]["transport"]["redirects"] = json!(9);
    })?;
    expect_state(&redirects, PublicState::Invalid, "silent redirect overrun")?;
    expect_blocker(&redirects, "redirect_budget_exceeded")?;

    let bytes = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["listing"]["transport"]["response_bytes"] = json!(64 * 1024 * 1024_u64);
    })?;
    expect_state(&bytes, PublicState::Invalid, "silent byte overrun")?;
    expect_blocker(&bytes, "byte_budget_exceeded")?;

    // A reported overrun is an instrument outcome, not a structural defect.
    let reported = receipt_with(AVAILABLE_EXACT, |document| {
        let transport = &mut document["cells"]["listing"]["transport"];
        transport["redirects"] = json!(9);
        transport["error_kind"] = json!("redirect_limit_exceeded");
    })?;
    expect_state(&reported, PublicState::ProviderNotProven, "reported redirect overrun")?;
    Ok(())
}

#[test]
fn an_observation_of_another_registry_or_schema_is_invalid() -> Result<()> {
    let registry = receipt_with(AVAILABLE_EXACT, |document| {
        document["registry"] = json!("marketplace");
    })?;
    expect_state(&registry, PublicState::Invalid, "foreign registry")?;
    expect_blocker(&registry, "unsupported_registry")?;

    let schema = receipt_with(AVAILABLE_EXACT, |document| {
        document["schema_version"] = json!("open_vsx_public_state_observation.v2");
    })?;
    expect_state(&schema, PublicState::Invalid, "foreign schema")?;
    expect_blocker(&schema, "unsupported_observation_schema")?;
    Ok(())
}

#[test]
fn a_status_without_a_response_is_invalid() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        let transport = &mut document["cells"]["listing"]["transport"];
        transport["outcome"] = json!("transport_error");
        transport["error_kind"] = json!("timeout");
    })?;
    expect_state(&receipt, PublicState::Invalid, "status without response")?;
    expect_blocker(&receipt, "status_without_response")?;
    Ok(())
}

#[test]
fn a_malformed_public_digest_is_invalid_rather_than_compared() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["versioned_file"]["sha256"] = json!("NOTAHEXDIGEST");
    })?;
    expect_state(&receipt, PublicState::Invalid, "malformed digest")?;
    expect_blocker(&receipt, "malformed_public_digest")?;
    Ok(())
}

#[test]
fn a_reachable_record_behind_an_absent_listing_is_not_called_missing() -> Result<()> {
    // The object is still there; only its public presentation is gone. Calling
    // that "missing" would send the incident toward republication.
    let receipt = receipt_with(INCIDENT, |document| {
        let metadata = &mut document["cells"]["extension_metadata"];
        metadata["transport"]["status"] = json!(200);
        metadata["identity_matches"] = json!(true);
        metadata["versions"] = json!(["0.17.0"]);
    })?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "reachable metadata")?;
    expect_blocker(&receipt, "listing_absent_with_reachable_metadata")?;
    Ok(())
}

#[test]
fn a_namespace_endpoint_that_does_not_confirm_the_namespace_is_a_publisher_problem() -> Result<()> {
    let receipt = receipt_with(INCIDENT, |document| {
        document["cells"]["namespace_metadata"]["namespace_present"] = json!(false);
    })?;
    expect_state(&receipt, PublicState::NamespaceOrPublisherProblem, "unconfirmed namespace")?;
    expect_blocker(&receipt, "namespace_not_confirmed")?;
    Ok(())
}

#[test]
fn definite_but_inconsistent_answers_support_no_availability_conclusion() -> Result<()> {
    // A resolving gallery page over an absent extension record is not a fact
    // pattern this classifier is willing to summarise either way.
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        let metadata = &mut document["cells"]["extension_metadata"];
        metadata["transport"]["status"] = json!(404);
        metadata["identity_matches"] = Value::Null;
        metadata["versions"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::ProviderNotProven, "contradictory answers")?;
    expect_blocker(&receipt, "contradictory_registry_evidence")?;
    Ok(())
}

#[test]
fn a_malformed_subject_identity_or_version_is_invalid() -> Result<()> {
    let identity = receipt_with(AVAILABLE_EXACT, |document| {
        document["identity"]["namespace"] = json!("Effortless/Metrics");
    })?;
    expect_state(&identity, PublicState::Invalid, "path-bearing namespace")?;
    expect_blocker(&identity, "malformed_identity")?;

    let version = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["versions"][0]["version"] = json!("../0.17.0");
    })?;
    expect_state(&version, PublicState::Invalid, "traversal in a version")?;
    expect_blocker(&version, "malformed_expected_version")?;

    let empty = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["versions"] = json!([]);
    })?;
    expect_state(&empty, PublicState::Invalid, "no expected version")?;
    expect_blocker(&empty, "missing_expected_version")?;
    if empty.probe_plan_digest.is_some() || empty.subject_version.is_some() {
        bail!("a receipt with no derivable plan must not claim a plan digest or subject");
    }
    Ok(())
}

#[test]
fn ambiguous_or_malformed_expected_identity_is_invalid() -> Result<()> {
    let duplicate = receipt_with(AVAILABLE_EXACT, |document| {
        let entry = document["expected"]["versions"][0].clone();
        document["expected"]["versions"] = json!([entry.clone(), entry]);
    })?;
    expect_state(&duplicate, PublicState::Invalid, "duplicate expected version")?;
    expect_blocker(&duplicate, "duplicate_expected_version")?;

    let digest = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["versions"][0]["vsix_sha256"] = json!("ABC");
    })?;
    expect_state(&digest, PublicState::Invalid, "malformed expected digest")?;
    expect_blocker(&digest, "malformed_expected_digest")?;
    Ok(())
}

#[test]
fn a_half_recorded_package_identity_is_invalid() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["versioned_file"]["byte_length"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::Invalid, "digest without a byte length")?;
    expect_blocker(&receipt, "incomplete_public_bytes")?;
    Ok(())
}

#[test]
fn an_http_response_without_a_status_is_invalid() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["listing"]["transport"]["status"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::Invalid, "response without a status")?;
    expect_blocker(&receipt, "missing_status")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: unparsed data is not affirmative evidence
//
// These four share one root cause found in review: a surface that answered but
// whose answer was never parsed was reading as "no objection" rather than as
// missing evidence, which is the same collapse this module exists to prevent.
// ---------------------------------------------------------------------------

#[test]
fn an_unparsed_namespace_response_cannot_underwrite_a_missing_verdict() -> Result<()> {
    // `extension_missing` is licensed by "the namespace still resolves". A 200
    // that produced no identity claim did not establish that.
    let receipt = receipt_with(INCIDENT, |document| {
        document["cells"]["namespace_metadata"]["namespace_present"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::ProviderNotProven, "unparsed namespace")?;
    expect_blocker(&receipt, "namespace_identity_not_parsed")?;
    Ok(())
}

#[test]
fn unparsed_version_rows_cannot_reach_available_exact() -> Result<()> {
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["version_rows"]["versions"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "unparsed version rows")?;
    expect_blocker(&receipt, "version_rows_not_parsed")?;
    Ok(())
}

#[test]
fn a_malformed_non_subject_version_is_invalid() -> Result<()> {
    // `expected` is copied verbatim into the receipt, so a malformed entry
    // anywhere in the list would emit a receipt violating its own schema.
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["versions"] = json!([
            {"version": "0.17.0", "vsix_sha256": Value::Null},
            {"version": "../0.16.0", "vsix_sha256": Value::Null},
        ]);
    })?;
    expect_state(&receipt, PublicState::Invalid, "malformed second version")?;
    expect_blocker(&receipt, "malformed_expected_version")?;
    Ok(())
}

#[test]
fn an_instant_that_cannot_be_placed_on_a_timeline_is_invalid() -> Result<()> {
    // The receipt's whole value is that it describes a *current* state, so a
    // non-instant — or one without an explicit offset — cannot back that claim.
    for invalid in ["yesterday", "2026-08-14", "2026-08-14T09:12:00", "not a date", "0"] {
        let receipt = receipt_with(AVAILABLE_EXACT, |document| {
            document["observed_at"] = json!(invalid);
        })?;
        expect_state(&receipt, PublicState::Invalid, invalid)?;
        expect_blocker(&receipt, "malformed_observed_at")?;
    }
    // An explicit non-UTC offset is a perfectly placeable instant.
    let offset = receipt_with(AVAILABLE_EXACT, |document| {
        document["observed_at"] = json!("2026-08-14T09:12:00+02:00");
    })?;
    expect_state(&offset, PublicState::AvailableExact, "explicit offset")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative controls: the strongest claim needs evidence from outside the input
// ---------------------------------------------------------------------------

#[test]
fn an_unbound_expected_digest_cannot_manufacture_available_exact() -> Result<()> {
    // Raised in review: the observed digest and the expected digest arrive
    // through the same input. A producer that simply copied the retrieved digest
    // into `expected` would otherwise mint the strongest claim from nothing.
    let receipt = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["authority"] = Value::Null;
    })?;
    expect_state(&receipt, PublicState::AvailableIdentityNotProven, "unbound expected digest")?;
    expect_blocker(&receipt, "expected_identity_unbound")?;
    Ok(())
}

#[test]
fn a_bounded_read_must_be_evidenced_before_exact_bytes_are_accepted() -> Result<()> {
    // Omitting the byte count skipped the budget check entirely, so an oversized
    // package could be reported as exact while the bounded read it claims was
    // never established.
    let unmeasured = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["versioned_file"]["transport"]["response_bytes"] = Value::Null;
    })?;
    expect_state(&unmeasured, PublicState::Invalid, "unmeasured package read")?;
    expect_blocker(&unmeasured, "unmeasured_response")?;

    // A digest only covers what was actually read.
    let short_read = receipt_with(AVAILABLE_EXACT, |document| {
        document["cells"]["versioned_file"]["byte_length"] = json!(12);
    })?;
    expect_state(&short_read, PublicState::Invalid, "partial package read")?;
    expect_blocker(&short_read, "package_byte_length_mismatch")?;
    Ok(())
}

#[test]
fn path_and_credential_shaped_references_cannot_cross_the_publication_boundary() -> Result<()> {
    // These fields are free text copied verbatim into a durable, shareable
    // receipt. Property-name closure says nothing about what a producer puts
    // inside a string, so the values are validated too.
    let hostile = [
        "file:///home/someone/.ssh/id_rsa",
        "https://user:hunter2@example.invalid/run",
        "/home/someone/secret-notes.txt",
        "~/.aws/credentials",
        "C:/Users/someone/token.txt",
        "C:\\Users\\someone\\token.txt",
        "ftp://example.invalid/x",
        "has\u{7}a\u{0}control",
    ];

    for value in hostile {
        let via_refs = receipt_with(AVAILABLE_EXACT, |document| {
            document["expected"]["publication_refs"] = json!([value]);
        })?;
        expect_state(&via_refs, PublicState::Invalid, value)?;
        expect_blocker(&via_refs, "unsafe_reference_value")?;
        if serde_json::to_string(&via_refs)?.contains("hunter2") {
            bail!("a credential-shaped reference reached the receipt for {value:?}");
        }

        let via_instrument = receipt_with(AVAILABLE_EXACT, |document| {
            document["instrument"]["source_ref"] = json!(value);
        })?;
        expect_state(&via_instrument, PublicState::Invalid, value)?;
        expect_blocker(&via_instrument, "unsafe_reference_value")?;
    }

    // An over-long reference is refused rather than retained.
    let oversized = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["publication_refs"] = json!(["x".repeat(5000)]);
    })?;
    expect_state(&oversized, PublicState::Invalid, "oversized reference")?;
    expect_blocker(&oversized, "unsafe_reference_value")?;

    // The shapes a real reference actually takes still pass.
    let benign = receipt_with(AVAILABLE_EXACT, |document| {
        document["expected"]["publication_refs"] = json!([
            "https://github.com/EffortlessMetrics/perl-lsp-swarm/actions/runs/0000000000",
            "Publish VSCode Extension / Publish Open VSX (2026-06-28) reported success",
        ]);
    })?;
    expect_state(&benign, PublicState::AvailableExact, "benign references")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Receipt shape
// ---------------------------------------------------------------------------

#[test]
fn the_receipt_is_byte_identical_for_a_fixed_observation() -> Result<()> {
    for fixture in [AVAILABLE_EXACT, INCIDENT, RATE_LIMITED, UNPLANNED_URL] {
        let first = serde_json::to_string_pretty(&receipt(fixture)?)?;
        let second = serde_json::to_string_pretty(&receipt(fixture)?)?;
        if first != second {
            bail!("classification is not deterministic for a fixed observation");
        }
    }
    Ok(())
}

#[test]
fn every_surface_is_reported_separately_and_in_a_fixed_order() -> Result<()> {
    let receipt = receipt(INCIDENT)?;
    let order: Vec<&str> = receipt.cells.iter().map(|cell| cell.cell).collect();
    let expected = [
        "listing",
        "search",
        "namespace_metadata",
        "extension_metadata",
        "version_rows",
        "versioned_file",
    ];
    if order != expected {
        bail!("receipt cell order drifted: {order:?}");
    }
    Ok(())
}

#[test]
fn only_an_affirmative_404_is_recorded_as_proven_absence() -> Result<()> {
    let receipt = receipt(RATE_LIMITED)?;
    let Some(file) = receipt.cells.iter().find(|cell| cell.cell == "versioned_file") else {
        bail!("versioned_file cell must be reported");
    };
    if file.observation != CellObservation::ProviderFailed {
        bail!("a 429 was recorded as {:?}", file.observation);
    }
    Ok(())
}

#[test]
fn blockers_are_present_exactly_when_the_state_is_not_exact() -> Result<()> {
    let cases = [
        AVAILABLE_EXACT,
        INCIDENT,
        LISTING_MISSING,
        RATE_LIMITED,
        NAMESPACE_ABSENT,
        DIGEST_MISMATCH,
        UNPLANNED_URL,
        INSTRUMENT_INCOMPLETE,
    ];
    for fixture in cases {
        let receipt = receipt(fixture)?;
        let exact = receipt.state == PublicState::AvailableExact;
        if exact != receipt.blockers.is_empty() {
            bail!("state {} and blocker set disagree: {:?}", receipt.state.key(), receipt.blockers);
        }
    }
    Ok(())
}
