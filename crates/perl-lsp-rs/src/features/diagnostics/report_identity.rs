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
use std::path::{Component, Path};

use perl_lsp_rs_core::config::CriticEngine;
use perl_lsp_rs_core::tooling::perl_critic::{
    CRITIC_IDENTITY_SCHEMA_VERSION, CriticPolicyIdentity, CriticPolicyIdentityError,
    DiagnosticFactIdentity, DiagnosticResultIdentityInput, DiagnosticResultSchemaVersions,
    DiagnosticSourceIdentity, NativeCriticProfile,
};
use perl_source_identity::{
    ContentDigest, LogicalPathError, LogicalSourceId, ProjectId, RootRelativeLogicalPath,
    WorkspaceRootId,
};

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
///
/// `#[non_exhaustive]` for the same reason `SourceOrigin` and
/// `PhysicalSourceRole` are in `perl-source-identity`: the set of reasons a
/// subject cannot be reused grows as more of the subject becomes typed, and each
/// new reason should not be a breaking change for downstream matchers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotReusable {
    /// No owning workspace/folder authority could be established for the
    /// document, so the logical source identity cannot be formed. A root key the
    /// server never resolved is not substituted by the standalone fallback.
    MissingRootAuthority,
    /// The accepted critic policy contradicts its engine's requirements.
    PolicyIncomplete(CriticPolicyIdentityError),
    /// The document URI could not be decoded to a filesystem path, so it cannot
    /// be positioned relative to any root (#15555).
    SourcePathUnavailable,
    /// The document path names no file — it is a filesystem root — so there is no
    /// logical source to identify.
    SourcePathHasNoFileName,
    /// A path segment is not valid UTF-8, so it has no identity-bearing
    /// spelling. Refused rather than passed through `to_string_lossy`, which
    /// maps distinct files onto one spelling.
    SourcePathNotRepresentable,
    /// The root-relative spelling is not a canonical logical path. Carries the
    /// typed reason; [`LogicalPathError`] deliberately holds no path material.
    SourcePathNotCanonical(LogicalPathError),
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
            Self::SourcePathUnavailable => {
                f.write_str("document URI does not decode to a filesystem path")
            }
            Self::SourcePathHasNoFileName => f.write_str("document path names no file"),
            Self::SourcePathNotRepresentable => {
                f.write_str("document path contains a segment that is not valid UTF-8")
            }
            // `error` carries no path material, so this stays leak-free.
            Self::SourcePathNotCanonical(error) => {
                write!(f, "document has no canonical root-relative logical path: {error}")
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
    logical_path: RootRelativeLogicalPath,
    content_digest: ContentDigest,
    document_generation: Option<u64>,
    engine: CriticEngine,
    profile: NativeCriticProfile,
    severity: u8,
    include: BTreeSet<String>,
    exclude: BTreeSet<String>,
    legacy_policy_digest: Option<ContentDigest>,
    facts_generation: Option<u64>,
    // Deliberately remains an opaque, normalized string fragment in this
    // bounded PR. Typed provenance for project configuration belongs to the
    // configuration authority; this field does not claim to model it.
    project_version: Option<String>,
    configuration_generation: Option<u64>,
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
    let (root_id, logical_path) = root_and_logical_path(uri, context)?;

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
        logical_path,
        document_generation,
        engine: context.critic_engine,
        profile,
        facts_generation: context.facts_generation,
        project_version: context
            .project_version
            .as_deref()
            .and_then(perl_lsp_rs_core::providers::diagnostics::version_compat::parse_configured_project_version)
            .map(|version| format!("{}.{}", version.major, version.minor))
            .or_else(|| context.project_version.as_ref().map(|raw| format!("invalid:{raw}"))),
        configuration_generation: context.configuration_generation,
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
        // Folder-owned configuration generations distinguish accepted config
        // reloads while the default preserves identities without that context.
        let configuration_generation = self.configuration_generation.unwrap_or(0);

        let policy = CriticPolicyIdentity::new(
            self.root_id.clone(),
            configuration_generation,
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
                LogicalSourceId::from_root_and_logical_path(&self.root_id, &self.logical_path),
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
        push_str(
            &mut canonical,
            "project_version",
            self.project_version.as_deref().unwrap_or("<none>"),
        );
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
/// Resolve the root authority and the document's path *relative to that root*.
///
/// Both halves come out of one decision so the pair is always coherent: the
/// returned logical path is relative to exactly the root the returned
/// [`WorkspaceRootId`] names. Minting a root ID from the workspace folder while
/// spelling the path relative to something else would satisfy the type checker
/// and still describe no real source.
///
/// Replaces a derivation that took `url::Url::path()` — the *host absolute*
/// path — stripped its leading slash and called the result root-relative. That
/// bound the host layout into the logical source (`/home/alice/proj/lib/App.pm`
/// and `/home/bob/proj/lib/App.pm` became different logical sources for the same
/// file) and, when URI parsing failed, passed the raw string through so `..`,
/// absolute and empty forms reached the digest verbatim.
///
/// Two cases, both root-relative:
///
/// - **owned** — the document sits inside the owning workspace root, so the root
///   key is the folder authority and the logical path is the document's path
///   relative to it. This is the case the fix is for.
/// - **standalone** — no workspace root owns the document (it lies outside every
///   root, or none was resolved). Its own directory is then the only root
///   authority available, so the logical path is the bare file name. Host
///   location moves into the *root key*, where this composer's keys already live
///   pending the #4832 authority bridge, instead of into the logical source,
///   which must stay root-relative. Behavior is preserved for these documents:
///   they keep a reusable result ID rather than losing one.
///
/// Absent root authority is still absent (#7480): a missing root key yields
/// [`NotReusable::MissingRootAuthority`] rather than a standalone identity.
fn root_and_logical_path(
    uri: &str,
    context: &PullDiagnosticsContext,
) -> Result<(WorkspaceRootId, RootRelativeLogicalPath), NotReusable> {
    let Some(root_key) = context.identity_root_key.as_deref() else {
        return Err(NotReusable::MissingRootAuthority);
    };
    let project = ProjectId::from_canonical_name(PULL_IDENTITY_PROJECT);
    let document_path =
        perl_uri::source_path_from_uri_or_path(uri).ok_or(NotReusable::SourcePathUnavailable)?;

    // Owned: the resolved root contains the document.
    if let Some(root_path) = context.identity_root_path.as_deref()
        && let Ok(relative) = document_path.strip_prefix(root_path)
        && let Some(spelling) = forward_slash_spelling(relative)
    {
        let logical_path = RootRelativeLogicalPath::parse(&spelling)
            .map_err(NotReusable::SourcePathNotCanonical)?;
        return Ok((WorkspaceRootId::from_project_and_root_key(&project, root_key), logical_path));
    }

    // Standalone: the document's own directory is its root. Requiring an
    // absolute path keeps relative, traversal-only and empty material out —
    // such input names no directory that could serve as a root authority.
    if !document_path.is_absolute() {
        return Err(NotReusable::SourcePathNotCanonical(LogicalPathError::LeadingSeparator));
    }
    let parent = document_path.parent().ok_or(NotReusable::SourcePathHasNoFileName)?;
    let file_name = document_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(NotReusable::SourcePathNotRepresentable)?;
    let standalone_root_key =
        parent.to_str().ok_or(NotReusable::SourcePathNotRepresentable)?.to_owned();
    let logical_path =
        RootRelativeLogicalPath::parse(file_name).map_err(NotReusable::SourcePathNotCanonical)?;
    Ok((WorkspaceRootId::from_project_and_root_key(&project, &standalone_root_key), logical_path))
}

/// Spell `relative` with forward slashes, or `None` if it is not a plain descent.
///
/// Built from typed [`Component`] values rather than by rewriting separators in a
/// string: anything that is not an ordinary segment (`..`, `.`, a root, a Windows
/// prefix) makes the path unusable here instead of being folded into something
/// that merely looks canonical. A non-UTF-8 segment is also rejected, because
/// `to_string_lossy` maps distinct files onto one spelling.
fn forward_slash_spelling(relative: &Path) -> Option<String> {
    let mut spelling = String::new();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return None;
        };
        let segment = segment.to_str()?;
        if !spelling.is_empty() {
            spelling.push('/');
        }
        spelling.push_str(segment);
    }
    Some(spelling)
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

    /// Build a context whose root key and root path describe the *same* root,
    /// which is what the production resolution guarantees.
    fn context_with(root: Option<&str>) -> PullDiagnosticsContext {
        let mut context = PullDiagnosticsContext::new();
        context.identity_root_key = root.map(str::to_string);
        context.identity_root_path = root.map(std::path::PathBuf::from);
        // Live fact-store state for the baseline subject.
        context.facts_generation = Some(13);
        context.projection = projection(PullPositionEncoding::Utf16, false);
        context
    }

    /// A context whose root key and root path are deliberately independent, for
    /// the cases that need to vary one without the other.
    fn context_with_split_root(key: Option<&str>, path: Option<&str>) -> PullDiagnosticsContext {
        let mut context = context_with(key);
        context.identity_root_path = path.map(std::path::PathBuf::from);
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

    /// Root A and a document inside it. The URI must actually sit under
    /// [`ROOT_A`]: the composer now positions the document relative to its
    /// owning root instead of treating the host path as the logical path.
    const ROOT_A: &str = "/tmp/ws-a";
    const URI_A: &str = "file:///tmp/ws-a/lib/Mod.pm";
    /// Root B holding a document at the *same* relative path as [`URI_A`].
    const ROOT_B: &str = "/tmp/ws-b";
    const URI_B: &str = "file:///tmp/ws-b/lib/Mod.pm";
    const CONTENT: &str = "my $x = 1;\n";

    #[test]
    fn identical_subjects_compose_identical_ids() {
        let context = context_with(Some(ROOT_A));
        let first = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        let second = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn composed_ids_parse_under_current_schema_only() {
        let context = context_with(Some(ROOT_A));
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
        let baseline_context = context_with(Some(ROOT_A));
        let baseline = subject_for(&baseline_context, URI_A, CONTENT).compose().ok().unwrap();

        // Owning root authority.
        // Same relative path (`lib/Mod.pm`) and identical bytes in a different
        // root must not share an ID. Both documents sit inside their own root,
        // so this now exercises real root ownership rather than two host paths.
        let moved =
            subject_for(&context_with(Some(ROOT_B)), URI_B, CONTENT).compose().ok().unwrap();
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

        // Project compatibility target: config-only changes must invalidate the ID.
        let mut context = baseline_context.clone();
        context.project_version = Some("5.20".to_string());
        let project_id = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        assert_ne!(baseline, project_id);
        context.project_version = Some("v5.20".to_string());
        assert_eq!(
            project_id,
            subject_for(&context, URI_A, CONTENT).compose().ok().unwrap(),
            "equivalent project version spellings must share the effective identity"
        );
        context.project_version = Some("5.38".to_string());
        assert_ne!(project_id, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        context.configuration_generation = Some(1);
        let generation_one = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        context.configuration_generation = Some(2);
        assert_ne!(generation_one, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

        context.project_version = Some("not-a-version".to_string());
        context.configuration_generation = Some(3);
        let invalid_one = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        context.project_version = Some("5.20.1".to_string());
        assert_ne!(invalid_one, subject_for(&context, URI_A, CONTENT).compose().ok().unwrap());

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
        let moved = subject_for(&baseline_context, "file:///tmp/ws-a/lib/Other.pm", CONTENT)
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
        // A private-looking root that genuinely contains the document, so the
        // subject composes and the assertions below test leakage rather than
        // accidentally testing a refusal.
        let context = context_with(Some("/tmp/private-root-name/ws-a"));
        let uri = "file:///tmp/private-root-name/ws-a/lib/Mod.pm";
        let id = pull_report_subject(uri, CONTENT, Some(1), &context)
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
        let mut context = context_with(Some(ROOT_A));
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

    // ── Root-relative logical path (#15555) ───────────────────────────────────

    /// The defect this claim fixes. The old derivation used the document's host
    /// absolute path, so the *same logical file* in two checkout locations of the
    /// same logical root produced different IDs. With a genuinely root-relative
    /// path, the checkout location drops out.
    #[test]
    fn identical_logical_source_in_two_checkout_locations_composes_one_id() {
        // Same project, same root authority key, same relative path and bytes —
        // only the host location of the checkout differs.
        let alice = context_with_split_root(Some("shared-root-key"), Some("/home/alice/proj"));
        let bob = context_with_split_root(Some("shared-root-key"), Some("/home/bob/proj"));

        let from_alice =
            subject_for(&alice, "file:///home/alice/proj/lib/App.pm", CONTENT).compose().ok();
        let from_bob =
            subject_for(&bob, "file:///home/bob/proj/lib/App.pm", CONTENT).compose().ok();

        assert_eq!(
            from_alice, from_bob,
            "the host checkout location must not change the logical source identity"
        );
        assert!(from_alice.is_some(), "both subjects must compose");
    }

    /// Negative control for the test above: it must not pass by making every
    /// document identical. A different relative path under the same root still
    /// has to move the ID.
    #[test]
    fn different_relative_paths_under_one_root_still_differ() {
        let context = context_with(Some(ROOT_A));
        let first = subject_for(&context, URI_A, CONTENT).compose().ok().unwrap();
        let second =
            subject_for(&context, "file:///tmp/ws-a/lib/Other.pm", CONTENT).compose().ok().unwrap();
        assert_ne!(first, second, "distinct relative paths must not share an ID");
    }

    /// A document no workspace root owns keeps a reusable ID — behavior is not
    /// regressed — but it is identified relative to its own directory, so the
    /// host location lands in the root key rather than in the logical source.
    ///
    /// Two files sharing a directory must still differ, and the same file name in
    /// two different directories must still differ.
    #[test]
    fn standalone_documents_are_identified_relative_to_their_own_directory() {
        let context = context_with(Some(ROOT_A));

        let outside = subject_for(&context, "file:///elsewhere/lib/Mod.pm", CONTENT).compose().ok();
        assert!(outside.is_some(), "a document outside the root must keep a reusable ID");

        let sibling =
            subject_for(&context, "file:///elsewhere/lib/Other.pm", CONTENT).compose().ok();
        assert_ne!(outside, sibling, "two files in one standalone directory must differ");

        let same_name_elsewhere =
            subject_for(&context, "file:///other-place/lib/Mod.pm", CONTENT).compose().ok();
        assert_ne!(
            outside, same_name_elsewhere,
            "one file name in two directories must not collapse to one identity"
        );
    }

    /// A root key with no root path cannot position the document inside the root,
    /// so the document is treated as standalone rather than refused.
    #[test]
    fn missing_root_path_falls_back_to_standalone_identity() {
        let with_path = context_with(Some(ROOT_A));
        let without_path = context_with_split_root(Some(ROOT_A), None);

        let owned = subject_for(&with_path, URI_A, CONTENT).compose().ok();
        let standalone = subject_for(&without_path, URI_A, CONTENT).compose().ok();

        assert!(standalone.is_some(), "a known root key must still yield a reusable ID");
        assert_ne!(
            owned, standalone,
            "root-relative and standalone identities describe different subjects"
        );
    }

    /// Absent root authority stays absent (#7480): the standalone fallback must
    /// not quietly manufacture authority the server never established.
    #[test]
    fn standalone_fallback_does_not_manufacture_missing_root_authority() {
        let context = context_with(None);
        assert_eq!(
            pull_report_subject("file:///elsewhere/lib/Mod.pm", CONTENT, Some(1), &context),
            Err(NotReusable::MissingRootAuthority)
        );
    }

    /// The fail-open fallback the old derivation had: when URI parsing failed it
    /// passed the raw string through, so traversal, relative and empty forms
    /// reached the durable digest verbatim. Such material names no directory that
    /// could serve as a root, so none of it may compose.
    #[test]
    fn relative_and_traversal_material_never_composes() {
        let context = context_with(Some(ROOT_A));
        for uri in ["../../etc/passwd", "lib/../secret.pm", "lib/Mod.pm", ""] {
            let outcome = pull_report_subject(uri, CONTENT, Some(1), &context);
            assert!(
                outcome.is_err(),
                "{uri:?} must not produce a reusable subject, got {outcome:?}"
            );
        }
    }

    /// Negative control for the test above: an absolute path that genuinely names
    /// a file does compose, as a standalone document. The refusals above must come
    /// from the material being unusable, not from refusing everything.
    #[test]
    fn absolute_document_paths_still_compose() {
        let context = context_with(Some(ROOT_A));
        for uri in ["/absolute/not/a/uri.pm", "file:///tmp/other/Mod.pm"] {
            let outcome = pull_report_subject(uri, CONTENT, Some(1), &context);
            assert!(outcome.is_ok(), "{uri:?} names a real file and must compose, got {outcome:?}");
        }
    }

    /// Refusals must not echo the material they refused — it is exactly the
    /// material most likely to be a host path.
    #[test]
    fn refusal_text_does_not_leak_document_or_root_paths() {
        let context = context_with(Some("/home/alice/private-root"));
        // Relative material is refused, and the refusal must not echo it.
        let outcome = pull_report_subject("../alice-secrets/creds.pm", CONTENT, Some(1), &context);
        let error = outcome.expect_err("relative material must be refused");
        let rendered = format!("{error} / {error:?}");
        assert!(!rendered.contains("alice"), "must not leak path material: {rendered}");
        assert!(!rendered.contains("creds"), "must not leak file name: {rendered}");
        assert!(!rendered.contains("secrets"), "must not leak directory name: {rendered}");
    }

    /// Percent-encoded URIs decode to the real filename, so the logical path is
    /// the name a user would recognize rather than its wire spelling. The two
    /// equivalent spellings of one document must therefore agree.
    #[test]
    fn equivalent_uri_spellings_describe_one_logical_source() {
        let context = context_with(Some(ROOT_A));
        let encoded =
            subject_for(&context, "file:///tmp/ws-a/lib/My%20File.pm", CONTENT).compose().ok();
        let decoded =
            subject_for(&context, "file:///tmp/ws-a/lib/My File.pm", CONTENT).compose().ok();
        assert_eq!(encoded, decoded, "one document must have one logical source identity");
        assert!(encoded.is_some(), "a space in a filename must still compose");
    }
}
