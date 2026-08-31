//! Pure classification of one Open VSX observation into a public-state receipt.
//!
//! The whole point of this module is that provider failure, instrument gaps and
//! contradictory answers stay distinguishable from proven absence. Only three
//! independent affirmative `404`s — gallery listing, extension metadata and the
//! versioned package file — can reach `extension_missing`, and nothing here can
//! reach `available_exact`: an exact digest match between two fields of the same
//! unbound observation is observed identity, not approval, so the ceiling is
//! `available_identity_not_proven`.
//!
//! Historical publication evidence is carried through to the receipt but is
//! never read here: `expected.publication_refs` has no path into the state.

use super::model::{
    Blocker, CellObservation, CellResult, ErrorKind, Expected, Instrument,
    OBSERVATION_SCHEMA_VERSION, Observation, PublicBytes, PublicState, RECEIPT_SCHEMA_VERSION,
    REGISTRY, Receipt, ReceiptIdentity, Transport, TransportOutcome,
};
use super::plan::{
    Cell, ProbePlan, SEARCH_PAGE_SIZE, probe_plan, valid_registry_segment, valid_version,
};
use chrono::DateTime;

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

    let cells = build_cells(&observation, plan.as_ref());
    // Raised in review: this was derived from the *classified* observation, and
    // `observe` maps any reported `error_kind` to `ProviderFailed` before it
    // looks at the outcome — so a cell saying "not attempted" while carrying an
    // error read as attempted-and-failed, `instrument_complete` stayed true, and
    // three 404s walked past the guard below into `extension_missing`. Whether a
    // request was attempted is a fact about the transport, not about how the
    // answer classifies, so it is read from the field that states it.
    let instrument_complete = transports(&observation)
        .iter()
        .all(|(_, transport)| transport.outcome != TransportOutcome::NotAttempted);
    if !instrument_complete {
        limitations.push(
            "At least one planned registry surface was not attempted; absence cannot be proven \
             from an incomplete instrument."
                .to_owned(),
        );
    }

    // Stated here rather than inside `classify_state`, because an observation
    // that fails structurally never reaches that function while `build_cells`
    // still publishes the search surface's identity match. Raised in review
    // against the first version of this limitation, which had exactly that hole:
    // the scope has to accompany the value wherever the value goes.
    limitations.push(search_scope(&observation));

    let public_bytes = retrievable_public_bytes(&observation);
    if public_bytes.is_none() {
        limitations.push(
            "Exact public package bytes were not retrieved, so public byte identity is unproven."
                .to_owned(),
        );
    }

    let state = if structural.is_empty() {
        let (state, mut state_blockers) =
            classify_state(&observation, subject_version.as_deref(), public_bytes.as_ref());
        blockers.append(&mut state_blockers);
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
        instrument: publishable_instrument(observation.instrument),
        instrument_complete,
        probe_plan_digest,
        subject_version,
        cells,
        public_bytes,
        expected: publishable_expected(observation.expected),
        limitations,
        blockers,
        state,
    }
}

/// Force a receipt to `invalid` for a finding this classifier cannot make itself.
///
/// Temporal plausibility is the only such finding: deciding whether an instant
/// is in the future needs a clock, and reading one inside [`classify`] would
/// make classification non-deterministic. Raised in review: routing that finding
/// through an early error meant the one untrustworthy observation the CLI caught
/// was also the only one that produced no durable receipt, which is backwards —
/// an operator needs the artifact most when the input could not be trusted.
pub(crate) fn invalidate(receipt: &mut Receipt, code: &str, message: impl Into<String>) {
    receipt.blockers.push(Blocker::new(code, message));
    receipt.blockers.sort();
    receipt.blockers.dedup();
    receipt.state = PublicState::Invalid;
}

