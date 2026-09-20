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
/// Moved here from the panic-test command. It is deliberately over-inclusive:
/// it sweeps every `mod` declaration after the boundary, not only the ones the
/// attribute guards. For panic-test that is the safe direction — the worst case
/// is scanning a file that holds no test panics.
///
/// Over-inclusive is a choice about *which declarations* to follow. Where a
/// declaration leads is not a choice, so the hop resolves through
/// [`module_directory`] like every other hop in this module. The version that
/// came over from the panic-test command used `with_file_name`, which reads a
/// declaration in `foo.rs` as naming a file beside `foo.rs` instead of one
/// inside `foo/`. Twenty-seven test-only module files across twelve crates
/// resolve only under the 2018 rule — among them
/// `xtask/src/tasks/emacs_train_packet/tests.rs`, whose six panic sites the
/// inventory never saw.
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
    let Some(name) = declared_module_name(line.trim()) else {
        return;
    };
    let Some(base) = module_directory(path) else {
        return;
    };
    // Both spellings of a child module. Rust allows only one to exist, so
    // probing for each is a disambiguation, not a guess. Paths stay as built
    // rather than canonicalized: the caller strips `repo_root` off them to
    // form the identity a registry row is keyed on.
    for candidate in [base.join(format!("{name}.rs")), base.join(&name).join("mod.rs")] {
        if candidate.is_file() {
            files.push(candidate);
        }
    }
}

