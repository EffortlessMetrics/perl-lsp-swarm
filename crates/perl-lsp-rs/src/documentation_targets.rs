//! Canonical documentation targets for Perl editor surfaces.
//!
//! Keep construction of editor-facing documentation URIs in one place so hover,
//! document links, and virtual documents do not drift as perldoc graph support
//! grows.

/// A validated Perl documentation name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PerlDocumentationTarget {
    name: String,
}

impl PerlDocumentationTarget {
    /// Build a documentation target for a Perl module, pragma, or perldoc topic.
    ///
    /// The current editor contract uses raw `perldoc://Name::Space` and
    /// `https://metacpan.org/pod/Name::Space` strings. This validator keeps that
    /// behavior for simple Perl names while rejecting empty or path-like input.
    pub(crate) fn new(name: &str) -> Option<Self> {
        let trimmed = name.trim();
        if !is_supported_perl_doc_name(trimmed) {
            return None;
        }

        Some(Self { name: trimmed.to_string() })
    }

    /// Build a documentation target from a virtual perldoc URI.
    pub(crate) fn from_perldoc_uri(uri: &str) -> Option<Self> {
        let name = uri.strip_prefix("perldoc://")?;
        if name != name.trim() {
            return None;
        }

        Self::new(name)
    }

    /// Build a documentation target from a simple POD `L<>` module target.
    ///
    /// This intentionally accepts only module-like names and the core pragma
    /// targets that virtual perldoc already enriches. Section-only links,
    /// URLs, and empty labels are left to the client as plain POD text.
    pub(crate) fn from_simple_pod_link_target(target: &str) -> Option<Self> {
        let candidate = if let Some((label, link_target)) = target.split_once('|') {
            if label.trim().is_empty() {
                return None;
            }
            link_target.trim()
        } else {
            target.trim()
        };

        if is_supported_core_pragma_pod_target(candidate) || candidate.contains("::") {
            Self::new(candidate)
        } else {
            None
        }
    }

    /// Return the validated perldoc target name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Return the virtual perldoc document URI.
    pub(crate) fn perldoc_uri(&self) -> String {
        format!("perldoc://{}", self.name)
    }

    /// Return the MetaCPAN POD URI for this target.
    pub(crate) fn metacpan_pod_uri(&self) -> String {
        format!("https://metacpan.org/pod/{}", self.name)
    }

    /// Return the perl.org perldoc URI for core pragmas/topics.
    pub(crate) fn perl_org_perldoc_uri(&self) -> String {
        format!("https://perldoc.perl.org/{}", self.name)
    }

    /// Return a markdown link to the virtual perldoc document.
    pub(crate) fn virtual_perldoc_markdown_link(&self) -> String {
        format!("[Open virtual perldoc]({})", self.perldoc_uri())
    }

    /// Return a markdown link to the MetaCPAN POD page.
    pub(crate) fn metacpan_markdown_link(&self, label: &str) -> String {
        format!("[{label}]({})", self.metacpan_pod_uri())
    }

    /// Return a markdown link to the perl.org perldoc page.
    pub(crate) fn perl_org_perldoc_markdown_link(&self) -> String {
        format!("[perldoc {}]({})", self.name, self.perl_org_perldoc_uri())
    }
}

/// Construct a MetaCPAN POD URI for a validated Perl documentation name.
pub(crate) fn metacpan_pod_uri(name: &str) -> Option<String> {
    PerlDocumentationTarget::new(name).map(|target| target.metacpan_pod_uri())
}

fn is_supported_perl_doc_name(name: &str) -> bool {
    if name.is_empty()
        || name.contains(char::is_whitespace)
        || name.contains(['/', '\\', '<', '>', '|', '#', '?'])
    {
        return false;
    }

    name.split("::").all(is_perl_doc_name_segment)
}

fn is_perl_doc_name_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_supported_core_pragma_pod_target(target: &str) -> bool {
    matches!(target, "strict" | "warnings")
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must, must_some};

    #[test]
    fn documentation_target_parses_valid_perldoc_uri() {
        let target = must_some(PerlDocumentationTarget::from_perldoc_uri("perldoc://Local::Doc"));

        require_eq(target.name(), "Local::Doc");
        require_eq(&target.perldoc_uri(), "perldoc://Local::Doc");
    }

    #[test]
    fn documentation_target_rejects_malformed_perldoc_uri() {
        for uri in [
            "perldoc://",
            "perldoc://Local/Doc",
            "perldoc://Local::>",
            "perldoc:// Local::Doc",
            "https://metacpan.org/pod/Local::Doc",
        ] {
            require(
                PerlDocumentationTarget::from_perldoc_uri(uri).is_none(),
                format!("expected {uri} to be rejected"),
            );
        }
    }

    #[test]
    fn documentation_target_extracts_simple_pod_module_targets() {
        let bare = must_some(PerlDocumentationTarget::from_simple_pod_link_target("Local::Doc"));
        let labeled =
            must_some(PerlDocumentationTarget::from_simple_pod_link_target("docs|Local::Labeled"));
        let pragma =
            must_some(PerlDocumentationTarget::from_simple_pod_link_target("strict docs|strict"));

        require_eq(&bare.perldoc_uri(), "perldoc://Local::Doc");
        require_eq(&labeled.perldoc_uri(), "perldoc://Local::Labeled");
        require_eq(&pragma.perldoc_uri(), "perldoc://strict");
    }

    #[test]
    fn documentation_target_rejects_non_module_pod_targets() {
        for target in [
            "/section",
            "docs|/section",
            "NotAModule",
            "|Local::Doc",
            "docs|Broken::",
            "docs|https://example.invalid",
        ] {
            require(
                PerlDocumentationTarget::from_simple_pod_link_target(target).is_none(),
                format!("expected {target} to be rejected"),
            );
        }
    }

    fn require(condition: bool, message: String) {
        if !condition {
            must(Err::<(), _>(message));
        }
    }

    fn require_eq(actual: &str, expected: &str) {
        if actual != expected {
            must(Err::<(), _>(format!("expected {expected}, got {actual}")));
        }
    }
}
