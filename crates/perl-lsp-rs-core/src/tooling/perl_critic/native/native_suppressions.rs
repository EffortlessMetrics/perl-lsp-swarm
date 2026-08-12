//! File-level native critic suppression directive parsing.

use std::collections::BTreeSet;

use super::native_contract::CriticFinding;
use super::super::identity::CriticIdentityRegistry;
use serde::{Deserialize, Serialize};

const NO_CRITIC_PREFIX: &str = "## no critic ";
const NO_NATIVE_CRITIC_PREFIX: &str = "## no perl-lsp-critic ";

/// Scope covered by a native critic suppression directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticSuppressionScope {
    /// Suppression applies to the whole file, regardless of directive line.
    File,
}

/// Parsed native critic suppression directive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriticSuppression {
    /// Canonical, native, or approved compatibility rule ID supplied by the user.
    pub rule_id: String,
    /// Scope covered by this directive.
    pub scope: CriticSuppressionScope,
    /// Zero-based line where the directive appears, retained for explanation only.
    pub line: usize,
    /// Optional human reason after `--`.
    pub reason: Option<String>,
}

/// Parsed native critic suppressions for a source file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriticSuppressionMap {
    suppressions: Vec<CriticSuppression>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleIdResolution {
    Canonical(&'static str),
    Ambiguous,
    Unknown,
}

impl CriticSuppressionMap {
    /// Parse file-level native critic suppression directives from source text.
    ///
    /// Both accepted prefixes are file-level declarations. The recorded line is
    /// evidence for explanations; it does not limit suppression to that line or
    /// to a following statement.
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let suppressions = source
            .lines()
            .enumerate()
            .flat_map(|(line, text)| parse_suppression_line(line, text))
            .collect();

        Self { suppressions }
    }

    /// Parsed suppression records in source order.
    #[must_use]
    pub fn suppressions(&self) -> &[CriticSuppression] {
        &self.suppressions
    }

    /// Unknown suppression IDs in deterministic lexical order.
    ///
    /// Runtime/editor layers can use this to emit one actionable warning. This
    /// core parser does not own user notifications.
    #[must_use]
    pub fn unknown_rule_ids(&self) -> Vec<&str> {
        self.rule_ids_with_status(RuleIdResolution::Unknown)
    }

    /// Ambiguous compatibility IDs in deterministic lexical order.
    ///
    /// A code such as `PL601` can name more than one canonical logical finding.
    /// Ambiguous IDs are retained as evidence and suppress nothing rather than
    /// guessing or suppressing every possible target.
    #[must_use]
    pub fn ambiguous_rule_ids(&self) -> Vec<&str> {
        self.rule_ids_with_status(RuleIdResolution::Ambiguous)
    }

    /// Whether this map suppresses a native critic finding for the whole file.
    ///
    /// Direct native/suppression-key matches remain supported. Canonical IDs and
    /// unambiguous compatibility aliases resolve through the identity registry.
    /// Ambiguous and unknown IDs suppress nothing.
    #[must_use]
    pub fn suppresses(&self, finding: &CriticFinding) -> bool {
        self.suppressions.iter().any(|suppression| {
            if suppression.rule_id == finding.rule_id
                || suppression.rule_id == finding.suppression_key
            {
                return true;
            }

            let Some(finding_canonical_id) = canonical_id_for_finding(finding) else {
                return false;
            };

            matches!(
                resolve_rule_id(&suppression.rule_id),
                RuleIdResolution::Canonical(canonical_id)
                    if canonical_id == finding_canonical_id
            )
        })
    }

    fn rule_ids_with_status(&self, wanted: RuleIdResolution) -> Vec<&str> {
        let ids: BTreeSet<&str> = self
            .suppressions
            .iter()
            .filter_map(|suppression| {
                (std::mem::discriminant(&resolve_rule_id(&suppression.rule_id))
                    == std::mem::discriminant(&wanted))
                .then_some(suppression.rule_id.as_str())
            })
            .collect();
        ids.into_iter().collect()
    }
}

fn canonical_id_for_finding(finding: &CriticFinding) -> Option<&'static str> {
    [finding.rule_id.as_str(), finding.suppression_key.as_str()]
        .into_iter()
        .find_map(|id| match resolve_rule_id(id) {
            RuleIdResolution::Canonical(canonical_id) => Some(canonical_id),
            RuleIdResolution::Ambiguous | RuleIdResolution::Unknown => None,
        })
}

fn resolve_rule_id(rule_id: &str) -> RuleIdResolution {
    if let Some(entry) = CriticIdentityRegistry::by_canonical_id(rule_id) {
        return RuleIdResolution::Canonical(entry.canonical_id());
    }

    let mut canonical_id = None;
    for entry in CriticIdentityRegistry::entries() {
        if !entry.aliases().iter().any(|alias| alias.code() == rule_id) {
            continue;
        }

        match canonical_id {
            None => canonical_id = Some(entry.canonical_id()),
            Some(existing) if existing == entry.canonical_id() => {}
            Some(_) => return RuleIdResolution::Ambiguous,
        }
    }

    canonical_id.map_or(RuleIdResolution::Unknown, RuleIdResolution::Canonical)
}

