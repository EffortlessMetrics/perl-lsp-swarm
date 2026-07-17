//! Document links provider for Perl LSP protocol compatibility.
//!
//! This crate provides document link detection for Perl source files,
//! identifying `use`, `require` module statements, POD links, and file includes.

use perl_module::import::{ModuleImportKind, RequireForm, parse_module_import_head};
use perl_module::path::module_name_to_path;
use serde_json::{Value, json};
use url::Url;

/// Computes document links for a given Perl document.
///
/// This function scans the text for `use` and `require` statements plus POD
/// `L<>` links and creates document links for them. Links are returned with a
/// `data` field containing metadata for deferred resolution via
/// `documentLink/resolve`.
#[must_use]
pub fn compute_links(uri: &str, text: &str, _roots: &[Url]) -> Vec<Value> {
    let mut out = Vec::new();
    let current_package = current_package_name(text);
    let mut in_pod = false;

    for (i, line) in text.lines().enumerate() {
        if in_pod && line.starts_with("=cut") {
            in_pod = false;
            continue;
        }

        if starts_pod_block(line) {
            in_pod = true;
        }

        if in_pod {
            collect_pod_document_links(uri, i as u32, line, current_package.as_deref(), &mut out);
            continue;
        }

        collect_module_runtime_links(uri, i as u32, line, &mut out);

        if let Some(import) = parse_module_import_head(line) {
            match import.kind {
                ModuleImportKind::Use => {
                    if !is_pragma(import.token)
                        && let Some(link) = make_deferred_module_link(
                            uri,
                            i as u32,
                            import.token,
                            import.token_start as u32,
                            import.token_end as u32,
                        )
                    {
                        out.push(link);
                    }
                }
                ModuleImportKind::Require => {
                    match import.require_form() {
                        Some(RequireForm::FilePath) if import.token.ends_with(".pm") => {
                            // Quoted .pm require → treat as a module link (Foo/Bar.pm → Foo::Bar)
                            let module_name = import.token_as_module_name();
                            if !is_pragma(&module_name) {
                                if let Some(link) = make_deferred_module_link(
                                    uri,
                                    i as u32,
                                    &module_name,
                                    import.token_start as u32,
                                    import.token_end as u32,
                                ) {
                                    out.push(link);
                                }
                            }
                        }
                        Some(RequireForm::FilePath) => {
                            // Quoted file path that is NOT a .pm (e.g. .pl, extensionless) → file link
                            out.push(json!({
                                "range": {
                                    "start": {"line": i as u32, "character": import.token_start as u32},
                                    "end":   {"line": i as u32, "character": import.token_end as u32}
                                },
                                "tooltip": format!("Open {}", import.token),
                                "data": {
                                    "type": "file",
                                    "path": import.token,
                                    "baseUri": uri
                                }
                            }));
                        }
                        Some(RequireForm::ModuleName) | None => {
                            // Bare module name form — existing behavior
                            if import.token.contains("::")
                                && !is_pragma(import.token)
                                && let Some(link) = make_deferred_module_link(
                                    uri,
                                    i as u32,
                                    import.token,
                                    import.token_start as u32,
                                    import.token_end as u32,
                                )
                            {
                                out.push(link);
                            }
                        }
                    }
                }
                ModuleImportKind::UseParent | ModuleImportKind::UseBase => {}
            }
        }
    }
    out
}

fn collect_module_runtime_links(uri: &str, line_number: u32, line: &str, out: &mut Vec<Value>) {
    let code = code_mask(line);
    let mut offset = 0;

    while offset < line.len() {
        let Some(rest) = line.get(offset..) else {
            break;
        };

        if code.get(offset).copied() != Some(true) {
            offset +=
                line.get(offset..).and_then(|rest| rest.chars().next()).map_or(1, char::len_utf8);
            continue;
        }

        let Some(name_end) = match_module_runtime_call(line, offset, &code) else {
            offset += rest.chars().next().map_or(1, char::len_utf8);
            continue;
        };

        if let Some((module, content_start, content_end, next_offset)) =
            parse_module_runtime_argument(line, name_end)
        {
            let start = byte_to_utf16_col(line, content_start);
            let end = byte_to_utf16_col(line, content_end);
            if let Some(link) = make_deferred_module_link(uri, line_number, module, start, end) {
                out.push(link);
            }
            offset = next_offset;
        } else {
            offset = name_end;
        }
    }
}

