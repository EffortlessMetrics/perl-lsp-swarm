//! `cargo xtask kwalitee-inventory` — machine-readable zero-caller authority for
//! the legacy `perl-kwalitee` / `perl_kwalitee` namespace (#8752).
//!
//! The final Kwalitee/readiness namespace cutover (#7166, #7185, #7192) needs to
//! know, mechanically, which references to the namespace still exist, which of
//! them are active callers, and which are only historical receipt readability.
//! This task is that authority:
//!
//! - it scans the working tree exhaustively for the namespace spellings
//!   (`perl-kwalitee`, `perl_kwalitee`, `PerlKwalitee`, `PERL_KWALITEE`, ...),
//!   including generated files and files that ordinary search tooling skips;
//! - it reconciles every occurrence against the hand-authored ledger in
//!   [`LEDGER_REL`], where each exact occurrence group carries exactly one
//!   classification, an owner issue, a migration target, a removal condition,
//!   and an allowed-to-remain flag;
//! - it fails closed on an unclassified (new or ambiguous) reference, on a
//!   stale classification whose source line moved or vanished, on a duplicate
//!   classification, and on any closed-vocabulary violation;
//! - it prints a deterministic report of unresolved active occurrence counts by
//!   migration target, which is the denominator #7185/#7192 consume.
//!
//! This changes no caller, no doc, and no alias. It only observes and gates.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::utils::project_root;

/// Hand-authored classification ledger. Humans own this file.
pub const LEDGER_REL: &str = "policy/kwalitee-namespace-inventory.toml";
/// Schema identity enforced at parse time.
pub const SCHEMA: &str = "kwalitee_namespace_inventory.v1";
/// Controlling issue for the inventory itself.
pub const CONTROLLER_ISSUE: u64 = 8752;

/// Lowercased spellings of the namespace that count as references. Every
/// current occurrence is one of these three shapes modulo ASCII case
/// (`perl-kwalitee`, `perl_kwalitee`, `PerlKwalitee`, `PERL_KWALITEE`).
const TOKENS_LOWER: [&str; 3] = ["perl-kwalitee", "perl_kwalitee", "perlkwalitee"];

/// Closed classification vocabulary. A new class is a governance decision and
/// a code change here, not a string someone coins in passing.
const CLASSIFICATIONS: [&str; 5] = [
    "real_kwalitee",
    "release_readiness",
    "legacy_compatibility",
    "historical_prose",
    "invalid_or_stale",
];

/// Closed migration-target vocabulary.
const TARGETS: [&str; 5] = [
    "native_distribution_analyser",
    "perl_release_readiness",
    "independent_readiness_rails",
    "legacy_receipt_readability",
    "none",
];

/// Classes that still owe migration or removal work. Their occurrences are the
/// "unresolved active" population the report exposes by migration target.
const UNRESOLVED_CLASSES: [&str; 3] =
    ["release_readiness", "legacy_compatibility", "invalid_or_stale"];

/// Files whose only reason to mention the namespace is that they *are* this
/// inventory. Counting the ledger as a caller would make it self-feeding.
const SELF_ARTIFACTS: &[&str] = &[LEDGER_REL];

/// One classification row: every occurrence of the namespace in `path` whose
/// source occurrences hash into `line_hashes` carries exactly this
/// classification. `line_hashes` is a multiset (one entry per occurrence) of
/// full SHA-256 digests over the one-based line number and trimmed line bytes,
/// so moving or editing a classified line invalidates the row instead of
/// silently passing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    classification: String,
    migration_target: String,
    owner_issue: u64,
    removal_condition: String,
    allowed_to_remain: bool,
    #[serde(default)]
    note: String,
    line_hashes: Vec<String>,
}

/// A declared non-content search surface. Ignored and generated trees are only
/// excluded when a reviewed row says why; nothing else can hide a caller.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExcludedSurface {
    pattern: String,
    reason: String,
    owner_issue: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: String,
    controller_issue: u64,
    #[serde(default, rename = "excluded_surface")]
    excluded_surfaces: Vec<ExcludedSurface>,
    #[serde(default)]
    entry: Vec<Entry>,
}

