use color_eyre::eyre::{Context, Result, eyre};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

/// Inline link body and destination after a live `[`.
static LINK_BODY_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\[[^\]]*\]\(\s*<?([^\s)>]+)>?"));

/// Reference link `[text][label]` or collapsed `[label][]`.
static REFERENCE_LINK_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        \[
            (?P<text>[^\]]*)
        \]
        \[
            (?P<label>[^\]]*)
        \]
    ",
    )
});

/// Reference definition at line start: `[label]: target`.
static REFERENCE_DEF_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^\s{0,3}\[(?P<label>[^\]]+)\]:\s+<?(?P<target>[^>\s]+)>?"));

/// An inline code span. ADR prose demonstrates Markdown syntax inside
/// backticks; that text does not render as a link, so it must not be resolved
/// as one. Fenced blocks are handled separately by the fence toggle in
/// [`check_file`].
static INLINE_CODE_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"`[^`]*`"));

pub(crate) fn check_doc_links(repo_root: &Path, docs_dir: Option<&str>) -> Result<i32> {
    let docs_dir = docs_dir.unwrap_or("docs/adr");
    let docs_path = resolve_docs_path(repo_root, docs_dir);
    if !docs_path.is_dir() {
        return Err(eyre!("Docs directory not found: {}", docs_path.display()));
    }

    let mut failures = Vec::new();
    let canonical_root = fs::canonicalize(repo_root)
        .with_context(|| format!("failed to resolve repository root {}", repo_root.display()))?;

    for entry in WalkDir::new(&docs_path).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to walk {}", docs_path.display()))?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|ext| ext.to_str()) != Some("md")
        {
            continue;
        }
        failures.extend(check_file(repo_root, &canonical_root, entry.path())?);
    }

    if failures.is_empty() {
        println!("✅ No broken relative Markdown links found in {}", docs_dir);
        return Ok(0);
    }

    println!("❌ Broken relative Markdown links found in {docs_dir}");
    for failure in failures {
        println!("{failure}");
    }
    Ok(1)
}

fn check_file(repo_root: &Path, canonical_root: &Path, path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read documentation file {}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let definitions = collect_reference_definitions(&lines)?;
    let mut failures = Vec::new();
    let mut fenced = false;

    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }

        for target in markdown_inline_link_targets(line)? {
            failures.extend(validate_local_target(
                repo_root,
                canonical_root,
                path,
                line_index,
                &target,
                None,
            )?);
        }

        for reference_label in markdown_reference_link_labels(line)? {
            let normalized = normalize_reference_label(&reference_label);
            let Some(target) = definitions.get(&normalized) else {
                failures.push(format!(
                    "{}:{}: undefined reference [{}]",
                    display_path(repo_root, path),
                    line_index + 1,
                    reference_label
                ));
                continue;
            };
            failures.extend(validate_local_target(
                repo_root,
                canonical_root,
                path,
                line_index,
                target,
                Some(&reference_label),
            )?);
        }
    }

    Ok(failures)
}

fn collect_reference_definitions(lines: &[&str]) -> Result<HashMap<String, String>> {
    let reference_def = REFERENCE_DEF_RE
        .as_ref()
        .map_err(|err| eyre!("failed to compile reference definition matcher: {err}"))?;
    let mut definitions = HashMap::new();
    let mut fenced = false;

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }

        let masked = mask_inline_code(line)?;
        let Some(capture) = reference_def.captures(&masked) else {
            continue;
        };
        let label = capture.name("label").map(|m| m.as_str()).unwrap_or_default();
        let target = capture.name("target").map(|m| m.as_str()).unwrap_or_default();
        if label.is_empty() || target.is_empty() {
            continue;
        }
        definitions.entry(normalize_reference_label(label)).or_insert_with(|| target.to_owned());
    }

    Ok(definitions)
}

