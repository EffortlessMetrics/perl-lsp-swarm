//! Recurrence guard for the residual TDD facade consumer cutover.
//!
//! Issue #11382 moved every remaining consumer off TDD/test-generation
//! authority imported through `perl-parser` compatibility paths (`tdd`,
//! `tdd_basic`, `tdd_workflow`, `test_generator`, `test_runner`, and their
//! root type re-exports). The facade crate itself still owns those
//! compatibility exports until #11385, so it is excluded here. Any
//! re-introduction elsewhere must register an owned, conditioned exception
//! below instead of silently returning.
//!
//! Detection normalizes each source before matching so formatting cannot hide
//! a violation: line and block comments are stripped while string, raw-string,
//! byte-string, and character literals are lexed and preserved verbatim, then
//! every `perl_parser ::` path head, its recorded import aliases, and
//! brace-group membership are scanned with whitespace-insensitive boundaries.
//! Multi-line forms like
//!
//! ```ignore
//! use perl_parser::{
//!     Parser,
//!     tdd_basic::TestGenerator,
//! };
//! ```
//!
//! are rejected exactly like their single-line equivalents, matching the
//! pre-image shape this cutover removed from
//! `crates/perl-lsp-rs/src/runtime/mod.rs`.
//!
//! The facade's `compat` escape hatch is covered as well: while #11385 has
//! not removed `perl_parser::compat::{tdd_basic, tdd_workflow, test_generator,
//! test_runner}`, a consumer such as
//! `use perl_parser::compat::test_generator::TestGenerator;` reaches the same
//! authority and is rejected with a `perl_parser::compat::...` token. Importing
//! the bare `compat` module without a governed segment stays allowed.
//!
//! Governed scan roots are the workspace `crates` tree plus the root-level
//! members `xtask/src` and `fuzz/fuzz_targets`; `crates/perl-parser/` itself
//! is excluded because it is the facade owner until #11385.
//! `proptest::test_runner` and similar foreign heads never match because
//! every hit must anchor on the `perl_parser` path head.
//!
//! Aliased imports are covered as well: `use perl_parser as parser_facade;`
//! followed by `parser_facade::tdd_basic::TestGenerator` resolves the alias
//! deterministically from normalized text (chained renames converge within a
//! bounded number of passes) and the governed usage under an aliased head is
//! rejected with its canonical `perl_parser::...` token.
//!
//! Known limitation: character-literal recognition is deliberately narrow so
//! lifetime ticks (`&'static T`) stay ordinary text; escaped literals wider
//! than a small fixed window fall back to being passed through, which can
//! never create violations and mirrors the precision of the sibling semantic
//! facade guard.

use std::{
    fs,
    path::{Path, PathBuf},
};

const FACADE_CRATE_PREFIX: &str = "crates/perl-parser/";
const FACADE_HEAD: &str = "perl_parser";

/// Scan roots for governed consumer sources. Root-level workspace members
/// (`xtask`, `fuzz`) are scanned explicitly alongside `crates`.
const SCAN_ROOTS: &[&str] = &["crates", "xtask/src", "fuzz/fuzz_targets"];

/// Leading path segments of `perl-parser` modules that re-export TDD and
/// test-generation authority.
const FORBIDDEN_FACADE_SEGMENTS: &[&str] =
    &["tdd", "tdd_basic", "tdd_workflow", "test_generator", "test_runner"];

/// Root re-export items of those same TDD modules. These only match in a
/// `perl_parser ::` path-head or brace-group membership context, never bare.
const FORBIDDEN_ROOT_REEXPORT_ITEMS: &[&str] =
    &["TestFramework", "TestGenerator", "TestRunner", "TddWorkflow"];

struct TemporaryException {
    path: &'static str,
    token: &'static str,
    owner_issue: &'static str,
    removal_condition: &'static str,
}

const TEMPORARY_EXCEPTIONS: &[TemporaryException] = &[];

fn repo_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

