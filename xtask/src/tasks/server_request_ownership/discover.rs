//! Discovery of the three surfaces the ownership matrix is joined against.
//!
//! Discovery is source-derived rather than hand-listed so a newly added request
//! shows up here without anyone remembering to extend a list. The direction
//! registry stays the classification authority; this module only reads it.
//!
//! Every reader here is written to fail closed. A call it cannot parse, a
//! forwarding site it cannot attribute to a declared forwarding signature, and
//! a catalog it cannot deserialize are all findings — never silent skips.

use super::model::{CatalogRow, Discovered, RegistryKind, Violation};
use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Method names whose call emits a server-initiated request.
const REQUEST_SENDERS: &[&str] = &["send_request", "send_request_internal"];

/// A function whose signature takes the method from its caller is the only
/// admitted forwarding shape. Anything else that fails to resolve is a finding.
///
/// The parameter is matched by name and type, not as a substring: testing for
/// `"method: &str"` inside the signature also matched `other_method: &str`,
/// which forwards nothing of the kind.
const FORWARDED_METHOD_PARAM: &str = "method";

/// Return the top-level, comma-separated arguments that follow an already
/// consumed `(`, stopping at its matching `)`.
///
/// Returns `None` when the matching `)` is not found. A truncated argument list
/// must never be mistaken for a complete one: that is how a wrapped call
/// silently leaves the denominator.
fn top_level_args(after_open_paren: &str) -> Option<Vec<&str>> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in after_open_paren.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => {
                args.push(&after_open_paren[start..index]);
                return Some(args);
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                args.push(&after_open_paren[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    None
}

/// Extract the string literal an argument consists of, if it is one.
fn string_literal(arg: &str) -> Option<String> {
    let trimmed = arg.trim();
    let inner = trimmed.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains('"') { None } else { Some(inner.to_string()) }
}

/// The identifier starting at `from`, if any.
fn identifier_at(source: &str, from: usize) -> &str {
    let rest = &source[from..];
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    &rest[..end]
}

/// Byte index just past the `(` that opens a call whose callee ends at `from`.
///
/// Rust permits whitespace and comments between a callee and its argument list.
/// Both callers blank comments to spaces before this runs, so skipping
/// whitespace covers both: `self.send_request ("m", p)` and
/// `self.send_request /* why */ ("m", p)` reach the same `(`.
fn call_open_paren(source: &str, from: usize) -> Option<usize> {
    let rest = &source[from..];
    let offset = rest.find(|c: char| !c.is_whitespace())?;
    rest[offset..].starts_with('(').then_some(from + offset + 1)
}

/// Leading identifier of an argument, when the argument is exactly one.
fn plain_identifier(arg: &str) -> Option<&str> {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(trimmed)
    } else {
        None
    }
}

/// Split a `cfg` predicate list on top-level commas.
fn split_predicates(list: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, ch) in list.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&list[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if !list[start..].trim().is_empty() {
        out.push(&list[start..]);
    }
    out
}

/// Whether a `cfg` predicate can only hold under `cfg(test)`.
///
/// `all(test, feature = "x")` is test-only — it cannot compile in production —
/// so its items carry no production emission. `any(test, feature = "x")` can,
/// so its items must stay in the denominator. Anything else, `not(..)` and bare
/// feature gates included, is treated as reachable: over-stripping would shrink
/// the denominator silently, which is the failure direction to avoid.
fn cfg_is_test_only(predicate: &str) -> bool {
    let predicate = predicate.trim();
    if predicate == "test" {
        return true;
    }
    if let Some(inner) = predicate.strip_prefix("all(").and_then(|rest| rest.strip_suffix(')')) {
        return split_predicates(inner).iter().any(|part| cfg_is_test_only(part));
    }
    if let Some(inner) = predicate.strip_prefix("any(").and_then(|rest| rest.strip_suffix(')')) {
        let parts = split_predicates(inner);
        return !parts.is_empty() && parts.iter().all(|part| cfg_is_test_only(part));
    }
    false
}

/// Remove every test-only `#[cfg(..)]`-gated item so test-only sends are never
/// reported as production emission.
///
/// The contract is deliberately broader than modules: any item behind a
/// test-only gate must not count, whether it is a `mod`, a helper `fn`, or an
/// `impl`. Both the block form `#[cfg(test)] item { .. }` and the brace-less
/// form `#[cfg(test)] mod name;` are handled — the brace-less one is the common
/// case here, with 18 in the scan root, and jumping to the next `{` anywhere
/// later in the file would delete unrelated production code with it.
///
/// Matching the literal `#[cfg(test)]` alone left `#[cfg(all(test, ..))]`
/// contributing production emission, so the predicate is now read and judged.
///
/// This is a bounded reader, not a Rust parser: an attribute appearing inside a
/// string literal would be treated as real. Comments and strings are blanked
/// before this runs, and the failure direction is over-stripping, which shows
/// up as a missing emitter rather than a silent pass.
fn strip_test_gated_items(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;

    while let Some(offset) = rest.find("#[cfg(") {
        let tail = &rest[offset..];
        let open = offset + "#[cfg(".len();

        // The predicate runs to the `(`'s matching `)`.
        let Some(predicate) = top_level_args(&rest[open..]).map(|args| args.join(",")) else {
            out.push_str(rest);
            return out;
        };
        if !cfg_is_test_only(&predicate) {
            // Keep the attribute and resume after it, so a production gate is
            // never mistaken for the start of a test region.
            out.push_str(&rest[..open]);
            rest = &rest[open..];
            continue;
        }
        out.push_str(&rest[..offset]);

        let brace = tail.find('{');
        let semicolon = tail.find(';');

        // `mod name;` — drop only the declaration itself.
        if let Some(semicolon) = semicolon
            && brace.is_none_or(|brace| semicolon < brace)
        {
            rest = &tail[semicolon + 1..];
            continue;
        }

        let Some(brace) = brace else {
            // Neither form: keep the remainder rather than discarding it.
            out.push_str(tail);
            return out;
        };
        let mut depth = 0i32;
        let mut end = None;
        for (index, ch) in tail[brace..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(brace + index + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(end) => rest = &tail[end..],
            None => {
                // Unbalanced: keep the remainder rather than silently losing it.
                out.push_str(tail);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Length of a raw string literal starting at `at`, if one does.
///
/// Handles `r"…"`, `r#"…"#`, and the `b`-prefixed byte forms.
fn raw_string_len(bytes: &[u8], at: usize) -> Option<usize> {
    let mut cursor = at;
    if bytes.get(cursor) == Some(&b'b') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'r') {
        return None;
    }
    cursor += 1;
    let hashes = bytes[cursor..].iter().take_while(|byte| **byte == b'#').count();
    cursor += hashes;
    if bytes.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;
    // The terminator is `"` followed by exactly as many `#` as opened it.
    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && bytes[cursor + 1..].iter().take(hashes).filter(|byte| **byte == b'#').count()
                == hashes
        {
            return Some(cursor + 1 + hashes - at);
        }
        cursor += 1;
    }
    // Unterminated: consume the remainder rather than resuming mid-literal.
    Some(bytes.len() - at)
}

/// Length of a character literal starting at `at`, if one does.
///
/// A lone `'` is far more often a lifetime, so anything that does not close is
/// left as code. The point of recognising the literal at all is `'"'`, which
/// would otherwise open a phantom string.
fn char_literal_len(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'\'') {
        return None;
    }
    let escaped = bytes.get(at + 1) == Some(&b'\\');
    let limit = if escaped { 12 } else { 4 };
    let start = if escaped { at + 2 } else { at + 1 };
    let end = (at + limit).min(bytes.len());
    for (offset, byte) in bytes[start..end].iter().enumerate() {
        if *byte == b'\n' {
            return None;
        }
        if *byte == b'\'' {
            return Some(start + offset + 1 - at);
        }
    }
    None
}

/// Blank every comment, and report which byte offsets are code.
///
/// String and character literal *contents* are preserved, because the method a
/// send site names is read back out of them — but they are marked non-code, so
/// sender-shaped text quoted inside a string can never register as a call site.
/// Comments are blanked outright and marked non-code too, nested block comments
/// included.
///
/// Both directions matter. A doc comment naming a send helper would inflate the
/// denominator; a *removed* request still quoted in a comment or a string would
/// keep its ownership row alive and the gate green, which is the worse of the
/// two. Byte offsets are preserved throughout so call positions stay valid.
fn blank_comments(source: &str) -> (String, Vec<bool>) {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut code = vec![true; bytes.len()];
    let mut at = 0usize;

    macro_rules! blank {
        ($index:expr) => {{
            let index = $index;
            if out[index] != b'\n' {
                out[index] = b' ';
            }
            code[index] = false;
        }};
    }

    while at < bytes.len() {
        // Line comment.
        if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'/') {
            while at < bytes.len() && bytes[at] != b'\n' {
                blank!(at);
                at += 1;
            }
            continue;
        }
        // Block comment, nesting as Rust does.
        if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
            let mut depth = 0usize;
            while at < bytes.len() {
                if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
                    depth += 1;
                    blank!(at);
                    blank!(at + 1);
                    at += 2;
                } else if bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/') {
                    blank!(at);
                    blank!(at + 1);
                    at += 2;
                    if depth <= 1 {
                        break;
                    }
                    depth -= 1;
                } else {
                    blank!(at);
                    at += 1;
                }
            }
            continue;
        }
        // Raw and byte-raw strings, which do not process escapes.
        if let Some(len) = raw_string_len(bytes, at) {
            let end = (at + len).min(bytes.len());
            code[(at + 1)..end].fill(false);
            at += len;
            continue;
        }
        // Character literal, recognised only so `'"'` cannot open a string.
        if let Some(len) = char_literal_len(bytes, at) {
            let end = (at + len).min(bytes.len());
            code[(at + 1)..end].fill(false);
            at += len;
            continue;
        }
        // Ordinary and byte strings.
        let quote = bytes[at] == b'"' || (bytes[at] == b'b' && bytes.get(at + 1) == Some(&b'"'));
        if quote {
            at += usize::from(bytes[at] == b'b') + 1;
            while at < bytes.len() {
                if bytes[at] == b'\\' {
                    code[at] = false;
                    code[(at + 1).min(bytes.len() - 1)] = false;
                    at += 2;
                    continue;
                }
                if bytes[at] == b'"' {
                    at += 1;
                    break;
                }
                code[at] = false;
                at += 1;
            }
            continue;
        }
        at += 1;
    }

    // Only ASCII comment bytes were replaced, each by one ASCII byte.
    let blanked = String::from_utf8(out).unwrap_or_else(|_| source.to_string());
    (blanked, code)
}

