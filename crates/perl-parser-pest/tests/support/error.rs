use std::fmt;

/// Fail-closed fixture-load and instrument errors.
///
/// These are distinct from a parser `Err` observation. A missing fixture,
/// empty selection, or parser panic is never reported as a parse rejection.
#[derive(Debug)]
pub enum FixtureError {
    /// Manifest `schema` is missing or not the supported identifier.
    InvalidSchema(String),
    /// Two rows share the same stable fixture id.
    DuplicateId(String),
    /// The catalog contains no rows.
    EmptyManifest,
    /// A required selection matched zero rows.
    EmptySelection {
        /// Requested fixture id, if any.
        id: Option<String>,
        /// Requested family, if any.
        family: Option<String>,
    },
    /// A relative path contained `..` or another non-normal component.
    PathEscape(String),
    /// A source or manifest path was absolute.
    AbsolutePath(String),
    /// A file source was not under `tests/fixtures/`.
    SourceNotUnderFixtures(String),
    /// The referenced source file does not exist.
    MissingSource {
        /// Fixture id that named the path.
        id: String,
        /// Package-relative path.
        path: String,
    },
    /// The source exists but could not be read as a regular file.
    Unreadable {
        /// Fixture id that named the path.
        id: String,
        /// Package-relative path.
        path: String,
        /// OS or type detail.
        detail: String,
    },
    /// Declared digest does not match the exact loaded bytes.
    DigestMismatch {
        /// Fixture id.
        id: String,
        /// Digest written in the manifest.
        declared: String,
        /// Digest of the loaded bytes.
        actual: String,
    },
    /// Two files declared the same identity but do not share bytes.
    IdentityByteMismatch {
        /// Shared declared digest or identity.
        identity: String,
        /// First source path or inline identity.
        left: String,
        /// Second source path or inline identity.
        right: String,
    },
    /// `disposition = "final-acceptance"` without `expected_outcome_owner`.
    FinalAcceptanceWithoutOwner {
        /// Fixture id.
        id: String,
    },
    /// Row set both `source` and `inline_source`, or neither.
    AmbiguousSource {
        /// Fixture id.
        id: String,
    },
    /// Row omitted `id`.
    MissingId,
    /// Row omitted `family`.
    MissingFamily {
        /// Fixture id.
        id: String,
    },
    /// Row omitted `execution_modes`.
    MissingExecutionModes {
        /// Fixture id.
        id: String,
    },
    /// The embedded parser panicked while observing a fixture.
    ParserPanic {
        /// Fixture id being observed.
        id: String,
        /// Panic payload rendered as text.
        message: String,
    },
    /// Source path is a symbolic link.
    SymlinkSource {
        /// Fixture id.
        id: String,
        /// Package-relative path.
        path: String,
    },
    /// Manifest TOML could not be parsed.
    InvalidToml {
        /// Package-relative or absolute path of the manifest.
        path: String,
        /// Parser detail.
        detail: String,
    },
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchema(schema) => {
                write!(f, "unsupported fixture manifest schema '{schema}'")
            }
            Self::DuplicateId(id) => write!(f, "duplicate fixture id '{id}'"),
            Self::EmptyManifest => write!(f, "fixture manifest contains no rows"),
            Self::EmptySelection { id, family } => {
                write!(f, "fixture selection matched zero rows (id={id:?}, family={family:?})")
            }
            Self::PathEscape(path) => {
                write!(f, "fixture path escapes the package root: {path}")
            }
            Self::AbsolutePath(path) => write!(f, "fixture path is absolute: {path}"),
            Self::SourceNotUnderFixtures(path) => {
                write!(f, "fixture source is not under tests/fixtures: {path}")
            }
            Self::MissingSource { id, path } => {
                write!(f, "fixture '{id}' source is missing: {path}")
            }
            Self::Unreadable { id, path, detail } => {
                write!(f, "fixture '{id}' source is unreadable ({path}): {detail}")
            }
            Self::DigestMismatch { id, declared, actual } => {
                write!(f, "fixture '{id}' declared digest {declared} but bytes digest {actual}")
            }
            Self::IdentityByteMismatch { identity, left, right } => {
                write!(
                    f,
                    "two source files share identity '{identity}' but bytes differ ({left} vs {right})"
                )
            }
            Self::FinalAcceptanceWithoutOwner { id } => {
                write!(f, "fixture '{id}' uses final-acceptance without expected_outcome_owner")
            }
            Self::AmbiguousSource { id } => {
                write!(f, "fixture '{id}' must set exactly one of source or inline_source")
            }
            Self::MissingId => write!(f, "fixture row is missing id"),
            Self::MissingFamily { id } => write!(f, "fixture '{id}' is missing family"),
            Self::MissingExecutionModes { id } => {
                write!(f, "fixture '{id}' has no execution_modes")
            }
            Self::ParserPanic { id, message } => {
                write!(f, "parser panicked while observing fixture '{id}': {message}")
            }
            Self::SymlinkSource { id, path } => {
                write!(f, "fixture '{id}' source is a symbolic link: {path}")
            }
            Self::InvalidToml { path, detail } => {
                write!(f, "failed to parse fixture manifest {path}: {detail}")
            }
        }
    }
}

impl std::error::Error for FixtureError {}
