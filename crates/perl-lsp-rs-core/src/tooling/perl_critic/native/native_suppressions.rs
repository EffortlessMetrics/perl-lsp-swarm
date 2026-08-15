//! Ordered, position-aware native critic suppression directive parsing.
//!
//! A `## no critic <ids>` comment opens a suppression at its own line; a later
//! `## use critic` comment closes it. Suppression therefore covers a bounded
//! line region rather than the whole file, and never applies to a finding that
//! occurs before the directive that requested it.

use std::collections::BTreeSet;
use std::mem;

use perl_parser_core::syntax::source_context::{SourceRegionIndex, SourceRegionKind};
use serde::{Deserialize, Serialize};

use super::super::identity::CriticIdentityRegistry;
use super::super::{CriticFindingOrigin, NormalizedCriticFinding};
use super::native_contract::CriticFinding;

const NO_CRITIC_KEYWORD: &str = "## no critic";
const NO_NATIVE_CRITIC_KEYWORD: &str = "## no perl-lsp-critic";
const USE_CRITIC_KEYWORD: &str = "## use critic";
const USE_NATIVE_CRITIC_KEYWORD: &str = "## use perl-lsp-critic";

/// Line region covered by a native critic suppression directive.
///
/// Both variants start at the directive's own line ([`CriticSuppression::line`]),
/// so a directive never suppresses a finding that appears above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriticSuppressionScope {
    /// The directive is never re-enabled, so it covers the directive line
    /// through the last line of the file.
    ToEndOfFile,
    /// A matching `## use critic` re-enable closed the directive.
    ///
    /// The region covers the directive line up to, but **not** including, the
    /// re-enable directive's line, so a finding on the `## use critic` line is
    /// reported again.
    UntilLine(u32),
}

/// Parsed native critic suppression directive and the region it covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriticSuppression {
    /// Canonical, native, or approved compatibility rule ID supplied by the user.
    pub rule_id: String,
    /// Line region this directive actually covers.
    pub scope: CriticSuppressionScope,
    /// Zero-based line where the `## no critic` directive appears.
    ///
    /// This is the inclusive start of the covered region, not merely
    /// explanatory evidence.
    pub line: u32,
    /// Optional human reason after `--`.
    pub reason: Option<String>,
}

impl CriticSuppression {
    /// Whether this directive is active on zero-based source `line`.
    #[must_use]
    pub const fn covers_line(&self, line: u32) -> bool {
        if line < self.line {
            return false;
        }
        match self.scope {
            CriticSuppressionScope::ToEndOfFile => true,
            CriticSuppressionScope::UntilLine(end_line) => line < end_line,
        }
    }
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

/// One parsed directive occurrence, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DirectiveEvent {
    /// `## no critic <id>` — open a suppression for one selector.
    Disable { line: u32, rule_id: String, reason: Option<String> },
    /// `## use critic <id>` — close the suppression open for one selector.
    Enable { line: u32, rule_id: String },
    /// Bare `## use critic` — close every suppression currently open.
    ///
    /// Only the re-enable form has a bare spelling. Bare `## no critic` is
    /// deliberately *not* a directive: disabling every rule at once is the
    /// fail-open direction, so this parser requires explicit selectors there
    /// while accepting the fail-closed bare re-enable.
    EnableAll { line: u32 },
}

impl DirectiveEvent {
    const fn line(&self) -> u32 {
        match self {
            Self::Disable { line, .. } | Self::Enable { line, .. } | Self::EnableAll { line } => {
                *line
            }
        }
    }
}

/// A `## no critic` directive that has not yet been re-enabled.
#[derive(Debug, Clone)]
struct OpenSuppression {
    /// Index of the opening directive among all parsed events, so closed
    /// regions can be reported in the source order the user wrote them rather
    /// than the order they happened to be re-enabled.
    order: usize,
    rule_id: String,
    line: u32,
    reason: Option<String>,
}

