//! Typed workspace launch authority for the native DAP adapter.
//!
//! # Why this exists
//!
//! The adapter used to keep a single mutable `Option<PathBuf>` workspace root.
//! That one field had to stand for two different things:
//!
//! 1. the **trusted authority** an operator granted the adapter at startup, and
//! 2. the **effective boundary** in force for one debug session.
//!
//! Collapsing them meant a per-launch `workspaceRoot` argument was written back
//! over the startup grant, so a project-supplied value silently became the
//! adapter's authority for every later session. This module separates the two:
//! [`WorkspaceAuthority`] is established once and never mutated, and
//! [`resolve_session_boundary`] derives a fresh [`SessionBoundary`] per launch.
//!
//! Authority may only ever be *narrowed* by launch data. Nothing a client sends
//! — `workspaceRoot`, `cwd`, or the program's own parent directory — can create
//! or widen it.
//!
//! # What this boundary does not cover
//!
//! It confines the launch **`program`**, and the source paths a session
//! validates. It does not confine the other channels a launch can use to bring
//! in code, and describing it as if it did would overstate it:
//!
//! - the **interpreter**. `perlPath`/`perl` is taken from launch data and gated
//!   only on its base name, so a bounded launch can still execute an
//!   out-of-workspace binary named `perl` — which then chooses what the
//!   authorized script even means. Confining it to the workspace roots is not
//!   the fix (`/usr/bin/perl` is the normal case); it needs its own trust rule.
//! - the **environment**. Launch-supplied `env` entries reach the child
//!   unconditionally, so `PERL5LIB`/`PERL5OPT` can load code from outside the
//!   roots, at `perl -c` time.
//! - `setBreakpoints`, whose handler does not consult this boundary at all
//!   (#14593).
//!
//! Interpreter and environment confinement are #14601; both are pre-existing and
//! both are behavior changes for bounded adapters, so they are tracked rather
//! than folded in here.
//!
//! Controlling issue: #14587 (parent #8145).

use std::path::{Path, PathBuf};

use super::validate_path;

/// Why the adapter is running without a workspace boundary.
///
/// Both variants mean "unbounded", but they are not equally deliberate, and the
/// remaining #8145 work turns exactly one of them into a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnboundedGrant {
    /// An operator explicitly opted in via `--allow-unbounded-workspace`.
    OperatorFlag,
    /// No launch authority was configured at all.
    ///
    /// This is the legacy compatibility state: it reproduces the historical
    /// behavior of an adapter started with no workspace knowledge. It is named
    /// and warned rather than silent so #8145 can flip it to a refusal once
    /// clients supply startup authority.
    UnconfiguredDefault,
}

impl UnboundedGrant {
    /// Stable identity for logs and diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OperatorFlag => "operator-flag",
            Self::UnconfiguredDefault => "unconfigured-default",
        }
    }
}

/// The canonical trusted roots of a workspace-bound authority.
///
/// The inner field is private, and that is load-bearing rather than stylistic.
/// [`WorkspaceAuthority::owning_root`]'s determinism rests on the set being
/// canonical, deduplicated, and non-empty, and only
/// [`WorkspaceAuthority::from_startup`] establishes that.
///
/// `#[non_exhaustive]` alone does **not** hold that invariant. It blocks
/// external *construction* of a variant and forces `..` in patterns; it does
/// not block external *mutation* of a public field, so
///
/// ```ignore
/// // from a downstream crate, with a `pub roots: Vec<PathBuf>` field:
/// if let WorkspaceAuthority::WorkspaceBound { roots, .. } = &mut authority {
///     roots.clear();
/// }
/// ```
///
/// compiles and yields a bounded authority with no roots — reporting
/// `is_bounded() == true` while confining nothing. A private field is what
/// actually prevents it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedRoots(Vec<PathBuf>);

impl TrustedRoots {
    /// The canonical, deduplicated roots. Never empty.
    #[must_use]
    pub fn as_slice(&self) -> &[PathBuf] {
        &self.0
    }

