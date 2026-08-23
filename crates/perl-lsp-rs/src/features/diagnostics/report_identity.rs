//! Complete pull-diagnostic report-subject identity (#7480).
//!
//! A pulled-diagnostics result ID names one complete accepted report subject:
//! the exact evaluation inputs (logical source revision, owning folder
//! authority, accepted critic policy, project-fact state, resolver
//! environment) plus the behavior-bearing negotiated wire projection
//! (position encoding, markup-message support). Two pulls may return
//! `Unchanged` only when every one of those fragments is identical and still
//! current; any movement yields a fresh `full` report with a new result ID.
//!
//! Composition reuses the landed typed identities from `perl-lsp-rs-core`
//! (#7201 substrate: source/policy/facts/schemas) and adds the transport-owned
//! fragments this layer owns today (projection profile, resolver roots,
//! external-critic admission). When #9942/#9945 land, their snapshot and
//! projector identities replace the provisional fragments inside this same
//! subject under a bumped schema version — old client-held IDs fail
//! [`PullReportResultId::from_wire`] and produce `full`, never `unchanged`.
//!
//! The public spelling is bounded, opaque, versioned and free of paths,
//! configuration values, source text and environment data: everything is
//! folded through the repository's domain-separated SHA-256 content-digest
//! authority before it reaches the wire.

use std::collections::BTreeSet;

use perl_lsp_rs_core::config::CriticEngine;
use perl_lsp_rs_core::tooling::perl_critic::{
    CRITIC_IDENTITY_SCHEMA_VERSION, CriticPolicyIdentity, CriticPolicyIdentityError,
    DiagnosticFactIdentity, DiagnosticResultIdentityInput, DiagnosticResultSchemaVersions,
    DiagnosticSourceIdentity, NativeCriticProfile,
};
use perl_source_identity::{ContentDigest, LogicalSourceId, ProjectId, WorkspaceRootId};

use super::PullDiagnosticsContext;

/// Schema/domain version of this composer. Bump whenever the set of
/// load-bearing fragments changes so prior client-held IDs stop parsing and
/// every report degrades honestly to `full`.
pub const PULL_REPORT_IDENTITY_SCHEMA_VERSION: u16 = 1;

/// Wire prefix of a composed pull-report result ID.
const PULL_REPORT_IDENTITY_PREFIX: &str = "diagnostic-pull-report.v";

/// Stable project scope for folder-authority IDs. Session-stable result IDs
/// do not need cross-machine identity; the project name only namespaces the
/// root keys below it.
const PULL_IDENTITY_PROJECT: &str = "perl-lsp";

/// Version pins for behavior-bearing catalogs whose movement must invalidate
/// prior results. Each pin is owned here until its future authority lands
/// (#9942 evaluation/result contract, #9945 wire projector) and is bumped by
/// the change that alters the corresponding behavior.
const RULE_CATALOG_SCHEMA_VERSION: u32 = 1;
const SUPPRESSION_CONTRACT_SCHEMA_VERSION: u16 = 1;
const PROJECTION_WIRE_SCHEMA_VERSION: u16 = 1;
const REMEDIATION_WIRE_SCHEMA_VERSION: u16 = 1;

/// Domain tag binding the legacy built-in analyzer's effective policy. The
/// built-in analyzer takes no user configuration beyond the encoded policy
/// fields, so a stable domain digest is its complete policy identity.
const LEGACY_BUILTIN_POLICY_DOMAIN: &str = "perl-lsp:pull-legacy-builtin-policy:v1";

/// Behavior-bearing negotiated wire-projection state.
///
/// Diagnostics are projected under the negotiated position encoding and the
/// negotiated message-markup support; movement in either changes the
/// client-visible items and therefore the report identity. This fragment is
/// the transport-owned stand-in for #9945's complete profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticProjectionFragment {
    /// Negotiated position encoding for wire ranges.
    pub position_encoding: PullPositionEncoding,
    /// Whether messages may be projected as `MarkupContent` rather than plain
    /// strings.
    pub markup_messages: bool,
}

/// Negotiated position encoding spellings that affect projected ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullPositionEncoding {
    /// UTF-8 code-unit positions.
    Utf8,
    /// UTF-16 code-unit positions (LSP default).
    Utf16,
}