/// Whether the registry affirmatively reports this identity live.
///
/// Deliberately strict. It takes either
///
/// - the gallery listing answering, the extension record confirming the
///   identity, and the versions endpoint publishing the subject version
///   together, or
/// - the exact versioned-file request returning a clean retrieval whose parsed
///   subject version, digest and byte length are on record — an individually
///   identity-bearing surface, because the plan addresses that request at this
///   namespace and extension alone.
///
/// Anything weaker — a bare `2xx`, an unparsed body — is not an affirmation and
/// must not be able to override a namespace answer, or a provider hiccup on
/// these surfaces would start masking real publisher problems. The point is to
/// detect a genuine contradiction, not to outvote.
fn extension_surfaces_affirm_presence(
    observation: &Observation,
    subject_version: Option<&str>,
) -> bool {
    let cells = &observation.cells;
    let present =
        |cell: Cell| observe(transport_for(observation, cell)) == CellObservation::Present;

    let listing_live = present(Cell::Listing);
    let record_live =
        present(Cell::ExtensionMetadata) && cells.extension_metadata.identity_matches == Some(true);
    let rows_live = present(Cell::VersionRows)
        && match (&cells.version_rows.versions, subject_version) {
            (Some(rows), Some(version)) => rows.iter().any(|row| row == version),
            _ => false,
        };

    if listing_live && record_live && rows_live {
        return true;
    }

    // A namespace denial beside a cleanly retrieved, fully parsed subject
    // package is the same contradiction the three-surface conjunction catches:
    // the registry cannot both lack the namespace and serve its package. The
    // identity claim is in the parsed payload (subject version, digest, byte
    // length), not in the bare status.
    let file = &cells.versioned_file;
    present(Cell::VersionedFile)
        && file.sha256.is_some()
        && file.byte_length.is_some()
        && match (file.version.as_deref(), subject_version) {
            (Some(retrieved), Some(expected)) => retrieved == expected,
            _ => false,
        }
}

/// Surfaces whose transport reported an affirmative `404` while their own parsed
/// payload affirms the thing that `404` denies, with what each one affirmed.
///
/// Sorted by [`Cell::ALL`] order, so the findings are deterministic.
fn absence_contradictions(observation: &Observation) -> Vec<(Cell, &'static str)> {
    let cells = &observation.cells;
    // `observe` maps 404 to ProvenAbsent, so this asks the same question the
    // classifier will ask rather than re-reading the raw status.
    let proven_absent =
        |cell: Cell| observe(transport_for(observation, cell)) == CellObservation::ProvenAbsent;

    let mut found = Vec::new();
    if proven_absent(Cell::Search) && cells.search.matched_identity == Some(true) {
        found.push((Cell::Search, "a search match for this exact identity"));
    }
    if proven_absent(Cell::NamespaceMetadata)
        && cells.namespace_metadata.namespace_present == Some(true)
    {
        found.push((Cell::NamespaceMetadata, "that the namespace is present"));
    }
    if proven_absent(Cell::ExtensionMetadata) {
        if cells.extension_metadata.identity_matches == Some(true) {
            found.push((Cell::ExtensionMetadata, "a matching extension identity"));
        }
        if cells.extension_metadata.versions.as_ref().is_some_and(|rows| !rows.is_empty()) {
            found.push((Cell::ExtensionMetadata, "published version rows"));
        }
    }
    if proven_absent(Cell::VersionRows)
        && cells.version_rows.versions.as_ref().is_some_and(|rows| !rows.is_empty())
    {
        found.push((Cell::VersionRows, "published version rows"));
    }
    if proven_absent(Cell::VersionedFile)
        && (cells.versioned_file.sha256.is_some()
            || cells.versioned_file.byte_length.is_some()
            || cells.versioned_file.version.is_some())
    {
        found.push((Cell::VersionedFile, "retrieved package bytes"));
    }
    found
}

