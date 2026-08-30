//! Pure classification of one Open VSX observation into a public-state receipt.
//!
//! The whole point of this module is that provider failure, instrument gaps and
//! contradictory answers stay distinguishable from proven absence. Only three
//! independent affirmative `404`s — gallery listing, extension metadata and the
//! versioned package file — can reach `extension_missing`, and only an exact
//! public digest match can reach `available_exact`.
//!
//! Historical publication evidence is carried through to the receipt but is
//! never read here: `expected.publication_refs` has no path into the state.

use super::model::{
    Blocker, CellObservation, CellResult, ErrorKind, OBSERVATION_SCHEMA_VERSION, Observation,
    PublicBytes, PublicState, RECEIPT_SCHEMA_VERSION, REGISTRY, Receipt, ReceiptIdentity,
    Transport, TransportOutcome,
};
use super::plan::{Cell, ProbePlan, probe_plan, valid_registry_segment, valid_version};

/// One decisive surface, reduced to the facts classification may use.
struct Decisive {
    cell: Cell,
    observation: CellObservation,
}

/// Classify an observation. Never fails: an observation this module cannot
/// trust becomes an `invalid` receipt rather than an error, so the caller
/// always has a durable artifact to retain.
pub(crate) fn classify(observation: Observation) -> Receipt {
    let mut blockers: Vec<Blocker> = Vec::new();
    let mut limitations: Vec<String> = Vec::new();

    let namespace = observation.identity.namespace.clone();
    let extension = observation.identity.extension.clone();
    let extension_id = format!("{namespace}.{extension}");

    let subject_version =
        observation.expected.versions.first().map(|version| version.version.clone());

    let plan = match subject_version.as_deref() {
        Some(version) => probe_plan(&namespace, &extension, version),
        None => None,
    };

    let mut structural =
        structural_findings(&observation, plan.as_ref(), subject_version.as_deref());

    let cells = build_cells(&observation);
    let instrument_complete =
        cells.iter().all(|cell| cell.observation != CellObservation::NotAttempted);
    if !instrument_complete {
        limitations.push(
            "At least one planned registry surface was not attempted; absence cannot be proven \
             from an incomplete instrument."
                .to_owned(),
        );
    }

    let public_bytes = retrievable_public_bytes(&observation);
    if public_bytes.is_none() {
        limitations.push(
            "Exact public package bytes were not retrieved, so public byte identity is unproven."
                .to_owned(),
        );
    }

    let state = if structural.is_empty() {
        let (state, mut state_blockers, mut state_limitations) =
            classify_state(&observation, subject_version.as_deref(), public_bytes.as_ref());
        blockers.append(&mut state_blockers);
        limitations.append(&mut state_limitations);
        state
    } else {
        blockers.append(&mut structural);
        PublicState::Invalid
    };

    // Absence is the one conclusion an incomplete run may never reach, even when
    // every surface it did attempt agreed. Search is not otherwise decisive, so
    // without this guard a skipped search cell could still ride along to
    // `extension_missing`.
    let state = if state == PublicState::ExtensionMissing && !instrument_complete {
        blockers.push(Blocker::new(
            "incomplete_instrument_cannot_prove_absence",
            "at least one planned surface was never attempted, so absence is unproven",
        ));
        PublicState::ProviderNotProven
    } else {
        state
    };

    limitations.push(
        "Registry-side deactivation, moderation and publisher-membership state are not exposed \
         by these public surfaces and remain unproven here."
            .to_owned(),
    );

    blockers.sort();
    blockers.dedup();
    limitations.sort();
    limitations.dedup();

    // Both fields are derived from the plan, so they are present together or
    // absent together. Absent means the observation could not name one exact
    // request set, which is always the invalid state.
    let (probe_plan_digest, subject_version) = match plan.as_ref().map(ProbePlan::digest) {
        Some(Ok(digest)) => (Some(digest), subject_version),
        Some(Err(_)) | None => (None, None),
    };

    Receipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        observed_at: observation.observed_at,
        registry: REGISTRY,
        identity: ReceiptIdentity { namespace, extension, extension_id },
        instrument: observation.instrument,
        instrument_complete,
        probe_plan_digest,
        subject_version,
        cells,
        public_bytes,
        expected: observation.expected,
        limitations,
        blockers,
        state,
    }
}

