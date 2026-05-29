use chrono::Utc;
use color_eyre::eyre::{Context, Result};
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use perl_ci_hygiene::categorize_ignore;

use crate::{GREEN, NC, RED, YELLOW, display_path, read_lines, walk_entries};

static IGNORE_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*#\[ignore\b(?:(?:\s*=\s*)?\"(?P<d>[^\"]+)\"|\s*=\s*\'(?P<s>[^\']+)\')?"#)
        .unwrap_or_else(|error| {
            unreachable!("IGNORE_ATTR_RE is a known-good static pattern: {error}")
        })
});

static FN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
        .unwrap_or_else(|error| unreachable!("FN_RE is a known-good static pattern: {error}"))
});

static COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"//\s*(.+)$")
        .unwrap_or_else(|error| unreachable!("COMMENT_RE is a known-good static pattern: {error}"))
});

const CATEGORIES: [&str; 9] =
    ["brokenpipe", "feature", "infra", "protocol", "manual", "stress", "bug", "bare", "other"];

pub(crate) fn count_ignored_tests(repo_root: &Path, update: bool, check: bool) -> Result<i32> {
    let baseline_path = repo_root.join("scripts").join(".ignored-baseline");
    let verbose = env::var("VERBOSE").as_deref() == Ok("1");
    if update && check {
        return Err(color_eyre::eyre::eyre!(
            "choose exactly one of --update or --check for ignored-test-count"
        ));
    }

    let (counts, records, total) = collect_ignored_summary(repo_root)?;
    let baseline = load_ignored_baseline(&baseline_path).unwrap_or_else(|_| empty_baseline());
    let baseline_total = baseline.get("total").copied().unwrap_or(0);

    print_summary(&counts, total, &baseline, baseline_total);
    print_debt_rollup(&counts);

    if verbose {
        print_verbose_details(&counts, &records);
    }

    match count_mode(update, check) {
        CountMode::Update => {
            write_ignored_baseline(&baseline_path, &counts, total)?;
            println!("{GREEN}Baseline updated successfully.{NC}");
            Ok(0)
        }
        CountMode::Check => check_against_baseline(total, baseline_total),
        CountMode::Show => show_next_steps(total),
    }
}

fn collect_ignored_summary(
    repo_root: &Path,
) -> Result<(HashMap<String, usize>, Vec<IgnoredDetail>, usize)> {
    let mut counts: HashMap<String, usize> =
        CATEGORIES.iter().map(|category| ((*category).to_string(), 0)).collect();

    let mut records: Vec<IgnoredDetail> = Vec::new();
    let crates_root = repo_root.join("crates");
    let detail_matches = collect_ignored_matches(&crates_root, repo_root)?;
    for detail in detail_matches {
        let category = categorize_ignore(&detail.reason, &detail.context);
        *counts.entry(category.clone()).or_default() += 1;
        records.push(IgnoredDetail {
            category,
            location: detail.location,
            test_name: detail.test_name,
            reason: detail.reason,
        });
    }

    let total: usize =
        CATEGORIES.iter().map(|category| counts.get(*category).copied().unwrap_or(0)).sum();

    Ok((counts, records, total))
}

fn empty_baseline() -> HashMap<String, usize> {
    let mut empty = HashMap::new();
    for category in CATEGORIES {
        empty.insert(category.to_string(), 0);
    }
    empty.insert("total".to_string(), 0);
    empty
}

fn print_summary(
    counts: &HashMap<String, usize>,
    total: usize,
    baseline: &HashMap<String, usize>,
    baseline_total: usize,
) {
    println!("===============================================");
    println!("        Ignored Tests Summary");
    println!("===============================================");
    println!("{:<12} {:>8} {:>8} {:>8}", "Category", "Count", "Baseline", "Delta");
    println!("-----------------------------------------------");
    for category in CATEGORIES {
        let current = counts.get(category).copied().unwrap_or(0);
        let previous = baseline.get(category).copied().unwrap_or(0);
        println!(
            "{:<12} {:>8} {:>8} {:>8}",
            category,
            current,
            previous,
            format_delta(current, previous),
        );
    }
    println!("-----------------------------------------------");
    println!(
        "{:<12} {:>8} {:>8} {:>8}",
        "TOTAL",
        total,
        baseline_total,
        format_delta(total, baseline_total),
    );
    println!("===============================================");
}

fn print_debt_rollup(counts: &HashMap<String, usize>) {
    let count = |category| counts.get(category).copied().unwrap_or(0);
    let ci_debt = count("brokenpipe") + count("bug") + count("bare") + count("other");
    let backlog = count("feature") + count("infra");
    let permanent = count("manual") + count("stress");
    println!();
    println!("CI_DEBT    = {ci_debt:>3}  (brokenpipe + bug + bare + other; must be 0)");
    println!("BACKLOG    = {backlog:>3}  (feature + infra; planned work)");
    println!("PERMANENT  = {permanent:>3}  (manual + stress; bench/helpers)");
    println!();
}

