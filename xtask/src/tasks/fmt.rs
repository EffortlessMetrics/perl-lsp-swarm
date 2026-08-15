//! Format task implementation

use color_eyre::eyre::{Context, Result, eyre};
use duct::cmd;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    pub(crate) dir: PathBuf,
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
    FormatAndRestage(PathBuf),
    /// Staged *and* separately modified in the worktree. Deliberately left
    /// alone: formatting the file would rewrite unstaged work, and `git add`
    /// would then sweep those unrelated changes into this commit. Reported so
    /// the author fixes it deliberately rather than discovering a widened
    /// commit afterwards.
    SkipPartiallyStaged(PathBuf),
}

/// Splits staged Rust paths into the ones `--staged` may safely rewrite and
/// the ones it must not touch.
///
/// Pure so the safety rule — never rewrite a partially staged file — is
/// testable without a git fixture.
pub(crate) fn classify_staged_paths(
    staged: &[PathBuf],
    unstaged: &HashSet<PathBuf>,
) -> Vec<StagedFormatAction> {
    staged
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| {
            if unstaged.contains(path) {
                StagedFormatAction::SkipPartiallyStaged(path.clone())
            } else {
                StagedFormatAction::FormatAndRestage(path.clone())
            }
        })
        .collect()
}

/// Runs git and returns its NUL-delimited output as paths.
///
/// `-z` rather than newline-delimited output, and raw bytes rather than text,
/// because git guarantees neither a newline-free nor a UTF-8 filename:
///
/// - a filename may legally contain `\n` on Unix, so splitting on lines turns
///   one staged path into two paths that do not exist;
/// - a Unix filename is an arbitrary byte string, so `from_utf8_lossy` would
///   replace the offending bytes and yield a path naming a different file (or
///   none), silently formatting and staging the wrong thing.
///
/// `core.quotePath=false` keeps git from quoting and octal-escaping non-ASCII
/// paths; combined with `-z` the output is the exact bytes, NUL-separated.
fn git_paths(args: &[&str]) -> Result<Vec<PathBuf>> {
    let mut full: Vec<&str> = vec!["-c", "core.quotePath=false"];
    full.extend_from_slice(args);
    let out = cmd("git", &full)
        .stdout_capture()
        .unchecked()
        .run()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !out.status.success() {
        return Err(eyre!("git {} failed", args.join(" ")));
    }
    split_nul_paths(&out.stdout)
}

/// Splits NUL-delimited git output into paths, preserving the original bytes.
///
/// Separate from the process call so the delimiter handling is testable without
/// creating a repository containing a newline-bearing filename.
fn split_nul_paths(stdout: &[u8]) -> Result<Vec<PathBuf>> {
    stdout.split(|byte| *byte == 0).filter(|record| !record.is_empty()).map(bytes_to_path).collect()
}

/// A git path record as a `PathBuf`, without lossy conversion.
#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
}

/// On Windows a path is UTF-16 and git reports it as UTF-8, so invalid UTF-8
/// here is a genuine anomaly. Reported rather than replaced: a mangled path
/// would name a different file, and formatting the wrong file is worse than
/// refusing.
#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> Result<PathBuf> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        eyre!("git reported a non-UTF-8 path this platform cannot represent: {error}")
    })?;
    Ok(PathBuf::from(text))
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
    path: &Path,
    packages: &'a [WorkspacePackage],
) -> Option<&'a WorkspacePackage> {
    packages
        .iter()
        // `Path::starts_with` matches whole components, so "crates/perl-parser"
        // cannot capture "crates/perl-parser-core/src/lib.rs" the way a string
        // prefix would. An empty dir (a workspace-root package) prefixes every
        // path, which is the intended "owns anything unclaimed" behaviour.
        .filter(|package| path.starts_with(&package.dir))
        .max_by_key(|package| package.dir.as_os_str().len())
}

