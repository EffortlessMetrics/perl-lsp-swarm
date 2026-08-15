//! Guard user-facing install commands against release-surface drift.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

const REQUIRED_HOMEBREW_COMMAND: &str = "brew install effortlessmetrics/tap/perllsp";
const REQUIRED_TAP_COMMAND: &str = "brew tap effortlessmetrics/tap";
const UNQUALIFIED_HOMEBREW_COMMAND: &str = "brew install perllsp";
const CARGO_INSTALL_COMMAND: &str = "cargo install perllsp";
const RELEASE_CHOOSER_HEADING: &str = "Which file should I download?";

/// Surfaces where a reader is choosing how to install, and therefore where the
/// crates.io name collision has to be stated next to the command itself.
///
/// Deliberately not every file that mentions the command. Release notes, closeout
/// audits, competitive analyses, publishing roadmaps, installer scripts, and extension
/// source all reference `cargo install perllsp` while documenting or performing
/// something other than an install decision; requiring a conflict warning beside each of
/// those 39 mentions would add noise to shipped history without protecting any decision.
const INSTALL_DECISION_SURFACES: &[&str] = &[
    "README.md",
    "book/src/getting-started/installation.md",
    "book/src/quick-start.md",
    "docs/how-to/INSTALLATION.md",
    "docs/reference/product-identity.md",
    "docs/EDITORS/",
];

const SCAN_ROOTS: &[&str] = &[
    "README.md",
    "CHANGELOG.md",
    "RELEASE_HISTORY.md",
    "book/src",
    "docs",
    "vscode-extension",
    "crates/perl-lsp-rs-core/src/runtime/launcher/mod.rs",
    "install.ps1",
    "install.sh",
    "scripts",
];

const FORBIDDEN_PATTERNS: &[(&str, &str)] = &[
    ("brew install perl-lsp", "retired Homebrew formula name"),
    ("brew tap effortlesssteven/tap", "retired Homebrew tap"),
    ("brew tap tree-sitter-perl/tap", "retired Homebrew tap"),
    ("cargo install perl-lsp-rs", "implementation crate used as install package"),
    ("cargo install perl-lsp", "different crates.io project used as install package"),
    ("perl-lsp --stdio", "product name used as executable"),
    ("perl-lsp --version", "product name used as executable"),
    ("perl-lsp --health", "product name used as executable"),
    ("perl-lsp-rs --stdio", "implementation crate used as executable"),
    ("perl-lsp-rs --version", "implementation crate used as executable"),
    ("install.ps1 | iex", "install.ps1 published at perl-lsp/master 404s; see #5461/#4348"),
];

/// Identities that are never executable names but *are* legitimate arguments to other
/// commands, such as `code --install-extension <id> --extensions-dir ...` and
/// `npx @vscode/vsce show <id> --json`.
///
/// These are checked in command position only. A plain substring rule would report
/// every valid invocation that merely passes the identity to another tool, which makes
/// the whole check fail on documentation that is already correct.
const FORBIDDEN_EXECUTABLES: &[(&str, &str)] =
    &[("EffortlessMetrics.perl-lsp-rs", "extension id used as executable")];

/// Shell separators after which a new command begins.
const COMMAND_SEPARATORS: &[&str] = &["|", "&&", "||", ";"];

const REQUIRED_PATTERNS: &[(&str, &str)] = &[
    (REQUIRED_HOMEBREW_COMMAND, "owned Homebrew tap install command"),
    ("cargo install perllsp --locked", "canonical Cargo package command"),
    ("perllsp --stdio", "canonical LSP server command"),
    ("perllsp --version", "canonical version check"),
    ("perllsp --health", "canonical health check"),
    ("perllsp --identity-json", "canonical support identity command"),
    ("perl-lsp.linuxLibc", "VS Code Linux libc selector setting"),
];

#[derive(Debug)]
struct SourceFile {
    rel_path: PathBuf,
    text: String,
}

#[derive(Debug)]
struct Violation {
    location: String,
    message: String,
}

pub fn run() -> Result<()> {
    let root = project_root()?;
    let files = collect_source_files(&root)?;
    let mut violations = Vec::new();

    check_forbidden_patterns(&files, &mut violations);
    check_forbidden_executables(&files, &mut violations);
    check_required_patterns(&files, &mut violations);
    check_unqualified_homebrew(&files, &mut violations);
    check_cargo_conflict_warning(&files, &mut violations);
    check_release_note_choosers(&files, &mut violations);

    if violations.is_empty() {
        println!("{}", success_message(files.len()));
        return Ok(());
    }

    eprintln!("INSTALL SURFACE VIOLATIONS:");
    eprintln!("{}", "=".repeat(60));
    for violation in &violations {
        eprintln!("  {}: {}", violation.location, violation.message);
    }
    eprintln!("{}", "=".repeat(60));
    bail!("install surface check failed with {} violation(s)", violations.len())
}

