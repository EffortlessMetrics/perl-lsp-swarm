//! File-level native critic suppression directive parsing.

use std::collections::BTreeSet;
use std::mem;

use perl_parser_core::syntax::source_context::{SourceRegionIndex, SourceRegionKind};
use serde::{Deserialize, Serialize};

use super::super::identity::CriticIdentityRegistry;
use super::super::{
    CriticFindingOrigin, NormalizedCriticFinding,
};
use super::native_contract::CriticFinding;

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
    NativeRule,
    Ambiguous,
    Unknown,
}

impl CriticSuppressionMap {
    /// Parse file-level native critic suppression directives from proven Perl
    /// line-comment spans.
    ///
    /// [`SourceRegionIndex`] resolves strings, quote-like forms, regexes,
    /// heredocs, POD, recovery spans, and data sections before exposing line
    /// comments. Directive-looking payload in any of those regions is therefore
    /// not interpreted as configuration.
    ///
    /// Both accepted prefixes are file-level declarations. The recorded line is
    /// evidence for explanations; it does not limit suppression to that line or
    /// to a following statement.
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let index = SourceRegionIndex::build(source);
        let suppressions = index
            .regions()
            .iter()
            .filter(|region| region.kind == SourceRegionKind::LineComment)
            .filter_map(|region| {
                source
                    .get(region.start..region.end)
                    .map(|comment| (line_for_offset(source, region.start), comment))
            })
            .flat_map(|(line, comment)| parse_suppression_comment(line, comment))
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
    /// A compatibility code such as `PL601` can name more than one canonical
    /// logical finding. Ambiguous compatibility IDs are retained as evidence
    /// and suppress nothing rather than guessing or suppressing every target.
    /// A shipped native rule ID remains a valid direct selector even when that
    /// producer emits more than one reviewed finding shape.
    #[must_use]
    pub fn ambiguous_rule_ids(&self) -> Vec<&str> {
        self.rule_ids_with_status(RuleIdResolution::Ambiguous)
    }

    /// Whether this map directly suppresses a pre-normalization native finding.
    ///
    /// This compatibility path intentionally accepts only the finding's exact
    /// native rule ID or suppression key. Canonical and compatibility aliases
    /// require producer-owned shape and therefore apply through
    /// [`Self::suppresses_normalized`], after normalization has established the
    /// logical finding. The raw path fails closed rather than guessing a shape.
    #[must_use]
    pub fn suppresses(&self, finding: &CriticFinding) -> bool {
        self.suppressions.iter().any(|suppression| {
            suppression.rule_id == finding.rule_id
                || suppression.rule_id == finding.suppression_key
        })
    }

    /// Whether this map suppresses one normalized logical critic finding.
    ///
    /// Canonical IDs and unambiguous compatibility aliases compare against the
    /// normalized canonical identity. Direct native rule IDs compare against
    /// retained producer aliases/contributors, so a combined rule such as
    /// `native.security.qx_readpipe` remains a valid broad native selector while
    /// shape-specific `PL606` can select only its `readpipe` logical finding.
    /// Ambiguous compatibility and unknown IDs suppress nothing.
    #[must_use]
    pub fn suppresses_normalized(&self, finding: &NormalizedCriticFinding) -> bool {
        self.suppressions.iter().any(|suppression| {
            match resolve_rule_id(&suppression.rule_id) {
                RuleIdResolution::Canonical(canonical_id) => {
                    finding.canonical_id() == Some(canonical_id)
                }
                RuleIdResolution::NativeRule => finding
                    .approved_aliases()
                    .iter()
                    .any(|identity| {
                        identity.origin() == CriticFindingOrigin::NativeCritic
                            && identity.code() == suppression.rule_id
                    })
                    || finding.contributors().iter().any(|contributor| {
                        contributor.identity().origin() == CriticFindingOrigin::NativeCritic
                            && contributor.identity().code() == suppression.rule_id
                    }),
                RuleIdResolution::Ambiguous | RuleIdResolution::Unknown => false,
            }
        })
    }