fn match_module_runtime_call(line: &str, start: usize, code: &[bool]) -> Option<usize> {
    if start > 0
        && line
            .get(..start)
            .and_then(|prefix| prefix.chars().next_back())
            .is_some_and(is_identifier_boundary)
    {
        return None;
    }
    if preceded_by_method_operator(line, start) {
        return None;
    }

    let mut pos = start;
    if line.get(pos..)?.starts_with("Module") {
        pos += "Module".len();
        pos = skip_ascii_whitespace(line, pos);
        if !line.get(pos..)?.starts_with("::") {
            return None;
        }
        pos += 2;
        pos = skip_ascii_whitespace(line, pos);
        if !line.get(pos..)?.starts_with("Runtime") {
            return None;
        }
        pos += "Runtime".len();
        pos = skip_ascii_whitespace(line, pos);
        if !line.get(pos..)?.starts_with("::") {
            return None;
        }
        pos += 2;
        pos = skip_ascii_whitespace(line, pos);
    }

    let name_start = pos;
    if line.get(pos..)?.starts_with("use_module") {
        pos += "use_module".len();
    } else if line.get(pos..)?.starts_with("require_module") {
        pos += "require_module".len();
    } else {
        return None;
    };

    if !is_code_range(code, start, pos)
        || line.get(pos..).and_then(|tail| tail.chars().next()).is_some_and(is_identifier_boundary)
        || (name_start > start
            && start > 0
            && line.get(..start).is_some_and(|prefix| prefix.ends_with(':')))
    {
        return None;
    }

    Some(pos)
}

fn parse_module_runtime_argument(
    line: &str,
    name_end: usize,
) -> Option<(&str, usize, usize, usize)> {
    let open = skip_ascii_whitespace(line, name_end);
    if line.as_bytes().get(open) != Some(&b'(') {
        return None;
    }

    let quote_start = skip_ascii_whitespace(line, open + 1);
    let quote = *line.as_bytes().get(quote_start)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }

    let content_start = quote_start + 1;
    let mut offset = content_start;
    while offset < line.len() {
        let byte = *line.as_bytes().get(offset)?;
        if byte == b'\\' {
            return None;
        }
        if byte == quote {
            let content_end = offset;
            let module = line.get(content_start..content_end)?;
            if !is_perl_module_name(module) {
                return None;
            }

            let close = skip_ascii_whitespace(line, offset + 1);
            if line.as_bytes().get(close) != Some(&b')') {
                return None;
            }

            return Some((module, content_start, content_end, close + 1));
        }
        offset += 1;
    }

    None
}

fn preceded_by_method_operator(line: &str, start: usize) -> bool {
    let bytes = line.as_bytes();
    let mut offset = start;
    while offset > 0 {
        offset -= 1;
        match bytes.get(offset) {
            Some(b' ' | b'\t') => {}
            Some(b'>') if offset > 0 => return bytes.get(offset - 1) == Some(&b'-'),
            _ => return false,
        }
    }
    false
}

fn code_mask(line: &str) -> Vec<bool> {
    let mut code = vec![true; line.len()];
    let bytes = line.as_bytes();
    let mut offset = 0;

    while offset < bytes.len() {
        match bytes[offset] {
            b'#' => {
                code[offset..].fill(false);
                break;
            }
            b'\'' | b'"' => {
                let quote = bytes[offset];
                code[offset] = false;
                offset += 1;
                let mut closed = false;
                while offset < bytes.len() {
                    code[offset] = false;
                    if bytes[offset] == b'\\' {
                        if let Some(next) = code.get_mut(offset + 1) {
                            *next = false;
                        }
                        offset = offset.saturating_add(2);
                    } else if bytes[offset] == quote {
                        offset += 1;
                        closed = true;
                        break;
                    } else {
                        offset += 1;
                    }
                }
                if !closed {
                    break;
                }
            }
            _ => offset += 1,
        }
    }

    code
}

fn is_code_range(code: &[bool], start: usize, end: usize) -> bool {
    code.get(start..end).is_some_and(|range| range.iter().all(|is_code| *is_code))
}

