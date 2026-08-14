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
    excluded_ranges: &[RegexRange],
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
        if let Some(excluded) = excluded_ranges.iter().find(|range| range.contains(i)) {
            i = excluded.end;
            continue;
        }
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
                                    diagnostics.extend(policy_diagnostic(
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
                let group_offset = i;
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

                match group {
                    GroupType::Lookbehind => {
                        let depth = stack
                            .iter()
                            .filter(|candidate| matches!(candidate, GroupType::Lookbehind))
                            .count();
                        if depth >= config.max_nesting && !emitted_lookbehind_limit {
                            diagnostics.extend(policy_diagnostic(
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
                            .filter(|candidate| matches!(candidate, GroupType::BranchReset { .. }))
                            .count();
                        if depth >= config.max_nesting && !emitted_branch_reset_nesting_limit {
                            diagnostics.extend(policy_diagnostic(
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
                        diagnostics.extend(policy_diagnostic(
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
) -> Option<RegexDiagnostic> {
    RegexRange::anchored(offset, width, input_len)
        .map(|range| RegexDiagnostic::new(code, range, Some(limit)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator::config::RegexValidationConfig;

    fn cfg() -> RegexValidationConfig {
        RegexValidationConfig {
            max_nesting: 1,
            max_unicode_properties: 1,
            max_branch_reset_branches: 1,
        }
    }

    #[test]
    fn emits_unicode_property_limit_outside_excluded_ranges() {
        let diagnostics = find_complexity_diagnostics(r"\p{L}\p{N}", &cfg(), &[]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, RegexDiagnosticCode::UnicodePropertyLimit);
    }

    #[test]
    fn skips_excluded_dynamic_span_before_live_limit() {
        let excluded = [RegexRange::new(0, 10).expect("range")];
        let diagnostics =
            find_complexity_diagnostics(r"(?{ \p{L}\p{N} })\p{L}\p{N}", &cfg(), &excluded);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].range.start >= excluded[0].end);
    }

    #[test]
    fn emits_lookbehind_and_branch_reset_limits() {
        let lookbehind = find_complexity_diagnostics(r"(?<=a(?<=b))", &cfg(), &[]);
        assert!(
            lookbehind.iter().any(|d| d.code == RegexDiagnosticCode::LookbehindNestingLimit),
            "{lookbehind:?}"
        );
        let branches = find_complexity_diagnostics(r"(?|a|b|c)", &cfg(), &[]);
        assert!(
            branches.iter().any(|d| d.code == RegexDiagnosticCode::BranchResetBranchLimit),
            "{branches:?}"
        );
    }

    #[test]
    fn quoted_literals_and_char_classes_do_not_count_as_properties() {
        let diagnostics = find_complexity_diagnostics(r"\Q\p{L}\E[\p{N}]", &cfg(), &[]);
        assert!(diagnostics.is_empty());
    }
}