    fn rule_ids_with_status(&self, wanted: RuleIdResolution) -> Vec<&str> {
        let ids: BTreeSet<&str> = self
            .suppressions
            .iter()
            .filter_map(|suppression| {
                (mem::discriminant(&resolve_rule_id(&suppression.rule_id))
                    == mem::discriminant(&wanted))
                .then_some(suppression.rule_id.as_str())
            })
            .collect();
        ids.into_iter().collect()
    }
}

fn resolve_rule_id(rule_id: &str) -> RuleIdResolution {
    if let Some(entry) = CriticIdentityRegistry::by_canonical_id(rule_id) {
        return RuleIdResolution::Canonical(entry.canonical_id());
    }

    let mut canonical_ids = BTreeSet::new();
    let mut is_native_rule = false;
    for entry in CriticIdentityRegistry::entries() {
        for alias in entry.aliases().iter().filter(|alias| alias.code() == rule_id) {
            canonical_ids.insert(entry.canonical_id());
            is_native_rule |= alias.origin() == CriticFindingOrigin::NativeCritic;
        }
    }

    if is_native_rule {
        return RuleIdResolution::NativeRule;
    }

    let mut canonical_ids = canonical_ids.into_iter();
    match (canonical_ids.next(), canonical_ids.next()) {
        (None, _) => RuleIdResolution::Unknown,
        (Some(canonical_id), None) => RuleIdResolution::Canonical(canonical_id),
        (Some(_), Some(_)) => RuleIdResolution::Ambiguous,
    }
}