    /// How many roots are configured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the root set is empty.
    ///
    /// Always `false` for a value [`WorkspaceAuthority::from_startup`] built;
    /// the accessor exists so callers need not assert the invariant themselves.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The adapter's launch authority, established once at startup.
///
/// There are exactly two modes. A launch never constructs or replaces one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAuthority {
    /// Launches are confined to one of these canonical trusted roots.
    ///
    /// Roots are canonicalized and deduplicated at construction, so no two
    /// entries denote the same directory. The payload's field is private, so a
    /// bounded authority can only come from
    /// [`WorkspaceAuthority::from_startup`] — see [`TrustedRoots`] for why
    /// `#[non_exhaustive]` is not sufficient on its own.
    #[non_exhaustive]
    WorkspaceBound {
        /// Canonical, deduplicated trusted roots. Never empty.
        roots: TrustedRoots,
    },
    /// Launches are not confined to any root.
    ///
    /// Sealed against external construction so unbounded access comes from
    /// [`WorkspaceAuthority::from_startup`]'s explicit inputs, never from a
    /// caller synthesising a grant. Mutating `grant` cannot widen anything —
    /// the authority is already unbounded — so the sealing here only needs to
    /// stop construction, which `#[non_exhaustive]` does.
    #[non_exhaustive]
    Unbounded {
        /// How the unbounded state came about.
        grant: UnboundedGrant,
    },
}

/// Errors establishing or applying workspace authority.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceAuthorityError {
    /// Trusted roots and an explicit unbounded grant were both requested.
    #[error(
        "contradictory workspace authority: --workspace-root confines launches to a directory \
         while --allow-unbounded-workspace removes the confinement. Pass one or the other."
    )]
    ContradictoryAuthority,

    /// A configured trusted root cannot be used as a boundary.
    #[error("workspace root is not usable as a trust boundary: {0}")]
    UnusableTrustedRoot(String),

    /// No trusted root owns the program being launched.
    #[error(
        "The script '{program}' is outside your workspace folder. Only scripts within a \
         configured workspace root can be debugged. Configured roots: {roots}."
    )]
    ProgramOutsideTrustedRoots {
        /// The rejected program path, as supplied by the client.
        program: String,
        /// Display form of the configured trusted roots.
        roots: String,
    },

    /// A launch-supplied `workspaceRoot` does not resolve to a real directory.
    ///
    /// Only reachable on an unbounded adapter, where there is no trusted root to
    /// resolve a relative value against.
    #[error(
        "The launch 'workspaceRoot' ('{launch_root}') does not resolve to an existing \
         directory, so it cannot confine this session. Set 'workspaceRoot' in your \
         launch.json to an absolute path that exists. Details: {detail}"
    )]
    UnusableLaunchRoot {
        /// The rejected launch-supplied root, as the client sent it.
        launch_root: String,
        /// The underlying resolution failure.
        detail: String,
    },

    /// A launch-supplied `workspaceRoot` fell outside every trusted root.
    #[error(
        "The launch 'workspaceRoot' ('{launch_root}') is outside your workspace folder and \
         cannot widen the configured boundary. Details: {detail}"
    )]
    LaunchRootWidensAuthority {
        /// The rejected launch-supplied root.
        launch_root: String,
        /// The underlying validation failure.
        detail: String,
    },
}

/// The workspace boundary in force for a single debug session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionBoundary {
    /// Every client-supplied path this session is confined to this root.
    ///
    /// Sealed against external construction so a boundary can only come from
    /// [`resolve_session_boundary`], which derives it from the startup
    /// authority.
    #[non_exhaustive]
    Bounded(PathBuf),
    /// No boundary is known; only path-shape checks apply.
    Unbounded,
}

impl SessionBoundary {
    /// The confining root, if this session has one.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        match self {
            Self::Bounded(root) => Some(root),
            Self::Unbounded => None,
        }
    }
}

