//! `dap.cli_native_only` — the shipped `perl-dap` CLI must stay native-only.
//!
//! The legacy `--bridge` mode (a proxy to `Perl::LanguageServer`) was removed
//! from the shipped `perl-dap` CLI (#3277); `BridgeAdapter` remains a
//! library-only path. This indicator guards against a clap `--bridge` flag
//! being reintroduced onto the product CLI.
//!
//! The check is deliberately precise: it looks for an actual clap flag
//! definition (`long = "bridge"`), not the string `"--bridge"`. That keeps it
//! from false-positiving on the crate's own regression test, which asserts the
//! flag is *absent* by matching the `"--bridge"` string.

use std::path::Path;

use crate::evidence::Outcome;
use crate::indicator::EvidenceRef;

/// CLI source files scanned for a reintroduced bridge flag.
const CLI_SOURCES: &[&str] = &["crates/perl-dap/src/main.rs"];

/// clap flag-definition markers that would expose a `--bridge` product flag.
const BRIDGE_FLAG_MARKERS: &[&str] = &["long = \"bridge\"", "long = \"Bridge\""];

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
        for (idx, line) in text.lines().enumerate() {
            let normalized = line.to_ascii_lowercase();
            if BRIDGE_FLAG_MARKERS.iter().any(|m| normalized.contains(&m.to_ascii_lowercase())) {
                hits.push(format!("{source}:{}", idx + 1));
            }
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
    fn reintroduced_flag_fails() {
        let dir = tempfile::tempdir().expect("tmp");
        write_main(dir.path(), "struct Args {\n  #[arg(long = \"bridge\")]\n  bridge: bool,\n}\n");
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Fail);
    }

    #[test]
    fn missing_source_is_unverified() {
        let dir = tempfile::tempdir().expect("tmp");
        assert_eq!(cli_native_only(dir.path()).status, IndicatorStatus::Unverified);
    }
}