fn parse_suppression_comment(line: usize, comment: &str) -> Vec<CriticSuppression> {
    let Some(rest) = comment
        .strip_prefix(NO_CRITIC_PREFIX)
        .or_else(|| comment.strip_prefix(NO_NATIVE_CRITIC_PREFIX))
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

fn line_for_offset(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let mut line = 0;
    let mut cursor = 0;
    let limit = offset.min(bytes.len());
    while cursor < limit {
        match bytes[cursor] {
            b'\r' => {
                line += 1;
                cursor += usize::from(cursor + 1 < limit && bytes[cursor + 1] == b'\n');
            }
            b'\n' => line += 1,
            _ => {}
        }
        cursor += 1;
    }
    line
}

#[cfg(test)]
mod tests {
    use perl_parser_core::position::{Position, Range};

    use super::{CriticSuppressionMap, CriticSuppressionScope};
    use crate::tooling::perl_critic::{
        CriticCategory, CriticFinding, CriticFindingCandidate, CriticFindingOrigin,
        CriticObservedIdentity, CriticSourceIdentity, NormalizedCriticFinding, Severity,
        normalize_critic_findings,
    };

    fn range() -> Range {
        Range {
            start: Position { byte: 0, line: 0, column: 0 },
            end: Position { byte: 1, line: 0, column: 1 },
        }
    }

    fn raw_finding(rule_id: &str) -> CriticFinding {
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

    fn normalized(identity: CriticObservedIdentity<'_>) -> Option<NormalizedCriticFinding> {
        normalize_critic_findings([CriticFindingCandidate::new(
            identity,
            CriticSourceIdentity::new([7; 16], 1),
            Severity::Harsh,
            range(),
            "test finding",
            Some("test explanation".to_string()),
        )])
        .into_iter()
        .next()
    }

    fn normalized_general(
        origin: CriticFindingOrigin,
        code: &str,
    ) -> Option<NormalizedCriticFinding> {
        CriticObservedIdentity::general(origin, code).ok().and_then(normalized)
    }

    #[test]
    fn direct_native_id_suppresses_for_the_whole_file() {
        let source = "print $x;\n## no critic native.testing.require_use_strict\nuse Foo;\n";
        let map = CriticSuppressionMap::from_source(source);
        assert!(map.suppresses(&raw_finding("native.testing.require_use_strict")));
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::File);
        assert_eq!(map.suppressions()[0].line, 1);
    }

    #[test]
    fn raw_finding_path_fails_closed_for_aliases_without_shape() {
        let map = CriticSuppressionMap::from_source("## no critic PL100\n");
        assert!(!map.suppresses(&raw_finding("native.testing.require_use_strict")));
    }

    #[test]
    fn canonical_id_suppresses_the_normalized_native_alias() {
        let map = CriticSuppressionMap::from_source(
            "## no critic critic.testing.require_use_strict\n",
        );
        let finding = normalized_general(
            CriticFindingOrigin::NativeCritic,
            "native.testing.require_use_strict",
        );
        assert!(finding.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
    }

    #[test]
    fn unambiguous_pl_alias_suppresses_the_normalized_native_finding() {
        let map = CriticSuppressionMap::from_source("## no critic PL100\n");
        let finding = normalized_general(
            CriticFindingOrigin::NativeCritic,
            "native.testing.require_use_strict",
        );
        assert!(finding.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
    }

    #[test]
    fn shape_specific_pl_aliases_select_the_right_combined_native_findings() {
        let system_map = CriticSuppressionMap::from_source("## no critic PL603\n");
        let exec_map = CriticSuppressionMap::from_source("## no critic PL604\n");
        let readpipe_map = CriticSuppressionMap::from_source("## no critic PL606\n");
        let system = normalized(CriticObservedIdentity::native_system_call());
        let exec = normalized(CriticObservedIdentity::native_exec_call());
        let readpipe = normalized(CriticObservedIdentity::native_readpipe_exec());

        assert!(system.as_ref().is_some_and(|finding| system_map.suppresses_normalized(finding)));
        assert!(exec.as_ref().is_some_and(|finding| exec_map.suppresses_normalized(finding)));
        assert!(readpipe
            .as_ref()
            .is_some_and(|finding| readpipe_map.suppresses_normalized(finding)));
        assert!(!exec.as_ref().is_some_and(|finding| system_map.suppresses_normalized(finding)));
    }

    #[test]
    fn ambiguous_compatibility_code_is_visible_and_suppresses_nothing() {
        let map = CriticSuppressionMap::from_source("## no critic PL601\n");
        let backtick = normalized(CriticObservedIdentity::native_backtick_exec());
        let qx = normalized(CriticObservedIdentity::native_qx_exec());

        assert!(!backtick.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
        assert!(!qx.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
        assert_eq!(map.ambiguous_rule_ids(), vec!["PL601"]);
        assert!(map.unknown_rule_ids().is_empty());
    }

    #[test]
    fn combined_native_rule_id_is_valid_and_suppresses_each_native_shape() {
        let map = CriticSuppressionMap::from_source(
            "## no critic native.security.qx_readpipe\n",
        );
        let qx = normalized(CriticObservedIdentity::native_qx_exec());
        let readpipe = normalized(CriticObservedIdentity::native_readpipe_exec());

        assert!(qx.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
        assert!(readpipe.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
        assert!(map.ambiguous_rule_ids().is_empty());
        assert!(map.unknown_rule_ids().is_empty());
    }

    #[test]
    fn unknown_id_is_visible_and_does_not_suppress() {
        let map = CriticSuppressionMap::from_source("## no critic native.unknown.rule\n");
        let finding = normalized_general(
            CriticFindingOrigin::NativeCritic,
            "native.testing.require_use_strict",
        );
        assert!(!finding.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
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
    fn directive_must_be_in_a_proven_line_comment() {
        let source = r#"my $double = "## no critic PL100";
my $single = '## no critic PL100';
my $quote = q{## no critic PL100};
my $doc = <<'PAYLOAD';
## no critic PL100
PAYLOAD
=pod
## no critic PL100
=cut
__DATA__
## no critic PL100
"#;
        let map = CriticSuppressionMap::from_source(source);
        assert!(map.suppressions().is_empty());
    }

    #[test]
    fn directive_in_trailing_line_comment_is_accepted() {
        let map = CriticSuppressionMap::from_source(
            "my $x = 1; ## no critic PL100 -- generated\n",
        );
        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].rule_id, "PL100");
        assert_eq!(map.suppressions()[0].reason.as_deref(), Some("generated"));
        assert_eq!(map.suppressions()[0].line, 0);
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

    #[test]
    fn line_evidence_handles_crlf_and_lone_cr_without_double_counting() {
        let map = CriticSuppressionMap::from_source(
            "my $x = 1;\r\nmy $y = 2;\r## no critic PL100\r",
        );
        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].line, 2);
    }
}
