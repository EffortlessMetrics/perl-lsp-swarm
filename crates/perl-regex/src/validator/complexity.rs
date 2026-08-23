use crate::syntax::event::{RegexEventKind, RegexEventStream, RegexGroupKind};

use super::{
    analysis::{RegexDiagnostic, RegexDiagnosticCode},
    config::RegexValidationConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupType {
    Normal,
    Lookbehind,
    BranchReset { branch_count: usize },
}

pub(crate) fn find_complexity_diagnostics(
    stream: &RegexEventStream,
    config: &RegexValidationConfig,
) -> Vec<RegexDiagnostic> {
    let mut stack = Vec::new();
    let mut unicode_property_count = 0usize;
    let mut emitted_unicode_limit = false;
    let mut emitted_lookbehind_limit = false;
    let mut emitted_branch_reset_nesting_limit = false;
    let mut emitted_branch_reset_branch_limit = false;
    let mut diagnostics = Vec::new();

    for event in &stream.events {
        match event.kind {
            RegexEventKind::UnicodeProperty { .. } => {
                unicode_property_count = unicode_property_count.saturating_add(1);
                if unicode_property_count > config.max_unicode_properties && !emitted_unicode_limit
                {
                    diagnostics.push(RegexDiagnostic::new(
                        RegexDiagnosticCode::UnicodePropertyLimit,
                        event.range,
                        Some(config.max_unicode_properties),
                    ));
                    emitted_unicode_limit = true;
                }
            }
            RegexEventKind::GroupOpen(kind) => {
                let group = match kind {
                    RegexGroupKind::Lookbehind | RegexGroupKind::NegativeLookbehind => {
                        let depth = stack
                            .iter()
                            .filter(|candidate| matches!(candidate, GroupType::Lookbehind))
                            .count();
                        if depth >= config.max_nesting && !emitted_lookbehind_limit {
                            diagnostics.push(RegexDiagnostic::new(
                                RegexDiagnosticCode::LookbehindNestingLimit,
                                event.range,
                                Some(config.max_nesting),
                            ));
                            emitted_lookbehind_limit = true;
                        }
                        GroupType::Lookbehind
                    }
                    RegexGroupKind::BranchReset => {
                        let depth = stack
                            .iter()
                            .filter(|candidate| matches!(candidate, GroupType::BranchReset { .. }))
                            .count();
                        if depth >= config.max_nesting && !emitted_branch_reset_nesting_limit {
                            diagnostics.push(RegexDiagnostic::new(
                                RegexDiagnosticCode::BranchResetNestingLimit,
                                event.range,
                                Some(config.max_nesting),
                            ));
                            emitted_branch_reset_nesting_limit = true;
                        }
                        GroupType::BranchReset { branch_count: 1 }
                    }
                    _ => GroupType::Normal,
                };
                stack.push(group);
            }
            RegexEventKind::Alternation => {
                if let Some(GroupType::BranchReset { branch_count }) = stack.last_mut() {
                    *branch_count = branch_count.saturating_add(1);
                    if *branch_count > config.max_branch_reset_branches
                        && !emitted_branch_reset_branch_limit
                    {
                        diagnostics.push(RegexDiagnostic::new(
                            RegexDiagnosticCode::BranchResetBranchLimit,
                            event.range,
                            Some(config.max_branch_reset_branches),
                        ));
                        emitted_branch_reset_branch_limit = true;
                    }
                }
            }
            RegexEventKind::GroupClose(_) => {
                stack.pop();
            }
            _ => {}
        }
    }

    diagnostics
}
