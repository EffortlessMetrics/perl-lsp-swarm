//! Discovery of the three surfaces the ownership matrix is joined against.
//!
//! Discovery is source-derived rather than hand-listed so a newly added request
//! shows up here without anyone remembering to extend a list. The direction
//! registry stays the classification authority; this module only reads it.

use super::model::{Discovered, RegistryKind, Violation};
use color_eyre::eyre::{Result, WrapErr};
use std::collections::BTreeMap;
use std::path::Path;

/// Call shapes that emit a server-initiated request.
const REQUEST_TRIGGERS: &[&str] = &[".send_request(", ".send_request_internal("];

/// Return the top-level, comma-separated arguments that follow an already
/// consumed `(`, stopping at its matching `)`.
fn top_level_args(after_open_paren: &str) -> Vec<&str> {
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
                return args;
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                args.push(&after_open_paren[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    args
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

/// Remove `#[cfg(test)] mod name { .. }` blocks so test-only sends are never
/// reported as production emission.
fn strip_test_modules(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;

    while let Some(offset) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..offset]);
        let tail = &rest[offset..];
        let Some(brace) = tail.find('{') else {
            // No block follows; keep nothing further from this point.
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
            None => return out,
        }
    }
    out.push_str(rest);
    out
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
pub(super) fn parse_direction_registry(source: &str) -> BTreeMap<String, RegistryKind> {
    let mut out = BTreeMap::new();
    let Some(start) = source.find("REGISTRY: &[MethodDescriptor] = &[") else {
        return out;
    };
    let body = &source[start..];

    // `s2c` is server-to-client by construction, `c2s` client-to-server, and
    // `ext` names its direction in an argument.
    for (name, implied) in [("s2c(", Some(false)), ("c2s(", Some(true)), ("ext(", None)] {
        let mut from = 0usize;
        while let Some(offset) = body[from..].find(name) {
            let open = from + offset + name.len();
            // Reject a match that is part of a longer identifier.
            let preceding = body[..from + offset].chars().next_back();
            if preceding.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                from = open;
                continue;
            }
            let args = top_level_args(&body[open..]);
            from = open;
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
/// Returns the discovered method -> emitting paths map plus any call site whose
/// method argument could not be resolved. An unresolved site is a finding, not
/// a silent skip: an unreadable emission surface must not read as "no emitter".
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
        let stripped = strip_test_modules(&source);
        let lines: Vec<&str> = stripped.lines().collect();

        for (index, line) in lines.iter().enumerate() {
            // A call may wrap; join the next two lines so its arguments are visible.
            let mut window = String::from(*line);
            for follow in lines.iter().skip(index + 1).take(2) {
                window.push(' ');
                window.push_str(follow);
            }

            for trigger in REQUEST_TRIGGERS {
                let mut from = 0usize;
                while let Some(offset) = line[from..].find(trigger) {
                    let open = from + offset + trigger.len();
                    from = open;
                    let args = top_level_args(&window[open.min(window.len())..]);
                    if args.is_empty() {
                        continue;
                    }

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

                    match resolved {
                        Some(method) => {
                            let entry = emitted.entry(method).or_default();
                            if !entry.contains(&relative) {
                                entry.push(relative.clone());
                            }
                        }
                        None if args.iter().all(|arg| plain_identifier(arg).is_some()) => {
                            // Pure forwarding plumbing (`send_request(method, params)`).
                        }
                        None => violations.push(Violation::new(
                            "emission-unresolved",
                            relative.clone(),
                            "a server-request send site's method argument could not be resolved; \
                             emission discovery is incomplete and cannot be read as complete",
                        )),
                    }
                }
            }
        }
    }

    Ok((emitted, violations))
}

/// Collect `features.toml` rows declaring `direction = "server_to_client"`,
/// mapped to their declared `spec`.
pub(super) fn parse_feature_catalog(source: &str) -> BTreeMap<String, String> {
    let mut rows = BTreeMap::new();
    for block in source.split("[[feature]]") {
        if !block.contains("direction = \"server_to_client\"") {
            continue;
        }
        let mut id = None;
        let mut spec = String::new();
        for line in block.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("id = \"") {
                id = rest.strip_suffix('"').map(str::to_string);
            } else if let Some(rest) = trimmed.strip_prefix("spec = \"") {
                if let Some(value) = rest.strip_suffix('"') {
                    spec = value.to_string();
                }
            }
        }
        if let Some(id) = id {
            rows.insert(id, spec);
        }
    }
    rows
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
            catalog_rows: parse_feature_catalog(&catalog_source),
        },
        violations,
    ))
}
