//! Falsifiers for the #13997 cancellation-error neutrality claim.
//!
//! The claim under test has three parts, and each has its own falsifier here:
//!
//! 1. `CancellationError` does not implement `perl_parser_core::ErrorClass` —
//!    proven at compile time, with an in-crate positive control so the probe
//!    cannot pass by always answering "no".
//! 2. The historical Bug/Protocol/Transient mapping survives unchanged behind
//!    one application-owned adapter.
//! 3. The taxonomy cannot creep back into the cancellation module, directly or
//!    through a re-export facade — proven by a source probe that is itself
//!    exercised against simulated reintroductions.
//!
//! Fixture reads propagate errors with `?`: the workspace denies
//! `clippy::unwrap_used` and `clippy::expect_used` for every target, including
//! test targets.

use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

use perl_lsp_rs_core::protocol::{
    Disposition, JsonRpcId, cancellation_error_category, cancellation_error_disposition,
};
use perl_lsp_rs_core::runtime::cancellation::{
    CancellationError, CancellationRegistry, PerlLspCancellationToken,
};
use perl_lsp_rs_core::transport::FramingError;
use perl_parser_core::ErrorCategory;

// ---------------------------------------------------------------------------
// 1. Compile-time neutrality probe
// ---------------------------------------------------------------------------

/// Compile-time answer to "does `T` implement `ErrorClass`?".
///
/// Inherent associated items take precedence over trait ones, but only when
/// the inherent impl actually applies. The inherent impl below is bounded on
/// `T: ErrorClass`, so it is selected exactly when `T` implements the trait and
/// the blanket trait impl answers otherwise.
struct ErrorClassProbe<T>(PhantomData<T>);

trait ErrorClassProbeFallback {
    const IMPLEMENTS_ERROR_CLASS: bool = false;
}

impl<T> ErrorClassProbeFallback for ErrorClassProbe<T> {}

impl<T: perl_parser_core::ErrorClass> ErrorClassProbe<T> {
    const IMPLEMENTS_ERROR_CLASS: bool = true;
}

// These are `const` items on purpose: reintroducing the trait impl must fail
// the build of this target, not merely fail an assertion at run time.

/// Positive control, same crate and same trait as the subject. Without it the
/// neutrality assertion below would also hold if the probe were simply broken
/// and always answered `false`.
const _: () = assert!(
    ErrorClassProbe::<FramingError>::IMPLEMENTS_ERROR_CLASS,
    "probe is broken: FramingError does implement ErrorClass and must be reported"
);

/// The subject: `CancellationError` must stay a language-neutral runtime fact
/// (#13997); its Perl category belongs to the app-owned adapter.
const _: () = assert!(
    !ErrorClassProbe::<CancellationError>::IMPLEMENTS_ERROR_CLASS,
    "CancellationError must not implement perl_parser_core::ErrorClass (#13997)"
);

// ---------------------------------------------------------------------------
// 2. Mapping parity — the adapter preserves the removed impl exactly
// ---------------------------------------------------------------------------

#[test]
fn adapter_preserves_the_historical_category_mapping() {
    // These four rows are the mapping the removed
    // `impl ErrorClass for CancellationError` produced. Moving ownership must
    // not move behavior.
    assert_eq!(
        cancellation_error_category(&CancellationError::LockError("poisoned".into())),
        ErrorCategory::Bug,
        "an internal lock failure is our bug"
    );
    assert_eq!(
        cancellation_error_category(&CancellationError::ProviderNotFound("hover".into())),
        ErrorCategory::Bug,
        "a routing failure is our bug"
    );
    assert_eq!(
        cancellation_error_category(&CancellationError::InvalidRequest("bad id".into())),
        ErrorCategory::Protocol,
        "a malformed request is a protocol violation"
    );
    assert_eq!(
        cancellation_error_category(&CancellationError::Timeout(Duration::from_millis(500))),
        ErrorCategory::Transient,
        "a timeout may succeed on retry"
    );
}

#[test]
fn adapter_categories_are_not_collapsed_onto_one_value() {
    // Negative control for the parity test above: a mapping that returned one
    // constant would satisfy any single row, so require the three distinct
    // categories to stay distinct.
    let bug = cancellation_error_category(&CancellationError::LockError("x".into()));
    let protocol = cancellation_error_category(&CancellationError::InvalidRequest("x".into()));
    let transient = cancellation_error_category(&CancellationError::Timeout(Duration::ZERO));

    assert_ne!(bug, protocol, "Bug and Protocol must not collapse");
    assert_ne!(protocol, transient, "Protocol and Transient must not collapse");
    assert_ne!(bug, transient, "Bug and Transient must not collapse");
}