fn collect_source_files(root: &Path) -> Result<Vec<SourceFile>> {
    let mut files = Vec::new();

    for scan_root in SCAN_ROOTS {
        let path = root.join(scan_root);
        if path.is_file() {
            push_file(root, &path, &mut files)?;
            continue;
        }

        if !path.is_dir() {
            continue;
        }

        for entry in WalkDir::new(&path).into_iter().filter_entry(|entry| {
            root_relative(root, entry.path()).map(|rel| !is_excluded_path(rel)).unwrap_or(true)
        }) {
            let entry = entry.with_context(|| format!("failed to walk {}", path.display()))?;
            let entry_path = entry.path();
            if entry_path.is_file() && is_scan_candidate(entry_path) {
                push_file(root, entry_path, &mut files)?;
            }
        }
    }

    files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    Ok(files)
}

fn push_file(root: &Path, path: &Path, files: &mut Vec<SourceFile>) -> Result<()> {
    let rel_path =
        root_relative(root, path).map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf());

    if is_excluded_path(&rel_path) || !is_scan_candidate(path) {
        return Ok(());
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read install-surface file {}", path.display()))?;
    files.push(SourceFile { rel_path, text });
    Ok(())
}

fn root_relative<'a>(root: &Path, path: &'a Path) -> Option<&'a Path> {
    path.strip_prefix(root).ok()
}

fn is_scan_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("md" | "json" | "rs" | "sh" | "ps1" | "ts" | "js" | "yml" | "yaml" | "toml")
    )
}

fn is_excluded_path(rel_path: &Path) -> bool {
    rel_path.starts_with(".git")
        || rel_path.starts_with("target")
        || rel_path.starts_with("book/book")
        || rel_path.starts_with("docs/reference/archive")
        || rel_path.starts_with("docs/issues")
        || rel_path.starts_with("vscode-extension/node_modules")
        || rel_path.starts_with("vscode-extension/out")
        || rel_path.starts_with("vscode-extension/dist")
        || is_historical_release_runbook(rel_path)
}

/// Version-pinned per-release runbooks (`RELEASE_RUNBOOK_0_12_3.md`) document a release
/// that already shipped. They must keep naming the executables and image tags that
/// release actually produced -- `docker run ... perl-lsp:0.12.3 perl-lsp-rs --version`
/// was correct for 0.12.3 -- so rewriting them to current identity would make the
/// historical record false. Un-pinned runbooks such as `GA_RUNBOOK.md` stay in scope.
fn is_historical_release_runbook(rel_path: &Path) -> bool {
    rel_path
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with("RELEASE_RUNBOOK_") && name.ends_with(".md"))
}

fn check_forbidden_patterns(files: &[SourceFile], violations: &mut Vec<Violation>) {
    for file in files {
        for (line_no, line) in file.text.lines().enumerate() {
            for &(pattern, reason) in FORBIDDEN_PATTERNS {
                if line.contains(pattern) {
                    violations.push(line_violation(
                        file,
                        line_no + 1,
                        format!("found {reason}: `{pattern}`"),
                    ));
                }
            }
        }
    }
}

fn check_forbidden_executables(files: &[SourceFile], violations: &mut Vec<Violation>) {
    for file in files {
        for (line_no, line) in file.text.lines().enumerate() {
            for &(identity, reason) in FORBIDDEN_EXECUTABLES {
                if line_invokes(line, identity) {
                    violations.push(line_violation(
                        file,
                        line_no + 1,
                        format!("found {reason}: `{identity}`"),
                    ));
                }
            }
        }
    }
}

/// True when `identity` is executed in some segment of `line`.
///
/// Two conditions must both hold, because either alone produces false positives on
/// documentation that is already correct:
///
/// 1. the identity is the command of the segment, not an argument -- otherwise
///    `code --install-extension <id> --extensions-dir ...` reports a violation;
/// 2. it is followed by an option -- otherwise a bare marketplace ID in a `text` block
///    or an inline-code mention that happens to open a wrapped prose line reports a
///    violation.
fn line_invokes(line: &str, identity: &str) -> bool {
    split_command_segments(line).into_iter().any(|segment| {
        let tokens = command_tokens(segment);
        tokens.first().is_some_and(|command| *command == identity)
            && tokens.get(1).is_some_and(|argument| argument.starts_with('-'))
    })
}