impl WorkspaceAuthority {
    /// Establish authority from adapter startup inputs.
    ///
    /// `roots` come from a machine/user-owned source such as the adapter's
    /// `--workspace-root` flag. `allow_unbounded` is the operator's explicit
    /// `--allow-unbounded-workspace` opt-in.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceAuthorityError::ContradictoryAuthority`] when both
    /// inputs are supplied, and [`WorkspaceAuthorityError::UnusableTrustedRoot`]
    /// when a root does not resolve to an existing directory.
    pub fn from_startup(
        roots: &[PathBuf],
        allow_unbounded: bool,
    ) -> Result<Self, WorkspaceAuthorityError> {
        if !roots.is_empty() && allow_unbounded {
            return Err(WorkspaceAuthorityError::ContradictoryAuthority);
        }

        if roots.is_empty() {
            let grant = if allow_unbounded {
                UnboundedGrant::OperatorFlag
            } else {
                UnboundedGrant::UnconfiguredDefault
            };
            return Ok(Self::Unbounded { grant });
        }

        let mut canonical: Vec<PathBuf> = Vec::with_capacity(roots.len());
        for root in roots {
            let resolved = root.canonicalize().map_err(|error| {
                WorkspaceAuthorityError::UnusableTrustedRoot(format!(
                    "{} ({error})",
                    root.display()
                ))
            })?;
            if !resolved.is_dir() {
                return Err(WorkspaceAuthorityError::UnusableTrustedRoot(format!(
                    "{} is not a directory",
                    root.display()
                )));
            }
            if !canonical.contains(&resolved) {
                canonical.push(resolved);
            }
        }

        Ok(Self::WorkspaceBound { roots: TrustedRoots(canonical) })
    }

    /// Build a bounded authority from already-canonical roots, for tests only.
    ///
    /// This is the sole way to reach a degenerate root set, and it exists so the
    /// fail-closed handling of one can be proven. Production code must use
    /// [`WorkspaceAuthority::from_startup`], which cannot produce an empty set.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn bound_from_canonical_for_test(roots: Vec<PathBuf>) -> Self {
        Self::WorkspaceBound { roots: TrustedRoots(roots) }
    }

    /// The legacy state: no authority was configured.
    #[must_use]
    pub fn unconfigured() -> Self {
        Self::Unbounded { grant: UnboundedGrant::UnconfiguredDefault }
    }

    /// The canonical trusted roots, empty when unbounded.
    #[must_use]
    pub fn trusted_roots(&self) -> &[PathBuf] {
        match self {
            Self::WorkspaceBound { roots } => roots.as_slice(),
            Self::Unbounded { .. } => &[],
        }
    }

    /// How unbounded access was granted, or `None` when launches are confined.
    ///
    /// Callers outside this crate cannot pattern-match the sealed variants, so
    /// this is the supported way to tell an operator's deliberate
    /// `--allow-unbounded-workspace` from the unconfigured legacy default.
    #[must_use]
    pub const fn unbounded_grant(&self) -> Option<UnboundedGrant> {
        match self {
            Self::WorkspaceBound { .. } => None,
            Self::Unbounded { grant, .. } => Some(*grant),
        }
    }

    /// Whether launches are confined to a trusted root.
    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        matches!(self, Self::WorkspaceBound { .. })
    }

    /// Stable identity for the startup log line.
    #[must_use]
    pub const fn mode_identity(&self) -> &'static str {
        match self {
            Self::WorkspaceBound { .. } => "workspace-bound",
            Self::Unbounded { .. } => "unbounded",
        }
    }

    /// The deepest trusted root that contains `path`.
    ///
    /// Deepest-wins makes nested trusted roots deterministic: `/ws` and
    /// `/ws/sub` both contain `/ws/sub/a.pl`, and the tighter one is selected.
    /// Because roots are canonical and deduplicated, two distinct roots can
    /// never be equally deep and both contain the same path, so the answer is
    /// unique.
    ///
    /// # Panics (debug builds)
    ///
    /// `path` **must be absolute**, and the uniqueness argument above holds only
    /// then. `validate_path` joins a relative candidate with the root it is
    /// checked against, so a relative `path` is contained in *every* root: ties
    /// become real and `max_by_key` silently returns the last maximum, making
    /// the answer depend on registration order. Callers resolve the program to
    /// an absolute path first; the assertion pins that so a future caller cannot
    /// reintroduce the ambiguity unnoticed.
    #[must_use]
    pub fn owning_root(&self, path: &Path) -> Option<&Path> {
        debug_assert!(
            path.is_absolute(),
            "owning_root requires an absolute path; a relative one matches every root \
             and makes the selection order-dependent (got {})",
            path.display()
        );
        self.trusted_roots()
            .iter()
            .filter(|root| validate_path(path, root).is_ok())
            .max_by_key(|root| root.components().count())
            .map(PathBuf::as_path)
    }
}