impl CriticSuppressionMap {
    /// Parse ordered native critic suppression directives from proven Perl
    /// line-comment spans.
    ///
    /// [`SourceRegionIndex`] resolves strings, quote-like forms, regexes,
    /// heredocs, POD, recovery spans, and data sections before exposing line
    /// comments. Directive-looking payload in any of those regions is therefore
    /// not interpreted as configuration.
    ///
    /// Directives are then folded in source order into line-bounded regions:
    /// `## no critic <ids>` opens a region at its own line, and a matching
    /// `## use critic` closes it. An unterminated directive runs to the end of
    /// the file. Nothing is retroactive.
    ///
    /// Repeating `## no critic X` while `X` is already disabled is a no-op: the
    /// outer region already covers the inner one, and the first re-enable closes
    /// it. This flat model is deliberate — regions do not nest.
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let index = SourceRegionIndex::build(source);
        let mut events: Vec<DirectiveEvent> = index
            .regions()
            .iter()
            .filter(|region| region.kind == SourceRegionKind::LineComment)
            .filter_map(|region| {
                source
                    .get(region.start..region.end)
                    .map(|comment| (line_for_offset(source, region.start), comment))
            })
            .flat_map(|(line, comment)| parse_directive_comment(line, comment))
            .collect();
        // `SourceRegionIndex::regions` is documented as sorted, so this is a
        // stable no-op today. It is kept because the fold below is only correct
        // for events in source order, and that must not depend silently on
        // another crate's ordering invariant.
        events.sort_by_key(DirectiveEvent::line);