/// Stack entry for one open literal: `usize::MAX` marks a plain `"…"` string,
/// any other value is the hash count of a raw-string terminator `"###…"`.
const PLAIN_LITERAL: usize = usize::MAX;

/// Closing index of a narrow character or byte-character literal opening at
/// `open`, or [`None`] when the tick is a lifetime marker, whitespace content,
/// or too wide to be a bounded literal; passing such ticks through verbatim
/// can never invent violations.
fn char_literal_close(chars: &[char], open: usize) -> Option<usize> {
    match chars.get(open + 1)? {
        '\\' => {
            let limit = (open + 15).min(chars.len());
            ((open + 2)..limit).find(|&probe| chars[probe] == '\'')
        }
        ch if !ch.is_whitespace() && *ch != '\'' => {
            if chars.get(open + 2) == Some(&'\'') {
                Some(open + 2)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Number of leading `#` characters starting just past an optional raw-string
/// prefix, together with whether a literal quote follows them.
fn raw_string_quote_after_hashes(chars: &[char], mut cursor: usize) -> Option<(usize, usize)> {
    let hashes = cursor;
    while chars.get(cursor) == Some(&'#') {
        cursor += 1;
    }
    let seen = cursor - hashes;
    if chars.get(cursor) == Some(&'"') { Some((seen, cursor + 1)) } else { None }
}

/// Strip line comments and (nested) block comments while preserving all other
/// structure, including newlines and brace groups. String, byte-string, raw
/// string, and character literals are lexed so their `/` characters never
/// start a comment: a `//` or unbalanced `/*` inside a literal can no longer
/// hide governed imports that follow it in real source.
fn code_without_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut block_depth = 0usize;
    let mut literal_hashes: Vec<usize> = Vec::new();
    while index < chars.len() {
        // Inside a literal everything is copied verbatim until its own
        // terminator closes it; comment markers carry no meaning there.
        if !literal_hashes.is_empty() {
            let open_hash = *literal_hashes.last().unwrap_or(&PLAIN_LITERAL);
            out.push(chars[index]);
            if open_hash == PLAIN_LITERAL {
                match chars[index] {
                    '\\' => {
                        if let Some(&escaped) = chars.get(index + 1) {
                            out.push(escaped);
                        }
                        index += 2;
                    }
                    '"' => {
                        literal_hashes.pop();
                        index += 1;
                    }
                    _ => index += 1,
                }
                continue;
            }
            if chars[index] == '"' {
                let mut cursor = index + 1;
                let mut seen = 0usize;
                while seen < open_hash && chars.get(cursor) == Some(&'#') {
                    cursor += 1;
                    seen += 1;
                }
                if seen == open_hash {
                    literal_hashes.pop();
                    out.extend(&chars[index + 1..cursor]);
                    index = cursor;
                    continue;
                }
            }
            index += 1;
            continue;
        }
        if block_depth == 0 && chars[index] == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
        } else if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
            block_depth += 1;
            index += 2;
        } else if block_depth > 0 && chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
            block_depth -= 1;
            index += 2;
        } else if chars[index] == '\''
            || (chars[index] == 'b' && chars.get(index + 1) == Some(&'\''))
        {
            let open = if chars[index] == '\'' { index } else { index + 1 };
            match char_literal_close(&chars, open) {
                Some(close) => {
                    out.extend(&chars[index..=close]);
                    index = close + 1;
                }
                None => {
                    out.push(chars[index]);
                    index += 1;
                }
            }
        } else if (chars[index] == 'r'
            || (chars[index] == 'b' && chars.get(index + 1) == Some(&'r')))
            && (index == 0
                || !(chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_'))
        {
            let prefix_end = if chars[index] == 'r' { index + 1 } else { index + 2 };
            match raw_string_quote_after_hashes(&chars, prefix_end) {
                Some((hashes, after_quote)) => {
                    out.extend(&chars[index..after_quote]);
                    literal_hashes.push(hashes);
                    index = after_quote;
                }
                None => {
                    out.push(chars[index]);
                    index += 1;
                }
            }
        } else if chars[index] == '"' || (chars[index] == 'b' && chars.get(index + 1) == Some(&'"'))
        {
            out.push(chars[index]);
            if chars[index] == 'b' {
                out.push('"');
                index += 2;
            } else {
                index += 1;
            }
            literal_hashes.push(PLAIN_LITERAL);
        } else {
            if block_depth == 0 {
                out.push(chars[index]);
            }
            index += 1;
        }
    }
    out
}

fn skip_whitespace(chars: &[char], mut index: usize) -> usize {
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    index
}

fn read_identifier(chars: &[char], start: usize, end: usize) -> String {
    let mut ident = String::new();
    let mut index = start;
    while index < end && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
        ident.push(chars[index]);
        index += 1;
    }
    ident
}

fn skip_to_identifier_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
        index += 1;
    }
    index
}

