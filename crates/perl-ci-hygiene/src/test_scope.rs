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
use std::collections::{BTreeMap, BTreeSet};
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
/// over-inclusion is a false negative. Use [`test_only_source_files`].
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

/// One `mod <name>;` declaration, resolved to the file(s) it can name.
///
/// `gated` is true only for a bare `#[cfg(test)]` directly above the
/// declaration. Every other predicate — `cfg(all(test, …))`,
/// `cfg(any(test, feature = "…"))` — leaves the edge ungated, which keeps the
/// module in production scope. That is the safe direction: the cost is a
/// checker scanning a few extra lines, where the opposite error hides a real
/// site.
#[derive(Debug)]
struct ModuleEdge {
    files: Vec<PathBuf>,
    gated: bool,
}

/// Every module declaration in `path`, with the attribute state that guards it.
///
/// An attribute guards the next item and nothing else, so each declaration is
/// paired with the attributes immediately above it — or on the same line, which
/// is legal Rust and which an attribute-only line test never sees.
///
/// `crates/perl-lsp-rs/src/runtime/mod.rs:46` is why this matters: it carries
/// `#[cfg(all(test, feature = "workspace"))]` over line 47, and `mod workspace;`
/// on line 61 is ungated production code holding two `unsafe impl` blocks.
/// Sweeping forward from the first `#[cfg(test)]` swallows it.
fn module_edges(path: &Path, lines: &[String]) -> Vec<ModuleEdge> {
    let Some(module_dir) = module_directory(path) else {
        return Vec::new();
    };
    let Some(file_dir) = path.parent().map(Path::to_path_buf) else {
        return Vec::new();
    };

    let mut edges = Vec::new();
    let mut gated = false;
    let mut redirect: Option<String> = None;
    // Each entry is the indent that opened the block, its module name, and
    // whether the declaration that opened it was gated. A `mod` inside a
    // `#[cfg(test)] mod tests { … }` is compiled only under `cfg(test)` just
    // as surely as one whose own declaration carries the attribute, so the
    // gate has to survive entering the block rather than being reset by it.
    let mut inline: Vec<(usize, String, bool)> = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let indent = line.len() - line.trim_start().len();

        if trimmed.starts_with('}') {
            while inline.last().is_some_and(|(open, _, _)| *open >= indent) {
                inline.pop();
            }
            gated = false;
            redirect = None;
            continue;
        }

        // Consume every attribute at the head of the line, then carry on with
        // whatever follows it: `#[cfg(test)] mod tests;` is one line, not two.
        let mut rest = trimmed;
        while let Some(end) = attribute_end(rest) {
            let Some(attr) = rest.get(..=end) else {
                break;
            };
            gated |= attr == "#[cfg(test)]";
            if let Some(value) =
                attr.strip_prefix("#[path = \"").and_then(|value| value.strip_suffix("\"]"))
            {
                redirect = Some(value.to_string());
            }
            let Some(tail) = rest.get(end + 1..) else {
                break;
            };
            rest = tail.trim_start();
        }
        if rest.is_empty() {
            continue;
        }

        if let Some(name) = inline_module_name(rest) {
            inline.push((indent, name, gated));
            gated = false;
            redirect = None;
            continue;
        }

        // `include!("literal.rs")` splices a file in at this point. It is not
        // a module declaration, so nothing above reads it -- and a production
        // file reachable only that way would be absent from the production
        // closure, which is the one direction phase 3 cannot recover from.
        // `perl-parser-core`'s parser is built this way. The path is relative
        // to the directory holding *this* file, and a computed path
        // (`concat!(env!("OUT_DIR"), ...)`) is skipped: it names generated
        // source outside the scanned tree, so it cannot be excluded anyway.
        if let Some(target) = rest
            .strip_prefix("include!(\"")
            .and_then(|rest| rest.split_once("\")"))
            .map(|(path, _)| path)
        {
            let included = file_dir.join(target);
            if included.is_file() {
                edges.push(ModuleEdge { files: vec![included], gated: false });
            }
            gated = false;
            redirect = None;
            continue;
        }

        if let Some(name) = declared_module_name(rest) {
            // A `#[path]` on a declaration outside every inline module block is
            // relative to the directory holding this source file. Inside one it
            // is relative to the module directory, walked down the inline
            // chain. The module directory is also where an ordinary
            // declaration resolves.
            let base = if redirect.is_some() && inline.is_empty() {
                file_dir.clone()
            } else {
                let mut dir = module_dir.clone();
                for (_, segment, _) in &inline {
                    dir = dir.join(segment);
                }
                dir
            };
            let files = module_files(&base, &name, redirect.as_deref());
            if !files.is_empty() {
                // An enclosing gated inline module gates everything it
                // declares, however the declaration itself is spelled.
                let gated = gated || inline.iter().any(|(_, _, open_gated)| *open_gated);
                edges.push(ModuleEdge { files, gated });
            }
        }

        // Any item ends the attribute block, whether or not it was a `mod`.
        gated = false;
        redirect = None;
    }
    edges
}

