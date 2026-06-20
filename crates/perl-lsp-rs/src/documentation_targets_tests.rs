use super::*;
use perl_tdd_support::{must, must_some};

#[test]
fn from_perldoc_uri_parses_valid_perldoc_uri() {
    let target = must_some(PerlDocumentationTarget::from_perldoc_uri("perldoc://Local::Doc"));

    require_eq(target.name(), "Local::Doc");
    require_eq(&target.perldoc_uri(), "perldoc://Local::Doc");
}

#[test]
fn from_perldoc_uri_rejects_non_perldoc_scheme() {
    require(
        PerlDocumentationTarget::from_perldoc_uri("https://metacpan.org/pod/Local::Doc").is_none(),
        "expected non-perldoc scheme to be rejected",
    );
}

#[test]
fn from_perldoc_uri_rejects_whitespace_instead_of_normalizing() {
    for uri in ["perldoc:// Local::Doc", "perldoc://Local::Doc "] {
        require(
            PerlDocumentationTarget::from_perldoc_uri(uri).is_none(),
            format!("expected whitespace-bearing URI {uri} to be rejected"),
        );
    }
}

#[test]
fn from_perldoc_uri_rejects_malformed_target_names() {
    for uri in ["perldoc://", "perldoc://Local/Doc", "perldoc://Local::>"] {
        require(
            PerlDocumentationTarget::from_perldoc_uri(uri).is_none(),
            format!("expected malformed URI {uri} to be rejected"),
        );
    }
}

#[test]
fn from_simple_pod_link_target_extracts_bare_module_target() {
    let bare = must_some(PerlDocumentationTarget::from_simple_pod_link_target("  Local::Doc  "));

    require_eq(&bare.perldoc_uri(), "perldoc://Local::Doc");
}

#[test]
fn from_simple_pod_link_target_extracts_labeled_module_target() {
    let labeled =
        must_some(PerlDocumentationTarget::from_simple_pod_link_target("docs|  Local::Labeled  "));

    require_eq(&labeled.perldoc_uri(), "perldoc://Local::Labeled");
}

#[test]
fn from_simple_pod_link_target_accepts_core_pragma_targets() {
    let bare = must_some(PerlDocumentationTarget::from_simple_pod_link_target("strict"));
    let labeled =
        must_some(PerlDocumentationTarget::from_simple_pod_link_target("warnings docs|warnings"));

    require_eq(&bare.perldoc_uri(), "perldoc://strict");
    require_eq(&labeled.perldoc_uri(), "perldoc://warnings");
}

#[test]
fn from_simple_pod_link_target_rejects_empty_labels() {
    for target in ["|Local::Doc", "   |Local::Doc"] {
        require(
            PerlDocumentationTarget::from_simple_pod_link_target(target).is_none(),
            format!("expected empty-label target {target} to be rejected"),
        );
    }
}

#[test]
fn from_simple_pod_link_target_rejects_non_module_targets() {
    for target in
        ["/section", "docs|/section", "NotAModule", "docs|Broken::", "docs|https://example.invalid"]
    {
        require(
            PerlDocumentationTarget::from_simple_pod_link_target(target).is_none(),
            format!("expected non-module target {target} to be rejected"),
        );
    }
}

fn require(condition: bool, message: impl Into<String>) {
    if !condition {
        must(Err::<(), _>(message.into()));
    }
}

fn require_eq(actual: &str, expected: &str) {
    if actual != expected {
        must(Err::<(), _>(format!("expected {expected}, got {actual}")));
    }
}
