use color_eyre::eyre::{Context, Result, eyre};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use walkdir::WalkDir;

static MARKDOWN_LINK_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"\[[^\]]*\]\(\s*<?([^\s)>]+)>?"#));

pub(crate) fn check_doc_links(repo_root: &Path, docs_dir: Option<&str>) -> Result<i32> {
    let docs_dir = docs_dir.unwrap_or("docs/adr");
    let docs_path = resolve_docs_path(repo_root, docs_dir);
    if !docs_path.is_dir() {
        return Err(eyre!("Docs directory not found: {}", docs_path.display()));
    }

    let mut failures = Vec::new();
    for entry in WalkDir::new(&docs_path).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to walk {}", docs_path.display()))?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|ext| ext.to_str()) != Some("md")
        {
            continue;
        }
        failures.extend(check_file(repo_root, entry.path())?);
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

fn check_file(repo_root: &Path, path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read documentation file {}", path.display()))?;
    let mut failures = Vec::new();
    let mut fenced = false;

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }

        for target in markdown_link_targets(line)? {
            let Some(target_path) = local_target_path(repo_root, path, &target) else {
                continue;
            };
            if !target_path.exists() {
                failures.push(format!(
                    "{}:{}: missing target {}",
                    display_path(repo_root, path),
                    line_index + 1,
                    target
                ));
                continue;
            }
            let canonical_root = fs::canonicalize(repo_root).with_context(|| {
                format!("failed to resolve repository root {}", repo_root.display())
            })?;
            let canonical_target = fs::canonicalize(&target_path).with_context(|| {
                format!("failed to resolve link target {}", target_path.display())
            })?;
            if !canonical_target.starts_with(&canonical_root) {
                failures.push(format!(
                    "{}:{}: target escapes repository {}",
                    display_path(repo_root, path),
                    line_index + 1,
                    target
                ));
            }
        }
    }

    Ok(failures)
}

fn markdown_link_targets(line: &str) -> Result<Vec<String>> {
    let regex = MARKDOWN_LINK_RE
        .as_ref()
        .map_err(|err| eyre!("failed to compile Markdown link matcher: {err}"))?;
    Ok(regex
        .captures_iter(line)
        .filter_map(|capture| capture.get(1))
        .map(|target| target.as_str().to_owned())
        .collect())
}

fn local_target_path(repo_root: &Path, source: &Path, target: &str) -> Option<PathBuf> {
    let target = target.split('#').next()?.trim();
    if target.is_empty()
        || target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
        || target.starts_with("file:")
        || target.starts_with("command:")
    {
        return None;
    }

    Some(if target.starts_with('/') {
        repo_root.join(target.trim_start_matches('/'))
    } else {
        source.parent()?.join(target)
    })
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
    use super::{check_doc_links, check_file};
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
    fn check_doc_links_skips_non_markdown_files_and_directories() -> TestResult {
        let root = unique_temp_dir("skipped-entries")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(docs.join("ignored.md"))?;
        fs::write(docs.join("notes.txt"), "[missing](missing-from-non-markdown-file.md)\n")?;
        fs::write(docs.join("source.md"), "[target](target.md)\n")?;
        fs::write(docs.join("target.md"), "target\n")?;

        let exit_code = check_doc_links(&root, None)?;
        if exit_code != 0 {
            return Err(format!("expected skipped entries to be ignored, got {exit_code}").into());
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn check_file_reports_documentation_read_errors_with_path_context() -> TestResult {
        let root = unique_temp_dir("read-error")?;
        let docs = root.join("docs/adr");
        fs::create_dir_all(&docs)?;
        let source = docs.join("invalid.md");
        fs::write(&source, [0xff, 0xfe])?;

        let error = match check_file(&root, &source) {
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
