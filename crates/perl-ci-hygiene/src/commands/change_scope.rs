//! Post-hoc scope gate: fail the change when it touches files outside its
//! declared scope (#17429, T6 Option 2).
//!
//! Trap T6 (scope creep) reports UNGUARDED because writer admission is
//! advisory-first: `writer_admission::run` always returns `Ok`, and no
//! in-repo seam routes agent file writes through an admission check (the
//! verdict has exactly one consumer — the diagnostic CLI itself — and agent
//! edits flow through harness tooling, not repository code). With no clean
//! write seam, the issue falls back to a required-tier post-hoc gate: this
//! check diffs the change against an admitted scope and fails naming every
//! unadmitted file.
//!
//! The admitted scope is a declaration committed **by the change itself** at
//! [`SCOPE_DECLARATION_PATH`], one pattern per line:
//!
//! - an exact repo-relative path (`crates/foo/src/lib.rs`), or
//! - a directory prefix with a trailing slash (`crates/foo/`), admitting
//!   everything beneath it.
//!
//! Blank lines and `#` comments are ignored. There is deliberately no glob
//! syntax: a line containing `*?[]` is a hard error naming its line number,
//! so a misread declaration fails loudly instead of silently admitting
//! nothing (or, worse, being misread as admitting everything).
//!
//! Three honesty rules keep the gate from lying or from breaking changes that
//! never opted in:
//!
//! - **No declaration, no verdict.** A worktree without the scope file exits 0
//!   reporting "not evaluated" — exactly like `must_context`'s unevaluated
//!   run. The gate constrains only changes that declare a scope, so landing
//!   it required breaks no existing PR.
//! - **Stale declarations are inert.** A scope file byte-identical to its base
//!   blob was not written by this change (it rode in on the base), so it is
//!   ignored rather than enforced. Scope cannot leak from one change into the
//!   next, and a merged scope file constrains nobody.
//! - **The declaration admits itself.** The scope file is always exempt from
//!   its own check; otherwise declaring a scope would itself be a violation.
//!
//! What this proves — and what it does not: an in-scope-declared change that
//! creeps into `neighbor.rs` fails merge-blocking with the file named. A
//! change that declares nothing is unevaluated (voluntary adoption, recorded
//! in the T6 trap verdict alongside the guard), and a change that widens its
//! own declaration sails — but the widening is itself a reviewed diff, so
//! silent creep becomes explicit creep. Untracked files are not in subject
//! until staged: they are not in any change-set yet.
//!
//! [`parse_scope_declaration`], [`is_admitted`], and [`find_unadmitted`] are
//! pure functions over text; [`check`] is the thin shell that obtains the
//! declaration and the diff from `git` and reports the findings.

use color_eyre::eyre::{Result, eyre};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::{GREEN, NC, RED, YELLOW};

/// Repo-relative path of the per-change scope declaration. Allowlisted for the
/// non-Rust inventory gate by `policy/non-rust-allowlist.toml`
/// (`.agents/**/*`), so declaring a scope never trips that gate.
pub(crate) const SCOPE_DECLARATION_PATH: &str = ".agents/change-scope";

/// One admitted-scope pattern: an exact repo-relative path or a directory
/// prefix (stored with its trailing slash) admitting everything beneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScopePattern {
    Exact(String),
    Prefix(String),
}

/// A parsed scope declaration: the admitted patterns in declaration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeScope {
    patterns: Vec<ScopePattern>,
}

impl ChangeScope {
    /// Returns `true` when the declaration carries no patterns. An honored
    /// but empty declaration admits nothing, so any change fails against it.
    fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    fn len(&self) -> usize {
        self.patterns.len()
    }
}

/// Normalizes one scope line or candidate path for comparison: surrounding
/// whitespace trimmed, backslashes folded to `/` (a Windows-authored
/// declaration must mean the same as a POSIX one), and a leading `./` or `/`
/// stripped so root-anchored spellings match `git diff --name-only` output.
fn normalize_path(text: &str) -> String {
    let mut normalized = text.trim().replace('\\', "/");
    if let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_owned();
    }
    if let Some(rest) = normalized.strip_prefix('/') {
        normalized = rest.to_owned();
    }
    normalized
}

