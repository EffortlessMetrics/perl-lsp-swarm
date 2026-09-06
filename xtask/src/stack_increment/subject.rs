//! Stack-local increment subject contract (`stack_increment_subject.v1`).
//!
//! Admission is explicit and fail-closed: a trusted same-repository context,
//! one machine-readable edge declaration, an exact parent-head binding of the
//! child base, admission-time head freshness, proven ancestor history, and a
//! child-only delta bound to both endpoint trees. Branch names, PR titles,
//! labels, and check names are structurally incapable of creating an edge.

use super::{
    ChildDelta, PROTECTED_MAIN_NOT_EVALUATED, STACK_INCREMENT_PRODUCER,
    STACK_INCREMENT_SUBJECT_SCHEMA, sha256_hex, validate_nonempty, validate_sha40,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Closed refusal-code vocabulary emitted by subject compilation. Each code
/// names exactly one fail-closed admission rule.
pub const STACK_SUBJECT_ERROR_CODES: &[&str] = &[
    "repository_untrusted",
    "edge_undeclared",
    "edge_declaration_invalid",
    "wrong_parent_base",
    "parent_moved_since_admission",
    "child_moved_since_admission",
    "delta_unbound_to_trees",
    "delta_fingerprint_mismatch",
    "unrelated_stack_history",
    "history_not_proven",
    "undeclared_delta_surface",
    "malformed_subject",
    "protected_main_promotion",
    "endpoint_identity_collision",
];

/// Typed subject-compilation refusal. The code is drawn from
/// [`STACK_SUBJECT_ERROR_CODES`] only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackSubjectCompileError {
    /// Closed refusal-code identity.
    pub code: String,
    /// Human-readable explanation naming the refused fact.
    pub message: String,
}

impl std::fmt::Display for StackSubjectCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for StackSubjectCompileError {}

fn refuse(code: &str, message: impl Into<String>) -> StackSubjectCompileError {
    debug_assert!(STACK_SUBJECT_ERROR_CODES.contains(&code), "unknown refusal code {code}");
    StackSubjectCompileError { code: code.to_string(), message: message.into() }
}

/// One exact stack endpoint: repository-internal PR identity plus immutable
/// head commit and tree SHAs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackEndpoint {
    /// Parent or child pull-request number inside the trusted repository.
    pub pr_number: u64,
    /// Issue/train node identity when known; otherwise empty.
    #[serde(default)]
    pub issue_node_id: String,
    /// Branch name, recorded for operators only — never admission evidence.
    pub branch: String,
    /// Exact head commit SHA (40-hex).
    pub head_sha: String,
    /// Exact head tree SHA (40-hex).
    pub head_tree: String,
}

/// Closed stable-edge vocabulary. Only reviewed programme dependencies may be
/// declared; there is deliberately no free-form variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The stacked pair shares a reviewed programme dependency and the child
    /// explains a coherent increment of the parent branch.
    ProgrammeDependency,
}

impl EdgeKind {
    /// Stable machine spelling used by declarations and receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProgrammeDependency => "programme_dependency",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "programme_dependency" => Some(Self::ProgrammeDependency),
            _ => None,
        }
    }
}

/// The explicit machine-readable parent/child edge. Nothing here is derived
/// from conventions; without this exact declaration there is no stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackEdgeDeclaration {
    /// Declared dependency kind from the closed [`EdgeKind`] vocabulary.
    pub dependency: EdgeKind,
    /// Parent PR number the child claims to increment.
    pub parent_pr_number: u64,
    /// Explicit path scopes the child increment is allowed to touch.
    /// Literal prefixes ending in `/` match whole subtrees; other entries
    /// match full paths only.
    pub scope_paths: Vec<String>,
    /// Parent head SHA when the declaration pinned one explicitly (`=40hex`).
    /// Live subject assembly requires this pin; fixture compilation may omit
    /// it and bind the base through [`StackSubjectInput`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_parent_head_sha: Option<String>,
}

/// Read-only history relation between the two heads, projected from the
/// `git_ancestry.v1` authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelatedHistory {
    /// Parent head is an ancestor of child head.
    Ancestor,
    /// Both commits are present and related, but neither contains the other.
    Diverged,
    /// No merge base exists in a complete-enough local graph.
    Unrelated,
    /// A shallow clone cannot decide the relation.
    NotProvenShallow,
    /// A partial/promisor clone cannot decide the relation.
    NotProvenPartialClone,
    /// One requested object could not be resolved locally.
    NotProvenMissingObject,
}