        Self { suppressions: fold_directive_events(events) }
    }

    /// Parsed suppression records in the source order of their `## no critic`
    /// directives, each carrying the line region it actually covers.
    #[must_use]
    pub fn suppressions(&self) -> &[CriticSuppression] {
        &self.suppressions
    }

    /// Unknown suppression IDs in deterministic lexical order.
    ///
    /// Runtime/editor layers can use this to emit one actionable warning. This
    /// core parser does not own user notifications. Only `## no critic`
    /// selectors are reported; validating re-enable selectors belongs to the
    /// projection layer that owns user-facing warnings.
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

    /// Whether this map directly suppresses a pre-normalization native finding
    /// **at that finding's own position**.
    ///
    /// This compatibility path intentionally accepts only the finding's exact
    /// native rule ID or suppression key. Canonical and compatibility aliases
    /// require producer-owned shape and therefore apply through
    /// [`Self::suppresses_normalized`], after normalization has established the
    /// logical finding. The raw path fails closed rather than guessing a shape.
    #[must_use]
    pub fn suppresses(&self, finding: &CriticFinding) -> bool {
        let line = finding.range.start.line;
        self.suppressions.iter().any(|suppression| {
            suppression.covers_line(line)
                && (suppression.rule_id == finding.rule_id
                    || suppression.rule_id == finding.suppression_key)
        })
    }

    /// Whether this map suppresses one normalized logical critic finding **at
    /// that finding's own position**.
    ///
    /// Canonical IDs and unambiguous compatibility aliases compare against the
    /// normalized canonical identity. Direct native rule IDs compare against
    /// retained producer aliases/contributors, so a combined rule such as
    /// `native.security.qx_readpipe` remains a valid broad native selector while
    /// shape-specific `PL606` can select only its `readpipe` logical finding.
    /// Ambiguous compatibility and unknown IDs suppress nothing.
    #[must_use]
    pub fn suppresses_normalized(&self, finding: &NormalizedCriticFinding) -> bool {
        let line = finding.range().start.line;
        self.suppressions.iter().any(|suppression| {
            suppression.covers_line(line) && selects_normalized(&suppression.rule_id, finding)
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

/// Fold ordered directive events into line-bounded suppression regions.
fn fold_directive_events(events: Vec<DirectiveEvent>) -> Vec<CriticSuppression> {
    let mut open: Vec<OpenSuppression> = Vec::new();
    let mut closed: Vec<(usize, CriticSuppression)> = Vec::new();

    for (order, event) in events.into_iter().enumerate() {
        match event {
            DirectiveEvent::Disable { line, rule_id, reason } => {
                if !open.iter().any(|entry| selectors_match(&entry.rule_id, &rule_id)) {
                    open.push(OpenSuppression { order, rule_id, line, reason });
                }
            }
            DirectiveEvent::Enable { line, rule_id } => {
                if let Some(position) =
                    open.iter().position(|entry| selectors_match(&entry.rule_id, &rule_id))
                {
                    let entry = open.remove(position);
                    closed.push(close_at(entry, CriticSuppressionScope::UntilLine(line)));
                }
            }
            DirectiveEvent::EnableAll { line } => {
                closed.extend(
                    open.drain(..)
                        .map(|entry| close_at(entry, CriticSuppressionScope::UntilLine(line))),
                );
            }
        }
    }

    closed
        .extend(open.into_iter().map(|entry| close_at(entry, CriticSuppressionScope::ToEndOfFile)));
    closed.sort_by_key(|(order, _)| *order);
    closed.into_iter().map(|(_, suppression)| suppression).collect()
}

fn close_at(entry: OpenSuppression, scope: CriticSuppressionScope) -> (usize, CriticSuppression) {
    (
        entry.order,
        CriticSuppression { rule_id: entry.rule_id, scope, line: entry.line, reason: entry.reason },
    )
}

/// Whether a `## use critic` selector names the same rule as an open
/// `## no critic` selector.
///
/// Exact spellings always match. Beyond that, two selectors that both resolve to
/// the *same* canonical identity match, so `## no critic PL100` can be closed by
/// `## use critic critic.testing.require_use_strict`. Native, ambiguous, and
/// unknown selectors match by exact spelling only — matching them by resolution
/// would either guess a shape or close more than the user named.
fn selectors_match(open: &str, requested: &str) -> bool {
    if open == requested {
        return true;
    }
    matches!(
        (resolve_rule_id(open), resolve_rule_id(requested)),
        (RuleIdResolution::Canonical(left), RuleIdResolution::Canonical(right)) if left == right
    )
}

fn selects_normalized(rule_id: &str, finding: &NormalizedCriticFinding) -> bool {
    match resolve_rule_id(rule_id) {
        RuleIdResolution::Canonical(canonical_id) => finding.canonical_id() == Some(canonical_id),
        RuleIdResolution::NativeRule => {
            finding.approved_aliases().iter().any(|identity| {
                identity.origin() == CriticFindingOrigin::NativeCritic && identity.code() == rule_id
            }) || finding.contributors().iter().any(|contributor| {
                contributor.identity().origin() == CriticFindingOrigin::NativeCritic
                    && contributor.identity().code() == rule_id
            })
        }
        RuleIdResolution::Ambiguous | RuleIdResolution::Unknown => false,
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

/// Strip a directive keyword, requiring a real token boundary after it.
///
/// `## no criticPL100` and `## use criticism` are near misses, not directives.
fn strip_keyword<'a>(comment: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = comment.strip_prefix(keyword)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then_some(rest)
}

fn parse_directive_comment(line: u32, comment: &str) -> Vec<DirectiveEvent> {
    if let Some(rest) = strip_keyword(comment, NO_NATIVE_CRITIC_KEYWORD)
        .or_else(|| strip_keyword(comment, NO_CRITIC_KEYWORD))
    {
        let (rules, reason) = split_reason(rest);
        return selectors(rules)
            .map(|rule_id| DirectiveEvent::Disable {
                line,
                rule_id: rule_id.to_string(),
                reason: reason.clone(),
            })
            .collect();
    }

    let Some(rest) = strip_keyword(comment, USE_NATIVE_CRITIC_KEYWORD)
        .or_else(|| strip_keyword(comment, USE_CRITIC_KEYWORD))
    else {
        return Vec::new();
    };

    let (rules, _) = split_reason(rest);
    let mut events: Vec<DirectiveEvent> = selectors(rules)
        .map(|rule_id| DirectiveEvent::Enable { line, rule_id: rule_id.to_string() })
        .collect();
    if events.is_empty() {
        events.push(DirectiveEvent::EnableAll { line });
    }
    events
}

fn split_reason(rest: &str) -> (&str, Option<String>) {
    rest.split_once("--").map_or((rest, None), |(rules, reason)| {
        let reason = reason.trim();
        (rules, (!reason.is_empty()).then(|| reason.to_string()))
    })
}

fn selectors(rules: &str) -> impl Iterator<Item = &str> {
    rules
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .map(str::trim)
        .filter(|rule_id| !rule_id.is_empty())
}

fn line_for_offset(source: &str, offset: usize) -> u32 {
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

    fn range_on_line(line: u32) -> Range {
        Range {
            start: Position { byte: 0, line, column: 0 },
            end: Position { byte: 1, line, column: 1 },
        }
    }

    fn range() -> Range {
        range_on_line(0)
    }

    fn raw_finding_on_line(rule_id: &str, line: u32) -> CriticFinding {
        CriticFinding {
            rule_id: rule_id.to_string(),
            category: CriticCategory::Syntax,
            severity: Severity::Harsh,
            range: range_on_line(line),
            message: "test finding".to_string(),
            explanation: "test explanation".to_string(),
            suppression_key: rule_id.to_string(),
            related: Vec::new(),
            fix: None,
        }
    }

    fn raw_finding(rule_id: &str) -> CriticFinding {
        raw_finding_on_line(rule_id, 0)
    }

    fn normalized_at(
        identity: CriticObservedIdentity<'_>,
        line: u32,
    ) -> Option<NormalizedCriticFinding> {
        normalize_critic_findings([CriticFindingCandidate::new(
            identity,
            CriticSourceIdentity::new([7; 16], 1),
            Severity::Harsh,
            range_on_line(line),
            "test finding",
            Some("test explanation".to_string()),
        )])
        .into_iter()
        .next()
    }

    fn normalized(identity: CriticObservedIdentity<'_>) -> Option<NormalizedCriticFinding> {
        normalized_at(identity, 0)
    }

    fn normalized_general(
        origin: CriticFindingOrigin,
        code: &str,
    ) -> Option<NormalizedCriticFinding> {
        CriticObservedIdentity::general(origin, code).ok().and_then(normalized)
    }

    fn normalized_general_at(
        origin: CriticFindingOrigin,
        code: &str,
        line: u32,
    ) -> Option<NormalizedCriticFinding> {
        CriticObservedIdentity::general(origin, code)
            .ok()
            .and_then(|identity| normalized_at(identity, line))
    }

    // ---------------------------------------------------------------------
    // #6968 negative controls: position-aware disable/re-enable.
    // ---------------------------------------------------------------------

    #[test]
    fn finding_before_the_directive_is_not_retroactively_suppressed() {
        let source = "print $x;\n## no critic native.testing.require_use_strict\nuse Foo;\n";
        let map = CriticSuppressionMap::from_source(source);

        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 0)));
        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 1)));
        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 2)));
        assert_eq!(map.suppressions()[0].line, 1);
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::ToEndOfFile);
    }

    #[test]
    fn finding_between_disable_and_re_enable_is_suppressed() {
        let source = "print $x;\n## no critic native.testing.require_use_strict\nuse Foo;\n## use critic\nuse Bar;\n";
        let map = CriticSuppressionMap::from_source(source);

        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 2)));
        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].line, 1);
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::UntilLine(3));
    }

    #[test]
    fn finding_after_re_enable_is_reported_again() {
        let source = "print $x;\n## no critic native.testing.require_use_strict\nuse Foo;\n## use critic\nuse Bar;\n";
        let map = CriticSuppressionMap::from_source(source);

        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 3)));
        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 4)));
    }

    #[test]
    fn named_re_enable_closes_only_its_own_selector() {
        let source = "\
## no critic native.testing.require_use_strict, native.testing.require_use_warnings
use Foo;
## use critic native.testing.require_use_strict
use Bar;
";
        let map = CriticSuppressionMap::from_source(source);

        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 3)));
        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_warnings", 3)));
        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 1)));
    }

    #[test]
    fn re_enable_matches_an_equivalent_canonical_spelling() {
        let source = "\
## no critic PL100
use Foo;
## use critic critic.testing.require_use_strict
use Bar;
";
        let map = CriticSuppressionMap::from_source(source);
        let inside = normalized_general_at(
            CriticFindingOrigin::NativeCritic,
            "native.testing.require_use_strict",
            1,
        );
        let after = normalized_general_at(
            CriticFindingOrigin::NativeCritic,
            "native.testing.require_use_strict",
            3,
        );

        assert!(inside.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
        assert!(!after.as_ref().is_some_and(|finding| map.suppresses_normalized(finding)));
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::UntilLine(2));
    }

    #[test]
    fn re_enable_before_any_disable_suppresses_nothing_and_does_not_arm_later_lines() {
        let source = "## use critic\nprint $x;\n";
        let map = CriticSuppressionMap::from_source(source);

        assert!(map.suppressions().is_empty());
        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 1)));
    }

    #[test]
    fn repeated_disable_and_enable_cycles_alternate_regions() {
        let source = "\
## no critic native.testing.require_use_strict
a;
## use critic
b;
## no critic native.testing.require_use_strict
c;
";
        let map = CriticSuppressionMap::from_source(source);

        assert_eq!(map.suppressions().len(), 2);
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::UntilLine(2));
        assert_eq!(map.suppressions()[1].line, 4);
        assert_eq!(map.suppressions()[1].scope, CriticSuppressionScope::ToEndOfFile);
        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 1)));
        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 3)));
        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 5)));
    }

    #[test]
    fn repeated_disable_of_an_open_selector_keeps_the_outer_region() {
        let source = "\
## no critic native.testing.require_use_strict
a;
## no critic native.testing.require_use_strict
b;
## use critic
c;
";
        let map = CriticSuppressionMap::from_source(source);

        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].line, 0);
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::UntilLine(4));
        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 5)));
    }

    #[test]
    fn native_spelled_directives_share_the_same_scope_engine() {
        let source = "\
## no perl-lsp-critic native.testing.require_use_strict
a;
## use perl-lsp-critic native.testing.require_use_strict
b;
";
        let map = CriticSuppressionMap::from_source(source);

        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::UntilLine(2));
        assert!(map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 1)));
        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 3)));
    }

    #[test]
    fn trailing_directive_covers_its_own_line_but_not_the_line_above() {
        let map =
            CriticSuppressionMap::from_source("a;\nmy $x = 1; ## no critic PL100 -- generated\n");

        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].rule_id, "PL100");
        assert_eq!(map.suppressions()[0].reason.as_deref(), Some("generated"));
        assert_eq!(map.suppressions()[0].line, 1);
        assert!(map.suppressions()[0].covers_line(1));
        assert!(!map.suppressions()[0].covers_line(0));
    }

    #[test]
    fn bare_no_critic_is_not_a_directive_and_disables_nothing() {
        let map = CriticSuppressionMap::from_source("## no critic\nprint $x;\n");

        assert!(map.suppressions().is_empty());
        assert!(!map.suppresses(&raw_finding_on_line("native.testing.require_use_strict", 1)));
    }

    // ---------------------------------------------------------------------
    // Retained #6968 substrate: parser-proven extraction and selector identity.
    // ---------------------------------------------------------------------

    #[test]
    fn raw_finding_path_fails_closed_for_aliases_without_shape() {
        let map = CriticSuppressionMap::from_source("## no critic PL100\n");
        assert!(!map.suppresses(&raw_finding("native.testing.require_use_strict")));
    }

    #[test]
    fn canonical_id_suppresses_the_normalized_native_alias() {
        let map =
            CriticSuppressionMap::from_source("## no critic critic.testing.require_use_strict\n");
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
        assert!(
            readpipe.as_ref().is_some_and(|finding| readpipe_map.suppresses_normalized(finding))
        );
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
        let map = CriticSuppressionMap::from_source("## no critic native.security.qx_readpipe\n");
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
        let source = r###"my $double = "## no critic PL100";
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
"###;
        let map = CriticSuppressionMap::from_source(source);
        assert!(map.suppressions().is_empty());
    }

    #[test]
    fn re_enable_must_also_be_in_a_proven_line_comment() {
        let source = r###"## no critic PL100
my $double = "## use critic";
my $doc = <<'PAYLOAD';
## use critic
PAYLOAD
=pod
## use critic
=cut
"###;
        let map = CriticSuppressionMap::from_source(source);

        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::ToEndOfFile);
    }

    #[test]
    fn near_miss_prefix_is_not_a_directive() {
        let map = CriticSuppressionMap::from_source(
            "# no critic PL100\n## no-critic PL100\n## no criticPL100\n## use criticism PL100\n",
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
        let map =
            CriticSuppressionMap::from_source("my $x = 1;\r\nmy $y = 2;\r## no critic PL100\r");
        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].line, 2);
        assert!(map.suppressions()[0].covers_line(2));
        assert!(!map.suppressions()[0].covers_line(1));
    }

    #[test]
    fn multi_byte_source_does_not_shift_the_region_boundaries() {
        let source = "\
my $s = \"héllo wörld — ✓\";
## no critic PL100
my $t = \"日本語\";
## use critic
my $u = 1;
";
        let map = CriticSuppressionMap::from_source(source);

        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].line, 1);
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::UntilLine(3));
        assert!(!map.suppressions()[0].covers_line(0));
        assert!(map.suppressions()[0].covers_line(2));
        assert!(!map.suppressions()[0].covers_line(3));
    }

    #[test]
    fn crlf_re_enable_bounds_the_region_on_the_right_line() {
        let map = CriticSuppressionMap::from_source(
            "## no critic PL100\r\nmy $x = 1;\r\n## use critic\r\nmy $y = 2;\r\n",
        );
        assert_eq!(map.suppressions().len(), 1);
        assert_eq!(map.suppressions()[0].line, 0);
        assert_eq!(map.suppressions()[0].scope, CriticSuppressionScope::UntilLine(2));
    }
}