/// Parses a scope declaration into its admitted patterns.
///
/// # Errors
///
/// Returns an error naming the 1-based line number when a line uses
/// unsupported glob syntax (`*?[]`) or when a line normalizes to empty after
/// stripping (which cannot happen for non-blank input, so any such error is a
/// bug made loud rather than a silent admission).
pub(crate) fn parse_scope_declaration(text: &str) -> Result<ChangeScope> {
    let mut patterns = Vec::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.contains(['*', '?', '[', ']']) {
            return Err(eyre!(
                "{SCOPE_DECLARATION_PATH}:{line_number}: unsupported glob syntax in \
                 {trimmed:?}: declare exact repo-relative paths or `dir/` prefixes, one per line"
            ));
        }
        let normalized = normalize_path(trimmed);
        if normalized.is_empty() {
            return Err(eyre!(
                "{SCOPE_DECLARATION_PATH}:{line_number}: line normalizes to an empty path"
            ));
        }
        if normalized.ends_with('/') {
            patterns.push(ScopePattern::Prefix(normalized));
        } else {
            patterns.push(ScopePattern::Exact(normalized));
        }
    }
    Ok(ChangeScope { patterns })
}

/// Returns `true` when `path` (a repo-relative changed path, as produced by
/// `git diff --name-only`) falls inside the admitted scope: an exact-pattern
/// hit, or a directory-prefix hit for anything beneath the prefix. The scope
/// declaration itself is always admitted — it is the declaration, not a
/// change under judgment. An empty candidate is never admitted.
pub(crate) fn is_admitted(path: &str, scope: &ChangeScope) -> bool {
    let normalized = normalize_path(path);
    if normalized.is_empty() {
        return false;
    }
    if normalized == SCOPE_DECLARATION_PATH {
        return true;
    }
    scope.patterns.iter().any(|pattern| match pattern {
        ScopePattern::Exact(exact) => normalized == *exact,
        ScopePattern::Prefix(prefix) => normalized.starts_with(prefix),
    })
}

/// Returns the changed paths that fall outside the admitted scope, in diff
/// order. The scope declaration itself is exempt; see [`is_admitted`].
pub(crate) fn find_unadmitted(changed: &[String], scope: &ChangeScope) -> Vec<String> {
    changed.iter().filter(|path| !is_admitted(path, scope)).cloned().collect()
}

/// Runs the guard against the change between `base` and the working tree.
///
/// Returns the process exit code: `0` when every changed file is admitted (or
/// when the gate is unevaluated — no declaration, a stale declaration, or no
/// resolvable base), `1` when the change touches unadmitted files.
///
/// # Errors
///
/// Returns an error when the declaration cannot be read or parsed, when `git`
/// cannot produce the changed-file list, or when an explicitly requested base
/// ref does not resolve. All fail closed: a change the gate cannot evaluate
/// against its own declaration must not read as clean.
pub(crate) fn check(repo_root: &Path, base: Option<&str>) -> Result<i32> {
    let Some(requested_base) = resolve_base(repo_root, base)? else {
        // Not a pass: nothing was compared. Said plainly so a green line is
        // never mistaken for evidence that the change stayed in scope.
        println!(
            "{YELLOW}• change-scope gate not evaluated{NC}: no base ref resolved (tried: {}). \
             Pass --base to name one.",
            base_candidates().join(", ")
        );
        return Ok(0);
    };
    let declaration_path = repo_root.join(SCOPE_DECLARATION_PATH);
    let declaration_bytes = match fs::read(&declaration_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{YELLOW}• change-scope gate not evaluated{NC}: no {SCOPE_DECLARATION_PATH} \
                 declaration in the worktree. Declare the change's scope there \
                 (one exact path or `dir/` prefix per line) to enforce it."
            );
            return Ok(0);
        }
        Err(error) => {
            return Err(eyre!("failed to read {SCOPE_DECLARATION_PATH}: {error}"));
        }
    };
    if base_blob_matches_worktree(repo_root, &requested_base, &declaration_bytes) {
        println!(
            "{YELLOW}• change-scope gate not evaluated{NC}: {SCOPE_DECLARATION_PATH} is \
             byte-identical to its {requested_base} blob, so this change declares no scope. \
             A scope file that rode in on the base constrains nobody."
        );
        return Ok(0);
    }
    let declaration_text = String::from_utf8(declaration_bytes).map_err(|error| {
        eyre!("{SCOPE_DECLARATION_PATH} is not valid UTF-8 ({error}): declare scope as text")
    })?;
    let scope = parse_scope_declaration(&declaration_text)?;
    let merge_base_commit = merge_base(repo_root, &requested_base);
    let changed = read_changed_files(repo_root, &merge_base_commit)?;

    if changed.is_empty() {
        println!(
            "{GREEN}✅ no changed files{NC} (base: {requested_base}, \
             scope: {SCOPE_DECLARATION_PATH} carries {} pattern(s))",
            scope.len()
        );
        return Ok(0);
    }
    let unadmitted = find_unadmitted(&changed, &scope);
    if unadmitted.is_empty() {
        println!(
            "{GREEN}✅ all {} changed file(s) are inside the declared scope{NC} (base: {requested_base}, \
             scope: {SCOPE_DECLARATION_PATH})",
            changed.len()
        );
        return Ok(0);
    }

    println!(
        "{RED}❌ change touches files outside the declared scope{NC} (base: {requested_base}, \
         scope: {SCOPE_DECLARATION_PATH})"
    );
    for path in &unadmitted {
        println!("  {path}");
    }
    if scope.is_empty() {
        println!();
        println!("The scope declaration carries no patterns, so it admits nothing.");
    }
    println!();
    println!(
        "Declare each file in {SCOPE_DECLARATION_PATH} (one exact path or `dir/` prefix per line) \
         or revert the out-of-scope hunks."
    );
    Ok(1)
}