/// Findings that make the observation untrustworthy as a description of the
/// planned probe. Any one of them forces `invalid`.
fn structural_findings(
    observation: &Observation,
    plan: Option<&ProbePlan>,
    subject_version: Option<&str>,
) -> Vec<Blocker> {
    let mut findings = Vec::new();

    if observation.schema_version != OBSERVATION_SCHEMA_VERSION {
        findings.push(Blocker::new(
            "unsupported_observation_schema",
            format!(
                "observation schema_version {:?} is not {OBSERVATION_SCHEMA_VERSION}",
                observation.schema_version
            ),
        ));
    }
    if observation.registry != REGISTRY {
        findings.push(Blocker::new(
            "unsupported_registry",
            format!("registry {:?} is not {REGISTRY}", observation.registry),
        ));
    }
    if observation.observed_at.trim().is_empty() {
        findings.push(Blocker::new(
            "missing_observed_at",
            "observation carries no observation instant, so it cannot establish current state",
        ));
    }
    if !valid_registry_segment(&observation.identity.namespace)
        || !valid_registry_segment(&observation.identity.extension)
    {
        findings.push(Blocker::new(
            "malformed_identity",
            "namespace or extension is not a canonical registry segment",
        ));
    }
    match subject_version {
        None => findings.push(Blocker::new(
            "missing_expected_version",
            "no expected version was supplied, so no versioned package could be addressed",
        )),
        Some(version) if !valid_version(version) => findings.push(Blocker::new(
            "malformed_expected_version",
            format!("expected version {version:?} is not a canonical version segment"),
        )),
        Some(_) => {}
    }

    let mut seen: Vec<&str> = Vec::new();
    for expected in &observation.expected.versions {
        if seen.contains(&expected.version.as_str()) {
            findings.push(Blocker::new(
                "duplicate_expected_version",
                format!("expected version {:?} is listed more than once", expected.version),
            ));
        }
        seen.push(&expected.version);
        if let Some(digest) = &expected.vsix_sha256
            && !is_sha256(digest)
        {
            findings.push(Blocker::new(
                "malformed_expected_digest",
                format!("expected digest for {:?} is not lower-case SHA-256", expected.version),
            ));
        }
    }

    let Some(plan) = plan else {
        return findings;
    };

    for (cell, transport) in transports(observation) {
        let Some(planned) = plan.request(cell) else {
            findings.push(Blocker::new(
                "unplanned_cell",
                format!("{} has no planned request", cell.key()),
            ));
            continue;
        };
        if transport.method != planned.method {
            findings.push(Blocker::new(
                "non_read_only_request",
                format!(
                    "{} used method {:?}; only GET is sanctioned",
                    cell.key(),
                    transport.method
                ),
            ));
        }
        if transport.url != planned.url {
            findings.push(Blocker::new(
                "unplanned_request_url",
                format!(
                    "{} addressed {:?} instead of the planned {:?}",
                    cell.key(),
                    transport.url,
                    planned.url
                ),
            ));
        }
        match transport.outcome {
            TransportOutcome::HttpResponse if transport.status.is_none() => {
                findings.push(Blocker::new(
                    "missing_status",
                    format!("{} reports an HTTP response without a status", cell.key()),
                ));
            }
            TransportOutcome::TransportError | TransportOutcome::NotAttempted
                if transport.status.is_some() =>
            {
                findings.push(Blocker::new(
                    "status_without_response",
                    format!("{} reports a status without an HTTP response", cell.key()),
                ));
            }
            _ => {}
        }
        if transport.redirects > planned.max_redirects
            && transport.error_kind != Some(ErrorKind::RedirectLimitExceeded)
        {
            findings.push(Blocker::new(
                "redirect_budget_exceeded",
                format!(
                    "{} followed {} redirects past the {} budget without reporting it",
                    cell.key(),
                    transport.redirects,
                    planned.max_redirects
                ),
            ));
        }
        if let Some(bytes) = transport.response_bytes
            && bytes > planned.max_response_bytes
            && !transport.truncated
        {
            findings.push(Blocker::new(
                "byte_budget_exceeded",
                format!(
                    "{} read {bytes} bytes past the {} budget without reporting truncation",
                    cell.key(),
                    planned.max_response_bytes
                ),
            ));
        }
    }

    let file = &observation.cells.versioned_file;
    if let Some(digest) = &file.sha256
        && !is_sha256(digest)
    {
        findings.push(Blocker::new(
            "malformed_public_digest",
            "observed public package digest is not lower-case SHA-256",
        ));
    }
    if file.sha256.is_some() != file.byte_length.is_some() {
        findings.push(Blocker::new(
            "incomplete_public_bytes",
            "public package digest and byte length must be recorded together",
        ));
    }

    findings
}

