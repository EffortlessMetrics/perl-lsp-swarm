//! Format task implementation

use color_eyre::eyre::{Context, Result, eyre};
use duct::cmd;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    manifest_path: String,
    edition: String,
}

/// One workspace member, reduced to what staged formatting needs.
///
/// `dir` is repository-relative so it can be prefix-matched against
/// `git diff --name-only` output. `edition` is carried because `rustfmt`
/// defaults to edition 2015 when invoked directly, which is *not* what
/// `cargo fmt` does — see [`run_staged`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspacePackage {
    pub(crate) name: String,
    pub(crate) dir: String,
    pub(crate) edition: String,
}

/// One crate's failure record collected during the per-crate iteration.
///
/// `unformatted_files` is populated in `--check` mode by parsing rustfmt's
/// `Diff in <path>` lines from stdout; in apply mode it stays empty (cargo
/// fmt without `--check` is expected to mutate files and exit zero unless
/// rustfmt itself errored, which we still report as a per-crate failure).
struct CrateFailure {
    manifest_path: String,
    unformatted_files: Vec<String>,
    spawn_error: Option<String>,
}

/// Classification of one staged Rust file for `--staged` formatting.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StagedFormatAction {
    /// Fully staged: safe to format in the worktree and re-stage, because the
    /// worktree content and the index content are the same bytes.
    FormatAndRestage(String),
    /// Staged *and* separately modified in the worktree. Deliberately left
    /// alone: formatting the file would rewrite unstaged work, and `git add`
    /// would then sweep those unrelated changes into this commit. Reported so
    /// the author fixes it deliberately rather than discovering a widened
    /// commit afterwards.
    SkipPartiallyStaged(String),
}

/// Splits staged Rust paths into the ones `--staged` may safely rewrite and
/// the ones it must not touch.
///
/// Pure so the safety rule — never rewrite a partially staged file — is
/// testable without a git fixture.
pub(crate) fn classify_staged_paths(
    staged: &[String],
    unstaged: &HashSet<String>,
) -> Vec<StagedFormatAction> {
    staged
        .iter()
        .filter(|path| path.ends_with(".rs"))
        .map(|path| {
            if unstaged.contains(path) {
                StagedFormatAction::SkipPartiallyStaged(path.clone())
            } else {
                StagedFormatAction::FormatAndRestage(path.clone())
            }
        })
        .collect()
}