fn split_command_segments(line: &str) -> Vec<&str> {
    let mut segments = vec![line];
    for separator in COMMAND_SEPARATORS {
        segments = segments.iter().flat_map(|part| part.split(*separator)).collect();
    }
    segments
}

/// Tokens of a command segment with documentation decoration removed: leading
/// whitespace, Markdown list bullets and checkboxes, inline backticks, and shell
/// prompts. A leading `#` is deliberately not stripped so that comments and Markdown
/// headings do not present their first word as a command.
fn command_tokens(segment: &str) -> Vec<&str> {
    let mut text = segment.trim();
    loop {
        let stripped = text.trim_start_matches(['`', '$', '>', '*', '-', ' ', '\t']).trim_start();
        let stripped = stripped.strip_prefix("[ ]").unwrap_or(stripped).trim_start();
        let stripped = stripped.strip_prefix("[x]").unwrap_or(stripped).trim_start();
        let stripped = stripped.strip_prefix("PS").unwrap_or(stripped).trim_start();
        if stripped == text {
            break;
        }
        text = stripped;
    }
    text.split_whitespace().map(|token| token.trim_matches('`')).filter(|t| !t.is_empty()).collect()
}

fn check_required_patterns(files: &[SourceFile], violations: &mut Vec<Violation>) {
    for &(pattern, description) in REQUIRED_PATTERNS {
        if !files.iter().any(|file| file.text.contains(pattern)) {
            violations.push(global_violation(format!("missing {description}: `{pattern}`")));
        }
    }
}

fn check_unqualified_homebrew(files: &[SourceFile], violations: &mut Vec<Violation>) {
    for file in files {
        let lines: Vec<&str> = file.text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains(UNQUALIFIED_HOMEBREW_COMMAND) {
                continue;
            }

            if has_nearby_tap_command(&lines, index) {
                continue;
            }

            violations.push(line_violation(
                file,
                index + 1,
                format!(
                    "unqualified `{UNQUALIFIED_HOMEBREW_COMMAND}` must be paired with `{REQUIRED_TAP_COMMAND}` in the same install flow"
                ),
            ));
        }
    }
}

fn has_nearby_tap_command(lines: &[&str], install_index: usize) -> bool {
    let start = install_index.saturating_sub(3);
    lines[start..install_index].iter().any(|line| line.contains(REQUIRED_TAP_COMMAND))
}

/// True when this path is a surface a reader uses to decide how to install.
fn is_install_decision_surface(rel_path: &Path) -> bool {
    let path = rel_path.to_string_lossy().replace('\\', "/");
    INSTALL_DECISION_SURFACES.iter().any(|surface| {
        if let Some(dir) = surface.strip_suffix('/') {
            path.starts_with(dir)
        } else {
            path == *surface
        }
    })
}

fn check_cargo_conflict_warning(files: &[SourceFile], violations: &mut Vec<Violation>) {
    for file in files {
        if !is_install_decision_surface(&file.rel_path) {
            continue;
        }
        let lines: Vec<&str> = file.text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.contains(CARGO_INSTALL_COMMAND) {
                continue;
            }
            if has_nearby_cargo_conflict_warning(&lines, index) {
                continue;
            }
            violations.push(line_violation(
                file,
                index + 1,
                concat!(
                    "`cargo install perllsp` must have a nearby warning that crates.io `perl-lsp` ",
                    "is a different project"
                )
                .to_string(),
            ));
        }
    }
}

fn has_nearby_cargo_conflict_warning(lines: &[&str], install_index: usize) -> bool {
    let start = install_index.saturating_sub(3);
    let end = (install_index + 4).min(lines.len());
    let context = lines[start..end].join("\n").to_ascii_lowercase();
    context.contains("perl-lsp") && context.contains("different project")
}

fn check_release_note_choosers(files: &[SourceFile], violations: &mut Vec<Violation>) {
    for file in files {
        if !requires_release_chooser(&file.rel_path, &file.text) {
            continue;
        }

        if file.text.contains(RELEASE_CHOOSER_HEADING) {
            continue;
        }

        violations.push(Violation {
            location: file.rel_path.display().to_string(),
            message: format!(
                "release notes with GNU/musl Linux assets must include `{RELEASE_CHOOSER_HEADING}`"
            ),
        });
    }
}