fn is_forbidden_ident(ident: &str) -> bool {
    FORBIDDEN_FACADE_SEGMENTS.contains(&ident) || FORBIDDEN_ROOT_REEXPORT_ITEMS.contains(&ident)
}

/// Record a forbidden identifier under its governing path-head token name,
/// threading the `compat` escape-hatch infix through the hit label.
fn record_forbidden_ident(ident: &str, compat: bool, hits: &mut Vec<String>) {
    if is_forbidden_ident(ident) {
        if compat {
            hits.push(format!("{FACADE_HEAD}::compat::{ident}"));
        } else {
            hits.push(format!("{FACADE_HEAD}::{ident}"));
        }
    }
}

/// Record the leading identifier of one brace-group member span, descending
/// into nested groups (for example `test_runner::{TestKind, TestRunner}`).
fn record_member(chars: &[char], start: usize, end: usize, compat: bool, hits: &mut Vec<String>) {
    let member_start = skip_whitespace(chars, start);
    if member_start >= end {
        return;
    }
    let ident = read_identifier(chars, member_start, end);
    record_forbidden_ident(&ident, compat, hits);
    let mut cursor = member_start + ident.len();
    while cursor < end {
        if chars[cursor] == '{' {
            cursor = scan_brace_group(chars, cursor + 1, compat, hits);
        } else {
            cursor += 1;
        }
    }
}