fn print_verbose_details(counts: &HashMap<String, usize>, records: &[IgnoredDetail]) {
    println!("Detailed breakdown by category:");
    println!();
    for category in CATEGORIES {
        let cat_count = counts.get(category).copied().unwrap_or(0);
        if cat_count == 0 {
            continue;
        }
        println!("{YELLOW}=== {category} ({cat_count}) ==={NC}");
        for record in records {
            if record.category != category {
                continue;
            }
            println!("  {}", record.location);
            if !record.test_name.is_empty() {
                println!("    fn: {}", record.test_name);
            }
            if !record.reason.is_empty() {
                println!("    reason: {}", record.reason);
            }
        }
        println!();
    }
}

enum CountMode {
    Update,
    Check,
    Show,
}

fn count_mode(update: bool, check: bool) -> CountMode {
    if update {
        CountMode::Update
    } else if check {
        CountMode::Check
    } else {
        CountMode::Show
    }
}

fn check_against_baseline(total: usize, baseline_total: usize) -> Result<i32> {
    if total > baseline_total {
        println!("{RED}ERROR: Ignored test count increased from {baseline_total} to {total}{NC}");
        println!();
        println!("New ignores must be justified. If intentional, run:");
        println!("  scripts/ignored-test-count.sh --update");
        println!();
        Ok(1)
    } else {
        println!(
            "{GREEN}OK: Ignored test count ({total}) is not higher than baseline ({baseline_total}){NC}"
        );
        Ok(0)
    }
}

fn show_next_steps(total: usize) -> Result<i32> {
    if total > 0 {
        println!("Run with VERBOSE=1 for detailed breakdown:");
        println!("  VERBOSE=1 scripts/ignored-test-count.sh");
        println!();
        println!("To update baseline:");
        println!("  scripts/ignored-test-count.sh --update");
    }
    Ok(0)
}

pub(crate) fn format_delta(current: usize, baseline: usize) -> String {
    let delta = current.abs_diff(baseline);
    if current > baseline {
        format!("{RED}+{delta}{NC}")
    } else if current < baseline {
        format!("{GREEN}-{delta}{NC}")
    } else {
        "0".to_string()
    }
}

fn load_ignored_baseline(path: &Path) -> Result<HashMap<String, usize>> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    let mut values = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Ok(parsed) = value.trim().parse::<usize>() else {
            continue;
        };
        values.insert(key.trim().to_string(), parsed);
    }
    Ok(values)
}

fn write_ignored_baseline(
    path: &Path,
    counts: &HashMap<String, usize>,
    total: usize,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut lines = Vec::new();
    lines.push(format!("# Ignored test baseline - {}", Utc::now().format("%Y-%m-%dT%H:%M:%SZ")));
    lines.push("# Updated by: ignored-test-count.sh --update".to_string());
    let mut ordered = BTreeMap::new();
    for key in CATEGORIES {
        ordered.insert(key, counts.get(key).copied().unwrap_or(0));
    }
    for (key, value) in &ordered {
        lines.push(format!("{key}={value}"));
    }
    lines.push(format!("total={total}"));
    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(())
}

struct IgnoreMatch {
    location: String,
    context: String,
    reason: String,
    test_name: String,
}

#[derive(Clone)]
struct IgnoredDetail {
    category: String,
    location: String,
    reason: String,
    test_name: String,
}

fn collect_ignored_matches(crates_root: &Path, repo_root: &Path) -> Result<Vec<IgnoreMatch>> {
    let mut results = Vec::new();
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
            if let Some(caps) = IGNORE_ATTR_RE.captures(line) {
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
                && COMMENT_RE.is_match(line)
                && let Some(comment) = COMMENT_RE.captures(line).and_then(|m| m.get(1))
            {
                reason = comment.as_str().to_string();
            }
            if reason.is_empty()
                && i + 1 < lines.len()
                && COMMENT_RE.is_match(&lines[i + 1])
                && let Some(comment) = COMMENT_RE.captures(&lines[i + 1]).and_then(|m| m.get(1))
            {
                reason = comment.as_str().to_string();
            }
            if reason.is_empty()
                && i + 2 < lines.len()
                && COMMENT_RE.is_match(&lines[i + 2])
                && let Some(comment) = COMMENT_RE.captures(&lines[i + 2]).and_then(|m| m.get(1))
            {
                reason = comment.as_str().to_string();
            }

            let mut test_name = String::new();
            if let Some(found) = FN_RE.captures(&context_lines).and_then(|m| m.get(1)) {
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