fn validate_local_target(
    repo_root: &Path,
    canonical_root: &Path,
    path: &Path,
    line_index: usize,
    target: &str,
    via_reference: Option<&str>,
) -> Result<Vec<String>> {
    let Some(target_path) = local_target_path(repo_root, path, target) else {
        return Ok(Vec::new());
    };

    let display = display_path(repo_root, path);
    let line = line_index + 1;
    let via = via_reference.map(|label| format!(" (via reference [{label}])")).unwrap_or_default();

    if !target_path.exists() {
        return Ok(vec![format!("{display}:{line}: missing target {target}{via}")]);
    }

    let canonical_target = fs::canonicalize(&target_path)
        .with_context(|| format!("failed to resolve link target {}", target_path.display()))?;
    if canonical_target.starts_with(canonical_root) {
        return Ok(Vec::new());
    }

    Ok(vec![format!("{display}:{line}: target escapes repository {target}{via}")])
}

fn markdown_inline_link_targets(line: &str) -> Result<Vec<String>> {
    let link_body = LINK_BODY_RE
        .as_ref()
        .map_err(|err| eyre!("failed to compile Markdown link matcher: {err}"))?;
    let line = mask_inline_code(line)?;
    let bytes = line.as_bytes();
    let mut targets = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && is_live_link_opener(bytes, i)
            && let Some(rest) = line.get(i..)
            && let Some(capture) = link_body.captures(rest)
        {
            if let Some(target) = capture.get(1) {
                targets.push(target.as_str().to_owned());
            }
            if let Some(full) = capture.get(0) {
                i += full.end();
                continue;
            }
        }
        i += 1;
    }
    Ok(targets)
}

fn markdown_reference_link_labels(line: &str) -> Result<Vec<String>> {
    let reference_link = REFERENCE_LINK_RE
        .as_ref()
        .map_err(|err| eyre!("failed to compile reference link matcher: {err}"))?;
    let line = mask_inline_code(line)?;
    let bytes = line.as_bytes();
    let mut labels = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && is_live_link_opener(bytes, i)
            && let Some(rest) = line.get(i..)
            && let Some(capture) = reference_link.captures(rest)
            && capture.get(0).is_some_and(|full| full.start() == 0)
        {
            let text = capture.name("text").map(|m| m.as_str()).unwrap_or_default();
            let label = capture.name("label").map(|m| m.as_str()).unwrap_or_default();
            if label.is_empty() {
                if !text.is_empty() {
                    labels.push(text.to_owned());
                }
            } else {
                labels.push(label.to_owned());
            }
            if let Some(full) = capture.get(0) {
                i += full.end();
                continue;
            }
        }
        i += 1;
    }
    Ok(labels)
}

fn mask_inline_code(line: &str) -> Result<String> {
    let inline_code = INLINE_CODE_RE
        .as_ref()
        .map_err(|err| eyre!("failed to compile inline code matcher: {err}"))?;
    // Blank out code spans rather than dropping them, so neighbouring text
    // cannot be spliced together into a link that was never written.
    Ok(inline_code.replace_all(line, " ").into_owned())
}

/// CommonMark reference labels are matched case-insensitively with collapsed whitespace.
fn normalize_reference_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
}

/// A `[` starts a live link when it is preceded by an even number of backslashes.
/// `\[escaped](x)` is literal text; `\\[live](x)` is a literal backslash plus link.
fn is_live_link_opener(bytes: &[u8], bracket_index: usize) -> bool {
    let mut backslashes = 0usize;
    let mut j = bracket_index;
    while j > 0 && bytes[j - 1] == b'\\' {
        backslashes += 1;
        j -= 1;
    }
    backslashes.is_multiple_of(2)
}

fn local_target_path(repo_root: &Path, source: &Path, target: &str) -> Option<PathBuf> {
    let target = target.split('#').next()?.split('?').next()?.trim();
    if target.is_empty() || is_external_uri_target(target) {
        return None;
    }

    Some(if target.starts_with('/') {
        repo_root.join(target.trim_start_matches('/'))
    } else {
        source.parent()?.join(target)
    })
}