impl PullPositionEncoding {
    fn as_token(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::Utf16 => "utf16",
        }
    }
}

/// Why a valid report cannot carry a reusable result ID.
///
/// A report in this state is still returned in full — LSP result IDs are
/// optional — but it must never come back as `Unchanged`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotReusable {
    /// No owning workspace/folder authority could be established for the
    /// document, so the logical source identity cannot be formed.
    MissingRootAuthority,
    /// The accepted critic policy contradicts its engine's requirements.
    PolicyIncomplete(CriticPolicyIdentityError),
}

impl std::fmt::Display for NotReusable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRootAuthority => {
                f.write_str("no owning workspace/root authority for the document")
            }
            Self::PolicyIncomplete(error) => {
                write!(f, "critic policy identity incomplete: {error}")
            }
        }
    }
}

/// Opaque deterministic result ID for one complete pull-report subject.
///
/// Spelled `diagnostic-pull-report.v<schema>-sha256:<64 lowercase hex>`.
/// Parsing rejects other schema versions and malformed bodies fail-closed, so
/// an old or unknown client-held ID can never authorize `Unchanged`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullReportResultId(String);

impl PullReportResultId {
    /// Parse a previously returned result ID under the current schema.
    ///
    /// Returns `None` for anything not produced by the current schema version
    /// — including IDs minted by older schemas or foreign composers — so such
    /// IDs degrade to a `full` report instead of being echoed as `Unchanged`.
    #[must_use]
    pub fn from_wire(raw: &str) -> Option<Self> {
        let expected_prefix =
            format!("{PULL_REPORT_IDENTITY_PREFIX}{PULL_REPORT_IDENTITY_SCHEMA_VERSION}-");
        let body = raw.strip_prefix(&expected_prefix)?;
        let hex = body.strip_prefix("sha256:")?;
        (hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
            .then(|| Self(raw.to_owned()))
    }

    /// Result-ID string suitable for LSP `resultId` fields.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the identity and return its result-ID string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

/// One complete accepted pull-report subject.
///
/// Constructed from a [`PullDiagnosticsContext`] plus the document inputs;
/// composed into an opaque result ID via [`Self::compose`]. Construction is
/// fallible because a subject missing a required authority is exactly the
/// not-reusable case: callers return the report in full without an ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullReportSubject {
    root_id: WorkspaceRootId,
    relative_path: String,
    content_digest: ContentDigest,
    document_generation: Option<u64>,
    engine: CriticEngine,
    profile: NativeCriticProfile,
    severity: u8,
    include: BTreeSet<String>,
    exclude: BTreeSet<String>,
    legacy_policy_digest: Option<ContentDigest>,
    facts_generation: Option<u64>,
    resolver_roots: BTreeSet<String>,
    projection: DiagnosticProjectionFragment,
    critic_enabled: bool,
}

/// Assemble the complete subject for one document report.
///
/// Mirrors the collection path's actual configuration interpretation so the
/// identity can never describe a different computation than the one performed
/// (for example the native profile fallback to `Strict`). Returns
/// [`NotReusable`] when a required authority is absent.
pub fn pull_report_subject(
    uri: &str,
    content: &str,
    document_generation: Option<u64>,
    context: &PullDiagnosticsContext,
) -> Result<PullReportSubject, NotReusable> {
    let Some(root_key) = context.identity_root_key.as_deref() else {
        return Err(NotReusable::MissingRootAuthority);
    };

    let project = ProjectId::from_canonical_name(PULL_IDENTITY_PROJECT);
    let root_id = WorkspaceRootId::from_project_and_root_key(&project, root_key);
    let relative_path = root_relative_path(uri);

    // Mirror `add_native_critic_diagnostics`: the effective native profile is
    // the configured spelling parsed leniently with the same Strict fallback.
    // Under the legacy engine the native profile is inert and pinned so
    // unrelated profile settings cannot churn legacy-engine identities.
    let (profile, legacy_policy_digest) = match context.critic_engine {
        CriticEngine::Native => (
            NativeCriticProfile::parse_legacy(&context.native_critic_profile)
                .unwrap_or(NativeCriticProfile::Strict),
            None,
        ),
        CriticEngine::Legacy => (
            NativeCriticProfile::Recommended,
            Some(ContentDigest::of_bytes(LEGACY_BUILTIN_POLICY_DOMAIN.as_bytes())),
        ),
    };

    Ok(PullReportSubject {
        content_digest: ContentDigest::of_bytes(content.as_bytes()),
        severity: context.perlcritic_severity.clamp(1, 5) as u8,
        include: context.native_critic_include.iter().cloned().collect(),
        exclude: context.native_critic_exclude.iter().cloned().collect(),
        resolver_roots: context.include_paths.iter().cloned().collect(),
        root_id,
        relative_path,
        document_generation,
        engine: context.critic_engine,
        profile,
        facts_generation: context.facts_generation,
        projection: context.projection,
        critic_enabled: context.perlcritic_enabled,
        legacy_policy_digest,
    })
}

/// Compose the reusable result ID for one document report, or `None` when the
/// report is valid but not safely reusable.
///
/// A not-ready evaluation (`ready == false`, e.g. a pending-parse gap) never
/// carries a reusable ID even when its subject would compose: a partial or
/// superseded subject must never masquerade as clean-unchanged (#7480).
pub(crate) fn compose_report_identity(
    uri: &str,
    content: &str,
    document_generation: Option<u64>,
    context: &PullDiagnosticsContext,
    ready: bool,
) -> Option<PullReportResultId> {
    if !ready {
        return None;
    }
    match pull_report_subject(uri, content, document_generation, context) {
        Ok(subject) => match subject.compose() {
            Ok(id) => Some(id),
            Err(reason) => {
                tracing::debug!(uri, reason = %reason, "pull diagnostics: result ID not reusable");
                None
            }
        },
        Err(reason) => {
            tracing::debug!(uri, reason = %reason, "pull diagnostics: result ID not reusable");
            None
        }
    }
}

impl PullReportSubject {
    /// Compose the opaque public result ID for this subject.
    ///
    /// Deterministic across processes for equal subjects; collision-resistant
    /// through SHA-256 over a length-prefixed canonical encoding that embeds
    /// the core substrate identity (#7201) plus this layer's fragments.
    pub fn compose(&self) -> Result<PullReportResultId, NotReusable> {
        // Configuration generations have no independent counter authority yet
        // (#6736/#7064 own one); the pinned 0 scopes the field explicitly while
        // every behavior-bearing configuration field is encoded on its own.
        const NO_CONFIGURATION_GENERATION_AUTHORITY: u64 = 0;

        let policy = CriticPolicyIdentity::new(
            self.root_id.clone(),
            NO_CONFIGURATION_GENERATION_AUTHORITY,
            self.engine,
            self.profile,
            self.severity,
            self.include.clone(),
            self.exclude.clone(),
            self.legacy_policy_digest.clone(),
        )
        .map_err(NotReusable::PolicyIncomplete)?;

        let facts = match self.facts_generation {
            Some(generation) => {
                DiagnosticFactIdentity::Live { workspace: self.root_id.clone(), generation }
            }
            None => DiagnosticFactIdentity::Unavailable,
        };
        let inner = DiagnosticResultIdentityInput::new(
            DiagnosticSourceIdentity::new(
                LogicalSourceId::from_root_and_path(&self.root_id, &self.relative_path),
                self.content_digest.clone(),
                self.document_generation,
            ),
            policy,
            facts,
            DiagnosticResultSchemaVersions::new(
                RULE_CATALOG_SCHEMA_VERSION,
                CRITIC_IDENTITY_SCHEMA_VERSION,
                SUPPRESSION_CONTRACT_SCHEMA_VERSION,
                PROJECTION_WIRE_SCHEMA_VERSION,
                REMEDIATION_WIRE_SCHEMA_VERSION,
            ),
        )
        .compose();

        let mut canonical = String::new();
        push_str(&mut canonical, "identity_schema", PULL_REPORT_IDENTITY_V1_TAG);
        push_str(&mut canonical, "substrate", inner.as_str());
        push_str(&mut canonical, "position_encoding", self.projection.position_encoding.as_token());
        push_u64(&mut canonical, "markup_messages", u64::from(self.projection.markup_messages));
        push_u64(&mut canonical, "critic_enabled", u64::from(self.critic_enabled));
        push_set(&mut canonical, "resolver_roots", &self.resolver_roots);

        let digest = ContentDigest::of_bytes(canonical.as_bytes());
        Ok(PullReportResultId(format!(
            "{PULL_REPORT_IDENTITY_PREFIX}{PULL_REPORT_IDENTITY_SCHEMA_VERSION}-{}",
            digest.as_wire()
        )))
    }
}

/// Domain tag for the outer composition, kept distinct from the substrate's
/// own schema field so the two layers cannot be confused.
const PULL_REPORT_IDENTITY_V1_TAG: &str = "perl-lsp:pull-report-identity:v1";

/// Reduce a document URI to a stable logical path spelling.
///
/// Forward-slash separated, no leading slash. Documents outside any root fall
/// back to their absolute URI path; the value only ever feeds the digested
/// logical-source ID and never appears in a public result ID.
fn root_relative_path(uri: &str) -> String {
    let path = url::Url::parse(uri)
        .ok()
        .map(|parsed| parsed.path().trim_start_matches('/').to_string())
        .unwrap_or_else(|| uri.to_string());
    path.replace('\\', "/")
}

fn push_set(output: &mut String, name: &str, values: &BTreeSet<String>) {
    push_u64(output, name, values.len() as u64);
    for value in values {
        push_str(output, "item", value);
    }
}

fn push_u64(output: &mut String, name: &str, value: u64) {
    push_str(output, name, &value.to_string());
}

fn push_str(output: &mut String, name: &str, value: &str) {
    // Length-prefix every token so field boundaries are unambiguous.
    output.push_str(&name.len().to_string());
    output.push(':');
    output.push_str(name);
    output.push(';');
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push(';');
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{DiagnosticProjectionFragment, PullPositionEncoding, *};
    use crate::features::diagnostics::PullDiagnosticsContext;
    use perl_lsp_rs_core::config::CriticEngine;

    fn projection(encoding: PullPositionEncoding, markup: bool) -> DiagnosticProjectionFragment {
        DiagnosticProjectionFragment { position_encoding: encoding, markup_messages: markup }
    }

    fn context_with(root: Option<&str>) -> PullDiagnosticsContext {
        let mut context = PullDiagnosticsContext::new();
        context.identity_root_key = root.map(str::to_string);
        // Live fact-store state for the baseline subject.
        context.facts_generation = Some(13);
        context.projection = projection(PullPositionEncoding::Utf16, false);
        context
    }

    fn subject_for(
        context: &PullDiagnosticsContext,
        uri: &str,
        content: &str,
    ) -> PullReportSubject {
        pull_report_subject(uri, content, Some(3), context)
            .expect("test context must form a complete subject")
    }

    const URI_A: &str = "file:///ws-a/lib/Mod.pm";
    const CONTENT: &str = "my $x = 1;\n";

    #[test]
    fn identical_subjects_compose_identical_ids() {
        let context = context_with(Some("/tmp/ws-a"));
        let first = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        let second = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn composed_ids_parse_under_current_schema_only() {
        let context = context_with(Some("/tmp/ws-a"));
        let id = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        assert_eq!(PullReportResultId::from_wire(id.as_str()), Some(id));

        assert!(PullReportResultId::from_wire("d34db33f").is_none(), "bare md5-style ID");
        assert!(
            PullReportResultId::from_wire(
                "diagnostic-result.v2-sha256:0000000000000000000000000000000000000000000000000000000000000000"
            )
            .is_none(),
            "foreign substrate composer ID"
        );
        assert!(
            PullReportResultId::from_wire(
                "diagnostic-pull-report.v99-sha256:0000000000000000000000000000000000000000000000000000000000000000"
            )
            .is_none(),
            "future schema version"
        );
        assert!(
            PullReportResultId::from_wire(
                "diagnostic-pull-report.v1-sha256:ABCDEF0000000000000000000000000000000000000000000000000000000000"
            )
            .is_none(),
            "uppercase hex body"
        );
        assert!(
            PullReportResultId::from_wire("diagnostic-pull-report.v1-sha256:short").is_none(),
            "malformed body"
        );
    }

    #[test]
    fn every_load_bearing_fragment_moves_the_id() {
        let baseline_context = context_with(Some("/tmp/ws-a"));
        let baseline = subject_for(&baseline_context, URI_A, CONTENT).compose().ok().unwrap();

        // Owning root authority.
        let moved =
            subject_for(&context_with(Some("/tmp/ws-b")), URI_A, CONTENT).compose().ok().unwrap();
        assert_ne!(baseline, moved, "two roots with equal bytes must not share an ID");

        // Content revision.
        let moved = subject_for(&baseline_context, URI_A, "my $x = 2;\n").compose().ok().unwrap();
        assert_ne!(baseline, moved, "source edit must move the ID");

        // Document generation (same bytes, later instance).
        let later_instance =
            pull_report_subject(URI_A, CONTENT, Some(4), &baseline_context).ok().unwrap();
        assert_ne!(baseline, later_instance.compose().ok().unwrap());

        // Engine selection.
        let mut context = baseline_context.clone();
        context.critic_engine = CriticEngine::Legacy;
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        // Severity.
        let mut context = baseline_context.clone();
        context.perlcritic_severity = 4;
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        // Native profile spelling.
        let mut context = baseline_context.clone();
        context.native_critic_profile = "strict".to_string();
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        // Include/exclude rule sets.
        let mut context = baseline_context.clone();
        context.native_critic_include = vec!["native.testing.require_use_strict".to_string()];
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        // Fact-store availability and generation.
        let mut context = baseline_context.clone();
        context.facts_generation = None;
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());
        let mut context = baseline_context.clone();
        context.facts_generation = Some(9);
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        // Resolver environment.
        let mut context = baseline_context.clone();
        context.include_paths = vec!["/tmp/ws-a/lib".to_string(), "/opt/perl5lib".to_string()];
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());
        let mut reordered = baseline_context.clone();
        reordered.include_paths = vec!["/opt/perl5lib".to_string(), "/tmp/ws-a/lib".to_string()];
        assert_eq!(
            subject_for(&context, URI_A, CONTENT).compose().ok().unwrap(),
            subject_for(&reordered, URI_A, CONTENT).compose().ok().unwrap(),
            "resolver roots are a set: order must not matter"
        );

        // Projection profile: position encoding and markup support.
        let mut context = baseline_context.clone();
        context.projection = projection(PullPositionEncoding::Utf8, false);
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());
        let mut context = baseline_context.clone();
        context.projection = projection(PullPositionEncoding::Utf16, true);
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        // External-critic admission state.
        let mut context = baseline_context.clone();
        context.perlcritic_enabled = false;
        assert_ne!(baseline, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        // Logical document identity: equal bytes and counters, different path.
        let moved = subject_for(&baseline_context, "file:///ws-a/lib/Other.pm", CONTENT)
            .compose()
            .ok()
            .unwrap();
        assert_ne!(baseline, moved);
    }