fn skip_ascii_whitespace(line: &str, mut offset: usize) -> usize {
    while line.as_bytes().get(offset).is_some_and(u8::is_ascii_whitespace) {
        offset += 1;
    }
    offset
}

fn is_identifier_boundary(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':')
}

fn is_perl_module_name(module: &str) -> bool {
    !module.is_empty() && module.split("::").all(is_perl_module_segment)
}

enum PodLinkTarget<'a> {
    Module(&'a str),
    Section(&'a str),
}

fn make_deferred_module_link(
    uri: &str,
    line: u32,
    module: &str,
    col_start: u32,
    col_end: u32,
) -> Option<Value> {
    if module.is_empty() || col_start >= col_end {
        return None;
    }

    Some(json!({
        "range": {
            "start": {"line": line, "character": col_start},
            "end": {"line": line, "character": col_end}
        },
        "tooltip": format!("Open {}", module),
        "data": {
            "type": "module",
            "module": module,
            "baseUri": uri
        }
    }))
}

fn make_deferred_pod_section_link(
    uri: &str,
    line: u32,
    section: &str,
    col_start: u32,
    col_end: u32,
) -> Option<Value> {
    if section.is_empty() || col_start >= col_end {
        return None;
    }

    Some(json!({
        "range": {
            "start": {"line": line, "character": col_start},
            "end": {"line": line, "character": col_end}
        },
        "tooltip": format!("Open POD section {}", section),
        "data": {
            "type": "pod_section",
            "section": section,
            "baseUri": uri
        }
    }))
}

fn collect_pod_document_links(
    uri: &str,
    line_number: u32,
    line: &str,
    current_package: Option<&str>,
    out: &mut Vec<Value>,
) {
    let mut search_start = 0;
    while let Some(open_offset) = line[search_start..].find("L<") {
        let content_start = search_start + open_offset + 2;
        let after_open = &line[content_start..];
        let Some(close_offset) = after_open.find('>') else {
            break;
        };

        let content_end = content_start + close_offset;
        let target = after_open[..close_offset].trim();
        let col_start = byte_to_utf16_col(line, content_start);
        let col_end = byte_to_utf16_col(line, content_end);

        match pod_link_target(target) {
            Some(PodLinkTarget::Module(module)) if Some(module) != current_package => {
                if let Some(link) =
                    make_deferred_module_link(uri, line_number, module, col_start, col_end)
                {
                    out.push(link);
                }
            }
            Some(PodLinkTarget::Section(section)) => {
                if let Some(link) =
                    make_deferred_pod_section_link(uri, line_number, section, col_start, col_end)
                {
                    out.push(link);
                }
            }
            _ => {}
        }

        search_start = content_end + 1;
    }
}

fn pod_link_target(target: &str) -> Option<PodLinkTarget<'_>> {
    let candidate = if let Some((label, link_target)) = target.split_once('|') {
        if label.trim().is_empty() {
            return None;
        }
        link_target.trim()
    } else {
        target.trim()
    };

    if let Some(section) = candidate.strip_prefix('/') {
        let section = section.trim();
        if is_pod_section_target(section) {
            return Some(PodLinkTarget::Section(section));
        }
        return None;
    }

    if is_simple_pod_module_target(candidate) {
        Some(PodLinkTarget::Module(candidate))
    } else {
        None
    }
}

fn starts_pod_block(line: &str) -> bool {
    line.starts_with("=head")
        || line.starts_with("=pod")
        || line.starts_with("=over")
        || line.starts_with("=begin")
        || line.starts_with("=for")
        || line.starts_with("=encoding")
        || line.starts_with("=item")
}

fn is_simple_pod_module_target(target: &str) -> bool {
    is_simple_package_pod_target(target) || is_supported_core_pragma_pod_target(target)
}

fn is_simple_package_pod_target(target: &str) -> bool {
    target.contains("::") && target.split("::").all(is_perl_module_segment)
}

fn is_supported_core_pragma_pod_target(target: &str) -> bool {
    matches!(target, "strict" | "warnings")
}

fn is_pod_section_target(section: &str) -> bool {
    !section.is_empty()
        && section.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ' '))
}