/// One observed occurrence, with enough context to make errors actionable.
struct Occurrence {
    line_no: usize,
    text: String,
    hash: String,
}

/// Hash a source occurrence the way [`Entry::line_hashes`] stores it: full
/// SHA-256 over the one-based line number, a separator, and the trimmed line.
fn line_hash(line_no: usize, trimmed: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(line_no.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(trimmed.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn is_reference(line_lower: &str) -> bool {
    TOKENS_LOWER.iter().any(|token| line_lower.contains(token))
}

/// Walk `root` and collect every reference occurrence per file, relative-path
/// keyed, in deterministic order. The walk is exhaustive over content: only
/// `.git` and the ledger's declared non-content surfaces are skipped, so
/// generated files and ignored-but-present files cannot hide callers.
fn scan(root: &Path, ledger: &Ledger) -> Result<BTreeMap<String, Vec<Occurrence>>> {
    let root = fs::canonicalize(root)
        .with_context(|| format!("canonicalizing inventory root {}", root.display()))?;
    let excluded: Vec<&str> =
        ledger.excluded_surfaces.iter().map(|surface| surface.pattern.as_str()).collect();
    let mut found = BTreeMap::new();
    let mut visited_dirs = BTreeSet::new();
    let mut visited_files = BTreeSet::new();
    walk(&root, &root, &excluded, &mut found, &mut visited_dirs, &mut visited_files)?;
    Ok(found)
}

fn walk(
    root: &Path,
    dir: &Path,
    excluded: &[&str],
    found: &mut BTreeMap<String, Vec<Occurrence>>,
    visited_dirs: &mut BTreeSet<PathBuf>,
    visited_files: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let dir = fs::canonicalize(dir)
        .with_context(|| format!("canonicalizing inventory directory {}", dir.display()))?;
    if !visited_dirs.insert(dir.clone()) {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("reading directory {}", dir.display()))?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name == ".git" {
            continue;
        }
        let meta =
            fs::symlink_metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        let resolved = fs::canonicalize(&path)
            .with_context(|| format!("resolving inventory path {}", path.display()))?;
        let rel = match resolved.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => bail!("symlinked inventory path escapes root: {}", path.display()),
        };
        // Bare component exclusions identify root-level cache/worktree
        // directories, not files.  Inspect metadata before applying them so
        // a file with a cache-like name cannot hide a reference, and so links
        // are rejected even when their name matches an exclusion.
        if surface_excluded(&name, &rel, excluded, meta.is_dir()) {
            continue;
        }
        let resolved_meta = fs::metadata(&resolved)
            .with_context(|| format!("stat resolved inventory path {}", resolved.display()))?;
        if resolved_meta.is_dir() {
            walk(root, &resolved, excluded, found, visited_dirs, visited_files)?;
        } else if resolved_meta.is_file()
            && visited_files.insert(resolved.clone())
            && !SELF_ARTIFACTS.contains(&rel.as_str())
        {
            let occurrences = scan_file(&resolved)?;
            if !occurrences.is_empty() {
                found.insert(rel, occurrences);
            }
        }
    }
    Ok(())
}

/// A surface is excluded when the declared pattern matches a repository-root
/// subtree (`/**`) or a direct child of the repository root (component
/// patterns such as `.wt-*`).  Component patterns are deliberately not
/// applied at arbitrary depth: a tracked directory named `node_modules` or
/// `.wt-*` below `crates/` is repository content, not a claim worktree/cache.
fn surface_excluded(_component: &str, rel: &str, excluded: &[&str], is_dir: bool) -> bool {
    excluded.iter().any(|pattern| {
        if pattern.contains('/') {
            let prefix = pattern.trim_end_matches('*').trim_end_matches('/');
            rel == prefix || rel.starts_with(&format!("{prefix}/"))
        } else {
            if !is_dir {
                return false;
            }
            // Bare component exclusions are root-relative. Match the first
            // relative component so descendants of an excluded root are
            // skipped, while a same-named directory below `crates/` remains
            // visible repository content.
            let root_component = match rel.split('/').next() {
                Some(component) => component,
                None => rel,
            };
            glob_component(pattern, root_component)
        }
    })
}

fn validate_exclusion_pattern(pattern: &str) -> Result<()> {
    if pattern.is_empty()
        || pattern == "*"
        || pattern.contains('\\')
        || pattern.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("excluded surface pattern {pattern:?} is too broad or not normalized");
    }
    if pattern.contains('/') {
        let prefix = pattern.strip_suffix("/**").ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "excluded surface pattern {pattern:?} must name a repository subtree with /**"
            )
        })?;
        if prefix.is_empty() || prefix.contains('*') || prefix.contains('?') || prefix.contains('[')
        {
            bail!("excluded surface pattern {pattern:?} is too broad");
        }
    } else if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        // Component globs are intentionally limited to the reviewed
        // worktree prefix convention; arbitrary globs can hide the root.
        if !pattern.starts_with('.') || !pattern.ends_with("-*") || pattern.len() <= 2 {
            bail!("excluded surface pattern {pattern:?} is too broad");
        }
    }
    Ok(())
}

