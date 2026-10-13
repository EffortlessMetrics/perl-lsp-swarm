/// Limits bounding structural validation of one regex pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexValidationConfig {
    /// Maximum nesting depth for lookbehind and branch-reset constructs.
    /// Ordinary capturing and noncapturing groups are not depth-limited.
    /// Excess emits `LookbehindNestingLimit` or `BranchResetNestingLimit`.
    pub max_nesting: usize,
    /// Maximum unicode-property lookups (`\p{...}`) before reporting excess.
    pub max_unicode_properties: usize,
    /// Maximum alternatives inside one branch-reset group `(?|...)`.
    pub max_branch_reset_branches: usize,
}

impl Default for RegexValidationConfig {
    fn default() -> Self {
        Self { max_nesting: 10, max_unicode_properties: 50, max_branch_reset_branches: 50 }
    }
}