/// What the search surface's published identity match is, and is not, scoped to.
///
/// The planned query reads one bounded page and does not paginate, so a `false`
/// says the subject was not on that page — never that the registry cannot
/// discover it. The classifier does not consume this value on any path; the
/// receipt publishes it, which is why the bound has to be published beside it.
fn search_scope(observation: &Observation) -> String {
    match observe(&observation.cells.search.transport) {
        CellObservation::Present => format!(
            "Search discoverability was assessed against a single bounded result page of at most \
             {SEARCH_PAGE_SIZE} results, so this surface cannot establish global discoverability \
             or its absence."
        ),
        _ => "Search discoverability was not established for this observation.".to_owned(),
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
    // The entire value of this receipt is that it describes a *current* state,
    // so the instant is parsed rather than merely required to be non-blank.
    // RFC 3339 also forces an explicit offset: a local timestamp cannot be
    // placed on a timeline by a later reader.
    if observation.observed_at.trim().is_empty() {
        findings.push(Blocker::new(
            "missing_observed_at",
            "observation carries no observation instant, so it cannot establish current state",
        ));
    } else if DateTime::parse_from_rfc3339(&observation.observed_at).is_err() {
        findings.push(Blocker::new(
            "malformed_observed_at",
            format!(
                "observed_at {:?} is not an RFC 3339 instant with an explicit offset",
                observation.observed_at
            ),
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
    // Syntax of the subject itself is covered by the per-entry loop below.
    if subject_version.is_none() {
        findings.push(Blocker::new(
            "missing_expected_version",
            "no expected version was supplied, so no versioned package could be addressed",
        ));
    }

    // Raised in review: property-name closure proves no field is *designed* to
    // carry a secret, but these free-text fields are copied verbatim into the
    // durable receipt, so their values need a boundary too.
    for (label, value) in [
        ("instrument.name", observation.instrument.name.as_str()),
        ("instrument.version", observation.instrument.version.as_str()),
        ("instrument.source_ref", observation.instrument.source_ref.as_str()),
    ] {
        if let Some(reason) = unsafe_reference(value) {
            findings.push(Blocker::new("unsafe_reference_value", format!("{label} {reason}")));
        }
    }
    for reference in &observation.expected.publication_refs {
        if let Some(reason) = unsafe_reference(reference) {
            findings.push(Blocker::new(
                "unsafe_reference_value",
                format!("a publication reference {reason}"),
            ));
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for expected in &observation.expected.versions {
        // Every entry, not just the subject: `expected` is copied verbatim into
        // the receipt, so a malformed non-subject version would emit a receipt
        // that violates its own published schema.
        if !valid_version(&expected.version) {
            findings.push(Blocker::new(
                "malformed_expected_version",
                format!(
                    "expected version {:?} is not a canonical version segment",
                    expected.version
                ),
            ));
        }
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
                    // The observed URL is producer-supplied, off-plan by
                    // construction, and this blocker is durable, so it goes
                    // through the same boundary as every other free-text
                    // value — with no planned match, its query and fragment
                    // are stripped. The planned URL is derived here.
                    publishable_url(&transport.url, Some(planned.url.as_str())),
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
        // The other half of the same contradiction: a request that was never
        // attempted cannot have produced an error, a redirect, or bytes. Raised
        // in review through its consequence — such a cell classified as an
        // attempted failure and stopped counting as an instrument gap.
        if transport.outcome == TransportOutcome::NotAttempted
            && (transport.error_kind.is_some()
                || transport.truncated
                || transport.redirects > 0
                || transport.response_bytes.is_some())
        {
            findings.push(Blocker::new(
                "unattempted_request_reports_activity",
                format!(
                    "{} reports an outcome it never attempted alongside evidence of an attempt",
                    cell.key()
                ),
            ));
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
        // The plan declares three bounds. Bytes and redirects were already
        // checkable from the record; the timeout was declared and then never
        // evidenced, so a consumer had to take that third bound on faith.
        if matches!(
            transport.outcome,
            TransportOutcome::HttpResponse | TransportOutcome::TransportError
        ) && transport.elapsed_ms.is_none()
        {
            findings.push(Blocker::new(
                "unmeasured_duration",
                format!(
                    "{} was attempted without recording how long it took, so its timeout budget \
                     is unevidenced",
                    cell.key()
                ),
            ));
        }
        if let Some(elapsed) = transport.elapsed_ms
            && elapsed > planned.timeout_ms
            && transport.error_kind != Some(ErrorKind::Timeout)
        {
            findings.push(Blocker::new(
                "timeout_budget_exceeded",
                format!(
                    "{} ran {elapsed}ms past the {}ms budget without reporting a timeout",
                    cell.key(),
                    planned.timeout_ms
                ),
            ));
        }

        // A 2xx that never reported how much it read cannot evidence a bounded
        // read, and without it the budget check below silently does nothing.
        if transport.outcome == TransportOutcome::HttpResponse
            && transport.status.is_some_and(|status| (200..300).contains(&status))
            && transport.response_bytes.is_none()
        {
            findings.push(Blocker::new(
                "unmeasured_response",
                format!(
                    "{} returned a success status without reporting a response size, so no \
                     bounded read is evidenced",
                    cell.key()
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

    // Raised in review: a `404` is the only status this classifier treats as
    // affirmative absence, and it was accepted no matter what the instrument
    // claimed to have parsed out of the same response. An observation whose
    // transport says the resource is not there while its own payload names a
    // matching identity, published versions or retrieved package bytes is
    // describing two different worlds. That is an untrustworthy instrument, not
    // a registry fact, so it is refused rather than resolved in either
    // direction — least of all toward absence, which is the conclusion this
    // module exists to make hard to reach.
    for (cell, affirmed) in absence_contradictions(observation) {
        findings.push(Blocker::new(
            "contradictory_absence_payload",
            format!(
                "the {} surface reported an affirmative 404 while also reporting {affirmed}",
                cell.key()
            ),
        ));
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
    // The digest is only over what was actually read. If the parsed package size
    // and the transport's byte count disagree, the digest describes something
    // other than the complete public package.
    if let Some(length) = file.byte_length
        && let Some(read) = file.transport.response_bytes
        && length != read
    {
        findings.push(Blocker::new(
            "package_byte_length_mismatch",
            format!(
                "the versioned package reports {length} bytes but {read} were read, so the \
                 digest does not cover the complete public package"
            ),
        ));
    }

    findings
}

/// The state machine.
fn classify_state(
    observation: &Observation,
    subject_version: Option<&str>,
    public_bytes: Option<&PublicBytes>,
) -> (PublicState, Vec<Blocker>) {
    // No limitations are produced here. Every limitation this receipt can carry
    // is unconditional and stated in `classify`, so returning a third value that
    // is provably always empty would promise a capability this function lacks.
    let mut blockers = Vec::new();

    let listing = observe(&observation.cells.listing.transport);
    // The search cell is deliberately absent from this function: discoverability
    // is reported by the receipt and never raises or lowers the classification.
    let namespace_metadata = observe(&observation.cells.namespace_metadata.transport);
    let extension_metadata = observe(&observation.cells.extension_metadata.transport);
    let version_rows = observe(&observation.cells.version_rows.transport);
    let versioned_file = observe(&observation.cells.versioned_file.transport);

    // Raised in review: this diagnosis used to be returned on the namespace cell
    // alone, before any cross-surface check ran. A namespace `404` beside a live
    // listing, a matching extension record and a retrieved package was reported
    // as a publisher problem while four surfaces said the opposite. A namespace
    // that does not resolve cannot be serving its extension, so that combination
    // is contradictory evidence, not a narrower diagnosis.
    let extension_is_live = extension_surfaces_affirm_presence(observation, subject_version);

    // A namespace that is gone or renamed is the narrower diagnosis; reporting
    // it as a missing extension would send the incident down the wrong path.
    if namespace_metadata == CellObservation::ProvenAbsent {
        if extension_is_live {
            blockers.push(Blocker::new(
                "contradictory_registry_scope",
                "the namespace endpoint reports the namespace absent while the extension surfaces \
                 report it live; the registry answers cannot both be true",
            ));
            return (PublicState::ProviderNotProven, blockers);
        }
        blockers.push(Blocker::new(
            "namespace_absent",
            "the namespace itself does not resolve; extension-level absence is not the diagnosis",
        ));
        return (PublicState::NamespaceOrPublisherProblem, blockers);
    }
    if namespace_metadata == CellObservation::Present {
        match observation.cells.namespace_metadata.namespace_present {
            Some(false) => {
                if extension_is_live {
                    blockers.push(Blocker::new(
                        "contradictory_registry_scope",
                        "the namespace endpoint did not confirm the namespace while the extension \
                         surfaces report it live; the registry answers cannot both be true",
                    ));
                    return (PublicState::ProviderNotProven, blockers);
                }
                blockers.push(Blocker::new(
                    "namespace_not_confirmed",
                    "the namespace endpoint responded without confirming this namespace",
                ));
                return (PublicState::NamespaceOrPublisherProblem, blockers);
            }
            None => {
                // A 2xx whose body was never parsed into an identity claim is
                // not affirmative namespace resolution. Letting it count as one
                // would license "the extension is gone but the namespace is
                // fine" from evidence that established neither.
                blockers.push(Blocker::new(
                    "namespace_identity_not_parsed",
                    "the namespace endpoint responded but no namespace identity was parsed from \
                     it, so namespace resolution is unproven",
                ));
                return (PublicState::ProviderNotProven, blockers);
            }
            Some(true) => {}
        }
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
        return (PublicState::ProviderNotProven, blockers);
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
            return (PublicState::AvailableIdentityNotProven, blockers);
        }

        // A versions endpoint that flatly denies any published rows, beside a
        // listing and a metadata record that both resolve, is definite but
        // mutually inconsistent. Falling through here would let that denial
        // pass unrecorded straight into the strongest claim.
        if version_rows == CellObservation::ProvenAbsent {
            blockers.push(Blocker::new(
                "contradictory_registry_evidence",
                "the listing and extension record resolve while the versions endpoint reports \
                 none; no single availability conclusion is supported",
            ));
            return (PublicState::ProviderNotProven, blockers);
        }
        // The extension record publishes versions too. Review showed only the
        // versions endpoint was being checked, so an extension record listing a
        // conflicting inventory passed unexamined.
        if let Some(subject) = subject_version
            && extension_metadata == CellObservation::Present
            && let Some(listed) = &observation.cells.extension_metadata.versions
            && !listed.iter().any(|row| row == subject)
        {
            blockers.push(Blocker::new(
                "subject_version_absent_from_extension_record",
                format!(
                    "the extension record does not list the subject version {subject:?} among \
                     its published versions"
                ),
            ));
            return (PublicState::AvailableIdentityNotProven, blockers);
        }

        if version_rows == CellObservation::Present {
            // Symmetric with the identity_matches requirement above: on the path
            // to the strongest claim, a surface that answered but whose answer
            // was never parsed proves nothing. Skipping the membership check
            // because the rows are missing would turn absent data into consent.
            let Some(rows) = &observation.cells.version_rows.versions else {
                blockers.push(Blocker::new(
                    "version_rows_not_parsed",
                    "the versions endpoint responded but no version rows were parsed from it, so \
                     publication of the subject version is unproven",
                ));
                return (PublicState::AvailableIdentityNotProven, blockers);
            };
            if let Some(subject) = subject_version
                && !rows.iter().any(|row| row == subject)
            {
                blockers.push(Blocker::new(
                    "subject_version_not_published",
                    format!(
                        "the published version rows do not list the subject version {subject:?}"
                    ),
                ));
                return (PublicState::AvailableIdentityNotProven, blockers);
            }
        }

        let Some(bytes) = public_bytes else {
            blockers.push(Blocker::new(
                "public_package_not_retrieved",
                "the listing resolves but the versioned package was not retrieved, so exact \
                 public bytes are unproven",
            ));
            return (PublicState::AvailableIdentityNotProven, blockers);
        };

        let Some(subject) = subject_version else {
            blockers.push(Blocker::new(
                "missing_expected_version",
                "no subject version to compare public bytes against",
            ));
            return (PublicState::AvailableIdentityNotProven, blockers);
        };
        if bytes.version != subject {
            blockers.push(Blocker::new(
                "public_version_mismatch",
                format!(
                    "the retrieved package reports version {:?}, not the subject {subject:?}",
                    bytes.version
                ),
            ));
            return (PublicState::AvailableIdentityNotProven, blockers);
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
                (PublicState::AvailableIdentityNotProven, blockers)
            }
            Some(expected) if expected == bytes.sha256 => {
                // The observed digest and the expected digest arrive through the
                // same unbound document, so their equality is self-attestable: a
                // producer that can write one can write the other. An earlier
                // revision tried to close this with a declared `authority`
                // reference, but a declared-and-unverified reference is the same
                // hole under a new name — review demonstrated `available_exact`
                // from an authority naming a file that does not exist.
                //
                // Establishing that public bytes are the *approved* bytes needs a
                // resolved candidate authority, which this tool does not have and
                // must not pretend to. So the honest ceiling for a purely
                // observational input is that the identity was observed, not that
                // it was approved. `available_exact` stays in the vocabulary for
                // #9138, which owns candidate identity; it is not reachable here.
                blockers.push(Blocker::new(
                    "exact_approval_requires_verified_authority",
                    "the public digest matches the expected one, but both arrive through this \
                     observation and nothing here verifies the expected digest against a \
                     resolved candidate authority; observed identity is not proven approval",
                ));
                (PublicState::AvailableIdentityNotProven, blockers)
            }
            Some(expected) => {
                blockers.push(Blocker::new(
                    "public_digest_mismatch",
                    format!(
                        "public package digest {} does not match the expected {expected}",
                        bytes.sha256
                    ),
                ));
                (PublicState::AvailableIdentityNotProven, blockers)
            }
        }
    } else if listing == CellObservation::ProvenAbsent {
        if versioned_file == CellObservation::Present {
            // The state name asserts the package is retrievable, so it has to be
            // backed by bytes actually retrieved. A clean 2xx that yielded no
            // package identity would otherwise produce a receipt claiming
            // retrievability beside a null `public_bytes`.
            let Some(bytes) = public_bytes else {
                blockers.push(Blocker::new(
                    "package_response_not_parsed",
                    "the versioned package endpoint responded but no package identity was parsed \
                     from it, so retrievability is unproven",
                ));
                return (PublicState::ProviderNotProven, blockers);
            };
            if subject_version.is_some_and(|subject| subject != bytes.version) {
                blockers.push(Blocker::new(
                    "public_version_mismatch",
                    format!(
                        "the retrieved package reports version {:?}, not the subject",
                        bytes.version
                    ),
                ));
                return (PublicState::AvailableIdentityNotProven, blockers);
            }
            blockers.push(Blocker::new(
                "listing_absent_with_retrievable_package",
                "the gallery listing does not resolve while the versioned package still does",
            ));
            return (PublicState::ListingMissingVersionRetrievable, blockers);
        }
        if metadata_reachable {
            blockers.push(Blocker::new(
                "listing_absent_with_reachable_metadata",
                "the gallery listing does not resolve while extension metadata still does; the \
                 object is reachable but not publicly presented",
            ));
            return (PublicState::AvailableIdentityNotProven, blockers);
        }
        if extension_metadata == CellObservation::ProvenAbsent
            && versioned_file == CellObservation::ProvenAbsent
        {
            blockers.push(Blocker::new(
                "extension_absent_on_every_surface",
                "listing, extension metadata and the versioned package are each independently \
                 absent while the namespace still resolves",
            ));
            return (PublicState::ExtensionMissing, blockers);
        }
        blockers.push(Blocker::new(
            "contradictory_registry_evidence",
            "the registry returned definite but mutually inconsistent answers; no single \
             availability conclusion is supported",
        ));
        (PublicState::ProviderNotProven, blockers)
    } else {
        blockers.push(Blocker::new(
            "contradictory_registry_evidence",
            "the registry returned definite but mutually inconsistent answers; no single \
             availability conclusion is supported",
        ));
        (PublicState::ProviderNotProven, blockers)
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
fn build_cells(observation: &Observation, plan: Option<&ProbePlan>) -> Vec<CellResult> {
    Cell::ALL
        .into_iter()
        .map(|cell| {
            let transport = transport_for(observation, cell);
            let planned = plan.and_then(|plan| plan.request(cell));
            CellResult {
                cell: cell.key(),
                url: publishable_url(&transport.url, planned.map(|request| request.url.as_str())),
                method: "GET",
                observation: observe(transport),
                status: transport.status,
                redirects: transport.redirects,
                response_bytes: transport.response_bytes,
                truncated: transport.truncated,
                error_kind: transport.error_kind.map(ErrorKind::key),
                elapsed_ms: transport.elapsed_ms,
                identity_match: identity_match(observation, cell),
                versions: published_versions(observation, cell),
            }
        })
        .collect()
}

/// Whether a surface affirmed this exact identity, for the three that can.
///
/// The other three answer a different question — a listing page, a version
/// table, a package file — so they report `None` rather than a default that
/// would read as a denial.
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

/// Every surface paired with its transport record, in the fixed cell order.
fn transports(observation: &Observation) -> Vec<(Cell, &Transport)> {
    Cell::ALL.into_iter().map(|cell| (cell, transport_for(observation, cell))).collect()
}

/// The transport record belonging to one surface.
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

/// Sentinel written in place of a value that must not be published.
const REDACTED: &str = "<redacted>";

/// A request URL, safe to write into a durable receipt.
///
/// Raised in review: `transport.url` is producer-supplied and was copied
/// verbatim into both the receipt's per-surface record and the
/// `unplanned_request_url` blocker, so the one field describing a request that
/// left the sanctioned plan — exactly the case where the value is least
/// trustworthy — was published unexamined. It now crosses the same boundary as
/// `instrument` and `publication_refs`.
///
/// A URL that matches the sanctioned plan exactly is plan-derived and is
/// published verbatim, query included: the planned search request legitimately
/// carries one. A producer-supplied URL that differs from the plan is off-plan
/// evidence, so beyond the `unsafe_reference` redaction its query and fragment
/// are stripped — `?token=…` and `#access_token=…` carry credentials that the
/// userinfo check cannot see, and a durable receipt must never publish them.
fn publishable_url(url: &str, planned: Option<&str>) -> String {
    if planned == Some(url) {
        return url.to_owned();
    }
    match unsafe_reference(url) {
        Some(_) => REDACTED.to_owned(),
        // A URL that is all query/fragment (`"?token=…"`) strips to nothing,
        // and the receipt's URL contract requires a non-empty value — fall
        // back to redaction rather than publish an empty cell.
        None => url
            .split(['?', '#'])
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(REDACTED)
            .to_owned(),
    }
}

/// Strip unpublishable values from the instrument identity.
///
/// Flagging an unsafe value is not a boundary on its own: the receipt is a
/// durable, shareable artifact, so a blocker sitting beside a retained
/// credential still publishes the credential. The finding is recorded in
/// `blockers`; the value itself never reaches the file.
fn publishable_instrument(mut instrument: Instrument) -> Instrument {
    for field in [&mut instrument.name, &mut instrument.version, &mut instrument.source_ref] {
        if unsafe_reference(field).is_some() {
            *field = REDACTED.to_owned();
        }
    }
    instrument
}

/// Strip unpublishable values from the expected identity.
fn publishable_expected(mut expected: Expected) -> Expected {
    for reference in &mut expected.publication_refs {
        if unsafe_reference(reference).is_some() {
            *reference = REDACTED.to_owned();
        }
    }
    expected
}

/// Why a free-text reference must not cross the publication boundary.
///
/// These values are retained verbatim in a durable, shareable receipt, so the
/// rule is deliberately narrow: a bounded, control-character-free reference that
/// is either plain text or an `https` URL without userinfo. That rejects
/// `file://` and other local-resource schemes, credential-bearing URLs, Windows
/// paths, and absolute POSIX paths, none of which is a publication reference.
fn unsafe_reference(value: &str) -> Option<&'static str> {
    const MAX_REFERENCE_BYTES: usize = 200;

    if value.trim().is_empty() {
        return Some("is empty");
    }
    if value.len() > MAX_REFERENCE_BYTES {
        return Some("exceeds the reference length budget");
    }
    if value.chars().any(char::is_control) {
        return Some("contains control characters");
    }
    if value.contains('\\') {
        return Some("looks like a filesystem path");
    }
    if value.starts_with('/') || value.starts_with('~') {
        return Some("looks like an absolute local path");
    }
    if value.contains('@') {
        return Some("could carry credential userinfo");
    }
    if let Some((scheme, _)) = value.split_once("://")
        && scheme != "https"
    {
        return Some("uses a scheme other than https");
    }
    // A bare Windows drive reference (`C:\...` is already rejected above, but
    // `C:/...` is not a path separator case).
    let mut characters = value.chars();
    if let (Some(drive), Some(':'), Some('/')) =
        (characters.next(), characters.next(), characters.next())
        && drive.is_ascii_alphabetic()
    {
        return Some("looks like an absolute local path");
    }
    None
}

/// Whether a value is a lowercase hexadecimal SHA-256.
///
/// Case is significant rather than normalised: an uppercase digest is refused
/// outright, so one digest can never reach comparison in two spellings.
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