fn glob_component(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((prefix, suffix)) => {
            name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
    }
}

fn scan_file(path: &Path) -> Result<Vec<Occurrence>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let count =
            TOKENS_LOWER.iter().map(|token| lower.match_indices(token).count()).sum::<usize>();
        for _ in 0..count {
            out.push(Occurrence {
                line_no: idx + 1,
                text: trimmed.to_string(),
                hash: line_hash(idx + 1, trimmed),
            });
        }
    }
    Ok(out)
}

impl Ledger {
    fn validate_static(&self) -> Result<()> {
        if self.schema_version != SCHEMA {
            bail!("ledger schema_version {:?} is not {SCHEMA:?}", self.schema_version);
        }
        if self.controller_issue != CONTROLLER_ISSUE {
            bail!("ledger controller_issue {} is not {CONTROLLER_ISSUE}", self.controller_issue);
        }
        for surface in &self.excluded_surfaces {
            if surface.reason.trim().is_empty() {
                bail!("excluded surface {:?} needs a pattern and a reason", surface.pattern);
            }
            validate_exclusion_pattern(&surface.pattern)?;
            if surface.owner_issue == 0 {
                bail!("excluded surface {:?} needs a nonzero owner_issue", surface.pattern);
            }
        }
        for entry in &self.entry {
            validate_path(&entry.path)?;
            if !CLASSIFICATIONS.contains(&entry.classification.as_str()) {
                bail!(
                    "entry {}: unknown classification {:?} (closed vocabulary: {})",
                    entry.path,
                    entry.classification,
                    CLASSIFICATIONS.join(", ")
                );
            }
            if !TARGETS.contains(&entry.migration_target.as_str()) {
                bail!(
                    "entry {}: unknown migration_target {:?} (closed vocabulary: {})",
                    entry.path,
                    entry.migration_target,
                    TARGETS.join(", ")
                );
            }
            validate_pairing(entry)?;
            if entry.classification == "invalid_or_stale" && entry.allowed_to_remain {
                bail!(
                    "entry {}: a reference classified invalid_or_stale is never allowed to remain",
                    entry.path
                );
            }
            if entry.owner_issue == 0 {
                bail!("entry {}: owner_issue is required", entry.path);
            }
            if entry.removal_condition.trim().is_empty() {
                bail!("entry {}: removal_condition is required", entry.path);
            }
            if entry.line_hashes.is_empty() {
                bail!("entry {}: line_hashes is empty; delete the stale row instead", entry.path);
            }
            for hash in &entry.line_hashes {
                if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                    bail!("entry {}: line hash {hash:?} is not 64 hex digits", entry.path);
                }
            }
        }
        Ok(())
    }
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path.split('/').any(|part| part == ".." || part.is_empty())
    {
        bail!("entry path {path:?} is not a normalized repo-relative path");
    }
    Ok(())
}

