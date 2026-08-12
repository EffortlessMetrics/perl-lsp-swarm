use std::cmp::Ordering;

pub(super) fn constraint_matches(constraint: &str, version: &str) -> Option<bool> {
    if constraint.contains("||") {
        return None;
    }
    let observed = parse_version(version)?;
    let clauses = constraint
        .split(',')
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        return None;
    }
    let mut matches = true;
    for clause in clauses {
        let (operator, required) = parse_constraint_clause(clause)?;
        let ordering = compare_versions(&observed, &required);
        matches &= match operator {
            VersionOperator::Equal => ordering == Ordering::Equal,
            VersionOperator::Greater => ordering == Ordering::Greater,
            VersionOperator::GreaterEqual => ordering != Ordering::Less,
            VersionOperator::Less => ordering == Ordering::Less,
            VersionOperator::LessEqual => ordering != Ordering::Greater,
        };
    }
    Some(matches)
}

#[derive(Debug, Clone, Copy)]
enum VersionOperator {
    Equal,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

fn parse_constraint_clause(clause: &str) -> Option<(VersionOperator, Vec<u64>)> {
    for (prefix, operator) in [
        (">=", VersionOperator::GreaterEqual),
        ("<=", VersionOperator::LessEqual),
        ("==", VersionOperator::Equal),
        (">", VersionOperator::Greater),
        ("<", VersionOperator::Less),
        ("=", VersionOperator::Equal),
    ] {
        if let Some(version) = clause.strip_prefix(prefix) {
            return Some((operator, parse_version(version.trim())?));
        }
    }
    Some((VersionOperator::Equal, parse_version(clause)?))
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let version = version.trim().strip_prefix('v').unwrap_or(version.trim());
    if version.is_empty() {
        return None;
    }
    version
        .split('.')
        .map(|segment| {
            if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                segment.parse().ok()
            }
        })
        .collect()
}

fn compare_versions(left: &[u64], right: &[u64]) -> Ordering {
    let width = left.len().max(right.len());
    (0..width)
        .map(|index| {
            left.get(index)
                .copied()
                .unwrap_or_default()
                .cmp(&right.get(index).copied().unwrap_or_default())
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}