impl RelatedHistory {
    /// Stable machine spelling used by live probes and receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ancestor => "ancestor",
            Self::Diverged => "diverged",
            Self::Unrelated => "unrelated",
            Self::NotProvenShallow => "not_proven_shallow",
            Self::NotProvenPartialClone => "not_proven_partial_clone",
            Self::NotProvenMissingObject => "not_proven_missing_object",
        }
    }
}

/// Repository trust surface admitted for this subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustContext {
    /// Parent and child endpoints declare the same repository string.
    pub same_repository_declared: bool,
    /// An operator explicitly admitted a named external context instead.
    /// False keeps every subject inside one trusted repository.
    pub external_context_admitted: bool,
}

/// Compile input projecting every independent admission fact onto the typed
/// subject domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackSubjectInput {
    /// Exact repository both endpoints live in.
    pub repository: String,
    /// Observation/event identity when the assembly had one.
    pub event_id: Option<String>,
    /// Parent endpoint facts.
    pub parent: StackEndpoint,
    /// Child endpoint facts.
    pub child: StackEndpoint,
    /// Explicit edge declaration; `None` refuses compilation.
    pub edge: Option<StackEdgeDeclaration>,
    /// Head SHA the child base ref must resolve to at admission.
    pub child_base_expected_head_sha: String,
    /// Parent head SHA observed when the subject was assembled.
    pub observed_parent_head_sha: String,
    /// Child head SHA observed when the subject was assembled.
    pub observed_child_head_sha: String,
    /// Admitted trust context.
    pub trust: TrustContext,
    /// Proven history relation between the two heads.
    pub history: RelatedHistory,
    /// Child-only semantic delta, fingerprinted over both endpoint trees.
    pub delta: ChildDelta,
}

/// Compiled stack-local increment subject (`stack_increment_subject.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackIncrementSubjectV1 {
    /// Contract identity.
    pub schema: String,
    /// Producer identity.
    pub producer: String,
    /// Exact repository both endpoints live in.
    pub repository: String,
    /// Observation/event identity when recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// Parent endpoint.
    pub parent: StackEndpoint,
    /// Child endpoint.
    pub child: StackEndpoint,
    /// The explicit edge that admits this pair.
    pub edge: StackEdgeDeclaration,
    /// Head SHA the child base ref must resolve to.
    pub child_base_expected_head_sha: String,
    /// Proven history relation between the two heads.
    pub history_relation: RelatedHistory,
    /// Child-only semantic delta bound to both endpoint trees.
    pub delta: ChildDelta,
    /// Permanent protected-main state of this artifact.
    pub protected_main_state: String,
}