/// Candidate base refs tried, in order, when no explicit base is supplied.
///
/// Mirrors `must_context::base_candidates`: both diff-vs-base hygiene gates
/// must agree on what "the base" means for the same change.
fn base_candidates() -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(value) = std::env::var("CI_SCOPE_BASE") {
        candidates.push(value);
    }
    if let Ok(value) = std::env::var("GITHUB_BASE_REF") {
        candidates.push(format!("origin/{value}"));
        candidates.push(value);
    }
    candidates.extend(["origin/main".to_owned(), "main".to_owned(), "HEAD~1".to_owned()]);
    candidates
}

/// Selects the first candidate base ref that `git` can resolve.
///
/// Mirrors `must_context::resolve_base`, including its honesty contract: an
/// explicitly requested base that does not resolve is an error, while
/// auto-resolution finding nothing is "no subject to evaluate" — an absent
/// diff is not scope creep.
fn resolve_base(repo_root: &Path, requested: Option<&str>) -> Result<Option<String>> {
    if let Some(base) = requested {
        if ref_exists(repo_root, base) {
            return Ok(Some(base.to_owned()));
        }
        return Err(eyre!("base ref '{base}' does not resolve in {}", repo_root.display()));
    }

    Ok(base_candidates().into_iter().find(|candidate| ref_exists(repo_root, candidate)))
}