/// Whether one signature parameter is the forwarded method: named exactly
/// `method`, typed `&str`.
fn declares_forwarded_method(parameter: &str) -> bool {
    let Some((name, ty)) = parameter.split_once(':') else { return false };
    let name = name.trim().strip_prefix("mut ").unwrap_or(name.trim()).trim();
    name == FORWARDED_METHOD_PARAM && ty.trim() == "&str"
}

/// One `fn` declaration: where it starts, its name, and whether it takes the
/// method from its caller.
struct FnDecl {
    offset: usize,
    name: String,
    forwards_method: bool,
}

/// Collect every `fn` declaration with its signature disposition.
fn fn_declarations(source: &str) -> Vec<FnDecl> {
    let mut decls = Vec::new();
    let mut from = 0usize;

    while let Some(found) = source[from..].find("fn ") {
        let start = from + found;
        let preceding = source[..start].chars().next_back();
        // Skip a match inside a longer identifier (e.g. `into_fn `).
        if preceding.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            from = start + 3;
            continue;
        }
        let after = &source[start + 3..];
        let name: String =
            after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let forwards_method = after
            .find('(')
            .and_then(|open| top_level_args(&after[open + 1..]))
            .is_some_and(|args| args.iter().any(|arg| declares_forwarded_method(arg)));

        if !name.is_empty() {
            decls.push(FnDecl { offset: start, name, forwards_method });
        }
        from = start + 3;
    }
    decls
}