/// Which migration targets each classification may name. Keeping the pairing
/// closed prevents a row from claiming `historical_prose` while pointing at a
/// live migration surface.
fn validate_pairing(entry: &Entry) -> Result<()> {
    let allowed: &[&str] = match entry.classification.as_str() {
        "real_kwalitee" => &["native_distribution_analyser", "none"],
        "release_readiness" => &["independent_readiness_rails", "perl_release_readiness"],
        "legacy_compatibility" => &["legacy_receipt_readability", "perl_release_readiness"],
        "historical_prose" => &["none"],
        "invalid_or_stale" => &["none"],
        other => bail!("unhandled classification {other:?}"),
    };
    if !allowed.contains(&entry.migration_target.as_str()) {
        bail!(
            "entry {}: classification {:?} cannot target {:?} (allowed: {})",
            entry.path,
            entry.classification,
            entry.migration_target,
            allowed.join(", ")
        );
    }
    Ok(())
}

fn counts(hashes: &[String]) -> BTreeMap<&str, usize> {
    let mut map: BTreeMap<&str, usize> = BTreeMap::new();
    for hash in hashes {
        *map.entry(hash.as_str()).or_insert(0) += 1;
    }
    map
}

/// Reconcile observed occurrences against ledger rows. Returns the human
/// report on success; every mismatch is a hard error listing the offending
/// path, line, and current source text.
fn reconcile(ledger: &Ledger, observed: &BTreeMap<String, Vec<Occurrence>>) -> Result<String> {
    let mut errors: Vec<String> = Vec::new();

    let mut rows_by_path: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for entry in &ledger.entry {
        rows_by_path.entry(entry.path.as_str()).or_default().push(entry);
    }

    // Observed files with no ledger coverage at all.
    for path in observed.keys() {
        if !rows_by_path.contains_key(path.as_str()) {
            let lines: Vec<String> =
                observed[path].iter().map(|o| format!("line {}: {}", o.line_no, o.text)).collect();
            errors.push(format!(
                "unclassified reference(s) in {path} — no ledger row covers this file: {}",
                lines.join(" | ")
            ));
        }
    }

    // Ledger rows whose path vanished.
    for entry in &ledger.entry {
        if !observed.contains_key(entry.path.as_str()) {
            errors.push(format!(
                "stale classification: {} is classified {:?} but the file no longer exists",
                entry.path, entry.classification
            ));
        }
    }

    for (path, entries) in &rows_by_path {
        let Some(occs) = observed.get(*path) else {
            continue;
        };

        // Claims per hash across all rows for this path.
        let mut claims: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for entry in entries {
            for (hash, count) in counts(&entry.line_hashes) {
                for _ in 0..count {
                    claims.entry(hash).or_default().push(entry.classification.as_str());
                }
            }
        }
        let mut observed_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for occ in occs {
            *observed_counts.entry(occ.hash.as_str()).or_insert(0) += 1;
        }

        for (hash, claimants) in &claims {
            let observed_count = observed_counts.get(hash).copied().unwrap_or(0);
            let distinct: BTreeSet<_> = claimants.iter().copied().collect();
            if claimants.len() == observed_count && (observed_count < 2 || distinct.len() == 1) {
                continue;
            }
            if claimants.len() < observed_count {
                errors.push(format!(
                    "unclassified duplicate occurrence(s) in {path}: {} row claim(s) for line \
                     hash {hash} but {observed_count} occurrence(s) on disk",
                    claimants.len()
                ));
                continue;
            }
            let kind = if distinct.len() > 1 {
                "duplicate classification"
            } else {
                "stale classification"
            };
            let context = entries
                .iter()
                .find(|entry| entry.line_hashes.iter().any(|h| h == hash))
                .map(|entry| {
                    format!("; row intent: {} (removal: {})", entry.note, entry.removal_condition)
                })
                .unwrap_or_default();
            errors.push(format!(
                "{kind} in {path}: {} row claim(s) for line hash {hash} but \
                 {observed_count} occurrence(s) on disk (claimants: {}){context}",
                claimants.len(),
                distinct.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }

        // Unclassified occurrences inside a covered file.
        for occ in occs {
            if !claims.contains_key(occ.hash.as_str()) {
                errors.push(format!(
                    "unclassified reference in {} line {}: {} (hash {})",
                    path, occ.line_no, occ.text, occ.hash
                ));
            }
        }
    }

    if !errors.is_empty() {
        let mut msg = String::new();
        let _ = writeln!(msg, "kwalitee namespace inventory rejected {} problem(s):", errors.len());
        for error in &errors {
            let _ = write!(msg, "\n  - {error}");
        }
        bail!("{msg}");
    }

    Ok(render_report(ledger))
}

/// Deterministic report: totals per classification plus the unresolved active
/// occurrence counts by migration target that #7185/#7192 consume.
fn render_report(ledger: &Ledger) -> String {
    let mut by_class: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut unresolved: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for entry in &ledger.entry {
        let slot = by_class.entry(entry.classification.as_str()).or_insert((0, 0));
        slot.0 += entry.line_hashes.len();
        slot.1 += 1;
        if UNRESOLVED_CLASSES.contains(&entry.classification.as_str()) {
            let slot = unresolved.entry(entry.migration_target.as_str()).or_insert((0, 0));
            slot.0 += entry.line_hashes.len();
            slot.1 += 1;
        }
    }

    let total: usize = by_class.values().map(|(occ, _)| occ).sum();
    let files = ledger.entry.iter().map(|entry| entry.path.as_str()).collect::<BTreeSet<_>>().len();
    let pending_removal = ledger
        .entry
        .iter()
        .filter(|entry| !entry.allowed_to_remain)
        .fold((0, 0), |(occ, rows), entry| (occ + entry.line_hashes.len(), rows + 1));

    let mut report = String::new();
    let _ = writeln!(
        report,
        "kwalitee namespace inventory ({SCHEMA}, #{CONTROLLER_ISSUE}): \
         {total} classified occurrence(s) across {files} file(s)"
    );
    for (class, (occ, rows)) in &by_class {
        let _ = writeln!(report, "  {class}: {occ} occurrence(s) in {rows} row(s)");
    }
    let _ = writeln!(
        report,
        "occurrences not allowed to remain: {} in {} row(s)",
        pending_removal.0, pending_removal.1
    );
    let _ = writeln!(report, "unresolved active occurrences by migration target:");
    if unresolved.is_empty() {
        let _ = writeln!(report, "  (none)");
    }
    for (target, (occ, rows)) in &unresolved {
        let _ = writeln!(report, "  {target}: {occ} occurrence(s) in {rows} row(s)");
    }
    report
}

/// `cargo xtask kwalitee-inventory --scaffold` — print entry skeletons with the
/// exact line hashes the current tree produces, for the initial bootstrap and
/// for repairing rows after a legitimate source move. Scaffold output is never
/// written anywhere; the maintainer copies hashes into reviewed rows and fills
/// the classification fields. `--check` keeps failing until they do.
fn scaffold(root: &Path, ledger: &Ledger) -> Result<String> {
    let observed = scan(root, ledger)?;
    let mut out = String::new();
    for (path, occs) in &observed {
        let mut hashes: Vec<String> = occs.iter().map(|o| o.hash.clone()).collect();
        hashes.sort();
        let _ = writeln!(out, "[[entry]]");
        let _ = writeln!(out, "path = {path:?}");
        let _ = writeln!(out, "classification = \"TODO\"");
        let _ = writeln!(out, "migration_target = \"TODO\"");
        let _ = writeln!(out, "owner_issue = 0");
        let _ = writeln!(out, "removal_condition = \"TODO\"");
        let _ = writeln!(out, "allowed_to_remain = true");
        let _ = writeln!(
            out,
            "line_hashes = [{}]",
            hashes.iter().map(|h| format!("{h:?}")).collect::<Vec<_>>().join(", ")
        );
        let _ = writeln!(out);
    }
    Ok(out)
}

fn parse_ledger(root: &Path) -> Result<Ledger> {
    let path = root.join(LEDGER_REL);
    let raw =
        fs::read_to_string(&path).with_context(|| format!("reading ledger {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("parsing ledger {}", path.display()))
}

/// Entry point for `cargo xtask kwalitee-inventory [--check] [--scaffold] [--root <dir>]`.
pub fn run(check: bool, scaffold_mode: bool, root_override: Option<PathBuf>) -> Result<()> {
    let root = match root_override {
        Some(dir) => {
            if !dir.is_dir() {
                bail!("--root {:?} is not a directory", dir);
            }
            dir
        }
        None => project_root()?,
    };
    let ledger = parse_ledger(&root)?;
    ledger.validate_static()?;

    if scaffold_mode {
        if check {
            bail!("--scaffold and --check are exclusive; scaffold never validates the tree");
        }
        print!("{}", scaffold(&root, &ledger)?);
        return Ok(());
    }

    let observed = scan(&root, &ledger)?;
    let report = reconcile(&ledger, &observed)?;
    print!("{report}");
    if check {
        println!("kwalitee namespace inventory is current.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_hash_is_deterministic_full_sha256_and_position_bound() {
        let a = line_hash(7, "cargo xtask perl-kwalitee report");
        let b = line_hash(7, "cargo xtask perl-kwalitee report");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, line_hash(8, "cargo xtask perl-kwalitee report"));
        assert_ne!(a, line_hash(7, "cargo xtask perl-kwalitee check"));
    }

    #[test]
    fn leading_whitespace_does_not_change_the_scanned_hash() {
        // Trimming happens at the scan layer; the hash sees the trimmed line.
        assert_eq!(
            line_hash(1, "  indented perl_kwalitee ref".trim()),
            line_hash(1, "indented perl_kwalitee ref")
        );
    }

    #[test]
    fn all_current_spellings_are_detected() {
        assert!(is_reference(&"uses perl-kwalitee crate".to_ascii_lowercase()));
        assert!(is_reference(&"kind = \"PERL_KWALITEE\"".to_ascii_lowercase()));
        assert!(is_reference(&"enum PerlKwaliteeCommand".to_ascii_lowercase()));
        assert!(is_reference(&"perl_kwalitee.v1 receipt".to_ascii_lowercase()));
        assert!(!is_reference("Module::CPANTS::SiteKwalitee only"));
        assert!(!is_reference("plain kwalitee prose without the namespace"));
    }

    #[test]
    fn pairing_rules_reject_mismatched_targets() {
        let mut entry = Entry {
            path: "docs/example.md".to_string(),
            classification: "historical_prose".to_string(),
            migration_target: "independent_readiness_rails".to_string(),
            owner_issue: 8752,
            removal_condition: "n/a".to_string(),
            allowed_to_remain: true,
            note: String::new(),
            line_hashes: vec![line_hash(1, "perl-kwalitee")],
        };
        assert!(validate_pairing(&entry).is_err());
        entry.migration_target = "none".to_string();
        assert!(validate_pairing(&entry).is_ok());
        entry.classification = "not_a_class".to_string();
        entry.migration_target = "none".to_string();
        assert!(validate_pairing(&entry).is_err());
    }

    #[test]
    fn surface_exclusion_matches_root_components_and_prefixes_only() {
        let excluded = ["target/**", ".wt-*", "generated/**"];
        assert!(surface_excluded("target", "target", &excluded, true));
        assert!(!surface_excluded("target", "target/file", &excluded, false));
        assert!(!surface_excluded("target", "crates/x/target", &excluded, true));
        assert!(surface_excluded(".wt-1234", ".wt-1234/sub/file", &excluded, true));
        assert!(!surface_excluded(".wt-1234", ".wt-1234/file", &excluded, false));
        assert!(!surface_excluded(".wt-1234", "crates/x/.wt-1234/sub/file", &excluded, true));
        assert!(surface_excluded("generated", "generated/out.json", &excluded, false));
        assert!(!surface_excluded("src", "src/lib.rs", &excluded, false));
        assert!(!surface_excluded("target_holder", "target_holder/f", &excluded, false));
    }
}