fn git_lines(args: &[&str]) -> Result<Vec<String>> {
    let out = cmd("git", args)
        .stdout_capture()
        .unchecked()
        .run()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !out.status.success() {
        return Err(eyre!("git {} failed", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

/// Maps a repository-relative file path to the workspace package that owns it.
///
/// Longest-prefix wins so a crate nested inside another crate's directory is
/// attributed to the inner one.
///
/// A package whose directory is the workspace root has an empty relative
/// directory, for which `"{dir}/"` would be `"/"` and match nothing. It is
/// treated as matching every path; longest-prefix then still prefers a more
/// specific subdirectory package when one exists. This workspace is a virtual
/// manifest today, so that case is latent rather than live — but a helper that
/// silently attributed root-package files to no package would skip formatting
/// them, which is the failure this whole task exists to prevent.
pub(crate) fn owning_package<'a>(
    path: &str,
    packages: &'a [WorkspacePackage],
) -> Option<&'a WorkspacePackage> {
    packages
        .iter()
        .filter(|package| package.dir.is_empty() || path.starts_with(&format!("{}/", package.dir)))
        .max_by_key(|package| package.dir.len())
}

/// Formats the staged Rust diff and re-stages it.
///
/// This is the apply half of the `rustfmt_staged` commit gate: that check
/// blocks a commit whose staged Rust would be reformatted and tells the author
/// to run `cargo xtask fmt`, which reformats the entire workspace. `--staged`
/// narrows that to the packages actually being committed, which is what makes
/// it cheap enough to run from the pre-commit hook on every commit.
///
/// Formatting invokes `rustfmt` on exactly the staged files, never
/// `cargo fmt -p <package>`. That distinction is a safety property, not a
/// performance one: `cargo fmt -p` formats every file in the package against
/// the live worktree, so a staged file with an unstaged sibling in the same
/// package would silently rewrite that sibling's uncommitted work. Re-staging
/// only the staged paths keeps the *commit* narrow but cannot undo the
/// worktree mutation, so the package-wide call is not usable here at all.
///
/// The reason to reach for `cargo fmt` in the first place was that bare
/// `rustfmt` disagreed with it: on 40 already-gate-clean files, bare `rustfmt`
/// rewrote 9. That disagreement was entirely the **edition**. `rustfmt`
/// invoked directly defaults to edition 2015; `cargo fmt` passes each
/// package's actual edition. Re-measured over 80 gate-clean files:
///
/// | invocation | files rewritten |
/// | --- | --- |
/// | `rustfmt --check` (defaults to 2015) | 29 / 80 |
/// | `rustfmt --edition 2024 --check` | **0 / 80** |
///
/// So each file is formatted with its owning package's declared edition, which
/// reproduces `cargo fmt`'s output while touching nothing but the staged paths.
/// `rustfmt` discovers `rustfmt.toml` by walking up from each file, so the
/// workspace style config applies unchanged.
///
/// Only fully staged files are re-staged — see [`StagedFormatAction`].
pub fn run_staged() -> Result<()> {
    let staged = git_lines(&["diff", "--cached", "--name-only", "--diff-filter=ACMR"])?;
    let unstaged: HashSet<String> =
        git_lines(&["diff", "--name-only", "--diff-filter=ACMR"])?.into_iter().collect();
    let actions = classify_staged_paths(&staged, &unstaged);

    if actions.is_empty() {
        println!("No staged Rust files — nothing to format.");
        return Ok(());
    }

    let to_format: Vec<&String> = actions
        .iter()
        .filter_map(|action| match action {
            StagedFormatAction::FormatAndRestage(path) => Some(path),
            StagedFormatAction::SkipPartiallyStaged(_) => None,
        })
        .collect();
    let skipped: Vec<&String> = actions
        .iter()
        .filter_map(|action| match action {
            StagedFormatAction::SkipPartiallyStaged(path) => Some(path),
            StagedFormatAction::FormatAndRestage(_) => None,
        })
        .collect();

    if !to_format.is_empty() {
        let metadata = load_workspace_metadata()?;
        let packages = workspace_packages(&metadata);

        // Group by edition so one `rustfmt` process covers every staged file
        // sharing an edition. This runs on every commit, and a process per file
        // is a noticeable cost on Windows in particular. Files whose owning
        // package cannot be determined get no edition and are left to the gate
        // rather than formatted under a guessed one.
        let mut by_edition: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut unowned: Vec<&str> = Vec::new();
        for path in &to_format {
            match owning_package(path, &packages) {
                Some(package) => {
                    by_edition.entry(package.edition.as_str()).or_default().push(path.as_str());
                }
                None => unowned.push(path.as_str()),
            }
        }

        let mut formatted: Vec<&str> = Vec::new();
        let mut editions: Vec<&&str> = by_edition.keys().collect();
        editions.sort_unstable();
        for edition in editions {
            let files = &by_edition[*edition];
            let mut args = vec!["--edition", edition];
            args.extend(files.iter().copied());
            cmd("rustfmt", &args).run().with_context(|| {
                format!("rustfmt failed on staged edition-{edition} file(s): {}", files.join(", "))
            })?;
            formatted.extend(files.iter().copied());
        }

        if formatted.is_empty() {
            println!(
                "Staged Rust files are outside every workspace package; leaving them to the gate."
            );
        } else {
            formatted.sort_unstable();
            let mut add_args = vec!["add", "--"];
            add_args.extend(formatted.iter().copied());
            cmd("git", &add_args).run().context("failed to re-stage formatted files")?;
            println!("Formatted and re-staged {} staged Rust file(s).", formatted.len());
        }

        if !unowned.is_empty() {
            println!(
                "Left {} staged Rust file(s) outside every workspace package to the gate.",
                unowned.len()
            );
        }
    }

    if !skipped.is_empty() {
        println!();
        println!(
            "Left {} partially staged file(s) untouched — they have unstaged changes,",
            skipped.len()
        );
        println!("and re-staging them would pull that unstaged work into this commit:");
        for path in &skipped {
            println!("   {path}");
        }
        println!();
        println!("   Stage or stash the rest, then re-run: cargo xtask fmt --staged");
    }

    Ok(())
}

pub fn run(check: bool, package_filters: Option<Vec<String>>) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {wide_msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );

    let action = if check { "Checking" } else { "Formatting" };
    spinner.set_message(format!("{} code", action));

    let mut failures: Vec<CrateFailure> = Vec::new();

    for manifest_path in workspace_manifest_paths(package_filters.as_deref())? {
        spinner.set_message(format!("{} {}", action, manifest_path));

        let mut args = vec!["fmt".to_string(), "--manifest-path".to_string(), manifest_path];
        if check {
            args.push("--".to_string());
            args.push("--check".to_string());
        }

        // Capture stdout in --check mode so we can name the unformatted files
        // in the aggregate error report. In apply mode we let cargo's output
        // pass through unchanged so users still see rustfmt warnings live.
        let result = if check {
            cmd("cargo", &args).stdout_capture().unchecked().run()
        } else {
            cmd("cargo", &args).unchecked().run()
        };

        match result {
            Ok(output) => {
                if !output.status.success() {
                    let unformatted_files =
                        if check { parse_unformatted_files(&output.stdout) } else { Vec::new() };
                    // In --check mode, echo the captured stdout so the user
                    // still sees the diff context in addition to the summary.
                    if check && !output.stdout.is_empty() {
                        print!("{}", String::from_utf8_lossy(&output.stdout));
                    }
                    failures.push(CrateFailure {
                        manifest_path: args[2].clone(),
                        unformatted_files,
                        spawn_error: None,
                    });
                }
            }
            Err(err) => {
                // Spawn / I/O failures are kept in the aggregate report
                // rather than aborting on the first crate, so a single run
                // still surfaces every per-crate problem.
                failures.push(CrateFailure {
                    manifest_path: args[2].clone(),
                    unformatted_files: Vec::new(),
                    spawn_error: Some(err.to_string()),
                });
            }
        }
    }

    if failures.is_empty() {
        spinner.finish_with_message(format!(
            "✅ Code {} successfully",
            if check { "check passed" } else { "formatted" }
        ));
        return Ok(());
    }

    spinner.finish_with_message(format!(
        "❌ Code {} failed in {} crate(s)",
        if check { "check" } else { "formatting" },
        failures.len()
    ));
    Err(eyre!("{}", format_failure_report(check, &failures)))
}

/// Parse rustfmt's `Diff in <path>` lines from captured stdout.
///
/// rustfmt emits one of these two header shapes per unformatted file:
///   * `Diff in <path> at line N:`  (older / verbose-diff)
///   * `Diff in <path>:<N>:`        (current default on recent toolchains)
///
/// Pulling the path out lets the aggregate error name every offending file,
/// not just the crate that contains them. Returns a deduplicated,
/// insertion-ordered list (rustfmt may repeat a path across multiple hunks).
fn parse_unformatted_files(stdout: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Diff in ") {
            let path = extract_diff_path(rest);
            if !path.is_empty() && seen.insert(path.to_string()) {
                files.push(path.to_string());
            }
        }
    }
    files
}