    #[test]
    fn missing_root_authority_is_not_reusable() {
        let context = context_with(None);
        assert_eq!(
            pull_report_subject(URI_A, CONTENT, Some(1), &context),
            Err(NotReusable::MissingRootAuthority)
        );
    }

    #[test]
    fn public_id_is_bounded_and_path_free() {
        let context = context_with(Some("/tmp/ws-a/private-root-name"));
        let id = pull_report_subject(URI_A, CONTENT, Some(1), &context)
            .ok()
            .unwrap()
            .compose()
            .ok()
            .unwrap()
            .into_string();

        assert!(id.len() < 128, "public ID must stay bounded: {id}");
        assert!(!id.contains("private-root-name"), "root key must not leak: {id}");
        assert!(!id.contains("/tmp"), "paths must not leak: {id}");
        assert!(!id.contains(CONTENT), "source text must not leak: {id}");
        assert!(
            !id.contains("native_critic") && !id.contains("recommended"),
            "configuration values must not leak: {id}"
        );
    }

    #[test]
    fn legacy_engine_pins_native_profile_but_carries_policy_digest() {
        let mut context = context_with(Some("/tmp/ws-a"));
        context.critic_engine = CriticEngine::Legacy;
        let baseline = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();

        let mut profile_moved = context.clone();
        profile_moved.native_critic_profile = "strict".to_string();
        assert_eq!(
            baseline,
            subject_for(&profile_moved, URI_A, CONTENT).compose().ok().unwrap(),
            "the native profile is inert under the legacy engine"
        );

        // The legacy engine composes successfully: its required policy digest
        // is supplied from the pinned built-in policy domain.
        let subject = pull_report_subject(URI_A, CONTENT, Some(1), &context);
        assert!(subject.is_ok(), "legacy engine subject must be complete");
    }
}