/// The end index of the attribute starting at the head of `rest`, if there is
/// one. Counts brackets, so `#[cfg(any(test, feature = "x"))]` is one
/// attribute and not a prefix of one.
fn attribute_end(rest: &str) -> Option<usize> {
    if !rest.starts_with("#[") {
        return None;
    }
    let mut depth = 0usize;
    for (index, ch) in rest.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                // The `#[` guard above means the first `[` precedes every `]`,
                // so `depth` is never zero here. Saying that with `checked_sub`
                // rather than a comment keeps the arithmetic total: a closing
                // bracket with nothing open is malformed input, and malformed
                // input yields "no attribute here", not a panic in a checker.
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The directory a file's `mod name;` declarations resolve against.
///
/// `lib.rs`, `main.rs` and `mod.rs` own their containing directory; any other
/// `foo.rs` owns `foo/`. That is the 2018-edition rule, and it is why the
/// legacy sweep's `with_file_name` cannot be reused here — not for the first
/// hop and not for the transitive ones either.
fn module_directory(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    match path.file_stem()?.to_str()? {
        "lib" | "main" | "mod" => Some(parent.to_path_buf()),
        stem => Some(parent.join(stem)),
    }
}

fn inline_module_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_suffix('{')?.trim_end();
    module_name_after_visibility(rest)
}

fn declared_module_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_suffix(';')?;
    module_name_after_visibility(rest)
}