/// The declaration a byte offset sits inside: the last one declared before it.
fn enclosing_fn(decls: &[FnDecl], offset: usize) -> Option<&FnDecl> {
    decls.iter().rev().find(|decl| decl.offset < offset)
}

/// Parse `pub const NAME: &str = "value";` declarations into a lookup table.
fn method_constants(source: &str) -> BTreeMap<String, String> {
    let mut constants = BTreeMap::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub const ") else { continue };
        let Some((name, value)) = rest.split_once(": &str = \"") else { continue };
        let Some(value) = value.strip_suffix("\";") else { continue };
        constants.insert(name.trim().to_string(), value.to_string());
    }
    constants
}

/// Read the `REGISTRY` table out of `method_direction.rs`.
///
/// Entries are `c2s(..)`, `s2c(..)`, or `ext(..)` constructor calls; `s2c` is
/// server-to-client by construction and `ext` names its direction explicitly.
/// Parsing stops at the table's closing `];` so a later test fixture in the
/// same file cannot inject phantom rows.
pub(super) fn parse_direction_registry(
    source: &str,
    constants: &BTreeMap<String, String>,
) -> (BTreeMap<String, RegistryKind>, Vec<Violation>) {
    let mut out = BTreeMap::new();
    let mut violations = Vec::new();
    // A commented-out or quoted `s2c(..)` is not a classification.
    let (source, is_code) = blank_comments(source);
    let source = source.as_str();
    let Some(start) = source.find("REGISTRY: &[MethodDescriptor] = &[") else {
        return (out, violations);
    };
    let open = start + "REGISTRY: &[MethodDescriptor] = &[".len();
    // Bound the scan to the table literal itself.
    let mut depth = 0i32;
    let mut end = source.len();
    for (index, ch) in source[open..].char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ')' => depth -= 1,
            ']' if depth == 0 => {
                end = open + index;
                break;
            }
            ']' => depth -= 1,
            _ => {}
        }
    }
    let body = &source[open..end];

    for (name, implied) in [("s2c", Some(false)), ("c2s", Some(true)), ("ext", None)] {
        let mut from = 0usize;
        while let Some(offset) = body[from..].find(name) {
            let start = from + offset;
            let after_name = start + name.len();
            from = after_name;

            // Reject a match inside a longer identifier in either direction,
            // or one that is not code at all.
            let preceding = body[..start].chars().next_back();
            if preceding.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                || !identifier_at(body, after_name).is_empty()
                || !is_code.get(open + start).copied().unwrap_or(false)
            {
                continue;
            }
            let Some(call_open) = call_open_paren(body, after_name) else { continue };
            let Some(args) = top_level_args(&body[call_open..]) else {
                violations.push(Violation::new(
                    "registry-entry-unresolved",
                    "<registry>",
                    format!("a `{name}(..)` registry entry has no matching `)`"),
                ));
                continue;
            };

            // A constant-named entry is resolved, not skipped: dropping it
            // would quietly shrink the coverage denominator so a newly
            // classified request would need no row.
            let method = match args.first().and_then(|arg| {
                string_literal(arg).or_else(|| {
                    plain_identifier(arg).and_then(|ident| constants.get(ident).cloned())
                })
            }) {
                Some(method) => method,
                None => {
                    violations.push(Violation::new(
                        "registry-entry-unresolved",
                        "<registry>",
                        format!(
                            "a `{name}(..)` registry entry names no resolvable method; the \
                             classification denominator is incomplete"
                        ),
                    ));
                    continue;
                }
            };

            let notification = args.iter().any(|arg| arg.contains("EnvelopeKind::Notification"));
            let client_to_server = match implied {
                Some(is_c2s) => is_c2s,
                None => args.iter().any(|arg| arg.contains("MethodDirection::ClientToServer")),
            };

            let kind = if client_to_server {
                RegistryKind::ClientToServer
            } else if notification {
                RegistryKind::ServerToClientNotification
            } else {
                RegistryKind::ServerToClientRequest
            };
            out.insert(method, kind);
        }
    }
    (out, violations)
}