#[test]
fn adapter_disposition_composes_the_canonical_mapping() {
    assert_eq!(
        cancellation_error_disposition(&CancellationError::LockError("poisoned".into())),
        Disposition::Repair,
    );
    assert_eq!(
        cancellation_error_disposition(&CancellationError::Timeout(Duration::from_secs(1))),
        Disposition::Retry,
    );
    assert_eq!(
        cancellation_error_disposition(&CancellationError::InvalidRequest("bad".into())),
        Disposition::Reject,
    );
}

#[test]
fn adapter_does_not_classify_from_display_text() {
    // Falsifier 4 in the issue: classification must come from the variant, not
    // from prose. Two variants carrying identical payload text must still
    // receive different categories.
    let same_text = "identical payload";
    assert_eq!(
        cancellation_error_category(&CancellationError::LockError(same_text.into())),
        ErrorCategory::Bug,
    );
    assert_eq!(
        cancellation_error_category(&CancellationError::InvalidRequest(same_text.into())),
        ErrorCategory::Protocol,
    );
}

// ---------------------------------------------------------------------------
// 3. Source probe — the taxonomy cannot creep back in
// ---------------------------------------------------------------------------

/// Tokens that mean "this source has taken a dependency on the Perl error
/// taxonomy", including the re-export spellings a compatibility facade would
/// use.
const TAXONOMY_TOKENS: &[&str] =
    &["perl_parser_core", "ErrorClass", "ErrorCategory", "error_class"];

/// Returns every taxonomy token that appears in `source`, ignoring comment and
/// doc-comment lines so that the prohibition can still be *documented* in the
/// module it governs.
fn taxonomy_tokens_in(source: &str) -> Vec<&'static str> {
    let code_only: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    TAXONOMY_TOKENS.iter().copied().filter(|token| code_only.contains(token)).collect()
}

fn cancellation_module_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime/cancellation/mod.rs")
}

#[test]
fn cancellation_module_source_is_free_of_the_perl_taxonomy() -> std::io::Result<()> {
    let source = std::fs::read_to_string(cancellation_module_path())?;
    let found = taxonomy_tokens_in(&source);

    assert!(
        found.is_empty(),
        "the cancellation module must not reference the Perl error taxonomy (#13997), found: {found:?}"
    );
    Ok(())
}

#[test]
fn source_probe_detects_a_direct_reintroduction() {
    // Negative control: the probe must fail on the exact impl this PR removed.
    let reintroduced = r"
impl perl_parser_core::ErrorClass for CancellationError {
    fn error_class(&self) -> perl_parser_core::ErrorCategory {
        perl_parser_core::ErrorCategory::Bug
    }
}
";
    assert!(
        !taxonomy_tokens_in(reintroduced).is_empty(),
        "probe must catch a direct ErrorClass impl"
    );
}

#[test]
fn source_probe_detects_a_re_export_facade() {
    // Falsifier 2 in the issue: a facade that pulls the trait back into the
    // neutral graph under a local alias must also fail.
    let facade = r"
use perl_parser_core::ErrorClass as Classify;
pub use perl_parser_core::ErrorCategory;
";
    assert!(
        !taxonomy_tokens_in(facade).is_empty(),
        "probe must catch a compatibility re-export facade"
    );
}

#[test]
fn source_probe_does_not_fire_on_the_documented_prohibition() {
    // Guard the guard: the module documents that it must not implement
    // ErrorClass. A probe that flagged its own prohibition would push authors
    // to delete the explanation.
    let documented = r"
/// This type deliberately does not implement perl_parser_core::ErrorClass.
// See ErrorCategory ownership in #13997.
pub enum CancellationError {}
";
    assert!(
        taxonomy_tokens_in(documented).is_empty(),
        "probe must ignore comments so the prohibition can stay documented"
    );
}

// ---------------------------------------------------------------------------
// 4. Mechanics parity — moving the category changed no cancellation behavior
// ---------------------------------------------------------------------------

#[test]
fn cancellation_mechanics_are_unchanged() -> Result<(), CancellationError> {
    let registry = CancellationRegistry::new();
    let id = JsonRpcId::Integer(1);
    let token = PerlLspCancellationToken::new(id.clone(), "textDocument/hover".into());

    registry.register_token(token.clone())?;
    assert_eq!(registry.active_count(), 1, "registration still tracks the request");
    assert!(!token.is_cancelled(), "a fresh token is live");

    registry.cancel_request(&id)?;
    assert!(token.is_cancelled(), "cancelling through the registry still cancels the token");
    Ok(())
}

#[test]
fn cancellation_error_still_displays_and_is_a_std_error() {
    // The removed impl sat beside Display/Error. Those must survive.
    let err = CancellationError::Timeout(Duration::from_millis(500));
    assert!(err.to_string().contains("Operation timeout"));

    let boxed: Box<dyn std::error::Error> = Box::new(CancellationError::LockError("x".into()));
    assert!(boxed.to_string().contains("Lock error"));
}