/// Walk one brace group starting just past `{`. Split top-level members on
/// commas and return the index just past the matching `}`.
fn scan_brace_group(chars: &[char], start: usize, compat: bool, hits: &mut Vec<String>) -> usize {
    let mut depth = 1usize;
    let mut member_start = start;
    let mut index = start;
    while index < chars.len() {
        match chars[index] {
            '{' => {
                depth += 1;
                index += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    record_member(chars, member_start, index, compat, hits);
                    return index + 1;
                }
                index += 1;
            }
            ',' if depth == 1 => {
                record_member(chars, member_start, index, compat, hits);
                member_start = index + 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    index
}

/// Consume one `::`-continuation of a `perl_parser` path head. A leading
/// `compat` segment is transparent: scanning resumes after it with the
/// `compat` infix enabled so escape-hatch consumers stay detected. Returns
/// the index the outer scan should resume from.
fn scan_facade_path(chars: &[char], start: usize, compat: bool, hits: &mut Vec<String>) -> usize {
    let mut cursor = skip_whitespace(chars, start);
    if chars.get(cursor) != Some(&':') {
        return start;
    }
    cursor = skip_whitespace(chars, cursor + 1);
    if chars.get(cursor) != Some(&':') {
        return start;
    }
    cursor = skip_whitespace(chars, cursor + 1);
    match chars.get(cursor) {
        Some('{') => scan_brace_group(chars, cursor + 1, compat, hits),
        Some(_) => {
            let ident_end = skip_to_identifier_end(chars, cursor);
            let ident = read_identifier(chars, cursor, ident_end);
            if ident == "compat" && !compat {
                return scan_facade_path(chars, ident_end, true, hits).max(start);
            }
            record_forbidden_ident(&ident, compat, hits);
            ident_end.max(start)
        }
        None => start,
    }
}

/// One import alias bound to facade-rooted authority, together with whether
/// its declaring path threaded the `compat` escape hatch.
struct FacadeAlias {
    name: String,
    compat: bool,
}

/// Upper bound on chained-alias resolution passes. Every declaration chain
/// converges far below this bound, keeping the fixpoint deterministic instead
/// of requiring whole-program name resolution.
const ALIAS_FIXPOINT_PASSES: usize = 8;

/// An identifier read at or after `start`, with its past-the-end index.
struct ReadIdent {
    name: String,
    end: usize,
}

/// Read one identifier beginning at or after `start`.
fn read_ident_at(chars: &[char], start: usize) -> Option<ReadIdent> {
    let begin = skip_whitespace(chars, start);
    let first = *chars.get(begin)?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let end = skip_to_identifier_end(chars, begin);
    Some(ReadIdent { name: read_identifier(chars, begin, end), end })
}

/// Whether the exact word `word` starts at `pos`, delimited on both sides by
/// non-identifier characters, so longer identifiers never half-match.
fn word_at(chars: &[char], pos: usize, word: &str) -> bool {
    let word_chars: Vec<char> = word.chars().collect();
    let after = pos + word_chars.len();
    chars.len() >= after
        && chars[pos..after] == word_chars[..]
        && (pos == 0 || !(chars[pos - 1].is_ascii_alphanumeric() || chars[pos - 1] == '_'))
        && !chars.get(after).is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_')
}

/// Index just past the next `;` at or after `from`, or end of input.
fn semicolon_end(chars: &[char], from: usize) -> usize {
    let mut probe = from;
    while probe < chars.len() && chars[probe] != ';' {
        probe += 1;
    }
    if probe < chars.len() { probe + 1 } else { chars.len() }
}

/// Whether `name` is already a known or newly collected facade alias.
fn is_registered_alias(name: &str, aliases: &[FacadeAlias], fresh: &[FacadeAlias]) -> bool {
    aliases.iter().any(|alias| alias.name == name) || fresh.iter().any(|alias| alias.name == name)
}

/// The `compat` inheritance carried by a previously seen alias name.
fn recorded_alias_compat(name: &str, aliases: &[FacadeAlias]) -> bool {
    aliases.iter().find(|alias| alias.name == name).is_some_and(|alias| alias.compat)
}

/// Register one `as Alias` binding discovered inside a facade-rooted use
/// declaration. Member spans may nest groups and generic brackets; any word
/// `as` followed by an identifier binds, because comments are already
/// stripped and strings cannot appear inside use paths.
fn record_member_alias(
    chars: &[char],
    start: usize,
    end: usize,
    compat: bool,
    fresh: &mut Vec<FacadeAlias>,
) {
    let mut pos = start;
    let mut depth = 0usize;
    while pos < end {
        match chars[pos] {
            '{' | '(' | '[' | '<' => {
                depth += 1;
                pos += 1;
            }
            '}' | ')' | ']' | '>' => {
                depth = depth.saturating_sub(1);
                pos += 1;
            }
            ch if ch.is_ascii_alphabetic() || ch == '_' => {
                let word_end = skip_to_identifier_end(chars, pos);
                let word = read_identifier(chars, pos, word_end);
                if word == "as" && depth == 0 && word_at(chars, pos, "as") {
                    if let Some(alias) = read_ident_at(chars, word_end) {
                        if !fresh.iter().any(|known| known.name == alias.name) {
                            fresh.push(FacadeAlias { name: alias.name, compat });
                        }
                        pos = alias.end;
                        continue;
                    }
                }
                pos = word_end.max(pos + 1);
            }
            _ => pos += 1,
        }
    }
}

/// Walk one brace group of a use declaration, registering `as` bindings from
/// each top-level member span when the parent path is facade-rooted, and
/// return the index past the matching `}`.
fn record_braced_group(
    chars: &[char],
    start: usize,
    facade_rooted: bool,
    compat: bool,
    fresh: &mut Vec<FacadeAlias>,
) -> usize {
    let mut depth = 1usize;
    let mut member_start = start;
    let mut index = start;
    while index < chars.len() {
        match chars[index] {
            '{' => {
                depth += 1;
                index += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if facade_rooted {
                        record_member_alias(chars, member_start, index, compat, fresh);
                    }
                    return index + 1;
                }
                index += 1;
            }
            ',' if depth == 1 => {
                if facade_rooted {
                    record_member_alias(chars, member_start, index, compat, fresh);
                }
                member_start = index + 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    index
}

/// Parse one `use … ;` statement starting right after the `use` keyword,
/// registering every `as Alias` whose declared path roots at `perl_parser`
/// (threading the `compat` escape hatch) or at an already recorded facade
/// alias. Brace-group members inherit their parent root, so renames like
/// `use perl_parser::{tdd_basic as tb};` register too. Returns the resume
/// index at or past the terminating `;`.
fn record_use_aliases(
    chars: &[char],
    after_keyword: usize,
    aliases: &[FacadeAlias],
    fresh: &mut Vec<FacadeAlias>,
) -> usize {
    let root = match read_ident_at(chars, after_keyword) {
        Some(found) => found,
        None => return semicolon_end(chars, after_keyword),
    };
    let mut compat = false;
    let facade_rooted = root.name == FACADE_HEAD || is_registered_alias(&root.name, aliases, fresh);
    if facade_rooted && root.name != FACADE_HEAD {
        compat = recorded_alias_compat(&root.name, aliases);
    }
    let mut pos = root.end;
    loop {
        pos = skip_whitespace(chars, pos);
        match chars.get(pos) {
            Some(':') if chars.get(pos + 1) == Some(&':') => match read_ident_at(chars, pos + 2) {
                Some(segment) => {
                    if segment.name == "compat" {
                        compat = true;
                    }
                    pos = segment.end;
                }
                None => return semicolon_end(chars, pos + 2),
            },
            Some('{') => {
                let group_end = record_braced_group(chars, pos + 1, facade_rooted, compat, fresh);
                let tail = skip_whitespace(chars, group_end);
                return if chars.get(tail) == Some(&';') {
                    tail + 1
                } else {
                    semicolon_end(chars, tail)
                };
            }
            Some(';') => return pos + 1,
            _ => {
                if word_at(chars, pos, "as") && facade_rooted {
                    if let Some(alias) = read_ident_at(chars, pos + 2) {
                        if !is_registered_alias(&alias.name, aliases, fresh) {
                            fresh.push(FacadeAlias { name: alias.name, compat });
                        }
                        return semicolon_end(chars, alias.end);
                    }
                }
                return semicolon_end(chars, pos);
            }
        }
    }
}

/// One scan for facade-rooted `as` bindings not yet recorded.
fn collect_fresh_aliases(chars: &[char], aliases: &[FacadeAlias]) -> Vec<FacadeAlias> {
    let use_keyword: Vec<char> = "use".chars().collect();
    let mut fresh = Vec::new();
    let mut index = 0usize;
    while index + use_keyword.len() <= chars.len() {
        if chars[index..index + use_keyword.len()] == use_keyword[..]
            && (index == 0
                || !(chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_'))
            && !chars
                .get(index + use_keyword.len())
                .is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_')
        {
            index = record_use_aliases(chars, index + use_keyword.len(), aliases, &mut fresh);
        } else {
            index += 1;
        }
    }
    fresh
}

/// Resolve every facade-rooted import alias from normalized text with a
/// deterministic bounded fixpoint, so chained renames converge without whole-
/// program name resolution.
fn facade_aliases(chars: &[char]) -> Vec<FacadeAlias> {
    let mut aliases: Vec<FacadeAlias> = Vec::new();
    for _pass in 0..ALIAS_FIXPOINT_PASSES {
        let fresh = collect_fresh_aliases(chars, &aliases);
        if fresh.is_empty() {
            break;
        }
        for alias in fresh {
            if !aliases.iter().any(|known| known.name == alias.name) {
                aliases.push(alias);
            }
        }
    }
    aliases
}

fn forbidden_facade_references(code: &str) -> Vec<String> {
    let chars: Vec<char> = code.chars().collect();
    let mut hits: Vec<String> = Vec::new();
    // Longest-head-first keeps overlapping names deterministic; an alias
    // equal to the facade head itself dedups naturally.
    let mut heads: Vec<(Vec<char>, bool)> = vec![(FACADE_HEAD.chars().collect(), false)];
    heads.extend(
        facade_aliases(&chars)
            .into_iter()
            .map(|alias| (alias.name.chars().collect(), alias.compat)),
    );
    heads.sort_unstable_by(|left, right| {
        right.0.len().cmp(&left.0.len()).then_with(|| left.0.cmp(&right.0))
    });
    let mut index = 0usize;
    while index < chars.len() {
        let mut resumed = None;
        for (head, compat) in &heads {
            let head_len = head.len();
            if index + head_len <= chars.len()
                && chars[index..index + head_len] == head[..]
                && (index == 0
                    || !(chars[index - 1].is_ascii_alphanumeric() || chars[index - 1] == '_'))
            {
                resumed = Some(
                    scan_facade_path(&chars, index + head_len, *compat, &mut hits).max(index + 1),
                );
                break;
            }
        }
        index = resumed.unwrap_or_else(|| index + 1);
    }
    hits.sort();
    hits.dedup();
    hits
}

fn collect_rs_files(
    dir: &Path,
    relative: &str,
    found: &mut Vec<String>,
    failures: &mut Vec<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            failures.push(format!("read {}: {error}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(format!("entry under {}: {error}", dir.display()));
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_relative = format!("{relative}/{name}");
        if child_relative.starts_with(FACADE_CRATE_PREFIX) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, &child_relative, found, failures);
        } else if name.ends_with(".rs") {
            found.push(child_relative);
        }
    }
}

fn unregistered_facade_imports() -> (Vec<(String, String)>, Vec<String>) {
    let root = repo_root();
    let mut files = Vec::new();
    let mut failures = Vec::new();
    for scan_root in SCAN_ROOTS {
        collect_rs_files(&root.join(scan_root), scan_root, &mut files, &mut failures);
    }
    files.sort();

    let mut violations = Vec::new();
    for relative in files {
        let source = match fs::read_to_string(root.join(&relative)) {
            Ok(source) => source,
            Err(error) => {
                failures.push(format!("read {relative}: {error}"));
                continue;
            }
        };
        for hit in forbidden_facade_references(&code_without_comments(&source)) {
            if !TEMPORARY_EXCEPTIONS
                .iter()
                .any(|exception| exception.path == relative && exception.token == hit)
            {
                violations.push((relative.clone(), hit));
            }
        }
    }
    (violations, failures)
}

#[test]
fn no_consumer_imports_tdd_authority_through_perl_parser() {
    let (violations, failures) = unregistered_facade_imports();
    assert!(
        failures.is_empty(),
        "governed scan must reach every governed file (issue #11382): {failures:?}"
    );
    assert!(
        violations.is_empty(),
        "consumers must not import TDD/test-generation authority through \
         perl-parser facade paths (issue #11382); migrate to perl_tdd_support \
         or register an owned exception: {violations:?}"
    );
}

#[test]
fn temporary_exceptions_are_unique_owned_and_still_consumed() {
    let root = repo_root();
    let mut unique = std::collections::BTreeSet::new();
    for exception in TEMPORARY_EXCEPTIONS {
        assert!(
            unique.insert((exception.path, exception.token)),
            "duplicate exception for {} / {}",
            exception.path,
            exception.token
        );
        assert!(exception.owner_issue.starts_with('#'), "exception needs an owning issue");
        assert!(
            !exception.removal_condition.trim().is_empty(),
            "exception needs a removal condition"
        );

        assert!(
            fs::read_to_string(root.join(exception.path))
                .is_ok_and(|source| code_without_comments(&source).contains(exception.token)),
            "stale exception {} / {} must be removed",
            exception.path,
            exception.token
        );
    }
}

#[test]
fn single_line_direct_path_imports_are_rejected() {
    let source = "\
use perl_parser::tdd_basic::TestGenerator;
use perl_parser::tdd_workflow::TddWorkflow;
use perl_parser::tdd::WorkflowState;
use perl_parser::test_generator::{TestGenerator, TestFramework};
use perl_parser::test_runner::{TestKind, TestRunner};
use perl_parser::TestGenerator;
use perl_parser::TestFramework;
use perl_parser::TestRunner;
use perl_parser::TddWorkflow;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec![
            "perl_parser::TddWorkflow".to_string(),
            "perl_parser::TestFramework".to_string(),
            "perl_parser::TestGenerator".to_string(),
            "perl_parser::TestRunner".to_string(),
            "perl_parser::tdd".to_string(),
            "perl_parser::tdd_basic".to_string(),
            "perl_parser::tdd_workflow".to_string(),
            "perl_parser::test_generator".to_string(),
            "perl_parser::test_runner".to_string(),
        ]
    );
}

#[test]
fn multi_line_brace_pre_image_is_rejected() {
    let source = "\
use perl_parser::{
    Parser,
    ast::{Node, NodeKind},
    declaration::ParentMap,
    tdd_basic::TestGenerator,
    test_runner::{TestKind, TestRunner},
};
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec![
            "perl_parser::TestRunner".to_string(),
            "perl_parser::tdd_basic".to_string(),
            "perl_parser::test_runner".to_string(),
        ]
    );
}

#[test]
fn single_line_brace_group_is_rejected() {
    let source = "use perl_parser::{Parser, tdd_basic::TestGenerator, test_runner::TestKind};\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::tdd_basic".to_string(), "perl_parser::test_runner".to_string()]
    );
}

