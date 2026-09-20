//! Validate known stale publication claims inside docs/articles, plus the
//! coro/thread claim-drift guards (#8355/#9076) over current generated and
//! reference surfaces.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::{fs, path::PathBuf};

const ARTICLES_DIR: &str = "docs/articles";

const STALE_PATTERNS: &[(&str, &str, &str)] = &[
    ("563,228 lines", "591,034 lines", "LOC claim (563K is stale; ledger: 591,034)"),
    ("563K lines", "591K lines", "LOC claim (563K is stale; ledger: 591K)"),
    ("546,000", "591,034", "LOC claim (546K is stale; ledger: 591,034)"),
    ("546K lines", "591K lines", "LOC claim (546K is stale; ledger: 591K)"),
    ("131 crates", "133 crates", "Crate count (131 is stale; ledger: 133)"),
    ("131 workspace crates", "133 workspace crates", "Crate count (131 is stale; ledger: 133)"),
    ("132 workspace crates", "133 workspace crates", "Crate count (132 is stale; ledger: 133)"),
    ("132 crates", "133 crates", "Crate count (132 is stale; ledger: 133)"),
    (
        "97 LSP and DAP features",
        "98 LSP and DAP features",
        "Feature count (97 is stale; ledger: 98)",
    ),
    ("97 LSP/DAP features", "98 LSP/DAP features", "Feature count (97 is stale; ledger: 98)"),
    ("97 features defined", "98 features defined", "Feature count (97 is stale; ledger: 98)"),
    ("97 features governed", "98 features governed", "Feature count (97 is stale; ledger: 98)"),
    ("97 features:", "98 features:", "Feature count (97 is stale; ledger: 98)"),
    ("2,700+ commits", "3,200+ commits", "Commit count (2,700+ is stale; ledger: 3,210)"),
    ("2,200+ pull requests", "2,646+ pull requests", "PR count (2,200+ is stale; ledger: 2,646+)"),
    ("2,200+ PRs", "2,646+ PRs", "PR count (2,200+ is stale; ledger: 2,646+)"),
];