/// Derive the boundary for one debug session.
///
/// This is a pure function of the startup authority and this launch's
/// arguments. It never mutates the authority, so a narrowing `workspaceRoot`
/// cannot survive into a later session.
///
/// Under [`WorkspaceAuthority::WorkspaceBound`]:
///
/// - a launch-supplied `launch_root` must resolve inside some trusted root, and
///   that narrowed path becomes the boundary; otherwise the launch is refused
///   rather than widened;
/// - with no `launch_root`, the deepest trusted root owning `program` is the
///   boundary, and a program owned by no trusted root is refused.
///
/// Under [`WorkspaceAuthority::Unbounded`] a launch-supplied `launch_root` still
/// confines this one session — narrowing from "no boundary" is always safe —
/// and is still discarded when the session ends. It must canonicalize, so the
/// boundary is an absolute directory that exists rather than a client string
/// whose meaning depends on the adapter's own working directory.
///
/// `program`'s own parent directory and the launch `cwd` are deliberately not
/// consulted: a boundary derived from the thing it is meant to confine would be
/// self-validating.
///
/// # Errors
///
/// Returns [`WorkspaceAuthorityError::LaunchRootWidensAuthority`] or
/// [`WorkspaceAuthorityError::ProgramOutsideTrustedRoots`] when the launch
/// cannot be confined, and [`WorkspaceAuthorityError::UnusableLaunchRoot`] when
/// an unbounded adapter's launch root does not resolve to an existing directory.
pub fn resolve_session_boundary(
    authority: &WorkspaceAuthority,
    program: &Path,
    launch_root: Option<&Path>,
) -> Result<SessionBoundary, WorkspaceAuthorityError> {
    match authority {
        WorkspaceAuthority::Unbounded { .. } => match launch_root {
            None => Ok(SessionBoundary::Unbounded),
            // Narrowing from "no boundary" is safe, but only to a boundary that
            // means the same thing everywhere. Taking the client's string
            // verbatim does not: a relative root is re-anchored later against
            // *this process's* working directory — for an editor-spawned
            // adapter, wherever the extension host happened to be — and a
            // non-existent root silently refuses every source path in the
            // session. The bounded branch below yields `validate_path`'s
            // canonical result; this one must be just as concrete.
            Some(root) => {
                let resolved = root.canonicalize().map_err(|error| {
                    WorkspaceAuthorityError::UnusableLaunchRoot {
                        launch_root: root.display().to_string(),
                        detail: error.to_string(),
                    }
                })?;
                Ok(SessionBoundary::Bounded(require_directory(root, resolved)?))
            }
        },
        WorkspaceAuthority::WorkspaceBound { roots } => {
            let roots = roots.as_slice();
            if let Some(requested) = launch_root {
                // The launch root is checked against the trust set, not against
                // the program's owner, so a client cannot pick a root by
                // pointing at a program.
                let narrowed = roots
                    .iter()
                    .find_map(|root| validate_path(requested, root).ok())
                    .ok_or_else(|| WorkspaceAuthorityError::LaunchRootWidensAuthority {
                        launch_root: requested.display().to_string(),
                        detail: format!(
                            "no configured workspace root contains it (configured roots: {})",
                            display_roots(roots)
                        ),
                    })?;
                return Ok(SessionBoundary::Bounded(require_directory(requested, narrowed)?));
            }

            authority
                .owning_root(program)
                .map(|root| SessionBoundary::Bounded(root.to_path_buf()))
                .ok_or_else(|| WorkspaceAuthorityError::ProgramOutsideTrustedRoots {
                    program: program.display().to_string(),
                    roots: display_roots(roots),
                })
        }
    }
}