/// Formats the staged Rust diff and re-stages it.
///
/// This is the apply half of the `rustfmt_staged` commit gate: that check
/// blocks a commit whose staged Rust would be reformatted and tells the author
/// to run `cargo xtask fmt`, which reformats the entire workspace. `--staged`
/// narrows that to the packages actually being committed, which is what makes
/// it cheap enough to run from the pre-commit hook on every commit.
///
/// Staged content is fed to `rustfmt` over stdin, one file at a time. Neither
/// `cargo fmt -p <package>` nor path-mode `rustfmt <file>` is usable here, and
/// both for the same reason — each writes files the author did not stage:
///
/// - `cargo fmt -p` formats every file in the package against the live
///   worktree, so an unstaged sibling gets rewritten.
/// - path-mode `rustfmt` resolves the file's out-of-line `mod child;`
///   declarations and rewrites those children too, so a staged `lib.rs` or
///   `mod.rs` reaches an unstaged child module. That path bypasses
///   [`classify_staged_paths`] entirely, since the child was never staged.
///
/// Re-staging only the staged paths bounds the *commit* but cannot undo either
/// worktree mutation. See [`rustfmt_text`] for the stdin contract and the
/// measurements behind it.
///
/// The run is transactional in two stages. Every file is formatted in memory
/// first, so a rustfmt failure writes nothing at all. The commit stage then
/// writes and re-stages under [`commit_formatted`], which restores the original
/// bytes if any write or the `git add` fails. Both matter for the same reason:
/// a file rewritten in the worktree but absent from the index is classified as
/// partially staged by the *next* run and skipped, so the author's formatting
/// silently stops happening.
///
/// The one case that is not fully recoverable is a rollback that itself fails;
/// that leaves the file formatted-but-unstaged and is reported by name rather
/// than swallowed.
///
/// Only fully staged files are re-staged — see [`StagedFormatAction`].
pub fn run_staged() -> Result<()> {
    let staged = git_paths(&["diff", "--cached", "--name-only", "-z", "--diff-filter=ACMR"])?;
    let unstaged: HashSet<PathBuf> =
        git_paths(&["diff", "--name-only", "-z", "--diff-filter=ACMR"])?.into_iter().collect();
    let actions = classify_staged_paths(&staged, &unstaged);

    if actions.is_empty() {
        println!("No staged Rust files — nothing to format.");
        return Ok(());
    }

    let to_format: Vec<&PathBuf> = actions
        .iter()
        .filter_map(|action| match action {
            StagedFormatAction::FormatAndRestage(path) => Some(path),
            StagedFormatAction::SkipPartiallyStaged(_) => None,
        })
        .collect();
    let skipped: Vec<&PathBuf> = actions
        .iter()
        .filter_map(|action| match action {
            StagedFormatAction::SkipPartiallyStaged(path) => Some(path),
            StagedFormatAction::FormatAndRestage(_) => None,
        })
        .collect();

    if !to_format.is_empty() {
        let root = repo_root()?;
        let metadata = load_workspace_metadata()?;
        let packages = workspace_packages(&metadata, &root);
        // The config actually being committed — see `staged_rustfmt_config`.
        let config_text = staged_rustfmt_config()?;

        // Index modes, so a staged symlink is recognised before it is read.
        let index_modes = staged_index_modes()?;

        // Phase 1 — format in memory. Nothing on disk is touched yet.
        let mut pending: Vec<FormattedFile> = Vec::new();
        let mut unowned: Vec<&Path> = Vec::new();
        let mut irregular: Vec<&Path> = Vec::new();
        for path in &to_format {
            let Some(package) = owning_package(path, &packages) else {
                // No package means no edition, and guessing one would format
                // against the wrong language rules. Leave it to the gate.
                unowned.push(path.as_path());
                continue;
            };
            // git paths are repository-relative and this may run from a
            // subdirectory, so resolve against the git root.
            let absolute = root.join(path);

            // Only ever rewrite a regular file, checked on both sides.
            //
            // `read_to_string` follows a symlink, but the commit phase renames
            // a regular temp file over the link — so a staged `foo.rs` symlink
            // became a regular file holding its target's bytes, and `git add`
            // recorded that as a 120000 -> 100644 type change. Measured before
            // the fix, with a target outside the source tree.
            //
            // The worktree type alone is not enough: the index is what gets
            // committed, so an entry the index calls a symlink is refused even
            // if the worktree copy currently looks regular (and vice versa).
            let worktree_regular = std::fs::symlink_metadata(&absolute)
                .with_context(|| format!("failed to stat staged file {}", path.display()))?
                .file_type()
                .is_file();
            let index_regular = index_modes
                .get(path.as_path())
                .is_none_or(|mode| mode == "100644" || mode == "100755");
            if !worktree_regular || !index_regular {
                irregular.push(path.as_path());
                continue;
            }

            let original = std::fs::read_to_string(&absolute)
                .with_context(|| format!("failed to read staged file {}", path.display()))?;
            let formatted = rustfmt_text(
                config_text.as_deref(),
                &package.edition,
                &original,
                &path.display().to_string(),
            )?;
            if formatted != original {
                pending.push(FormattedFile { path: absolute, original, formatted });
            }
        }

        // Phase 2 — commit the results, with rollback. See `commit_formatted`.
        if pending.is_empty() {
            println!("Staged Rust files are already formatted.");
        } else {
            commit_formatted(&pending, &mut write_file_atomically, &mut git_add_paths)?;
            println!("Formatted and re-staged {} staged Rust file(s).", pending.len());
        }

        if !unowned.is_empty() {
            println!(
                "Left {} staged Rust file(s) outside every workspace package to the gate.",
                unowned.len()
            );
        }

        if !irregular.is_empty() {
            println!();
            println!(
                "Left {} staged path(s) untouched — they are not regular files, and formatting",
                irregular.len()
            );
            println!("one would replace it with a regular file holding its target's bytes:");
            for path in &irregular {
                println!("   {}", path.display());
            }
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
            println!("   {}", path.display());
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
/// `repo_root` must be the git top level, **not** the process working
/// directory. `git diff --name-only` reports repository-relative paths wherever
/// it is invoked from (verified: run inside `xtask/`, it still prints
/// `xtask/src/main.rs`). Anchoring on the working directory breaks as soon as
/// the command runs from a subdirectory: every manifest outside it keeps its
/// absolute path and matches nothing, while that subdirectory's own package
/// strips to `""`, which [`owning_package`] treats as a workspace-root package
/// owning *every* path — so files get formatted at the wrong edition and
/// rustfmt is handed paths that do not resolve.
fn workspace_packages(metadata: &CargoMetadata, repo_root: &Path) -> Vec<WorkspacePackage> {
    metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.iter().any(|member| member == &package.id))
        .filter_map(|package| {
            let manifest = Path::new(&package.manifest_path);
            let dir = manifest.parent()?;
            // A package outside the repository cannot own a git-reported path;
            // leaving it absolute makes it match nothing, so its files fall
            // through to the gate instead of being misattributed.
            let relative = dir.strip_prefix(repo_root).unwrap_or(dir);
            Some(WorkspacePackage {
                name: package.name.clone(),
                dir: relative.to_path_buf(),
                edition: package.edition.clone(),
            })
        })
        .collect()
}

/// One staged file that rustfmt changed, with the bytes needed to roll back.
pub(crate) struct FormattedFile {
    pub(crate) path: PathBuf,
    pub(crate) original: String,
    pub(crate) formatted: String,
}

/// Writes every formatted file and re-stages them, restoring the worktree if
/// any step fails.
///
/// Formatting in memory first (phase 1) only makes rustfmt failures safe. The
/// commit phase has its own partial-failure modes, and both were real:
///
/// - a later `write` failing leaves earlier files rewritten while the index
///   still holds the old bytes;
/// - every `write` succeeding but `git add` failing leaves the whole set
///   rewritten and unstaged.
///
/// Either way the next run sees worktree ≠ index, classifies those files as
/// partially staged, and skips them — the author's formatting silently stops
/// happening. So on any failure this restores the original bytes of everything
/// it had already written, leaving the worktree as it found it.
///
/// Rollback is best-effort by nature: if restoring a file *also* fails, that
/// file genuinely is left modified, and the returned error names it explicitly
/// rather than implying a clean state.
///
/// `write` and `stage` are injected so the failure paths are testable without
/// a read-only filesystem or a broken git.
pub(crate) fn commit_formatted(
    files: &[FormattedFile],
    write: &mut dyn FnMut(&Path, &str) -> Result<()>,
    stage: &mut dyn FnMut(&[&Path]) -> Result<()>,
) -> Result<()> {
    let mut written: Vec<&FormattedFile> = Vec::with_capacity(files.len());

    for file in files {
        if let Err(error) = write(&file.path, &file.formatted) {
            let context = format!("failed to write formatted {}", file.path.display());
            return Err(rollback(&written, write, error, context));
        }
        written.push(file);
    }

    let paths: Vec<&Path> = written.iter().map(|file| file.path.as_path()).collect();
    if let Err(error) = stage(&paths) {
        return Err(rollback(&written, write, error, "failed to re-stage formatted files".into()));
    }
    Ok(())
}

/// Restores `written` to their original bytes and builds the reported error.
fn rollback(
    written: &[&FormattedFile],
    write: &mut dyn FnMut(&Path, &str) -> Result<()>,
    cause: color_eyre::Report,
    context: String,
) -> color_eyre::Report {
    let mut unrestored: Vec<String> = Vec::new();
    for file in written {
        if write(&file.path, &file.original).is_err() {
            unrestored.push(file.path.display().to_string());
        }
    }

    if unrestored.is_empty() {
        return cause.wrap_err(format!("{context}; the worktree was restored, nothing re-staged"));
    }
    cause.wrap_err(format!(
        "{context}; rollback also failed, so these file(s) are left formatted in the worktree but \
         not staged — re-run `cargo xtask fmt --staged` or `git checkout --` them: {}",
        unrestored.join(", ")
    ))
}

/// Replaces `path`'s contents via a same-directory temp file and a rename, so
/// a crash or a full disk cannot leave a half-written source file behind.
fn write_file_atomically(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .ok_or_else(|| eyre!("cannot write {} — it has no parent directory", path.display()))?;

    // The destination's mode must be carried onto the replacement. `persist`
    // renames, so the file the author ends up with is the *temp* file, and
    // `NamedTempFile` creates at 0600. Without this, every formatted file
    // silently became owner-only — and an executable `.rs` lost its exec bit,
    // which `git add` then recorded as a real 100755 -> 100644 index change.
    // Measured before the fix: 644 -> 600, and 755/100755 -> 600/100644.
    //
    // Read before writing anything, so a mode we cannot determine aborts the
    // write instead of silently downgrading the file.
    let permissions = std::fs::metadata(path)
        .with_context(|| format!("failed to read the current mode of {}", path.display()))?
        .permissions();

    // Same directory, because a rename across filesystems is not atomic (and
    // on many systems simply fails).
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create a temp file beside {}", path.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("failed to write formatted bytes for {}", path.display()))?;
    file.as_file().set_permissions(permissions).with_context(|| {
        format!("failed to carry the mode of {} onto its replacement", path.display())
    })?;
    file.persist(path).map_err(|error| eyre!("failed to replace {}: {error}", path.display()))?;
    Ok(())
}

/// Staged entries as `path -> index mode`, e.g. `100644`, `100755`, `120000`.
///
/// `git ls-files -s -z` emits `<mode> <object> <stage>\t<path>\0`, so the mode
/// is the leading field and the path is everything past the first tab —
/// NUL-terminated, and therefore safe for paths containing newlines.
fn staged_index_modes() -> Result<HashMap<PathBuf, String>> {
    let out = cmd("git", ["-c", "core.quotePath=false", "ls-files", "-s", "-z"])
        .stdout_capture()
        .unchecked()
        .run()
        .context("failed to run git ls-files -s -z")?;
    if !out.status.success() {
        return Err(eyre!("git ls-files -s -z failed"));
    }

    let mut modes = HashMap::new();
    for record in out.stdout.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            continue;
        };
        let (meta, path) = record.split_at(tab);
        let Some(mode) = String::from_utf8_lossy(meta).split_whitespace().next().map(String::from)
        else {
            continue;
        };
        modes.insert(bytes_to_path(&path[1..])?, mode);
    }
    Ok(modes)
}