fn parse_suppression_line(line: usize, text: &str) -> Vec<CriticSuppression> {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed
        .strip_prefix(NO_CRITIC_PREFIX)
        .or_else(|| trimmed.strip_prefix(NO_NATIVE_CRITIC_PREFIX))
    else {
        return Vec::new();
    };

    let (rules, reason) = rest.split_once("--").map_or((rest, None), |(rules, reason)| {
        let reason = reason.trim();
        (rules, (!reason.is_empty()).then(|| reason.to_string()))
    });

    rules
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter_map(|rule_id| {
            let rule_id = rule_id.trim();
            (!rule_id.is_empty()).then(|| CriticSuppression {
                rule_id: rule_id.to_string(),
                scope: CriticSuppressionScope::File,
                line,
                reason: reason.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use perl_parser_core::position::{Position, Range};

    use super::{CriticSuppressionMap, CriticSuppressionScope};
    use crate::tooling::perl_critic::{CriticCategory, CriticFinding, Severity};

    fn range() -> Range {
        Range {
            start: Position { byte: 0, line: 0, column: 0 },
            end: Position { byte: 1, line: 0, column: 1 },
        }
    }

    fn finding(rule_id: &str) -> CriticFinding {
        CriticFinding {
            rule_id: rule_id.to_string(),
            category: CriticCategory::Syntax,
            severity: Severity::Harsh,
            range: range(),
            message: "test finding".to_string(),
            explanation: "test explanation".to_string(),
            suppression_key: rule_id.to_string(),
            related: Vec::new(),
            fix: None,
        }
    }

    #[test]
    fn direct_native_id_suppresses_for_the_whole_file() {
        let source = "print $x;\n## no critic native.testing.require_use_strict\nuse Foo;\n";
        let map = CriticSuppressionMap::from_source(source);
        assert!(map.suppresses(&finding("native.testing.require_use_strict")));
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::File);
        assert_eq!(map.suppressions()[0].line, 1);
    }

    #[test]
    fn canonical_id_suppresses_the_native_alias() {
        let map = CriticSuppressionMap::from_source(
            "## no critic critic.testing.require_use_strict\n",
        );
        assert!(map.suppresses(&finding("native.testing.require_use_strict")));
    }

    #[test]
    fn unambiguous_pl_alias_suppresses_the_native_finding() {
        let map = CriticSuppressionMap::from_source("## no critic PL100\n");
        assert!(map.suppresses(&finding("native.testing.require_use_strict")));
    }

    #[test]
    fn ambiguous_compatibility_code_is_visible_and_suppresses_nothing() {
        let map = CriticSuppressionMap::from_source("## no critic PL601\n");
        assert!(!map.suppresses(&finding("native.security.backtick_exec")));
        assert_eq!(map.ambiguous_rule_ids(), vec!["PL601"]);
        assert!(map.unknown_rule_ids().is_empty());
    }

    #[test]
    fn combined_native_rule_id_still_suppresses_by_direct_match() {
        let map = CriticSuppressionMap::from_source(
            "## no critic native.security.qx_readpipe\n",
        );
        assert!(map.suppresses(&finding("native.security.qx_readpipe")));
    }

    #[test]
    fn unknown_id_is_visible_and_does_not_suppress() {
        let map = CriticSuppressionMap::from_source("## no critic native.unknown.rule\n");
        assert!(!map.suppresses(&finding("native.testing.require_use_strict")));
        assert_eq!(map.unknown_rule_ids(), vec!["native.unknown.rule"]);
        assert!(map.ambiguous_rule_ids().is_empty());
    }

    #[test]
    fn multiple_ids_and_one_reason_parse_deterministically() {
        let map = CriticSuppressionMap::from_source(
            "  ## no perl-lsp-critic PL100, native.testing.require_use_warnings -- generated fixture\n",
        );
        assert_eq!(map.suppressions().len(), 2);
        assert_eq!(map.suppressions()[0].rule_id, "PL100");
        assert_eq!(map.suppressions()[1].rule_id, "native.testing.require_use_warnings");
        assert_eq!(map.suppressions()[0].reason.as_deref(), Some("generated fixture"));
        assert_eq!(map.suppressions()[1].reason.as_deref(), Some("generated fixture"));
    }

    #[test]
    fn near_miss_prefix_is_not_a_directive() {
        let map = CriticSuppressionMap::from_source(
            "# no critic PL100\n## no-critic PL100\n## no criticPL100\n",
        );
        assert!(map.suppressions().is_empty());
    }

    #[test]
    fn duplicate_unknown_and_ambiguous_ids_are_reported_once_in_lexical_order() {
        let map = CriticSuppressionMap::from_source(
            "## no critic z.unknown PL601\n## no critic PL601, a.unknown\n",
        );
        assert_eq!(map.unknown_rule_ids(), vec!["a.unknown", "z.unknown"]);
        assert_eq!(map.ambiguous_rule_ids(), vec!["PL601"]);
    }
}