/// The state machine.
fn classify_state(
    observation: &Observation,
    subject_version: Option<&str>,
    public_bytes: Option<&PublicBytes>,
) -> (PublicState, Vec<Blocker>, Vec<String>) {
    let mut blockers = Vec::new();
    let mut limitations = Vec::new();

    let listing = observe(&observation.cells.listing.transport);
    let search = observe(&observation.cells.search.transport);
    let namespace_metadata = observe(&observation.cells.namespace_metadata.transport);
    let extension_metadata = observe(&observation.cells.extension_metadata.transport);
    let version_rows = observe(&observation.cells.version_rows.transport);
    let versioned_file = observe(&observation.cells.versioned_file.transport);

    if search == CellObservation::ProviderFailed || search == CellObservation::NotAttempted {
        limitations
            .push("Search discoverability was not established for this observation.".to_owned());
    }

    // A namespace that is gone or renamed is the narrower diagnosis; reporting
    // it as a missing extension would send the incident down the wrong path.
    if namespace_metadata == CellObservation::ProvenAbsent {
        blockers.push(Blocker::new(
            "namespace_absent",
            "the namespace itself does not resolve; extension-level absence is not the diagnosis",
        ));
        return (PublicState::NamespaceOrPublisherProblem, blockers, limitations);
    }
    if namespace_metadata == CellObservation::Present
        && observation.cells.namespace_metadata.namespace_present == Some(false)
    {
        blockers.push(Blocker::new(
            "namespace_not_confirmed",
            "the namespace endpoint responded without confirming this namespace",
        ));
        return (PublicState::NamespaceOrPublisherProblem, blockers, limitations);
    }

    let decisive = [
        Decisive { cell: Cell::Listing, observation: listing },
        Decisive { cell: Cell::NamespaceMetadata, observation: namespace_metadata },
        Decisive { cell: Cell::ExtensionMetadata, observation: extension_metadata },
        Decisive { cell: Cell::VersionRows, observation: version_rows },
        Decisive { cell: Cell::VersionedFile, observation: versioned_file },
    ];

    let mut unresolved = false;
    for entry in &decisive {
        match entry.observation {
            CellObservation::ProviderFailed => {
                unresolved = true;
                blockers.push(Blocker::new(
                    "provider_evidence_missing",
                    format!(
                        "{} did not yield a usable answer; provider or instrument failure is not \
                         evidence of absence",
                        entry.cell.key()
                    ),
                ));
            }
            CellObservation::NotAttempted => {
                unresolved = true;
                blockers.push(Blocker::new(
                    "cell_not_attempted",
                    format!("{} was never attempted", entry.cell.key()),
                ));
            }
            CellObservation::Present | CellObservation::ProvenAbsent => {}
        }
    }
    if unresolved {
        return (PublicState::ProviderNotProven, blockers, limitations);
    }

    // Every decisive surface now holds an affirmative present-or-absent answer.
    let metadata_reachable =
        extension_metadata == CellObservation::Present || version_rows == CellObservation::Present;

    if listing == CellObservation::Present && extension_metadata == CellObservation::Present {
        if observation.cells.extension_metadata.identity_matches != Some(true) {
            blockers.push(Blocker::new(
                "metadata_identity_mismatch",
                "the extension record did not affirm this exact namespace and name",
            ));
            return (PublicState::AvailableIdentityNotProven, blockers, limitations);
        }

        if let Some(subject) = subject_version
            && version_rows == CellObservation::Present
            && let Some(rows) = &observation.cells.version_rows.versions
            && !rows.iter().any(|row| row == subject)
        {
            blockers.push(Blocker::new(
                "subject_version_not_published",
                format!("the published version rows do not list the subject version {subject:?}"),
            ));
            return (PublicState::AvailableIdentityNotProven, blockers, limitations);
        }

        let Some(bytes) = public_bytes else {
            blockers.push(Blocker::new(
                "public_package_not_retrieved",
                "the listing resolves but the versioned package was not retrieved, so exact \
                 public bytes are unproven",
            ));
            return (PublicState::AvailableIdentityNotProven, blockers, limitations);
        };

        let Some(subject) = subject_version else {
            blockers.push(Blocker::new(
                "missing_expected_version",
                "no subject version to compare public bytes against",
            ));
            return (PublicState::AvailableIdentityNotProven, blockers, limitations);
        };
        if bytes.version != subject {
            blockers.push(Blocker::new(
                "public_version_mismatch",
                format!(
                    "the retrieved package reports version {:?}, not the subject {subject:?}",
                    bytes.version
                ),
            ));
            return (PublicState::AvailableIdentityNotProven, blockers, limitations);
        }

        let expected_digest = observation
            .expected
            .versions
            .iter()
            .find(|candidate| candidate.version == subject)
            .and_then(|candidate| candidate.vsix_sha256.as_deref());
        match expected_digest {
            None => {
                blockers.push(Blocker::new(
                    "expected_digest_absent",
                    "no expected package digest was supplied, so the public bytes cannot be \
                     proven to be the approved ones",
                ));
                (PublicState::AvailableIdentityNotProven, blockers, limitations)
            }
            Some(expected) if expected == bytes.sha256 => {
                (PublicState::AvailableExact, blockers, limitations)
            }
            Some(expected) => {
                blockers.push(Blocker::new(
                    "public_digest_mismatch",
                    format!(
                        "public package digest {} does not match the expected {expected}",
                        bytes.sha256
                    ),
                ));
                (PublicState::AvailableIdentityNotProven, blockers, limitations)
            }
        }
    } else if listing == CellObservation::ProvenAbsent {
        if versioned_file == CellObservation::Present {
            blockers.push(Blocker::new(
                "listing_absent_with_retrievable_package",
                "the gallery listing does not resolve while the versioned package still does",
            ));
            return (PublicState::ListingMissingVersionRetrievable, blockers, limitations);
        }
        if metadata_reachable {
            blockers.push(Blocker::new(
                "listing_absent_with_reachable_metadata",
                "the gallery listing does not resolve while extension metadata still does; the \
                 object is reachable but not publicly presented",
            ));
            return (PublicState::AvailableIdentityNotProven, blockers, limitations);
        }
        if extension_metadata == CellObservation::ProvenAbsent
            && versioned_file == CellObservation::ProvenAbsent
        {
            blockers.push(Blocker::new(
                "extension_absent_on_every_surface",
                "listing, extension metadata and the versioned package are each independently \
                 absent while the namespace still resolves",
            ));
            return (PublicState::ExtensionMissing, blockers, limitations);
        }
        blockers.push(Blocker::new(
            "contradictory_registry_evidence",
            "the registry returned definite but mutually inconsistent answers; no single \
             availability conclusion is supported",
        ));
        (PublicState::ProviderNotProven, blockers, limitations)
    } else {
        blockers.push(Blocker::new(
            "contradictory_registry_evidence",
            "the registry returned definite but mutually inconsistent answers; no single \
             availability conclusion is supported",
        ));
        (PublicState::ProviderNotProven, blockers, limitations)
    }
}