/// Validate every cross-field invariant of a compiled subject. Pure.
///
/// This re-checks the full admission surface, not just artifact shape: a
/// serialized subject edited to a non-ancestor `history_relation`, a parent
/// endpoint that disagrees with its edge declaration, or a declared parent
/// head that disagrees with the parent endpoint must refuse exactly as it
/// would at compile time (#13360 root cause 1).
pub fn validate_subject(subject: &StackIncrementSubjectV1) -> Result<(), StackSubjectCompileError> {
    if subject.schema != STACK_INCREMENT_SUBJECT_SCHEMA {
        return Err(refuse(
            "malformed_subject",
            format!("unsupported schema {:?}", subject.schema),
        ));
    }
    if subject.producer != STACK_INCREMENT_PRODUCER {
        return Err(refuse(
            "malformed_subject",
            format!("unsupported producer {:?}", subject.producer),
        ));
    }
    validate_endpoint("parent", &subject.parent)?;
    validate_endpoint("child", &subject.child)?;
    validate_nonempty("repository", &subject.repository)
        .map_err(|message| refuse("malformed_subject", message))?;
    if subject.parent.head_sha == subject.child.head_sha {
        return Err(refuse(
            "endpoint_identity_collision",
            "parent and child endpoints claim one head identity",
        ));
    }
    if subject.protected_main_state != PROTECTED_MAIN_NOT_EVALUATED {
        return Err(refuse(
            "protected_main_promotion",
            format!(
                "protected-main state {:?} is not permitted on a stack-local subject; the only \
                 legal value is {PROTECTED_MAIN_NOT_EVALUATED}",
                subject.protected_main_state
            ),
        ));
    }
    if subject.child_base_expected_head_sha != subject.parent.head_sha {
        return Err(refuse(
            "wrong_parent_base",
            format!(
                "child base expectation {} does not bind to the parent head {}",
                subject.child_base_expected_head_sha, subject.parent.head_sha
            ),
        ));
    }
    // Admission invariants re-checked at every validation, so an edited
    // serialized artifact can never smuggle a non-ancestor history or a
    // parent identity its own edge declaration does not assert (#13360
    // root cause 1).
    if !matches!(subject.history_relation, RelatedHistory::Ancestor) {
        return Err(refuse_non_ancestor_history(subject.history_relation));
    }
    if subject.edge.parent_pr_number != subject.parent.pr_number {
        return Err(refuse(
            "edge_declaration_invalid",
            format!(
                "declaration names parent PR {} while the parent endpoint is PR {}",
                subject.edge.parent_pr_number, subject.parent.pr_number
            ),
        ));
    }
    if let Some(declared_head) = &subject.edge.declared_parent_head_sha
        && declared_head != &subject.parent.head_sha
    {
        return Err(refuse(
            "edge_declaration_invalid",
            format!(
                "declaration pins parent head {declared_head} while the endpoint carries {}",
                subject.parent.head_sha
            ),
        ));
    }
    if subject.delta.bound_parent_tree != subject.parent.head_tree
        || subject.delta.bound_child_tree != subject.child.head_tree
    {
        return Err(refuse(
            "delta_unbound_to_trees",
            "child-only delta is not bound to both endpoint trees",
        ));
    }
    let recomputed = super::delta_fingerprint(
        &subject.delta.bound_parent_tree,
        &subject.delta.bound_child_tree,
        &subject.delta.paths,
    );
    if recomputed != subject.delta.fingerprint {
        return Err(refuse(
            "delta_fingerprint_mismatch",
            format!("declared delta fingerprint {} does not reconcile", subject.delta.fingerprint),
        ));
    }
    super::check_declared_scope(&subject.delta, &subject.edge)
        .map_err(|(code, message)| refuse(&code, message))?;
    Ok(())
}

/// Canonical byte identity of the subject; binds results to the exact
/// parent/child tree pair.
#[must_use]
pub fn subject_digest(subject: &StackIncrementSubjectV1) -> String {
    // Serialization of these structs is deterministic: struct fields serialize
    // in declaration order and every collection is ordered.
    match serde_json::to_vec(subject) {
        Ok(bytes) => sha256_hex(&bytes),
        // These payloads derive from plain owned strings and integers; a
        // serialization failure would be a programming defect surfaced by the
        // compile gate, never runtime data.
        Err(error) => format!("serialization-failed:{error}"),
    }
}

/// Re-check head currentness against freshly observed heads before any later
/// stage executes or publishes. Movement invalidates the subject.
///
/// # Errors
/// Returns a typed refusal when either endpoint moved since admission.
pub fn refresh_currentness(
    subject: &StackIncrementSubjectV1,
    observed_parent_head_sha: &str,
    observed_child_head_sha: &str,
) -> Result<(), StackSubjectCompileError> {
    if observed_parent_head_sha != subject.parent.head_sha {
        return Err(refuse(
            "parent_moved_since_admission",
            format!(
                "observed parent head {observed_parent_head_sha} no longer equals the admitted \
                 head {}",
                subject.parent.head_sha
            ),
        ));
    }
    if observed_child_head_sha != subject.child.head_sha {
        return Err(refuse(
            "child_moved_since_admission",
            format!(
                "observed child head {observed_child_head_sha} no longer equals the admitted \
                 head {}",
                subject.child.head_sha
            ),
        ));
    }
    Ok(())
}

/// One shared refusal for every non-ancestor history relation, used by both
/// compile-time admission and artifact revalidation.
fn refuse_non_ancestor_history(history: RelatedHistory) -> StackSubjectCompileError {
    let code = match history {
        RelatedHistory::Diverged | RelatedHistory::Unrelated => "unrelated_stack_history",
        _ => "history_not_proven",
    };
    refuse(
        code,
        format!("stack edges require an exact ancestor relation; observed {}", history.as_str()),
    )
}

