//! Distribution-metadata facts.
//!
//! Reads a Perl distribution's metadata files — `META.json` (CPAN Meta Spec
//! v2, via `serde_json`), `cpanfile` (a Perl DSL, via a dependency-light
//! statement scan) — into typed facts: name, version, abstract, licenses, and
//! prerequisites. The extraction mirrors the proven, std+serde_json approach in
//! `perl-lsp-rs-core::config::metadata_dependencies` (which the substrate may
//! not depend on, being above the leaf line), ported here so dist facts sit in
//! the substrate for Kwalitee and other consumers (PLSP-ADR-0006 PR 7).

use serde::{Deserialize, Serialize};

use crate::id::FileId;

/// Which metadata file a dist fact came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistMetadataSource {
    /// `META.json` (CPAN Meta Spec v2).
    MetaJson,
    /// A `cpanfile` (Perl DSL).
    Cpanfile,
}

/// One declared prerequisite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prereq {
    /// The required module.
    pub module: String,
    /// The version requirement, if any (`"0"` means "any").
    pub version: Option<String>,
    /// Phase: `configure` / `build` / `test` / `runtime` / `develop`.
    pub phase: String,
    /// Relation: `requires` / `recommends` / `suggests` / `conflicts`.
    pub relation: String,
}

/// Distribution-metadata facts extracted from one metadata file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistMetadataFacts {
    /// The metadata file these facts came from.
    pub file_id: FileId,
    /// Which metadata format.
    pub source: DistMetadataSource,
    /// Distribution name (e.g. `Foo-Bar`), if declared.
    pub name: Option<String>,
    /// Distribution version, if declared.
    pub version: Option<String>,
    /// The `abstract` one-line description, if declared.
    pub summary: Option<String>,
    /// Declared licenses (SPDX-ish tokens like `perl_5`).
    pub licenses: Vec<String>,
    /// Declared prerequisites.
    pub prereqs: Vec<Prereq>,
}

/// The prereq relations recognized in cpanfile / META.json.
const RELATIONS: &[&str] = &["requires", "recommends", "suggests", "conflicts"];
/// Canonical prerequisite phases recognized in cpanfile `on` blocks.
const CPANFILE_PHASES: &[&str] = &["configure", "build", "test", "runtime", "develop"];
/// META 1.x phase-specific top-level prerequisite keys → canonical phase.
const META_V1_PHASED_REQUIRES: &[(&str, &str)] =
    &[("configure_requires", "configure"), ("build_requires", "build")];
/// cpanfile statement keywords → (relation, phase).
///
/// Order matters: the longest keyword must come first, because
/// `starts_with_cpanfile_keyword` takes the first prefix match and
/// `configure_requires` shares a suffix with `requires`.
const CPANFILE_KEYWORDS: &[(&str, &str, &str)] = &[
    ("configure_requires", "requires", "configure"),
    ("build_requires", "requires", "build"),
    ("test_requires", "requires", "test"),
    ("author_requires", "requires", "develop"),
    ("requires", "requires", "runtime"),
    ("recommends", "recommends", "runtime"),
    ("suggests", "suggests", "runtime"),
    ("conflicts", "conflicts", "runtime"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum CpanfileBlock {
    Phase(String),
    Unsupported,
}

/// Parse a `META.json` (CPAN Meta Spec v2, with a v1.4 flat fallback).
///
/// Returns `None` when the content is not valid JSON.
#[must_use]
pub fn parse_meta_json(file_id: FileId, content: &str) -> Option<DistMetadataFacts> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;

    let name = value.get("name").and_then(json_string);
    let version = value.get("version").and_then(json_scalar_string);
    let summary = value.get("abstract").and_then(json_string);
    let licenses = match value.get("license") {
        // v2: an array of license strings.
        Some(serde_json::Value::Array(items)) => {
            items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
        }
        // v1.4 / META.yml: a single string.
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };

    let mut prereqs = Vec::new();
    let mut recovered_v2_entries = false;
    // v2: prereqs[phase][relation] = { module: version }.
    if let Some(serde_json::Value::Object(phases)) = value.get("prereqs") {
        for (phase, relations) in phases {
            let serde_json::Value::Object(relations) = relations else { continue };
            for (relation, modules) in relations {
                if !RELATIONS.contains(&relation.as_str()) {
                    continue;
                }
                recovered_v2_entries |= collect_modules(modules, phase, relation, &mut prereqs);
            }
        }
    }
    // v1.4 flat fallback: phase-specific *_requires plus runtime relations.
    if !recovered_v2_entries {
        for &(key, phase) in META_V1_PHASED_REQUIRES {
            if let Some(modules) = value.get(key) {
                let _ = collect_modules(modules, phase, "requires", &mut prereqs);
            }
        }
        for relation in RELATIONS {
            if let Some(modules) = value.get(relation) {
                let _ = collect_modules(modules, "runtime", relation, &mut prereqs);
            }
        }
    }
    prereqs.sort_by(|a, b| {
        (&a.phase, &a.relation, &a.module).cmp(&(&b.phase, &b.relation, &b.module))
    });

    Some(DistMetadataFacts {
        file_id,
        source: DistMetadataSource::MetaJson,
        name,
        version,
        summary,
        licenses,
        prereqs,
    })
}

/// Perl statement modifiers that make a declaration conditional.
const CPANFILE_STATEMENT_MODIFIERS: &[&str] =
    &["if", "unless", "while", "until", "for", "foreach", "when"];
/// Quote-like operators this scanner recognizes, with how many delimited parts
/// each takes. Order matters: the longest word must come first, so `qq`, `qw`,
/// `qr`, and `tr` are not read as `q` or `t`.
const CPANFILE_QUOTE_LIKE: &[(&str, usize)] =
    &[("qq", 1), ("qw", 1), ("qr", 1), ("tr", 2), ("q", 1), ("m", 1), ("s", 2), ("y", 2)];
/// Delimiters accepted after a quote-like operator. Deliberately narrow: a
/// wider set would read ordinary syntax such as `s => 1` as a substitution.
const CPANFILE_QUOTE_LIKE_DELIMITERS: &[char] =
    &['/', '{', '(', '[', '<', '|', '!', '~', '^', '\'', '"'];

/// What one character of cpanfile text is, once comments and string literals
/// are recognized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CpanfileChar {
    /// Code outside any comment or string literal.
    Code,
    /// A string literal, including its delimiters: statement text that cannot
    /// open a block, close one, or end a statement.
    Literal,
    /// A `#` comment, which is not statement text at all.
    Comment,
}

