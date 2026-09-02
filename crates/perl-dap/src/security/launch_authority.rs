//! Typed launch-authority startup contract (#8656).
//!
//! Every prospective debug session has exactly one explicit authority mode
//! decided at adapter startup, before any launch request can use paths:
//!
//! ```text
//! workspace_bound { trusted_roots }
//! | explicit_unbounded { user_owned_acknowledgement }
//! ```
//!
//! No implicit or `None` mode remains for the native launch lifecycle: a
//! [`DapServer`](crate::server::DapServer) without either trusted roots or an
//! explicit unbounded acknowledgement refuses every debuggee launch before a
//! process can spawn.
//!
//! Authority inputs are user/machine-owned adapter startup configuration. They
//! are structurally separate from DAP launch arguments and opened-project data:
//! a launch argument can never create, widen, or replace authority. A
//! launch-args `workspaceRoot` may only narrow an existing workspace-bound
//! authority, and project-controlled `allowUnbounded` cannot exist because the
//! acknowledgement type is only constructible from user-owned
//! [`LaunchAuthoritySource`] inputs.
//!
//! The resolved [`LaunchAuthority`] stores one immutable authority identity for
//! the session. Each launch begins a new session generation; the identity and
//! mode themselves never change without a new adapter startup. Receipts expose
//! stable identities and counts, never private absolute paths.

use sha2::{Digest, Sha256};
use std::fs::Metadata;
use std::path::{Path, PathBuf};

/// User/machine-owned source of a startup authority decision.
///
/// This enumeration deliberately has no project-controlled variant: authority
/// originates before and outside any DAP launch request or opened-project data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchAuthoritySource {
    /// Explicit operator command-line flags (for example `--trusted-root`,
    /// `--allow-unbounded`).
    CommandLine,
    /// Host/editor or machine configuration supplied at adapter startup.
    HostSetting,
}

impl LaunchAuthoritySource {
    /// Stable receipt label for the authority source.
    pub fn label(self) -> &'static str {
        match self {
            Self::CommandLine => "command_line",
            Self::HostSetting => "host_setting",
        }
    }
}

/// Explicit, user-owned acknowledgement that sessions may run without a
/// workspace boundary.
///
/// The acknowledgement is recorded in the authority identity and receipt so an
/// unbounded session is always visible as a deliberate operator decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnboundedAcknowledgement {
    /// User/machine-owned surface that carried the acknowledgement.
    pub source: LaunchAuthoritySource,
    /// Stable operator-facing note recorded in receipts (never private data).
    pub note: String,
}

impl UnboundedAcknowledgement {
    /// Create an acknowledgement from a user-owned source.
    pub fn new(source: LaunchAuthoritySource, note: impl Into<String>) -> Self {
        Self { source, note: note.into() }
    }
}

/// Adapter startup inputs for the launch-authority decision.
///
/// Constructed only from user/machine-owned configuration surfaces. Fields are
/// consumed once by [`LaunchAuthority::resolve`] at server construction.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LaunchAuthorityStartup {
    /// Trusted root directories supplied by user/machine-owned configuration.
    pub trusted_roots: Vec<PathBuf>,
    /// Explicit unbounded acknowledgement, when the operator enabled it.
    pub allow_unbounded: Option<UnboundedAcknowledgement>,
}

/// Authority mode for a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchAuthorityMode {
    /// Launch paths must stay inside the configured trusted-root set.
    WorkspaceBound,
    /// An operator explicitly accepted unbounded launch paths for the session.
    ExplicitUnbounded,
}

impl LaunchAuthorityMode {
    /// Stable receipt label for the mode.
    pub fn label(self) -> &'static str {
        match self {
            Self::WorkspaceBound => "workspace_bound",
            Self::ExplicitUnbounded => "explicit_unbounded",
        }
    }
}

/// A canonicalized, validated trusted root with its stable identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedRoot {
    canonical: PathBuf,
    identity: String,
    filesystem_identity: FilesystemIdentity,
}

