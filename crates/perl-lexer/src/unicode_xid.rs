//! Unicode 17 XID data/query authority for Perl name classification (#13326).
//!
//! This module is the U01 lower authority: inspectable Unicode-data identity
//! and deterministic `XID_Start` / `XID_Continue` queries. Lexer identifier
//! scanning stays in the lexer identifier helpers until #13327. Regex group
//! names are #13328. Identifier and regex consumers should call this module
//! rather than copying tables or substituting `char::is_alphanumeric`.
//!
//! Profile selection is explicit. This authority binds Perl 5.44 to the
//! reviewed Unicode 17.0 table and does not apply that table retroactively
//! to older or unreviewed profiles.

use core::fmt;
use unicode_ident::{is_xid_continue as table_is_xid_continue, is_xid_start as table_is_xid_start};

/// Machine-readable Unicode version of the executable XID tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnicodeVersion {
    /// Unicode major version (for example `17` in 17.0.0).
    pub major: u8,
    /// Unicode minor version.
    pub minor: u8,
    /// Unicode update number.
    pub update: u8,
}

impl UnicodeVersion {
    /// Unicode 17.0.0, the reviewed Perl 5.44 name-classification table.
    pub const UNICODE_17_0: Self = Self { major: 17, minor: 0, update: 0 };

    /// Pack the version as `(major, minor, update)`.
    #[must_use]
    pub const fn as_tuple(self) -> (u8, u8, u8) {
        (self.major, self.minor, self.update)
    }
}

impl fmt::Display for UnicodeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.update)
    }
}

/// Perl language-profile identity used to select a Unicode/XID policy.
///
/// This is not the [`perl-pragma`](https://docs.rs/perl-pragma) version model
/// (#8237 / #8698). Consumers pass an explicit major/minor pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PerlXidProfile {
    /// Perl major version (`5` for every currently reviewed profile).
    pub major: u16,
    /// Perl minor version (`44` for Perl 5.44).
    pub minor: u16,
}

impl PerlXidProfile {
    /// Perl 5.44, bound to Unicode 17.0 by `perl5440delta`.
    pub const PERL_5_44: Self = Self { major: 5, minor: 44 };

    /// Construct an explicit profile selector.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

/// Whether a Perl profile may treat this authority as exact Unicode 17.0 XID data.
///
/// `Unsupported` means this lower authority has no reviewed binding for the
/// profile (older/unknown profiles must not silently inherit Unicode 17).
/// `DataMismatch` means the executable table is not the reviewed 17.0.0
/// identity, so it cannot masquerade as exact 17.0 support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnicodeXidApplicability {
    /// The executable table is Unicode 17.0.0 and the profile is Perl 5.44.
    Exact,
    /// No reviewed Unicode/XID binding exists for this profile.
    Unsupported,
    /// The executable table version is not the reviewed Unicode 17.0.0 identity.
    DataMismatch,
}

/// Oracle/data-source identity for the executable XID tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnicodeXidDataSource {
    /// Reviewed [`unicode_ident`] tables. The executable version is
    /// [`unicode_ident::UNICODE_VERSION`].
    UnicodeIdent,
}

/// Inspectable Unicode version of the executable XID tables.
#[must_use]
pub fn unicode_data_version() -> UnicodeVersion {
    let (major, minor, update) = unicode_ident::UNICODE_VERSION;
    UnicodeVersion { major, minor, update }
}

/// Oracle identity for the XID tables used by [`is_xid_start`] / [`is_xid_continue`].
#[must_use]
pub fn unicode_xid_data_source() -> UnicodeXidDataSource {
    UnicodeXidDataSource::UnicodeIdent
}

/// Whether `ch` has Unicode `XID_Start`.
#[must_use]
pub fn is_xid_start(ch: char) -> bool {
    table_is_xid_start(ch)
}

/// Whether `ch` has Unicode `XID_Continue`.
#[must_use]
pub fn is_xid_continue(ch: char) -> bool {
    table_is_xid_continue(ch)
}

/// Select the reviewed Unicode/XID policy for an explicit Perl profile.
///
/// Perl 5.44 is `Exact` only when [`unicode_data_version`] is Unicode 17.0.0.
/// Every other profile is `Unsupported`; this authority does not decide
/// historical identifier legality.
#[must_use]
pub fn xid_policy_for_perl(profile: PerlXidProfile) -> UnicodeXidApplicability {
    applicability(profile, unicode_data_version())
}

fn applicability(profile: PerlXidProfile, table: UnicodeVersion) -> UnicodeXidApplicability {
    if profile.major == 5 && profile.minor == 44 {
        if table == UnicodeVersion::UNICODE_17_0 {
            UnicodeXidApplicability::Exact
        } else {
            UnicodeXidApplicability::DataMismatch
        }
    } else {
        UnicodeXidApplicability::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::{PerlXidProfile, UnicodeVersion, UnicodeXidApplicability, applicability};

    #[test]
    fn stale_or_newer_table_cannot_be_exact_for_perl_5_44() {
        assert_eq!(
            applicability(
                PerlXidProfile::PERL_5_44,
                UnicodeVersion { major: 16, minor: 0, update: 0 }
            ),
            UnicodeXidApplicability::DataMismatch
        );
        assert_eq!(
            applicability(
                PerlXidProfile::PERL_5_44,
                UnicodeVersion { major: 18, minor: 0, update: 0 }
            ),
            UnicodeXidApplicability::DataMismatch
        );
        assert_eq!(
            applicability(PerlXidProfile::PERL_5_44, UnicodeVersion::UNICODE_17_0),
            UnicodeXidApplicability::Exact
        );
    }

    #[test]
    fn unreviewed_profile_stays_unsupported_even_with_unicode_17_tables() {
        assert_eq!(
            applicability(PerlXidProfile::new(5, 42), UnicodeVersion::UNICODE_17_0),
            UnicodeXidApplicability::Unsupported
        );
    }
}
