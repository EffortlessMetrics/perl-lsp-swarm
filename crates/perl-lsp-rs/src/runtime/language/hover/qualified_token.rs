//! Classify a `::`-qualified hover token as a package/module or a callable.
//!
//! `get_package_name_at_position` previously treated every `::` span as a
//! module name, so `PodHeavy::documented_sub()` reached the missing-module
//! `cpanm` card. This classifier keeps module hover for `use`/`require`
//! (handled before token fallback), arrow receivers (`File::Path->`), and
//! package prefixes, and names the last component of `Pkg::sub` as a callable.

/// How a `::`-qualified token under the cursor should be hovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum QualifiedHoverKind {
    /// Package or module reference (`File::Path->`, or a prefix of `Pkg::sub`).
    Package(String),
    /// Final component of a qualified callable (`Pkg::sub`, `&Pkg::sub`, `Pkg::sub()`).
    Callable {
        /// Full `Package::name` span.
        qualified: String,
        /// Package prefix (`Foo::Bar` in `Foo::Bar::helper`).
        package: String,
        /// Final component (`helper`).
        name: String,
    },
}

/// Classify the `::`-qualified identifier spanning `offset`, if any.
///
/// The scanner is ASCII-only, matching the previous package-name extractor.
/// Unicode qualified-name bounds remain #14616 and are not widened here.
pub(super) fn classify_qualified_hover_token(
    text: &str,
    offset: usize,
) -> Option<QualifiedHoverKind> {
    let (start, span_end, candidate) = qualified_span_at(text, offset)?;
    if !candidate.contains("::") {
        return None;
    }

    let last_sep = candidate.rfind("::")?;
    let final_component_start = last_sep.checked_add(2)?;
    let package = candidate.get(..last_sep)?;
    let name = candidate.get(final_component_start..)?;
    if package.is_empty() || name.is_empty() {
        return None;
    }

    let followed_by_arrow = text
        .get(span_end..)
        .is_some_and(|rest| rest.trim_start_matches([' ', '\t']).starts_with("->"));

    // `File::Path->method`: the whole span is the receiver package, including
    // a cursor on the `File` prefix (existing package-hover coverage).
    if followed_by_arrow {
        return Some(QualifiedHoverKind::Package(candidate.to_string()));
    }

    let rel = offset.saturating_sub(start);
    if rel < final_component_start {
        let prefix = package_prefix_through_cursor(candidate, rel)?;
        if prefix.is_empty() {
            return None;
        }
        return Some(QualifiedHoverKind::Package(prefix));
    }

    Some(QualifiedHoverKind::Callable {
        qualified: candidate.to_string(),
        package: package.to_string(),
        name: name.to_string(),
    })
}

/// Byte span of a `::`-qualified identifier at `offset`, with trailing `:` trimmed.
fn qualified_span_at(text: &str, offset: usize) -> Option<(usize, usize, &str)> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if offset >= len {
        return None;
    }

    let mut start = offset;
    while start > 0 {
        let prev = start - 1;
        if bytes[prev].is_ascii_alphanumeric() || bytes[prev] == b'_' {
            start -= 1;
        } else if prev >= 1 && bytes[prev] == b':' && bytes[prev - 1] == b':' {
            start -= 2;
        } else {
            break;
        }
    }

    let mut end = offset;
    while end < len {
        if bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' {
            end += 1;
        } else if end + 1 < len && bytes[end] == b':' && bytes[end + 1] == b':' {
            end += 2;
        } else {
            break;
        }
    }

    let mut span_end = end;
    while span_end > start && bytes.get(span_end - 1) == Some(&b':') {
        span_end -= 1;
    }
    let candidate = text.get(start..span_end)?;
    Some((start, span_end, candidate))
}