/// One lexical pass over cpanfile text.
///
/// This is the single place that knows where Perl comments and string literals
/// begin and end. Block tracking, statement splitting, module and version
/// extraction, and statement-modifier detection all read this classification
/// rather than each re-deriving quote state, which is how an unbalanced brace
/// inside a quote-like literal used to swallow every later declaration.
///
/// Known limits, all fail-closed for the block scan: a bare `/.../` regex is
/// not separable from division without a real parser, and heredocs, POD, and
/// `__END__`/`__DATA__` sections are treated as ordinary code.
struct CpanfileLex {
    /// Per-character classification, parallel to the source characters.
    class: Vec<CpanfileChar>,
    /// Content ranges of plain single- and double-quoted literals, in source
    /// order.
    plain: Vec<(usize, usize)>,
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// The delimiter that closes `open`.
fn closing_delimiter(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        other => other,
    }
}

/// Consume the delimited section opening at `chars[start]`, returning the index
/// just past its closing delimiter, or `None` when it is never closed.
///
/// Bracketing delimiters nest; every other delimiter closes on its first
/// unescaped repeat.
fn skip_delimited(chars: &[char], start: usize) -> Option<usize> {
    let open = *chars.get(start)?;
    let close = closing_delimiter(open);
    let nests = close != open;
    let mut depth = 1_usize;
    let mut index = start + 1;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            ch if nests && ch == open => depth += 1,
            ch if ch == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// If the identifier `word`, ending at `word_end`, is a quote-like operator
/// whose delimiter follows, return the index just past the whole literal.
fn quote_like_literal_end(chars: &[char], word: &str, word_end: usize) -> Option<usize> {
    let &(_, parts) = CPANFILE_QUOTE_LIKE.iter().find(|(operator, _)| *operator == word)?;
    let mut cursor = word_end;
    while chars.get(cursor).is_some_and(|ch| ch.is_whitespace()) {
        cursor += 1;
    }
    let open = *chars.get(cursor)?;
    if !CPANFILE_QUOTE_LIKE_DELIMITERS.contains(&open) {
        return None;
    }
    let mut end = skip_delimited(chars, cursor)?;
    if parts == 2 {
        end = if closing_delimiter(open) == open {
            // `s/a/b/` shares its middle delimiter between both parts.
            skip_delimited(chars, end - 1)?
        } else {
            // `s{a}{b}` opens a fresh pair, possibly after whitespace.
            let mut second = end;
            while chars.get(second).is_some_and(|ch| ch.is_whitespace()) {
                second += 1;
            }
            skip_delimited(chars, second)?
        };
    }
    // Trailing modifiers, as in `qr/.../i`.
    while chars.get(end).is_some_and(char::is_ascii_alphabetic) {
        end += 1;
    }
    Some(end)
}

impl CpanfileLex {
    /// Classify every character of `chars`.
    fn scan(chars: &[char]) -> Self {
        let mut class = vec![CpanfileChar::Code; chars.len()];
        let mut plain = Vec::new();
        let mut index = 0;
        while index < chars.len() {
            let ch = chars[index];
            if ch == '#' {
                while index < chars.len() && chars[index] != '\n' {
                    class[index] = CpanfileChar::Comment;
                    index += 1;
                }
                continue;
            }
            if ch == '\'' || ch == '"' {
                // An unterminated literal runs to the end of the file, and has
                // no closing delimiter to exclude from its content.
                let (content_end, literal_end) = skip_delimited(chars, index)
                    .map_or((chars.len(), chars.len()), |end| (end - 1, end));
                plain.push((index + 1, content_end));
                for slot in &mut class[index..literal_end] {
                    *slot = CpanfileChar::Literal;
                }
                index = literal_end;
                continue;
            }
            // A sigil binds its variable name, so `$q{...}` subscripts a hash
            // rather than opening a `q{...}` literal.
            if matches!(ch, '$' | '@' | '%' | '&') {
                index += 1;
                while chars.get(index).is_some_and(|&next| is_identifier_char(next)) {
                    index += 1;
                }
                continue;
            }
            if ch.is_alphabetic() || ch == '_' {
                let mut word_end = index;
                while chars.get(word_end).is_some_and(|&next| is_identifier_char(next)) {
                    word_end += 1;
                }
                let word: String = chars[index..word_end].iter().collect();
                if let Some(end) = quote_like_literal_end(chars, &word, word_end) {
                    for slot in &mut class[index..end] {
                        *slot = CpanfileChar::Literal;
                    }
                    index = end;
                } else {
                    index = word_end;
                }
                continue;
            }
            index += 1;
        }
        Self { class, plain }
    }

    /// Whether `index` is code outside any comment or string literal.
    fn is_code(&self, index: usize) -> bool {
        self.class.get(index) == Some(&CpanfileChar::Code)
    }

    /// Whether `index` is statement text: code or a string literal, but not a
    /// comment.
    fn is_statement_text(&self, index: usize) -> bool {
        self.class.get(index) != Some(&CpanfileChar::Comment)
    }
}

/// Parse a `cpanfile` for its unconditional prerequisites (heuristic statement
/// scan — no Perl parser). Name/version/abstract are not declared in a cpanfile.
///
/// Handles both the flat form (`requires`, `test_requires`, …) and recognized
/// block forms (`on 'test' => sub { requires '...' }`). Other blocks are
/// deliberately ignored because this fact type cannot retain their predicates.
#[must_use]
pub fn parse_cpanfile(file_id: FileId, content: &str) -> DistMetadataFacts {
    let chars: Vec<char> = content.chars().collect();
    let lex = CpanfileLex::scan(&chars);
    let mut prereqs = Vec::new();
    let mut block_stack: Vec<CpanfileBlock> = Vec::new();
    let mut buf = String::new();
    // How many hash/array subscripts are open in the current statement. Their
    // braces are expression syntax, not blocks, so they must not change scope.
    let mut subscript_depth = 0_usize;

    for (index, &ch) in chars.iter().enumerate() {
        // A string literal stays statement text; a comment is not text at all.
        if !lex.is_code(index) {
            if lex.is_statement_text(index) {
                buf.push(ch);
            }
            continue;
        }
        if subscript_depth > 0 {
            match ch {
                '{' | '[' => subscript_depth += 1,
                '}' | ']' => subscript_depth -= 1,
                _ => {}
            }
            buf.push(ch);
            continue;
        }
        match ch {
            ';' => {
                flush_cpanfile_statement(&buf, &block_stack, &mut prereqs);
                buf.clear();
            }
            '{' if opens_hash_subscript(&buf) => {
                subscript_depth = 1;
                buf.push(ch);
            }
            '{' => {
                let block = if matches!(block_stack.last(), Some(CpanfileBlock::Unsupported)) {
                    CpanfileBlock::Unsupported
                } else if let Some(phase) = parse_on_phase(&buf) {
                    CpanfileBlock::Phase(phase)
                } else {
                    CpanfileBlock::Unsupported
                };
                block_stack.push(block);
                buf.clear();
            }
            '}' => {
                flush_cpanfile_statement(&buf, &block_stack, &mut prereqs);
                buf.clear();
                block_stack.pop();
            }
            _ => buf.push(ch),
        }
    }
    flush_cpanfile_statement(&buf, &block_stack, &mut prereqs);

    prereqs.sort_by(|a, b| {
        (&a.phase, &a.relation, &a.module).cmp(&(&b.phase, &b.relation, &b.module))
    });
    DistMetadataFacts {
        file_id,
        source: DistMetadataSource::Cpanfile,
        name: None,
        version: None,
        summary: None,
        licenses: Vec::new(),
        prereqs,
    }
}

enum CpanfileScope<'a> {
    TopLevel,
    Phase(&'a str),
    Unsupported,
}

fn active_cpanfile_scope(block_stack: &[CpanfileBlock]) -> CpanfileScope<'_> {
    match block_stack.last() {
        None => CpanfileScope::TopLevel,
        Some(CpanfileBlock::Phase(phase)) => CpanfileScope::Phase(phase.as_str()),
        Some(CpanfileBlock::Unsupported) => CpanfileScope::Unsupported,
    }
}

