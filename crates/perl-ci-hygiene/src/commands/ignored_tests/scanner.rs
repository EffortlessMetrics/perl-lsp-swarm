use color_eyre::eyre::Result;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::{display_path, read_lines, walk_entries};

static IGNORE_ATTR_RE: LazyLock<std::result::Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(r#"^\s*#\[ignore\b(?:(?:\s*=\s*)?\"(?P<d>[^\"]+)\"|\s*=\s*\'(?P<s>[^\']+)\')?"#)
});
static FN_RE: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)"));
static COMMENT_RE: LazyLock<std::result::Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"//\s*(.+)$"));

fn ignore_attr_re() -> Result<&'static Regex> {
    IGNORE_ATTR_RE
        .as_ref()
        .map_err(|err| color_eyre::eyre::eyre!("invalid ignore attr regex: {err}"))
}

fn fn_re() -> Result<&'static Regex> {
    FN_RE.as_ref().map_err(|err| color_eyre::eyre::eyre!("invalid test function regex: {err}"))
}

fn comment_re() -> Result<&'static Regex> {
    COMMENT_RE.as_ref().map_err(|err| color_eyre::eyre::eyre!("invalid comment regex: {err}"))
}

pub(super) struct IgnoreMatch {
    pub(super) location: String,
    pub(super) context: String,
    pub(super) reason: String,
    pub(super) test_name: String,
}

pub(super) fn collect(crates_root: &Path, repo_root: &Path) -> Result<Vec<IgnoreMatch>> {
    let mut results = Vec::new();
    let ignore_attr_re = ignore_attr_re()?;
    let fn_re = fn_re()?;
    let comment_re = comment_re()?;

    for entry in walk_entries(crates_root) {
        let path = entry.path();
        if !entry.file_type().is_file() || path.extension().is_some_and(|ext| ext != "rs") {
            continue;
        }
        let rel = display_path(repo_root, path);
        let lines = read_lines(path)?;
        for i in 0..lines.len() {
            let line = &lines[i];
            if !line.trim_start().starts_with("#[ignore") {
                continue;
            }

            let mut reason = String::new();
            if let Some(caps) = ignore_attr_re.captures(line) {
                if let Some(matched) = caps.name("d") {
                    reason = matched.as_str().to_string();
                } else if let Some(matched) = caps.name("s") {
                    reason = matched.as_str().to_string();
                }
            }
            let context_lines = {
                let end = std::cmp::min(lines.len(), i + 4);
                lines[i..end].join("\n")
            };
            if reason.is_empty()
                && comment_re.is_match(line)
                && let Some(comment) = comment_re.captures(line).and_then(|m| m.get(1))
            {
                reason = comment.as_str().to_string();
            }
            if reason.is_empty()
                && i + 1 < lines.len()
                && comment_re.is_match(&lines[i + 1])
                && let Some(comment) = comment_re.captures(&lines[i + 1]).and_then(|m| m.get(1))
            {
                reason = comment.as_str().to_string();
            }
            if reason.is_empty()
                && i + 2 < lines.len()
                && comment_re.is_match(&lines[i + 2])
                && let Some(comment) = comment_re.captures(&lines[i + 2]).and_then(|m| m.get(1))
            {
                reason = comment.as_str().to_string();
            }

            let mut test_name = String::new();
            if let Some(found) = fn_re.captures(&context_lines).and_then(|m| m.get(1)) {
                test_name = found.as_str().to_string();
            }

            results.push(IgnoreMatch {
                location: format!("{rel}:{}", i + 1),
                context: context_lines,
                reason,
                test_name,
            });
        }
    }
    Ok(results)
}
