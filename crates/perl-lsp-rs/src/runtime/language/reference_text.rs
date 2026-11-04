//! Text-search occurrence kind and location identity for `textDocument/references`.
//!
//! Word-boundary search is not identity: a subroutine query for `name` must not
//! keep hash-key literals (`name =>`, `$args{name}`, `$self->{name}`), and a
//! variable query for `$AUTOLOAD` must not absorb `sub AUTOLOAD`. Combined
//! index-plus-text answers are then collapsed by identical `(uri, range)`, not
//! by JSON object equality, so key-order differences cannot reintroduce
//! duplicates (#16638).
//!
//! Identifier bounds treat `:` as a separator so `Pkg::name` is a hit for
//! needle `name`. `perl-symbol::is_word_boundary` does not, because `:` is a
//! name byte. Hash-key braces require `->` or a sigiled identifier; block
//! bodies are not keys. Double-quoted `$name` stays a variable reference.

use crate::util::byte_to_utf16_col;
use serde_json::{Value, json};

/// Line context of a candidate match. Quote/comment scanning is line-local and
/// does not model `q{}`/`qq{}` or interpolating heredocs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineMatchContext {
    Code,
    SingleString,
    DoubleString,
    Comment,
}

/// Query identity used to filter text-search hits.
#[derive(Debug, Clone, Copy)]
pub(super) struct TextReferenceQuery<'a> {
    pub needle: &'a str,
    pub sigil: Option<char>,
    pub include_declaration: bool,
}

/// Kind of a word occurrence in a single source line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReferenceWordKind {
    Subroutine,
    Variable,
    HashKey,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct LocationId<'a> {
    uri: &'a str,
    start_line: u64,
    start_character: u64,
    end_line: u64,
    end_character: u64,
}

/// Strip a leading sigil from the needle so word-boundary search can run on the
/// identifier. `$`/`@`/`%` become variable queries; `&`/`*` stay subroutine
/// queries because they name callables, not storage.
pub(super) fn normalized_text_query(needle: &str, sigil: Option<char>) -> (&str, Option<char>) {
    if let Some(sigil) = sigil {
        if let Some(stripped) = needle.strip_prefix(sigil) {
            return (stripped, Some(sigil));
        }
        return (needle, Some(sigil));
    }
    match needle.as_bytes().first().copied() {
        Some(b @ (b'$' | b'@' | b'%')) => (needle.get(1..).unwrap_or(""), Some(char::from(b))),
        Some(b'&' | b'*') => (needle.get(1..).unwrap_or(""), None),
        _ => (needle, None),
    }
}

/// Classify one identifier occurrence using only the current line.
pub(super) fn classify_reference_word(
    line: &str,
    match_start: usize,
    match_end: usize,
) -> ReferenceWordKind {
    if !valid_span(line, match_start, match_end) {
        return ReferenceWordKind::Other;
    }
    if is_sigiled_variable(line, match_start) || is_braced_sigil_deref(line, match_start, match_end)
    {
        return ReferenceWordKind::Variable;
    }
    if is_hash_key_occurrence(line, match_start, match_end) {
        return ReferenceWordKind::HashKey;
    }
    if is_subroutine_occurrence(line, match_start, match_end) {
        return ReferenceWordKind::Subroutine;
    }
    ReferenceWordKind::Other
}

/// Keep a text-search hit only when its word-kind matches the query.
pub(super) fn keep_text_reference_match(
    line: &str,
    match_start: usize,
    match_end: usize,
    sigil: Option<char>,
    include_declaration: bool,
) -> bool {
    if !valid_span(line, match_start, match_end) {
        return false;
    }
    let context = line_match_context(line, match_start);
    if matches!(context, LineMatchContext::Comment | LineMatchContext::SingleString) {
        return false;
    }
    let kind = classify_reference_word(line, match_start, match_end);
    if context == LineMatchContext::DoubleString {
        // Double quotes interpolate `$name` / `@name` / `%name`; uninterpolated
        // literals (`"name"`) and quoted hash keys stay dropped.
        return matches!(sigil, Some('$' | '@' | '%'))
            && kind == ReferenceWordKind::Variable
            && !should_skip_text_reference_match(line, match_start, sigil, include_declaration);
    }
    match sigil {
        Some('$' | '@' | '%') => {
            kind == ReferenceWordKind::Variable
                && !should_skip_text_reference_match(line, match_start, sigil, include_declaration)
        }
        Some('&' | '*') => kind == ReferenceWordKind::Subroutine,
        None => matches!(kind, ReferenceWordKind::Subroutine | ReferenceWordKind::Other),
        Some(_) => false,
    }
}