fn git_add_paths(paths: &[&Path]) -> Result<()> {
    let mut args: Vec<&std::ffi::OsStr> = vec!["add".as_ref(), "--".as_ref()];
    args.extend(paths.iter().map(|path| path.as_os_str()));
    cmd("git", &args).run().context("git add failed")?;
    Ok(())
}

/// Formats `text` with rustfmt and returns the result, writing no files.
///
/// Content goes in over **stdin**, never as a file path, and that is a safety
/// property rather than a convenience: given a path, rustfmt resolves the
/// file's out-of-line `mod child;` declarations and rewrites those children
/// too. Verified — `rustfmt --edition 2024 src/lib.rs` on a crate whose
/// `lib.rs` declares `mod child;` rewrites `src/child.rs`; the same content
/// piped over stdin leaves it byte-identical.
///
/// The check half of this same gate pipes stdin for this reason already, so
/// apply and check now agree on both mechanism and bytes — see
/// `commit_checks::rustfmt_would_reformat`.
///
/// stdin mode has no file location to search upward from, so `--config-path`
/// must be supplied explicitly; `config_text` carries the **staged**
/// `rustfmt.toml`. `None` means the tree has no config and rustfmt's defaults
/// apply.
///
/// Equivalence to `cargo fmt` was measured, not assumed: over 80 gate-clean
/// files this path reproduced each file byte-for-byte in all 80 cases, whereas
/// bare `rustfmt` (edition 2015 by default) rewrote 29 of them.
fn rustfmt_text(
    config_text: Option<&str>,
    edition: &str,
    text: &str,
    path_for_errors: &str,
) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut command = Command::new("rustfmt");
    command.args(["--edition", edition, "--emit", "stdout", "--quiet"]);

    // Keep the temp file alive until rustfmt exits, or --config-path dangles.
    let _config_guard = match config_text {
        Some(content) => {
            let mut file = tempfile::NamedTempFile::new()
                .context("failed to create a temp file for the staged rustfmt.toml")?;
            file.write_all(content.as_bytes())
                .context("failed to write the staged rustfmt.toml to a temp file")?;
            command.arg("--config-path").arg(file.path());
            Some(file)
        }
        None => None,
    };

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn rustfmt")?;
    child
        .stdin
        .take()
        .ok_or_else(|| eyre!("rustfmt stdin was not piped"))?
        .write_all(text.as_bytes())
        .context("failed to write staged content to rustfmt stdin")?;
    let output = child.wait_with_output().context("failed to wait for rustfmt")?;

    // A syntax error in the staged content, or a bad config, must surface as a
    // real error rather than silently yielding empty or partial output.
    if !output.status.success() || !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("rustfmt failed on staged {path_for_errors}: {stderr}"));
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("rustfmt returned non-UTF-8 output for {path_for_errors}"))
}

