//! Which source files are test-only, when the `#[cfg(test)]` is not in the file.
//!
//! `first_cfg_test_line_number` answers "where does this file's test scope
//! begin?" and that is enough for a file carrying an inline `#[cfg(test)] mod
//! tests { … }`. It is not enough when the whole file *is* the test module: the
//! attribute then sits on the parent's declaration,
//!
//! ```text
//! // crates/perl-lsp-rs/src/runtime/lifecycle/mod.rs
//! #[cfg(test)]
//! mod final_surface_census;
//! ```
//!
//! and the child file contains no `#[cfg(test)]` line at all. A file-local scan
//! reads every line of it as production.
//!
//! That is not hypothetical: it is what put 14 phantom production `expect` sites
//! into `check-unwraps-prod` against a baseline of 1, and kept #13838 open for
//! three weeks asking whether a module that has never been compiled into a
//! non-test build was production-reachable. Clippy got the same tree right,
//! because it runs after `cfg` expansion rather than over lines of text.
//!
//! This module resolves the declarations instead, so a checker can ask the
//! question the compiler answers.

use color_eyre::eyre::Result;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{first_cfg_test_line_number, read_lines};

/// Files declared by `mod <name>;` anywhere at or after `path`'s first
/// `#[cfg(test)]` boundary.
///
/// Moved here from the panic-test command unchanged. It is deliberately
/// over-inclusive: it sweeps every `mod` declaration after the boundary, not
/// only the ones the attribute guards. For panic-test that is the safe
/// direction — the worst case is scanning a file that holds no test panics.
///
/// **Do not use it to exclude a file from a production check.** There the same
/// over-inclusion is a false negative. Use [`cfg_test_declared_module_files`].
pub(crate) fn external_test_module_files(path: &Path, lines: &[String]) -> Vec<PathBuf> {
    let Some(start_line) = first_cfg_test_line_number(path).ok().filter(|line| *line != usize::MAX)
    else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for line in lines.iter().skip(start_line.saturating_sub(1)) {
        push_declared_module_files(path, line, &mut files);
    }
    files
}

/// Files declared by a `mod <name>;` that carries its own bare `#[cfg(test)]`.
///
/// The precise question, for the exclusion direction. An attribute guards the
/// next item and nothing else, so this pairs each declaration with the
/// attributes immediately above it rather than with the first `#[cfg(test)]`
/// anywhere in the file.
///
/// The difference is not academic. `crates/perl-lsp-rs/src/runtime/mod.rs:46`
/// carries `#[cfg(all(test, feature = "workspace"))]` over the declaration on
/// line 47, and `mod workspace;` on line 61 is ungated production code holding
/// two `unsafe impl` blocks. Sweeping from the boundary hides them.
///
/// Resolves `#[path = "…"]` and inline `mod name { … }` nesting, because this
/// repository uses both: 232 module declarations carry `#[path]`, 31 of them
/// under `#[cfg(test)]`.
fn cfg_test_declared_module_files(path: &Path, lines: &[String]) -> Vec<PathBuf> {
    let Some(base) = module_directory(path) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    let mut gated = false;
    let mut redirect: Option<String> = None;
    let mut inline: Vec<(usize, String)> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        // Close any inline module whose body ended at or above this indent.
        if trimmed.starts_with('}') {
            while inline.last().is_some_and(|(open, _)| *open >= indent) {
                inline.pop();
            }
            gated = false;
            redirect = None;
            continue;
        }

        if trimmed.starts_with("#[") {
            // Only a bare `#[cfg(test)]` is unconditional. `cfg(all(test, …))`
            // also requires test, but reading the rest of that predicate is more
            // than this needs: leaving such a module in production scope costs a
            // checker a few extra lines, while taking it out could hide a real
            // site if the predicate were ever read wrong.
            gated |= trimmed == "#[cfg(test)]";
            if let Some(value) =
                trimmed.strip_prefix("#[path = \"").and_then(|rest| rest.strip_suffix("\"]"))
            {
                redirect = Some(value.to_string());
            }
            continue;
        }

        if let Some(name) = inline_module_name(trimmed) {
            inline.push((indent, name));
            gated = false;
            redirect = None;
            continue;
        }

        if gated {
            let mut dir = base.clone();
            for (_, segment) in &inline {
                dir = dir.join(segment);
            }
            push_module_files(&dir, trimmed, redirect.as_deref(), &mut files);
        }
        // Any item ends the attribute block, whether or not it was a `mod`.
        gated = false;
        redirect = None;
    }
    files
}

/// The directory a file's `mod name;` declarations resolve against.
///
/// `lib.rs`, `main.rs` and `mod.rs` own their containing directory; any other
/// `foo.rs` owns `foo/`. That is the 2018-edition rule, and it is why the
/// legacy sweep's `with_file_name` is not reused here.
fn module_directory(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    match path.file_stem()?.to_str()? {
        "lib" | "main" | "mod" => Some(parent.to_path_buf()),
        stem => Some(parent.join(stem)),
    }
}