fn validate_endpoint(role: &str, endpoint: &StackEndpoint) -> Result<(), StackSubjectCompileError> {
    if endpoint.pr_number == 0 {
        return Err(refuse("malformed_subject", format!("{role} PR number must be positive")));
    }
    validate_nonempty(&(format!("{role} branch")), &endpoint.branch)
        .map_err(|message| refuse("malformed_subject", message))?;
    validate_sha40(&(format!("{role} head_sha")), &endpoint.head_sha)
        .map_err(|message| refuse("malformed_subject", message))?;
    validate_sha40(&(format!("{role} head_tree")), &endpoint.head_tree)
        .map_err(|message| refuse("malformed_subject", message))?;
    Ok(())
}

/// Compile and fully validate one stack-local subject.
///
/// # Errors
/// Returns a typed refusal for every failed admission rule; see
/// [`STACK_SUBJECT_ERROR_CODES`].
pub fn compile_subject(
    input: StackSubjectInput,
) -> Result<StackIncrementSubjectV1, StackSubjectCompileError> {
    let StackSubjectInput {
        repository,
        event_id,
        parent,
        child,
        edge,
        child_base_expected_head_sha,
        observed_parent_head_sha,
        observed_child_head_sha,
        trust,
        history,
        delta,
    } = input;
    let Some(edge) = edge else {
        return Err(refuse(
            "edge_undeclared",
            "no explicit machine-readable edge declaration was supplied; branch names, titles, \
             labels, and paths can never admit a stack edge",
        ));
    };
    if edge.parent_pr_number != parent.pr_number {
        return Err(refuse(
            "edge_declaration_invalid",
            format!(
                "declaration names parent PR {} while the parent endpoint is PR {}",
                edge.parent_pr_number, parent.pr_number
            ),
        ));
    }
    if let Some(declared_head) = &edge.declared_parent_head_sha
        && declared_head != &parent.head_sha
    {
        return Err(refuse(
            "edge_declaration_invalid",
            format!(
                "declaration pins parent head {declared_head} while the endpoint carries {}",
                parent.head_sha
            ),
        ));
    }
    if !trust.same_repository_declared && !trust.external_context_admitted {
        return Err(refuse(
            "repository_untrusted",
            "endpoints do not declare one shared repository and no external context was \
             explicitly admitted",
        ));
    }
    if observed_parent_head_sha != parent.head_sha {
        return Err(refuse(
            "parent_moved_since_admission",
            format!(
                "parent head moved to {observed_parent_head_sha} after admission pinned {}",
                parent.head_sha
            ),
        ));
    }
    if observed_child_head_sha != child.head_sha {
        return Err(refuse(
            "child_moved_since_admission",
            format!(
                "child head moved to {observed_child_head_sha} after admission pinned {}",
                child.head_sha
            ),
        ));
    }
    if !matches!(history, RelatedHistory::Ancestor) {
        return Err(refuse_non_ancestor_history(history));
    }
    let subject = StackIncrementSubjectV1 {
        schema: STACK_INCREMENT_SUBJECT_SCHEMA.to_string(),
        producer: STACK_INCREMENT_PRODUCER.to_string(),
        repository,
        event_id,
        parent,
        child,
        edge,
        child_base_expected_head_sha,
        history_relation: history,
        delta,
        protected_main_state: PROTECTED_MAIN_NOT_EVALUATED.to_string(),
    };
    validate_subject(&subject)?;
    Ok(subject)
}