/// Reduce one transport record to what classification may rely on.
fn observe(transport: &Transport) -> CellObservation {
    // A truncated read or any reported error means the instrument did not come
    // away with a clean answer, whatever status accompanied it. A 200 carrying
    // `redirect_limit_exceeded` is not an affirmative presence.
    if transport.truncated || transport.error_kind.is_some() {
        return CellObservation::ProviderFailed;
    }
    match transport.outcome {
        TransportOutcome::NotAttempted => CellObservation::NotAttempted,
        TransportOutcome::TransportError => CellObservation::ProviderFailed,
        TransportOutcome::HttpResponse => match transport.status {
            Some(status) if (200..300).contains(&status) => CellObservation::Present,
            Some(404) => CellObservation::ProvenAbsent,
            // Every other status — including 401, 403, 410, 429 and 5xx — is an
            // answer about the request, not proof about the extension.
            _ => CellObservation::ProviderFailed,
        },
    }
}

/// Public bytes are recorded only when the exact file was retrieved in full.
fn retrievable_public_bytes(observation: &Observation) -> Option<PublicBytes> {
    let cell = &observation.cells.versioned_file;
    if observe(&cell.transport) != CellObservation::Present {
        return None;
    }
    let version = cell.version.clone()?;
    let sha256 = cell.sha256.clone()?;
    let byte_length = cell.byte_length?;
    if !is_sha256(&sha256) {
        return None;
    }
    Some(PublicBytes { version, sha256, byte_length })
}

