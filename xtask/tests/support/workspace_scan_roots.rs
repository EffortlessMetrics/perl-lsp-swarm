//! Shared discovery primitive for the facade recurrence guards (#14300).
//!
//! Both facade guards scan the repository for consumers importing authority
//! through `perl-parser` compatibility paths. Which directories they scan is
//! the question this module answers, and it is the question a hand-written
//! root list answers badly: a literal list governs exactly the directories
//! somebody remembered to add, so the first workspace member declared outside
//! those prefixes is ungoverned and merges unseen. That is the recurrence
//! #14300 asks to close at the discovery primitive rather than per site.
//!
//! Roots are therefore derived from `[workspace] members` in the root
//! manifest. A member is governed the moment it is declared.

use std::{fs, path::Path};

/// Cargo workspaces that are not members of the root workspace and so never
/// appear in its member list. They are governed consumers all the same:
/// `fuzz/fuzz_targets` imports parser and semantic authority directly.
pub const EXTERNAL_WORKSPACE_ROOTS: &[&str] = &["fuzz"];

/// Directory names never walked, whatever the root. Build output is generated
/// rather than authored, and can contain vendored copies of real consumers.
pub const SKIPPED_DIR_NAMES: &[&str] = &["target"];

/// Read `[workspace] members` out of a root manifest's text.
///
/// Fails closed rather than guessing: a member carrying a glob is refused,
/// because an expansion computed here that disagreed with Cargo's own would
/// present as coverage.
pub fn declared_workspace_members(manifest: &str) -> Result<Vec<String>, String> {
    let document: toml::Table =
        manifest.parse().map_err(|error| format!("root manifest is not valid TOML: {error}"))?;
    let members = document
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "root manifest declares no [workspace] members array".to_string())?;

    let mut declared = Vec::with_capacity(members.len());
    for member in members {
        let member = member
            .as_str()
            .ok_or_else(|| format!("workspace member is not a string: {member:?}"))?;
        if member.contains('*') {
            return Err(format!(
                "workspace member {member:?} is a glob; the facade guards derive their \
                 governed surface from literal member paths and will not guess an \
                 expansion (#14300)"
            ));
        }
        declared.push(member.to_string());
    }
    Ok(declared)
}

/// The governed surface: every declared workspace member, plus the separately
/// rooted workspaces above.
pub fn scan_roots_from_manifest(manifest: &str) -> Result<Vec<String>, String> {
    let mut roots: Vec<String> = declared_workspace_members(manifest)?
        .into_iter()
        .chain(EXTERNAL_WORKSPACE_ROOTS.iter().map(|root| (*root).to_string()))
        .collect();
    roots.sort();
    roots.dedup();
    Ok(roots)
}

/// The governed surface for a repository checkout.
pub fn scan_roots(repo_root: &Path) -> Result<Vec<String>, String> {
    let manifest_path = repo_root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
    scan_roots_from_manifest(&manifest)
}

/// Whether a governed root reaches `path`: the root itself, or anything
/// beneath it.
pub fn root_covers(root: &str, path: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The falsifier this module exists for, and the reason the roots are
    /// derived rather than merely widened.
    ///
    /// A literal root set passes every coverage assertion against this
    /// repository — `crates`, `xtask`, and `fuzz` do reach today's members —
    /// and still ungoverns the first member declared outside those prefixes.
    /// Stated over manifest text, because the defect cannot be observed here
    /// without adding such a member.
    #[test]
    fn a_member_outside_the_familiar_prefixes_is_still_governed() -> Result<(), String> {
        let manifest = "\
[workspace]
members = [\"crates/perl-ast\", \"tools/analyzer\", \"xtask\"]
";
        let derived = scan_roots_from_manifest(manifest)?;
        assert!(
            derived.iter().any(|root| root_covers(root, "tools/analyzer")),
            "a member outside crates/xtask/fuzz must be governed; derived: {derived:?}"
        );
        assert!(
            !["crates", "xtask", "fuzz"].iter().any(|root| root_covers(root, "tools/analyzer")),
            "control: the literal root set this replaces must genuinely miss that member"
        );
        Ok(())
    }

    #[test]
    fn a_glob_member_fails_closed_rather_than_being_guessed() {
        let outcome = declared_workspace_members("[workspace]\nmembers = [\"crates/*\"]\n");
        assert!(
            outcome.as_ref().is_err_and(|error| error.contains("glob")),
            "a glob member must be refused, not expanded: {outcome:?}"
        );
    }

    #[test]
    fn a_manifest_without_workspace_members_fails_closed() {
        let outcome = declared_workspace_members("[package]\nname = \"solo\"\n");
        assert!(
            outcome.as_ref().is_err_and(|error| error.contains("members")),
            "a manifest declaring no members must be refused: {outcome:?}"
        );
    }

    #[test]
    fn declared_members_are_read_verbatim_and_completely() {
        let manifest = "\
[workspace]
resolver = \"2\"
members = [
    \"crates/alpha\",
    # a comment between members must not drop the one after it
    \"crates/beta\",
    \"xtask\",
]
";
        assert_eq!(
            declared_workspace_members(manifest),
            Ok(vec!["crates/alpha".to_string(), "crates/beta".to_string(), "xtask".to_string()])
        );
    }

    #[test]
    fn root_coverage_is_path_segment_wise_not_string_prefix() {
        assert!(root_covers("crates", "crates/perl-ast"));
        assert!(root_covers("xtask", "xtask"));
        assert!(!root_covers("crates", "crates-extra/thing"));
    }
}