/// Extract the file path from a rustfmt `Diff in ...` line tail.
///
/// Handles both the `<path> at line N:` and `<path>:<N>:` shapes, plus
/// Windows paths like `\\?\C:\...\lib.rs:11:` where the trailing `:line:`
/// must not be confused with the drive-letter colon earlier in the path.
fn extract_diff_path(rest: &str) -> &str {
    // Verbose-diff shape: `<path> at line N:` — split on the literal marker
    // and ignore the trailing line/column completely.
    if let Some(idx) = rest.rfind(" at line ") {
        return rest[..idx].trim();
    }
    // Default shape: `<path>:<line>:` — strip the trailing `:`, then strip the
    // trailing digit run (line number), then strip the separator `:`. Keeping
    // this lexical (rather than regex) matches the rest of the xtask style.
    let mut s = rest.trim().trim_end_matches(':');
    let stripped = s.trim_end_matches(|c: char| c.is_ascii_digit());
    if stripped.len() < s.len() && stripped.ends_with(':') {
        s = stripped.trim_end_matches(':');
    }
    s.trim()
}

/// Build the aggregate error message that lists every failing crate.
///
/// In `--check` mode the report names each unformatted file under the crate
/// that owns it, replacing the historical generic "Failed to format
/// Cargo.toml" message that masked per-PR drift as a master cascade.
fn format_failure_report(check: bool, failures: &[CrateFailure]) -> String {
    let mut report = String::new();
    let header = if check {
        format!("cargo fmt --check found unformatted files in {} crate(s):", failures.len())
    } else {
        format!("cargo fmt failed in {} crate(s):", failures.len())
    };
    report.push_str(&header);
    for failure in failures {
        report.push_str("\n  - ");
        report.push_str(&failure.manifest_path);
        if let Some(spawn_error) = &failure.spawn_error {
            report.push_str(" (spawn failed: ");
            report.push_str(spawn_error);
            report.push(')');
        }
        for file in &failure.unformatted_files {
            report.push_str("\n      ");
            report.push_str(file);
        }
    }
    report
}

