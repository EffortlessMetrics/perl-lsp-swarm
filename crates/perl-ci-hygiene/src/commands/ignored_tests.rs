use color_eyre::eyre::Result;
use perl_ci_hygiene::categorize_ignore;
use std::env;
mod baseline;
mod scanner;

use std::collections::HashMap;
use std::path::Path;

use crate::{GREEN, NC, RED, YELLOW};

const CATEGORIES: [&str; 9] =
    ["brokenpipe", "feature", "infra", "protocol", "manual", "stress", "bug", "bare", "other"];

#[derive(Clone)]
struct IgnoredDetail {
    category: String,
    location: String,
    reason: String,
    test_name: String,
}

pub(crate) fn count(repo_root: &Path, update: bool, check: bool) -> Result<i32> {
    let baseline_path = repo_root.join("scripts").join(".ignored-baseline");
    let verbose = env::var("VERBOSE").as_deref() == Ok("1");
    if update && check {
        return Err(color_eyre::eyre::eyre!(
            "choose exactly one of --update or --check for ignored-test-count"
        ));
    }

    let mut counts: HashMap<String, usize> =
        CATEGORIES.iter().map(|category| ((*category).to_string(), 0)).collect();

    let mut records: Vec<IgnoredDetail> = Vec::new();
    let crates_root = repo_root.join("crates");
    let detail_matches = scanner::collect(&crates_root, repo_root)?;
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

    let total: usize = CATEGORIES.iter().map(|category| category_count(&counts, category)).sum();

    let baseline = baseline::load(&baseline_path).unwrap_or_else(|_| {
        let mut empty = HashMap::new();
        for category in &CATEGORIES {
            empty.insert((*category).to_string(), 0);
        }
        empty.insert("total".to_string(), 0);
        empty
    });

    let baseline_total = baseline.get("total").copied().unwrap_or(0);

    println!("===============================================");
    println!("        Ignored Tests Summary");
    println!("===============================================");
    println!("{:<12} {:>8} {:>8} {:>8}", "Category", "Count", "Baseline", "Delta");
    println!("-----------------------------------------------");
    for category in CATEGORIES {
        let current = category_count(&counts, category);
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

    let ci_debt = category_count(&counts, "brokenpipe")
        + category_count(&counts, "bug")
        + category_count(&counts, "bare")
        + category_count(&counts, "other");
    let backlog = category_count(&counts, "feature") + category_count(&counts, "infra");
    let permanent = category_count(&counts, "manual") + category_count(&counts, "stress");
    println!();
    println!("CI_DEBT    = {ci_debt:>3}  (brokenpipe + bug + bare + other; must be 0)");
    println!("BACKLOG    = {backlog:>3}  (feature + infra; planned work)");
    println!("PERMANENT  = {permanent:>3}  (manual + stress; bench/helpers)");
    println!();

    if verbose {
        println!("Detailed breakdown by category:");
        println!();
        for category in CATEGORIES {
            let cat_count = category_count(&counts, category);
            if cat_count == 0 {
                continue;
            }
            println!("{YELLOW}=== {category} ({cat_count}) ==={NC}");
            for record in &records {
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

    let next_mode = if update {
        Some("update")
    } else if check {
        Some("check")
    } else {
        None
    };
    let next_mode = next_mode.unwrap_or("show");

    match next_mode {
        "update" => {
            baseline::write(&baseline_path, &counts, total)?;
            println!("{GREEN}Baseline updated successfully.{NC}");
            Ok(0)
        }
        "check" => {
            if total > baseline_total {
                println!(
                    "{RED}ERROR: Ignored test count increased from {baseline_total} to {total}{NC}"
                );
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
        "show" => {
            if total > 0 {
                println!("Run with VERBOSE=1 for detailed breakdown:");
                println!("  VERBOSE=1 scripts/ignored-test-count.sh");
                println!();
                println!("To update baseline:");
                println!("  scripts/ignored-test-count.sh --update");
            }
            Ok(0)
        }
        _ => Ok(0),
    }
}

fn category_count(counts: &HashMap<String, usize>, category: &str) -> usize {
    counts.get(category).copied().unwrap_or(0)
}

fn format_delta(current: usize, baseline: usize) -> String {
    let delta = current.abs_diff(baseline);
    if current > baseline {
        format!("{RED}+{delta}{NC}")
    } else if current < baseline {
        format!("{GREEN}-{delta}{NC}")
    } else {
        "0".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::format_delta;
    use crate::{GREEN, NC, RED};

    #[test]
    fn format_delta_adds_directional_colored_deltas() {
        assert_eq!(format_delta(5, 5), "0");
        assert_eq!(format_delta(7, 5), format!("{RED}+2{NC}"));
        assert_eq!(format_delta(4, 7), format!("{GREEN}-3{NC}"));
    }
}