#[test]
fn compat_escape_hatch_paths_are_rejected() {
    let direct = "use perl_parser::compat::test_generator::TestGenerator;\n";
    let hits = forbidden_facade_references(&code_without_comments(direct));
    assert_eq!(hits, vec!["perl_parser::compat::test_generator".to_string()]);

    let braced = "use perl_parser::compat::{tdd_basic, tdd_workflow, TddWorkflow};\n";
    let hits = forbidden_facade_references(&code_without_comments(braced));
    assert_eq!(
        hits,
        vec![
            "perl_parser::compat::TddWorkflow".to_string(),
            "perl_parser::compat::tdd_basic".to_string(),
            "perl_parser::compat::tdd_workflow".to_string(),
        ]
    );

    let spaced = "use perl_parser :: compat :: tdd_basic ;\n";
    let hits = forbidden_facade_references(&code_without_comments(spaced));
    assert_eq!(hits, vec!["perl_parser::compat::tdd_basic".to_string()]);
}

#[test]
fn bare_compat_module_import_remains_allowed() {
    let source = "use perl_parser::compat;\nuse perl_parser::prelude::*;\n";
    assert!(forbidden_facade_references(&code_without_comments(source)).is_empty());
}

#[test]
fn whitespace_between_path_segments_is_normalized_before_matching() {
    let source = "use perl_parser :: {\n    tdd_basic :: TestGenerator ,\n};\nlet x =\n    perl_parser\n        ::\n        test_runner ;\n";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::tdd_basic".to_string(), "perl_parser::test_runner".to_string()]
    );
}