/// Parse the one machine-readable edge declaration allowed inside a child PR
/// body. Grammar (tokens separated by spaces on a single line):
///
/// ```text
/// stack-edge: dependency=programme_dependency parent-pr=<u64> scope=<csv|->
/// ```
///
/// Duplicate declarations, unknown tokens, unknown dependency kinds, and
/// zero parent numbers all refuse. Any other body content — titles, labels,
/// checklist prose — is never consulted.
///
/// # Errors
/// Returns [`StackSubjectCompileError`] codes `edge_undeclared`,
/// `edge_declaration_invalid`, or `malformed_subject`.
pub fn parse_stack_edge_declaration(
    body: &str,
) -> Result<StackEdgeDeclaration, StackSubjectCompileError> {
    let mut found: Option<Vec<String>> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix(super::STACK_EDGE_DECLARATION_PREFIX) else {
            continue;
        };
        if found.is_some() {
            return Err(refuse(
                "edge_declaration_invalid",
                "more than one stack-edge declaration is present",
            ));
        }
        let tokens: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
        found = Some(tokens);
    }
    let Some(tokens) = found else {
        return Err(refuse(
            "edge_undeclared",
            format!(
                "the child PR body carries no `{}` declaration line",
                super::STACK_EDGE_DECLARATION_PREFIX
            ),
        ));
    };
    let mut dependency: Option<EdgeKind> = None;
    let mut parent_pr_number: Option<u64> = None;
    let mut parent_head_sha: Option<String> = None;
    let mut scope_paths: Vec<String> = Vec::new();
    let mut scope_seen = false;
    for token in tokens {
        let Some((key, value)) = token.split_once('=') else {
            return Err(refuse(
                "edge_declaration_invalid",
                format!("declaration token {token:?} is not key=value"),
            ));
        };
        match key {
            "dependency" => {
                let parsed = EdgeKind::parse(value).ok_or_else(|| {
                    refuse("edge_declaration_invalid", format!("unknown dependency kind {value:?}"))
                })?;
                if dependency.replace(parsed).is_some() {
                    return Err(refuse(
                        "edge_declaration_invalid",
                        "dependency kind declared twice",
                    ));
                }
            }
            "parent-pr" => {
                let parsed: u64 = value.parse().map_err(|_| {
                    refuse(
                        "edge_declaration_invalid",
                        format!("parent-pr value {value:?} is not a positive number"),
                    )
                })?;
                if parsed == 0 || parent_pr_number.replace(parsed).is_some() {
                    return Err(refuse(
                        "edge_declaration_invalid",
                        "parent-pr must be positive and declared once",
                    ));
                }
            }
            "scope" => {
                if scope_seen {
                    return Err(refuse("edge_declaration_invalid", "scope declared twice"));
                }
                scope_seen = true;
                if value != "-" {
                    for entry in value.split(',') {
                        let entry = entry.trim();
                        if entry.is_empty() {
                            return Err(refuse(
                                "edge_declaration_invalid",
                                "scope list contains an empty entry",
                            ));
                        }
                        scope_paths.push(entry.to_string());
                    }
                }
            }
            "parent-head" => {
                validate_sha40("declared parent-head", value)
                    .map_err(|message| refuse("edge_declaration_invalid", message))?;
                if parent_head_sha.replace(value.to_string()).is_some() {
                    return Err(refuse("edge_declaration_invalid", "parent-head declared twice"));
                }
            }
            unknown => {
                return Err(refuse(
                    "edge_declaration_invalid",
                    format!("unknown declaration key {unknown:?}"),
                ));
            }
        }
    }
    let dependency = dependency.ok_or_else(|| {
        refuse("edge_declaration_invalid", "dependency kind missing from declaration")
    })?;
    let parent_pr_number = parent_pr_number
        .ok_or_else(|| refuse("edge_declaration_invalid", "parent-pr missing from declaration"))?;
    if !scope_seen {
        return Err(refuse(
            "edge_declaration_invalid",
            "scope missing from declaration; pass `scope=-` to declare an empty scope \
             explicitly",
        ));
    }
    Ok(StackEdgeDeclaration {
        dependency,
        parent_pr_number,
        scope_paths,
        declared_parent_head_sha: parent_head_sha,
    })
}

/// Resolve the head and tree SHAs of one revision in a checked-out
/// repository. Thin read-only Git adapter.
///
/// # Errors
/// Returns the failing Git stderr when the revision cannot resolve.
pub fn resolve_endpoint_live(
    repository: &Path,
    revision: &str,
) -> Result<(String, String), String> {
    let head = git_stdout(repository, &["rev-parse", "--verify", revision])?;
    let tree_revision = format!("{head}^{{tree}}");
    let tree = git_stdout(repository, &["rev-parse", "--verify", tree_revision.as_str()])?;
    Ok((head, tree))
}

fn git_stdout(repository: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .map_err(|error| format!("failed to spawn git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
