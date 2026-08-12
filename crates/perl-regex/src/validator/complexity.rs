use crate::syntax::cursor::quoted_literal_end;

use super::{
    analysis::{RegexDiagnostic, RegexDiagnosticCode, RegexRange},
    config::RegexValidationConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupType {
    Normal,
    Lookbehind,
    BranchReset { branch_count: usize },
}

pub(crate) fn find_complexity_diagnostics(
    pattern: &str,
    config: &RegexValidationConfig,
) -> Vec<RegexDiagnostic> {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut stack = Vec::new();
    let mut unicode_property_count = 0usize;
    let mut emitted_unicode_limit = false;
    let mut emitted_lookbehind_limit = false;
    let mut emitted_branch_reset_nesting_limit = false;
    let mut emitted_branch_reset_branch_limit = false;
    let mut diagnostics = Vec::new();

    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if let Some(end) = quoted_literal_end(bytes, i) {
                    i = end;
                    continue;
                }

                if i + 1 < bytes.len() {
                    match bytes[i + 1] {
                        b'p' | b'P' => {
                            let property_offset = i;
                            i += 2;
                            if i < bytes.len() && bytes[i] == b'{' {
                                unicode_property_count = unicode_property_count.saturating_add(1);
                                if unicode_property_count > config.max_unicode_properties
                                    && !emitted_unicode_limit
                                {
                                    diagnostics.push(policy_diagnostic(
                                        RegexDiagnosticCode::UnicodePropertyLimit,
                                        property_offset,
                                        2,
                                        bytes.len(),
                                        config.max_unicode_properties,
                                    ));
                                    emitted_unicode_limit = true;
                                }
                            }
                            continue;
                        }
                        _ => {
                            i += 2;
                            continue;
                        }
                    }
                }
            }
            b'[' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b']' {
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'(' => {
                let mut group = GroupType::Normal;
                if i + 1 < bytes.len() && bytes[i + 1] == b'?' {
                    i += 2;
                    if i < bytes.len() && bytes[i] == b'<' {
                        i += 1;
                        if i < bytes.len() && (bytes[i] == b'=' || bytes[i] == b'!') {
                            i += 1;
                            group = GroupType::Lookbehind;
                        }
                    } else if i < bytes.len() && bytes[i] == b'|' {
                        i += 1;
                        group = GroupType::BranchReset { branch_count: 1 };
                    }
                } else {
                    i += 1;
                }

                let group_offset = i.saturating_sub(1);
                match group {
                    GroupType::Lookbehind => {
                        let depth = stack
                            .iter()
                            .filter(|candidate| matches!(candidate, GroupType::Lookbehind))
                            .count();
                        if depth >= config.max_nesting && !emitted_lookbehind_limit {
                            diagnostics.push(policy_diagnostic(
                                RegexDiagnosticCode::LookbehindNestingLimit,
                                group_offset,
                                1,
                                bytes.len(),
                                config.max_nesting,
                            ));
                            emitted_lookbehind_limit = true;
                        }
                    }
                    GroupType::BranchReset { .. } => {
                        let depth = stack
                            .iter()
                            .filter(|candidate| {
                                matches!(candidate, GroupType::BranchReset { .. })
                            })
                            .count();
                        if depth >= config.max_nesting
                            && !emitted_branch_reset_nesting_limit
                        {
                            diagnostics.push(policy_diagnostic(
                                RegexDiagnosticCode::BranchResetNestingLimit,
                                group_offset,
                                1,
                                bytes.len(),
                                config.max_nesting,
                            ));
                            emitted_branch_reset_nesting_limit = true;
                        }
                    }
                    GroupType::Normal => {}
                }
                stack.push(group);
                continue;
            }
            b'|' => {
                if let Some(GroupType::BranchReset { branch_count }) = stack.last_mut() {
                    *branch_count = branch_count.saturating_add(1);
                    if *branch_count > config.max_branch_reset_branches
                        && !emitted_branch_reset_branch_limit
                    {
                        diagnostics.push(policy_diagnostic(
                            RegexDiagnosticCode::BranchResetBranchLimit,
                            i,
                            1,
                            bytes.len(),
                            config.max_branch_reset_branches,
                        ));
                        emitted_branch_reset_branch_limit = true;
                    }
                }
            }
            b')' => {
                stack.pop();
            }
            _ => {}
        }
        i += 1;
    }

    diagnostics
}

fn policy_diagnostic(
    code: RegexDiagnosticCode,
    offset: usize,
    width: usize,
    input_len: usize,
    limit: usize,
) -> RegexDiagnostic {
    RegexDiagnostic::new(code, RegexRange::anchored(offset, width, input_len), Some(limit))
}