/// The `rustfmt.toml` content as staged, or `None` when the index has none.
///
/// Deliberately the index copy, not the working tree's. The check half reads
/// the staged config; if the apply half read the worktree copy instead, a
/// staged `rustfmt.toml` policy change with an unrelated unstaged edit on top
/// would have the author's staged Rust rewritten under settings that are not
/// the ones being committed — and the gate would then reject the very index
/// this step produced.
fn staged_rustfmt_config() -> Result<Option<String>> {
    let out = cmd("git", ["show", ":rustfmt.toml"])
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .context("failed to read the staged rustfmt.toml")?;
    if !out.status.success() {
        // Not in the index at all — rustfmt defaults apply, matching the
        // check half's `config_text: None` fallback.
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// The git top level, which is what `git diff --name-only` paths are relative to.
///
/// Propagates rather than falling back: without a usable root every package
/// directory stays absolute, no staged path matches any package, and staged
/// formatting silently becomes a no-op.
fn repo_root() -> Result<PathBuf> {
    // Not `git_paths`: `git rev-parse` has no `-z`, and passing one makes it
    // echo "-z" back as a second output line (verified). Its output is a single
    // path terminated by exactly one newline, so strip that and keep the rest of
    // the bytes verbatim — a repository path may itself contain a newline.
    let out = cmd("git", ["rev-parse", "--show-toplevel"])
        .stdout_capture()
        .unchecked()
        .run()
        .context("failed to run git rev-parse --show-toplevel")?;
    if !out.status.success() {
        return Err(eyre!("git rev-parse --show-toplevel failed"));
    }
    let mut bytes = out.stdout;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(eyre!("git rev-parse --show-toplevel returned no path"));
    }
    bytes_to_path(&bytes)
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

    fn unstaged(paths: &[&str]) -> HashSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    fn staged(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn staged_only_rust_files_are_formatted_and_restaged() {
        let paths = staged(&["crates/a/src/lib.rs", "docs/readme.md"]);
        let actions = classify_staged_paths(&paths, &unstaged(&[]));
        assert_eq!(
            actions,
            vec![StagedFormatAction::FormatAndRestage(PathBuf::from("crates/a/src/lib.rs"))],
            "non-Rust staged paths must not be handed to rustfmt"
        );
    }

    #[test]
    fn a_partially_staged_file_is_never_rewritten() {
        // The footgun this guards: formatting the worktree copy would rewrite
        // the author's unstaged work, and the follow-up `git add` would sweep
        // it into the commit. Skipping is the only safe action.
        let paths = staged(&["crates/a/src/lib.rs"]);
        let actions = classify_staged_paths(&paths, &unstaged(&["crates/a/src/lib.rs"]));
        assert_eq!(
            actions,
            vec![StagedFormatAction::SkipPartiallyStaged(PathBuf::from("crates/a/src/lib.rs"))],
        );
    }

    #[test]
    fn unrelated_unstaged_files_do_not_block_a_fully_staged_one() {
        // Only an overlap between the staged and unstaged sets is dangerous.
        let paths = staged(&["crates/a/src/lib.rs"]);
        let actions = classify_staged_paths(&paths, &unstaged(&["crates/b/src/other.rs"]));
        assert_eq!(
            actions,
            vec![StagedFormatAction::FormatAndRestage(PathBuf::from("crates/a/src/lib.rs"))],
        );
    }

    fn package(name: &str, dir: &str, edition: &str) -> WorkspacePackage {
        WorkspacePackage {
            name: name.to_string(),
            dir: PathBuf::from(dir),
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
        super::owning_package(Path::new(path), packages).map(|package| package.name.clone())
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
        let owner = super::owning_package(Path::new("crates/legacy/src/lib.rs"), &packages)
            .expect("legacy path must resolve to its package");
        assert_eq!(owner.edition, "2021", "each file must be formatted at its own package edition");
    }

    #[test]
    fn nothing_staged_yields_no_actions() {
        assert!(classify_staged_paths(&[], &unstaged(&["crates/a/src/lib.rs"])).is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn writing_a_file_preserves_its_mode_including_the_executable_bit() -> Result<()> {
        // `persist` renames, so the file the author keeps is the *temp* file,
        // and NamedTempFile creates at 0600. Measured before this was fixed:
        // 644 -> 600, and an executable 755/100755 -> 600/100644, which `git
        // add` recorded as a real index-mode change in the commit.
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir()?;
        for mode in [0o644, 0o755] {
            let path = dir.path().join(format!("probe{mode:o}.rs"));
            std::fs::write(&path, "fn a() {}\n")?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;

            super::write_file_atomically(&path, "fn b() {}\n")?;

            let after = std::fs::metadata(&path)?.permissions().mode() & 0o777;
            assert_eq!(after, mode, "mode {mode:o} must survive the atomic replace, got {after:o}");
            assert_eq!(std::fs::read_to_string(&path)?, "fn b() {}\n");
        }
        Ok(())
    }

    #[test]
    fn a_newline_in_a_filename_stays_one_path() -> Result<()> {
        // A Unix filename may legally contain a newline. Splitting git output on
        // lines would turn this single staged file into two paths that do not
        // exist, and the run would fail trying to read them. `-z` plus NUL
        // splitting is what makes that impossible.
        let stdout = b"crates/a/src/we\nird.rs\0crates/a/src/lib.rs\0";
        let paths = super::split_nul_paths(stdout)?;
        assert_eq!(
            paths,
            vec![PathBuf::from("crates/a/src/we\nird.rs"), PathBuf::from("crates/a/src/lib.rs"),],
            "a newline inside a filename must not split it into two paths"
        );

        // ...and such a file is still classified normally.
        let actions = classify_staged_paths(&paths, &unstaged(&[]));
        assert_eq!(actions.len(), 2, "both .rs paths must be classified");
        Ok(())
    }

    use std::path::{Path, PathBuf};

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
    fn staged_formatting_never_delegates_to_the_package_wide_formatter() -> Result<()> {
        // The safety property, guarded at the source because the hazard is in
        // *which process gets spawned*, not in any value this module returns.
        //
        // `cargo fmt -p <package>` formats every file in the package against
        // the live worktree. With a staged file and a separately modified
        // sibling in the same package, it rewrites the sibling's uncommitted
        // work; re-staging only the staged paths keeps the commit narrow but
        // cannot undo that. So `run_staged` must format the staged paths
        // themselves, at their package's edition.
        //
        // End-to-end verification of both halves of that claim (sibling bytes
        // preserved, staged file formatted in the index) is recorded on the PR;
        // this test keeps the implementation from quietly reverting to the
        // package-wide call.
        let fmt_source = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tasks").join("fmt.rs"),
        )?;
        let body = fmt_source
            .split_once("pub fn run_staged()")
            .map(|(_, rest)| rest)
            .and_then(|rest| rest.split_once("\npub fn run("))
            .map(|(body, _)| body)
            .ok_or_else(|| color_eyre::eyre::eyre!("could not isolate run_staged body"))?;

        assert!(
            !body.contains("run(false"),
            "run_staged must not call the package-wide formatter: it would rewrite unstaged \
             siblings in the same package"
        );
        // The internal call is not the only spelling of the hazard: a direct
        // `cmd("cargo", &["fmt", "-p", name])` or `--manifest-path` spawn
        // reintroduces it exactly.
        assert!(
            !body.contains("\"fmt\""),
            "run_staged must not spawn `cargo fmt` in any form: every package- or \
             manifest-scoped invocation rewrites unstaged siblings"
        );
        // Path-mode rustfmt resolves `mod child;` and rewrites unstaged child
        // modules, bypassing classify_staged_paths entirely.
        assert!(
            body.contains("rustfmt_text("),
            "run_staged must format through rustfmt_text (stdin), never by handing rustfmt a \
             file path: path mode traverses into unstaged child modules"
        );
        assert!(
            body.contains("package.edition"),
            "run_staged must pass each file's own package edition through to rustfmt"
        );

        // The edition and stdin contracts live in rustfmt_text.
        let formatter = fmt_source
            .split_once("fn rustfmt_text(")
            .map(|(_, rest)| rest)
            .and_then(|rest| rest.split_once("\n/// The `rustfmt.toml` content"))
            .map(|(body, _)| body)
            .ok_or_else(|| color_eyre::eyre::eyre!("could not isolate rustfmt_text body"))?;
        assert!(
            formatter.contains("\"--edition\""),
            "rustfmt_text must pass --edition; bare rustfmt defaults to edition 2015 and \
             reformats gate-clean edition-2024 files"
        );
        assert!(
            formatter.contains("Stdio::piped()"),
            "rustfmt_text must pipe content over stdin so rustfmt has no path to resolve \
             out-of-line child modules from"
        );
        Ok(())
    }

    fn formatted(name: &str) -> super::FormattedFile {
        super::FormattedFile {
            path: PathBuf::from(name),
            original: format!("original {name}"),
            formatted: format!("formatted {name}"),
        }
    }

    /// Records every write so a test can assert the final on-"disk" state.
    #[derive(Default)]
    struct FakeFs {
        writes: Vec<(String, String)>,
    }

    impl FakeFs {
        fn state(&self) -> std::collections::BTreeMap<String, String> {
            self.writes.iter().cloned().collect()
        }
    }

    #[test]
    fn a_successful_commit_writes_every_file_and_stages_exactly_those_paths() -> Result<()> {
        let files = vec![formatted("a.rs"), formatted("b.rs")];
        let mut fs = FakeFs::default();
        let mut staged: Vec<String> = Vec::new();

        super::commit_formatted(
            &files,
            &mut |path, text| {
                fs.writes.push((path.display().to_string(), text.to_string()));
                Ok(())
            },
            &mut |paths| {
                staged = paths.iter().map(|path| path.display().to_string()).collect();
                Ok(())
            },
        )?;

        assert_eq!(
            fs.state(),
            [
                ("a.rs".to_string(), "formatted a.rs".to_string()),
                ("b.rs".to_string(), "formatted b.rs".to_string()),
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(staged, vec!["a.rs".to_string(), "b.rs".to_string()]);
        Ok(())
    }

    #[test]
    fn a_failed_write_restores_the_files_already_written_and_stages_nothing() {
        // The divergence this prevents: `a.rs` rewritten in the worktree while
        // the index still holds the old bytes. The next run would classify it
        // as partially staged and skip it, so formatting silently stops.
        let files = vec![formatted("a.rs"), formatted("b.rs")];
        let mut fs = FakeFs::default();
        let mut stage_called = false;

        let error = super::commit_formatted(
            &files,
            &mut |path, text| {
                if path.display().to_string() == "b.rs" && text.starts_with("formatted") {
                    return Err(color_eyre::eyre::eyre!("disk full"));
                }
                fs.writes.push((path.display().to_string(), text.to_string()));
                Ok(())
            },
            &mut |_| {
                stage_called = true;
                Ok(())
            },
        )
        .expect_err("a failed write must not report success");

        assert!(!stage_called, "nothing may be staged when a write failed");
        assert_eq!(
            fs.state().get("a.rs").map(String::as_str),
            Some("original a.rs"),
            "the already-written file must be restored"
        );
        let rendered = format!("{error:?}");
        assert!(rendered.contains("disk full"), "the cause must survive: {rendered}");
        assert!(rendered.contains("worktree was restored"), "{rendered}");
    }

    #[test]
    fn a_failed_stage_restores_every_written_file() {
        // All writes succeed, `git add` fails: without rollback the whole set
        // is left rewritten and unstaged.
        let files = vec![formatted("a.rs"), formatted("b.rs")];
        let mut fs = FakeFs::default();

        let error = super::commit_formatted(
            &files,
            &mut |path, text| {
                fs.writes.push((path.display().to_string(), text.to_string()));
                Ok(())
            },
            &mut |_| Err(color_eyre::eyre::eyre!("index.lock exists")),
        )
        .expect_err("a failed stage must not report success");

        let state = fs.state();
        assert_eq!(state.get("a.rs").map(String::as_str), Some("original a.rs"));
        assert_eq!(state.get("b.rs").map(String::as_str), Some("original b.rs"));
        let rendered = format!("{error:?}");
        assert!(rendered.contains("index.lock exists"), "{rendered}");
        assert!(rendered.contains("worktree was restored"), "{rendered}");
    }

    #[test]
    fn a_failed_rollback_names_the_files_left_modified() {
        // Rollback is best-effort. When it cannot restore a file, the error
        // must say so by name rather than claiming a clean worktree — the
        // author needs to know exactly what to `git checkout --`.
        let files = vec![formatted("a.rs")];

        let error = super::commit_formatted(
            &files,
            &mut |_, text| {
                // The formatted write succeeds; the restore write fails.
                if text.starts_with("original") {
                    return Err(color_eyre::eyre::eyre!("read-only filesystem"));
                }
                Ok(())
            },
            &mut |_| Err(color_eyre::eyre::eyre!("index.lock exists")),
        )
        .expect_err("a failed stage must not report success");

        let rendered = format!("{error:?}");
        assert!(rendered.contains("rollback also failed"), "{rendered}");
        assert!(rendered.contains("a.rs"), "the unrestored file must be named: {rendered}");
        assert!(
            !rendered.contains("worktree was restored"),
            "must not claim a clean worktree: {rendered}"
        );
    }

    #[test]
    fn package_dirs_are_relative_to_the_git_root_not_the_working_directory() {
        // `git diff --name-only` reports repository-relative paths wherever it
        // is invoked from. Passing a root that is NOT the working directory
        // pins the git-root anchor: under the previous `current_dir()`
        // behaviour these could not come out repository-relative.
        let metadata = sample_metadata();
        let packages = super::workspace_packages(&metadata, Path::new("/repo"));

        let mut dirs: Vec<(&str, PathBuf)> =
            packages.iter().map(|package| (package.name.as_str(), package.dir.clone())).collect();
        dirs.sort_unstable();
        assert_eq!(
            dirs,
            vec![
                ("perl-parser", PathBuf::from("crates/perl-parser")),
                ("xtask", PathBuf::from("xtask")),
            ]
        );
        assert_eq!(
            super::owning_package(Path::new("crates/perl-parser/src/lib.rs"), &packages)
                .map(|package| package.name.as_str()),
            Some("perl-parser"),
        );
    }

    #[test]
    fn a_package_outside_the_repository_root_owns_nothing() {
        // strip_prefix fails, the dir stays absolute, and an absolute dir can
        // never prefix-match a repository-relative git path. Those files fall
        // through to the gate rather than being misattributed.
        let metadata = sample_metadata();
        let packages = super::workspace_packages(&metadata, Path::new("/somewhere/else"));
        assert!(
            packages.iter().all(|package| package.dir.is_absolute()),
            "packages outside the given root must keep absolute dirs: {packages:?}"
        );
        assert_eq!(
            super::owning_package(Path::new("crates/perl-parser/src/lib.rs"), &packages),
            None
        );
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