fn is_perl_module_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn current_package_name(text: &str) -> Option<String> {
    let mut in_pod = false;

    for line in text.lines() {
        if in_pod && line.starts_with("=cut") {
            in_pod = false;
            continue;
        }

        if starts_pod_block(line) {
            in_pod = true;
            continue;
        }

        if in_pod {
            continue;
        }

        let Some(rest) = line.trim_start().strip_prefix("package ") else {
            continue;
        };
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == ':')
            .collect();
        if name.split("::").all(is_perl_module_segment) {
            return Some(name);
        }
    }
    None
}

fn byte_to_utf16_col(line: &str, byte_offset: usize) -> u32 {
    line.get(..byte_offset)
        .map(|prefix| prefix.encode_utf16().count() as u32)
        .unwrap_or(byte_offset as u32)
}

fn is_pragma(pkg: &str) -> bool {
    matches!(
        pkg,
        "strict"
            | "warnings"
            | "utf8"
            | "bytes"
            | "integer"
            | "feature"
            | "constant"
            | "lib"
            | "vars"
            | "subs"
            | "overload"
            | "parent"
            | "base"
            | "fields"
            | "if"
            | "attributes"
            | "autouse"
            | "autodie"
            | "bigint"
            | "bignum"
            | "bigrat"
            | "blib"
            | "charnames"
            | "diagnostics"
            | "encoding"
            | "filetest"
            | "locale"
            | "open"
            | "ops"
            | "re"
            | "sigtrap"
            | "sort"
            | "threads"
            | "vmsish"
    )
}

#[allow(dead_code)]
fn resolve_pkg(pkg: &str, roots: &[Url]) -> Option<String> {
    let rel = module_name_to_path(pkg);
    if let Some(base) = roots.first() {
        let mut u = base.clone();
        let mut p = u.path().to_string();
        if !p.ends_with('/') {
            p.push('/');
        }
        if let Some(lib_dir) = ["lib/", "blib/lib/", ""].first() {
            let full_path = format!("{}{}{}", p, lib_dir, rel);
            u.set_path(&full_path);
            return Some(u.to_string());
        }
    }
    None
}

#[allow(dead_code)]
fn resolve_file(path: &str, roots: &[Url]) -> Option<String> {
    if let Some(base) = roots.first() {
        let mut u = base.clone();
        let mut p = u.path().to_string();
        if !p.ends_with('/') {
            p.push('/');
        }
        p.push_str(path);
        u.set_path(&p);
        return Some(u.to_string());
    }
    None
}