/// One `mod <name>;` declaration, resolved to the file(s) it can name.
///
/// `gated` is true when an attribute directly above the declaration carries a
/// `cfg` predicate that cannot hold outside a test build — `cfg(test)` itself,
/// or an `all(…)` with such a predicate among its conjuncts. A predicate that
/// can hold in a production build, `cfg(any(test, feature = "…"))` among them,
/// leaves the edge ungated and the module in production scope.
///
/// Gating is safe here and would not be safe in a pre-filter over the file
/// list, because phase 3 of [`test_only_source_files`] lets any production
/// declaration reaching the same file win the tie. A file excluded before that
/// phase runs never reaches the rule that would have protected it.
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
            gated |= attribute_is_a_test_gate(attr);
            if let Some(value) = path_attribute_target(attr) {
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
            // Canonicalized for the same reason `module_files` canonicalizes:
            // the graph is keyed on canonical paths. An include spelled
            // `live/../shared.rs` would otherwise enter the production set
            // under a `PathBuf` that never equals the gated alias's canonical
            // spelling of the same file, so phase 3's `difference` would not
            // let production win the tie and would exclude a file the compiler
            // builds. One file, two spellings, opposite verdicts.
            if let Ok(included) = file_dir.join(target).canonicalize()
                && included.is_file()
            {
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

/// The file `#[path = "…"]` redirects a declaration to, or `None` when `attr`
/// is some other attribute.
///
/// Whitespace around `=` is optional in Rust, so `#[path="cases/lifecycle.rs"]`
/// names the same file as the spaced spelling. Reading only one of them sends
/// the declaration back to its plain module name, which resolves to a different
/// file or to none — and a gated declaration that lands on the wrong file
/// classifies that file as test-only. The repository happens to write all 221 of
/// its `#[path]` attributes with spaces today, so this is the parser agreeing
/// with the language rather than with the current tree.
fn path_attribute_target(attr: &str) -> Option<&str> {
    attr.strip_prefix("#[path")?
        .trim_start()
        .strip_prefix('=')?
        .trim_start()
        .strip_prefix('"')?
        .split_once('"')
        .map(|(target, _)| target)
}

/// Whether `attr` is a `cfg` attribute whose predicate cannot hold outside a
/// test build.
fn attribute_is_a_test_gate(attr: &str) -> bool {
    attr.strip_prefix("#[cfg(")
        .and_then(|rest| rest.strip_suffix(")]"))
        .is_some_and(predicate_is_test_only)
}

/// Whether a `cfg` predicate can only hold when `test` is set.
///
/// `test` itself qualifies. So does `all(…)` with a qualifying conjunct: every
/// conjunct of an `all` must hold, so one that cannot hold outside a test build
/// makes the whole predicate unsatisfiable outside one.
///
/// `any(…)` never qualifies — it holds when a single arm does, and an arm like
/// `feature = "workspace"` holds in a production build. `not(…)` never
/// qualifies either, and `not(test)` is the opposite claim.
///
/// `#[cfg(all(test, feature = "workspace"))]` at
/// `crates/perl-lsp-rs/src/runtime/mod.rs:46` is the live instance. Reading it
/// as production put a module no production build compiles back into scope,
/// which is what #16251's own resolver got right and this one did not.
fn predicate_is_test_only(predicate: &str) -> bool {
    let predicate = predicate.trim();
    if predicate == "test" {
        return true;
    }
    let Some(inner) = predicate.strip_prefix("all(").and_then(|rest| rest.strip_suffix(')')) else {
        return false;
    };
    conjuncts(inner).iter().any(|arm| predicate_is_test_only(arm))
}

/// `inner` split on the commas that separate one predicate from the next.
///
/// Splits at parenthesis depth zero only, and skips commas inside a string
/// literal, so `all(test, feature = "a,b")` is two conjuncts rather than three.
fn conjuncts(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut start = 0usize;
    let mut previous = '\0';
    for (index, ch) in inner.char_indices() {
        if in_string {
            if ch == '"' && previous != '\\' {
                in_string = false;
            }
        } else {
            match ch {
                '"' => in_string = true,
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    if let Some(part) = inner.get(start..index) {
                        parts.push(part);
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
        previous = ch;
    }
    if let Some(part) = inner.get(start..) {
        parts.push(part);
    }
    parts
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
    use color_eyre::eyre::ensure;

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

    /// The panic-test sweep resolves its hop the same way every other hop in
    /// this module does. `census.rs` owns `census/`, so `mod rows;` inside it
    /// names `census/rows.rs` — never the `rows.rs` lying beside it, which is
    /// a different module belonging to the crate root.
    #[test]
    fn the_panic_sweep_resolves_a_child_under_the_module_directory() -> Result<()> {
        let tree = Tree::new("sweep-dir")?;
        let census = tree.write("census.rs", "#[cfg(test)]\nmod rows;\n")?;
        let test_child = tree.write("census/rows.rs", "fn f() { panic!(\"boom\"); }\n")?;
        let unrelated_sibling = tree.write("rows.rs", "fn g() {}\n")?;

        let found = external_test_module_files(&census, &read_lines(&census)?);
        ensure!(found.contains(&test_child), "census/rows.rs is the module census.rs declares");
        ensure!(
            !found.contains(&unrelated_sibling),
            "the sibling rows.rs belongs to the crate root, not to census"
        );
        Ok(())
    }

    #[test]
    fn child_declared_under_cfg_test_is_test_only() -> Result<()> {
        let tree = Tree::new("child")?;
        let root = tree.write("lib.rs", "mod real;\n#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;
        let real = tree.write("real.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[root, census.clone(), real.clone()])?;
        ensure!(found.contains(&census), "the cfg(test) child must be test-only");
        ensure!(!found.contains(&real), "an ungated sibling must stay in production scope");
        Ok(())
    }

    #[test]
    fn nested_mod_rs_spelling_is_resolved() -> Result<()> {
        let tree = Tree::new("nested")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod census;\n")?;
        let nested = tree.write("census/mod.rs", "fn f() {}\n")?;

        let found = test_only_source_files(&[root, nested.clone()])?;
        ensure!(found.contains(&nested), "the mod.rs spelling resolves; found {found:?}");
        Ok(())
    }

    #[test]
    fn test_only_subtree_is_transitive() -> Result<()> {
        let tree = Tree::new("transitive")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census/mod.rs", "mod deep;\n")?;
        let deep = tree.write("census/deep.rs", "fn f() { y.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, census.clone(), deep.clone()])?;
        ensure!(found.contains(&census));
        ensure!(found.contains(&deep), "a module of a test-only module is test-only");
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
        ensure!(found.contains(&census));
        ensure!(found.contains(&test_child), "census/rows.rs is the module census.rs declares");
        ensure!(
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
        ensure!(
            !found.contains(&shared),
            "a production #[path] alias also names this file, so it is production"
        );
        ensure!(!found.contains(&live));
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
        ensure!(found.contains(&shared), "nothing in production reaches it now");
        Ok(())
    }

    /// `#[cfg(test)] mod tests;` on one physical line is legal Rust, and an
    /// attribute-only line test never sees it.
    #[test]
    fn an_attribute_and_its_declaration_on_one_line_are_read() -> Result<()> {
        let tree = Tree::new("same-line")?;
        let root = tree.write("lib.rs", "#[cfg(test)] mod assertions;\n")?;
        let assertions = tree.write("assertions.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, assertions.clone()])?;
        ensure!(found.contains(&assertions), "the redirected file is test-only; found {found:?}");
        Ok(())
    }

    /// Rust does not require whitespace around the `=` in an attribute, so the
    /// parser must not either. Reading only the spaced spelling sends the
    /// declaration back to its plain module name — here `under_test.rs`, which
    /// does not exist — and the redirect target stays in production scope.
    #[test]
    fn a_path_attribute_without_spaces_names_the_same_file() -> Result<()> {
        let tree = Tree::new("unspaced-path")?;
        let root =
            tree.write("lib.rs", "#[cfg(test)]\n#[path=\"shared.rs\"]\nmod under_test;\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, shared.clone()])?;
        ensure!(
            found.contains(&shared),
            "#[path=\"…\"] redirects the gated declaration just as #[path = \"…\"] does"
        );
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
        ensure!(found.contains(&beside), "the redirect is relative to dispatch.rs's own directory");
        ensure!(
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

        let found = test_only_source_files(&[root, census.clone()])?;
        ensure!(
            !found.contains(&census),
            "a feature-gated module is compiled into production when the feature is on; found {found:?}"
        );
        Ok(())
    }

    /// `all(test, …)` cannot hold outside a test build, so the module it
    /// guards is test-only. Carried over from #16251's `is_cfg_test_module_file`,
    /// which read this correctly where this module did not, before that
    /// resolver was removed as duplicate authority. The live instance is
    /// `crates/perl-lsp-rs/src/runtime/mod.rs:46`.
    #[test]
    fn a_conjunction_requiring_test_gates_the_module_it_guards() -> Result<()> {
        let tree = Tree::new("all-test")?;
        let root = tree.write(
            "lib.rs",
            "#[cfg(all(test, feature = \"workspace\"))]\nmod scan_gate_observation;\n",
        )?;
        let child = tree.write("scan_gate_observation.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, child.clone()])?;
        ensure!(
            found.contains(&child),
            "all(test, …) requires test, so no production build compiles this; found {found:?}"
        );
        Ok(())
    }

    /// The opposite direction, so the conjunction rule cannot be satisfied by
    /// matching the word `test` anywhere in a predicate.
    #[test]
    fn a_disjunction_offering_test_does_not_gate_the_module() -> Result<()> {
        let tree = Tree::new("any-test")?;
        let root =
            tree.write("lib.rs", "#[cfg(any(test, feature = \"workspace\"))]\nmod census;\n")?;
        let child = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, child.clone()])?;
        ensure!(
            !found.contains(&child),
            "any(test, feature) holds with the feature alone, so this is production; found {found:?}"
        );
        Ok(())
    }

    /// A negation naming `test` is the opposite claim, and a conjunct that
    /// merely contains one is not one.
    #[test]
    fn a_negated_or_nested_predicate_is_read_for_what_it_means() -> Result<()> {
        ensure!(predicate_is_test_only("test"), "the bare predicate gates");
        ensure!(
            predicate_is_test_only("all(feature = \"a\", all(test, feature = \"b\"))"),
            "a conjunction nested inside a conjunction still requires test"
        );
        ensure!(!predicate_is_test_only("not(test)"), "not(test) is production only");
        ensure!(
            !predicate_is_test_only("all(not(test), feature = \"a\")"),
            "a conjunct that negates test does not gate"
        );
        ensure!(
            !predicate_is_test_only("all(feature = \"tested\")"),
            "a feature whose name contains test is not the test predicate"
        );
        ensure!(
            !predicate_is_test_only("any(all(test, feature = \"a\"), feature = \"b\")"),
            "a disjunction is satisfiable by its other arm"
        );
        Ok(())
    }

    /// A comma inside a string literal does not start a new conjunct.
    #[test]
    fn a_comma_inside_a_feature_name_does_not_split_the_predicate() -> Result<()> {
        ensure!(
            predicate_is_test_only("all(test, feature = \"a,b\")"),
            "the literal carries the comma; the conjunction is still two arms"
        );
        ensure!(
            !predicate_is_test_only("all(feature = \"a,test\")"),
            "a comma inside a literal must not manufacture a bare test conjunct"
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
        ensure!(
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
        ensure!(found.contains(&gated));
        ensure!(
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

        let found = test_only_source_files(&[root, redirected.clone()])?;
        ensure!(found.contains(&redirected), "the nested redirect resolves; found {found:?}");
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

        let found = test_only_source_files(&[root, helper.clone()])?;
        ensure!(
            found.contains(&helper),
            "a file declared inside a gated inline module is test-only; found {found:?}"
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
        ensure!(found.contains(&helper));
        ensure!(!found.contains(&live), "the gate ended with the block that opened it");
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
        ensure!(
            !found.contains(&shared),
            "an include! reaches it in production, so a gated alias cannot exclude it"
        );
        Ok(())
    }

    /// The same file named two ways: production reaches it through an
    /// `include!` spelled with a `..` hop, and a gated `#[path]` alias names
    /// it directly. Production has to win that tie, which it can only do if
    /// both spellings reduce to one identity.
    #[test]
    fn an_include_spelled_with_a_parent_hop_still_overrides_a_gated_alias() -> Result<()> {
        let tree = Tree::new("include-altspelling")?;
        let root = tree.write(
            "lib.rs",
            "mod engine;\n#[cfg(test)]\n#[path = \"engine/shared.rs\"]\nmod under_test;\n",
        )?;
        let engine = tree.write("engine/mod.rs", "include!(\"live/../shared.rs\");\n")?;
        let shared = tree.write("engine/shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;
        tree.write("engine/live/placeholder.rs", "// keeps `live/` on disk\n")?;

        let found = test_only_source_files(&[root, engine, shared.clone()])?;
        ensure!(
            !found.contains(&shared),
            "`live/../shared.rs` and `shared.rs` are one file; the production include must override the gated alias"
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

        let found = test_only_source_files(&[root, census.clone()])?;
        ensure!(
            found.contains(&census),
            "an uninvented include! leaves the gate intact; found {found:?}"
        );
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
        ensure!(found.contains(&cases));
        ensure!(!found.contains(&decoy), "the sibling cases.rs is a different module");
        Ok(())
    }

    #[test]
    fn a_file_with_no_declarations_yields_nothing() -> Result<()> {
        let tree = Tree::new("empty")?;
        let root = tree.write("lib.rs", "fn f() {}\n")?;

        let found = test_only_source_files(&[root])?;
        ensure!(found.is_empty(), "a file declaring nothing excludes nothing; found {found:?}");
        Ok(())
    }

    #[test]
    fn a_declaration_naming_no_file_is_not_invented() -> Result<()> {
        let tree = Tree::new("absent")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod nothing_here;\n")?;

        let found = test_only_source_files(&[root])?;
        ensure!(
            found.is_empty(),
            "a declaration naming no file resolves to nothing; found {found:?}"
        );
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
        ensure!(found.is_empty(), "unexpected exclusions: {found:?}");
        Ok(())
    }

    #[test]
    fn an_attribute_with_nested_brackets_is_one_attribute() -> Result<()> {
        let end = attribute_end("#[cfg(test)]");
        ensure!(end == Some(11), "a flat attribute ends at its own bracket, got {end:?}");
        let nested = "#[cfg(any(test, feature = \"x\"))]";
        let nested_end = attribute_end(&format!("{nested} mod m;")).map(|end| end + 1);
        ensure!(
            nested_end == Some(nested.len()),
            "the whole attribute, not a prefix ending at the first closing bracket; got {nested_end:?}"
        );
        let none = attribute_end("mod m;");
        ensure!(none.is_none(), "a line carrying no attribute ends nowhere, got {none:?}");
        Ok(())
    }
}