/// A boundary must be a directory, on every path that can produce one.
///
/// `from_startup` already refuses a non-directory trusted root; a launch root
/// has to meet the same bar. `canonicalize` succeeds for a file, and
/// `validate_path` only checks containment, so without this a client could send
/// `workspaceRoot: "<root>/script.pl"` and get a "boundary" that admits exactly
/// one file and rejects every sibling. Pointed at the program itself it is worse
/// than useless: the launch would authorize itself, which is precisely the
/// self-validating boundary this module exists to prevent.
fn require_directory(
    requested: &Path,
    resolved: PathBuf,
) -> Result<PathBuf, WorkspaceAuthorityError> {
    if resolved.is_dir() {
        return Ok(resolved);
    }
    Err(WorkspaceAuthorityError::UnusableLaunchRoot {
        launch_root: requested.display().to_string(),
        detail: "a workspace root must be a directory".to_string(),
    })
}

fn display_roots(roots: &[PathBuf]) -> String {
    roots.iter().map(|root| root.display().to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_err};
    use std::fs;

    fn dir(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        must(fs::create_dir_all(&path));
        must(path.canonicalize())
    }

    fn file(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        must(fs::write(&path, "print 'x';\n"));
        path
    }

    fn bound(roots: &[PathBuf]) -> WorkspaceAuthority {
        must(WorkspaceAuthority::from_startup(roots, false))
    }

    // --- startup authority ---

    #[test]
    fn no_inputs_resolve_to_the_named_legacy_default() {
        let authority = must(WorkspaceAuthority::from_startup(&[], false));
        assert_eq!(
            authority,
            WorkspaceAuthority::Unbounded { grant: UnboundedGrant::UnconfiguredDefault }
        );
        assert!(!authority.is_bounded());
    }

    #[test]
    fn operator_opt_in_is_distinguishable_from_the_legacy_default() {
        let explicit = must(WorkspaceAuthority::from_startup(&[], true));
        assert_eq!(explicit, WorkspaceAuthority::Unbounded { grant: UnboundedGrant::OperatorFlag });
        assert_ne!(explicit, WorkspaceAuthority::unconfigured());
    }

    #[test]
    fn roots_and_unbounded_opt_in_together_are_refused() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        assert_eq!(
            must_err(WorkspaceAuthority::from_startup(&[root], true)),
            WorkspaceAuthorityError::ContradictoryAuthority
        );
    }

    #[test]
    fn a_missing_root_is_refused_rather_than_silently_dropped() {
        let temp = must(tempfile::tempdir());
        let missing = temp.path().join("absent");
        let error = must_err(WorkspaceAuthority::from_startup(&[missing], false));
        assert!(
            matches!(error, WorkspaceAuthorityError::UnusableTrustedRoot(_)),
            "missing root must fail startup, got {error:?}"
        );
    }

    #[test]
    fn a_file_cannot_serve_as_a_trust_boundary() {
        let temp = must(tempfile::tempdir());
        let script = file(temp.path(), "a.pl");
        let error = must_err(WorkspaceAuthority::from_startup(&[script], false));
        assert!(
            matches!(error, WorkspaceAuthorityError::UnusableTrustedRoot(_)),
            "a file root must fail startup, got {error:?}"
        );
    }

    #[test]
    fn duplicate_roots_collapse_to_one_entry() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let authority = bound(&[root.clone(), root.clone()]);
        assert_eq!(authority.trusted_roots(), std::slice::from_ref(&root));
    }

    // --- owning-root selection ---

    #[test]
    fn nested_roots_select_the_deepest_owner() {
        let temp = must(tempfile::tempdir());
        let outer = dir(temp.path(), "ws");
        let inner = dir(&outer, "sub");
        let script = file(&inner, "a.pl");

        // Registration order must not decide the answer.
        for roots in [vec![outer.clone(), inner.clone()], vec![inner.clone(), outer.clone()]] {
            let authority = bound(&roots);
            assert_eq!(
                authority.owning_root(&script),
                Some(inner.as_path()),
                "deepest containing root must win regardless of order"
            );
        }
    }

    #[test]
    fn sibling_roots_do_not_own_each_others_programs() {
        let temp = must(tempfile::tempdir());
        let alpha = dir(temp.path(), "alpha");
        let beta = dir(temp.path(), "beta");
        let script = file(&beta, "a.pl");

        assert_eq!(bound(std::slice::from_ref(&alpha)).owning_root(&script), None);
        assert_eq!(bound(&[alpha, beta.clone()]).owning_root(&script), Some(beta.as_path()));
    }

    #[test]
    fn a_component_prefix_collision_does_not_grant_ownership() {
        let temp = must(tempfile::tempdir());
        let workspace = dir(temp.path(), "workspace");
        let lookalike = dir(temp.path(), "workspace-evil");
        let script = file(&lookalike, "a.pl");

        assert_eq!(
            bound(&[workspace]).owning_root(&script),
            None,
            "'workspace-evil' must not be owned by 'workspace'"
        );
    }

    // --- per-launch boundary ---

    #[test]
    fn a_bound_launch_without_a_launch_root_uses_its_owning_root() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let script = file(&root, "a.pl");

        let boundary =
            must(resolve_session_boundary(&bound(std::slice::from_ref(&root)), &script, None));
        assert_eq!(boundary, SessionBoundary::Bounded(root));
    }

    #[test]
    fn a_launch_root_inside_a_trusted_root_narrows_the_session() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let inner = dir(&root, "sub");
        let script = file(&inner, "a.pl");

        let boundary = must(resolve_session_boundary(&bound(&[root]), &script, Some(&inner)));
        assert_eq!(boundary, SessionBoundary::Bounded(inner));
    }

    #[test]
    fn a_launch_root_outside_every_trusted_root_is_refused() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let outside = dir(temp.path(), "elsewhere");
        let script = file(&outside, "a.pl");

        let error = must_err(resolve_session_boundary(&bound(&[root]), &script, Some(&outside)));
        assert!(
            matches!(error, WorkspaceAuthorityError::LaunchRootWidensAuthority { .. }),
            "widening launch root must be refused, got {error:?}"
        );
    }

    #[test]
    fn a_launch_root_may_not_cross_from_one_trusted_root_to_a_sibling_of_them_all() {
        let temp = must(tempfile::tempdir());
        let alpha = dir(temp.path(), "alpha");
        let beta = dir(temp.path(), "beta");
        // The parent of both roots is not itself trusted.
        let parent = must(temp.path().canonicalize());
        let script = file(&alpha, "a.pl");

        let error =
            must_err(resolve_session_boundary(&bound(&[alpha, beta]), &script, Some(&parent)));
        assert!(
            matches!(error, WorkspaceAuthorityError::LaunchRootWidensAuthority { .. }),
            "the common parent of two roots must not become the boundary, got {error:?}"
        );
    }

    #[test]
    fn a_launch_root_narrows_to_its_own_trusted_root_not_the_program_s() {
        // The launch root is checked against the trust set, not against the
        // program's owner, so under multiple roots a client may name a root
        // that does not contain its program. That is a narrowing, not a
        // widening: the boundary becomes root B, and `launch_debugger` then
        // refuses the program for being outside it (proven end to end by
        // `a_launch_root_from_another_root_cannot_launch_an_outside_program`
        // in tests/dap_launch_security_test.rs). This test pins the gate's
        // actual semantics so the asymmetry cannot change unnoticed.
        let temp = must(tempfile::tempdir());
        let alpha = dir(temp.path(), "alpha");
        let beta = dir(temp.path(), "beta");
        let script = file(&alpha, "a.pl");

        let boundary = must(resolve_session_boundary(
            &bound(&[alpha.clone(), beta.clone()]),
            &script,
            Some(&beta),
        ));
        assert_eq!(
            boundary,
            SessionBoundary::Bounded(beta),
            "a launch root inside a trusted root confines the session to that root"
        );
        assert_ne!(
            boundary,
            SessionBoundary::Bounded(alpha),
            "the program's own root must not be substituted for the requested one"
        );
    }

    #[test]
    fn a_traversal_launch_root_is_refused() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let script = file(&root, "a.pl");
        let escape = root.join("..").join("..");

        let error = must_err(resolve_session_boundary(&bound(&[root]), &script, Some(&escape)));
        assert!(
            matches!(error, WorkspaceAuthorityError::LaunchRootWidensAuthority { .. }),
            "a '..' launch root must be refused, got {error:?}"
        );
    }

    #[test]
    fn a_program_owned_by_no_trusted_root_is_refused_before_any_process_work() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let outside = dir(temp.path(), "elsewhere");
        let script = file(&outside, "a.pl");

        let error = must_err(resolve_session_boundary(&bound(&[root]), &script, None));
        assert!(
            matches!(error, WorkspaceAuthorityError::ProgramOutsideTrustedRoots { .. }),
            "expected an ownership refusal, got {error:?}"
        );
    }

    #[test]
    fn the_refusal_message_names_the_workspace_boundary() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let outside = dir(temp.path(), "elsewhere");
        let script = file(&outside, "a.pl");

        let message =
            must_err(resolve_session_boundary(&bound(&[root]), &script, None)).to_string();
        assert!(
            message.contains("outside your workspace"),
            "clients match on the workspace wording, got: {message}"
        );
    }

    #[test]
    fn an_unbounded_launch_stays_unbounded_without_a_launch_root() {
        let temp = must(tempfile::tempdir());
        let script = file(temp.path(), "a.pl");

        for grant in [UnboundedGrant::OperatorFlag, UnboundedGrant::UnconfiguredDefault] {
            let boundary = must(resolve_session_boundary(
                &WorkspaceAuthority::Unbounded { grant },
                &script,
                None,
            ));
            assert_eq!(boundary, SessionBoundary::Unbounded, "grant {grant:?}");
        }
    }

    #[test]
    fn an_unbounded_adapter_still_honors_a_launch_supplied_root() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let script = file(&root, "a.pl");

        let boundary = must(resolve_session_boundary(
            &WorkspaceAuthority::unconfigured(),
            &script,
            Some(&root),
        ));
        assert_eq!(boundary, SessionBoundary::Bounded(must(root.canonicalize())));
    }

    /// An unbounded adapter's launch root becomes a concrete directory.
    ///
    /// The bounded branch yields `validate_path`'s canonical result. This branch
    /// used to store the client's string verbatim, so a relative `workspaceRoot`
    /// stayed relative and was re-anchored later against *this process's*
    /// working directory — for an editor-spawned adapter, wherever the extension
    /// host happened to be. A boundary whose meaning depends on that is not a
    /// boundary.
    #[test]
    fn an_unbounded_launch_root_is_canonicalized_not_stored_verbatim() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let script = file(&root, "a.pl");

        // Spell the same directory as `<root>/sub/..`. A `.` component will not
        // do: `Path`'s `PartialEq` compares `components()`, which silently drops
        // `CurDir`, so `<root>/.` and `<root>` compare *equal* and the assertion
        // below would hold even against a verbatim store. `ParentDir` is not
        // normalized away, so this spelling is only equal after canonicalizing.
        let nested = dir(&root, "sub");
        let noncanonical = nested.join("..");
        let boundary = must(resolve_session_boundary(
            &WorkspaceAuthority::unconfigured(),
            &script,
            Some(&noncanonical),
        ));
        let SessionBoundary::Bounded(resolved) = boundary else {
            unreachable!("a launch root must confine the session")
        };
        assert!(resolved.is_absolute(), "the session boundary must be absolute, got {resolved:?}");
        assert_eq!(resolved, must(root.canonicalize()));
    }

    /// A launch root that does not resolve refuses the launch.
    ///
    /// Storing it verbatim made every source check in the session fail against a
    /// directory that does not exist — a client-triggered, session-wide brick
    /// with no message naming the cause. Refusing up front says what to fix.
    #[test]
    fn an_unbounded_launch_root_that_does_not_resolve_is_refused() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let script = file(&root, "a.pl");
        let missing = temp.path().join("absent");

        let error = must_err(resolve_session_boundary(
            &WorkspaceAuthority::unconfigured(),
            &script,
            Some(&missing),
        ));
        assert!(
            matches!(error, WorkspaceAuthorityError::UnusableLaunchRoot { .. }),
            "expected an unusable-launch-root refusal, got {error:?}"
        );
    }

    /// A file-valued launch root is refused, on both branches.
    ///
    /// `canonicalize` succeeds for a file and `validate_path` only checks
    /// containment, so without an explicit directory check a client could send
    /// `workspaceRoot: "<root>/script.pl"` and get a boundary admitting exactly
    /// one file. Pointed at the program, the launch would authorize itself —
    /// the self-validating boundary this module exists to prevent.
    /// `from_startup` already refuses a non-directory trusted root; a launch
    /// root has to meet the same bar.
    #[test]
    fn a_file_valued_launch_root_is_refused_on_both_branches() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let script = file(&root, "a.pl");

        for authority in [
            WorkspaceAuthority::unconfigured(),
            must(WorkspaceAuthority::from_startup(&[root], false)),
        ] {
            let error = must_err(resolve_session_boundary(&authority, &script, Some(&script)));
            assert!(
                matches!(error, WorkspaceAuthorityError::UnusableLaunchRoot { .. }),
                "a program must not be able to serve as its own boundary, got {error:?}"
            );
        }
    }

    /// A relative launch root is refused rather than silently re-anchored.
    #[test]
    fn an_unbounded_relative_launch_root_does_not_anchor_to_the_process_directory() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let script = file(&root, "a.pl");

        let boundary = resolve_session_boundary(
            &WorkspaceAuthority::unconfigured(),
            &script,
            Some(Path::new("definitely-not-a-real-relative-root")),
        );
        match boundary {
            Err(WorkspaceAuthorityError::UnusableLaunchRoot { .. }) => {}
            other => {
                unreachable!("a relative, non-existent launch root must be refused: {other:?}")
            }
        }
    }

    // --- negative controls: nothing in launch data creates authority ---

    #[test]
    fn resolving_a_boundary_never_mutates_the_authority() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let inner = dir(&root, "sub");
        let script = file(&inner, "a.pl");

        let authority = bound(std::slice::from_ref(&root));
        let before = authority.clone();

        let narrowed = must(resolve_session_boundary(&authority, &script, Some(&inner)));
        assert_eq!(narrowed, SessionBoundary::Bounded(inner));

        assert_eq!(authority, before, "a narrowing launch must not rewrite the trust set");

        // The next session, with no launch root, is bounded by the original
        // trusted root again — the narrowing did not persist.
        let sibling = file(&root, "b.pl");
        let next = must(resolve_session_boundary(&authority, &sibling, None));
        assert_eq!(next, SessionBoundary::Bounded(root));
    }

    #[test]
    fn a_program_parent_directory_cannot_become_the_boundary() {
        let temp = must(tempfile::tempdir());
        let root = dir(temp.path(), "ws");
        let outside = dir(temp.path(), "elsewhere");
        let script = file(&outside, "a.pl");

        // `outside` is exactly `script.parent()`. If the program's own directory
        // could authorize itself, this would resolve instead of refusing.
        let error = must_err(resolve_session_boundary(&bound(&[root]), &script, None));
        assert!(
            matches!(error, WorkspaceAuthorityError::ProgramOutsideTrustedRoots { .. }),
            "the program's parent directory must not authorize its own launch, got {error:?}"
        );
    }
}
