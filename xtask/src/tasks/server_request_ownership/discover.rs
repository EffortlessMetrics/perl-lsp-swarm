//! Discovery of the three surfaces the ownership matrix is joined against.
//!
//! Discovery is source-derived rather than hand-listed so a newly added request
//! shows up here without anyone remembering to extend a list. The direction
//! registry stays the classification authority; this module only reads it.
//!
//! Every reader here is written to fail closed. A call it cannot parse, a
//! forwarding site it cannot attribute to a declared forwarding signature, and
//! a catalog it cannot deserialize are all findings — never silent skips.

use super::model::{Discovered, RegistryKind, Violation};
use color_eyre::eyre::{Result, WrapErr};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Call shapes that emit a server-initiated request.
const REQUEST_TRIGGERS: &[&str] = &[".send_request(", ".send_request_internal("];

/// A function whose signature takes the method from its caller is the only
/// admitted forwarding shape. Anything else that fails to resolve is a finding.
const FORWARDED_METHOD_PARAM: &str = "method: &str";

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

/// Remove test-only modules so their sends are never reported as production
/// emission.
///
/// Both `#[cfg(test)] mod name { .. }` and the brace-less `#[cfg(test)] mod
/// name;` form are handled. The brace-less form is the common one here — the
/// scan root carries 18 of them — and jumping to the next `{` anywhere later
/// in the file would delete unrelated production code along with it.
fn strip_test_modules(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;

    while let Some(offset) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..offset]);
        let tail = &rest[offset..];
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

/// Blank out whole-line comments, preserving byte offsets so call positions
/// stay valid. This keeps a doc comment that mentions a send helper from
/// registering as an emission site.
fn blank_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                " ".repeat(line.len())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
            .is_some_and(|args| args.iter().any(|arg| arg.contains(FORWARDED_METHOD_PARAM)));

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
pub(super) fn parse_direction_registry(source: &str) -> BTreeMap<String, RegistryKind> {
    let mut out = BTreeMap::new();
    let Some(start) = source.find("REGISTRY: &[MethodDescriptor] = &[") else {
        return out;
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

    for (name, implied) in [("s2c(", Some(false)), ("c2s(", Some(true)), ("ext(", None)] {
        let mut from = 0usize;
        while let Some(offset) = body[from..].find(name) {
            let call_open = from + offset + name.len();
            let preceding = body[..from + offset].chars().next_back();
            if preceding.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                from = call_open;
                continue;
            }
            from = call_open;
            let Some(args) = top_level_args(&body[call_open..]) else { continue };
            let Some(method) = args.first().and_then(|arg| string_literal(arg)) else { continue };

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
    out
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
) -> Result<(BTreeMap<String, Vec<String>>, Vec<Violation>)> {
    let mut emitted: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
        let stripped = blank_line_comments(&strip_test_modules(&source));
        let decls = fn_declarations(&stripped);

        for trigger in REQUEST_TRIGGERS {
            let mut from = 0usize;
            while let Some(offset) = stripped[from..].find(trigger) {
                let open = from + offset + trigger.len();
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
                    // The only admitted unresolved shape is a function that
                    // declares the method as its own caller-supplied parameter.
                    None if symbol.is_some_and(|decl| decl.forwards_method) => {}
                    None => violations.push(Violation::new(
                        "emission-unresolved",
                        relative.clone(),
                        format!(
                            "a server-request send site in `{}` names no resolvable method and \
                             is not a declared `{FORWARDED_METHOD_PARAM}` forwarder",
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
    Ok((emitted, violations))
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
}

/// Collect `features.toml` rows declaring `direction = "server_to_client"`,
/// mapped to their declared `spec`.
///
/// Parsed as real TOML: a substring scan would classify a row whose prose
/// merely quotes the direction key, and would drop a real row whose spelling or
/// spacing differs.
pub(super) fn parse_feature_catalog(source: &str) -> Result<BTreeMap<String, String>> {
    let catalog: FeatureCatalog =
        toml::from_str(source).wrap_err("parsing the feature catalog as TOML")?;
    Ok(catalog
        .feature
        .into_iter()
        .filter(|row| row.direction == "server_to_client")
        .map(|row| (row.id, row.spec))
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
    let (emitted, violations) = scan_emission(repo_root, emission_scan_root, &constants)?;

    Ok((
        Discovered {
            registry: parse_direction_registry(&registry_source),
            emitted,
            catalog_rows: parse_feature_catalog(&catalog_source)?,
        },
        violations,
    ))
}