fn requires_release_chooser(rel_path: &Path, text: &str) -> bool {
    if !rel_path.starts_with("docs/releases") {
        return false;
    }

    if !text.contains("unknown-linux-gnu") && !text.contains("unknown-linux-musl") {
        return false;
    }

    release_version(rel_path).is_some_and(|version| version >= (0, 12, 4))
}

fn release_version(rel_path: &Path) -> Option<(u64, u64, u64)> {
    let file_stem = rel_path.file_stem()?.to_str()?;
    let version = file_stem.strip_prefix('v')?;
    let numeric = version.split_once('-').map(|(base, _suffix)| base).unwrap_or(version);
    let mut parts = numeric.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn line_violation(file: &SourceFile, line_no: usize, message: String) -> Violation {
    Violation { location: format!("{}:{line_no}", file.rel_path.display()), message }
}

fn global_violation(message: String) -> Violation {
    Violation { location: "install surface".to_string(), message }
}

fn success_message(files_count: usize) -> String {
    format!(
        "Install surface check passed: {files_count} active files scanned, {forbidden} \
         forbidden patterns and {required} required patterns checked. Scope: only the \
         hardcoded literals in FORBIDDEN_PATTERNS and REQUIRED_PATTERNS are checked; new \
         install-surface drift is NOT caught.",
        forbidden = FORBIDDEN_PATTERNS.len(),
        required = REQUIRED_PATTERNS.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::Result;

    #[test]
    fn unqualified_brew_install_requires_nearby_tap() -> Result<()> {
        let allowed = vec!["brew tap effortlessmetrics/tap", "brew install perllsp"];
        let rejected = vec!["brew update", "brew install perllsp"];

        assert!(has_nearby_tap_command(&allowed, 1));
        assert!(!has_nearby_tap_command(&rejected, 1));
        Ok(())
    }

    #[test]
    fn cargo_install_warning_must_be_in_the_same_local_flow() {
        let allowed = vec![
            "The crates.io package `perl-lsp` is a different project.",
            "cargo install perllsp --locked",
        ];
        let rejected = vec![
            "cargo install perllsp --locked",
            "ordinary installation prose",
            "more prose",
            "more prose",
            "the crates.io package perl-lsp is a different project",
        ];
        assert!(has_nearby_cargo_conflict_warning(&allowed, 1));
        assert!(!has_nearby_cargo_conflict_warning(&rejected, 0));
    }

    #[test]
    fn global_conflict_phrase_does_not_satisfy_a_distant_install_command() {
        let file = SourceFile {
            rel_path: PathBuf::from("README.md"),
            text: [
                "cargo install perllsp --locked",
                "line 2",
                "line 3",
                "line 4",
                "line 5",
                "The crates.io package perl-lsp is a different project.",
            ]
            .join("\n"),
        };
        let mut violations = Vec::new();
        check_cargo_conflict_warning(&[file], &mut violations);
        assert_eq!(violations.len(), 1, "got: {violations:?}");
    }

    #[test]
    fn release_notes_need_chooser_from_v0_12_4_forward() -> Result<()> {
        let old_release = Path::new("docs/releases/v0.12.3.md");
        let current_release = Path::new("docs/releases/v0.12.4.md");
        let text = "perllsp-0.12.4-x86_64-unknown-linux-gnu.tar.gz";

        assert!(!requires_release_chooser(old_release, text));
        assert!(requires_release_chooser(current_release, text));
        Ok(())
    }

    #[test]
    fn rc_release_version_uses_base_semver() -> Result<()> {
        let version = release_version(Path::new("docs/releases/v0.13.0-rc1.md"));

        assert_eq!(version, Some((0, 13, 0)));
        Ok(())
    }

    #[test]
    fn success_message_states_hardcoded_literal_scope() {
        let msg = success_message(12);
        assert!(msg.contains("hardcoded literals"), "msg: {msg}");
        assert!(msg.contains("new install-surface drift is NOT caught"), "msg: {msg}");
        assert!(msg.contains("12 active files scanned"), "msg: {msg}");
        assert!(msg.contains("forbidden patterns"), "msg: {msg}");
        assert!(msg.contains("required patterns"), "msg: {msg}");
    }

    #[test]
    fn piped_install_ps1_is_forbidden_but_explanatory_prose_is_not() {
        let file = SourceFile {
            rel_path: PathBuf::from("docs/how-to/INSTALLATION.md"),
            text: [
                "irm https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.ps1 | iex",
                "The PowerShell installer script is **not usable yet**: `install.ps1` 404s.",
                "irm https://raw.githubusercontent.com/EffortlessMetrics/perl-lsp/master/install.ps1 -OutFile install.ps1",
                ".\\install.ps1 -Version 0.17.0 -InstallDir C:\\tools\\bin",
                "(`Unblock-File .\\install.ps1`) or run it in a session that allows local scripts.",
            ]
            .join("\n"),
        };

        let mut violations = Vec::new();
        check_forbidden_patterns(&[file], &mut violations);

        let piped: Vec<_> =
            violations.iter().filter(|v| v.message.contains("install.ps1 | iex")).collect();
        assert_eq!(piped.len(), 1, "got: {violations:?}");
        assert!(piped[0].location.ends_with(":1"));
    }

    #[test]
    fn wrong_package_executable_and_extension_commands_are_rejected() {
        let file = SourceFile {
            rel_path: PathBuf::from("README.md"),
            text: [
                "cargo install perl-lsp",
                "perl-lsp --stdio",
                "perl-lsp-rs --version",
                "EffortlessMetrics.perl-lsp-rs --stdio",
            ]
            .join("\n"),
        };
        let files = [file];
        let mut violations = Vec::new();
        check_forbidden_patterns(&files, &mut violations);
        assert_eq!(violations.len(), 4, "got: {violations:?}");

        // The extension line is independently reachable through the executable rule,
        // and the substring rule also catches it via `perl-lsp-rs --stdio`.
        let mut executable_violations = Vec::new();
        check_forbidden_executables(&files, &mut executable_violations);
        assert_eq!(executable_violations.len(), 1, "got: {executable_violations:?}");
        assert!(executable_violations[0].location.ends_with(":4"));
    }

    #[test]
    fn extension_id_passed_as_an_argument_is_not_an_invocation() {
        let file = SourceFile {
            rel_path: PathBuf::from("docs/RELEASE_PROCESS.md"),
            text: [
                "code --install-extension EffortlessMetrics.perl-lsp-rs --extensions-dir ~/.vscode-oss/extensions",
                "npx @vscode/vsce show EffortlessMetrics.perl-lsp-rs --json",
                "code --uninstall-extension EffortlessMetrics.perl-lsp-rs",
            ]
            .join("\n"),
        };

        let mut violations = Vec::new();
        check_forbidden_executables(&[file], &mut violations);
        assert!(violations.is_empty(), "argument use must not be a violation, got: {violations:?}");
    }

    #[test]
    fn extension_id_in_command_position_is_still_rejected() {
        for line in [
            "EffortlessMetrics.perl-lsp-rs --stdio",
            "  $ EffortlessMetrics.perl-lsp-rs --version",
            "- [ ] `EffortlessMetrics.perl-lsp-rs --health`",
            "cat log | EffortlessMetrics.perl-lsp-rs --stdio",
        ] {
            assert!(
                line_invokes(line, "EffortlessMetrics.perl-lsp-rs"),
                "must detect invocation in: {line}"
            );
        }

        // Every line below is real active documentation on this tree.
        for line in [
            "code --install-extension EffortlessMetrics.perl-lsp-rs --extensions-dir /tmp",
            "npx @vscode/vsce show EffortlessMetrics.perl-lsp-rs --json",
            "EffortlessMetrics.perl-lsp-rs",
            "`EffortlessMetrics.perl-lsp-rs` extension. For manual binary management, set:",
            "`EffortlessMetrics.perl-lsp-rs` and keep auto-download enabled unless you need",
            "# EffortlessMetrics.perl-lsp-rs",
            "The extension ID is EffortlessMetrics.perl-lsp-rs.",
        ] {
            assert!(!line_invokes(line, "EffortlessMetrics.perl-lsp-rs"), "false positive: {line}");
        }
    }

    #[test]
    fn conflict_warning_is_required_on_install_decision_surfaces_only() {
        for surface in [
            "README.md",
            "docs/how-to/INSTALLATION.md",
            "docs/reference/product-identity.md",
            "docs/EDITORS/NEOVIM_SETUP.md",
            "book/src/quick-start.md",
        ] {
            assert!(is_install_decision_surface(Path::new(surface)), "{surface}");
        }

        // These reference the command while documenting something other than a choice.
        for other in [
            "docs/releases/v0.17.0.md",
            "docs/articles/COMPETITIVE_ANALYSIS.md",
            "docs/project/RELEASE_CHECKLIST.md",
            "scripts/install.sh",
            "vscode-extension/src/extension.ts",
        ] {
            assert!(!is_install_decision_surface(Path::new(other)), "{other}");
        }
    }

    #[test]
    fn install_decision_surface_without_a_nearby_warning_is_rejected() {
        let file = SourceFile {
            rel_path: PathBuf::from("docs/EDITORS/NEOVIM_SETUP.md"),
            text: ["```bash", "cargo install perllsp", "```"].join("\n"),
        };
        let mut violations = Vec::new();
        check_cargo_conflict_warning(&[file], &mut violations);
        assert_eq!(violations.len(), 1, "got: {violations:?}");
    }

    #[test]
    fn version_pinned_release_runbooks_are_historical_evidence() {
        assert!(is_excluded_path(Path::new("docs/project/RELEASE_RUNBOOK_0_12_3.md")));
        assert!(!is_excluded_path(Path::new("docs/project/GA_RUNBOOK.md")));
    }

    /// The unit tests above only prove the rules behave on synthetic input. This is the
    /// oracle that fails when active documentation actually drifts, and the one that
    /// would have caught both the extension-id argument matches in
    /// `docs/RELEASE_PROCESS.md` and the version-pinned container commands in the 0.12.3
    /// runbook.
    #[test]
    fn live_install_surface_has_no_forbidden_commands() -> Result<()> {
        let root = project_root()?;
        let files = collect_source_files(&root)?;
        let mut violations = Vec::new();
        check_forbidden_patterns(&files, &mut violations);
        check_forbidden_executables(&files, &mut violations);
        check_cargo_conflict_warning(&files, &mut violations);
        check_unqualified_homebrew(&files, &mut violations);

        let reported: Vec<_> =
            violations.iter().map(|v| format!("{}: {}", v.location, v.message)).collect();
        assert!(
            reported.is_empty(),
            "active install surface has forbidden commands: {reported:#?}"
        );
        Ok(())
    }

    #[test]
    fn historical_identity_mentions_remain_outside_active_scan_scope() {
        assert!(is_excluded_path(Path::new(
            "docs/reference/archive/old-infrastructure/install.md"
        )));
    }

    #[test]
    fn installer_scripts_are_in_scan_scope() -> Result<()> {
        let root = project_root()?;
        let files = collect_source_files(&root)?;
        let scanned: Vec<_> = files.iter().map(|f| f.rel_path.as_path()).collect();

        for required in ["install.ps1", "install.sh", "scripts/install.sh"] {
            assert!(scanned.contains(&Path::new(required)), "{required} must be scanned");
        }
        Ok(())
    }

    #[test]
    fn live_install_surface_has_no_piped_install_ps1() -> Result<()> {
        let root = project_root()?;
        let files = collect_source_files(&root)?;
        let mut violations = Vec::new();
        check_forbidden_patterns(&files, &mut violations);

        let piped: Vec<_> = violations
            .iter()
            .filter(|v| v.message.contains("install.ps1 | iex"))
            .map(|v| v.location.clone())
            .collect();
        assert!(piped.is_empty(), "piped install.ps1 still documented at: {piped:?}");
        Ok(())
    }

    /// The canonical guide must not attribute an executable to policy that policy does
    /// not declare.
    ///
    /// A previous revision stated that the `perl-lsp-rs` crate "still builds a
    /// `perl-lsp` binary" recorded as `server.compatibility_executable`. Both claims
    /// were false once the library-only server landed: the policy key does not exist
    /// and no workspace crate declares a `perl-lsp` binary target. A guide that
    /// contradicts the policy file it cites is the exact failure this guide exists to
    /// prevent, so the two are pinned against each other.
    #[test]
    fn identity_guide_does_not_claim_executables_policy_does_not_declare() -> Result<()> {
        let root = project_root()?;
        let policy = fs::read_to_string(root.join("policy/product-identity.toml"))?;
        let guide = fs::read_to_string(root.join("docs/reference/product-identity.md"))?;

        assert_eq!(
            guide.contains("compatibility_executable"),
            policy.contains("compatibility_executable"),
            "the guide may name `compatibility_executable` only while \
             policy/product-identity.toml declares it"
        );
        Ok(())
    }

    #[test]
    fn forbidden_and_required_pattern_tables_are_non_empty() {
        assert!(!FORBIDDEN_PATTERNS.is_empty(), "FORBIDDEN_PATTERNS must stay populated");
        assert!(!REQUIRED_PATTERNS.is_empty(), "REQUIRED_PATTERNS must stay populated");
    }
}