type ClaimHit = (PathBuf, usize, &'static str, &'static str, &'static str);
const FORBIDDEN_CRATE_NAME: &str = "`perl-workspace-index`";
const CRATE_NAME_GUARD_FILES: &[&str] = &[
    "README.md",
    "crates/perl-workspace/README.md",
    "crates/perl-workspace/src/api.rs",
    "docs/project/status/workspace.md",
];
const CRATE_NAME_EXCEPTIONS: &[&str] = &["docs/MIGRATION_v0.13.md"];

/// The release runbook and its published mirror. Every step in them is a
/// template an operator copies verbatim into a tag, a Homebrew formula, release
/// notes, or social copy, so a literal in either file becomes a literal in a
/// shipped artifact.
const RELEASE_RUNBOOK_FILES: &[&str] =
    &["docs/project/GA_RUNBOOK.md", "book/src/resources/ga-runbook.md"];

/// Literals the runbook must not contain, and why.
///
/// Three different hardcoded versions and a push to the wrong default branch
/// coexisted here, so following the runbook end to end tagged one release,
/// published a formula for a second, and bumped the extension to a third
/// (#5464). The coverage headlines were templated unconditionally, which
/// republished unverified numbers every release.
const RELEASE_RUNBOOK_FORBIDDEN: &[(&str, &str)] = &[
    ("git push origin master", "the default branch is `main`; use `git push origin main`"),
    ("100% Edge Case Coverage", "quote a verified figure from docs/project/CURRENT_STATUS.md"),
    ("141 edge cases", "quote a verified figure from docs/project/CURRENT_STATUS.md"),
];

pub fn run() -> Result<()> {
    let root = project_root()?;
    let articles_dir = root.join(ARTICLES_DIR);
    let mut files = Vec::new();

    if !articles_dir.is_dir() {
        bail!("expected articles directory not found at {}", articles_dir.display());
    }

    for entry in fs::read_dir(&articles_dir).context("failed to read docs/articles directory")? {
        let entry = entry.context("failed to read directory entry")?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();

    let mut hits: Vec<ClaimHit> = Vec::new();
    for md_file in &files {
        let text = fs::read_to_string(md_file)
            .with_context(|| format!("failed to read article file {}", md_file.display()))?;
        for (line_no, line) in text.lines().enumerate() {
            for &(stale, replacement, description) in STALE_PATTERNS {
                if line.contains(stale) {
                    hits.push((md_file.clone(), line_no + 1, stale, replacement, description));
                }
            }
        }
    }

    if hits.is_empty() {
        check_forbidden_workspace_crate_name(&root)?;
        check_release_runbook_is_parameterised(&root)?;
        check_coro_thread_claim_drift(&root)?;
        // #4649: this validator only checks a fixed list of hardcoded stale
        // literals. It cannot detect new staleness patterns (e.g. a crate count
        // drifting past the last hand-edited value); it only catches
        // regressions of the literals listed below. State that scope explicitly
        // so "0 violations" is not mistaken for a clean bill of health.
        println!("{}", success_message(files.len()));
        eprintln!(
            "doc-claims scope (#4649): checked {} hardcoded stale literals: {}",
            STALE_PATTERNS.len(),
            STALE_PATTERNS.iter().map(|(stale, _, _)| *stale).collect::<Vec<_>>().join(", ")
        );
        return Ok(());
    }

    eprintln!("DOC CLAIM VIOLATIONS:");
    eprintln!("{}", "=".repeat(60));
    for (file, line_no, stale, replacement, description) in &hits {
        let rel = file.strip_prefix(&root).unwrap_or(file.as_path());
        eprintln!("  {}:{}: {}", rel.display(), line_no, description);
        eprintln!("    found:    {:?}", stale);
        eprintln!("    expected: {:?}", replacement);
    }
    eprintln!("{}", "=".repeat(60));
    eprintln!("{} stale claim(s) found in docs/articles.", hits.len());
    eprintln!("\nTo fix: update the article to match docs/project/PUBLICATION_FACTS_LEDGER.md");
    bail!("doc claim check failed");
}

/// Versions the runbook used to hardcode. Scope is deliberately the same as
/// `STALE_PATTERNS`: named literals, not a general version regex. A regex would
/// have to accept the factual `v0.17.0` in the header note and the asset names
/// in the step-5 evidence block, which are statements about the tree rather
/// than templates an operator copies.
const RELEASE_RUNBOOK_FORBIDDEN_VERSIONS: &[&str] = &["0.8.3", "0.13.1", "0.6.0"];

/// The runbook must keep defining its version once and deriving the tag from
/// it. Without this, the forbidden-literal list above could be satisfied by
/// deleting the parameterization rather than keeping it.
const RELEASE_RUNBOOK_REQUIRED: &[(&str, &str)] = &[
    ("VERSION=", "the runbook must set `VERSION` once in step 0"),
    ("TAG=\"v$VERSION\"", "the tag must derive from `$VERSION`, not be typed again"),
];

fn check_release_runbook_is_parameterised(root: &std::path::Path) -> Result<()> {
    for rel in RELEASE_RUNBOOK_FILES {
        let path = root.join(rel);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read release runbook {}", path.display()))?;

        for &(forbidden, why) in RELEASE_RUNBOOK_FORBIDDEN {
            if text.contains(forbidden) {
                bail!("{rel}: contains {forbidden:?} — {why} (#5464)");
            }
        }
        for &version in RELEASE_RUNBOOK_FORBIDDEN_VERSIONS {
            if text.contains(version) {
                bail!(
                    "{rel}: contains the hardcoded version {version:?}. Every step reads \
                     `$VERSION`, set once in step 0 — a literal here ships in a tag, a formula, \
                     or release notes for a different release than the one being cut (#5464)"
                );
            }
        }
        for &(required, why) in RELEASE_RUNBOOK_REQUIRED {
            if !text.contains(required) {
                bail!("{rel}: missing {required:?} — {why} (#5464)");
            }
        }
    }
    Ok(())
}

/// Coro/thread claim-drift guards (#8355 controller, #9076 truth repair).
///
/// The catalog and docs used to collapse "one synthetic DAP main context" into
/// "Perl is single-threaded", presented Coro-shaped future work as current
/// thread support, and pointed coroutine ownership at `#3539` (now an
/// unrelated PR). These guards keep the repaired surfaces at their exact
/// strength: a forbidden literal is the undifferentiated claim, a required
/// literal is the live owner or exact-strength marker that replaced it.
struct CoroClaimGuard {
    file: &'static str,
    forbidden: &'static [&'static str],
    required: &'static [&'static str],
}