/// The set of call names in one file whose invocation emits a server request:
/// the sender primitives, plus every `method: &str` helper whose own body
/// reaches one of them, closed transitively.
fn forwarding_closure<'a>(source: &'a str, decls: &'a [FnDecl]) -> BTreeSet<&'a str> {
    let mut senders: BTreeSet<&str> = REQUEST_SENDERS.iter().copied().collect();
    let candidates: Vec<&FnDecl> = decls.iter().filter(|decl| decl.forwards_method).collect();

    // A forwarder may call another forwarder declared later in the file, so
    // repeat until no further name joins.
    loop {
        let mut grew = false;
        for decl in &candidates {
            if senders.contains(decl.name.as_str()) {
                continue;
            }
            let body = fn_body(source, decls, decl);
            if senders.iter().any(|sender| {
                body.contains(&format!(".{sender}")) || body.contains(&format!("::{sender}"))
            }) {
                senders.insert(decl.name.as_str());
                grew = true;
            }
        }
        if !grew {
            break senders;
        }
    }
}

/// The source between a declaration and the next one. Over-inclusive for a
/// nested `fn`, which can only keep a forwarder rather than drop one.
fn fn_body<'a>(source: &'a str, decls: &[FnDecl], decl: &FnDecl) -> &'a str {
    let end = decls
        .iter()
        .map(|other| other.offset)
        .filter(|offset| *offset > decl.offset)
        .min()
        .unwrap_or(source.len());
    &source[decl.offset..end]
}