#[test]
fn comments_cannot_hide_or_create_violations() {
    let hidden = "// use perl_parser::tdd_basic::TestGenerator;\n\
                  /* use perl_parser::{\n       tdd_basic::TestGenerator,\n   }; */\n\
                  let ok = 1;\n";
    assert!(forbidden_facade_references(&code_without_comments(hidden)).is_empty());

    let allowed =
        "use perl_parser::Parser;\n/* perl_parser::tdd_basic */\n// perl_parser::test_runner\n";
    assert!(forbidden_facade_references(&code_without_comments(allowed)).is_empty());
}

#[test]
fn parser_authority_and_canonical_owner_members_remain_allowed() {
    let source = "\
use perl_parser::{Node, NodeKind, Parser, SourceLocation};
use perl_parser::{
    ast::{Node as AstNode, NodeKind},
    error, parser, position,
};
use perl_tdd_support::{
    tdd_basic::TestGenerator,
    test_runner::{TestKind, TestRunner},
};
use proptest::test_runner;
use perl_parser_pest::PureRustPerlParser;
";
    assert!(forbidden_facade_references(&code_without_comments(source)).is_empty());
}

#[test]
fn boundary_check_rejects_longer_path_prefixes_and_other_heads() {
    let source = "\
mod tdd_basic_helpers;
use perl_parser::tdd_basic_local::Thing;
use perl_parser::test_runners_local::Other;
use my_perl_parser::tdd_basic::Wrong;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert!(hits.is_empty(), "unexpected boundary hits: {hits:?}");
}