/// Publish whatever prereq the buffered statement declares, under the innermost
/// block's scope. A statement inside an unsupported block publishes nothing.
fn flush_cpanfile_statement(buf: &str, block_stack: &[CpanfileBlock], out: &mut Vec<Prereq>) {
    match active_cpanfile_scope(block_stack) {
        CpanfileScope::TopLevel => handle_cpanfile_statement(buf, None, out),
        CpanfileScope::Phase(phase) => handle_cpanfile_statement(buf, Some(phase), out),
        CpanfileScope::Unsupported => {}
    }
}

/// Whether a `{` directly after `buf` subscripts or dereferences a variable
/// rather than opening a block.
///
/// `$versions{if}` and `$h->{unless}` are expressions whose braces must not
/// change block scope. `sub {` and `if (...) {` stay blocks and keep their
/// fail-closed treatment, so a conditional declaration is never promoted.
///
/// Perl allows whitespace before a subscript, so `$versions {base}` is the same
/// expression as `$versions{base}`. It is the sigil requirement below, not
/// adjacency, that keeps `sub {` a block: `sub` ends in identifier characters
/// but carries no sigil. Trimming here also accepts a block whose brace opens
/// on the following line.
fn opens_hash_subscript(buf: &str) -> bool {
    let chars: Vec<char> = buf.trim_end().chars().collect();
    match chars.last() {
        // A chained subscript: `$h{a}{b}`, `$h->[0]{b}`.
        Some('}' | ']') => true,
        // A dereference block: `${name}`, `@{$ref}`.
        Some('$' | '@' | '%') => true,
        // An arrow dereference: `$h->{k}`.
        Some('>') => chars.len() >= 2 && chars[chars.len() - 2] == '-',
        // A variable name, which must carry a sigil. Without the sigil check
        // `sub {` would read as a subscript and stop suppressing conditionals.
        Some(&last) if is_identifier_char(last) => {
            let mut start = chars.len();
            while start > 0 && is_identifier_char(chars[start - 1]) {
                start -= 1;
            }
            start > 0 && matches!(chars[start - 1], '$' | '@' | '%')
        }
        _ => false,
    }
}

/// Recognize a prereq statement and push it, resolving its phase.
///
/// A prefixed keyword (`configure_requires`/`build_requires`/`test_requires`)
/// carries its own phase; a plain `requires`/`recommends`/`suggests` uses the
/// enclosing `on 'phase'` block's phase, defaulting to `runtime`.
fn handle_cpanfile_statement(buf: &str, block_phase: Option<&str>, out: &mut Vec<Prereq>) {
    let statement = buf.trim();
    // The keyword boundary check prevents prefix collisions.
    let Some((keyword, relation, kw_phase)) =
        CPANFILE_KEYWORDS.iter().find(|(kw, _, _)| starts_with_cpanfile_keyword(statement, kw))
    else {
        return;
    };
    // A postfix modifier is a predicate too, even without a surrounding block.
    // Do not expose its quoted condition as a version or its module as a fact.
    if has_cpanfile_statement_modifier(statement) {
        return;
    }
    let phase = if *kw_phase == "runtime" { block_phase.unwrap_or("runtime") } else { kw_phase };
    let Some(arguments) = statement.strip_prefix(keyword) else { return };
    // Only a literally named module becomes a fact; see `cpanfile_call_literals`.
    let Some((module, version)) = cpanfile_call_literals(arguments) else { return };
    out.push(Prereq {
        module,
        version,
        phase: phase.to_string(),
        relation: (*relation).to_string(),
    });
}