/// Package components from the start of `candidate` through the component at `rel`.
fn package_prefix_through_cursor(candidate: &str, rel: usize) -> Option<String> {
    let bytes = candidate.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut cut = rel.min(candidate.len());
    // Only walk left across `::` when the cursor is on a separator. Walking
    // left from the first byte of a later component (`Bar` in `Foo::Bar`)
    // would otherwise collapse that component into the previous package.
    if bytes.get(cut) == Some(&b':') {
        while cut > 0 && bytes.get(cut - 1) == Some(&b':') {
            cut -= 1;
        }
    }
    while cut < candidate.len() {
        let byte = bytes[cut];
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            cut += 1;
        } else {
            break;
        }
    }
    let prefix = candidate.get(..cut)?.trim_end_matches(':');
    if prefix.is_empty() { None } else { Some(prefix.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::{QualifiedHoverKind, classify_qualified_hover_token};
    use perl_tdd_support::must_some;

    fn offset_of(text: &str, needle: &str) -> usize {
        must_some(text.find(needle))
    }

    #[test]
    fn qualified_call_last_component_is_callable() {
        let text = "print PodHeavy::documented_sub(), \"\\n\";\n";
        let offset = offset_of(text, "documented_sub");
        assert_eq!(
            classify_qualified_hover_token(text, offset),
            Some(QualifiedHoverKind::Callable {
                qualified: "PodHeavy::documented_sub".to_string(),
                package: "PodHeavy".to_string(),
                name: "documented_sub".to_string(),
            })
        );
    }

    #[test]
    fn qualified_call_package_prefix_is_package_not_full_name() {
        let text = "print PodHeavy::documented_sub();\n";
        let offset = offset_of(text, "PodHeavy::");
        assert_eq!(
            classify_qualified_hover_token(text, offset),
            Some(QualifiedHoverKind::Package("PodHeavy".to_string()))
        );
    }

    #[test]
    fn ampersand_qualified_call_is_callable() {
        let text = "print &PodHeavy::documented_sub;\n";
        let offset = offset_of(text, "documented_sub");
        assert_eq!(
            classify_qualified_hover_token(text, offset),
            Some(QualifiedHoverKind::Callable {
                qualified: "PodHeavy::documented_sub".to_string(),
                package: "PodHeavy".to_string(),
                name: "documented_sub".to_string(),
            })
        );
    }

    #[test]
    fn three_component_last_is_callable_and_middle_is_package() {
        let text = "print Foo::Bar::helper();\n";
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "helper")),
            Some(QualifiedHoverKind::Callable {
                qualified: "Foo::Bar::helper".to_string(),
                package: "Foo::Bar".to_string(),
                name: "helper".to_string(),
            })
        );
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "Bar::")),
            Some(QualifiedHoverKind::Package("Foo::Bar".to_string()))
        );
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "::Bar")),
            Some(QualifiedHoverKind::Package("Foo".to_string())),
            "cursor on the separator before Bar names Foo, not Foo::Bar"
        );
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "Foo::")),
            Some(QualifiedHoverKind::Package("Foo".to_string()))
        );
    }

    #[test]
    fn arrow_receiver_is_package_even_on_prefix() {
        let text = "File::Path->make_path('/tmp/test');\n";
        assert_eq!(
            classify_qualified_hover_token(text, 0),
            Some(QualifiedHoverKind::Package("File::Path".to_string()))
        );
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "Path")),
            Some(QualifiedHoverKind::Package("File::Path".to_string()))
        );
    }

    #[test]
    fn arrow_receiver_allows_whitespace_around_arrow() {
        let text = "File::Path -> make_path('/tmp');\n";
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "Path")),
            Some(QualifiedHoverKind::Package("File::Path".to_string()))
        );
    }

    #[test]
    fn bareword_qualified_call_without_parens_is_callable() {
        let text = "print PodHeavy::documented_sub;\n";
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "documented_sub")),
            Some(QualifiedHoverKind::Callable {
                qualified: "PodHeavy::documented_sub".to_string(),
                package: "PodHeavy".to_string(),
                name: "documented_sub".to_string(),
            })
        );
    }

    #[test]
    fn trailing_separator_is_not_a_qualified_name() {
        let text = "print PodHeavy::;\n";
        let offset = offset_of(text, "PodHeavy");
        assert_eq!(
            classify_qualified_hover_token(text, offset),
            None,
            "trimmed trailing :: leaves a bare package, not a ::-qualified token"
        );
    }

    #[test]
    fn fat_comma_is_not_an_arrow_receiver() {
        let text = "Foo::Bar => 1;\n";
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "Bar")),
            Some(QualifiedHoverKind::Callable {
                qualified: "Foo::Bar".to_string(),
                package: "Foo".to_string(),
                name: "Bar".to_string(),
            })
        );
    }

    #[test]
    fn unresolved_call_shape_is_still_callable_classification() {
        let text = "print Missing::definitely_not_a_workspace_sub();\n";
        assert_eq!(
            classify_qualified_hover_token(text, offset_of(text, "definitely_not_a_workspace_sub")),
            Some(QualifiedHoverKind::Callable {
                qualified: "Missing::definitely_not_a_workspace_sub".to_string(),
                package: "Missing".to_string(),
                name: "definitely_not_a_workspace_sub".to_string(),
            })
        );
    }

    #[test]
    fn bare_identifier_is_not_qualified() {
        assert_eq!(classify_qualified_hover_token("print helper();\n", 6), None);
    }
}