#[allow(dead_code)]
fn make_link(_src: &str, line: u32, line_text: &str, pkg: &str, target: String) -> Option<Value> {
    if let Some(idx) = line_text.find(pkg) {
        let start = idx as u32;
        let end = (idx + pkg.len()) as u32;
        Some(json!({
            "range": {
                "start": {"line": line, "character": start},
                "end":   {"line": line, "character": end}
            },
            "target": target,
            "tooltip": format!("Open {}", pkg)
        }))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::compute_links;
    use serde_json::Value;

    fn uri() -> &'static str {
        "file:///workspace/test.pl"
    }

    fn data_str<'a>(link: &'a Value, pointer: &str) -> Option<&'a str> {
        link.pointer(pointer).and_then(Value::as_str)
    }

    fn check_module_link(
        link: &Value,
        module: &str,
        line: &str,
        line_number: u64,
    ) -> Result<(), String> {
        if data_str(link, "/data/type") != Some("module")
            || data_str(link, "/data/module") != Some(module)
            || data_str(link, "/data/baseUri") != Some(uri())
        {
            return Err(format!("unexpected module link metadata: {link:?}"));
        }

        let start = line
            .find(module)
            .ok_or_else(|| format!("module {module:?} is absent from source line {line:?}"))?;
        let end = start + module.len();
        let actual_start_line = link.pointer("/range/start/line").and_then(Value::as_u64);
        let actual_start = link.pointer("/range/start/character").and_then(Value::as_u64);
        let actual_end_line = link.pointer("/range/end/line").and_then(Value::as_u64);
        let actual_end = link.pointer("/range/end/character").and_then(Value::as_u64);
        if actual_start_line != Some(line_number)
            || actual_start != Some(start as u64)
            || actual_end_line != Some(line_number)
            || actual_end != Some(end as u64)
        {
            return Err(format!("unexpected module link range: {link:?}"));
        }
        Ok(())
    }

    #[test]
    fn module_runtime_literal_calls_emit_module_links() -> Result<(), String> {
        let text = "use_module('Foo::Bar');\nrequire_module(\"Baz::Qux\");\n";
        let links = compute_links(uri(), text, &[]);

        let [first, second] = links.as_slice() else {
            return Err(format!("expected two links, got {links:?}"));
        };
        check_module_link(first, "Foo::Bar", "use_module('Foo::Bar');", 0)?;
        check_module_link(second, "Baz::Qux", "require_module(\"Baz::Qux\");", 1)
    }

    #[test]
    fn qualified_module_runtime_calls_emit_module_links() -> Result<(), String> {
        let text = "Module::Runtime::use_module(\"Foo::Bar\");\n".to_owned()
            + "Module::Runtime::require_module('Baz::Qux');\n";
        let links = compute_links(uri(), &text, &[]);

        let [first, second] = links.as_slice() else {
            return Err(format!("expected two links, got {links:?}"));
        };
        check_module_link(first, "Foo::Bar", "Module::Runtime::use_module(\"Foo::Bar\");", 0)?;
        check_module_link(second, "Baz::Qux", "Module::Runtime::require_module('Baz::Qux');", 1)
    }

    #[test]
    fn module_runtime_calls_accept_valid_whitespace() -> Result<(), String> {
        let line = "  Module::Runtime::require_module (  'Foo::Bar'  );";
        let links = compute_links(uri(), line, &[]);

        let [link] = links.as_slice() else {
            return Err(format!("expected one link, got {links:?}"));
        };
        check_module_link(link, "Foo::Bar", line, 0)
    }

    #[test]
    fn module_runtime_calls_on_one_line_are_not_dropped() -> Result<(), String> {
        let line = "use_module('Foo::Bar'); require_module(\"Baz::Qux\");";
        let links = compute_links(uri(), line, &[]);

        let [first, second] = links.as_slice() else {
            return Err(format!("expected two links, got {links:?}"));
        };
        check_module_link(first, "Foo::Bar", line, 0)?;
        check_module_link(second, "Baz::Qux", line, 0)
    }

    #[test]
    fn module_runtime_calls_in_comments_are_ignored() -> Result<(), String> {
        let text = "# use_module('Ignored::FullLine');\n".to_owned()
            + "my $value = 1; # require_module(\"Ignored::Trailing\");\n";

        let links = compute_links(uri(), &text, &[]);

        if !links.is_empty() {
            return Err(format!("comments produced module links: {links:?}"));
        }
        Ok(())
    }

    #[test]
    fn dynamic_module_runtime_calls_are_ignored() -> Result<(), String> {
        let text = "use_module($module);\n".to_owned()
            + "require_module(\"Foo\" . \"::Bar\");\n"
            + "Module::Runtime::use_module(\"${name}::Baz\");\n";

        let links = compute_links(uri(), &text, &[]);

        if !links.is_empty() {
            return Err(format!("dynamic module names produced links: {links:?}"));
        }
        Ok(())
    }

    #[test]
    fn module_runtime_call_inside_string_is_ignored() -> Result<(), String> {
        let text = "my $source = \"use_module('No::Link')\"; use_module('Foo::Bar');";
        let links = compute_links(uri(), text, &[]);

        let [link] = links.as_slice() else {
            return Err(format!("quoted source text produced an unexpected result: {links:?}"));
        };
        if data_str(link, "/data/module") != Some("Foo::Bar") {
            return Err(format!("unexpected link from quoted source text: {link:?}"));
        }
        Ok(())
    }

    #[test]
    fn module_runtime_call_boundaries_are_exact() -> Result<(), String> {
        let text = "my $source = \"use_module('No::Link')\"; ".to_owned()
            + "obj->use_module('No::Method'); obj->   use_module('No::SpacedMethod'); "
            + "Other::use_module('No::Other'); "
            + "use_module_extra('No::Extra');";
        let links = compute_links(uri(), &text, &[]);

        if !links.is_empty() {
            return Err(format!("unrelated call names produced links: {links:?}"));
        }
        Ok(())
    }

    #[test]
    fn module_runtime_link_range_is_utf16_safe() -> Result<(), String> {
        let line = "my \u{1f600} = 1; use_module('Foo::Bar');";
        let links = compute_links(uri(), line, &[]);
        let link = links.first().ok_or("expected a Module::Runtime link")?;
        let byte_start = line.find("Foo::Bar").ok_or("test input must contain the module")?;
        let expected_start = line
            .get(..byte_start)
            .ok_or("module start is not a UTF-8 boundary")?
            .encode_utf16()
            .count() as u64;
        let expected_end = expected_start + "Foo::Bar".encode_utf16().count() as u64;
        let actual_start = link.pointer("/range/start/character").and_then(Value::as_u64);
        let actual_end = link.pointer("/range/end/character").and_then(Value::as_u64);
        if actual_start != Some(expected_start) || actual_end != Some(expected_end) {
            return Err(format!("unexpected UTF-16 range: {link:?}"));
        }
        Ok(())
    }

    #[test]
    fn malformed_module_runtime_literals_fail_closed() -> Result<(), String> {
        let cases = [
            "use_module('Foo::Bar);",
            "require_module(\"Baz::Qux);",
            r#"use_module('Foo\'Bar');"#,
            r#"require_module("Baz\"Qux");"#,
        ];

        for source in cases {
            let links = compute_links(uri(), source, &[]);
            if !links.is_empty() {
                return Err(format!(
                    "malformed or escaped literal produced links: {source:?}, {links:?}"
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn existing_document_link_forms_remain_unchanged() -> Result<(), String> {
        let text = "use Existing::Module;\n".to_owned()
            + "require \"Existing/Path.pm\";\n"
            + "require \"helper.pl\";\n"
            + "use strict;\n"
            + "=pod\nSee L<Docs::Existing>.\n=cut\n";
        let links = compute_links(uri(), &text, &[]);

        let expected = [
            ("/data/module", "Existing::Module"),
            ("/data/module", "Existing::Path"),
            ("/data/type", "file"),
            ("/data/module", "Docs::Existing"),
        ];
        if links.len() != expected.len()
            || expected.iter().any(|(pointer, value)| {
                !links.iter().any(|link| data_str(link, pointer) == Some(*value))
            })
        {
            return Err(format!("existing document links changed: {links:?}"));
        }
        Ok(())
    }

    // ── use statement ──────────────────────────────────────────

    #[test]
    fn emits_module_link_for_use_statement() {
        let links = compute_links(uri(), "use Foo::Bar;\n", &[]);
        assert_eq!(links.len(), 1);
        if let Some(link) = links.first() {
            assert_eq!(link.pointer("/data/type").and_then(Value::as_str), Some("module"));
            assert_eq!(link.pointer("/data/module").and_then(Value::as_str), Some("Foo::Bar"));
        }
    }

    #[test]
    fn does_not_emit_link_for_pragma_use_strict() {
        let links = compute_links(uri(), "use strict;\n", &[]);
        assert!(links.is_empty(), "pragmas should not produce document links");
    }

    #[test]
    fn does_not_emit_link_for_pragma_use_warnings() {
        let links = compute_links(uri(), "use warnings;\n", &[]);
        assert!(links.is_empty(), "pragmas should not produce document links");
    }

    #[test]
    fn does_not_emit_link_for_use_feature_pragma() {
        let links = compute_links(uri(), "use feature 'say';\n", &[]);
        assert!(links.is_empty(), "'feature' is a pragma");
    }

    // ── use parent / use base ─────────────────────────────────

    #[test]
    fn does_not_emit_module_link_for_use_parent_statement() {
        let links = compute_links(uri(), "use parent 'Foo::Bar';\n", &[]);
        assert!(links.is_empty());
    }

    #[test]
    fn does_not_emit_module_link_for_use_base_statement() {
        let links = compute_links(uri(), "use base 'Foo::Bar';\n", &[]);
        assert!(links.is_empty(), "use base is a base-class declaration, not a module link");
    }

    // ── require statement ─────────────────────────────────────

    #[test]
    fn emits_module_link_for_module_form_require_statement() {
        let links = compute_links(uri(), "require Foo::Bar;\n", &[]);
        assert_eq!(links.len(), 1);
        if let Some(link) = links.first() {
            assert_eq!(link.pointer("/data/type").and_then(Value::as_str), Some("module"));
            assert_eq!(link.pointer("/data/module").and_then(Value::as_str), Some("Foo::Bar"));
        }
    }

    #[test]
    fn emits_file_link_for_require_with_double_quoted_string() {
        // .pm files are normalized to module links (my/file.pm → my::file)
        let links = compute_links(uri(), r#"require "my/file.pm";"#, &[]);
        assert_eq!(links.len(), 1, "require .pm with double-quotes should emit a module link");
        if let Some(link) = links.first() {
            assert_eq!(link.pointer("/data/type").and_then(Value::as_str), Some("module"));
            assert_eq!(link.pointer("/data/module").and_then(Value::as_str), Some("my::file"));
        }
    }

    #[test]
    fn emits_file_link_for_require_with_single_quoted_string() {
        // lib/helper.pm → lib::helper (lib/ prefix is NOT stripped by module_path_to_name)
        let links = compute_links(uri(), "require 'lib/helper.pm';", &[]);
        assert_eq!(links.len(), 1, "require .pm with single-quotes should emit a module link");
        if let Some(link) = links.first() {
            assert_eq!(link.pointer("/data/type").and_then(Value::as_str), Some("module"));
            assert_eq!(link.pointer("/data/module").and_then(Value::as_str), Some("lib::helper"));
        }
    }

    #[test]
    fn require_pm_produces_no_duplicate_links() {
        // The old hardcoded scan was separate from the parsed-require path, producing 2 links.
        // The new unified path must produce exactly 1 link.
        let links = compute_links(uri(), r#"require "Foo/Bar.pm";"#, &[]);
        assert_eq!(links.len(), 1, "require .pm must produce exactly one link, not duplicates");
        if let Some(link) = links.first() {
            assert_eq!(link.pointer("/data/type").and_then(Value::as_str), Some("module"));
            assert_eq!(link.pointer("/data/module").and_then(Value::as_str), Some("Foo::Bar"));
        }
    }

    #[test]
    fn require_pl_produces_file_link() {
        // .pl files are script includes, not module names — must remain file links
        let links = compute_links(uri(), r#"require "helper.pl";"#, &[]);
        assert_eq!(links.len(), 1, "require .pl should emit a file link");
        if let Some(link) = links.first() {
            assert_eq!(link.pointer("/data/type").and_then(Value::as_str), Some("file"));
            assert_eq!(link.pointer("/data/path").and_then(Value::as_str), Some("helper.pl"));
        }
    }

    #[test]
    fn does_not_emit_link_for_require_bare_word_without_colons() {
        // A bare word without '::' is not a module form require
        let links = compute_links(uri(), "require Something;\n", &[]);
        assert!(links.is_empty(), "bare require without '::' should not emit a module link");
    }

    // ── link range / metadata ─────────────────────────────────

    #[test]
    fn link_range_is_on_correct_line() {
        let text = "# comment\nuse Foo::Bar;\n";
        let links = compute_links(uri(), text, &[]);
        assert_eq!(links.len(), 1);
        if let Some(link) = links.first() {
            let line = link.pointer("/range/start/line").and_then(Value::as_u64);
            assert_eq!(line, Some(1), "link should be on line 1 (0-indexed)");
        }
    }

    #[test]
    fn link_tooltip_contains_module_name() {
        let links = compute_links(uri(), "use Foo::Bar;\n", &[]);
        assert_eq!(links.len(), 1);
        if let Some(link) = links.first() {
            let tooltip = link.pointer("/tooltip").and_then(Value::as_str).unwrap_or("");
            assert!(tooltip.contains("Foo::Bar"), "tooltip should reference the module name");
        }
    }

    // ── multiple statements ───────────────────────────────────

    #[test]
    fn emits_link_for_each_use_statement_in_multi_line_file() {
        let text = "use Foo;\nuse Bar::Baz;\nuse strict;\n";
        let links = compute_links(uri(), text, &[]);
        // 'strict' is a pragma → only Foo and Bar::Baz get links
        // 'Foo' has no '::', but some parsers may still emit a link; what matters is 'strict' is excluded
        let has_strict = links
            .iter()
            .any(|l| l.pointer("/data/module").and_then(Value::as_str) == Some("strict"));
        assert!(!has_strict, "strict pragma must not appear in links");
    }

    #[test]
    fn pod_emits_module_links_for_plain_and_labeled_targets() {
        let text =
            "package Local::Doc;\n=head1 SEE ALSO\n\nSee L<Foo::Bar> and L<docs|Baz::Qux>.\n=cut\n";

        let links = compute_links(uri(), text, &[]);

        assert_eq!(links.len(), 2);
        assert!(
            links.iter().any(|link| data_str(link, "/data/module") == Some("Foo::Bar")),
            "missing plain POD module link: {links:#?}"
        );
        assert!(
            links.iter().any(|link| data_str(link, "/data/module") == Some("Baz::Qux")),
            "missing labeled POD module link: {links:#?}"
        );
        let foo = links
            .iter()
            .find(|link| data_str(link, "/data/module") == Some("Foo::Bar"))
            .unwrap_or(&Value::Null);
        assert_eq!(data_str(foo, "/data/type"), Some("module"));
        assert_eq!(foo.pointer("/range/start/line").and_then(Value::as_u64), Some(3));
        assert_eq!(foo.pointer("/range/start/character").and_then(Value::as_u64), Some(6));
        assert_eq!(foo.pointer("/range/end/character").and_then(Value::as_u64), Some(14));
    }

    #[test]
    fn pod_emits_core_pragma_links_for_supported_perldoc_topics() {
        let text = "=pod\nSee L<strict>, L<warnings>, and L<feature>.\n=cut\n";

        let links = compute_links(uri(), text, &[]);

        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|link| data_str(link, "/data/module") == Some("strict")));
        assert!(links.iter().any(|link| data_str(link, "/data/module") == Some("warnings")));
        assert!(!links.iter().any(|link| data_str(link, "/data/module") == Some("feature")));
    }

    #[test]
    fn pod_emits_section_links_for_plain_and_labeled_local_sections() {
        let text = "=pod\nSee L</method_name> and L<section docs|/setup>.\n=cut\n";

        let links = compute_links(uri(), text, &[]);

        assert_eq!(links.len(), 2);
        assert!(
            links.iter().any(|link| {
                data_str(link, "/data/type") == Some("pod_section")
                    && data_str(link, "/data/section") == Some("method_name")
            }),
            "missing plain POD section link: {links:#?}"
        );
        assert!(
            links.iter().any(|link| {
                data_str(link, "/data/type") == Some("pod_section")
                    && data_str(link, "/data/section") == Some("setup")
            }),
            "missing labeled POD section link: {links:#?}"
        );
    }

    #[test]
    fn pod_skips_empty_label_path_like_single_segment_and_self_targets() {
        let text = "package Local::Doc;\n=pod\nSee L<|Other::Module>, L<docs|Local::>, L<docs|https://example.invalid>, L<Local::Doc>, L<Foo/Bar>, and L<NotAModule>.\n=cut\n";

        let links = compute_links(uri(), text, &[]);

        assert!(links.is_empty(), "malformed POD targets must stay quiet: {links:#?}");
    }

    #[test]
    fn pod_package_example_does_not_suppress_matching_link() {
        let text = "=pod\npackage Example::Module;\nSee L<Example::Module>.\n=cut\n";

        let links = compute_links(uri(), text, &[]);

        assert!(
            links.iter().any(|link| data_str(link, "/data/module") == Some("Example::Module")),
            "POD package examples must not be treated as the current code package: {links:#?}"
        );
    }

    #[test]
    fn pod_ignores_markup_after_cut_and_use_statements_inside_pod() {
        let text = "=pod\nuse Pod::Example;\n=cut\nSee L<Foo::Bar>.\n";

        let links = compute_links(uri(), text, &[]);

        assert!(links.is_empty(), "non-POD L<> and use statements inside POD must stay quiet");
    }

    #[test]
    fn pod_directives_must_start_at_column_zero() {
        let text = "    =pod\nuse Foo::Bar;\n";

        let links = compute_links(uri(), text, &[]);

        assert!(
            links.iter().any(|link| data_str(link, "/data/module") == Some("Foo::Bar")),
            "indented POD-looking text must not suppress code links: {links:#?}"
        );
    }
}