/// The exact pre-repair literals. `features.toml` is the root catalog
/// authority; the four crate-local `features_sot.toml` files are byte
/// projections of it (#7029), so they carry the same guard.
const CORO_CLAIM_GUARDS: &[CoroClaimGuard] = &[
    CoroClaimGuard {
        file: "features.toml",
        forbidden: &[
            // #9076 negative control: the exact stale description.
            "Perl is single-threaded so returns one synthetic thread",
            // #9076 drift check: "Perl is single-threaded" as a universal
            // product fact must not return anywhere in the catalog.
            "Perl is single-threaded",
        ],
        required: &["at most one synthetic execution context for the active session"],
    },
    CoroClaimGuard {
        file: "crates/perl-dap/features_sot.toml",
        forbidden: CORO_CLAIM_GUARDS_STALE_CATALOG_CLAIMS,
        required: &["at most one synthetic execution context for the active session"],
    },
    CoroClaimGuard {
        file: "crates/perl-lsp-rs/features_sot.toml",
        forbidden: CORO_CLAIM_GUARDS_STALE_CATALOG_CLAIMS,
        required: &["at most one synthetic execution context for the active session"],
    },
    CoroClaimGuard {
        file: "crates/perl-lsp-rs-core/features_sot.toml",
        forbidden: CORO_CLAIM_GUARDS_STALE_CATALOG_CLAIMS,
        required: &["at most one synthetic execution context for the active session"],
    },
    CoroClaimGuard {
        file: "crates/perl-parser/features_sot.toml",
        forbidden: CORO_CLAIM_GUARDS_STALE_CATALOG_CLAIMS,
        required: &["at most one synthetic execution context for the active session"],
    },
    CoroClaimGuard {
        file: "docs/project/ISSUE_3539_COROUTINES_SCOPE.md",
        forbidden: &[
            // #9076 negative control: the title that presented #3539 as the
            // live coroutine owner.
            "# Issue #3539 Coroutine Scope Decision",
        ],
        required: &[
            // The live ownership graph must stay named (#8290 programme).
            "#8290",
        ],
    },
    CoroClaimGuard {
        file: "docs/reference/DAP_PHASE5_NATIVE.md",
        forbidden: &[
            // "Multi-threading: Single-threaded execution model (Perl
            // limitation)" erased Coro and interpreter threads.
            "Single-threaded execution model (Perl limitation)",
        ],
        required: &["at most one synthetic execution context for the active session"],
    },
    CoroClaimGuard {
        file: "docs/how-to/DEBUGGING.md",
        forbidden: &["- [ ] Multi-threaded debugging support"],
        required: &["synthetic per-session execution context"],
    },
    CoroClaimGuard {
        file: "book/src/user-guides/debugging.md",
        forbidden: &["- [ ] Multi-threaded debugging support"],
        required: &["synthetic per-session execution context"],
    },
];

const CORO_CLAIM_GUARDS_STALE_CATALOG_CLAIMS: &[&str] =
    &["Perl is single-threaded so returns one synthetic thread", "Perl is single-threaded"];

fn check_forbidden_workspace_crate_name(root: &std::path::Path) -> Result<()> {
    for rel in CRATE_NAME_GUARD_FILES {
        if CRATE_NAME_EXCEPTIONS.contains(rel) {
            continue;
        }
        let path = root.join(rel);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read guard file {}", path.display()))?;
        if text.contains(FORBIDDEN_CRATE_NAME) || text.contains(" -p perl-workspace-index ") {
            bail!("forbidden stale crate name '{}' found in {}", FORBIDDEN_CRATE_NAME, rel);
        }
    }
    Ok(())
}