/// Resolves the merge base of `base` and `HEAD`, falling back to `base` itself.
///
/// Mirrors `must_context::merge_base`. The scan then diffs that commit against
/// the **working tree**, so an out-of-scope edit a contributor has staged but
/// not yet committed is in subject. In CI the tree is clean, so the range
/// agrees with `base...HEAD`.
fn merge_base(repo_root: &Path, base: &str) -> String {
    Command::new("git")
        .current_dir(repo_root)
        .args(["merge-base", base, "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_owned())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| base.to_owned())
}

/// Returns `true` when `git rev-parse --verify` resolves `reference`.
fn ref_exists(repo_root: &Path, reference: &str) -> bool {
    Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Returns `true` when `base` carries a scope-declaration blob byte-identical
/// to the worktree file. A `git show` failure means the base has no such blob
/// (or the ref is otherwise unreadable), which is a new declaration by this
/// change — honored, not stale. Git breakage surfaces fail-closed in
/// [`read_changed_files`] instead.
fn base_blob_matches_worktree(repo_root: &Path, base: &str, worktree_bytes: &[u8]) -> bool {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["show", &format!("{base}:{SCOPE_DECLARATION_PATH}")])
        .output();
    output.is_ok_and(|output| output.status.success() && output.stdout == worktree_bytes)
}

/// Reads the repo-relative paths changed between `base` and the working tree.
///
/// `base` is already the merge base, and the range is two-dot so staged and
/// unstaged edits to tracked files are included. `core.quotePath=false` keeps
/// non-ASCII paths unquoted so they compare equal to their scope lines;
/// anything still undecodable is lossy-mapped, which can only mismatch — a
/// false unadmitted, never a false admission.
fn read_changed_files(repo_root: &Path, base: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["-c", "core.quotePath=false", "diff", "--name-only", "--no-color", base])
        .output()
        .map_err(|error| eyre!("failed to run `git diff --name-only` against '{base}': {error}"))?;

    if !output.status.success() {
        return Err(eyre!(
            "`git diff --name-only {base}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        ChangeScope, SCOPE_DECLARATION_PATH, ScopePattern, find_unadmitted, is_admitted,
        parse_scope_declaration,
    };
    use color_eyre::eyre::{Result, eyre};

    fn changed(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    fn scope_of(text: &str) -> Result<ChangeScope> {
        parse_scope_declaration(text)
    }

    #[test]
    fn scope_creep_neighbor_is_named_as_unadmitted() -> Result<()> {
        // The T6 shape: the fix is declared, the "obvious cleanup" next door
        // is not — the gate must name exactly the creeping file.
        let scope = scope_of("interpolation_scan.rs\n")?;
        let unadmitted = find_unadmitted(
            &changed(&["interpolation_scan.rs", "neighbor.rs", SCOPE_DECLARATION_PATH]),
            &scope,
        );
        assert_eq!(unadmitted, vec!["neighbor.rs".to_owned()]);
        Ok(())
    }

    #[test]
    fn in_scope_change_is_fully_admitted() -> Result<()> {
        let scope = scope_of("interpolation_scan.rs\nneighbor.rs\n")?;
        let unadmitted =
            find_unadmitted(&changed(&["interpolation_scan.rs", "neighbor.rs"]), &scope);
        assert!(unadmitted.is_empty(), "unexpected unadmitted: {unadmitted:?}");
        Ok(())
    }

    #[test]
    fn directory_prefix_admits_everything_beneath_it() -> Result<()> {
        let scope = scope_of("crates/perl-ci-hygiene/\n")?;
        assert!(is_admitted("crates/perl-ci-hygiene/src/commands/change_scope.rs", &scope));
        assert!(is_admitted("crates/perl-ci-hygiene/Cargo.toml", &scope));
        Ok(())
    }

    #[test]
    fn directory_prefix_does_not_admit_siblings() -> Result<()> {
        // The trailing slash is load-bearing: without it `crates/foo` would
        // prefix-match `crates/foobar/baz.rs`.
        let scope = scope_of("crates/foo/\n")?;
        assert!(!is_admitted("crates/foobar/baz.rs", &scope));
        assert!(!is_admitted("crates/foo", &scope));
        assert!(is_admitted("crates/foo/baz.rs", &scope));
        Ok(())
    }

    #[test]
    fn scope_declaration_admits_itself() -> Result<()> {
        // Declaring a scope must never itself be the violation.
        let scope = scope_of("interpolation_scan.rs\n")?;
        assert!(is_admitted(SCOPE_DECLARATION_PATH, &scope));
        let unadmitted =
            find_unadmitted(&changed(&["interpolation_scan.rs", SCOPE_DECLARATION_PATH]), &scope);
        assert!(unadmitted.is_empty(), "unexpected unadmitted: {unadmitted:?}");
        Ok(())
    }

    #[test]
    fn comments_and_blank_lines_carry_no_patterns() -> Result<()> {
        let scope = scope_of("# fix scope for #17429\n\n   \ninterpolation_scan.rs\n# trailer\n")?;
        assert_eq!(
            scope,
            ChangeScope { patterns: vec![ScopePattern::Exact("interpolation_scan.rs".to_owned())] }
        );
        Ok(())
    }

    #[test]
    fn glob_syntax_is_rejected_with_its_line_number() -> Result<()> {
        for glob in ["crates/*", "src/**/lib.rs", "file?.rs", "src/[ab].rs"] {
            let text = format!("interpolation_scan.rs\n{glob}\n");
            let error = match scope_of(&text) {
                Ok(_) => return Err(eyre!("glob line {glob:?} must be rejected")),
                Err(error) => error,
            };
            let message = format!("{error:#}");
            assert!(
                message.contains(".agents/change-scope:2"),
                "error must name line 2, got: {message}"
            );
        }
        Ok(())
    }

    #[test]
    fn windows_and_rooted_spellings_normalize() -> Result<()> {
        let scope = scope_of("crates\\foo\\bar.rs\n/foo/baz.rs\n./qux.rs\n")?;
        assert!(is_admitted("crates/foo/bar.rs", &scope));
        assert!(is_admitted("foo/baz.rs", &scope));
        assert!(is_admitted("qux.rs", &scope));
        // And the changed side normalizes too: a diff path with a stray
        // prefix still matches its declared exact path.
        let plain = scope_of("qux.rs\n")?;
        assert!(is_admitted("./qux.rs", &plain));
        Ok(())
    }

    #[test]
    fn empty_scope_admits_nothing() -> Result<()> {
        // An honored declaration with zero patterns is "this change touches
        // nothing" — any change fails against it.
        let scope = scope_of("# nothing declared\n")?;
        assert!(scope.is_empty());
        let unadmitted = find_unadmitted(&changed(&["anything.rs"]), &scope);
        assert_eq!(unadmitted, vec!["anything.rs".to_owned()]);
        Ok(())
    }

    #[test]
    fn empty_candidate_is_never_admitted() -> Result<()> {
        let scope = scope_of("anything.rs\n")?;
        assert!(!is_admitted("", &scope));
        assert!(!is_admitted("   ", &scope));
        Ok(())
    }

    #[test]
    fn unadmitted_keep_diff_order() -> Result<()> {
        let scope = scope_of("scope.rs\n")?;
        let unadmitted = find_unadmitted(&changed(&["zeta.rs", "scope.rs", "alpha.rs"]), &scope);
        assert_eq!(unadmitted, vec!["zeta.rs".to_owned(), "alpha.rs".to_owned()]);
        Ok(())
    }
}