/// Identity of the directory object captured at startup.  Pathnames are not
/// sufficient authority: a root can be renamed and replaced while the adapter
/// is alive.  Rechecking this identity makes such retargeting fail closed.
#[derive(Clone, Debug, Eq, PartialEq)]
struct FilesystemIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    created: Option<std::time::SystemTime>,
    #[cfg(windows)]
    modified: Option<std::time::SystemTime>,
    #[cfg(not(any(unix, windows)))]
    canonical: PathBuf,
}

impl FilesystemIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            return Self { device: metadata.dev(), inode: metadata.ino() };
        }
        #[cfg(windows)]
        {
            return Self { created: metadata.created().ok(), modified: metadata.modified().ok() };
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self { canonical: PathBuf::new() }
        }
    }
}

impl TrustedRoot {
    /// Canonical absolute path of the root (used for validation, not receipts).
    pub fn canonical(&self) -> &Path {
        &self.canonical
    }

    /// Stable short identity of the root for receipts.
    pub fn identity(&self) -> &str {
        &self.identity
    }
}

/// Startup authority resolution failures.
///
/// Every variant fails closed: no authority is produced unless the inputs name
/// exactly one explicit mode with valid roots or a valid acknowledgement.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum LaunchAuthorityError {
    /// Neither trusted roots nor an explicit unbounded acknowledgement were
    /// configured. The adapter must refuse to start.
    #[error(
        "no launch authority configured: pass trusted roots (--trusted-root) or an explicit \
         unbounded acknowledgement (--allow-unbounded) before starting the adapter"
    )]
    NoAuthorityConfigured,
    /// Trusted roots and an explicit unbounded acknowledgement were both
    /// configured; the authority mode would be ambiguous.
    #[error(
        "ambiguous launch authority: trusted roots and --allow-unbounded are mutually exclusive"
    )]
    AmbiguousAuthorityMode,
    /// A trusted-root path does not exist.
    #[error("trusted root {path:?} does not exist")]
    TrustedRootNotFound {
        /// The offending raw path.
        path: PathBuf,
    },
    /// A trusted-root path exists but is not a directory.
    #[error("trusted root {path:?} is not a directory")]
    TrustedRootNotADirectory {
        /// The offending raw path.
        path: PathBuf,
    },
    /// A trusted-root path is a symbolic link; links are rejected so the
    /// validated boundary cannot be retargeted after startup.
    #[error("trusted root {path:?} is a symbolic link, which cannot be a trusted root")]
    TrustedRootSymlink {
        /// The offending raw path.
        path: PathBuf,
    },
    /// The same trusted-root input appeared twice.
    #[error("trusted root {path:?} is duplicated")]
    DuplicateTrustedRoot {
        /// The duplicated raw path.
        path: PathBuf,
    },
    /// Two different trusted-root inputs canonicalize to the same directory.
    #[error("trusted roots {first:?} and {second:?} alias the same directory {canonical:?}")]
    TrustedRootAliasConflict {
        /// First recorded raw spelling.
        first: PathBuf,
        /// Second raw spelling of the same directory.
        second: PathBuf,
        /// The shared canonical directory.
        canonical: PathBuf,
    },
    /// The explicit unbounded acknowledgement is malformed.
    #[error("invalid unbounded acknowledgement: {0}")]
    InvalidAcknowledgement(String),
}

/// Resolved launch authority for the adapter.
///
/// The mode and identity are immutable for the life of the adapter; each launch
/// begins a new session generation via [`LaunchAuthority::begin_session`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchAuthority {
    mode: LaunchAuthorityMode,
    roots: Vec<TrustedRoot>,
    acknowledgement: Option<UnboundedAcknowledgement>,
    identity: String,
    generation: u64,
}