/// Pure form of the coro/thread claim-drift guard so the negative control can
/// run against synthetic mutated text without touching the tree.
fn coro_claim_guard_violations(file: &str, text: &str) -> Vec<String> {
    let Some(guard) = CORO_CLAIM_GUARDS.iter().find(|g| g.file == file) else {
        return vec![format!(
            "CORO_GUARD_TABLE: {file:?} is not covered by any coro/thread claim guard (#8355/#9076)"
        )];
    };
    let mut violations = Vec::new();
    for &stale in guard.forbidden {
        if text.contains(stale) {
            violations.push(format!(
                "CORO_CLAIM: {} contains {:?} — the undifferentiated coro/thread claim \
                 repaired by #9076 must not return (#8355)",
                guard.file, stale
            ));
        }
    }
    for &marker in guard.required {
        if !text.contains(marker) {
            violations.push(format!(
                "CORO_MARKER: {} no longer contains {:?} — the exact-strength/live-owner \
                 wording it must keep (#8355/#9076)",
                guard.file, marker
            ));
        }
    }
    violations
}

fn check_coro_thread_claim_drift(root: &std::path::Path) -> Result<()> {
    let mut violations = Vec::new();
    for guard in CORO_CLAIM_GUARDS {
        let path = root.join(guard.file);
        let text = fs::read_to_string(&path).with_context(|| {
            format!("failed to read guarded coro/thread surface {}", path.display())
        })?;
        violations.extend(coro_claim_guard_violations(guard.file, &text));
    }
    if violations.is_empty() {
        return Ok(());
    }
    for violation in &violations {
        eprintln!("{violation}");
    }
    bail!("coro/thread claim drift check failed ({} violation(s))", violations.len());
}

