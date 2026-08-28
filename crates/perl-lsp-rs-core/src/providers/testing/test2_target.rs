//! Static resolution for aliases installed by `Test2::Tools::Target`.
//!
//! The module installs both a constant and a package scalar for each alias.
//! `Test2::V0` and `Test2::V1` expose the same behavior through `-target`.
//! Only literal forms are resolved; dynamic expressions remain unknown rather
//! than producing invented completions.

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

static TARGET_ALIAS_PAIR: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r#"(?xs)^\s*(?:'(?P<single>[A-Za-z_][A-Za-z0-9_]*|)'|"(?P<double>[A-Za-z_][A-Za-z0-9_]*|)"|(?P<bare>[A-Za-z_][A-Za-z0-9_]*))\s*=>\s*(?P<target>.*?)\s*$"#,
    )
    .ok()
});

static STATIC_PACKAGE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*$").ok());

/// A statically resolved Test2 target import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Test2TargetImport {
    /// Local alias names. Each exists as both `NAME()` and `$NAME` at runtime.
    pub(crate) aliases: BTreeSet<String>,
    /// Bundle arguments with `-target` removed.
    ///
    /// Completion feeds these arguments to the ordinary Test2 import resolver
    /// so target hash keys are not mistaken for an explicit export list.
    pub(crate) remaining_args: Option<String>,
}

/// Resolve aliases from `Test2::Tools::Target` or a Test2 bundle's `-target`.
///
/// Returns `None` when the module has no target semantics or a bundle import
/// does not contain `-target`.
pub(crate) fn resolve_target_import(module: &str, raw_args: &str) -> Option<Test2TargetImport> {
    match module {
        "Test2::Tools::Target" => Some(Test2TargetImport {
            aliases: parse_target_aliases(raw_args),
            remaining_args: None,
        }),
        "Test2::V0" | "Test2::V1" => {
            let (option_start, option_end, value) = find_bundle_target_option(raw_args)?;
            Some(Test2TargetImport {
                aliases: parse_target_aliases(value),
                remaining_args: Some(remove_option(raw_args, option_start, option_end)),
            })
        }
        _ => None,
    }
}

fn parse_target_aliases(raw_value: &str) -> BTreeSet<String> {
    let value = trim_outer_delimiters(raw_value.trim());
    let mut aliases = BTreeSet::new();

    if value.contains("=>") {
        let Some(pattern) = TARGET_ALIAS_PAIR.as_ref() else {
            return aliases;
        };
        let Some(entries) = split_top_level_entries(value) else {
            return aliases;
        };
        for entry in entries {
            let Some(captures) = pattern.captures(entry) else {
                continue;
            };
            let Some(target) = captures.name("target").map(|capture| capture.as_str().trim())
            else {
                continue;
            };
            if !is_static_target_package(target) {
                continue;
            }

            let name = captures
                .name("single")
                .or_else(|| captures.name("double"))
                .or_else(|| captures.name("bare"))
                .map(|capture| capture.as_str())
                .unwrap_or_default();
            let name = if name.is_empty() { "CLASS" } else { name };
            aliases.insert(name.to_string());
        }
    } else {
        if is_static_target_package(value) {
            aliases.insert("CLASS".to_string());
        }
    }

    aliases
}

fn is_static_target_package(value: &str) -> bool {
    let value = value.trim();
    let quoted = value.len() >= 2
        && matches!(
            (value.as_bytes().first(), value.as_bytes().last()),
            (Some(b'\''), Some(b'\'')) | (Some(b'"'), Some(b'"'))
        );
    let package = strip_quotes(value);
    (quoted || package != "undef")
        && STATIC_PACKAGE.as_ref().is_some_and(|pattern| pattern.is_match(package))
}

fn find_bundle_target_option(raw_args: &str) -> Option<(usize, usize, &str)> {
    let bytes = raw_args.as_bytes();
    let option = b"-target";

    let mut delimiters = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            index += 1;
            continue;
        }

        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if matches!(byte, b'{' | b'[' | b'(') {
            delimiters.push(byte);
        } else if matches!(byte, b'}' | b']' | b')') {
            let expected = match byte {
                b'}' => b'{',
                b']' => b'[',
                b')' => b'(',
                _ => return None,
            };
            if delimiters.pop() != Some(expected) {
                return None;
            }
        } else if byte == b'-'
            && delimiters.is_empty()
            && bytes.get(index..index + option.len()) == Some(option)
            && option_has_boundaries(bytes, index, option.len())
        {
            let mut value_start = index + option.len();
            while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
                value_start += 1;
            }
            if bytes.get(value_start..value_start + 2) == Some(b"=>") {
                value_start += 2;
                while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
                    value_start += 1;
                }
            }

            let value_end = scan_bundle_target_value(raw_args, value_start)?;
            return Some((index, value_end, &raw_args[value_start..value_end]));
        }
        index += 1;
    }

    None
}