/// The module name in `pub(crate) mod foo` and its spellings, or `None` when
/// the text is not a module declaration at all.
fn module_name_after_visibility(rest: &str) -> Option<String> {
    let rest = rest.trim().strip_prefix("pub").map_or(rest, |tail| tail);
    let rest = rest.trim_start();
    let rest = match (rest.starts_with('('), rest.find(')')) {
        (true, Some(close)) => rest.get(close + 1..)?.trim(),
        _ => rest,
    };
    let name = rest.trim().strip_prefix("mod ")?.trim();
    (!name.is_empty() && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
        .then(|| name.to_string())
}

/// The file(s) a declaration can name, keeping only those that exist.
fn module_files(base: &Path, name: &str, redirect: Option<&str>) -> Vec<PathBuf> {
    if let Some(relative) = redirect {
        let redirected = base.join(relative);
        return if redirected.is_file() {
            redirected.canonicalize().into_iter().collect()
        } else {
            Vec::new()
        };
    }
    // Both spellings of a child module. Rust allows only one to exist, so
    // probing for each is a disambiguation, not a guess.
    let mut files = Vec::new();
    let sibling = base.join(format!("{name}.rs"));
    let nested = base.join(name).join("mod.rs");
    if sibling.is_file() {
        files.extend(sibling.canonicalize());
    }
    if nested.is_file() {
        files.extend(nested.canonicalize());
    }
    files
}

/// Whether the compiler reaches this file without any `mod` declaration.
fn is_crate_root(path: &Path) -> bool {
    let is_bin_target = path.parent().and_then(Path::file_name).is_some_and(|dir| dir == "bin");
    let stem = path.file_stem().and_then(|stem| stem.to_str());
    is_bin_target || matches!(stem, Some("lib" | "main" | "build"))
}

/// Every file in `files` that is compiled only under `cfg(test)` because an
/// ancestor module declared it that way.
///
/// Three properties carry the exclusion, and the dangerous direction — dropping
/// a file the compiler really does build into production — is what each of them
/// is for:
///
/// 1. **Production reachability is computed first**, from the crate roots over
///    ungated declarations only. A file the compiler can reach without passing
///    a `#[cfg(test)]` is production, whatever else also names it.
/// 2. **A gated edge only seeds test scope from a production-reachable file.**
///    A declaration in a file nothing reaches establishes nothing.
/// 3. **Production reachability wins the tie.** One gated reference does not
///    make a physical file test-only when a production declaration or `#[path]`
///    alias also names it, so the intersection stays in scope.
///
/// Transitive, because a test-only module makes its whole subtree test-only,
/// and resolved through the same 2018-edition rule at every hop rather than by
/// looking beside the declaring file. The closure terminates: each pass can
/// only add files from the finite input set, and a file already in the set is
/// never expanded twice.
pub(crate) fn test_only_source_files(files: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    // Reachability is a question about files, not about spellings. A
    // `#[path = "../shared.rs"]` in `live/mod.rs` resolves to
    // `live/../shared.rs`: the same file as `shared.rs` and a different
    // `PathBuf`. Compared as written, a production alias and a test alias for
    // one file never meet, and rule (3) below could not do its job. So the
    // graph is keyed canonically and mapped back to the caller's own paths at
    // the end.
    let mut canonical: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for path in files {
        if let Ok(real) = path.canonicalize() {
            canonical.entry(real).or_insert_with(|| path.clone());
        }
    }

    let mut edges: BTreeMap<PathBuf, Vec<ModuleEdge>> = BTreeMap::new();
    for (real, path) in &canonical {
        let lines = read_lines(path)?;
        edges.insert(real.clone(), module_edges(path, &lines));
    }

    // (1) What the compiler reaches without crossing a `#[cfg(test)]`.
    let mut production: BTreeSet<PathBuf> =
        canonical.keys().filter(|path| is_crate_root(path)).cloned().collect();
    let mut frontier: Vec<PathBuf> = production.iter().cloned().collect();
    while let Some(path) = frontier.pop() {
        for edge in edges.get(&path).into_iter().flatten() {
            if edge.gated {
                continue;
            }
            for child in &edge.files {
                if production.insert(child.clone()) {
                    frontier.push(child.clone());
                }
            }
        }
    }

    // (2) What a gated declaration in production code puts behind `cfg(test)`,
    // and everything those modules declare in turn.
    let mut test_only: BTreeSet<PathBuf> = BTreeSet::new();
    let mut frontier: Vec<PathBuf> = Vec::new();
    for (path, file_edges) in &edges {
        if !production.contains(path) {
            continue;
        }
        for edge in file_edges.iter().filter(|edge| edge.gated) {
            for child in &edge.files {
                if test_only.insert(child.clone()) {
                    frontier.push(child.clone());
                }
            }
        }
    }
    while let Some(path) = frontier.pop() {
        for edge in edges.get(&path).into_iter().flatten() {
            for child in &edge.files {
                if test_only.insert(child.clone()) {
                    frontier.push(child.clone());
                }
            }
        }
    }

    // (3) A file production also reaches is production.
    Ok(test_only.difference(&production).filter_map(|real| canonical.get(real).cloned()).collect())
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

    // Every fixture is rooted at a `lib.rs`, because reachability from a crate
    // root is what decides production scope. A fixture that handed the
    // resolver a bare `mod.rs` would be asking a question the compiler never
    // asks.

    #[test]
    fn child_declared_under_cfg_test_is_test_only() -> Result<()> {
        let tree = Tree::new("child")?;
        let root = tree.write("lib.rs", "mod real;\n#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;
        let real = tree.write("real.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[root, census.clone(), real.clone()])?;
        assert!(found.contains(&census), "the cfg(test) child must be test-only");
        assert!(!found.contains(&real), "an ungated sibling must stay in production scope");
        Ok(())
    }

    #[test]
    fn nested_mod_rs_spelling_is_resolved() -> Result<()> {
        let tree = Tree::new("nested")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod census;\n")?;
        let nested = tree.write("census/mod.rs", "fn f() {}\n")?;

        assert!(test_only_source_files(&[root, nested.clone()])?.contains(&nested));
        Ok(())
    }

    #[test]
    fn test_only_subtree_is_transitive() -> Result<()> {
        let tree = Tree::new("transitive")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census/mod.rs", "mod deep;\n")?;
        let deep = tree.write("census/deep.rs", "fn f() { y.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, census.clone(), deep.clone()])?;
        assert!(found.contains(&census));
        assert!(found.contains(&deep), "a module of a test-only module is test-only");
        Ok(())
    }

    /// The finding that falsified the first version of this module.
    ///
    /// A test-only `census.rs` declaring `mod rows;` means `census/rows.rs`,
    /// not the sibling `rows.rs`. Resolving transitive edges beside the
    /// declaring file gets it wrong in both directions at once: it misses the
    /// real test child and it excludes a production file that merely shares
    /// the name. `protocol/final_surface_inventory.rs` -> `rows.rs` is the
    /// live instance in this repository.
    #[test]
    fn transitive_edges_resolve_under_the_module_directory_not_beside_the_file() -> Result<()> {
        let tree = Tree::new("transitive-dir")?;
        let root = tree.write("lib.rs", "mod rows;\n#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census.rs", "mod rows;\n")?;
        let test_child = tree.write("census/rows.rs", "fn f() { y.unwrap(); }\n")?;
        let production_sibling = tree.write("rows.rs", "fn g() { z.unwrap(); }\n")?;

        let found = test_only_source_files(&[
            root,
            census.clone(),
            test_child.clone(),
            production_sibling.clone(),
        ])?;
        assert!(found.contains(&census));
        assert!(found.contains(&test_child), "census/rows.rs is the module census.rs declares");
        assert!(
            !found.contains(&production_sibling),
            "the sibling rows.rs is a different module and stays in production scope"
        );
        Ok(())
    }

    /// One gated reference does not make a physical file test-only.
    #[test]
    fn a_file_a_production_declaration_also_reaches_stays_in_scope() -> Result<()> {
        let tree = Tree::new("shared")?;
        let root = tree.write(
            "lib.rs",
            "mod live;\n#[cfg(test)]\n#[path = \"shared.rs\"]\nmod under_test;\n",
        )?;
        let live = tree.write("live/mod.rs", "#[path = \"../shared.rs\"]\nmod shared;\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, live.clone(), shared.clone()])?;
        assert!(
            !found.contains(&shared),
            "a production #[path] alias also names this file, so it is production"
        );
        assert!(!found.contains(&live));
        Ok(())
    }

    /// The same fixture without the production alias: now the exclusion is
    /// correct, which is what makes the control above discriminating.
    #[test]
    fn the_same_file_is_test_only_once_no_production_declaration_names_it() -> Result<()> {
        let tree = Tree::new("shared-negative")?;
        let root = tree.write(
            "lib.rs",
            "mod live;\n#[cfg(test)]\n#[path = \"shared.rs\"]\nmod under_test;\n",
        )?;
        let live = tree.write("live/mod.rs", "fn g() {}\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, live, shared.clone()])?;
        assert!(found.contains(&shared), "nothing in production reaches it now");
        Ok(())
    }

    /// `#[cfg(test)] mod tests;` on one physical line is legal Rust, and an
    /// attribute-only line test never sees it.
    #[test]
    fn an_attribute_and_its_declaration_on_one_line_are_read() -> Result<()> {
        let tree = Tree::new("same-line")?;
        let root = tree.write("lib.rs", "#[cfg(test)] mod assertions;\n")?;
        let assertions = tree.write("assertions.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        assert!(test_only_source_files(&[root, assertions.clone()])?.contains(&assertions));
        Ok(())
    }

    /// A `#[path]` outside every inline module block is relative to the
    /// directory holding the source file, not to the module directory.
    #[test]
    fn a_top_level_path_attribute_resolves_beside_its_source_file() -> Result<()> {
        let tree = Tree::new("top-level-path")?;
        let root = tree.write("lib.rs", "mod dispatch;\n")?;
        let dispatch =
            tree.write("dispatch.rs", "#[cfg(test)]\n#[path = \"cases/lifecycle.rs\"]\nmod c;\n")?;
        let beside = tree.write("cases/lifecycle.rs", "fn f() { x.unwrap(); }\n")?;
        let under_module_dir =
            tree.write("dispatch/cases/lifecycle.rs", "fn g() { z.unwrap(); }\n")?;

        let found =
            test_only_source_files(&[root, dispatch, beside.clone(), under_module_dir.clone()])?;
        assert!(found.contains(&beside), "the redirect is relative to dispatch.rs's own directory");
        assert!(
            !found.contains(&under_module_dir),
            "dispatch/cases/lifecycle.rs is not what that attribute names"
        );
        Ok(())
    }

    #[test]
    fn feature_gated_child_stays_in_production_scope() -> Result<()> {
        let tree = Tree::new("feature-gated")?;
        let root = tree.write("lib.rs", "#[cfg(any(test, feature = \"extra\"))]\nmod census;\n")?;
        let census = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        assert!(
            !test_only_source_files(&[root, census.clone()])?.contains(&census),
            "a feature-gated module is compiled into production when the feature is on"
        );
        Ok(())
    }

    /// The near-miss control. The first version of this change reused the
    /// panic-test sweep, which takes every `mod` after a file's first
    /// `#[cfg(test)]`, and hid two real `unsafe impl` blocks in
    /// `runtime/workspace.rs`.
    #[test]
    fn an_ungated_sibling_after_a_cfg_test_boundary_stays_in_production_scope() -> Result<()> {
        let tree = Tree::new("boundary")?;
        let root = tree.write(
            "lib.rs",
            "#[cfg(all(test, feature = \"workspace\"))]\nmod harness;\n\nmod workspace;\n",
        )?;
        let harness = tree.write("harness.rs", "fn f() {}\n")?;
        let workspace = tree.write("workspace.rs", "unsafe impl Send for X {}\n")?;

        let found = test_only_source_files(&[root, harness, workspace.clone()])?;
        assert!(
            !found.contains(&workspace),
            "an ungated declaration below a gated one is still production code"
        );
        Ok(())
    }

    #[test]
    fn only_the_declaration_the_attribute_guards_is_taken() -> Result<()> {
        let tree = Tree::new("guarded")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod gated;\nmod ungated;\n")?;
        let gated = tree.write("gated.rs", "fn f() {}\n")?;
        let ungated = tree.write("ungated.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[root, gated.clone(), ungated.clone()])?;
        assert!(found.contains(&gated));
        assert!(
            !found.contains(&ungated),
            "an attribute guards one item, not the rest of the file"
        );
        Ok(())
    }

    #[test]
    fn path_attribute_and_inline_module_nesting_are_resolved() -> Result<()> {
        let tree = Tree::new("path-inline")?;
        let root = tree.write(
            "lib.rs",
            "mod outer {\n    #[cfg(test)]\n    #[path = \"redirected.rs\"]\n    mod inner;\n}\n",
        )?;
        let redirected = tree.write("outer/redirected.rs", "fn f() { x.unwrap(); }\n")?;

        assert!(test_only_source_files(&[root, redirected.clone()])?.contains(&redirected));
        Ok(())
    }

    /// A `#[cfg(test)]` on an inline module block gates every file that block
    /// declares. The first version pushed the block onto the inline stack and
    /// reset `gated` in the same step, so `helper` below came out as an ungated
    /// edge and the production closure reached a file the compiler builds only
    /// under `cfg(test)`.
    #[test]
    fn a_gated_inline_module_gates_the_files_it_declares() -> Result<()> {
        let tree = Tree::new("gated-inline")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod tests {\n    mod helper;\n}\n")?;
        let helper = tree.write("tests/helper.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        assert!(
            test_only_source_files(&[root, helper.clone()])?.contains(&helper),
            "a file declared inside a gated inline module is test-only"
        );
        Ok(())
    }

    /// The companion: the gate must not leak back out of the block it opened.
    #[test]
    fn a_declaration_after_a_gated_inline_block_closes_is_not_gated() -> Result<()> {
        let tree = Tree::new("gate-scope")?;
        let root =
            tree.write("lib.rs", "#[cfg(test)]\nmod tests {\n    mod helper;\n}\n\nmod live;\n")?;
        let helper = tree.write("tests/helper.rs", "fn f() {}\n")?;
        let live = tree.write("live.rs", "fn g() { y.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, helper.clone(), live.clone()])?;
        assert!(found.contains(&helper));
        assert!(!found.contains(&live), "the gate ended with the block that opened it");
        Ok(())
    }

    /// The exclusion path's one genuinely dangerous asymmetry: an *unread*
    /// production edge paired with a *read* gated one. `include!` is the form
    /// that actually occurs here -- `perl-parser-core`'s parser is assembled
    /// from thirteen of them -- so a literal `include!` is read as a production
    /// edge. Without that, `shared.rs` below is absent from the production
    /// closure and the gated alias takes a file the compiler builds into
    /// production straight out of scope.
    #[test]
    fn a_file_reached_only_through_an_include_stays_in_production_scope() -> Result<()> {
        let tree = Tree::new("include-prod")?;
        let root = tree.write(
            "lib.rs",
            "mod engine;\n#[cfg(test)]\n#[path = \"engine/shared.rs\"]\nmod under_test;\n",
        )?;
        let engine = tree.write("engine/mod.rs", "include!(\"shared.rs\");\n")?;
        let shared = tree.write("engine/shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, engine, shared.clone()])?;
        assert!(
            !found.contains(&shared),
            "an include! reaches it in production, so a gated alias cannot exclude it"
        );
        Ok(())
    }

    /// A computed `include!` names generated source outside the scanned tree,
    /// so it resolves to nothing rather than to a wrong path.
    #[test]
    fn a_computed_include_path_is_not_invented() -> Result<()> {
        let tree = Tree::new("include-computed")?;
        let root = tree.write(
            "lib.rs",
            "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n#[cfg(test)]\nmod census;\n",
        )?;
        let census = tree.write("census.rs", "fn f() { x.unwrap(); }\n")?;

        assert!(test_only_source_files(&[root, census.clone()])?.contains(&census));
        Ok(())
    }

    #[test]
    fn a_non_mod_rs_file_owns_its_own_directory() -> Result<()> {
        let tree = Tree::new("owns-dir")?;
        let root = tree.write("lib.rs", "mod runtime;\n")?;
        let runtime = tree.write("runtime.rs", "#[cfg(test)]\nmod cases;\n")?;
        let cases = tree.write("runtime/cases.rs", "fn f() {}\n")?;
        let decoy = tree.write("cases.rs", "fn g() { w.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, runtime, cases.clone(), decoy.clone()])?;
        assert!(found.contains(&cases));
        assert!(!found.contains(&decoy), "the sibling cases.rs is a different module");
        Ok(())
    }

    #[test]
    fn a_file_with_no_declarations_yields_nothing() -> Result<()> {
        let tree = Tree::new("empty")?;
        let root = tree.write("lib.rs", "fn f() {}\n")?;

        assert!(test_only_source_files(&[root])?.is_empty());
        Ok(())
    }

    #[test]
    fn a_declaration_naming_no_file_is_not_invented() -> Result<()> {
        let tree = Tree::new("absent")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod nothing_here;\n")?;

        assert!(test_only_source_files(&[root])?.is_empty());
        Ok(())
    }

    /// A gated declaration in a file nothing reaches establishes nothing: the
    /// module it names stays in scope rather than being excluded on the word
    /// of an unreachable parent.
    #[test]
    fn a_gated_declaration_in_an_unreachable_file_excludes_nothing() -> Result<()> {
        let tree = Tree::new("orphan")?;
        let root = tree.write("lib.rs", "fn f() {}\n")?;
        let orphan = tree.write("orphan.rs", "#[cfg(test)]\nmod child;\n")?;
        let child = tree.write("orphan/child.rs", "fn g() { q.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, orphan, child.clone()])?;
        assert!(found.is_empty(), "unexpected exclusions: {found:?}");
        Ok(())
    }

    #[test]
    fn an_attribute_with_nested_brackets_is_one_attribute() -> Result<()> {
        assert_eq!(attribute_end("#[cfg(test)]"), Some(11));
        let nested = "#[cfg(any(test, feature = \"x\"))]";
        assert_eq!(
            attribute_end(&format!("{nested} mod m;")).map(|end| end + 1),
            Some(nested.len()),
            "the whole attribute, not a prefix ending at the first closing bracket"
        );
        assert_eq!(attribute_end("mod m;"), None);
        Ok(())
    }
}
