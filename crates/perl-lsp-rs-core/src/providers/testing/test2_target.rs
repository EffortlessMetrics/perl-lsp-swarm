//! Static resolution for aliases installed by `Test2::Tools::Target`.
//!
//! The module installs both a constant and a package scalar for each alias.
//! `Test2::V0` and `Test2::V1` expose the same behavior through `-target`.
//! Only literal forms are resolved; dynamic expressions remain unknown rather
//! than producing invented completions.

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

static BUNDLE_TARGET_OPTION: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r#"(?xs)(?:^|[\s,])(?P<option>-target\s*(?:=>)?\s*(?P<value>\{[^{}]*\}|'(?:\\.|[^'])*'|"(?:\\.|[^"])*"|[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*))"#,
    )
    .ok()
});

static TARGET_ALIAS_PAIR: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)(?:^|,)\s*(?:'(?P<single>[A-Za-z_][A-Za-z0-9_]*|)'|"(?P<double>[A-Za-z_][A-Za-z0-9_]*|)"|(?P<bare>[A-Za-z_][A-Za-z0-9_]*))\s*=>"#,
    )
    .ok()
});

static STATIC_PACKAGE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*$").ok()
});

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
            let captures = BUNDLE_TARGET_OPTION.as_ref()?.captures(raw_args)?;
            let option = captures.name("option")?;
            let value = captures.name("value")?;
            Some(Test2TargetImport {
                aliases: parse_target_aliases(value.as_str()),
                remaining_args: Some(remove_option(raw_args, option.start(), option.end())),
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
        for captures in pattern.captures_iter(value) {
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
        let package = strip_quotes(value);
        if STATIC_PACKAGE.as_ref().is_some_and(|pattern| pattern.is_match(package)) {
            aliases.insert("CLASS".to_string());
        }
    }

    aliases
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

    format!("{}{}", &raw_args[..remove_start], &raw_args[remove_end..])
        .trim()
        .to_string()
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
    fn direct_target_pairs_preserve_static_aliases() {
        assert_eq!(
            aliases(
                "Test2::Tools::Target",
                "service => 'My::Service', repo => 'My::Repo'",
            ),
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
            resolve_target_import(
                "Test2::V1",
                "-import, -target => { service => 'My::Service' }",
            ),
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
    }
}