/// Bounded receipt describing the session authority without private paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchAuthorityReceipt {
    /// Authority mode label.
    pub mode: &'static str,
    /// Number of trusted roots (zero for unbounded mode).
    pub trusted_root_count: usize,
    /// Stable identities of the trusted roots, sorted.
    pub trusted_root_identities: Vec<String>,
    /// Acknowledgement identity, when the session is explicitly unbounded.
    pub acknowledgement_identity: Option<String>,
    /// Immutable authority identity for the session.
    pub authority_identity: String,
    /// Current session generation (resets on the next launch).
    pub session_generation: u64,
}

fn short_identity(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let hex = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    hex[..16].to_string()
}

fn canonicalize_trusted_root(raw: &Path) -> Result<PathBuf, LaunchAuthorityError> {
    let metadata = std::fs::symlink_metadata(raw)
        .map_err(|_| LaunchAuthorityError::TrustedRootNotFound { path: raw.to_path_buf() })?;
    if metadata.file_type().is_symlink() {
        return Err(LaunchAuthorityError::TrustedRootSymlink { path: raw.to_path_buf() });
    }
    if !metadata.is_dir() {
        return Err(LaunchAuthorityError::TrustedRootNotADirectory { path: raw.to_path_buf() });
    }
    std::fs::canonicalize(raw)
        .map_err(|_| LaunchAuthorityError::TrustedRootNotFound { path: raw.to_path_buf() })
}

fn trusted_root_is_current(root: &TrustedRoot) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(&root.canonical) else {
        return false;
    };
    metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && FilesystemIdentity::from_metadata(&metadata) == root.filesystem_identity
}

/// Return the first recorded raw input whose canonical directory matches
/// `canonical`, i.e. an alias of a directory already trusted through a
/// different spelling.
fn detect_trusted_root_alias(
    seen_canonical: &[(PathBuf, PathBuf)],
    canonical: &Path,
) -> Option<PathBuf> {
    seen_canonical
        .iter()
        .find(|(_, existing)| existing == &canonical)
        .map(|(input, _)| input.clone())
}

impl LaunchAuthority {
    /// Resolve the startup authority decision from user/machine-owned inputs.
    ///
    /// Fails closed unless exactly one authority mode is configured with
    /// valid, conflict-free inputs.
    pub fn resolve(startup: &LaunchAuthorityStartup) -> Result<Self, LaunchAuthorityError> {
        if startup.allow_unbounded.is_some() && !startup.trusted_roots.is_empty() {
            return Err(LaunchAuthorityError::AmbiguousAuthorityMode);
        }
        if let Some(acknowledgement) = startup.allow_unbounded.as_ref() {
            if acknowledgement.note.trim().is_empty() {
                return Err(LaunchAuthorityError::InvalidAcknowledgement(
                    "the acknowledgement note must not be empty".to_string(),
                ));
            }
            return Ok(Self {
                mode: LaunchAuthorityMode::ExplicitUnbounded,
                roots: Vec::new(),
                acknowledgement: Some(acknowledgement.clone()),
                identity: Self::identity_input(
                    LaunchAuthorityMode::ExplicitUnbounded,
                    &[],
                    Some(acknowledgement),
                ),
                generation: 1,
            });
        }
        if startup.trusted_roots.is_empty() {
            return Err(LaunchAuthorityError::NoAuthorityConfigured);
        }

        let mut roots: Vec<TrustedRoot> = Vec::new();
        let mut seen_canonical: Vec<(PathBuf, PathBuf)> = Vec::new();
        for raw in &startup.trusted_roots {
            if seen_canonical.iter().any(|(input, _)| input == raw) {
                return Err(LaunchAuthorityError::DuplicateTrustedRoot { path: raw.clone() });
            }
            let canonical = canonicalize_trusted_root(raw)?;
            if let Some(first) = detect_trusted_root_alias(&seen_canonical, &canonical) {
                return Err(LaunchAuthorityError::TrustedRootAliasConflict {
                    first,
                    second: raw.clone(),
                    canonical,
                });
            }
            seen_canonical.push((raw.clone(), canonical.clone()));
            let filesystem_identity = FilesystemIdentity::from_metadata(
                &std::fs::symlink_metadata(&canonical)
                    .map_err(|_| LaunchAuthorityError::TrustedRootNotFound { path: raw.clone() })?,
            );
            roots.push(TrustedRoot {
                identity: short_identity(&canonical.to_string_lossy()),
                canonical,
                filesystem_identity,
            });
        }
        roots.sort_by(|left, right| left.identity.cmp(&right.identity));

        Ok(Self {
            mode: LaunchAuthorityMode::WorkspaceBound,
            identity: Self::identity_input(LaunchAuthorityMode::WorkspaceBound, &roots, None),
            roots,
            acknowledgement: None,
            generation: 1,
        })
    }

