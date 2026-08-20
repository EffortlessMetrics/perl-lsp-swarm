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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVersion {
    components: Vec<u64>,
    alpha: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
enum VersionOperator {
    Equal,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

fn parse_constraint_clause(clause: &str) -> Option<(VersionOperator, ParsedVersion)> {
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

fn parse_version(version: &str) -> Option<ParsedVersion> {
    let version = version.trim().strip_prefix('v').unwrap_or(version.trim());
    if version.is_empty() {
        return None;
    }

    let (version, alpha) = match version.split_once('_') {
        Some((version, alpha)) if !version.is_empty() && !alpha.is_empty() => {
            if alpha.contains('_') || !alpha.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            (version, Some(alpha.parse().ok()?))
        }
        Some(_) => return None,
        None => (version, None),
    };

    let components = version
        .split('.')
        .map(|segment| {
            if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                segment.parse().ok()
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ParsedVersion { components, alpha })
}

fn compare_versions(left: &ParsedVersion, right: &ParsedVersion) -> Ordering {
    let width = left.components.len().max(right.components.len());
    (0..width)
        .map(|index| {
            left.components
                .get(index)
                .copied()
                .unwrap_or_default()
                .cmp(&right.components.get(index).copied().unwrap_or_default())
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or_else(|| match (left.alpha, right.alpha) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        })
}

#[cfg(test)]
mod tests {
    use super::constraint_matches;

    #[test]
    fn supports_trailing_perl_alpha_components() {
        assert_eq!(constraint_matches("<1.23", "1.23_01"), Some(true));
        assert_eq!(constraint_matches(">=1.23", "1.23_01"), Some(false));
        assert_eq!(constraint_matches(">=1.23_01", "1.23_02"), Some(true));
        assert_eq!(constraint_matches("=1.23_01", "1.23_01"), Some(true));
    }

    #[test]
    fn rejects_malformed_or_nonfinal_alpha_components() {
        assert_eq!(constraint_matches("=1.23_", "1.23_01"), None);
        assert_eq!(constraint_matches("=1.23_01_02", "1.23_01"), None);
        assert_eq!(constraint_matches("=1.23_01.2", "1.23_01"), None);
    }
}