/// Scan production runtime sources for server-request emission call sites.
///
/// Each discovered site is attributed to the function that contains it, so the
/// matrix can be required to cite the exact emitting symbol. A call whose
/// arguments cannot be parsed to their matching `)`, or whose method cannot be
/// resolved outside a declared forwarding signature, is a finding.
pub(super) fn scan_emission(
    repo_root: &Path,
    scan_root: &str,
    constants: &BTreeMap<String, String>,
) -> Result<(BTreeMap<String, Vec<String>>, BTreeSet<String>, Vec<Violation>)> {
    let mut emitted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ambiguous: BTreeSet<String> = BTreeSet::new();
    let mut violations = Vec::new();

    let root = repo_root.join(scan_root);
    let mut files: Vec<_> = walkdir::WalkDir::new(&root)
        .sort_by_file_name()
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();

    for path in files {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        // Whole-file test modules carry no production emission.
        if name.ends_with("_tests.rs") {
            continue;
        }
        let relative = path
            .strip_prefix(repo_root)
            .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
        let source = std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("reading emission source {relative}"))?;
        // Blank first, strip second: an attribute or a `mod` brace quoted in a
        // comment must not steer the test-gate stripper either.
        let (blanked, _) = blank_comments(&source);
        let (stripped, is_code) = blank_comments(&strip_test_gated_items(&blanked));
        let decls = fn_declarations(&stripped);

        // Record symbol names declared more than once in this file; attributing
        // an emitter to such a name cannot express distinct ownership.
        let mut seen_names: BTreeSet<&str> = BTreeSet::new();
        for decl in &decls {
            if !seen_names.insert(decl.name.as_str()) {
                ambiguous.insert(format!("{relative}#{}", decl.name));
            }
        }

        // A helper that declares `method: &str` *and* reaches a sender forwards
        // a caller-supplied method, so its own callers are send sites too.
        // Requiring the body keeps a new wrapper from hiding a concrete method
        // behind an exempt callee without promoting every helper that merely
        // inspects a method name — `is_lifecycle_method`, `record_latency` — to
        // a request emitter and inventing phantom requests from its callers.
        let senders = forwarding_closure(&stripped, &decls);

        // `self.send_request(..)` and `Type::send_request(..)` are the same
        // emission; matching only the method-call form let the associated
        // function form leave discovery entirely.
        for (sender, separator) in senders.iter().flat_map(|sender| [(sender, "."), (sender, "::")])
        {
            let needle = format!("{separator}{sender}");
            let mut from = 0usize;
            while let Some(offset) = stripped[from..].find(&needle) {
                let trigger = from + offset;
                let at = trigger + separator.len();
                let name = identifier_at(&stripped, at);
                from = at + name.len().max(1);
                if name != *sender {
                    continue;
                }
                // Sender-shaped text inside a string literal is not a call.
                if !is_code.get(trigger).copied().unwrap_or(false) {
                    continue;
                }
                let Some(open) = call_open_paren(&stripped, at + name.len()) else { continue };
                from = open;

                // Parse to the matching `)` over the whole remaining source, so
                // a call spread across any number of lines is still complete.
                let Some(args) = top_level_args(&stripped[open..]) else {
                    violations.push(Violation::new(
                        "emission-unresolved",
                        relative.clone(),
                        "a server-request send site's argument list has no matching `)`; \
                         emission discovery is incomplete and cannot be read as complete",
                    ));
                    continue;
                };

                // The method is the first argument that is a literal or a
                // resolvable protocol constant; ids and params may precede it.
                let resolved = args.iter().find_map(|arg| {
                    string_literal(arg).or_else(|| {
                        plain_identifier(arg)
                            .filter(|ident| {
                                ident.len() > 3
                                    && ident.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                            })
                            .and_then(|ident| constants.get(ident).cloned())
                    })
                });

                let symbol = enclosing_fn(&decls, open);
                match resolved {
                    Some(method) => {
                        let owner = symbol.map_or("<unknown>", |decl| decl.name.as_str());
                        let reference = format!("{relative}#{owner}");
                        let entry = emitted.entry(method).or_default();
                        if !entry.contains(&reference) {
                            entry.push(reference);
                        }
                    }
                    // The only admitted unresolved shape is a forwarder
                    // passing along its own caller-supplied parameter. Keying
                    // the exemption on the signature alone let a forwarder send
                    // any other expression — a computed or remapped method —
                    // with no row and no finding.
                    None if symbol.is_some_and(|decl| decl.forwards_method)
                        && args.iter().any(|arg| arg.trim() == FORWARDED_METHOD_PARAM) => {}
                    None => violations.push(Violation::new(
                        "emission-unresolved",
                        relative.clone(),
                        format!(
                            "a server-request send site in `{}` names no resolvable method and \
                             does not pass a declared `{FORWARDED_METHOD_PARAM}: &str` parameter",
                            symbol.map_or("<unknown>", |decl| decl.name.as_str())
                        ),
                    )),
                }
            }
        }
    }

    for paths in emitted.values_mut() {
        paths.sort();
        paths.dedup();
    }
    Ok((emitted, ambiguous, violations))
}