#[test]
fn aliased_facade_head_imports_are_rejected() {
    let source = "\
use perl_parser as parser_facade;
use parser_facade::tdd_basic::TestGenerator;
use parser_facade::{tdd_workflow::TddWorkflow, test_runner::TestRunner};
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec![
            "perl_parser::tdd_basic".to_string(),
            "perl_parser::tdd_workflow".to_string(),
            "perl_parser::test_runner".to_string(),
        ]
    );
}

#[test]
fn aliased_compat_escape_hatch_is_rejected() {
    let source = "\
use perl_parser::compat as legacy;
use legacy::test_generator::TestGenerator;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::compat::test_generator".to_string()]);
}

#[test]
fn chained_facade_aliases_resolve_within_bounded_passes() {
    let source = "\
use perl_parser as pf;
use pf::test_runner as tr;
let runner = tr::TestRunner::default();
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(
        hits,
        vec!["perl_parser::TestRunner".to_string(), "perl_parser::test_runner".to_string(),]
    );
}

#[test]
fn braced_member_renames_of_governed_segments_register_aliases() {
    let source = "\
use perl_parser::{tdd_basic as tb, Parser};
use tb::TestGenerator;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::tdd_basic".to_string()]);
}

#[test]
fn foreign_roots_and_shadowing_names_never_register_or_flag() {
    let source = "\
use other_crate as parser_like;
use parser_like::tdd_basic::TestGenerator;
use unrelated::stuff as tb;
let tb_value = 1;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert!(hits.is_empty(), "unexpected alias hits: {hits:?}");
}

