from pathlib import Path

path = Path("crates/perl-lexer/src/symbol_table.rs")
text = path.read_text()


def replace_once(old: str, new: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one occurrence, found {count}: {old[:120]!r}")
    text = text.replace(old, new, 1)


replace_once(
    "            scan_code_line(line, &mut state, &mut known_subs);\n",
    "            scan_code_line(input, line_start, line, &mut state, &mut known_subs);\n",
)
replace_once(
    "fn scan_code_line(line: &str, state: &mut ScanState, known_subs: &mut HashSet<Box<str>>) {\n",
    "fn scan_code_line(\n    input: &str,\n    line_start: usize,\n    line: &str,\n    state: &mut ScanState,\n    known_subs: &mut HashSet<Box<str>>,\n) {\n",
)
replace_once(
    "        if let Some(end) = start_quote_like(line, offset, state) {\n",
    "        if let Some(end) = start_quote_like(input, line_start, line, offset, state) {\n",
)
replace_once(
    "fn start_quote_like(line: &str, offset: usize, state: &mut ScanState) -> Option<usize> {\n",
    "fn start_quote_like(\n    input: &str,\n    line_start: usize,\n    line: &str,\n    offset: usize,\n    state: &mut ScanState,\n) -> Option<usize> {\n",
)
replace_once(
    "    delimiter_offset += delimiter.len_utf8();\n    state.quote_like = Some(QuoteLikeState::new(delimiter, bodies));\n    Some(delimiter_offset)\n}\n\nfn scan_quote_like_character",
    "    delimiter_offset += delimiter.len_utf8();\n    let quote_like = QuoteLikeState::new(delimiter, bodies);\n    let absolute_body_start = line_start + delimiter_offset;\n    if input\n        .get(absolute_body_start..)\n        .is_some_and(|suffix| quote_like_closes(suffix, quote_like))\n    {\n        state.quote_like = Some(quote_like);\n        Some(delimiter_offset)\n    } else {\n        // A malformed quote-like construct must not hide every declaration in\n        // the rest of the file. Keep its current physical line opaque, then\n        // resume the declaration prepass on the following line.\n        state.quote_like = None;\n        Some(line.len())\n    }\n}\n\nfn quote_like_closes(source: &str, mut quote_like: QuoteLikeState) -> bool {\n    let mut chars = source.chars();\n\n    while let Some(ch) = chars.next() {\n        if quote_like.phase == QuoteLikePhase::AwaitingPairedBody {\n            if ch.is_whitespace() {\n                continue;\n            }\n            if ch.is_alphanumeric() || ch == '_' {\n                return false;\n            }\n            quote_like.begin_paired_body(ch);\n            continue;\n        }\n\n        if ch == '\\\\' {\n            let _ = chars.next();\n            continue;\n        }\n\n        if quote_like.paired && ch == quote_like.opener {\n            quote_like.depth = quote_like.depth.saturating_add(1);\n            continue;\n        }\n        if ch != quote_like.closer {\n            continue;\n        }\n\n        if quote_like.paired && quote_like.depth > 1 {\n            quote_like.depth -= 1;\n            continue;\n        }\n\n        quote_like.bodies_remaining = quote_like.bodies_remaining.saturating_sub(1);\n        if quote_like.bodies_remaining == 0 {\n            return true;\n        }\n        if quote_like.paired {\n            quote_like.phase = QuoteLikePhase::AwaitingPairedBody;\n        }\n    }\n\n    false\n}\n\nfn scan_quote_like_character",
)

extra_tests = r'''

    #[test]
    fn unclosed_quote_like_does_not_hide_later_declarations() {
        let source = concat!(
            "my @items = qw[word1 word2\n",
            "emit \"bad\";\n",
            "print 1;\n",
            "sub emit { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("emit"));
    }

    #[test]
    fn closed_multiline_quote_like_still_excludes_declaration_text() {
        let source = concat!(
            "my $doc = q{\n",
            "sub fake { }\n",
            "};\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake"));
    }

    #[test]
    fn closed_multiline_two_body_quote_like_excludes_both_bodies() {
        let source = concat!(
            "my $value = 'before';\n",
            "$value =~ s{\n",
            "sub fake_pattern { }\n",
            "}{\n",
            "sub fake_replacement { }\n",
            "};\n",
            "sub real { }\n",
        );
        let table = LocalSymbolTable::scan_subs(source);

        assert!(table.is_known_sub("real"));
        assert!(!table.is_known_sub("fake_pattern"));
        assert!(!table.is_known_sub("fake_replacement"));
    }
'''
head, separator, tail = text.rpartition("\n}")
if not separator:
    raise SystemExit("could not locate test module terminator")
text = head + extra_tests + separator + tail

path.write_text(text)