fn inline_module_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_suffix('{')?.trim_end();
    let rest = rest.trim_start_matches("pub");
    let rest = match rest.find(')') {
        Some(close) if rest.trim_start().starts_with('(') => &rest[close + 1..],
        _ => rest,
    };
    let name = rest.trim().strip_prefix("mod ")?.trim();
    (!name.is_empty() && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
        .then(|| name.to_string())
}

fn push_module_files(dir: &Path, trimmed: &str, redirect: Option<&str>, files: &mut Vec<PathBuf>) {
    let Some(name) = declared_module_name(trimmed) else {
        return;
    };
    if let Some(relative) = redirect {
        let redirected = dir.join(relative);
        if redirected.is_file() {
            files.push(redirected);
        }
        return;
    }
    // Both spellings of a child module. Rust allows only one to exist, so
    // probing for each is a disambiguation, not a guess.
    let sibling = dir.join(format!("{name}.rs"));
    let nested = dir.join(&name).join("mod.rs");
    if sibling.is_file() {
        files.push(sibling);
    }
    if nested.is_file() {
        files.push(nested);
    }
}

fn declared_module_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_suffix(';')?;
    let rest = rest.trim_start_matches("pub").trim();
    let rest = match (rest.starts_with('('), rest.find(')')) {
        (true, Some(close)) => rest[close + 1..].trim(),
        _ => rest,
    };
    let name = rest.strip_prefix("mod ")?.trim();
    (!name.is_empty() && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
        .then(|| name.to_string())
}

/// Files declared by `mod <name>;` anywhere in `path`.
///
/// Used only for files already known to be test-only: a module of a test-only
/// module is test-only too, whatever its own declaration looks like.
fn all_declared_module_files(path: &Path, lines: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for line in lines {
        push_declared_module_files(path, line, &mut files);
    }
    files
}

fn push_declared_module_files(path: &Path, line: &str, files: &mut Vec<PathBuf>) {
    let Some(name) = line
        .trim()
        .strip_prefix("mod ")
        .and_then(|name| name.strip_suffix(';'))
        .map(str::trim)
        .filter(|name| {
            !name.is_empty() && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        })
    else {
        return;
    };
    // Both spellings of a child module. Rust allows only one to exist, so
    // probing for each is a disambiguation, not a guess.
    let sibling = path.with_file_name(format!("{name}.rs"));
    let nested = path.with_file_name(name).join("mod.rs");
    if sibling.is_file() {
        files.push(sibling);
    }
    if nested.is_file() {
        files.push(nested);
    }
}

