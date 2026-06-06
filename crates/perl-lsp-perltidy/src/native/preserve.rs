pub(super) fn literal_preserve_region(source: &str) -> Option<&'static str> {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if is_pod_start(trimmed) {
            return Some("POD");
        }
        if matches!(trimmed.trim_end(), "__DATA__" | "__END__") {
            return Some("DATA/END section");
        }
        if contains_likely_heredoc_start(line) {
            return Some("heredoc");
        }
        if is_format_declaration_start(trimmed) {
            return Some("format body");
        }
    }
    token_literal_preserve_region(source)
}

pub(super) fn token_literal_preserve_region(source: &str) -> Option<&'static str> {
    use perl_parser_core::TokenKind;

    let mut stream = perl_parser_core::TokenStream::new(source);
    loop {
        let Ok(token) = stream.next() else {
            return None;
        };
        match token.kind {
            TokenKind::Eof => return None,
            TokenKind::Regex => return Some("regex literal"),
            TokenKind::Substitution => return Some("substitution operator"),
            TokenKind::Transliteration => return Some("transliteration operator"),
            TokenKind::QuoteSingle
            | TokenKind::QuoteDouble
            | TokenKind::QuoteWords
            | TokenKind::QuoteCommand => return Some("quote-like operator"),
            TokenKind::FormatBody => return Some("format body"),
            _ => {}
        }
    }
}

pub(super) fn is_pod_start(trimmed_line: &str) -> bool {
    matches!(
        trimmed_line.split_whitespace().next(),
        Some(
            "=pod"
                | "=head1"
                | "=head2"
                | "=head3"
                | "=head4"
                | "=over"
                | "=item"
                | "=back"
                | "=begin"
                | "=end"
                | "=for"
                | "=encoding"
                | "=cut"
        )
    )
}

pub(super) fn contains_likely_heredoc_start(line: &str) -> bool {
    let Some((_, after_marker)) = line.split_once("<<") else {
        return false;
    };
    if after_marker.starts_with('<') {
        return false;
    }

    let after_indent = after_marker.trim_start();
    let marker = after_indent.strip_prefix('~').unwrap_or(after_indent).trim_start();
    let marker = marker.strip_prefix(['\'', '"', '`']).unwrap_or(marker);
    marker.chars().next().is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

pub(super) fn is_format_declaration_start(trimmed_line: &str) -> bool {
    if !trimmed_line.ends_with('=') {
        return false;
    }

    let Some(rest) = trimmed_line.strip_prefix("format") else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
}