/// URI schemes are case-insensitive (RFC 3986); do not treat them as repo-local paths.
fn is_external_uri_target(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("file:")
        || lower.starts_with("command:")
}

fn resolve_docs_path(repo_root: &Path, docs_dir: &str) -> PathBuf {
    if Path::new(docs_dir).is_absolute() {
        PathBuf::from(docs_dir)
    } else {
        repo_root.join(docs_dir)
    }
}

fn display_path(repo_root: &Path, path: &Path) -> String {
    let relative = match path.strip_prefix(repo_root) {
        Ok(relative) => relative,
        Err(_) => path,
    };
    relative.display().to_string().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{
        check_doc_links, check_file, collect_reference_definitions, markdown_inline_link_targets,
        markdown_reference_link_labels,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn unique_temp_dir(label: &str) -> TestResult<PathBuf> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let dir = std::env::temp_dir().join(format!("perl-ci-hygiene-doc-links-{label}-{nanos}"));
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn check_doc_links_rejects_missing_relative_targets() -> TestResult {
        let root = unique_temp_dir("missing")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        fs::write(docs.join("target.md"), "target\n")?;
        fs::write(
            docs.join("source.md"),
            "[valid](target.md) [missing](missing.md) [external](https://example.com)\n```\n[ignored](ignored.md)\n```\n",
        )?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 1 {
            return Err(format!("expected one broken-link result, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_accepts_existing_relative_targets_and_fragments() -> TestResult {
        let root = unique_temp_dir("clean")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        fs::write(docs.join("target.md"), "target\n")?;
        fs::write(docs.join("source.md"), "[target](target.md#section)\n")?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(format!("expected clean-link result, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_accepts_resolvable_reference_links() -> TestResult {
        let root = unique_temp_dir("reference-clean")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        fs::write(docs.join("target.md"), "target\n")?;
        fs::write(
            docs.join("source.md"),
            "See [guide][ref] and [target][].\n\n[ref]: target.md\n[target]: target.md\n",
        )?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(format!("expected clean reference links, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_rejects_reference_link_to_missing_file() -> TestResult {
        let root = unique_temp_dir("reference-missing-target")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        let source = docs.join("source.md");
        fs::write(&source, "See [guide][ref].\n\n[ref]: missing.md\n")?;
        let canonical_root = fs::canonicalize(&root)?;

        let failures = check_file(&root, &canonical_root, &source)?;
        if failures.len() != 1 || !failures[0].contains("missing target missing.md") {
            return Err(format!("expected missing target failure, got {failures:?}").into());
        }

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 1 {
            return Err(format!("expected broken reference target, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_rejects_undefined_reference_label() -> TestResult {
        let root = unique_temp_dir("reference-undefined")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        let source = docs.join("source.md");
        fs::write(&source, "See [guide][missing].\n")?;
        let canonical_root = fs::canonicalize(&root)?;

        let failures = check_file(&root, &canonical_root, &source)?;
        if failures.len() != 1 || !failures[0].contains("undefined reference [missing]") {
            return Err(format!("expected undefined reference failure, got {failures:?}").into());
        }

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 1 {
            return Err(format!("expected undefined reference failure, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_ignores_uppercase_uri_scheme_reference_targets() -> TestResult {
        let root = unique_temp_dir("reference-uppercase-uri")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        fs::write(docs.join("source.md"), "See [guide][ref].\n\n[ref]: HTTPS://example.com\n")?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(
                format!("expected external URI reference to be ignored, got {exit_code}").into()
            );
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_ignores_reference_definitions_in_fenced_blocks_and_inline_code() -> TestResult
    {
        let root = unique_temp_dir("reference-ignored-defs")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        fs::write(
            docs.join("source.md"),
            "See [guide][ref].\n\n```\n[ref]: missing.md\n```\n\nWrite `[ref]: missing.md` in prose.\n",
        )?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 1 {
            return Err(format!("expected undefined reference failure, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    /// Each skip test pairs the skipped entry with a positive control: the same
    /// broken-link text in a real `.md` file must still be reported. Without
    /// the control, a walker that skipped *everything* would also pass.
    #[test]
    fn check_doc_links_skips_directories_named_like_markdown() -> TestResult {
        let root = unique_temp_dir("skip-dir")?;
        let docs = root.join("docs/adr");
        // A directory whose name ends in `.md` passes the extension test and is
        // rejected only by `!entry.file_type().is_file()`.
        fs::create_dir_all(docs.join("ignored.md"))?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(format!("a directory named *.md must be skipped, got {exit_code}").into());
        }

        // Positive control: identical name, but a real file with a broken link.
        fs::remove_dir_all(docs.join("ignored.md"))?;
        fs::write(docs.join("ignored.md"), "[missing](missing.md)\n")?;
        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 1 {
            return Err(format!("the same path as a file must be checked, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_skips_files_without_a_markdown_extension() -> TestResult {
        let root = unique_temp_dir("skip-ext")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        // A real file, so `is_file()` holds; rejected only by the extension test.
        fs::write(docs.join("notes.txt"), "[missing](missing.md)\n")?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(format!("a non-markdown file must be skipped, got {exit_code}").into());
        }

        // Positive control: identical content, `.md` extension.
        fs::write(docs.join("notes.md"), "[missing](missing.md)\n")?;
        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 1 {
            return Err(format!("the same content as .md must be checked, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_exact_error_variant() -> TestResult {
        let root = unique_temp_dir("exact-error-variant")?;
        let err = check_doc_links(&root, Some("docs/does-not-exist"))
            .expect_err("expected docs directory missing error");
        let message = err.to_string();
        if !message.contains("Docs directory not found") || !message.contains("does-not-exist") {
            return Err(format!("unexpected missing-directory error: {message}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_boundary_discriminator() -> TestResult {
        let root = unique_temp_dir("boundary-discriminator")?;
        let docs = root.join("docs/adr");
        // Directory whose name ends in `.md` exercises the `!is_file()` disjunct.
        fs::create_dir_all(docs.join("ignored.md"))?;
        // Real file with a non-markdown extension exercises the extension disjunct.
        fs::write(docs.join("notes.txt"), "[missing](missing.md)\n")?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(format!("skip-only tree should pass, got {exit_code}").into());
        }

        // Positive control: the same broken-link text in a real `.md` file must fail.
        fs::write(docs.join("live.md"), "[missing](missing.md)\n")?;
        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 1 {
            return Err(format!("live .md broken link must fail, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_doc_links_errors_when_docs_directory_is_missing() -> TestResult {
        // A mistyped or moved docs directory must fail loudly. Returning Ok(0)
        // here would report "no broken links" for a tree that was never read.
        let root = unique_temp_dir("missing-docs-dir")?;

        let error = match check_doc_links(&root, Some("docs/does-not-exist")) {
            Ok(code) => {
                return Err(format!("a missing docs directory must error, got exit {code}").into());
            }
            Err(error) => error.to_string(),
        };
        if !error.contains("Docs directory not found") || !error.contains("does-not-exist") {
            return Err(format!("unexpected missing-directory error: {error}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn markdown_link_targets_ignores_inline_code_and_escaped_links() -> TestResult {
        // ADRs that document Markdown syntax must not be rejected: neither
        // form renders as a link, so neither is a resolvable target.
        let live = markdown_inline_link_targets("see [guide](../reference/GUIDE.md) for detail")?;
        if live != vec!["../reference/GUIDE.md".to_string()] {
            return Err(format!("expected the live link target, got {live:?}").into());
        }

        let in_code = markdown_inline_link_targets("write it as `[literal](missing.md)` in prose")?;
        if !in_code.is_empty() {
            return Err(format!("inline code must yield no targets, got {in_code:?}").into());
        }

        let escaped = markdown_inline_link_targets(r"escape it as \[literal](missing.md) instead")?;
        if !escaped.is_empty() {
            return Err(format!("an escaped link must yield no targets, got {escaped:?}").into());
        }

        let doubled =
            markdown_inline_link_targets(r"write \\[live](missing.md) after a literal backslash")?;
        if doubled != vec!["missing.md".to_string()] {
            return Err(format!("a doubled-backslash link must stay live, got {doubled:?}").into());
        }

        // A code span inside a link *label* still leaves a real link, so the
        // target must remain checked -- masking must not swallow the link.
        let labelled = markdown_inline_link_targets("[the `Foo` type](../reference/FOO.md)")?;
        if labelled != vec!["../reference/FOO.md".to_string()] {
            return Err(
                format!("code in a link label must keep the target, got {labelled:?}").into()
            );
        }
        Ok(())
    }

    #[test]
    fn markdown_reference_link_labels_resolve_explicit_and_collapsed_forms() -> TestResult {
        let explicit = markdown_reference_link_labels("see [guide][ref] for detail")?;
        if explicit != vec!["ref".to_string()] {
            return Err(format!("expected explicit reference label, got {explicit:?}").into());
        }

        let collapsed = markdown_reference_link_labels("see [target][] for detail")?;
        if collapsed != vec!["target".to_string()] {
            return Err(format!("expected collapsed reference label, got {collapsed:?}").into());
        }

        let in_code = markdown_reference_link_labels("write `[guide][ref]` in prose")?;
        if !in_code.is_empty() {
            return Err(
                format!("inline code must yield no reference labels, got {in_code:?}").into()
            );
        }

        let escaped = markdown_reference_link_labels(r"escape it as \[guide][ref] instead")?;
        if !escaped.is_empty() {
            return Err(
                format!("an escaped reference link must yield no labels, got {escaped:?}").into()
            );
        }

        Ok(())
    }

    #[test]
    fn collect_reference_definitions_preserves_first_duplicate_label() -> TestResult {
        let lines = vec!["[ref]: existing.md", "[ref]: missing.md"];
        let defs = collect_reference_definitions(&lines)?;
        if defs.get("ref") != Some(&"existing.md".to_string()) {
            return Err(format!("expected first duplicate definition to win, got {defs:?}").into());
        }
        Ok(())
    }

    #[test]
    fn check_doc_links_normalizes_reference_label_whitespace() -> TestResult {
        let root = unique_temp_dir("reference-label-whitespace")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        fs::write(docs.join("target.md"), "target\n")?;
        fs::write(docs.join("source.md"), "See [guide][foo   bar].\n\n[foo bar]: target.md\n")?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(
                format!("expected normalized reference label to resolve, got {exit_code}").into()
            );
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn collect_reference_definitions_ignores_fenced_blocks_and_inline_code() -> TestResult {
        let lines = vec![
            "[live]: target.md",
            "```",
            "[fenced]: missing.md",
            "```",
            "prose `[inline]: missing.md` here",
        ];
        let defs = collect_reference_definitions(&lines)?;
        if defs.len() != 1 || defs.get("live") != Some(&"target.md".to_string()) {
            return Err(format!("expected only the live definition, got {defs:?}").into());
        }
        Ok(())
    }

    #[test]
    fn check_file_reports_documentation_read_errors_with_path_context() -> TestResult {
        let root = unique_temp_dir("read-error")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        let source = docs.join("invalid.md");
        fs::write(&source, [0xff, 0xfe])?;
        let canonical_root = fs::canonicalize(&root)?;

        let error = match check_file(&root, &canonical_root, &source) {
            Ok(_) => return Err("invalid UTF-8 should fail to read".into()),
            Err(error) => error,
        };
        let message = error.to_string();
        if !message.contains("failed to read documentation file") || !message.contains("invalid.md")
        {
            return Err(format!("unexpected documentation read error: {message}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