/// Every file in `files` that is compiled only under `cfg(test)` because an
/// ancestor module declared it that way.
///
/// Transitive, because a test-only `mod.rs` makes its whole subtree test-only.
/// The closure terminates: each pass can only add files from the finite input
/// set, and a file already in the set is never expanded twice.
///
/// Deliberately narrow. Only a bare `#[cfg(test)]` declaration counts, which is
/// what `first_cfg_test_line_number` already treats as an unconditional test
/// boundary. `#[cfg(any(test, feature = "…"))]` is compiled into production
/// builds when the feature is on, so a file declared that way stays in scope —
/// a checker that skipped it would be wrong in the dangerous direction.
pub(crate) fn test_only_source_files(files: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let mut test_only: BTreeSet<PathBuf> = BTreeSet::new();
    let mut frontier: Vec<PathBuf> = Vec::new();

    for path in files {
        let lines = read_lines(path)?;
        for child in cfg_test_declared_module_files(path, &lines) {
            if test_only.insert(child.clone()) {
                frontier.push(child);
            }
        }
    }

    while let Some(path) = frontier.pop() {
        let lines = read_lines(&path)?;
        for child in all_declared_module_files(&path, &lines) {
            if test_only.insert(child.clone()) {
                frontier.push(child);
            }
        }
    }

    Ok(test_only)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Same shape as the panic-test command's `TempRepo`: this crate builds
    /// throwaway trees from `std::env::temp_dir` rather than taking a
    /// `tempfile` dev-dependency.
    struct Tree {
        path: PathBuf,
    }

    impl Tree {
        fn new(label: &str) -> Result<Self> {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir()
                .join(format!("perl-ci-hygiene-test-scope-{label}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn write(&self, rel: &str, contents: &str) -> Result<PathBuf> {
            let path = self.path.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, contents)?;
            Ok(path)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn child_declared_under_cfg_test_is_test_only() -> Result<()> {
        let tree = Tree::new("child")?;
        let parent = tree.write("mod.rs", "mod real;\n#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;
        let real = tree.write("real.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[parent])?;
        assert!(found.contains(&census), "the cfg(test) child must be test-only");
        assert!(!found.contains(&real), "an ungated sibling must stay in production scope");
        Ok(())
    }

    #[test]
    fn nested_mod_rs_spelling_is_resolved() -> Result<()> {
        let tree = Tree::new("nested")?;
        let parent = tree.write("mod.rs", "#[cfg(test)]\nmod census;\n")?;
        let nested = tree.write("census/mod.rs", "fn f() {}\n")?;

        assert!(test_only_source_files(&[parent])?.contains(&nested));
        Ok(())
    }

    #[test]
    fn test_only_subtree_is_transitive() -> Result<()> {
        let tree = Tree::new("transitive")?;
        let parent = tree.write("mod.rs", "#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census/mod.rs", "mod deep;\n")?;
        let deep = tree.write("census/deep.rs", "fn f() { y.unwrap(); }\n")?;

        let found = test_only_source_files(&[parent])?;
        assert!(found.contains(&census));
        assert!(found.contains(&deep), "a module of a test-only module is test-only");
        Ok(())
    }

    #[test]
    fn feature_gated_child_stays_in_production_scope() -> Result<()> {
        let tree = Tree::new("feature-gated")?;
        let parent =
            tree.write("mod.rs", "#[cfg(any(test, feature = \"extra\"))]\nmod census;\n")?;
        let census = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[parent])?;
        assert!(
            !found.contains(&census),
            "cfg(any(test, feature)) compiles in production builds; skipping it would \
             hide real sites"
        );
        Ok(())
    }

    #[test]
    fn an_ungated_sibling_after_a_cfg_test_boundary_stays_in_production_scope() -> Result<()> {
        // The exact shape of crates/perl-lsp-rs/src/runtime/mod.rs. The first
        // cfg(test) boundary appears partway down the module list; everything
        // below it is still ordinary production code. `runtime/workspace.rs`
        // holds two `unsafe impl` blocks, and sweeping from the boundary hid
        // them from check-unsafe-prod.
        let tree = Tree::new("ungated-after-boundary")?;
        let parent = tree.write(
            "mod.rs",
            "mod alpha;\n#[cfg(all(test, feature = \"w\"))]\nmod gated;\nmod workspace;\n",
        )?;
        let workspace = tree.write("workspace.rs", "unsafe impl Send for T {}\n")?;
        let alpha = tree.write("alpha.rs", "fn a() {}\n")?;
        tree.write("gated.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[parent])?;
        assert!(
            !found.contains(&workspace),
            "a declaration below a cfg(test) boundary is not itself gated"
        );
        assert!(!found.contains(&alpha));
        Ok(())
    }

    #[test]
    fn only_the_declaration_the_attribute_guards_is_taken() -> Result<()> {
        let tree = Tree::new("adjacent-only")?;
        let parent = tree.write("mod.rs", "#[cfg(test)]\nmod guarded;\nmod after;\n")?;
        let guarded = tree.write("guarded.rs", "fn f() {}\n")?;
        let after = tree.write("after.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[parent])?;
        assert!(found.contains(&guarded));
        assert!(!found.contains(&after), "an attribute guards one item, not the rest of the file");
        Ok(())
    }

    #[test]
    fn path_attribute_and_inline_module_nesting_are_resolved() -> Result<()> {
        // crates/perl-core-harness/src/lib.rs declares its test support this
        // way. 232 module declarations in this repository carry #[path], 31 of
        // them under #[cfg(test)], so this is an idiom here rather than a
        // corner: without it the checker reports two phantom production panics.
        let tree = Tree::new("path-attr")?;
        let parent = tree.write(
            "lib.rs",
            "pub mod invocation_trace {\n             \x20   #[cfg(test)]\n             \x20   #[path = \"test_support.rs\"]\n             \x20   pub(crate) mod test_support;\n             }\n",
        )?;
        let support = tree.write(
            "invocation_trace/test_support.rs",
            "fn f() { g().unwrap_or_else(|e| panic!(\"{e}\")); }\n",
        )?;

        assert!(
            test_only_source_files(&[parent])?.contains(&support),
            "a #[path] redirect inside an inline module must resolve"
        );
        Ok(())
    }

    #[test]
    fn a_non_mod_rs_file_owns_its_own_directory() -> Result<()> {
        // 2018-edition paths: `foo.rs` declaring `mod bar;` means `foo/bar.rs`,
        // not a sibling `bar.rs`.
        let tree = Tree::new("edition-paths")?;
        let parent = tree.write("foo.rs", "#[cfg(test)]\nmod bar;\n")?;
        let correct = tree.write("foo/bar.rs", "fn f() {}\n")?;
        let decoy = tree.write("bar.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[parent])?;
        assert!(found.contains(&correct));
        assert!(!found.contains(&decoy), "a sibling of foo.rs is not foo's child module");
        Ok(())
    }

    #[test]
    fn a_file_with_no_declarations_yields_nothing() -> Result<()> {
        let tree = Tree::new("no-decls")?;
        let leaf = tree.write("leaf.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        assert!(test_only_source_files(&[leaf])?.is_empty());
        Ok(())
    }

    #[test]
    fn a_declaration_naming_no_file_is_not_invented() -> Result<()> {
        let tree = Tree::new("absent")?;
        let parent = tree.write("mod.rs", "#[cfg(test)]\nmod absent;\n")?;

        assert!(test_only_source_files(&[parent])?.is_empty());
        Ok(())
    }
}
