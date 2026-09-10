//! Compile-time effect facts: what `use strict` / `use warnings` /
//! `use feature` / `use v5.x` established for a file.
//!
//! These are computed by **reusing `perl-pragma`**, which owns the
//! strict/warnings semantics and the full `use v5.10`→`v5.42` feature-bundle
//! progression. Hand-rolling a version→feature table is exactly the
//! fabricated-fact hazard the external-truth-gate doctrine warns against, so
//! the substrate borrows perl-pragma's correctness and only projects the result
//! into its own serde-serializable fact shape.

use serde::{Deserialize, Serialize};

use crate::id::FileId;

/// The effective compile-time pragma state for a file, as a fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileEffectFacts {
    /// The file these effects apply to.
    pub file_id: FileId,
    /// Whether `use strict` (all three of vars/subs/refs) is in effect at file
    /// scope.
    pub strict: bool,
    /// Whether warnings are globally enabled.
    pub warnings: bool,
    /// Whether `use utf8` is in effect.
    pub utf8: bool,
    /// Whether `unicode_strings` (via feature or bundle) is in effect.
    pub unicode_strings: bool,
    /// Enabled features (from `use feature` and version bundles).
    pub features: Vec<String>,
    /// Warning categories explicitly disabled via `no warnings 'CATEGORY'`.
    pub disabled_warnings: Vec<String>,
    /// The Perl language version requested via `use v5.x` / `use 5.0xx`, if any.
    pub perl_version: Option<String>,
}

impl CompileEffectFacts {
    /// Project a `perl_pragma::PragmaState` into a fact for `file_id`.
    ///
    /// `perl_version` is not part of `PragmaState`; the caller passes the
    /// version it saw on a bare `use v5.x` statement, if any.
    #[must_use]
    pub fn from_pragma_state(
        file_id: FileId,
        state: &perl_pragma::PragmaState,
        perl_version: Option<String>,
    ) -> Self {
        Self {
            file_id,
            strict: state.strict_vars && state.strict_subs && state.strict_refs,
            warnings: state.warnings,
            utf8: state.utf8,
            unicode_strings: state.unicode_strings,
            features: state.features.iter().map(|s| (*s).to_string()).collect(),
            disabled_warnings: state.disabled_warning_categories.clone(),
            perl_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::Digest;

    fn test_file_id() -> FileId {
        FileId::new("lib/App.pm", &Digest::of("x"))
    }

    fn alternate_file_id() -> FileId {
        FileId::new("script.pl", &Digest::of("y"))
    }

    #[test]
    fn projects_complete_pragma_state() {
        let state = perl_pragma::PragmaState {
            strict_vars: true,
            strict_subs: true,
            strict_refs: true,
            warnings: true,
            utf8: true,
            unicode_strings: true,
            features: vec!["signatures", "say"],
            disabled_warning_categories: vec![
                "uninitialized".to_string(),
                "deprecated".to_string(),
            ],
            ..Default::default()
        };
        let file_id = test_file_id();
        let facts = CompileEffectFacts::from_pragma_state(
            file_id.clone(),
            &state,
            Some("v5.38".to_string()),
        );

        assert_eq!(facts.file_id, file_id);
        assert!(facts.strict);
        assert!(facts.warnings);
        assert!(facts.utf8);
        assert!(facts.unicode_strings);
        assert_eq!(facts.features, vec!["signatures", "say"]);
        assert_eq!(
            facts.disabled_warnings,
            vec!["uninitialized".to_string(), "deprecated".to_string()]
        );
        assert_eq!(facts.perl_version.as_deref(), Some("v5.38"));
    }

    #[test]
    fn empty_projected_inputs_do_not_fabricate_effects() {
        let state = perl_pragma::PragmaState { features: Vec::new(), ..Default::default() };
        let file_id = alternate_file_id();
        let facts = CompileEffectFacts::from_pragma_state(file_id.clone(), &state, None);

        assert_eq!(facts.file_id, file_id);
        assert!(!facts.strict);
        assert!(!facts.warnings);
        assert!(!facts.utf8);
        assert!(!facts.unicode_strings);
        assert!(facts.features.is_empty());
        assert!(facts.disabled_warnings.is_empty());
        assert!(facts.perl_version.is_none());
    }

    #[test]
    fn projects_boolean_effect_fields_independently() {
        let cases = [
            ("strict", true, false, false, false),
            ("warnings", false, true, false, false),
            ("utf8", false, false, true, false),
            ("unicode_strings", false, false, false, true),
        ];

        for (enabled_field, strict, warnings, utf8, unicode_strings) in cases {
            let state = perl_pragma::PragmaState {
                strict_vars: strict,
                strict_subs: strict,
                strict_refs: strict,
                warnings,
                utf8,
                unicode_strings,
                features: Vec::new(),
                ..Default::default()
            };
            let file_id = alternate_file_id();
            let facts = CompileEffectFacts::from_pragma_state(
                file_id.clone(),
                &state,
                Some("v5.42".to_string()),
            );

            assert_eq!(facts.file_id, file_id);
            assert_eq!(
                (facts.strict, facts.warnings, facts.utf8, facts.unicode_strings),
                (strict, warnings, utf8, unicode_strings),
                "{enabled_field} must remain attached to its own projection field"
            );
            assert_eq!(facts.perl_version.as_deref(), Some("v5.42"));
        }
    }

    #[test]
    fn strict_requires_every_strict_category() {
        let cases =
            [("vars", false, true, true), ("subs", true, false, true), ("refs", true, true, false)];

        for (missing_category, strict_vars, strict_subs, strict_refs) in cases {
            let state = perl_pragma::PragmaState {
                strict_vars,
                strict_subs,
                strict_refs,
                ..Default::default()
            };
            let facts = CompileEffectFacts::from_pragma_state(test_file_id(), &state, None);
            assert!(
                !facts.strict,
                "strict must be false when the {missing_category} category is disabled"
            );
        }
    }
}
