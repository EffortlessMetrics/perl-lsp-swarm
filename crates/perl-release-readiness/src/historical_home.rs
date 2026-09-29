//! Frozen code-home the mixed evaluator still inspects.
//!
//! After the #8421 namespace move, the live package lives at
//! `crates/perl-release-readiness`. These strings remain the historical
//! membership, publish-policy, and license subject so `perl_kwalitee.v1`
//! evaluation does not silently retarget the renamed crate.

/// Relative workspace path the frozen manifest indicators still read.
pub(crate) const HISTORICAL_CRATE_MEMBER_PATH: &str = "crates/perl-kwalitee";

/// Package name the frozen publish-policy allowlist still looks for.
pub(crate) const HISTORICAL_PACKAGE_NAME: &str = "perl-kwalitee";
