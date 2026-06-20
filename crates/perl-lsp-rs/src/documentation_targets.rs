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

/// Construct a virtual perldoc URI for a validated Perl documentation name.
pub(crate) fn perldoc_uri(name: &str) -> Option<String> {
    PerlDocumentationTarget::new(name).map(|target| target.perldoc_uri())
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

#[cfg(test)]
mod tests {
    use super::{PerlDocumentationTarget, metacpan_pod_uri, perldoc_uri};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn documentation_target_builds_stable_perldoc_and_metacpan_uris() -> TestResult {
        let target = PerlDocumentationTarget::new("Local::Doc")
            .ok_or("expected Local::Doc to be a valid documentation target")?;

        if target.perldoc_uri() != "perldoc://Local::Doc" {
            return Err(format!("unexpected perldoc URI: {}", target.perldoc_uri()).into());
        }
        if target.metacpan_pod_uri() != "https://metacpan.org/pod/Local::Doc" {
            return Err(format!("unexpected MetaCPAN URI: {}", target.metacpan_pod_uri()).into());
        }
        if target.perl_org_perldoc_uri() != "https://perldoc.perl.org/Local::Doc" {
            return Err(
                format!("unexpected perl.org URI: {}", target.perl_org_perldoc_uri()).into()
            );
        }
        if target.virtual_perldoc_markdown_link() != "[Open virtual perldoc](perldoc://Local::Doc)"
        {
            return Err(format!(
                "unexpected virtual markdown link: {}",
                target.virtual_perldoc_markdown_link()
            )
            .into());
        }
        if target.metacpan_markdown_link("View on MetaCPAN")
            != "[View on MetaCPAN](https://metacpan.org/pod/Local::Doc)"
        {
            return Err(format!(
                "unexpected MetaCPAN markdown link: {}",
                target.metacpan_markdown_link("View on MetaCPAN")
            )
            .into());
        }

        Ok(())
    }

    #[test]
    fn documentation_target_keeps_core_pragma_virtual_uri_shape() -> TestResult {
        if perldoc_uri("strict").as_deref() != Some("perldoc://strict") {
            return Err(
                format!("unexpected strict perldoc URI: {:?}", perldoc_uri("strict")).into()
            );
        }
        if metacpan_pod_uri("warnings").as_deref() != Some("https://metacpan.org/pod/warnings") {
            return Err(format!(
                "unexpected warnings MetaCPAN URI: {:?}",
                metacpan_pod_uri("warnings")
            )
            .into());
        }

        Ok(())
    }

    #[test]
    fn documentation_target_rejects_empty_path_like_or_malformed_names() -> TestResult {
        for name in [
            "",
            " ",
            "Local::",
            "::Local",
            "Local::Bad-Name",
            "Local/Bad",
            "Local\\Bad",
            "Local::Bad?query",
            "Local::Bad#fragment",
            "Local::Bad Target",
            "https://example.invalid",
        ] {
            if PerlDocumentationTarget::new(name).is_some() {
                return Err(format!(
                    "expected invalid documentation target to be rejected: {name:?}"
                )
                .into());
            }
        }

        Ok(())
    }
}