/// Whether the statement carries a postfix statement modifier, which makes the
/// declaration conditional.
///
/// A modifier word only counts at the statement's own bracket depth. That keeps
/// an expression such as `$versions{if}` or `qw(unless)` from reading as a
/// condition, while `requires 'Win32' if $^O eq 'MSWin32';` is still rejected.
fn has_cpanfile_statement_modifier(statement: &str) -> bool {
    let chars: Vec<char> = statement.chars().collect();
    let lex = CpanfileLex::scan(&chars);
    let mut depth = 0_usize;
    let mut word_start = None;
    for index in 0..=chars.len() {
        if lex.is_code(index) && is_identifier_char(chars[index]) {
            let _ = word_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = word_start.take() {
            let word: String = chars[start..index].iter().collect();
            if depth == 0 && CPANFILE_STATEMENT_MODIFIERS.contains(&word.as_str()) {
                return true;
            }
        }
        if lex.is_code(index) {
            match chars[index] {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

fn starts_with_cpanfile_keyword(statement: &str, keyword: &str) -> bool {
    let Some(rest) = statement.strip_prefix(keyword) else {
        return false;
    };
    rest.chars().next().is_none_or(|ch| ch.is_whitespace() || matches!(ch, '(' | '\'' | '"'))
}

/// Extract a canonical phase from an `on 'phase' => sub` block header.
fn parse_on_phase(buf: &str) -> Option<String> {
    let rest = buf.trim().strip_prefix("on")?;
    // `on` must be followed by whitespace or a quote, not be part of a longer word.
    if !rest.starts_with(|c: char| c.is_whitespace() || matches!(c, '(' | '\'' | '"')) {
        return None;
    }
    // Prefer a quoted phase (`on 'test'`); fall back to a bareword (`on test`).
    // A quoted candidate takes precedence, so a non-canonical first quoted string does not consult the bareword fallback.
    // `on(develop => sub {...})` is a bareword phase behind a parenthesis; the
    // fat comma quotes it, so it names the same phase as `on 'develop'`.
    let bareword = rest.trim_start();
    let bareword = bareword.strip_prefix('(').unwrap_or(bareword).trim_start();
    let phase = quoted_strings(buf).into_iter().next().or_else(|| {
        bareword.split(|ch: char| !is_identifier_char(ch)).next().map(str::to_string)
    })?;
    CPANFILE_PHASES.contains(&phase.as_str()).then_some(phase)
}

/// Collect `{ module: version }` object entries into prereqs.
fn collect_modules(
    modules: &serde_json::Value,
    phase: &str,
    relation: &str,
    out: &mut Vec<Prereq>,
) -> bool {
    let serde_json::Value::Object(map) = modules else { return false };
    let mut recovered = false;
    for (module, version) in map {
        let Some(version) = json_scalar_string(version) else { continue };
        out.push(Prereq {
            module: module.clone(),
            version: Some(version),
            phase: phase.to_string(),
            relation: relation.to_string(),
        });
        recovered = true;
    }
    recovered
}

/// A JSON value as a string, only if it *is* a string.
fn json_string(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

/// A JSON scalar (string or number) coerced to a string.
///
/// Note: a bare JSON *number* version like `1.20` round-trips through `f64` and
/// serializes back as `1.2` (trailing zeros lost). CPAN Meta Spec recommends
/// versions be strings for exactly this reason; string versions are preserved
/// verbatim.
fn json_scalar_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Strip one wrapping parenthesis pair from a prereq call's arguments.
fn strip_call_parentheses(arguments: &str) -> String {
    let trimmed = arguments.trim().trim_end_matches(';').trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.first() == Some(&'(') && skip_delimited(&chars, 0) == Some(chars.len()) {
        return chars[1..chars.len() - 1].iter().collect();
    }
    trimmed.to_string()
}

/// Split a prereq call's arguments on `,` and `=>`, ignoring separators inside
/// literals or nested brackets.
fn split_cpanfile_arguments(arguments: &str) -> Vec<String> {
    let chars: Vec<char> = arguments.chars().collect();
    let lex = CpanfileLex::scan(&chars);
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0_usize;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if !lex.is_code(index) {
            if lex.is_statement_text(index) {
                current.push(ch);
            }
            index += 1;
            continue;
        }
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            '=' if depth == 0 && chars.get(index + 1) == Some(&'>') => {
                parts.push(std::mem::take(&mut current));
                index += 1;
            }
            _ => current.push(ch),
        }
        index += 1;
    }
    parts.push(current);
    parts
}

/// The value of `slice` when it is exactly one plain quoted literal, and
/// nothing else.
///
/// `'Foo'` qualifies; `'Foo' . 'Bar'`, `$versions{base}`, and
/// `$enabled ? 'Foo' : 'Bar'` do not.
fn sole_plain_literal(slice: &str) -> Option<String> {
    let chars: Vec<char> = slice.trim().chars().collect();
    if !matches!(chars.first(), Some('\'' | '"')) {
        return None;
    }
    let end = skip_delimited(&chars, 0)?;
    if end != chars.len() {
        return None;
    }
    Some(chars[1..end - 1].iter().collect())
}

/// The module and version a prereq call declares, when its argument shape is
/// one this scanner can read without evaluating Perl.
///
/// The module argument must be a single plain quoted literal. A computed
/// module, as in `requires($enabled ? 'Foo' : 'Bar')` or
/// `requires helper('Foo')`, names its dependency only at run time; publishing
/// the first literal found inside it would claim a dependency the cpanfile
/// never unconditionally declares, and would invent a version out of the
/// remaining literals. A computed *version* is weaker — the module is still
/// named literally — so it yields `None` instead of suppressing the fact.
fn cpanfile_call_literals(arguments: &str) -> Option<(String, Option<String>)> {
    let parts = split_cpanfile_arguments(&strip_call_parentheses(arguments));
    let module = sole_plain_literal(parts.first()?)?;
    let version = parts.get(1).and_then(|part| sole_plain_literal(part));
    Some((module, version))
}

/// Extract plain single- or double-quoted string literals from a statement.
///
/// Quote-like operator bodies are deliberately excluded: this scanner reads
/// module and version text only from literals it can read without evaluating
/// Perl.
fn quoted_strings(statement: &str) -> Vec<String> {
    let chars: Vec<char> = statement.chars().collect();
    CpanfileLex::scan(&chars)
        .plain
        .into_iter()
        .map(|(start, end)| chars[start..end].iter().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::*;
    use crate::id::Digest;

    fn fid() -> FileId {
        FileId::new("META.json", &Digest::of("x"))
    }

    #[test]
    fn parses_meta_json_v2() {
        let content = r#"{
            "name": "Foo-Bar",
            "version": "1.23",
            "abstract": "does foo to bar",
            "license": ["perl_5"],
            "prereqs": {
                "runtime": { "requires": { "strict": "0", "Moo": "2.0" } },
                "test":    { "requires": { "Test::More": "0.98" } }
            }
        }"#;
        let facts = parse_meta_json(fid(), content).unwrap();
        assert_eq!(facts.name.as_deref(), Some("Foo-Bar"));
        assert_eq!(facts.version.as_deref(), Some("1.23"));
        assert_eq!(facts.summary.as_deref(), Some("does foo to bar"));
        assert_eq!(facts.licenses, vec!["perl_5"]);
        assert!(facts.prereqs.iter().any(|p| p.module == "Moo" && p.phase == "runtime"));
        assert!(facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"));
    }

    #[test]
    fn coerces_numeric_version() {
        let facts = parse_meta_json(fid(), r#"{"name":"X","version":1.5}"#).unwrap();
        assert_eq!(facts.version.as_deref(), Some("1.5"), "numeric version coerced to string");
    }

    #[test]
    fn v1_4_flat_prereqs_and_string_license() {
        let content = r#"{"name":"X","license":"perl","requires":{"Carp":"0"}}"#;
        let facts = parse_meta_json(fid(), content).unwrap();
        assert_eq!(facts.licenses, vec!["perl"], "v1.4 single-string license");
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Carp" && p.relation == "requires"),
            "flat top-level requires read as fallback"
        );
    }

    #[test]
    fn v1_4_phase_specific_prereqs_are_retained() {
        let content = r#"{
            "configure_requires": {"ExtUtils::MakeMaker": "6.64"},
            "build_requires": {"Test::More": "0.88"},
            "requires": {"Carp": "0"}
        }"#;
        let facts = parse_meta_json(fid(), content).unwrap();
        let mapped = facts
            .prereqs
            .iter()
            .map(|p| {
                (p.module.as_str(), p.version.as_deref(), p.phase.as_str(), p.relation.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            mapped,
            vec![
                ("Test::More", Some("0.88"), "build", "requires"),
                ("ExtUtils::MakeMaker", Some("6.64"), "configure", "requires"),
                ("Carp", Some("0"), "runtime", "requires"),
            ],
            "META 1.x prerequisite keys retain phase, relation, version, and deterministic order"
        );
    }

    #[test]
    fn v2_prereqs_take_precedence_over_flat_v1_fields() {
        let facts = parse_meta_json(
            fid(),
            r#"{
                "prereqs": {"runtime": {"requires": {"V2::Only": "1"}}},
                "configure_requires": {"V1::Only": "1"},
                "requires": {"V1::Runtime": "1"}
            }"#,
        )
        .unwrap();

        assert_eq!(
            facts.prereqs,
            vec![Prereq {
                module: "V2::Only".to_string(),
                version: Some("1".to_string()),
                phase: "runtime".to_string(),
                relation: "requires".to_string(),
            }]
        );
    }

    #[test]
    fn empty_or_malformed_v2_prereqs_fall_back_to_flat_v1_fields() {
        for v2 in [r#"{}"#, r#"{"runtime": []}"#, r#"{"runtime": "bad"}"#] {
            let content =
                format!(r#"{{"prereqs": {v2}, "configure_requires": {{"V1::Only": "1"}}}}"#);
            let facts = parse_meta_json(fid(), &content).unwrap();
            assert_eq!(facts.prereqs.len(), 1, "v2={v2}");
            assert_eq!(facts.prereqs[0].module, "V1::Only", "v2={v2}");
            assert_eq!(facts.prereqs[0].phase, "configure", "v2={v2}");
        }
    }

    #[test]
    fn malformed_phase_maps_do_not_fabricate_prereqs_or_panic() {
        let facts = parse_meta_json(
            fid(),
            r#"{
                "prereqs": {
                    "runtime": [],
                    "test": "not a relation map",
                    "develop": null,
                    "build": {"requires": ["not", "a", "module map"]}
                },
                "configure_requires": [],
                "build_requires": "not a module map",
                "requires": null
            }"#,
        )
        .unwrap();

        assert!(facts.prereqs.is_empty());
    }

    #[test]
    fn malformed_v2_relations_are_ignored_without_fabricated_facts() {
        let facts = parse_meta_json(
            fid(),
            r#"{
                "prereqs": {
                    "runtime": {
                        "unknown_relation": {"Fabricated::Fact": "1"},
                        "requires": null,
                        "recommends": {"Real::Fact": "2"}
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(facts.prereqs.len(), 1);
        assert_eq!(facts.prereqs[0].module, "Real::Fact");
        assert!(!facts.prereqs.iter().any(|p| p.module == "Fabricated::Fact"));
    }

    #[test]
    fn invalid_json_returns_none() {
        assert!(parse_meta_json(fid(), "{not json").is_none());
    }

    #[test]
    fn parses_cpanfile() {
        let content = "requires 'Moo', '2.0';\n# a comment with 'quotes'\ntest_requires 'Test::More';\nrequires 'Path::Tiny';\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);
        assert!(facts.prereqs.iter().any(|p| p.module == "Moo"
            && p.version.as_deref() == Some("2.0")
            && p.phase == "runtime"));
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"),
            "test_requires → test phase; prereqs={:?}",
            facts.prereqs
        );
        assert!(facts.prereqs.iter().any(|p| p.module == "Path::Tiny"));
        // The comment's quoted text must not leak in as a module.
        assert!(!facts.prereqs.iter().any(|p| p.module == "quotes"));
    }

    #[test]
    fn cpanfile_quote_like_literals_do_not_swallow_later_declarations() {
        // An unbalanced brace inside a quote-like literal must stay literal
        // text. Before the shared lexer each of these opened an unsupported
        // block that never closed, dropping every later declaration.
        for literal in
            [r"qr/\{/", r"q{\{}", "qq(})", "qw( { )", "m|{|", "s/{/}/", r"s{\{}{}", r"tr{\{}{x}"]
        {
            let content = format!("my $x = {literal};\nrequires 'Path::Tiny';\n");
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), &content);
            assert!(
                facts.prereqs.iter().any(|p| p.module == "Path::Tiny" && p.phase == "runtime"),
                "literal {literal} must not suppress later declarations: {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_unbalanced_quote_like_delimiter_stays_fail_closed() {
        // `q{ {{ }` never closes, so it is not valid Perl and the file would
        // not run. The scanner must not invent facts from it; suppressing the
        // rest of the file is the safe reading of unparsable source.
        let content = "my $x = q{ {{ };\nrequires 'Path::Tiny';\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            facts.prereqs.is_empty(),
            "unbalanced delimiters must not publish facts: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_quote_like_bodies_are_not_module_names() {
        // `qw(...)` is not a literal this scanner can read a module name from.
        let content = "requires qw(Not::A::Fact);\nrequires 'Real::Fact';\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(facts.prereqs.iter().any(|p| p.module == "Real::Fact"));
        assert!(
            !facts.prereqs.iter().any(|p| p.module == "Not::A::Fact"),
            "quote-like bodies are not read as module text: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_bareword_subscripts_are_not_conditions() {
        // A modifier word used as a hash key is an expression, not a postfix
        // condition, and must not suppress an unconditional declaration.
        for expression in [
            "$versions{if}",
            "$versions->{unless}",
            "$versions{while}{until}",
            "$list[0]{for}",
            "${default}",
        ] {
            let content = format!("requires 'Foo', {expression};\n");
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), &content);
            assert!(
                facts.prereqs.iter().any(|p| p.module == "Foo" && p.phase == "runtime"),
                "{expression} is a version expression, not a condition: {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_subscript_braces_do_not_leak_block_scope() {
        // The subscript must not open a block that swallows the next statement.
        let content = "requires 'First', $versions{if};\nrequires 'Second';\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(facts.prereqs.iter().any(|p| p.module == "First"));
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Second"),
            "a subscript must not open a block: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_real_postfix_conditions_are_still_suppressed() {
        // The subscript and quote-like relaxations must not reopen the
        // fail-open path #13627 exists to close.
        let content = concat!(
            "requires 'Win32' if $^O eq 'MSWin32';\n",
            "test_requires 'Author::Only' unless $ENV{CI};\n",
            "requires('Looped') for 1 .. 3;\n",
            "requires 'Keyed', $versions{base} if $want{extra};\n",
            "requires 'Unconditional';\n",
        );
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        for suppressed in ["Win32", "Author::Only", "Looped", "Keyed"] {
            assert!(
                !facts.prereqs.iter().any(|p| p.module == suppressed),
                "{suppressed} is conditional and must not become an unconditional fact: {:?}",
                facts.prereqs
            );
        }
        assert!(facts.prereqs.iter().any(|p| p.module == "Unconditional"));
    }

    #[test]
    fn cpanfile_sub_braces_still_open_blocks() {
        // `sub{` ends in identifier characters. Without the sigil requirement
        // in `opens_hash_subscript` it would read as a subscript and promote
        // the feature block's dependency to an unconditional fact.
        let content = "feature 'SQLite' => sub{ requires 'DBD::SQLite'; };\nrequires 'Moo';\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            !facts.prereqs.iter().any(|p| p.module == "DBD::SQLite"),
            "a feature block stays unsupported: {:?}",
            facts.prereqs
        );
        assert!(facts.prereqs.iter().any(|p| p.module == "Moo"));
    }

    #[test]
    fn cpanfile_comments_are_not_statement_text() {
        // Comments are stripped by the same lexer, so a `#` inside a literal
        // stays text and a brace inside a comment cannot open a block.
        let content = concat!(
            "# requires 'Commented::Out';\n",
            "requires 'Has#Hash';\n",
            "# a stray { in a comment\n",
            "requires 'After::Comment';\n",
        );
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(!facts.prereqs.iter().any(|p| p.module == "Commented::Out"));
        assert!(facts.prereqs.iter().any(|p| p.module == "Has#Hash"));
        assert!(
            facts.prereqs.iter().any(|p| p.module == "After::Comment"),
            "a brace in a comment must not open a block: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_spaced_subscripts_keep_their_declaration() {
        // Perl allows whitespace before a subscript, and a version expression
        // written that way must still yield its module fact.
        for expression in
            ["$versions {base}", "$versions -> {base}", "$versions\n            {base}"]
        {
            let content = format!("requires 'Foo', {expression};\n");
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), &content);
            assert!(
                facts.prereqs.iter().any(|p| p.module == "Foo"),
                "{expression} is a subscript, not a block: {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_blocks_opening_on_the_next_line_stay_suppressed() {
        // Trimming trailing whitespace must not turn a block header into a
        // subscript. `sub` and `)` carry no sigil, so both stay blocks.
        let content = concat!(
            "feature 'SQLite' => sub\n{\n    requires 'DBD::SQLite';\n};\n",
            "if ($^O eq 'MSWin32')\n{\n    requires 'Win32';\n}\n",
            "requires 'Moo';\n",
        );
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        for suppressed in ["DBD::SQLite", "Win32"] {
            assert!(
                !facts.prereqs.iter().any(|p| p.module == suppressed),
                "{suppressed} sits in an unsupported block: {:?}",
                facts.prereqs
            );
        }
        assert!(facts.prereqs.iter().any(|p| p.module == "Moo"));
    }

    #[test]
    fn cpanfile_on_phase_block_opening_on_the_next_line_is_recognized() {
        let content = "on 'test' =>\n    sub\n{\n    requires 'Test::More';\n};\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"),
            "a canonical phase survives a next-line brace: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_computed_modules_do_not_become_unconditional_facts() {
        // The module a run-time expression chooses is not declared
        // unconditionally. Publishing the first literal inside one would claim
        // a dependency the cpanfile never states, and would read the remaining
        // literals as its version.
        for arguments in [
            "($enabled ? 'Foo' : 'Bar')",
            " helper('Foo')",
            " 'Foo' . 'Bar'",
            " $module_name",
            "(lc 'Foo')",
        ] {
            let content = format!("requires{arguments};\nrequires 'Unconditional';\n");
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), &content);

            assert_eq!(
                facts.prereqs.iter().filter(|p| p.module == "Unconditional").count(),
                1,
                "the plain declaration survives: {:?}",
                facts.prereqs
            );
            assert!(
                facts.prereqs.iter().all(|p| p.module == "Unconditional"),
                "requires{arguments} names its module at run time: {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_computed_versions_keep_the_named_module() {
        // A dynamic *version* is weaker than a dynamic module: the module is
        // still named literally, so the fact stands without a version rather
        // than being suppressed.
        for version in ["$versions{base}", "$versions->{base}", "compute_version()", "0.88"] {
            let content = format!("requires 'Foo', {version};\n");
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), &content);

            assert_eq!(
                facts.prereqs,
                vec![Prereq {
                    module: "Foo".to_string(),
                    version: None,
                    phase: "runtime".to_string(),
                    relation: "requires".to_string(),
                }],
                "a computed version yields no version, not a lost module: {version}"
            );
        }
    }

    #[test]
    fn cpanfile_literal_versions_are_still_recorded() {
        // The relaxation above must not stop reading a version that *is* a
        // literal, in either call spelling.
        for content in ["requires 'Foo', '0.88';\n", "requires('Foo' => '0.88');\n"] {
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);
            assert_eq!(
                facts.prereqs,
                vec![Prereq {
                    module: "Foo".to_string(),
                    version: Some("0.88".to_string()),
                    phase: "runtime".to_string(),
                    relation: "requires".to_string(),
                }],
                "literal version survives: {content:?}"
            );
        }
    }

    #[test]
    fn cpanfile_parenthesized_bare_phase_is_recognized() {
        // `on(develop => sub {...})` is a bareword phase behind a parenthesis;
        // the fat comma quotes it, so it names the same phase as `on 'develop'`.
        for phase in CPANFILE_PHASES {
            let content = format!("on({phase} => sub {{ requires 'Phase::Dep'; }});\n");
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), &content);

            assert!(
                facts.prereqs.iter().any(|p| p.module == "Phase::Dep" && p.phase == *phase),
                "on({phase} => ...) keeps its canonical phase: {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_parenthesized_unknown_phase_is_still_rejected() {
        // Accepting the parenthesized spelling must not accept a phase that is
        // not canonical: that block stays unsupported.
        let content = "on(deploy => sub { requires 'Deploy::Dep'; });\nrequires 'Moo';\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            !facts.prereqs.iter().any(|p| p.module == "Deploy::Dep"),
            "an unknown phase stays unsupported: {:?}",
            facts.prereqs
        );
        assert!(facts.prereqs.iter().any(|p| p.module == "Moo"));
    }

    #[test]
    fn adversarial_no_panic_on_hostile_shapes() {
        // Arithmetic and slicing in the lexer, `opens_hash_subscript`,
        // `sole_plain_literal`, and `strip_call_parentheses` must survive
        // truncated, unbalanced, and unicode input.
        for content in [
            "'",
            "\"",
            "q",
            "q{",
            "s{a}",
            "tr{a}",
            "requires '",
            "requires(",
            "requires()",
            "on(",
            "on",
            "{",
            "}",
            "$",
            "$h->{",
            "->{",
            "#",
            "requires 'Ünïcodé', '→';",
            "requires '\\''; requires 'After';",
            "()",
            "((((",
            "))))",
            ";;;;",
            "=>",
            "requires =>",
            "on(('test') => sub {",
            "$$$${",
            "qq",
            "m",
            "y",
            "requires 'a' . ",
            "'''",
        ] {
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);
            // No assertion on contents: the property under test is that the
            // scanner terminates and does not panic on hostile input.
            let _ = facts.prereqs.len();
        }
    }

    #[test]
    fn adversarial_fail_open_sweep() {
        // Every one of these encloses a declaration in a construct that is not
        // an unconditional top-level or canonical-phase declaration. None may
        // reach the facts.
        let hostile = [
            "feature 'X' => sub{ requires 'Leak'; };",
            "feature 'X' => sub\n{\n requires 'Leak';\n}\n;",
            "if ($^O eq 'MSWin32') { requires 'Leak'; }",
            "unless ($ENV{CI}) { requires 'Leak'; }",
            "for my $m (@list) { requires 'Leak'; }",
            "on 'deploy' => sub { requires 'Leak'; };",
            "on(deploy => sub { requires 'Leak'; });",
            "requires 'Leak' if $^O eq 'MSWin32';",
            "requires 'Leak' unless $ENV{CI};",
            "requires('Leak') for 1 .. 3;",
            "requires 'Leak', $v{base} if $want{extra};",
            "requires($enabled ? 'Leak' : 'Other');",
            "requires helper('Leak');",
            "requires 'Le' . 'ak';",
            "feature 'X' => sub { on 'test' => sub { requires 'Leak'; }; };",
            "if (1) { on 'test' => sub { requires 'Leak'; }; }",
        ];
        for content in hostile {
            let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);
            assert!(
                !facts.prereqs.iter().any(|p| p.module.contains("Leak")),
                "FAIL-OPEN: {content:?} published {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_block_form_phase_deps() {
        // Module::CPANfile block syntax: `on 'phase' => sub { requires ... }`.
        let content = "requires 'Moo';\non 'test' => sub {\n    requires 'Test::More', '0.88';\n};\non 'develop' => sub {\n    requires 'Perl::Critic';\n};\n";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Moo" && p.phase == "runtime"),
            "flat requires stays runtime"
        );
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"),
            "block-form requires picks up the on-phase; prereqs={:?}",
            facts.prereqs
        );
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Perl::Critic" && p.phase == "develop"),
            "develop block phase"
        );
    }

    #[test]
    fn cpanfile_quoted_delimiters_are_statement_text() {
        let content = r#"
            my $open = '{';
            my $close = "}";
            my $separator = ";";
            requires 'Path::Tiny';
        "#;
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            facts.prereqs.iter().any(|p| p.module == "Path::Tiny" && p.phase == "runtime"),
            "quoted braces and semicolons must remain statement text: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_parenthesized_on_phase_is_recognized() {
        let content = "on('test' => sub { requires 'Test::More'; });";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(
            facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"),
            "parenthesized on blocks retain their canonical phase: {:?}",
            facts.prereqs
        );
    }

    #[test]
    fn cpanfile_nested_on_blocks_use_innermost_phase() {
        let content = "on 'test' => sub { on 'build' => sub { requires 'Nested::Build'; }; };";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        let nested_build: Vec<_> =
            facts.prereqs.iter().filter(|p| p.module == "Nested::Build").collect();
        assert_eq!(nested_build.len(), 1);
        assert_eq!(nested_build[0].phase, "build");
    }

    #[test]
    fn cpanfile_canonical_on_phases_emit_declared_phase() {
        let content = "on 'runtime' => sub { requires 'Runtime::Dep'; }; on 'configure' => sub { requires 'Configure::Dep'; };";
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(facts.prereqs.iter().any(|p| p.module == "Runtime::Dep" && p.phase == "runtime"));
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Configure::Dep" && p.phase == "configure")
        );
    }

    #[test]
    fn cpanfile_unsupported_blocks_do_not_become_unconditional_facts() {
        let content = r#"
            requires 'Top::Level';
            feature 'SQLite' => sub {
                requires 'DBD::SQLite';
                on 'test' => sub { requires 'Feature::Test'; };
            };
            if ($^O eq 'MSWin32') {
                build_requires 'Win32::Build';
            }
            on 'test' => sub {
                requires 'Test::More';
                if ($ENV{AUTHOR_TESTING}) { requires 'Test::Warnings'; }
                recommends 'Test::Deep';
            };
        "#;
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert!(facts.prereqs.iter().any(|p| p.module == "Top::Level" && p.phase == "runtime"));
        assert!(facts.prereqs.iter().any(|p| p.module == "Test::More" && p.phase == "test"));
        assert!(facts.prereqs.iter().any(|p| p.module == "Test::Deep" && p.phase == "test"));
        for conditional in ["DBD::SQLite", "Feature::Test", "Win32::Build", "Test::Warnings"] {
            assert!(
                !facts.prereqs.iter().any(|p| p.module == conditional),
                "conditional prerequisite {conditional} must not become unconditional: {:?}",
                facts.prereqs
            );
        }
    }

    #[test]
    fn cpanfile_unknown_phases_and_keyword_prefixes_are_rejected() {
        let content = r#"
            on 'deploy' => sub { requires 'Deploy::Only'; };
            requires_extra 'Prefix::Collision';
            oncall 'test' => sub { requires 'Not::An::On::Block'; };
            on 'build' => sub { requires 'Build::Known'; };
        "#;
        let facts = parse_cpanfile(FileId::new("cpanfile", &Digest::of("x")), content);

        assert_eq!(
            facts.prereqs,
            vec![Prereq {
                module: "Build::Known".to_string(),
                version: None,
                phase: "build".to_string(),
                relation: "requires".to_string(),
            }]
        );
    }

    #[test]
    fn cpanfile_postfix_modifiers_do_not_publish_conditional_facts() {
        for &(keyword, relation, phase) in CPANFILE_KEYWORDS {
            for modifier in ["if", "unless", "while", "until", "for", "foreach", "when"] {
                for declaration in [
                    format!("{keyword} 'Conditional::Dep', '0'"),
                    format!("{keyword}('Conditional::Dep', '0')"),
                ] {
                    let content =
                        format!("{declaration} {modifier} $enabled; {keyword} 'Kept::Dep', '1';");
                    let facts = parse_cpanfile(fid(), &content);
                    assert_eq!(
                        facts.prereqs,
                        vec![Prereq {
                            module: "Kept::Dep".to_string(),
                            version: Some("1".to_string()),
                            phase: phase.to_string(),
                            relation: relation.to_string(),
                        }],
                        "{content}"
                    );

                    // The final statement before a block's closing brace need
                    // not have a semicolon. Exercise that extraction path too.
                    let content =
                        format!("on 'test' => sub {{ {declaration} {modifier} $enabled }};");
                    assert!(parse_cpanfile(fid(), &content).prereqs.is_empty(), "{content}");
                }
            }
        }
    }

    #[test]
    fn cpanfile_modifier_words_in_literals_are_not_conditions() {
        let content = r#"
            requires 'if', '0';
            recommends "unless", "1";
            on 'test' => sub { requires 'while', '2'; };
        "#;
        let facts = parse_cpanfile(fid(), content);
        let rows = facts
            .prereqs
            .iter()
            .map(|p| (p.module.as_str(), p.version.as_deref(), p.phase.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            vec![
                ("unless", Some("1"), "runtime"),
                ("if", Some("0"), "runtime"),
                ("while", Some("2"), "test"),
            ]
        );
        for statement in [
            r#"requires 'if_required'"#,
            r#"requires 'X', "escaped\" if literal""#,
            r#"requires 'X', 'escaped\' unless literal'"#,
            r#"requires 'X', if_required()"#,
        ] {
            assert!(!has_cpanfile_statement_modifier(statement), "{statement}");
        }
        assert!(has_cpanfile_statement_modifier("requires('X')unless$enabled"));
        assert!(has_cpanfile_statement_modifier("requires 'X' if\n$enabled"));
        assert!(has_cpanfile_statement_modifier("requires 'X' if"));
    }

    #[test]
    fn cpanfile_longest_keyword_wins() {
        // `configure_requires` must not be captured by the `requires` prefix.
        let facts = parse_cpanfile(
            FileId::new("cpanfile", &Digest::of("x")),
            "configure_requires 'ExtUtils::MakeMaker';\n",
        );
        let p = facts.prereqs.iter().find(|p| p.module == "ExtUtils::MakeMaker").unwrap();
        assert_eq!(p.phase, "configure");
    }

    #[test]
    fn cpanfile_conflicts_is_recognized() {
        // Regression: `conflicts` is a documented cpanfile/META relation
        // (RELATIONS includes it), but CPANFILE_KEYWORDS previously had no
        // entry for it, so `conflicts 'Foo';` was silently dropped.
        let facts = parse_cpanfile(
            FileId::new("cpanfile", &Digest::of("x")),
            "conflicts 'Some::Broken::Module';\n",
        );
        assert!(
            facts.prereqs.iter().any(|p| p.module == "Some::Broken::Module"),
            "conflicts statement must produce a prereq entry; prereqs={:?}",
            facts.prereqs
        );
        let p = facts.prereqs.iter().find(|p| p.module == "Some::Broken::Module").unwrap();
        assert_eq!(p.relation, "conflicts");
        assert_eq!(p.phase, "runtime");
    }
}