fn load_workspace_metadata() -> Result<CargoMetadata> {
    let metadata_json = cmd("cargo", ["metadata", "--format-version", "1", "--no-deps"])
        .read()
        .context("Failed to query cargo metadata for workspace formatting")?;
    serde_json::from_str(&metadata_json).context("Failed to parse cargo metadata JSON")
}

/// Workspace members reduced to name, repository-relative directory, and edition.
///
/// The directory is the manifest's parent, relative to the workspace root, so
/// it can be prefix-matched against `git diff --name-only` output.
fn workspace_packages(metadata: &CargoMetadata) -> Vec<WorkspacePackage> {
    let root = std::env::current_dir().ok();
    metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.iter().any(|member| member == &package.id))
        .filter_map(|package| {
            let manifest = Path::new(&package.manifest_path);
            let dir = manifest.parent()?;
            let relative = match &root {
                Some(root) => dir.strip_prefix(root).unwrap_or(dir),
                None => dir,
            };
            Some(WorkspacePackage {
                name: package.name.clone(),
                dir: relative.to_string_lossy().replace('\\', "/"),
                edition: package.edition.clone(),
            })
        })
        .collect()
}

fn workspace_manifest_paths(package_filters: Option<&[String]>) -> Result<Vec<String>> {
    let metadata = load_workspace_metadata()?;
    collect_workspace_manifest_paths(&metadata, package_filters)
}

fn collect_workspace_manifest_paths(
    metadata: &CargoMetadata,
    package_filters: Option<&[String]>,
) -> Result<Vec<String>> {
    let package_by_id: HashMap<_, _> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package.manifest_path.clone()))
        .collect();
    let member_name_to_manifest: HashMap<_, _> = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.iter().any(|member| member == &package.id))
        .map(|package| (package.name.as_str(), package.manifest_path.clone()))
        .collect();

    if let Some(filters) = package_filters {
        let mut selected = Vec::with_capacity(filters.len());
        for package_name in filters {
            if let Some(manifest_path) = member_name_to_manifest.get(package_name.as_str()) {
                selected.push(manifest_path.clone());
            } else {
                // Sort the available list so the error message is stable across runs.
                let mut available: Vec<_> = member_name_to_manifest.keys().copied().collect();
                available.sort_unstable();
                return Err(eyre!(
                    "Unknown package `{package_name}`. Available workspace packages: {}",
                    available.join(", ")
                ));
            }
        }
        return Ok(dedup_preserve_order(selected));
    }

    metadata
        .workspace_members
        .iter()
        .map(|member_id| {
            package_by_id
                .get(member_id.as_str())
                .cloned()
                .ok_or_else(|| eyre!("Workspace member not found in cargo metadata: {member_id}"))
        })
        .collect()
}