    fn identity_input(
        mode: LaunchAuthorityMode,
        roots: &[TrustedRoot],
        acknowledgement: Option<&UnboundedAcknowledgement>,
    ) -> String {
        // One canonical spelling of every identity input: mode label, sorted
        // canonical root paths, and the acknowledgement source/note. The same
        // startup decision therefore always yields the same authority identity.
        let mut input = String::new();
        input.push_str("perl_dap.launch_authority.v1\x1f");
        input.push_str(mode.label());
        input.push('\x1f');
        for root in roots {
            input.push_str(&root.canonical.to_string_lossy());
            input.push('\x1f');
        }
        if let Some(acknowledgement) = acknowledgement {
            input.push_str(acknowledgement.source.label());
            input.push('\x1f');
            input.push_str(acknowledgement.note.trim());
            input.push('\x1f');
        }
        let digest = Sha256::digest(input.as_bytes());
        let hex = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        format!("sha256:{hex}")
    }

    /// Immutable authority identity for the session.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Authority mode for the session.
    pub fn mode(&self) -> LaunchAuthorityMode {
        self.mode
    }

    /// Trusted roots, in stable identity order (empty for unbounded mode).
    pub fn trusted_roots(&self) -> &[TrustedRoot] {
        &self.roots
    }

    /// Begin a new session generation, resetting per-session state while the
    /// authority identity and mode stay immutable.
    pub fn begin_session(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.generation
    }

    /// Current session generation.
    pub fn session_generation(&self) -> u64 {
        self.generation
    }

    /// Admit a launch program path.
    ///
    /// Workspace-bound authority requires the program to validate inside one
    /// trusted root. Explicitly unbounded authority admits any path.
    pub fn admits_launch_path(&self, program: &Path) -> Result<(), String> {
        match self.mode {
            LaunchAuthorityMode::ExplicitUnbounded => Ok(()),
            LaunchAuthorityMode::WorkspaceBound => {
                for root in &self.roots {
                    if !trusted_root_is_current(root) {
                        continue;
                    }
                    if crate::security::validate_path(program, root.canonical()).is_ok() {
                        return Ok(());
                    }
                }
                Err("the launch 'program' is outside every trusted root configured at startup; \
                     only scripts inside a startup trusted root can be debugged"
                    .to_string())
            }
        }
    }

    /// Narrow a launch-args `workspaceRoot` against the authority.
    ///
    /// Returns the validated narrowing root for workspace-bound authority.
    /// Unbounded authority never creates a boundary from launch arguments and
    /// returns `None`.
    pub fn narrow_launch_root(&self, requested: &Path) -> Result<Option<PathBuf>, String> {
        match self.mode {
            LaunchAuthorityMode::ExplicitUnbounded => Ok(None),
            LaunchAuthorityMode::WorkspaceBound => {
                for root in &self.roots {
                    if !trusted_root_is_current(root) {
                        continue;
                    }
                    if let Ok(narrowed) =
                        crate::security::validate_path(requested, root.canonical())
                    {
                        return Ok(Some(narrowed));
                    }
                }
                Err("the launch 'workspaceRoot' is outside every trusted root configured at \
                     startup and cannot create or widen authority"
                    .to_string())
            }
        }
    }