/// Skip lexical declaration targets when `includeDeclaration` is false.
pub(super) fn should_skip_text_reference_match(
    line: &str,
    match_start: usize,
    sigil: Option<char>,
    include_declaration: bool,
) -> bool {
    if include_declaration {
        return false;
    }

    let Some(sigil) = sigil else {
        return false;
    };

    let symbol_start = line
        .get(..match_start)
        .and_then(|prefix| prefix.char_indices().next_back())
        .and_then(|(idx, ch)| (ch == sigil).then_some(idx))
        .unwrap_or(match_start);
    let Some(prefix) = line.get(..symbol_start) else {
        return false;
    };

    let statement_prefix =
        prefix.rfind([';', '{', '}']).map(|idx| &prefix[idx + 1..]).unwrap_or(prefix);
    if statement_prefix.contains('=') {
        return false;
    }

    statement_prefix
        .split(|ch: char| !ch.is_ascii_alphabetic() && ch != '_')
        .any(|token| matches!(token, "my" | "our" | "state" | "local"))
}

/// Scan documents for word-boundary occurrences of the query, filtered by kind.
pub(super) fn search_document_texts_for_references<'a, I>(
    documents: I,
    query: TextReferenceQuery<'_>,
    cap: usize,
) -> Vec<Value>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let (needle, sigil) = normalized_text_query(query.needle, query.sigil);
    if needle.is_empty() || cap == 0 {
        return Vec::new();
    }

    let needle_bytes = needle.as_bytes();
    let mut out = Vec::new();

    'docs: for (doc_uri, doc_text) in documents {
        for (line_num, line) in doc_text.lines().enumerate() {
            let line_bytes = line.as_bytes();
            let mut start = 0usize;
            while let Some(idx) = line.get(start..).and_then(|tail| tail.find(needle)) {
                let byte_pos = start + idx;
                let match_end = byte_pos + needle_bytes.len();
                if is_reference_ident_boundary(line_bytes, byte_pos, needle_bytes.len())
                    && keep_text_reference_match(
                        line,
                        byte_pos,
                        match_end,
                        sigil,
                        query.include_declaration,
                    )
                {
                    let (range_start, range_end) =
                        emitted_match_byte_range(line, byte_pos, match_end);
                    let start_utf16 = byte_to_utf16_col(line, range_start);
                    let end_utf16 = byte_to_utf16_col(line, range_end);
                    out.push(location_value(doc_uri, line_num, start_utf16, end_utf16));
                    if out.len() >= cap {
                        break 'docs;
                    }
                }
                start = match_end;
            }
        }
    }

    finalize_reference_locations(out, cap)
}

/// Collapse identical `(uri, range)` locations, independent of JSON key order.
pub(super) fn dedupe_reference_locations(locations: &mut Vec<Value>) {
    locations.sort_by(|left, right| location_id(left).cmp(&location_id(right)));
    locations.dedup_by(|left, right| match (location_id(left), location_id(right)) {
        (Some(left_id), Some(right_id)) => left_id == right_id,
        _ => false,
    });
}

/// Deduplicate, then enforce the result cap so duplicates cannot consume it.
pub(super) fn finalize_reference_locations(mut locations: Vec<Value>, cap: usize) -> Vec<Value> {
    dedupe_reference_locations(&mut locations);
    locations.truncate(cap);
    locations
}

fn location_value(uri: &str, line: usize, start_utf16: usize, end_utf16: usize) -> Value {
    json!({
        "uri": uri,
        "range": {
            "start": { "line": line, "character": start_utf16 },
            "end": { "line": line, "character": end_utf16 },
        },
    })
}

fn location_id(location: &Value) -> Option<LocationId<'_>> {
    Some(LocationId {
        uri: location.get("uri")?.as_str()?,
        start_line: location.pointer("/range/start/line")?.as_u64()?,
        start_character: location.pointer("/range/start/character")?.as_u64()?,
        end_line: location.pointer("/range/end/line")?.as_u64()?,
        end_character: location.pointer("/range/end/character")?.as_u64()?,
    })
}