/// Build the success message reported when no stale-literal regressions are
/// found. Extracted so the #4649 scope caveat ("only N hardcoded literals are
/// checked; new staleness patterns are NOT caught") can be unit-tested.
fn success_message(files_count: usize) -> String {
    format!(
        "Doc claims OK: {files_count} articles scanned, {n} hardcoded stale literals checked, \
         0 regressions found. Scope: only the {n} hardcoded literals below are checked; \
         new staleness patterns are NOT caught. Coro/thread claim guards additionally cover \
         {m} generated/reference surfaces (#8355/#9076).",
        n = STALE_PATTERNS.len(),
        m = CORO_CLAIM_GUARDS.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_message_states_hardcoded_literal_scope() {
        let msg = success_message(7);
        // #4649 acceptance: the OK message must explicitly state that only
        // hardcoded literals are checked and new staleness is NOT caught.
        assert!(msg.contains("hardcoded stale literals checked"), "msg: {msg}");
        assert!(msg.contains("new staleness patterns are NOT caught"), "msg: {msg}");
        assert!(msg.contains("0 regressions found"), "msg: {msg}");
        assert!(msg.contains("7 articles scanned"), "msg: {msg}");
        assert!(
            msg.contains("generated/reference surfaces"),
            "the coro guard coverage must stay part of the honest scope statement: {msg}"
        );
    }

    #[test]
    fn release_runbook_guard_passes_on_the_current_tree() -> Result<()> {
        // The guard is only worth having if it is live against the real files;
        // a table checked against nothing would pass forever.
        check_release_runbook_is_parameterised(&project_root()?)
    }

    #[test]
    fn release_runbook_guard_covers_both_the_source_and_its_published_mirror() {
        // `scripts/populate-book.sh` copies the runbook into the book. Guarding
        // only the source would let the copy users actually read go stale —
        // which is exactly how the broken Windows command reached the published
        // book (#5461).
        assert!(RELEASE_RUNBOOK_FILES.contains(&"docs/project/GA_RUNBOOK.md"));
        assert!(RELEASE_RUNBOOK_FILES.contains(&"book/src/resources/ga-runbook.md"));
    }

    #[test]
    fn release_runbook_guard_names_every_defect_the_issue_found() {
        // #5464 found four: three hardcoded versions, a push to `master`, and
        // unconditional coverage headlines. Dropping any entry silently narrows
        // the guard to less than the issue it closes.
        assert!(
            RELEASE_RUNBOOK_FORBIDDEN
                .iter()
                .any(|(literal, _)| *literal == "git push origin master")
        );
        assert!(
            RELEASE_RUNBOOK_FORBIDDEN.iter().any(|(literal, _)| literal.contains("Edge Case")),
            "the templated coverage headline must stay forbidden"
        );
        assert!(
            RELEASE_RUNBOOK_FORBIDDEN.iter().any(|(literal, _)| *literal == "141 edge cases"),
            "the templated coverage figure must stay forbidden"
        );
        assert_eq!(
            RELEASE_RUNBOOK_FORBIDDEN_VERSIONS,
            ["0.8.3", "0.13.1", "0.6.0"],
            "the tag, formula, and extension versions that disagreed"
        );
        assert!(!RELEASE_RUNBOOK_REQUIRED.is_empty(), "the parameterization must stay asserted");
    }

    #[test]
    fn coro_drift_guard_catches_every_negative_control_from_the_issue() {
        // #9076 negative control: mutate a repaired surface back to its stale
        // claim and the focused check must fail. Each mutation is the exact
        // pre-repair text.
        let mutations: &[(&str, &str)] = &[
            (
                "features.toml",
                "description = \"Thread listing (threads request); Perl is single-threaded so returns one synthetic thread\"",
            ),
            (
                "docs/project/ISSUE_3539_COROUTINES_SCOPE.md",
                "# Issue #3539 Coroutine Scope Decision",
            ),
            (
                "docs/reference/DAP_PHASE5_NATIVE.md",
                "3. **Multi-threading**: Single-threaded execution model (Perl limitation)",
            ),
            ("docs/how-to/DEBUGGING.md", "- [ ] Multi-threaded debugging support"),
            ("book/src/user-guides/debugging.md", "- [ ] Multi-threaded debugging support"),
        ];
        for (file, mutated) in mutations {
            let violations = coro_claim_guard_violations(file, mutated);
            // Assert the forbidden-literal match itself fired (CORO_CLAIM), not
            // just a non-empty result: a standalone mutated snippet also omits
            // the required marker, so a bare non-empty assertion would pass
            // even if the forbidden table lost its entry (#12273 review).
            assert!(
                violations.iter().any(|v| v.starts_with("CORO_CLAIM:")),
                "mutating {file} back to its stale claim must trip the forbidden-literal \
                 guard, got only: {violations:?} (mutation: {mutated})"
            );
        }
    }

    #[test]
    fn coro_drift_guard_passes_on_the_current_tree() -> Result<()> {
        // Like the runbook guard, the coro guard is only worth having if it is
        // live against the real repaired files.
        check_coro_thread_claim_drift(&project_root()?)
    }

    #[test]
    fn coro_drift_guard_covers_catalog_authority_and_every_vendored_projection() {
        // The stale description lived in the root authority plus four byte
        // projections (#7029); guarding only one would let the others
        // reintroduce it.
        let guarded: Vec<&str> = CORO_CLAIM_GUARDS.iter().map(|g| g.file).collect();
        for file in [
            "features.toml",
            "crates/perl-dap/features_sot.toml",
            "crates/perl-lsp-rs/features_sot.toml",
            "crates/perl-lsp-rs-core/features_sot.toml",
            "crates/perl-parser/features_sot.toml",
            "docs/project/ISSUE_3539_COROUTINES_SCOPE.md",
            "docs/reference/DAP_PHASE5_NATIVE.md",
            "docs/how-to/DEBUGGING.md",
            "book/src/user-guides/debugging.md",
        ] {
            assert!(guarded.contains(&file), "coro guard must cover {file}");
        }
    }

    #[test]
    fn coro_drift_guard_rejects_unguarded_surfaces_loudly() {
        // A file with no guard row must not silently pass.
        let violations = coro_claim_guard_violations("docs/not-a-guarded-surface.md", "anything");
        assert!(!violations.is_empty());
        assert!(violations[0].contains("not covered"), "violation: {}", violations[0]);
    }

    #[test]
    fn stale_patterns_table_is_non_empty() {
        // A non-empty table is what makes the scope count meaningful; if it
        // ever empties the message would be misleading.
        assert!(!STALE_PATTERNS.is_empty());
        for (stale, replacement, _desc) in STALE_PATTERNS {
            assert!(!stale.is_empty(), "stale literal must not be empty");
            assert!(!replacement.is_empty(), "replacement literal must not be empty");
            assert_ne!(stale, replacement, "stale and replacement must differ");
        }
    }
}