fn option_has_boundaries(bytes: &[u8], start: usize, len: usize) -> bool {
    let before_is_boundary = start == 0
        || bytes.get(start - 1).is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',');
    let after = start + len;
    let after_is_boundary = bytes
        .get(after)
        .is_none_or(|byte| byte.is_ascii_whitespace() || matches!(*byte, b',' | b'='));
    before_is_boundary && after_is_boundary
}

fn scan_bundle_target_value(raw: &str, start: usize) -> Option<usize> {
    let bytes = raw.as_bytes();
    match *bytes.get(start)? {
        b'\'' | b'"' => scan_quoted_value(bytes, start),
        b'{' | b'[' | b'(' => scan_balanced_value(bytes, start),
        _ => {
            let end = bytes[start..]
                .iter()
                .position(|byte| matches!(byte, b',' | b' ' | b'\t' | b'\r' | b'\n'))
                .map_or(bytes.len(), |offset| start + offset);
            let value = raw.get(start..end)?.trim();
            (is_static_target_package(value) || value == "undef").then_some(end)
        }
    }
}

fn scan_quoted_value(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = *bytes.get(start)?;
    let mut escaped = false;
    for (offset, byte) in bytes[start + 1..].iter().enumerate() {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == quote {
            return Some(start + offset + 2);
        }
    }
    None
}