/// Reflect what was actually observed.
///
/// The URL reported is the one the observation addressed, never the one the plan
/// wanted: on an `invalid` receipt the discrepancy is the finding, and
/// substituting the planned URL would hide it.
fn build_cells(observation: &Observation) -> Vec<CellResult> {
    Cell::ALL
        .into_iter()
        .map(|cell| {
            let transport = transport_for(observation, cell);
            CellResult {
                cell: cell.key(),
                url: transport.url.clone(),
                method: "GET",
                observation: observe(transport),
                status: transport.status,
                redirects: transport.redirects,
                response_bytes: transport.response_bytes,
                truncated: transport.truncated,
                error_kind: transport.error_kind.map(ErrorKind::key),
                identity_match: identity_match(observation, cell),
                versions: published_versions(observation, cell),
            }
        })
        .collect()
}

fn identity_match(observation: &Observation, cell: Cell) -> Option<bool> {
    match cell {
        Cell::Search => observation.cells.search.matched_identity,
        Cell::NamespaceMetadata => observation.cells.namespace_metadata.namespace_present,
        Cell::ExtensionMetadata => observation.cells.extension_metadata.identity_matches,
        Cell::Listing | Cell::VersionRows | Cell::VersionedFile => None,
    }
}

/// Version rows a surface published, for the two surfaces that publish any.
fn published_versions(observation: &Observation, cell: Cell) -> Option<Vec<String>> {
    match cell {
        Cell::ExtensionMetadata => observation.cells.extension_metadata.versions.clone(),
        Cell::VersionRows => observation.cells.version_rows.versions.clone(),
        Cell::Listing | Cell::Search | Cell::NamespaceMetadata | Cell::VersionedFile => None,
    }
}

fn transports(observation: &Observation) -> Vec<(Cell, &Transport)> {
    Cell::ALL.into_iter().map(|cell| (cell, transport_for(observation, cell))).collect()
}

fn transport_for(observation: &Observation, cell: Cell) -> &Transport {
    match cell {
        Cell::Listing => &observation.cells.listing.transport,
        Cell::Search => &observation.cells.search.transport,
        Cell::NamespaceMetadata => &observation.cells.namespace_metadata.transport,
        Cell::ExtensionMetadata => &observation.cells.extension_metadata.transport,
        Cell::VersionRows => &observation.cells.version_rows.transport,
        Cell::VersionedFile => &observation.cells.versioned_file.transport,
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
