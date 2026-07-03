//! File-level classification primitives.
//!
//! The full [`FileFact`] record (id, digest, line index, parse status, role)
//! is assembled by the file-facts layer in a follow-up. This module ships the
//! deterministic, pure building blocks it depends on: the [`FileRole`] taxonomy,
//! a path-only [`classify_role`] heuristic, and the [`ParseStatus`] enum.

use crate::path::RepoRelativePath;
use serde::{Deserialize, Serialize};

/// The role a file plays in a Perl distribution.
///
/// Classification here is path-based and therefore a heuristic: it never reads
/// file contents (no shebang sniffing, no `package` scanning). Callers needing
/// content-confirmed roles refine this once the file is parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FileRole {
    /// Library module under `lib/` (or otherwise a `.pm`).
    Lib,
    /// Test file (`t/`, `xt/`, or `*.t`).
    Test,
    /// Executable script (`script/`, `bin/`, or a `.pl`).
    Script,
    /// Distribution metadata (`META.json`, `Makefile.PL`, `cpanfile`, ...).
    DistMetadata,
    /// Standalone POD documentation (`*.pod`).
    Pod,
    /// Build-generated output (`blib/`, `_build/`, ...).
    Generated,
    /// Could not be classified from the path alone.
    Unknown,
}

/// Whether parsing a file succeeded, partially recovered, or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParseStatus {
    /// Parsed cleanly with no errors.
    Parsed,
    /// Parsed with recovery — some regions produced error nodes.
    Recovered,
    /// Parsing failed; facts from this file are unavailable or degraded.
    Failed,
    /// Not yet parsed.
    Unparsed,
}

/// Classify a file's [`FileRole`] from its repo-relative path alone.
///
/// The classification is deterministic and total. Precedence is deliberate:
/// generated output and dist metadata win over extension-based guesses, and
/// test/pod placement wins over the generic lib/script fallbacks.
#[must_use]
pub fn classify_role(path: &RepoRelativePath) -> FileRole {
    let components: Vec<&str> = path.components().collect();
    let name = path.file_name();
    let ext = path.extension();
    let ext = ext.as_deref();

    // 1. Build-generated output — never treat as source, regardless of extension.
    if components.iter().any(|c| matches!(*c, "blib" | "_build" | ".build" | "pm_to_blib")) {
        return FileRole::Generated;
    }

    // 2. Distribution metadata — matched by exact file name.
    if is_dist_metadata(name) {
        return FileRole::DistMetadata;
    }

    // 3. Standalone POD.
    if ext == Some("pod") {
        return FileRole::Pod;
    }

    // 4. Tests — by directory placement or `.t` extension.
    let top = components.first().copied();
    if ext == Some("t") || matches!(top, Some("t") | Some("xt")) {
        return FileRole::Test;
    }

    // 5. Library modules.
    if ext == Some("pm") {
        return FileRole::Lib;
    }

    // 6. Scripts — `.pl`, or placed under script/ or bin/.
    if ext == Some("pl") || matches!(top, Some("script") | Some("bin")) {
        return FileRole::Script;
    }

    FileRole::Unknown
}

/// Exact file names recognised as distribution metadata.
fn is_dist_metadata(name: &str) -> bool {
    matches!(
        name,
        "META.json"
            | "META.yml"
            | "MYMETA.json"
            | "MYMETA.yml"
            | "Makefile.PL"
            | "Build.PL"
            | "dist.ini"
            | "cpanfile"
            | "cpanfile.snapshot"
            | "MANIFEST"
            | "MANIFEST.SKIP"
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    fn role(p: &str) -> FileRole {
        classify_role(&RepoRelativePath::new(p).expect("valid path"))
    }

    #[test]
    fn classifies_lib_modules() {
        assert_eq!(role("lib/Foo/Bar.pm"), FileRole::Lib);
        assert_eq!(role("Foo.pm"), FileRole::Lib);
    }

    #[test]
    fn classifies_tests() {
        assert_eq!(role("t/basic.t"), FileRole::Test);
        assert_eq!(role("xt/author/pod.t"), FileRole::Test);
        assert_eq!(role("t/lib/Helper.t"), FileRole::Test);
    }

    #[test]
    fn classifies_scripts() {
        assert_eq!(role("script/run.pl"), FileRole::Script);
        assert_eq!(role("bin/tool"), FileRole::Script);
        assert_eq!(role("misc/oneoff.pl"), FileRole::Script);
    }

    #[test]
    fn classifies_dist_metadata() {
        assert_eq!(role("META.json"), FileRole::DistMetadata);
        assert_eq!(role("Makefile.PL"), FileRole::DistMetadata);
        assert_eq!(role("cpanfile"), FileRole::DistMetadata);
        assert_eq!(role("MANIFEST.SKIP"), FileRole::DistMetadata);
    }

    #[test]
    fn classifies_pod() {
        assert_eq!(role("lib/Foo.pod"), FileRole::Pod);
    }

    #[test]
    fn generated_wins_over_extension() {
        assert_eq!(role("blib/lib/Foo/Bar.pm"), FileRole::Generated);
        assert_eq!(role("_build/lib/Foo.pm"), FileRole::Generated);
    }

    #[test]
    fn makefile_pl_is_metadata_not_script() {
        // `.PL` extension lowercases to `pl`, but the exact-name rule wins.
        assert_eq!(role("Makefile.PL"), FileRole::DistMetadata);
        assert_eq!(role("Build.PL"), FileRole::DistMetadata);
    }

    #[test]
    fn unknown_fallback() {
        assert_eq!(role("README.md"), FileRole::Unknown);
        assert_eq!(role("notes/todo.txt"), FileRole::Unknown);
    }

    #[test]
    fn classification_is_deterministic() {
        for _ in 0..8 {
            assert_eq!(role("lib/Foo.pm"), FileRole::Lib);
        }
    }
}