    /// Bounded receipt for the current session (never private absolute paths).
    pub fn receipt(&self) -> LaunchAuthorityReceipt {
        LaunchAuthorityReceipt {
            mode: self.mode.label(),
            trusted_root_count: self.roots.len(),
            trusted_root_identities: self.roots.iter().map(|root| root.identity.clone()).collect(),
            acknowledgement_identity: self.acknowledgement.as_ref().map(|acknowledgement| {
                short_identity(&format!(
                    "{}\x1f{}",
                    acknowledgement.source.label(),
                    acknowledgement.note.trim()
                ))
            }),
            authority_identity: self.identity.clone(),
            session_generation: self.generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LaunchAuthority, LaunchAuthorityError, LaunchAuthorityMode, LaunchAuthoritySource,
        LaunchAuthorityStartup, UnboundedAcknowledgement,
    };
    use std::path::{Path, PathBuf};

    fn tempfile_name(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pldap-authority-{name}-{}", std::process::id()))
    }

    fn make_root(name: &str) -> PathBuf {
        let path = tempfile_name(name);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("test root creation");
        path
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn unbounded(note: &str) -> UnboundedAcknowledgement {
        UnboundedAcknowledgement::new(LaunchAuthoritySource::CommandLine, note)
    }

    #[test]
    fn no_authority_configured_fails_closed() {
        let error = LaunchAuthority::resolve(&LaunchAuthorityStartup::default()).unwrap_err();
        assert_eq!(error, LaunchAuthorityError::NoAuthorityConfigured);
    }

    #[test]
    fn ambiguous_authority_rejects_roots_with_acknowledgement() {
        let root = make_root("ambiguous");
        let startup = LaunchAuthorityStartup {
            trusted_roots: vec![root.clone()],
            allow_unbounded: Some(unbounded("operator")),
        };
        let error = LaunchAuthority::resolve(&startup).unwrap_err();
        assert_eq!(error, LaunchAuthorityError::AmbiguousAuthorityMode);
        cleanup(&root);
    }

    #[test]
    fn one_trusted_root_resolves_deterministically() {
        let root = make_root("single");
        let startup =
            LaunchAuthorityStartup { trusted_roots: vec![root.clone()], allow_unbounded: None };
        let authority = LaunchAuthority::resolve(&startup).expect("resolution");
        assert_eq!(authority.mode(), LaunchAuthorityMode::WorkspaceBound);
        assert_eq!(authority.trusted_roots().len(), 1);
        let again = LaunchAuthority::resolve(&startup).expect("resolution");
        assert_eq!(authority.identity(), again.identity());
        cleanup(&root);
    }

    #[test]
    fn multiple_roots_validate_and_sort_by_identity() {
        let first = make_root("multi-a");
        let second = make_root("multi-b");
        let startup = LaunchAuthorityStartup {
            trusted_roots: vec![first.clone(), second.clone()],
            allow_unbounded: None,
        };
        let authority = LaunchAuthority::resolve(&startup).expect("resolution");
        let identities: Vec<&str> =
            authority.trusted_roots().iter().map(|root| root.identity()).collect();
        let mut sorted = identities.clone();
        sorted.sort_unstable();
        assert_eq!(identities, sorted);
        cleanup(&first);
        cleanup(&second);
    }

    #[test]
    fn missing_and_non_directory_roots_reject() {
        let missing = tempfile_name("does-not-exist");
        let _ = std::fs::remove_dir_all(&missing);
        let startup =
            LaunchAuthorityStartup { trusted_roots: vec![missing.clone()], allow_unbounded: None };
        assert_eq!(
            LaunchAuthority::resolve(&startup).unwrap_err(),
            LaunchAuthorityError::TrustedRootNotFound { path: missing }
        );

        let file = tempfile_name("a-file");
        let _ = std::fs::remove_file(&file);
        std::fs::write(&file, b"not a directory").expect("test file");
        let startup =
            LaunchAuthorityStartup { trusted_roots: vec![file.clone()], allow_unbounded: None };
        assert_eq!(
            LaunchAuthority::resolve(&startup).unwrap_err(),
            LaunchAuthorityError::TrustedRootNotADirectory { path: file.clone() }
        );
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn duplicate_input_rejects() {
        let root = make_root("duplicate");
        let startup = LaunchAuthorityStartup {
            trusted_roots: vec![root.clone(), root.clone()],
            allow_unbounded: None,
        };
        assert_eq!(
            LaunchAuthority::resolve(&startup).unwrap_err(),
            LaunchAuthorityError::DuplicateTrustedRoot { path: root.clone() }
        );
        cleanup(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_extended_path_alias_conflicts_end_to_end() {
        // The Windows extended-length prefix is a distinct raw spelling of the
        // same directory: raw inputs differ, canonical directories match, so
        // the alias rule (not the duplicate rule) must fire.
        let root = make_root("alias-extended");
        let extended = PathBuf::from(format!(r"\\?\{}", root.display()));
        assert_ne!(root, extended);
        let startup = LaunchAuthorityStartup {
            trusted_roots: vec![root.clone(), extended],
            allow_unbounded: None,
        };
        assert!(matches!(
            LaunchAuthority::resolve(&startup).unwrap_err(),
            LaunchAuthorityError::TrustedRootAliasConflict { .. }
        ));
        cleanup(&root);
    }

    #[test]
    fn alias_helper_detects_second_spelling_of_one_directory() {
        use super::detect_trusted_root_alias;
        let seen = vec![
            (PathBuf::from("/first-spelling"), PathBuf::from("/canonical/dir")),
            (PathBuf::from("/other"), PathBuf::from("/canonical/other")),
        ];
        assert_eq!(
            detect_trusted_root_alias(&seen, Path::new("/canonical/dir")),
            Some(PathBuf::from("/first-spelling"))
        );
        assert_eq!(detect_trusted_root_alias(&seen, Path::new("/canonical/unique")), None);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_root_rejects() {
        let target = make_root("symlink-target");
        let link = tempfile_name("symlink-link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&target, &link).expect("test symlink");
        let startup =
            LaunchAuthorityStartup { trusted_roots: vec![link.clone()], allow_unbounded: None };
        assert_eq!(
            LaunchAuthority::resolve(&startup).unwrap_err(),
            LaunchAuthorityError::TrustedRootSymlink { path: link.clone() }
        );
        let _ = std::fs::remove_file(&link);
        cleanup(&target);
    }

    #[test]
    fn empty_acknowledgement_note_rejects() {
        let startup = LaunchAuthorityStartup {
            trusted_roots: Vec::new(),
            allow_unbounded: Some(unbounded("   ")),
        };
        assert!(matches!(
            LaunchAuthority::resolve(&startup).unwrap_err(),
            LaunchAuthorityError::InvalidAcknowledgement(_)
        ));
    }

    #[test]
    fn unbounded_mode_is_accepted_and_recorded() {
        let startup = LaunchAuthorityStartup {
            trusted_roots: Vec::new(),
            allow_unbounded: Some(unbounded("operator session")),
        };
        let authority = LaunchAuthority::resolve(&startup).expect("resolution");
        assert_eq!(authority.mode(), LaunchAuthorityMode::ExplicitUnbounded);
        let receipt = authority.receipt();
        assert_eq!(receipt.mode, "explicit_unbounded");
        assert!(receipt.acknowledgement_identity.is_some());
        assert_eq!(receipt.trusted_root_count, 0);
    }

    #[test]
    fn bounded_session_does_not_inherit_unbounded_state() {
        let unbounded_startup = LaunchAuthorityStartup {
            trusted_roots: Vec::new(),
            allow_unbounded: Some(unbounded("earlier session")),
        };
        let _ = LaunchAuthority::resolve(&unbounded_startup).expect("resolution");

        let root = make_root("later-bounded");
        let bounded_startup =
            LaunchAuthorityStartup { trusted_roots: vec![root.clone()], allow_unbounded: None };
        let bounded = LaunchAuthority::resolve(&bounded_startup).expect("resolution");
        assert_eq!(bounded.mode(), LaunchAuthorityMode::WorkspaceBound);
        cleanup(&root);
    }

    #[test]
    fn identity_is_stable_across_sessions_but_generation_resets() {
        let startup = LaunchAuthorityStartup {
            trusted_roots: Vec::new(),
            allow_unbounded: Some(unbounded("operator session")),
        };
        let mut authority = LaunchAuthority::resolve(&startup).expect("resolution");
        let identity = authority.identity().to_string();
        let first = authority.begin_session();
        let second = authority.begin_session();
        assert_eq!(authority.identity(), identity);
        assert_eq!(second, first + 1);
    }

    #[test]
    fn workspace_bound_rejects_paths_outside_roots() {
        let root = make_root("admit");
        let startup =
            LaunchAuthorityStartup { trusted_roots: vec![root.clone()], allow_unbounded: None };
        let authority = LaunchAuthority::resolve(&startup).expect("resolution");

        let inside = root.join("script.pl");
        std::fs::write(&inside, b"print 1;").expect("test script");
        assert!(authority.admits_launch_path(&inside).is_ok());

        let outside = tempfile_name("outside-script");
        std::fs::write(&outside, b"print 1;").expect("test script");
        assert!(authority.admits_launch_path(&outside).is_err());
        let _ = std::fs::remove_file(&outside);
        cleanup(&root);
    }

    #[test]
    fn workspace_bound_rejects_root_replacement_after_startup() {
        let root = make_root("retarget");
        let startup =
            LaunchAuthorityStartup { trusted_roots: vec![root.clone()], allow_unbounded: None };
        let authority = LaunchAuthority::resolve(&startup).expect("resolution");
        let displaced = tempfile_name("retarget-displaced");
        let _ = std::fs::remove_dir_all(&displaced);
        std::fs::rename(&root, &displaced).expect("displace startup root");
        std::fs::create_dir_all(&root).expect("replacement root");
        let replacement_program = root.join("replacement.pl");
        std::fs::write(&replacement_program, b"print 1;").expect("replacement script");

        assert!(authority.admits_launch_path(&replacement_program).is_err());
        assert!(authority.narrow_launch_root(&root).is_err());

        cleanup(&root);
        cleanup(&displaced);
    }

    #[test]
    fn unbounded_admits_paths_but_never_narrows_from_launch_args() {
        let startup = LaunchAuthorityStartup {
            trusted_roots: Vec::new(),
            allow_unbounded: Some(unbounded("operator session")),
        };
        let authority = LaunchAuthority::resolve(&startup).expect("resolution");
        let anywhere = std::env::temp_dir().join("anywhere.pl");
        assert!(authority.admits_launch_path(&anywhere).is_ok());
        assert_eq!(authority.narrow_launch_root(&anywhere).expect("narrow"), None);
    }

    #[test]
    fn receipts_never_contain_absolute_paths() {
        let root = make_root("receipt");
        let startup =
            LaunchAuthorityStartup { trusted_roots: vec![root.clone()], allow_unbounded: None };
        let authority = LaunchAuthority::resolve(&startup).expect("resolution");
        let rendered = format!("{:?}", authority.receipt());
        // The canonical directory name is unique per test run; a receipt that
        // leaked paths would contain it.
        assert!(!rendered.contains("pldap-authority-receipt"));
        cleanup(&root);
    }

    #[test]
    fn acknowledgement_source_label_is_user_owned() {
        assert_eq!(LaunchAuthoritySource::CommandLine.label(), "command_line");
        assert_eq!(LaunchAuthoritySource::HostSetting.label(), "host_setting");
    }
}
