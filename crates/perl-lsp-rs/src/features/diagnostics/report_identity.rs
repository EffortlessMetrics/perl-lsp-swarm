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

use perl_lsp_rs_core::tooling::perl_critic::{
    AcceptedCriticPolicyIdentity, CRITIC_IDENTITY_SCHEMA_VERSION, DiagnosticFactIdentity,
    DiagnosticResultIdentityInput, DiagnosticResultSchemaVersions, DiagnosticSourceIdentity,
};
use perl_source_identity::{
    ContentDigest, LogicalPathError, LogicalSourceId, ProjectId, RootRelativeLogicalPath,
    WorkspaceRootId,
};

use super::PullDiagnosticsContext;

/// Schema/domain version of this composer. Bump whenever the set of
/// load-bearing fragments changes so prior client-held IDs stop parsing and
/// every report degrades honestly to `full`.
pub const PULL_REPORT_IDENTITY_SCHEMA_VERSION: u16 = 3;

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
// Bumped to 2 by #7024: canonical regex diagnostics changed what the built-in
// catalog emits. `PL1000`-`PL1007` are new identities, a backtracking risk moved
// off `PL001` onto `PL1000`, and `PL609` narrowed from the whole pattern node to
// the `(?{ ... })` block. Result IDs are deterministic across processes for equal
// subjects, so without this bump a client holding a pre-change ID could be told
// `unchanged` and keep diagnostics that predate every one of those. The cost is one
// `full` report per document after upgrade, which is what the pin is for.
const RULE_CATALOG_SCHEMA_VERSION: u32 = 2;
const SUPPRESSION_CONTRACT_SCHEMA_VERSION: u16 = 1;
const PROJECTION_WIRE_SCHEMA_VERSION: u16 = 1;
const REMEDIATION_WIRE_SCHEMA_VERSION: u16 = 1;

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
    /// The accepted Critic snapshot lacks its owning root authority.
    MissingCriticRootAuthority,
    /// The document URI could not be decoded to a filesystem path, so it cannot
    /// be positioned relative to any root (#15555).
    SourcePathUnavailable,
    /// The document path names no file — it is a filesystem root — so there is no
    /// logical source to identify.
    SourcePathHasNoFileName,
    /// The document path is not a plain absolute descent, so it cannot be
    /// positioned under a root. Either the form relative to the established
    /// owning root still contains a non-ordinary component such as `..`, or the
    /// path is not absolute and so names no directory that could be its root
    /// without consulting the process working directory.
    ///
    /// Deliberately a refusal rather than a fall-through to the standalone case:
    /// re-keying a document whose owning root *is* known, under a key still
    /// carrying the components that made it unusable, would be less honest than
    /// declining.
    SourcePathNotPlainDescent,
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
            Self::MissingCriticRootAuthority => {
                f.write_str("accepted critic snapshot has no owning root authority")
            }
            Self::SourcePathUnavailable => {
                f.write_str("document URI does not decode to a filesystem path")
            }
            Self::SourcePathHasNoFileName => f.write_str("document path names no file"),
            Self::SourcePathNotPlainDescent => {
                f.write_str("document path is not a plain absolute descent under a root")
            }
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
    critic_root_id: WorkspaceRootId,
    accepted_critic_fingerprint: String,
    facts_generation: Option<u64>,
    // Deliberately remains an opaque, normalized string fragment in this
    // bounded PR. Typed provenance for project configuration belongs to the
    // configuration authority; this field does not claim to model it.
    project_version: Option<String>,
    configuration_generation: Option<u64>,
    resolver_roots: BTreeSet<String>,
    projection: DiagnosticProjectionFragment,
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
    let project = ProjectId::from_canonical_name(PULL_IDENTITY_PROJECT);

    let Some(critic_root_key) = context.accepted_critic_snapshot.owning_root() else {
        return Err(NotReusable::MissingCriticRootAuthority);
    };
    let critic_root_id = WorkspaceRootId::from_project_and_root_key(&project, critic_root_key);

    Ok(PullReportSubject {
        content_digest: ContentDigest::of_bytes(content.as_bytes()),
        critic_root_id,
        accepted_critic_fingerprint: context
            .accepted_critic_snapshot
            .result_identity_fingerprint()
            .as_wire()
            .to_string(),
        resolver_roots: context.include_paths.iter().cloned().collect(),
        root_id,
        logical_path,
        document_generation,
        facts_generation: context.facts_generation,
        project_version: context
            .project_version
            .as_deref()
            .and_then(perl_lsp_rs_core::providers::diagnostics::version_compat::parse_configured_project_version)
            .map(|version| format!("{}.{}", version.major, version.minor))
            .or_else(|| context.project_version.as_ref().map(|raw| format!("invalid:{raw}"))),
        configuration_generation: context.configuration_generation,
        projection: context.projection,
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
        let policy = AcceptedCriticPolicyIdentity::new(
            self.critic_root_id.clone(),
            self.accepted_critic_fingerprint.clone(),
        );

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
        push_str(&mut canonical, "identity_schema", PULL_REPORT_IDENTITY_V3_TAG);
        push_str(&mut canonical, "substrate", inner.as_str());
        push_str(&mut canonical, "position_encoding", self.projection.position_encoding.as_token());
        push_u64(&mut canonical, "markup_messages", u64::from(self.projection.markup_messages));
        match self.configuration_generation {
            Some(generation) => {
                push_str(&mut canonical, "configuration_generation", "some");
                push_u64(&mut canonical, "configuration_generation_value", generation);
            }
            None => push_str(&mut canonical, "configuration_generation", "none"),
        }
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
const PULL_REPORT_IDENTITY_V3_TAG: &str = "perl-lsp:pull-report-identity:v3";

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
///   The context's `identity_root_key` is deliberately **not** the root key here.
///   It gates admission — no key, no identity — but the standalone root is keyed
///   on the document's own directory instead, and that substitution is what keeps
///   the branch sound. Reusing the context key with a bare file name would make
///   `/a/Mod.pm` and `/b/Mod.pm` one logical source whenever they share a key,
///   which the provider-default constructors guarantee: they ship
///   `identity_root_key: Some(PROVIDER_DEFAULT_ROOT_AUTHORITY)` with no path, so
///   every path-less document would collapse onto that one synthetic root.
///   `standalone_documents_are_identified_relative_to_their_own_directory` pins
///   the non-collision.
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
    //
    // Once `strip_prefix` succeeds the owning root is established, so a spelling
    // failure from here is a refusal rather than a fall-through to standalone.
    // Falling through would silently re-key a document whose owner the server
    // knows, under a root key still carrying the very `.`/`..` components that
    // made the spelling fail.
    if let Some(root_path) = context.identity_root_path.as_deref()
        && let Ok(relative) = document_path.strip_prefix(root_path)
    {
        let spelling = forward_slash_spelling(relative)?;
        let logical_path = RootRelativeLogicalPath::parse(&spelling)
            .map_err(NotReusable::SourcePathNotCanonical)?;
        return Ok((WorkspaceRootId::from_project_and_root_key(&project, root_key), logical_path));
    }

    // Standalone: the document's own directory is its root, which requires an
    // absolute path to name. A relative path reaching here would otherwise key
    // the identity on a directory that depends on the process's working
    // directory. `perl_uri` already rejects most such input with
    // `SourcePathUnavailable`; this stays as the explicit invariant rather than
    // an assumption about a lower crate's current behavior.
    if !document_path.is_absolute() {
        return Err(NotReusable::SourcePathNotPlainDescent);
    }
    let parent = document_path.parent().ok_or(NotReusable::SourcePathHasNoFileName)?;
    // The standalone root key is a directory path, and it has to be as plainly
    // spelled as an owned root-relative path: `/outside/dir/..` names the same
    // directory as `/outside` but keys a different identity, so two spellings of
    // one directory would otherwise become two roots.
    if parent.components().any(|c| matches!(c, Component::CurDir | Component::ParentDir)) {
        return Err(NotReusable::SourcePathNotPlainDescent);
    }
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

/// Spell `relative` with forward slashes.
///
/// Built from typed [`Component`] values rather than by rewriting separators in a
/// string: anything that is not an ordinary segment (`..`, `.`, a root, a Windows
/// prefix) makes the path unusable here instead of being folded into something
/// that merely looks canonical. A non-UTF-8 segment is also rejected, because
/// `to_string_lossy` maps distinct files onto one spelling.
///
/// One normalization *is* inherited from [`Path::components`] and is deliberate:
/// a repeated separator is dropped, so `lib//Mod.pm` and `lib/Mod.pm` yield one
/// spelling. Those name the same file, so collapsing them gives that file one
/// identity — the opposite of the `..` case, where folding would silently
/// redirect identity to a *different* file. `RootRelativeLogicalPath` is stricter
/// because it validates a path someone already claims is canonical; this function
/// derives a canonical path from an OS path, which is a different question.
///
/// # Errors
///
/// Returns a typed refusal naming which of those two conditions applied, so the
/// caller does not have to re-derive it from an `Option`.
fn forward_slash_spelling(relative: &Path) -> Result<String, NotReusable> {
    let mut spelling = String::new();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(NotReusable::SourcePathNotPlainDescent);
        };
        let segment = segment.to_str().ok_or(NotReusable::SourcePathNotRepresentable)?;
        if !spelling.is_empty() {
            spelling.push('/');
        }
        spelling.push_str(segment);
    }
    Ok(spelling)
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
    use perl_lsp_rs_core::config::{AcceptedCriticSnapshot, CriticEngine, ServerConfig};

    fn projection(encoding: PullPositionEncoding, markup: bool) -> DiagnosticProjectionFragment {
        DiagnosticProjectionFragment { position_encoding: encoding, markup_messages: markup }
    }

    /// Build a context whose root key and root path describe the *same* root,
    /// which is what the production resolution guarantees.
    fn context_with(root: Option<&str>) -> PullDiagnosticsContext {
        let mut context = PullDiagnosticsContext::new();
        context.identity_root_key = root.map(str::to_string);
        let config = ServerConfig::default();
        context.accepted_critic_snapshot = AcceptedCriticSnapshot::capture(&config, root);
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
    const ROOT_A: &str = if cfg!(windows) { "C:/tmp/ws-a" } else { "/tmp/ws-a" };
    const URI_A: &str = if cfg!(windows) {
        "file:///C:/tmp/ws-a/lib/Mod.pm"
    } else {
        "file:///tmp/ws-a/lib/Mod.pm"
    };
    /// Root B holding a document at the *same* relative path as [`URI_A`].
    const ROOT_B: &str = if cfg!(windows) { "C:/tmp/ws-b" } else { "/tmp/ws-b" };
    const URI_B: &str = if cfg!(windows) {
        "file:///C:/tmp/ws-b/lib/Mod.pm"
    } else {
        "file:///tmp/ws-b/lib/Mod.pm"
    };
    const CONTENT: &str = "my $x = 1;\n";

    fn native_fixture(path_or_uri: &str) -> String {
        if cfg!(windows) {
            if let Some(path) = path_or_uri.strip_prefix("file://") {
                return format!("file:///C:{path}");
            }
            return format!("C:{path_or_uri}");
        }
        path_or_uri.to_string()
    }

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
    fn old_result_ids_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        for legacy in [
            "diagnostic-pull-report.v1-sha256:0000000000000000000000000000000000000000000000000000000000000000",
            "diagnostic-pull-report.v2-sha256:0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            if PullReportResultId::from_wire(legacy).is_some() {
                return Err(format!("old result ID must fail closed to Full: {legacy}").into());
            }
        }
        Ok(())
    }

    #[test]
    fn accepted_state_identity_uses_the_snapshot_sha256_fingerprint()
    -> Result<(), Box<dyn std::error::Error>> {
        let context = context_with(Some(ROOT_A));
        let subject = subject_for(&context, URI_A, CONTENT);
        let expected = context.accepted_critic_snapshot.result_identity_fingerprint();
        if subject.accepted_critic_fingerprint != expected.as_wire() {
            return Err("pull identity must consume the snapshot's SHA-256 fingerprint".into());
        }
        if subject.accepted_critic_fingerprint == context.accepted_critic_snapshot.fingerprint() {
            return Err(
                "the legacy 64-bit observation token must not authorize result reuse".into()
            );
        }
        Ok(())
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

        // Raw legacy observations are not accepted authority. Moving all of
        // them alone must leave the identity unchanged.
        let mut context = baseline_context.clone();
        context.critic_engine = CriticEngine::Legacy;
        context.perlcritic_severity = 4;
        context.native_critic_profile = "strict".to_string();
        context.native_critic_include = vec!["native.testing.require_use_strict".to_string()];
        context.native_critic_exclude = vec!["native.security.string_eval".to_string()];
        context.perlcritic_enabled = false;
        assert_eq!(
            baseline,
            subject_for(&context, URI_A, CONTENT).compose().ok().unwrap(),
            "raw legacy selector movement must not change accepted Critic identity"
        );

        // Accepted snapshot movement is load-bearing.
        let mut context = baseline_context.clone();
        let moved_config = ServerConfig { perlcritic_severity: 4, ..ServerConfig::default() };
        context.accepted_critic_snapshot =
            AcceptedCriticSnapshot::capture(&moved_config, Some(ROOT_A));
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

        // Logical document identity: equal bytes and counters, different path.
        let moved = subject_for(
            &baseline_context,
            &native_fixture("file:///tmp/ws-a/lib/Other.pm"),
            CONTENT,
        )
        .compose()
        .ok()
        .unwrap();
        assert_ne!(baseline, moved);
    }

    #[test]
    fn configuration_generation_presence_moves_identity() -> Result<(), String> {
        let mut context = context_with(Some(ROOT_A));
        context.configuration_generation = None;
        let unavailable =
            subject_for(&context, URI_A, CONTENT).compose().map_err(|error| error.to_string())?;
        context.configuration_generation = Some(0);
        let zero =
            subject_for(&context, URI_A, CONTENT).compose().map_err(|error| error.to_string())?;
        if unavailable == zero {
            return Err("unavailable and zero configuration generations share an ID".to_string());
        }
        Ok(())
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
        let context = context_with(Some(&native_fixture("/tmp/private-root-name/ws-a")));
        let uri = &native_fixture("file:///tmp/private-root-name/ws-a/lib/Mod.pm");
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
    fn raw_legacy_selector_is_observation_only() -> Result<(), Box<dyn std::error::Error>> {
        let mut context = context_with(Some(ROOT_A));
        context.critic_engine = CriticEngine::Legacy;
        let baseline =
            subject_for(&context, URI_A, CONTENT).compose().map_err(|error| error.to_string())?;

        let mut profile_moved = context.clone();
        profile_moved.native_critic_profile = "strict".to_string();
        let moved = subject_for(&profile_moved, URI_A, CONTENT)
            .compose()
            .map_err(|error| error.to_string())?;
        if baseline != moved {
            return Err("raw native profile movement must remain observation-only".into());
        }
        Ok(())
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
        let alice = context_with_split_root(
            Some("shared-root-key"),
            Some(&native_fixture("/home/alice/proj")),
        );
        let bob = context_with_split_root(
            Some("shared-root-key"),
            Some(&native_fixture("/home/bob/proj")),
        );

        let from_alice =
            subject_for(&alice, &native_fixture("file:///home/alice/proj/lib/App.pm"), CONTENT)
                .compose()
                .ok();
        let from_bob =
            subject_for(&bob, &native_fixture("file:///home/bob/proj/lib/App.pm"), CONTENT)
                .compose()
                .ok();

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
            subject_for(&context, &native_fixture("file:///tmp/ws-a/lib/Other.pm"), CONTENT)
                .compose()
                .ok()
                .unwrap();
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

        let outside =
            subject_for(&context, &native_fixture("file:///elsewhere/lib/Mod.pm"), CONTENT)
                .compose()
                .ok();
        assert!(outside.is_some(), "a document outside the root must keep a reusable ID");

        let sibling =
            subject_for(&context, &native_fixture("file:///elsewhere/lib/Other.pm"), CONTENT)
                .compose()
                .ok();
        assert_ne!(outside, sibling, "two files in one standalone directory must differ");

        let same_name_elsewhere =
            subject_for(&context, &native_fixture("file:///other-place/lib/Mod.pm"), CONTENT)
                .compose()
                .ok();
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
            pull_report_subject(
                &native_fixture("file:///elsewhere/lib/Mod.pm"),
                CONTENT,
                Some(1),
                &context
            ),
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

    /// A document whose owning root *is* established, but whose root-relative form
    /// is not a plain descent, is refused rather than silently re-keyed as
    /// standalone under a root key carrying the offending `..`.
    ///
    /// Only reachable through the bare-path input route, since `url::Url` resolves
    /// dot segments before this code sees them — which is exactly why it needs a
    /// test rather than an assumption about the lower crate.
    #[test]
    fn dot_segments_under_an_established_root_are_refused_not_re_keyed() {
        let context = context_with(Some(&native_fixture("/root")));
        assert_eq!(
            pull_report_subject(
                &native_fixture("/root/dir/../file.pm"),
                CONTENT,
                Some(1),
                &context
            ),
            Err(NotReusable::SourcePathNotPlainDescent),
            "a document the server knows the root of must not fall back to standalone"
        );
    }

    /// Exhaustive over the refusal enum: adding a variant without naming it here is
    /// a compile error, so no refusal can ship without a rendered message.
    fn not_reusable_label(outcome: &NotReusable) -> &'static str {
        match outcome {
            NotReusable::MissingRootAuthority => "MissingRootAuthority",
            NotReusable::MissingCriticRootAuthority => "MissingCriticRootAuthority",
            NotReusable::SourcePathUnavailable => "SourcePathUnavailable",
            NotReusable::SourcePathHasNoFileName => "SourcePathHasNoFileName",
            NotReusable::SourcePathNotPlainDescent => "SourcePathNotPlainDescent",
            NotReusable::SourcePathNotRepresentable => "SourcePathNotRepresentable",
            NotReusable::SourcePathNotCanonical(_) => "SourcePathNotCanonical",
            // `#[non_exhaustive]` only constrains other crates; in-crate this match
            // stays exhaustive on purpose so a new refusal cannot skip its message.
        }
    }

    /// Every refusal must render something a maintainer can act on, and the
    /// variants must be distinguishable from each other.
    ///
    /// Covers every variant `not_reusable_label` names, including the wrapping
    /// `MissingCriticRootAuthority` case, so no refusal's message goes unrendered.
    ///
    /// Deliberately does *not* assert the absence of `/`: these messages contain
    /// no input, and separators appear legitimately as prose and punctuation
    /// ("workspace/root authority"). Leak-freedom is a property of refusals
    /// carrying real rejected material, and is proven against a canary by
    /// `refusal_text_does_not_leak_document_or_root_paths`.
    #[test]
    fn every_not_reusable_variant_renders_a_distinct_message() {
        let all = [
            NotReusable::MissingRootAuthority,
            NotReusable::MissingCriticRootAuthority,
            NotReusable::SourcePathUnavailable,
            NotReusable::SourcePathHasNoFileName,
            NotReusable::SourcePathNotPlainDescent,
            NotReusable::SourcePathNotRepresentable,
            NotReusable::SourcePathNotCanonical(LogicalPathError::ParentDirectorySegment),
        ];

        let mut rendered = Vec::new();
        for outcome in &all {
            let text = format!("{outcome}");
            let label = not_reusable_label(outcome);
            assert!(!text.is_empty(), "{label} must render");
            // A phrase, not a bare token: the point of a typed refusal is that a
            // maintainer can read it.
            assert!(text.contains(' '), "{label} must render a phrase, got {text:?}");
            rendered.push(text);
        }

        let mut unique = rendered.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            rendered.len(),
            "each refusal must be distinguishable: {rendered:?}"
        );
    }

    /// The exact configuration the provider-default constructors ship — a synthetic
    /// `identity_root_key` with no `identity_root_path` — must not collapse every
    /// path-less document onto that one synthetic root. This is why the standalone
    /// branch keys on the document's own directory instead of the context key.
    #[test]
    fn provider_default_key_without_a_path_does_not_collapse_distinct_documents() {
        let context = PullDiagnosticsContext::new();
        assert!(
            context.identity_root_key.is_some() && context.identity_root_path.is_none(),
            "this test is only meaningful for the provider-default key/path shape"
        );

        let first =
            subject_for(&context, &native_fixture("file:///a/Mod.pm"), CONTENT).compose().ok();
        let second =
            subject_for(&context, &native_fixture("file:///b/Mod.pm"), CONTENT).compose().ok();

        assert!(first.is_some(), "a path-less document must still compose");
        assert_ne!(
            first, second,
            "same file name under one synthetic key must not become one logical source"
        );
    }

    /// The standalone root key is a directory path and must be as plainly spelled
    /// as an owned root-relative path. `/outside/dir/..` names the same directory
    /// as `/outside` but would key a different identity, so two spellings of one
    /// directory would otherwise become two roots.
    #[test]
    fn standalone_root_key_rejects_dot_segments() {
        let context = context_with(Some(ROOT_A));
        assert_eq!(
            pull_report_subject(
                &native_fixture("/outside/dir/../Mod.pm"),
                CONTENT,
                Some(1),
                &context
            ),
            Err(NotReusable::SourcePathNotPlainDescent)
        );
    }

    /// A repeated separator names the same file, so it must yield the *same*
    /// identity — the deliberate exception to "nothing is folded", and the
    /// opposite of the `..` case where folding would redirect identity to a
    /// different file.
    #[test]
    fn repeated_separators_name_one_file_and_one_identity() {
        let context = context_with(Some(ROOT_A));
        let doubled =
            subject_for(&context, &native_fixture("/tmp/ws-a/lib//Mod.pm"), CONTENT).compose().ok();
        let single =
            subject_for(&context, &native_fixture("/tmp/ws-a/lib/Mod.pm"), CONTENT).compose().ok();
        assert_eq!(doubled, single, "one file must have one identity");
        assert!(doubled.is_some(), "both spellings must compose");
    }

    /// A filesystem root names no file, so there is no logical source.
    #[test]
    fn a_document_path_naming_no_file_is_refused() {
        let context = context_with(Some(ROOT_A));
        assert_eq!(
            pull_report_subject(&native_fixture("file:///"), CONTENT, Some(1), &context),
            Err(NotReusable::SourcePathHasNoFileName)
        );
    }

    /// A path segment that is not valid UTF-8 has no identity-bearing spelling.
    /// `to_string_lossy` would map every such distinct file onto one U+FFFD
    /// spelling, so it is refused instead.
    #[cfg(unix)]
    #[test]
    fn non_utf8_path_segments_are_refused_not_lossily_spelled() {
        let context = context_with(Some(ROOT_A));
        // %FF decodes to a byte that is not valid UTF-8 on its own.
        assert_eq!(
            pull_report_subject("file:///tmp/ws-a/lib/%FF.pm", CONTENT, Some(1), &context),
            Err(NotReusable::SourcePathNotRepresentable)
        );
    }

    /// Negative control for the test above: an absolute path that genuinely names
    /// a file does compose, as a standalone document. The refusals above must come
    /// from the material being unusable, not from refusing everything.
    #[test]
    fn absolute_document_paths_still_compose() {
        let context = context_with(Some(ROOT_A));
        for uri in
            [&native_fixture("/absolute/not/a/uri.pm"), &native_fixture("file:///tmp/other/Mod.pm")]
        {
            let outcome = pull_report_subject(uri, CONTENT, Some(1), &context);
            assert!(outcome.is_ok(), "{uri:?} names a real file and must compose, got {outcome:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn driveless_standalone_uri_has_no_absolute_root() -> Result<(), String> {
        let context = context_with(Some(ROOT_A));
        let outcome = pull_report_subject("file:///elsewhere/Mod.pm", CONTENT, Some(1), &context);
        if outcome != Err(NotReusable::SourcePathNotPlainDescent) {
            return Err(format!(
                "drive-less standalone URI must have no absolute root: {outcome:?}"
            ));
        }
        Ok(())
    }

    /// Refusals must not echo the material they refused — it is exactly the
    /// material most likely to be a host path.
    #[test]
    fn refusal_text_does_not_leak_document_or_root_paths() {
        let context = context_with(Some(&native_fixture("/home/alice/private-root")));
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
            subject_for(&context, &native_fixture("file:///tmp/ws-a/lib/My%20File.pm"), CONTENT)
                .compose()
                .ok();
        let decoded =
            subject_for(&context, &native_fixture("file:///tmp/ws-a/lib/My File.pm"), CONTENT)
                .compose()
                .ok();
        assert_eq!(encoded, decoded, "one document must have one logical source identity");
        assert!(encoded.is_some(), "a space in a filename must still compose");
    }
}
