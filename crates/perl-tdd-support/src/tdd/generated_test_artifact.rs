//! Capability-truthful generated Perl test artifacts (#8154).

use serde::{Deserialize, Serialize};

/// Readiness of a generated test artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeneratedTestReadiness {
    /// May authorize ordinary pass/fail execution when consumed by a runner.
    Runnable,
    /// Missing behavioral oracle or guessed inputs/outcomes.
    DraftNeedsOracle,
    /// Missing setup, integration environment, or verification steps.
    DraftNeedsSetup,
    /// No renderer for this framework/test kind.
    Unsupported,
}

impl GeneratedTestReadiness {
    /// Whether Rust-side metadata treats this row as TODO/pending.
    #[must_use]
    pub fn is_todo(self) -> bool {
        !matches!(self, Self::Runnable)
    }
}

/// Stable reason for [`GeneratedTestReadiness::Unsupported`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnsupportedReason {
    /// Human-readable explanation (framework × test kind).
    pub reason: String,
}

/// Escape a string for single-quoted Perl literals.
pub fn perl_single_quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Open a Test::More subtest with an escaped label.
#[must_use]
pub fn emit_subtest_open(label: &str) -> String {
    format!("subtest '{}' => sub {{\n", perl_single_quote(label))
}

/// Close a Test::More subtest callback.
#[must_use]
pub fn emit_subtest_close() -> &'static str {
    "};\n"
}

/// Emit a single-file Test::More draft that cannot report an authorizing pass.
#[must_use]
pub fn emit_test_more_draft_fail(reason: &str) -> String {
    let todo = perl_single_quote(reason);
    let fail_msg = perl_single_quote(&format!("not implemented: {reason}"));
    format!(
        "use Test::More;\n\n\
         TODO: {{\n    \
         local $TODO = '{todo}';\n    \
         fail('{fail_msg}');\n\
         }};\n\n\
         done_testing();\n"
    )
}

/// Emit a subtest body draft (caller wraps in `subtest`).
#[must_use]
pub fn emit_subtest_draft_fail(reason: &str) -> String {
    let todo = perl_single_quote(reason);
    let fail_msg = perl_single_quote(&format!("not implemented: {reason}"));
    format!(
        "    TODO: {{\n        \
         local $TODO = '{todo}';\n        \
         fail('{fail_msg}');\n    \
         }};\n"
    )
}

/// Unsupported combinations: no executable success path.
#[must_use]
pub fn emit_unsupported_stub(reason: &str) -> String {
    let msg = perl_single_quote(reason);
    format!(
        "# generated-test unsupported: {msg}\n\
         use Test::More;\n\n\
         plan skip_all => '{msg}';\n"
    )
}

/// A structural false-green finding in generated Perl source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FalseGreenViolation {
    /// Short rule id for fixtures.
    pub rule: &'static str,
    /// Explanation for humans and tests.
    pub detail: String,
}

/// Policy gate for generated Perl templates (#8154).
#[derive(Debug, Clone, Copy, Default)]
pub struct FalseGreenPolicy;

impl FalseGreenPolicy {
    /// Check generated source against readiness expectations.
    pub fn check(
        &self,
        source: &str,
        readiness: GeneratedTestReadiness,
    ) -> Vec<FalseGreenViolation> {
        let mut out = Vec::new();
        if readiness == GeneratedTestReadiness::Runnable {
            out.extend(self.forbidden_pass_patterns(source));
            if source.trim().is_empty() {
                out.push(FalseGreenViolation {
                    rule: "empty_runnable",
                    detail: "runnable artifact must not be empty".to_string(),
                });
            }
        } else if readiness != GeneratedTestReadiness::Unsupported {
            if !source.contains("TODO: {") {
                out.push(FalseGreenViolation {
                    rule: "draft_missing_todo",
                    detail: "draft artifact must encode a TODO block".to_string(),
                });
            }
            if !source.contains("fail(") {
                out.push(FalseGreenViolation {
                    rule: "draft_missing_fail",
                    detail: "draft artifact must include fail(...) inside TODO".to_string(),
                });
            }
            if source.contains("pass(") || source.contains("ok(1") {
                out.push(FalseGreenViolation {
                    rule: "draft_placeholder_pass",
                    detail: "draft must not contain unconditional pass/ok(1)".to_string(),
                });
            }
        }
        out.extend(self.swallowed_eval_pass(source));
        out
    }

    fn forbidden_pass_patterns(&self, source: &str) -> Vec<FalseGreenViolation> {
        let mut out = Vec::new();
        if source.contains("pass(") {
            out.push(FalseGreenViolation {
                rule: "unconditional_pass",
                detail: "runnable source must not use pass(...)".to_string(),
            });
        }
        if source.contains("ok(1") {
            out.push(FalseGreenViolation {
                rule: "ok_one",
                detail: "runnable source must not use ok(1...)".to_string(),
            });
        }
        out
    }

    fn swallowed_eval_pass(&self, source: &str) -> Vec<FalseGreenViolation> {
        let mut out = Vec::new();
        if source.contains("eval {") && source.contains("pass(") {
            out.push(FalseGreenViolation {
                rule: "eval_then_pass",
                detail: "eval blocks must not be followed by unconditional pass".to_string(),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_fail_emits_todo_and_fail() {
        let src = emit_test_more_draft_fail("oracle required");
        assert!(src.contains("TODO:"));
        assert!(src.contains("fail("));
        assert!(!src.contains("pass("));
    }

    #[test]
    fn policy_rejects_runnable_pass() {
        let policy = FalseGreenPolicy;
        let v = policy.check("pass('x');\n", GeneratedTestReadiness::Runnable);
        assert!(v.iter().any(|x| x.rule == "unconditional_pass"));
    }
}