fn valid_span(line: &str, match_start: usize, match_end: usize) -> bool {
    match_start <= match_end && match_end <= line.len() && line.is_char_boundary(match_start)
}

fn char_before(line: &str, idx: usize) -> Option<char> {
    line.get(..idx)?.chars().next_back()
}

fn char_at(line: &str, idx: usize) -> Option<char> {
    line.get(idx..)?.chars().next()
}

fn skip_ws_left(line: &str, mut idx: usize) -> usize {
    while idx > 0 {
        let Some(ch) = char_before(line, idx) else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        idx = idx.saturating_sub(ch.len_utf8());
    }
    idx
}

fn skip_ws_right(line: &str, mut idx: usize) -> usize {
    while let Some(ch) = char_at(line, idx) {
        if !ch.is_whitespace() {
            break;
        }
        idx = idx.saturating_add(ch.len_utf8());
        if idx > line.len() {
            break;
        }
    }
    idx
}

fn unwrap_adjacent_quotes(line: &str, start: usize, end: usize) -> (usize, usize) {
    let Some(left) = char_before(line, start) else {
        return (start, end);
    };
    let Some(right) = char_at(line, end) else {
        return (start, end);
    };
    if left == right && (left == '\'' || left == '"') {
        (start.saturating_sub(left.len_utf8()), end.saturating_add(right.len_utf8()))
    } else {
        (start, end)
    }
}

fn is_sigiled_variable(line: &str, match_start: usize) -> bool {
    matches!(char_before(line, match_start), Some('$' | '@' | '%'))
}

/// `${name}`, `@{name}`, `%{name}` (optional whitespace inside the braces).
fn is_braced_sigil_deref(line: &str, match_start: usize, match_end: usize) -> bool {
    let inner_start = skip_ws_left(line, match_start);
    let inner_end = skip_ws_right(line, match_end);
    if char_before(line, inner_start) != Some('{') || char_at(line, inner_end) != Some('}') {
        return false;
    }
    let brace_at = inner_start.saturating_sub('{'.len_utf8());
    let before_brace = skip_ws_left(line, brace_at);
    matches!(char_before(line, before_brace), Some('$' | '@' | '%'))
}

fn is_hash_key_occurrence(line: &str, match_start: usize, match_end: usize) -> bool {
    let (extent_start, extent_end) = unwrap_adjacent_quotes(line, match_start, match_end);
    let after = skip_ws_right(line, extent_end);
    if line.get(after..).is_some_and(|rest| rest.starts_with("=>")) {
        return true;
    }

    let inner_start = skip_ws_left(line, extent_start);
    let inner_end = skip_ws_right(line, extent_end);
    if char_before(line, inner_start) != Some('{') || char_at(line, inner_end) != Some('}') {
        return false;
    }
    if is_braced_sigil_deref(line, match_start, match_end) {
        return false;
    }
    has_hash_subscript_context(line, inner_start)
}

/// `{ident}` is a hash key only after `->` or a sigiled identifier (`$h{k}`).
/// Bare block bodies (`if { name }`, `map { name }`) are not hash keys.
fn has_hash_subscript_context(line: &str, inner_start: usize) -> bool {
    let brace_at = inner_start.saturating_sub('{'.len_utf8());
    let before_brace = skip_ws_left(line, brace_at);
    if char_before(line, before_brace) == Some('>') {
        let gt_at = before_brace.saturating_sub('>'.len_utf8());
        let before_gt = skip_ws_left(line, gt_at);
        return char_before(line, before_gt) == Some('-');
    }
    let Some(ident) = previous_ident(line, brace_at) else {
        return false;
    };
    let ident_end = skip_ws_left(line, brace_at);
    let ident_start = ident_end.saturating_sub(ident.len());
    matches!(char_before(line, ident_start), Some('$' | '@' | '%'))
}