#[test]
fn string_literals_no_longer_hide_governed_imports() {
    let block_in_string = "let marked = \"/*\";\nuse perl_parser::tdd::WorkflowState;\n";
    let hits = forbidden_facade_references(&code_without_comments(block_in_string));
    assert_eq!(hits, vec!["perl_parser::tdd".to_string()]);

    let raw_string_block =
        "let raw_open = r#\"/*\"#;\nuse perl_parser::test_generator::TestGenerator;\n";
    let hits = forbidden_facade_references(&code_without_comments(raw_string_block));
    assert_eq!(hits, vec!["perl_parser::test_generator".to_string()]);

    let slash_comment_literal =
        "let separator = \"//\";\nuse perl_parser::test_runner::TestRunner;\n";
    let hits = forbidden_facade_references(&code_without_comments(slash_comment_literal));
    assert_eq!(hits, vec!["perl_parser::test_runner".to_string()]);
}

#[test]
fn literals_and_lifetimes_stay_text_without_corrupting_the_scan() {
    let source = "\
let escaped_quote = '\'';
let grade = 'B';
let reborrow: &'static str = \"safe\";
let raw_ident = r#struct;
let bytes = b\"bytes\";
let byte_char = b'x';
use perl_parser::test_runner::TestRunner;
";
    let hits = forbidden_facade_references(&code_without_comments(source));
    assert_eq!(hits, vec!["perl_parser::test_runner".to_string()]);
}