fn dedup_preserve_order(paths: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::with_capacity(paths.len());
    let mut deduped = Vec::with_capacity(paths.len());
    for path in paths {
        if seen.insert(path.clone()) {
            deduped.push(path);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::{
        CargoMetadata, CargoPackage, CrateFailure, StagedFormatAction, WorkspacePackage,
        classify_staged_paths, collect_workspace_manifest_paths, format_failure_report,
        parse_unformatted_files,
    };
    use color_eyre::eyre::Result;
    use std::collections::HashSet;
    use std::fs;

    fn unstaged(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    #[test]
    fn staged_only_rust_files_are_formatted_and_restaged() {
        let staged = vec!["crates/a/src/lib.rs".to_string(), "docs/readme.md".to_string()];
        let actions = classify_staged_paths(&staged, &unstaged(&[]));
        assert_eq!(
            actions,
            vec![StagedFormatAction::FormatAndRestage("crates/a/src/lib.rs".to_string())],
            "non-Rust staged paths must not be handed to rustfmt"
        );
    }

    #[test]
    fn a_partially_staged_file_is_never_rewritten() {
        // The footgun this guards: formatting the worktree copy would rewrite
        // the author's unstaged work, and the follow-up `git add` would sweep
        // it into the commit. Skipping is the only safe action.
        let staged = vec!["crates/a/src/lib.rs".to_string()];
        let actions = classify_staged_paths(&staged, &unstaged(&["crates/a/src/lib.rs"]));
        assert_eq!(
            actions,
            vec![StagedFormatAction::SkipPartiallyStaged("crates/a/src/lib.rs".to_string())],
        );
    }

    #[test]
    fn unrelated_unstaged_files_do_not_block_a_fully_staged_one() {
        // Only an overlap between the staged and unstaged sets is dangerous.
        let staged = vec!["crates/a/src/lib.rs".to_string()];
        let actions = classify_staged_paths(&staged, &unstaged(&["crates/b/src/other.rs"]));
        assert_eq!(
            actions,
            vec![StagedFormatAction::FormatAndRestage("crates/a/src/lib.rs".to_string())],
        );
    }

    fn package(name: &str, dir: &str, edition: &str) -> WorkspacePackage {
        WorkspacePackage {
            name: name.to_string(),
            dir: dir.to_string(),
            edition: edition.to_string(),
        }
    }

    fn package_dirs() -> Vec<WorkspacePackage> {
        vec![
            package("perl-parser", "crates/perl-parser", "2024"),
            package("perl-parser-core", "crates/perl-parser-core", "2024"),
            package("xtask", "xtask", "2024"),
        ]
    }

    fn owner_name(path: &str, packages: &[WorkspacePackage]) -> Option<String> {
        super::owning_package(path, packages).map(|package| package.name.clone())
    }

    #[test]
    fn a_staged_path_maps_to_its_owning_package() {
        assert_eq!(
            owner_name("crates/perl-parser-core/src/lib.rs", &package_dirs()).as_deref(),
            Some("perl-parser-core")
        );
        assert_eq!(owner_name("xtask/src/main.rs", &package_dirs()).as_deref(), Some("xtask"));
    }

    #[test]
    fn a_similar_prefix_does_not_capture_a_sibling_package() {
        // "crates/perl-parser" is a prefix of "crates/perl-parser-core" as a
        // string; only a full path-segment match may win.
        assert_eq!(
            owner_name("crates/perl-parser-core/src/lib.rs", &package_dirs()).as_deref(),
            Some("perl-parser-core"),
        );
        assert_eq!(
            owner_name("crates/perl-parser/src/lib.rs", &package_dirs()).as_deref(),
            Some("perl-parser"),
        );
    }

    #[test]
    fn a_workspace_root_package_owns_paths_no_subpackage_claims() {
        // A root package's relative directory is "", for which "{dir}/" would
        // be "/" and match nothing. It must still own its own files.
        let dirs = vec![package("root-crate", "", "2024"), package("xtask", "xtask", "2024")];
        assert_eq!(owner_name("src/lib.rs", &dirs).as_deref(), Some("root-crate"));
        // ...and must not shadow a more specific package.
        assert_eq!(owner_name("xtask/src/main.rs", &dirs).as_deref(), Some("xtask"));
    }

    #[test]
    fn a_path_outside_every_package_maps_to_nothing() {
        // Falls through to the gate rather than guessing a package.
        assert_eq!(owner_name("docs/notes.rs", &package_dirs()), None);
    }

    #[test]
    fn the_owning_package_carries_the_edition_rustfmt_must_be_given() {
        // The whole reason `owning_package` returns the package rather than its
        // name: bare `rustfmt` defaults to edition 2015 and reformats
        // gate-clean edition-2024 files. Measured on this workspace over 80
        // gate-clean files: `rustfmt --check` rewrote 29, `rustfmt --edition
        // 2024 --check` rewrote 0.
        let packages = vec![package("legacy-crate", "crates/legacy", "2021")];
        let owner = super::owning_package("crates/legacy/src/lib.rs", &packages)
            .expect("legacy path must resolve to its package");
        assert_eq!(owner.edition, "2021", "each file must be formatted at its own package edition");
    }

    #[test]
    fn nothing_staged_yields_no_actions() {
        assert!(classify_staged_paths(&[], &unstaged(&["crates/a/src/lib.rs"])).is_empty());
    }

    use std::path::Path;

    fn sample_metadata() -> CargoMetadata {
        CargoMetadata {
            packages: vec![
                CargoPackage {
                    id: "path+file:///repo/xtask#0.1.0".to_string(),
                    name: "xtask".to_string(),
                    manifest_path: "/repo/xtask/Cargo.toml".to_string(),
                    edition: "2024".to_string(),
                },
                CargoPackage {
                    id: "path+file:///repo/crates/perl-parser#0.1.0".to_string(),
                    name: "perl-parser".to_string(),
                    manifest_path: "/repo/crates/perl-parser/Cargo.toml".to_string(),
                    edition: "2024".to_string(),
                },
            ],
            workspace_members: vec![
                "path+file:///repo/xtask#0.1.0".to_string(),
                "path+file:///repo/crates/perl-parser#0.1.0".to_string(),
            ],
        }
    }

    #[test]
    fn package_filters_select_requested_manifest_paths() -> Result<()> {
        let metadata = sample_metadata();
        let filters = vec!["perl-parser".to_string()];
        let manifests = collect_workspace_manifest_paths(&metadata, Some(&filters))?;
        assert_eq!(manifests, vec!["/repo/crates/perl-parser/Cargo.toml".to_string()]);
        Ok(())
    }

    #[test]
    fn package_filters_are_deduplicated() -> Result<()> {
        let metadata = sample_metadata();
        let filters = vec!["xtask".to_string(), "xtask".to_string()];
        let manifests = collect_workspace_manifest_paths(&metadata, Some(&filters))?;
        assert_eq!(manifests, vec!["/repo/xtask/Cargo.toml".to_string()]);
        Ok(())
    }

    #[test]
    fn package_filters_report_unknown_package() -> Result<()> {
        let metadata = sample_metadata();
        let filters = vec!["missing-package".to_string()];
        let message = match collect_workspace_manifest_paths(&metadata, Some(&filters)) {
            Ok(paths) => {
                return Err(color_eyre::eyre::eyre!("expected error, got paths: {paths:?}"));
            }
            Err(err) => format!("{err}"),
        };
        assert!(message.contains("missing-package"));
        assert!(message.contains("Available workspace packages"));
        Ok(())
    }

    #[test]
    fn package_filters_error_lists_packages_in_stable_sorted_order() -> Result<()> {
        let metadata = sample_metadata();
        let filters = vec!["nonexistent".to_string()];
        let message = match collect_workspace_manifest_paths(&metadata, Some(&filters)) {
            Ok(paths) => {
                return Err(color_eyre::eyre::eyre!("expected error, got paths: {paths:?}"));
            }
            Err(err) => format!("{err}"),
        };
        // The available list must be sorted — both packages appear in alphabetical order.
        let perl_pos = message.find("perl-parser").expect("perl-parser in error");
        let xtask_pos = message.find("xtask").expect("xtask in error");
        assert!(perl_pos < xtask_pos, "available packages must be listed in sorted order");
        Ok(())
    }

    #[test]
    fn parse_unformatted_files_extracts_paths_from_verbose_diff_lines() {
        // Older rustfmt and verbose-diff modes emit `<path> at line N:`.
        let stdout = b"Diff in /repo/crates/foo/src/lib.rs at line 12:\n\
             -    let x = 1;\n\
             +    let x = 1;\n\
             Diff in /repo/crates/bar/src/main.rs at line 3:\n\
             -fn main(){}\n";
        let files = parse_unformatted_files(stdout);
        assert_eq!(
            files,
            vec![
                "/repo/crates/foo/src/lib.rs".to_string(),
                "/repo/crates/bar/src/main.rs".to_string(),
            ]
        );
    }

    #[test]
    fn parse_unformatted_files_extracts_paths_from_default_diff_lines() {
        // Current default rustfmt output: `<path>:<line>:` — must not chop the
        // drive-letter colon out of `\\?\C:\...` style Windows paths.
        let stdout = b"Diff in /repo/crates/foo/src/lib.rs:11:\n\
             -    let x = 1;\n\
             Diff in \\\\?\\C:\\repo\\crates\\bar\\src\\main.rs:42:\n\
             -fn main(){}\n";
        let files = parse_unformatted_files(stdout);
        assert_eq!(
            files,
            vec![
                "/repo/crates/foo/src/lib.rs".to_string(),
                "\\\\?\\C:\\repo\\crates\\bar\\src\\main.rs".to_string(),
            ]
        );
    }

    #[test]
    fn parse_unformatted_files_deduplicates_repeated_paths() {
        let stdout = b"Diff in /repo/a/src/lib.rs at line 1:\n\
             Diff in /repo/a/src/lib.rs at line 42:\n";
        let files = parse_unformatted_files(stdout);
        assert_eq!(files, vec!["/repo/a/src/lib.rs".to_string()]);
    }

    #[test]
    fn parse_unformatted_files_returns_empty_for_clean_output() {
        let files = parse_unformatted_files(b"");
        assert!(files.is_empty());
        let files = parse_unformatted_files(b"some unrelated cargo output\n");
        assert!(files.is_empty());
    }

    #[test]
    fn format_failure_report_lists_every_failing_crate_in_check_mode() {
        let failures = vec![
            CrateFailure {
                manifest_path: "crates/foo/Cargo.toml".to_string(),
                unformatted_files: vec!["crates/foo/src/lib.rs".to_string()],
                spawn_error: None,
            },
            CrateFailure {
                manifest_path: "crates/bar/Cargo.toml".to_string(),
                unformatted_files: vec![
                    "crates/bar/src/lib.rs".to_string(),
                    "crates/bar/src/util.rs".to_string(),
                ],
                spawn_error: None,
            },
        ];
        let report = format_failure_report(true, &failures);
        // Both crates and every unformatted file must appear so operators can
        // distinguish per-PR drift from a shared master cascade in one read.
        assert!(report.contains("2 crate(s)"));
        assert!(report.contains("crates/foo/Cargo.toml"));
        assert!(report.contains("crates/bar/Cargo.toml"));
        assert!(report.contains("crates/foo/src/lib.rs"));
        assert!(report.contains("crates/bar/src/lib.rs"));
        assert!(report.contains("crates/bar/src/util.rs"));
        // Regression: never emit the original misleading generic message.
        assert!(!report.contains("Failed to format Cargo.toml"));
    }

    #[test]
    fn format_failure_report_surfaces_spawn_errors_inline() {
        let failures = vec![CrateFailure {
            manifest_path: "crates/foo/Cargo.toml".to_string(),
            unformatted_files: Vec::new(),
            spawn_error: Some("rustfmt not found".to_string()),
        }];
        let report = format_failure_report(false, &failures);
        assert!(report.contains("cargo fmt failed"));
        assert!(report.contains("crates/foo/Cargo.toml"));
        assert!(report.contains("rustfmt not found"));
    }

    #[test]
    fn xtask_tasks_do_not_shell_out_to_workspace_wide_cargo_fmt_all() -> Result<()> {
        let xtask_tasks = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tasks");
        let mut offenders = Vec::new();
        collect_workspace_fmt_all_offenders(&xtask_tasks, &mut offenders)?;

        assert!(
            offenders.is_empty(),
            "repo-owned xtask gates must route formatting through fmt::run, not raw workspace fmt: {offenders:?}"
        );
        Ok(())
    }

    fn collect_workspace_fmt_all_offenders(dir: &Path, offenders: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                collect_workspace_fmt_all_offenders(&path, offenders)?;
                continue;
            }

            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }

            let source = fs::read_to_string(&path)?;
            if path.file_name().and_then(|name| name.to_str()) == Some("fmt.rs") {
                continue;
            }

            let argv_pattern = ["\"fmt\"", ", \"--all\""].concat();
            let shell_pattern = ["cargo fmt", " --all"].concat();
            let args_pattern = ["args([\"fmt\"", ", \"--all\""].concat();
            if source.contains(&argv_pattern)
                || source.contains(&shell_pattern)
                || source.contains(&args_pattern)
            {
                offenders.push(path.display().to_string());
            }
        }

        Ok(())
    }
}
