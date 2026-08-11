from pathlib import Path

path = Path("crates/perl-lexer/src/symbol_table.rs")
text = path.read_text()


def replace_once(old: str, new: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one occurrence, found {count}: {old[:100]!r}")
    text = text.replace(old, new, 1)


replace_once(
    "//! line comments, ordinary quoted strings, POD, heredoc bodies, and data\n//! sections. Quote-like operators, regex bodies, formats, imports, generated\n//! code, and source filters remain explicit follow-up boundaries under #6732.\n",
    "//! line comments, ordinary and quote-like strings, pattern bodies, POD,\n//! heredoc bodies, and data sections. Formats, imports, generated code, and\n//! source filters remain explicit follow-up boundaries under #6732.\n",
)
replace_once(
    "use crate::unicode::{is_perl_identifier_continue, is_perl_identifier_start};\n",
    "use crate::lexer::helpers::is_builtin_function;\nuse crate::unicode::{is_perl_identifier_continue, is_perl_identifier_start};\n",
)
replace_once(
    "    /// It excludes declarations spelled inside line comments, ordinary quoted\n    /// strings, POD, recognized heredoc bodies, and `__DATA__`/`__END__`.\n    ///\n    /// Imported symbols, dynamic declarations, quote-like/regex/format bodies,\n    /// `eval`, `AUTOLOAD`, source filters, and workspace symbols are not inferred.\n",
    "    /// It excludes declarations spelled inside line comments, ordinary and\n    /// quote-like strings, pattern bodies, POD, recognized heredoc bodies, and\n    /// `__DATA__`/`__END__`.\n    ///\n    /// Imported symbols, dynamic declarations, format bodies, `eval`, `AUTOLOAD`,\n    /// source filters, and workspace symbols are not inferred.\n",
)
replace_once(
    "                if line.starts_with(\"=cut\") {\n",
    "                if is_pod_cut(line) {\n",
)
replace_once(
    "#[derive(Debug, Default)]\nstruct ScanState {\n    quote: QuoteState,\n    awaiting_sub_name: bool,\n",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\nenum QuoteLikePhase {\n    InBody,\n    AwaitingPairedBody,\n}\n\n#[derive(Debug, Clone, Copy)]\nstruct QuoteLikeState {\n    opener: char,\n    closer: char,\n    paired: bool,\n    depth: usize,\n    bodies_remaining: u8,\n    phase: QuoteLikePhase,\n}\n\nimpl QuoteLikeState {\n    fn new(opener: char, bodies_remaining: u8) -> Self {\n        let (closer, paired) = paired_delimiter(opener).map_or((opener, false), |closer| {\n            (closer, true)\n        });\n        Self {\n            opener,\n            closer,\n            paired,\n            depth: usize::from(paired),\n            bodies_remaining,\n            phase: QuoteLikePhase::InBody,\n        }\n    }\n\n    fn begin_paired_body(&mut self, opener: char) {\n        let (closer, paired) = paired_delimiter(opener).map_or((opener, false), |closer| {\n            (closer, true)\n        });\n        self.opener = opener;\n        self.closer = closer;\n        self.paired = paired;\n        self.depth = usize::from(paired);\n        self.phase = QuoteLikePhase::InBody;\n    }\n}\n\n#[derive(Debug, Default)]\nstruct ScanState {\n    quote: QuoteState,\n    quote_like: Option<QuoteLikeState>,\n    awaiting_sub_name: bool,\n",
)
replace_once(
    "impl PendingHeredoc {\n    fn matches_terminator(&self, line: &str) -> bool {\n        let line = line.trim_end_matches([' ', '\\t']);\n        if self.allow_indent {\n            line.trim_start_matches([' ', '\\t']) == self.label.as_ref()\n        } else {\n            line == self.label.as_ref()\n        }\n    }\n}\n",
    "impl PendingHeredoc {\n    fn matches_terminator(&self, line: &str) -> bool {\n        if self.allow_indent {\n            line.trim_start_matches([' ', '\\t']) == self.label.as_ref()\n        } else {\n            line == self.label.as_ref()\n        }\n    }\n}\n",
)
replace_once(
    "fn starts_pod(line: &str) -> bool {\n    [\n        \"=pod\",\n        \"=head\",\n        \"=over\",\n        \"=item\",\n        \"=back\",\n        \"=begin\",\n        \"=end\",\n        \"=for\",\n        \"=encoding\",\n    ]\n    .iter()\n    .any(|directive| line.starts_with(directive))\n}\n",
    "fn pod_command(line: &str) -> Option<&str> {\n    let rest = line.strip_prefix('=')?;\n    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());\n    rest.get(..end)\n}\n\nfn starts_pod(line: &str) -> bool {\n    let Some(command) = pod_command(line) else {\n        return false;\n    };\n    matches!(\n        command,\n        \"pod\" | \"over\" | \"item\" | \"back\" | \"begin\" | \"end\" | \"for\" | \"encoding\"\n    ) || command == \"head\"\n        || command\n            .strip_prefix(\"head\")\n            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))\n}\n\nfn is_pod_cut(line: &str) -> bool {\n    pod_command(line) == Some(\"cut\")\n}\n",
)
replace_once(
    "    while offset < line.len() {\n        if state.quote != QuoteState::Code {\n",
    "    while offset < line.len() {\n        if state.quote_like.is_some() {\n            offset = scan_quote_like_character(line, offset, state);\n            continue;\n        }\n\n        if state.quote != QuoteState::Code {\n",
)
replace_once(
    "        if ch == '#' {\n            return;\n        }\n\n        if ch == '\\'' {\n            if apostrophe_is_package_separator(line, offset) {\n",
    "        if ch == '#' {\n            return;\n        }\n\n        if let Some(end) = start_quote_like(line, offset, state) {\n            offset = end;\n            continue;\n        }\n\n        if ch == '\\'' {\n            if apostrophe_is_package_separator(line, offset, known_subs) {\n",
)
replace_once(
    "        if line[offset..].starts_with(\"<<\")\n            && heredoc_allowed_before(line, offset)\n",
    "        if line[offset..].starts_with(\"<<\")\n            && heredoc_allowed_before(line, offset, known_subs)\n",
)
replace_once(
    "fn apostrophe_is_package_separator(line: &str, offset: usize) -> bool {\n    let before = line[..offset].chars().next_back();\n    let after = line[offset + '\\''.len_utf8()..].chars().next();\n    before.is_some_and(is_name_segment_continue) && after.is_some_and(is_perl_identifier_start)\n}\n",
    "fn apostrophe_is_package_separator(\n    line: &str,\n    offset: usize,\n    known_subs: &HashSet<Box<str>>,\n) -> bool {\n    let before = line[..offset].chars().next_back();\n    let after = line[offset + '\\''.len_utf8()..].chars().next();\n    if !before.is_some_and(is_name_segment_continue)\n        || !after.is_some_and(is_perl_identifier_start)\n    {\n        return false;\n    }\n\n    previous_word_before(line, offset)\n        .is_none_or(|word| !is_builtin_function(word) && !known_subs.contains(word))\n}\n",
)
marker = "fn is_sub_keyword_boundary(line: &str, offset: usize) -> bool {\n"
if text.count(marker) != 1:
    raise SystemExit("missing quote-like insertion marker")
quote_like_helpers = r'''fn paired_delimiter(opener: char) -> Option<char> {
    match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '<' => Some('>'),
        _ => None,
    }
}

fn quote_like_operator_at(line: &str, offset: usize) -> Option<(&'static str, u8)> {
    let prefix = line[..offset].trim_end_matches([' ', '\t']);
    if prefix.ends_with("->")
        || prefix.chars().next_back().is_some_and(|ch| {
            is_perl_identifier_continue(ch)
                || matches!(ch, ':' | '\'' | '$' | '@' | '%' | '&' | '*' | '-')
        })
    {
        return None;
    }

    for (operator, bodies) in [
        ("tr", 2),
        ("qq", 1),
        ("qw", 1),
        ("qx", 1),
        ("qr", 1),
        ("q", 1),
        ("m", 1),
        ("s", 2),
        ("y", 2),
    ] {
        if !line[offset..].starts_with(operator) {
            continue;
        }
        let after = offset + operator.len();
        if line[after..].chars().next().is_some_and(|ch| {
            is_perl_identifier_continue(ch) || matches!(ch, ':' | '\'')
        }) {
            continue;
        }
        return Some((operator, bodies));
    }
    None
}

fn start_quote_like(line: &str, offset: usize, state: &mut ScanState) -> Option<usize> {
    let (operator, bodies) = quote_like_operator_at(line, offset)?;
    let mut delimiter_offset = skip_horizontal_whitespace(line, offset + operator.len());
    if delimiter_offset >= line.len() || line[delimiter_offset..].starts_with("=>") {
        return None;
    }

    let delimiter = line[delimiter_offset..].chars().next()?;
    if delimiter.is_alphanumeric() || delimiter == '_' {
        return None;
    }

    let prefix = line[..offset].trim_end_matches([' ', '\t']);
    if prefix.ends_with('{') && delimiter == '}' {
        return None;
    }

    delimiter_offset += delimiter.len_utf8();
    state.quote_like = Some(QuoteLikeState::new(delimiter, bodies));
    Some(delimiter_offset)
}

fn scan_quote_like_character(line: &str, mut offset: usize, state: &mut ScanState) -> usize {
    let Some(mut quote_like) = state.quote_like else {
        return offset;
    };

    if quote_like.phase == QuoteLikePhase::AwaitingPairedBody {
        offset = skip_horizontal_whitespace(line, offset);
        let Some(opener) = line[offset..].chars().next() else {
            state.quote_like = Some(quote_like);
            return line.len();
        };
        if opener.is_alphanumeric() || opener == '_' {
            state.quote_like = None;
            return offset;
        }
        quote_like.begin_paired_body(opener);
        state.quote_like = Some(quote_like);
        return offset + opener.len_utf8();
    }

    let Some(ch) = line[offset..].chars().next() else {
        state.quote_like = Some(quote_like);
        return line.len();
    };

    if ch == '\\' {
        let after_slash = offset + ch.len_utf8();
        state.quote_like = Some(quote_like);
        return line[after_slash..]
            .chars()
            .next()
            .map_or(after_slash, |escaped| after_slash + escaped.len_utf8());
    }

    if quote_like.paired && ch == quote_like.opener {
        quote_like.depth = quote_like.depth.saturating_add(1);
    }

    if ch == quote_like.closer {
        if quote_like.paired && quote_like.depth > 1 {
            quote_like.depth -= 1;
        } else {
            quote_like.bodies_remaining = quote_like.bodies_remaining.saturating_sub(1);
            if quote_like.bodies_remaining == 0 {
                state.quote_like = None;
                return offset + ch.len_utf8();
            }
            if quote_like.paired {
                quote_like.phase = QuoteLikePhase::AwaitingPairedBody;
            }
        }
    }

    state.quote_like = Some(quote_like);
    offset + ch.len_utf8()
}

fn previous_word_before(text: &str, end: usize) -> Option<&str> {
    let prefix = text[..end].trim_end_matches([' ', '\t']);
    let mut start = prefix.len();
    while let Some(ch) = prefix[..start].chars().next_back() {
        if is_perl_identifier_continue(ch) || matches!(ch, ':' | '\'') {
            start -= ch.len_utf8();
        } else {
            break;
        }
    }
    (start < prefix.len()).then(|| &prefix[start..])
}

'''
text = text.replace(marker, quote_like_helpers + marker, 1)
replace_once(
    "fn heredoc_allowed_before(line: &str, offset: usize) -> bool {\n    let prefix = line[..offset].trim_end_matches([' ', '\\t']);\n    if prefix.is_empty() {\n        return true;\n    }\n\n    if prefix\n        .chars()\n        .next_back()\n        .is_some_and(|ch| matches!(ch, '=' | '(' | '[' | '{' | ',' | ';' | ':' | '?'))\n    {\n        return true;\n    }\n\n    prefix\n        .split(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'))\n        .next_back()\n        .is_some_and(|word| matches!(word, \"print\" | \"say\" | \"warn\" | \"die\" | \"return\"))\n}\n",
    "fn heredoc_allowed_before(\n    line: &str,\n    offset: usize,\n    known_subs: &HashSet<Box<str>>,\n) -> bool {\n    let prefix = line[..offset].trim_end_matches([' ', '\\t']);\n    if prefix.is_empty() {\n        return true;\n    }\n\n    if prefix\n        .chars()\n        .next_back()\n        .is_some_and(|ch| matches!(ch, '=' | '(' | '[' | '{' | ',' | ';' | ':' | '?'))\n    {\n        return true;\n    }\n\n    previous_word_before(line, offset)\n        .is_some_and(|word| is_builtin_function(word) || known_subs.contains(word))\n}\n",
)
replace_once(
    "    let (label, end) = if matches!(first, '\\'' | '\"' | '`') {\n        parse_quoted_heredoc_label(line, offset, first)?\n",
    "    let (label, end) = if matches!(first, '\\'' | '\"' | '`') {\n        parse_quoted_heredoc_label(line, offset, first)?\n",
)
replace_once(
    "\n    if label.is_empty() {\n        return None;\n    }\n\n    Some((PendingHeredoc { label: label.into_boxed_str(), allow_indent }, end))\n",
    "\n    Some((PendingHeredoc { label: label.into_boxed_str(), allow_indent }, end))\n",
)
replace_once(
    "        if ch == '\\\\' {\n            offset += ch.len_utf8();\n            let escaped = line[offset..].chars().next()?;\n            label.push(escaped);\n            offset += escaped.len_utf8();\n            continue;\n        }\n",
    "        if ch == '\\\\' {\n            offset += ch.len_utf8();\n            let escaped = line[offset..].chars().next()?;\n            if quote == '\\'' && !matches!(escaped, '\\'' | '\\\\') {\n                label.push('\\\\');\n            }\n            label.push(escaped);\n            offset += escaped.len_utf8();\n            continue;\n        }\n",
)

tests = r'''

    #[test]
    fn pod_cut_requires_the_exact_command_name() {
        let source = "=pod\nsub fake_one { }\n=cutlery\nsub fake_two { }\n=cut\nsub real { }\n";
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake_one"));
        assert!(!table.is_known_sub("fake_two"));
    }

    #[test]
    fn declared_callable_heredoc_excludes_its_body() {
        let source = concat!(
            "sub sink { }\n",
            "sink <<END;\n",
            "sub fake { }\n",
            "END\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("sink"));
        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn adjacent_builtin_string_does_not_hide_following_declarations() {
        let source = "print'hello';\nsub builder { }\nbuilder /x/;";
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("builder"));
        let tokens = tokens_with_table(source, table);
        assert!(tokens.iter().any(|token| matches!(&token.token_type, TokenType::RegexMatch)));
    }

    #[test]
    fn single_quoted_heredoc_labels_preserve_literal_backslashes() {
        let source = concat!(
            "my $doc = <<'A\\B';\n",
            "sub fake { }\n",
            "A\\B\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn ordinary_heredoc_terminators_reject_trailing_whitespace() {
        let source = concat!(
            "my $doc = <<END;\n",
            "sub fake_one { }\n",
            "END \n",
            "sub fake_two { }\n",
            "END\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake_one"));
        assert!(!table.is_known_sub("fake_two"));
    }

    #[test]
    fn empty_quoted_heredoc_labels_are_excluded_until_a_blank_line() {
        let source = concat!(
            "my $doc = <<\"\";\n",
            "sub fake { }\n",
            "\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn quote_like_and_pattern_bodies_cannot_create_known_subs() {
        let source = concat!(
            "my $q = q{sub q_fake { }};\n",
            "my $qq = qq{sub qq_fake { }};\n",
            "my $qw = qw(sub qw_fake);\n",
            "my $qx = qx{sub qx_fake};\n",
            "my $qr = qr{sub qr_fake { }};\n",
            "my $m = m{sub m_fake { }};\n",
            "my $s = s{sub s_fake { }}{replacement};\n",
            "my $tr = tr{sub tr_fake}{replacement};\n",
            "my $y = y{sub y_fake}{replacement};\n",
            "sub q { }\n",
            "sub Foo::s { }\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        for real in ["q", "Foo::s", "real"] {
            assert!(table.is_known_sub(real), "missing real declaration {real:?}");
        }
        for fake in [
            "q_fake", "qq_fake", "qw_fake", "qx_fake", "qr_fake", "m_fake", "s_fake",
            "tr_fake", "y_fake",
        ] {
            assert!(!table.is_known_sub(fake), "quote-like body leaked {fake:?}");
        }
    }

    #[test]
    fn repeated_non_heredoc_shifts_do_not_invent_names() {
        let mut source = String::from("my $x = 1");
        for _ in 0..4096 {
            source.push_str(" << 1");
        }
        source.push_str(";\nsub real { }\n");
        let table = LocalSymbolTable::scan_subs(&source);

        assert_eq!(table.len(), 1);
        assert!(table.is_known_sub("real"));
    }
'''
head, separator, tail = text.rpartition("\n}")
if not separator:
    raise SystemExit("could not locate test module terminator")
text = head + tests + separator + tail

path.write_text(text)