/// Minimal typed view of `features.toml`. Unknown fields are ignored; the
/// catalog carries many columns this join does not consume.
#[derive(Debug, Deserialize)]
struct FeatureCatalog {
    #[serde(default)]
    feature: Vec<FeatureRow>,
}

#[derive(Debug, Deserialize)]
struct FeatureRow {
    id: String,
    #[serde(default)]
    spec: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    advertised: bool,
    #[serde(default)]
    maturity: String,
    #[serde(default)]
    state_owner: String,
}

/// Collect `features.toml` rows declaring `direction = "server_to_client"`,
/// mapped to the fields this join consumes.
///
/// Parsed as real TOML: a substring scan would classify a row whose prose
/// merely quotes the direction key, and would drop a real row whose spelling or
/// spacing differs.
pub(super) fn parse_feature_catalog(source: &str) -> Result<BTreeMap<String, CatalogRow>> {
    let catalog: FeatureCatalog =
        toml::from_str(source).wrap_err("parsing the feature catalog as TOML")?;
    Ok(catalog
        .feature
        .into_iter()
        .filter(|row| row.direction == "server_to_client")
        .map(|row| {
            (
                row.id,
                CatalogRow {
                    spec: row.spec,
                    advertised: row.advertised,
                    maturity: row.maturity,
                    state_owner: row.state_owner,
                },
            )
        })
        .collect())
}

/// Join all three surfaces.
pub(super) fn discover(
    repo_root: &Path,
    direction_registry: &str,
    feature_catalog: &str,
    emission_scan_root: &str,
) -> Result<(Discovered, Vec<Violation>)> {
    let registry_source = std::fs::read_to_string(repo_root.join(direction_registry))
        .wrap_err_with(|| format!("reading direction registry {direction_registry}"))?;
    let catalog_source = std::fs::read_to_string(repo_root.join(feature_catalog))
        .wrap_err_with(|| format!("reading feature catalog {feature_catalog}"))?;
    let constants_source =
        std::fs::read_to_string(repo_root.join("crates/perl-lsp-rs-core/src/protocol/methods.rs"))
            .wrap_err("reading protocol method constants")?;

    let constants = method_constants(&constants_source);
    let (emitted, ambiguous_symbols, violations) =
        scan_emission(repo_root, emission_scan_root, &constants)?;

    let (registry, mut registry_findings) = parse_direction_registry(&registry_source, &constants);
    registry_findings.extend(violations);

    Ok((
        Discovered {
            registry,
            emitted,
            catalog_rows: parse_feature_catalog(&catalog_source)?,
            ambiguous_symbols,
        },
        registry_findings,
    ))
}
