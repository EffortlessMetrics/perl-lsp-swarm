//! `dap.cli_native_only` — the shipped `perl-dap` CLI must stay native-only.
//!
//! The legacy `--bridge` mode (a proxy to `Perl::LanguageServer`) was removed
//! from the shipped `perl-dap` CLI (#3277); `BridgeAdapter` remains a
//! library-only path. This indicator guards against a clap `--bridge` flag
//! being reintroduced onto the product CLI.
//!
//! The check catches both ways clap can expose a `--bridge` flag:
//!
//! - an explicit long name — `long = "bridge"`; and
//! - the derive **shorthand** `#[arg(long)]` (or `#[clap(long)]`) on a field
//!   literally named `bridge`, where clap derives the flag name from the field
//!   (the same mechanism that turns `log_level` into `--log-level`).
//!
//! It is deliberately precise: a bare `"--bridge"` string (as in the crate's own
//! regression test, which asserts the flag is *absent*) is not a flag
//! definition and must not trip the check.

use std::path::Path;

use crate::evidence::Outcome;
use crate::indicator::EvidenceRef;

/// CLI source files scanned for a reintroduced bridge flag.
const CLI_SOURCES: &[&str] = &["crates/perl-dap/src/main.rs"];

/// How many preceding lines to look back for a clap arg attribute above a
/// `bridge` field declaration (the attribute usually sits on its own line).
const ATTR_LOOKBACK: usize = 3;

/// Whether `line` is a clap arg attribute that opts a field into a long flag
/// (`#[arg(long ...)]` / `#[clap(long ...)]`), covering both the shorthand
/// `long` and the explicit `long = "..."` forms.
fn is_clap_long_attr(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    (l.contains("#[arg(") || l.contains("#[clap(")) && l.contains("long")
}

/// Whether `line` declares a struct field literally named `bridge`
/// (e.g. `bridge: bool` / `pub bridge : Bar`). Anchored at the start of the
/// trimmed line so comments and paths like `bridge_adapter::new` do not match.
fn is_bridge_field_decl(line: &str) -> bool {
    let t = line.trim().trim_start_matches("pub ").trim_start();
    t.starts_with("bridge:") || t.starts_with("bridge :")
}

/// Scan CLI source text; return 1-based line numbers that expose a bridge flag.
fn scan(text: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut hits = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        // Explicit long name, or a same-line `#[arg(long)] bridge: ...`.
        if line.to_ascii_lowercase().contains("long = \"bridge\"")
            || (is_clap_long_attr(line) && line.replace(' ', "").contains("bridge:"))
        {
            hits.push(idx + 1);
            continue;
        }
        // Shorthand: a `bridge` field under a nearby clap long attribute.
        if is_bridge_field_decl(line) {
            let start = idx.saturating_sub(ATTR_LOOKBACK);
            if lines[start..idx].iter().any(|l| is_clap_long_attr(l)) {
                hits.push(idx + 1);
            }
        }
    }
    hits
}

/// `dap.cli_native_only`.
pub(crate) fn cli_native_only(repo_root: &Path) -> Outcome {
    let mut evidence =
        vec![EvidenceRef::new("test", "perl-dap main.rs::cli_help_has_no_bridge_product_surface")];

    let mut hits = Vec::new();
    let mut any_source_read = false;
    for source in CLI_SOURCES {
        let path = repo_root.join(source);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        any_source_read = true;
        for line in scan(&text) {
            hits.push(format!("{source}:{line}"));
        }
    }

    if !any_source_read {
        return Outcome::unverified(
            evidence,
            "Could not read the perl-dap CLI source to verify native-only status.",
        );
    }

    if hits.is_empty() {
        Outcome::pass(evidence)
    } else {
        for hit in hits.iter().take(10) {
            evidence.push(EvidenceRef::file(hit.clone()));
        }
        Outcome::fail(
            evidence,
            "Remove the `--bridge` flag from the shipped perl-dap CLI; bridge mode is a \
             library-only path, not a product surface.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::IndicatorStatus;
    use std::fs;

    fn write_main(root: &Path, body: &str) {
        let p = root.join("crates/perl-dap/src/main.rs");
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, body).expect("write");
    }

    #[test]
    fn clean_cli_passes() {
        let dir = tempfile::tempdir().expect("tmp");
        // Contains the string "--bridge" only inside an assertion, like the real
        // regression test — must NOT be flagged.
        write_main(
            dir.path(),
            "fn main() {}\n#[test]\nfn t() { assert!(!help.contains(\"--bridge\")); }\n",
        );
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Pass);
    }

    #[test]
    fn reintroduced_explicit_flag_fails() {
        let dir = tempfile::tempdir().expect("tmp");
        write_main(dir.path(), "struct Args {\n  #[arg(long = \"bridge\")]\n  bridge: bool,\n}\n");
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Fail);
    }

    #[test]
    fn reintroduced_shorthand_flag_fails() {
        // The idiomatic clap form: `#[arg(long)]` derives `--bridge` from the
        // field name. Must be caught even though there is no `long = "bridge"`.
        let dir = tempfile::tempdir().expect("tmp");
        write_main(dir.path(), "struct Args {\n  #[arg(long)]\n  bridge: bool,\n}\n");
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Fail);
    }

    #[test]
    fn shorthand_on_same_line_fails() {
        let dir = tempfile::tempdir().expect("tmp");
        write_main(dir.path(), "struct Args {\n  #[arg(long)] bridge: bool,\n}\n");
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Fail);
    }

    #[test]
    fn bridge_field_without_clap_attr_passes() {
        // A `bridge` field with no clap long attribute is not a CLI flag.
        let dir = tempfile::tempdir().expect("tmp");
        write_main(dir.path(), "struct Internal {\n  bridge: bool,\n}\n");
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Pass);
    }

    #[test]
    fn unrelated_long_flag_passes() {
        // A different long flag (log_level -> --log-level) must not false-positive.
        let dir = tempfile::tempdir().expect("tmp");
        write_main(
            dir.path(),
            "struct Args {\n  #[arg(long, default_value = \"info\")]\n  log_level: String,\n}\n",
        );
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Pass);
    }

    #[test]
    fn missing_source_is_unverified() {
        let dir = tempfile::tempdir().expect("tmp");
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Unverified);
    }
}