fn scan_balanced_value(bytes: &[u8], start: usize) -> Option<usize> {
    let mut delimiters = Vec::new();
    let mut quote = None;
    let mut escaped = false;

    for (index, byte) in bytes[start..].iter().enumerate() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == active_quote {
                quote = None;
            }
            continue;
        }

        match *byte {
            b'\'' | b'"' => quote = Some(*byte),
            b'{' | b'[' | b'(' => delimiters.push(*byte),
            b'}' | b']' | b')' => {
                let expected = match *byte {
                    b'}' => b'{',
                    b']' => b'[',
                    b')' => b'(',
                    _ => unreachable!(),
                };
                if delimiters.pop() != Some(expected) {
                    return None;
                }
                if delimiters.is_empty() {
                    return Some(start + index + 1);
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level_entries(value: &str) -> Option<Vec<&str>> {
    let mut entries = Vec::new();
    let mut start = 0;
    let mut delimiters = Vec::new();
    let mut quote = None;
    let mut escaped = false;

    for (index, byte) in value.bytes().enumerate() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            continue;
        }

        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'{' | b'[' | b'(' => delimiters.push(byte),
            b'}' | b']' | b')' => {
                let expected = match byte {
                    b'}' => Some(b'{'),
                    b']' => Some(b'['),
                    b')' => Some(b'('),
                    _ => None,
                };
                let Some(expected) = expected else {
                    return None;
                };
                if delimiters.pop() != Some(expected) {
                    return None;
                }
            }
            b',' if delimiters.is_empty() => {
                entries.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }

    if quote.is_some() || !delimiters.is_empty() {
        return None;
    }
    entries.push(&value[start..]);
    Some(entries)
}

fn remove_option(raw_args: &str, option_start: usize, option_end: usize) -> String {
    let bytes = raw_args.as_bytes();
    let mut remove_start = option_start;
    let mut remove_end = option_end;

    while bytes.get(remove_end).is_some_and(u8::is_ascii_whitespace) {
        remove_end += 1;
    }
    if bytes.get(remove_end) == Some(&b',') {
        remove_end += 1;
        while bytes.get(remove_end).is_some_and(u8::is_ascii_whitespace) {
            remove_end += 1;
        }
    } else {
        while remove_start > 0 && bytes[remove_start - 1].is_ascii_whitespace() {
            remove_start -= 1;
        }
        if remove_start > 0 && bytes[remove_start - 1] == b',' {
            remove_start -= 1;
        }
    }

    format!("{}{}", &raw_args[..remove_start], &raw_args[remove_end..]).trim().to_string()
}

fn trim_outer_delimiters(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if matches!((bytes[0], bytes[bytes.len() - 1]), (b'{', b'}') | (b'(', b')')) {
            return value[1..value.len() - 1].trim();
        }
    }
    value
}

fn strip_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if matches!((bytes[0], bytes[bytes.len() - 1]), (b'\'', b'\'') | (b'"', b'"')) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(module: &str, args: &str) -> BTreeSet<String> {
        resolve_target_import(module, args).map_or_else(BTreeSet::new, |resolved| resolved.aliases)
    }

    #[test]
    fn direct_target_literal_uses_class_alias() {
        assert_eq!(
            aliases("Test2::Tools::Target", "'My::Service'"),
            BTreeSet::from(["CLASS".to_string()])
        );
    }

    #[test]
    fn quoted_undef_is_a_literal_but_bare_undef_is_not() {
        assert_eq!(
            aliases("Test2::Tools::Target", "'undef'"),
            BTreeSet::from(["CLASS".to_string()])
        );
        assert_eq!(
            aliases("Test2::V0", "-target => \"undef\""),
            BTreeSet::from(["CLASS".to_string()])
        );
        assert!(aliases("Test2::Tools::Target", "undef").is_empty());
        assert!(aliases("Test2::V0", "-target => undef").is_empty());
    }

    #[test]
    fn direct_target_pairs_preserve_static_aliases() {
        assert_eq!(
            aliases("Test2::Tools::Target", "service => 'My::Service', repo => 'My::Repo'",),
            BTreeSet::from(["repo".to_string(), "service".to_string()])
        );
    }

    #[test]
    fn v0_target_hash_preserves_other_import_args() {
        assert_eq!(
            resolve_target_import(
                "Test2::V0",
                "':DEFAULT', -target => { service => 'My::Service' }, '!meta'",
            ),
            Some(Test2TargetImport {
                aliases: BTreeSet::from(["service".to_string()]),
                remaining_args: Some("':DEFAULT', '!meta'".to_string()),
            })
        );
    }

    #[test]
    fn v0_target_literal_leaves_default_imports_intact() {
        assert_eq!(
            resolve_target_import("Test2::V0", "-target => 'My::Service'"),
            Some(Test2TargetImport {
                aliases: BTreeSet::from(["CLASS".to_string()]),
                remaining_args: Some(String::new()),
            })
        );
    }

    #[test]
    fn v1_target_preserves_import_option() {
        assert_eq!(
            resolve_target_import("Test2::V1", "-import, -target => { service => 'My::Service' }",),
            Some(Test2TargetImport {
                aliases: BTreeSet::from(["service".to_string()]),
                remaining_args: Some("-import".to_string()),
            })
        );
    }

    #[test]
    fn bundle_without_target_is_not_claimed() {
        assert_eq!(resolve_target_import("Test2::V0", "'!meta'"), None);
        assert_eq!(resolve_target_import("Test2::V1", "-import"), None);
    }

    #[test]
    fn dynamic_target_expression_does_not_invent_an_alias() {
        assert!(aliases("Test2::Tools::Target", "$target").is_empty());
        assert!(resolve_target_import("Test2::V0", "-target => $target").is_none());
    }

    #[test]
    fn target_pairs_require_static_package_values() {
        assert!(aliases("Test2::Tools::Target", "service => $target").is_empty());
        assert!(aliases("Test2::V0", "-target => { service => $target }").is_empty());

        assert_eq!(
            aliases("Test2::Tools::Target", "service => 'My::Service', dynamic => $target",),
            BTreeSet::from(["service".to_string()])
        );
    }

    #[test]
    fn nested_target_pairs_are_not_flattened_into_aliases() {
        let nested = "service => [$target, repo => 'My::Repo',], actual => 'My::Actual'";
        let expected = BTreeSet::from(["actual".to_string()]);

        assert_eq!(aliases("Test2::Tools::Target", nested), expected);
        assert_eq!(aliases("Test2::V0", &format!("-target => {{ {nested} }}")), expected);
    }

    #[test]
    fn nested_hash_target_values_are_not_flattened_into_aliases() {
        let nested = "service => { repo => 'My::Repo' }, actual => 'My::Actual'";
        let expected = BTreeSet::from(["actual".to_string()]);

        assert_eq!(aliases("Test2::Tools::Target", nested), expected);
        assert_eq!(aliases("Test2::V0", &format!("-target => {{ {nested} }}")), expected);
    }

    #[test]
    fn undef_target_is_not_treated_as_a_static_package() {
        assert!(aliases("Test2::Tools::Target", "undef").is_empty());
        assert_eq!(
            resolve_target_import("Test2::V0", "-target => undef"),
            Some(Test2TargetImport {
                aliases: BTreeSet::new(),
                remaining_args: Some(String::new()),
            })
        );
    }

    #[test]
    fn bundle_target_option_requires_exact_top_level_token() {
        assert_eq!(resolve_target_import("Test2::V0", "-targetish => 'Fake::Target'"), None);
        assert_eq!(
            resolve_target_import(
                "Test2::V0",
                "-srand => \"contains -target => 'Fake::Target'\", -target => 'Real::Target'",
            )
            .map(|resolved| resolved.aliases),
            Some(BTreeSet::from(["CLASS".to_string()]))
        );
        assert_eq!(
            resolve_target_import(
                "Test2::V0",
                "-T2 => { -as => \"contains -target => 'Fake::Target'\" }",
            ),
            None
        );
    }
}