/// `perl-symbol::is_word_boundary` treats `:` as a name byte, so `Pkg::name`
/// is one token. Regex `\b` (the previous combined-path scanner) does not.
/// Identifier-only bounds restore `Pkg::name` hits for needle `name` without
/// matching `namesake`.
fn is_reference_ident_boundary(text: &[u8], pos: usize, word_len: usize) -> bool {
    let Some(end_pos) = pos.checked_add(word_len) else {
        return false;
    };
    if pos > 0 && is_ident_byte(text[pos - 1]) {
        return false;
    }
    end_pos >= text.len() || !is_ident_byte(text[end_pos])
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Include a leading `$`/`@`/`%` (and `${name}` braces) so text hits share
/// identity with index locations that span the sigil.
fn emitted_match_byte_range(line: &str, match_start: usize, match_end: usize) -> (usize, usize) {
    if is_braced_sigil_deref(line, match_start, match_end) {
        let inner_start = skip_ws_left(line, match_start);
        let brace_at = inner_start.saturating_sub('{'.len_utf8());
        let before_brace = skip_ws_left(line, brace_at);
        let Some(sigil) = char_before(line, before_brace) else {
            return (match_start, match_end);
        };
        if !matches!(sigil, '$' | '@' | '%') {
            return (match_start, match_end);
        }
        let start = before_brace.saturating_sub(sigil.len_utf8());
        let inner_end = skip_ws_right(line, match_end);
        let end = if char_at(line, inner_end) == Some('}') {
            inner_end.saturating_add('}'.len_utf8())
        } else {
            match_end
        };
        return (start, end);
    }
    if let Some(sigil) = char_before(line, match_start)
        && matches!(sigil, '$' | '@' | '%')
    {
        return (match_start.saturating_sub(sigil.len_utf8()), match_end);
    }
    (match_start, match_end)
}

fn is_subroutine_occurrence(line: &str, match_start: usize, match_end: usize) -> bool {
    if matches!(char_before(line, match_start), Some('&' | '*')) {
        return true;
    }

    let left = skip_ws_left(line, match_start);
    if char_before(line, left) == Some('>') {
        return true;
    }
    if line.get(..left).is_some_and(|prefix| prefix.ends_with("::")) {
        return true;
    }
    if matches!(previous_ident(line, match_start), Some("sub" | "method")) {
        return true;
    }

    let right = skip_ws_right(line, match_end);
    char_at(line, right) == Some('(')
}

fn previous_ident(line: &str, match_start: usize) -> Option<&str> {
    let idx = skip_ws_left(line, match_start);
    let prefix = line.get(..idx)?;
    let start = match prefix.char_indices().rev().find(|(_, ch)| !is_ident_char(*ch)) {
        Some((byte_idx, ch)) => byte_idx.saturating_add(ch.len_utf8()),
        None => 0,
    };
    let ident = prefix.get(start..)?;
    if ident.is_empty() { None } else { Some(ident) }
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn line_match_context(line: &str, match_start: usize) -> LineMatchContext {
    let Some(prefix) = line.get(..match_start) else {
        return LineMatchContext::Comment;
    };
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = prefix.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' && (in_single || in_double) {
            let _ = chars.next();
            continue;
        }
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return LineMatchContext::Comment,
            _ => {}
        }
    }
    if in_single {
        LineMatchContext::SingleString
    } else if in_double {
        LineMatchContext::DoubleString
    } else {
        LineMatchContext::Code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn start_of(location: &Value) -> Result<(u64, u64), Box<dyn Error>> {
        let line = location["range"]["start"]["line"].as_u64().ok_or("missing start line")?;
        let character =
            location["range"]["start"]["character"].as_u64().ok_or("missing start character")?;
        Ok((line, character))
    }

    fn uri_start_of(location: &Value) -> Result<(String, u64, u64), Box<dyn Error>> {
        let uri = location["uri"].as_str().ok_or("missing uri")?.to_string();
        let (line, character) = start_of(location)?;
        Ok((uri, line, character))
    }

    fn scan(docs: &[(&str, &str)], needle: &str, sigil: Option<char>, cap: usize) -> Vec<Value> {
        search_document_texts_for_references(
            docs.iter().copied(),
            TextReferenceQuery { needle, sigil, include_declaration: true },
            cap,
        )
    }

    #[test]
    fn hash_key_fat_comma_is_not_a_subroutine_reference() -> Result<(), Box<dyn Error>> {
        let line = "my $self = { name => $args{name} // 'anon' };";
        let fat = line.find("name").ok_or("missing fat-comma key")?;
        let brace = line.rfind("name").ok_or("missing brace key")?;
        assert_eq!(classify_reference_word(line, fat, fat + 4), ReferenceWordKind::HashKey);
        assert_eq!(classify_reference_word(line, brace, brace + 4), ReferenceWordKind::HashKey);
        assert!(!keep_text_reference_match(line, fat, fat + 4, None, true));
        assert!(!keep_text_reference_match(line, brace, brace + 4, None, true));
        Ok(())
    }

    #[test]
    fn arrow_deref_hash_key_is_not_a_subroutine_reference() -> Result<(), Box<dyn Error>> {
        let line = "return $self->{name};";
        let start = line.find("name").ok_or("missing deref key")?;
        assert_eq!(classify_reference_word(line, start, start + 4), ReferenceWordKind::HashKey);
        assert!(!keep_text_reference_match(line, start, start + 4, None, true));
        Ok(())
    }

    #[test]
    fn quoted_hash_keys_are_not_subroutine_references() -> Result<(), Box<dyn Error>> {
        let quoted_fat = r#"Classic->new("name" => 'x');"#;
        let quoted_brace = "$self->{'name'};";
        let fat = quoted_fat.find("name").ok_or("missing quoted fat-comma")?;
        let brace = quoted_brace.find("name").ok_or("missing quoted brace")?;
        assert!(!keep_text_reference_match(quoted_fat, fat, fat + 4, None, true));
        assert!(!keep_text_reference_match(quoted_brace, brace, brace + 4, None, true));
        Ok(())
    }

    #[test]
    fn method_call_and_sub_declaration_stay_subroutine_references() -> Result<(), Box<dyn Error>> {
        let decl = "sub name {";
        let method = r#"return "Classic: " . $self->name;"#;
        let call = "my $n = $obj->name;";
        let qualified = "my $x = Classic::name();";
        let ampersand = "goto &name;";
        let decl_at = decl.find("name").ok_or("missing decl")?;
        let method_at = method.find("name").ok_or("missing method")?;
        let call_at = call.find("name").ok_or("missing call")?;
        let qualified_at = qualified.find("name").ok_or("missing qualified")?;
        let amp_at = ampersand.find("name").ok_or("missing ampersand")?;
        assert_eq!(
            classify_reference_word(decl, decl_at, decl_at + 4),
            ReferenceWordKind::Subroutine
        );
        assert_eq!(
            classify_reference_word(method, method_at, method_at + 4),
            ReferenceWordKind::Subroutine
        );
        assert_eq!(
            classify_reference_word(call, call_at, call_at + 4),
            ReferenceWordKind::Subroutine
        );
        assert_eq!(
            classify_reference_word(qualified, qualified_at, qualified_at + 4),
            ReferenceWordKind::Subroutine
        );
        assert_eq!(
            classify_reference_word(ampersand, amp_at, amp_at + 4),
            ReferenceWordKind::Subroutine
        );
        assert!(keep_text_reference_match(decl, decl_at, decl_at + 4, None, true));
        assert!(keep_text_reference_match(method, method_at, method_at + 4, None, true));
        assert!(keep_text_reference_match(call, call_at, call_at + 4, None, true));
        assert!(keep_text_reference_match(qualified, qualified_at, qualified_at + 4, None, true));
        assert!(keep_text_reference_match(ampersand, amp_at, amp_at + 4, None, true));
        Ok(())
    }

    #[test]
    fn bare_sub_call_without_parens_is_retained() -> Result<(), Box<dyn Error>> {
        let line = "name;";
        let start = 0usize;
        assert_eq!(classify_reference_word(line, start, 4), ReferenceWordKind::Other);
        assert!(
            keep_text_reference_match(line, start, 4, None, true),
            "bareword calls must survive the hash-key filter"
        );
        Ok(())
    }

    #[test]
    fn variable_query_does_not_absorb_same_named_sub() -> Result<(), Box<dyn Error>> {
        let var = "our $AUTOLOAD;";
        let sub = "sub AUTOLOAD {";
        let usage = "my $method = $AUTOLOAD;";
        let var_at = var.find("AUTOLOAD").ok_or("missing var")?;
        let sub_at = sub.find("AUTOLOAD").ok_or("missing sub")?;
        let usage_at = usage.find("AUTOLOAD").ok_or("missing usage")?;
        assert_eq!(classify_reference_word(var, var_at, var_at + 8), ReferenceWordKind::Variable);
        assert_eq!(classify_reference_word(sub, sub_at, sub_at + 8), ReferenceWordKind::Subroutine);
        assert!(keep_text_reference_match(var, var_at, var_at + 8, Some('$'), true));
        assert!(!keep_text_reference_match(sub, sub_at, sub_at + 8, Some('$'), true));
        assert!(keep_text_reference_match(usage, usage_at, usage_at + 8, Some('$'), true));
        assert!(!keep_text_reference_match(var, var_at, var_at + 8, None, true));
        Ok(())
    }

    #[test]
    fn braced_sigil_deref_is_variable_not_hash_key() -> Result<(), Box<dyn Error>> {
        for (line, sigil) in
            [("print ${name};", '$'), ("print @{name};", '@'), ("print %{name};", '%')]
        {
            let start = line.find("name").ok_or("missing braced deref")?;
            assert_eq!(
                classify_reference_word(line, start, start + 4),
                ReferenceWordKind::Variable
            );
            assert!(keep_text_reference_match(line, start, start + 4, Some(sigil), true));
            assert!(!keep_text_reference_match(line, start, start + 4, None, true));
        }
        Ok(())
    }

    #[test]
    fn comment_and_string_occurrences_are_dropped() -> Result<(), Box<dyn Error>> {
        let comment = "my $x = 1; # name is documented";
        let string = r#"print "name";"#;
        let comment_at = comment.find("name").ok_or("missing comment")?;
        let string_at = string.find("name").ok_or("missing string")?;
        assert!(!keep_text_reference_match(comment, comment_at, comment_at + 4, None, true));
        assert!(!keep_text_reference_match(string, string_at, string_at + 4, None, true));
        Ok(())
    }

    #[test]
    fn search_skips_hash_keys_and_keeps_method_calls() -> Result<(), Box<dyn Error>> {
        let classic = concat!(
            "package Classic;\n",
            "sub new {\n",
            "    my $self = { name => $args{name} // 'anon' };\n",
            "}\n",
            "sub name {\n",
            "    return $self->{name};\n",
            "}\n",
            "sub describe {\n",
            "    return $self->name;\n",
            "}\n",
        );
        let consumer = concat!(
            "use Classic;\n",
            "my $obj = Classic->new(name => 'x');\n",
            "my $n = $obj->name;\n",
            "my $q = Classic::name();\n",
        );
        let refs = scan(
            &[("file:///Classic.pm", classic), ("file:///consumer.pl", consumer)],
            "name",
            None,
            20,
        );
        let starts: Vec<_> = refs.iter().map(uri_start_of).collect::<Result<_, _>>()?;
        if starts.iter().any(|(uri, line, _)| uri.ends_with("Classic.pm") && *line == 2) {
            return Err("constructor hash-key line must not appear in sub-name references".into());
        }
        if starts.iter().any(|(uri, line, _)| uri.ends_with("Classic.pm") && *line == 5) {
            return Err("$self->{name} must not appear in sub-name references".into());
        }
        if starts.iter().any(|(uri, line, _)| uri.ends_with("consumer.pl") && *line == 1) {
            return Err("constructor-call hash key must not appear in sub-name references".into());
        }
        if !starts.iter().any(|(uri, line, character)| {
            uri.ends_with("Classic.pm") && *line == 4 && *character == 4
        }) {
            return Err(format!("sub name declaration missing: {starts:?}").into());
        }
        if !starts.iter().any(|(uri, line, character)| {
            uri.ends_with("Classic.pm") && *line == 8 && *character == 18
        }) {
            return Err(format!("$self->name call missing: {starts:?}").into());
        }
        if !starts.iter().any(|(uri, line, character)| {
            uri.ends_with("consumer.pl") && *line == 2 && *character == 14
        }) {
            return Err(format!("$obj->name call missing: {starts:?}").into());
        }
        if !starts.iter().any(|(uri, line, character)| {
            uri.ends_with("consumer.pl") && *line == 3 && *character == 17
        }) {
            return Err(format!("Classic::name qualified call missing: {starts:?}").into());
        }
        Ok(())
    }

    #[test]
    fn search_document_texts_for_references_keeps_word_boundaries() -> Result<(), Box<dyn Error>> {
        let docs = [("file:///refs.pl", "my $var = 1;\nmy $variant = $var;\n")];
        let refs = scan(&docs, "var", Some('$'), 10);
        let starts: Vec<_> = refs.iter().map(start_of).collect::<Result<_, _>>()?;
        if starts != vec![(0, 3), (1, 14)] {
            return Err(
                format!("expected $var at (0,3) and (1,14) without $variant: {starts:?}").into()
            );
        }
        Ok(())
    }

    #[test]
    fn search_document_texts_for_references_reports_utf16_columns() -> Result<(), Box<dyn Error>> {
        let docs = [("file:///refs.pl", "my $heart = \"♥\"; $heart\n")];
        let refs = scan(&docs, "heart", Some('$'), 10);
        if refs.len() != 2 {
            return Err(format!("expected 2 references, got {}", refs.len()).into());
        }
        let starts: Vec<_> = refs.iter().map(start_of).collect::<Result<_, _>>()?;
        if starts != vec![(0, 3), (0, 17)] {
            return Err(format!("unexpected UTF-16 starts: {starts:?}").into());
        }
        Ok(())
    }

    #[test]
    fn empty_needle_or_zero_cap_returns_empty() -> Result<(), Box<dyn Error>> {
        let docs = [("file:///refs.pl", "$var\n")];
        assert!(scan(&docs, "", Some('$'), 10).is_empty());
        assert!(scan(&docs, "var", Some('$'), 0).is_empty());
        Ok(())
    }

    #[test]
    fn cap_stops_scan_after_kept_hits() -> Result<(), Box<dyn Error>> {
        let docs = [("file:///refs.pl", "$var $var $var\n")];
        assert_eq!(scan(&docs, "var", Some('$'), 2).len(), 2);
        Ok(())
    }

    #[test]
    fn cap_counts_kept_hits_not_hash_key_false_positives() -> Result<(), Box<dyn Error>> {
        let text = "my $h = { name => 1 };\nsub name {\n    name();\n    name();\n}\n";
        let refs = scan(&[("file:///cap.pl", text)], "name", None, 2);
        if refs.len() != 2 {
            return Err(format!("cap must apply after filtering, got {}", refs.len()).into());
        }
        let starts: Vec<_> = refs.iter().map(start_of).collect::<Result<_, _>>()?;
        if starts.iter().any(|&(line, _)| line == 0) {
            return Err("hash-key false positives must not consume the cap".into());
        }
        Ok(())
    }

    #[test]
    fn dedupe_collapses_identical_ranges_across_json_key_order() -> Result<(), Box<dyn Error>> {
        let mut locations = vec![
            json!({
                "uri": "file:///a.pl",
                "range": {
                    "start": {"line": 1, "character": 4},
                    "end": {"line": 1, "character": 8},
                },
            }),
            json!({
                "range": {
                    "end": {"line": 1, "character": 8},
                    "start": {"line": 1, "character": 4},
                },
                "uri": "file:///a.pl",
            }),
            json!({
                "uri": "file:///a.pl",
                "range": {
                    "start": {"line": 1, "character": 0},
                    "end": {"line": 3, "character": 1},
                },
            }),
        ];
        let out = finalize_reference_locations(locations.split_off(0), 10);
        if out.len() != 2 {
            return Err(format!("expected 2 unique ranges, got {}", out.len()).into());
        }
        Ok(())
    }

    #[test]
    fn should_skip_text_reference_match_omits_variable_declarations_when_requested()
    -> Result<(), Box<dyn Error>> {
        let line = "my $total = 1;";
        let match_start = line.find("total").ok_or("missing total match")?;
        assert!(
            should_skip_text_reference_match(line, match_start, Some('$'), false),
            "includeDeclaration=false must omit lexical declaration matches"
        );
        assert!(
            !should_skip_text_reference_match(line, match_start, Some('$'), true),
            "includeDeclaration=true must keep declaration matches"
        );
        assert!(
            !should_skip_text_reference_match(line, match_start, None, false),
            "subroutine/bareword text matches are not variable declarations"
        );
        Ok(())
    }

    #[test]
    fn should_skip_text_reference_match_keeps_initializer_rhs_usages() -> Result<(), Box<dyn Error>>
    {
        let line = "my $other = $total;";
        let match_start = line.find("total").ok_or("missing total match")?;
        assert!(
            !should_skip_text_reference_match(line, match_start, Some('$'), false),
            "RHS usages inside a declaration statement are still references"
        );
        Ok(())
    }

    #[test]
    fn should_skip_text_reference_match_omits_variable_list_declaration_targets()
    -> Result<(), Box<dyn Error>> {
        let line = "for my ($first, $total) {";
        let match_start = line.find("total").ok_or("missing total match")?;
        assert!(
            should_skip_text_reference_match(line, match_start, Some('$'), false),
            "declaration targets inside variable lists must be omitted"
        );
        Ok(())
    }

    #[test]
    fn qualified_final_component_survives_colon_name_bytes() -> Result<(), Box<dyn Error>> {
        let docs = [("file:///q.pl", "my $x = Classic::name();\nmy $y = namesake;\n")];
        let refs = scan(&docs, "name", None, 10);
        let starts: Vec<_> = refs.iter().map(start_of).collect::<Result<_, _>>()?;
        if !starts.contains(&(0, 17)) {
            return Err(format!("Classic::name must be a text-search hit: {starts:?}").into());
        }
        if starts.iter().any(|&(line, _)| line == 1) {
            return Err("namesake must not match needle name".into());
        }
        Ok(())
    }

    #[test]
    fn block_brace_bareword_is_not_a_hash_key() -> Result<(), Box<dyn Error>> {
        let block = "if ($ok) { name; }";
        let map = "map { name } @xs;";
        let hash = "$args{name}";
        let arrow = "$self->{name}";
        let block_at = block.find("name").ok_or("missing block call")?;
        let map_at = map.find("name").ok_or("missing map call")?;
        let hash_at = hash.find("name").ok_or("missing hash key")?;
        let arrow_at = arrow.find("name").ok_or("missing arrow key")?;
        assert_ne!(
            classify_reference_word(block, block_at, block_at + 4),
            ReferenceWordKind::HashKey
        );
        assert_ne!(classify_reference_word(map, map_at, map_at + 4), ReferenceWordKind::HashKey);
        assert!(keep_text_reference_match(block, block_at, block_at + 4, None, true));
        assert!(keep_text_reference_match(map, map_at, map_at + 4, None, true));
        assert_eq!(classify_reference_word(hash, hash_at, hash_at + 4), ReferenceWordKind::HashKey);
        assert_eq!(
            classify_reference_word(arrow, arrow_at, arrow_at + 4),
            ReferenceWordKind::HashKey
        );
        assert!(!keep_text_reference_match(hash, hash_at, hash_at + 4, None, true));
        assert!(!keep_text_reference_match(arrow, arrow_at, arrow_at + 4, None, true));
        Ok(())
    }

    #[test]
    fn interpolated_variable_in_double_quotes_is_kept() -> Result<(), Box<dyn Error>> {
        let interpolated = r#"print "hello $name";"#;
        let literal = r#"print "name";"#;
        let single = "print 'hello $name';";
        let interp_at = interpolated.find("name").ok_or("missing interpolated")?;
        let literal_at = literal.find("name").ok_or("missing literal")?;
        let single_at = single.find("name").ok_or("missing single")?;
        assert!(keep_text_reference_match(interpolated, interp_at, interp_at + 4, Some('$'), true));
        assert!(!keep_text_reference_match(interpolated, interp_at, interp_at + 4, None, true));
        assert!(!keep_text_reference_match(literal, literal_at, literal_at + 4, Some('$'), true));
        assert!(!keep_text_reference_match(literal, literal_at, literal_at + 4, None, true));
        assert!(!keep_text_reference_match(single, single_at, single_at + 4, Some('$'), true));
        Ok(())
    }

    #[test]
    fn variable_match_range_includes_leading_sigil() -> Result<(), Box<dyn Error>> {
        let docs = [("file:///v.pl", "my $total = $total;\n")];
        let refs = scan(&docs, "total", Some('$'), 10);
        let starts: Vec<_> = refs.iter().map(start_of).collect::<Result<_, _>>()?;
        if starts != vec![(0, 3), (0, 12)] {
            return Err(format!("variable ranges must start at '$': {starts:?}").into());
        }
        Ok(())
    }

    #[test]
    fn search_keeps_block_calls_and_drops_hash_subscripts() -> Result<(), Box<dyn Error>> {
        let text = concat!("if ($ok) { name; }\n", "$h{name} = 1;\n", "map { name } @xs;\n",);
        let refs = scan(&[("file:///b.pl", text)], "name", None, 10);
        let starts: Vec<_> = refs.iter().map(start_of).collect::<Result<_, _>>()?;
        if starts.iter().any(|&(line, _)| line == 1) {
            return Err(format!("hash subscript leaked: {starts:?}").into());
        }
        if !starts.contains(&(0, 11)) {
            return Err(format!("if-block bare call missing: {starts:?}").into());
        }
        if !starts.iter().any(|&(line, _)| line == 2) {
            return Err(format!("map-block bare call missing: {starts:?}").into());
        }
        Ok(())
    }
}
