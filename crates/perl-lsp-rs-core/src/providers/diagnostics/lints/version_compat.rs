//! Perl version compatibility lint (PL900)
//!
//! Warns when code uses features not available in the declared Perl version.
//!
//! # How it works
//!
//! 1. First pass over top-level statements: collect the declared version (`use vN.NN`
//!    or `use N.NNN`) and any builtin imports that affect version checks.
//! 2. Build lexical pragma state with `PragmaTracker`, so explicit `use feature`
//!    and `no feature` pragmas are honored at each AST node.
//! 3. Second pass (via walker): detect version-gated AST constructs and emit
//!    `PL900` warnings for those not covered by the effective feature set.
//!
//! When no version is declared at all, the check emits nothing — undeclared
//! version is ambiguous (the file may be targeting the system Perl).
//!
//! # Perl 5.42 feature-gating
//!
//! In Perl 5.42 the `given`/`when`/`default` constructs and the smartmatch
//! operator `~~` were **not removed** — their removal was indefinitely
//! postponed and they became feature-gated instead. The deprecation warning
//! was removed in 5.42. These constructs now require an explicit
//! `use feature 'switch';` (for given/when/default) or
//! `use feature 'smartmatch';` (for `~~`) because they are no longer part of
//! the default `:5.42` feature bundle. This lint emits a `PL900` **Warning**
//! (never Error) when the feature is not enabled.
//!
//! Reference: <https://perldoc.perl.org/5.42.0/perldelta> — "After extensive
//! discussion their removal has been indefinitely postponed. Using them no
//! longer produces a deprecation warning."

use perl_diagnostics::codes::DiagnosticCode;
use perl_parser_core::ast::{Node, NodeKind};
use perl_pragma::{PerlVersion, PragmaQueryCursor, PragmaTracker, parse_perl_version};

use super::super::internal_types::Diagnostic;
use super::super::walker::walk_node;
use perl_diagnostics::codes::DiagnosticSeverity;

/// Feature → minimum (major, minor) version table.
///
/// If a feature's minimum version is met by the declared version (either
/// directly or via bundle implication), no warning is emitted.
///
/// Covers only the features whose remediation is a plain version bump. The
/// pragma-gated constructs (`class`, `defer`, `try`) need both a version and a
/// pragma, so their minimums live on [`CLASS_MIN_VERSION`],
/// [`DEFER_MIN_VERSION`] and [`TRY_MIN_VERSION`] next to the two-part
/// remediation that reads them; duplicating them here would let the two
/// diverge.
const FEATURE_VERSIONS: &[(&str, u32, u32)] = &[
    ("say", 5, 10),
    ("state", 5, 10),
    // switch: the feature bundle name for given/when/default constructs (Perl 5.10+)
    ("switch", 5, 10),
    // smartmatch: the feature name for the `~~` operator (Perl 5.10+).
    // In Perl 5.42 smartmatch was removed from the default feature bundle but
    // remains available via `use feature 'smartmatch';`.
    ("smartmatch", 5, 10),
    ("postfix_deref", 5, 20),
    // signatures: experimental since v5.20 but only stable-bundled at v5.36.
    // We use 5.36 as the effective minimum to match features_enabled_by_version,
    // preventing false-positive warnings on `use v5.20` files that rely on the
    // experimental pragma (`use feature 'signatures'`).
    ("signatures", 5, 36),
    ("field", 5, 38),
    // isa: experimental in v5.32, stable-bundled at v5.36.
    // `$obj isa 'ClassName'` — infix operator for class membership testing.
    ("isa", 5, 36),
    ("builtin", 5, 40),
];

/// `builtin` bundle and import minimums.
///
/// The namespace-level bundle still gates at 5.40, but individual functions
/// were introduced across multiple releases.
const BUILTIN_BUNDLE_MIN_VERSION: PerlVersion = PerlVersion::new(5, 40);

const BUILTIN_FUNCTION_VERSIONS: &[(&str, u32, u32)] = &[
    ("true", 5, 36),
    ("false", 5, 36),
    ("is_bool", 5, 36),
    ("inf", 5, 40),
    ("nan", 5, 40),
    ("weaken", 5, 36),
    ("unweaken", 5, 36),
    ("is_weak", 5, 36),
    ("blessed", 5, 36),
    ("refaddr", 5, 36),
    ("reftype", 5, 36),
    ("created_as_string", 5, 36),
    ("created_as_number", 5, 36),
    ("stringify", 5, 36),
    ("ceil", 5, 36),
    ("floor", 5, 36),
    ("indexed", 5, 36),
    ("trim", 5, 36),
    ("is_tainted", 5, 38),
    ("export_lexically", 5, 38),
    ("load_module", 5, 40),
];

/// Releases that introduced the pragma-gated constructs this lint reports.
///
/// These are the versions at which `use feature 'NAME';` starts being accepted:
/// perl rejects the pragma outright on an older interpreter (v5.38.2 answers
/// `use feature 'field';` with `Feature "field" is not supported by Perl
/// 5.38.2`), so a declared version below the minimum is not remediable by the
/// pragma alone.
const CLASS_MIN_VERSION: PerlVersion = PerlVersion::new(5, 38);
const DEFER_MIN_VERSION: PerlVersion = PerlVersion::new(5, 36);
const TRY_MIN_VERSION: PerlVersion = PerlVersion::new(5, 34);

const GIVEN_WHEN_DEPRECATION_VERSION: PerlVersion = PerlVersion::new(5, 38);
/// Perl 5.42: given/when/default became feature-gated (not removed).
/// The `switch` feature is no longer in the `:5.42` bundle.
const GIVEN_WHEN_FEATURE_GATE_VERSION: PerlVersion = PerlVersion::new(5, 42);
const SMARTMATCH_DEPRECATION_VERSION: PerlVersion = PerlVersion::new(5, 38);
/// Perl 5.42: smartmatch became feature-gated (not removed).
/// The `smartmatch` feature is no longer in the `:5.42` bundle.
const SMARTMATCH_FEATURE_GATE_VERSION: PerlVersion = PerlVersion::new(5, 42);

/// Non-interpolated postfix dereference became unconditional in v5.24
/// (perldelta 5.24: "Postfix dereferencing is no longer experimental"). Above
/// this floor no lexical feature state can switch the syntax off — in
/// particular `no feature 'postderef_qq'`, which only governs interpolation,
/// must leave every non-interpolated spelling silent.
const POSTDEREF_UNCONDITIONAL_VERSION: PerlVersion = PerlVersion::new(5, 24);

/// Check for Perl version compatibility issues.
///
/// Walks the AST looking for uses of version-gated features and emits
/// `PL900` warnings when the declared version does not support them.
/// `source` is the full document text the AST was parsed from. It is required
/// because one postfix-dereference spelling (`$href->@{...}`) shares its
/// [`NodeKind::HashSlice`] node kind with the ordinary and legacy slice
/// families, and the node carries no spelling discriminator: classification
/// reads the exact source gap between the receiver end and the keys start.
/// The same source also supplies the exact expression end for the star-form
/// spellings, whose `Unary` nodes span only the receiver.
pub fn check_version_compat(node: &Node, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    check_version_compat_with_project_version(node, source, diagnostics, None);
}

/// Check version compatibility using an optional folder-owned fallback target.
///
/// A source `use VERSION` declaration remains authoritative. When the source
/// declares no version, `project_version` supplies the PL900 target only if its
/// complete configured spelling is valid; malformed values fail closed.
pub fn check_version_compat_with_project_version(
    node: &Node,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    project_version: Option<&str>,
) {
    // Collect the declared version and builtin imports from top-level statements.
    let statements = match &node.kind {
        NodeKind::Program { statements } => statements,
        _ => return,
    };

    let mut declared_version: Option<PerlVersion> = None;
    let mut source_version_declared = false;
    let mut builtin_imports: Vec<String> = Vec::new();
    let mut builtin_bundle_declared = false;

    for stmt in statements {
        if let NodeKind::Use { module, args, .. } = &stmt.kind {
            // Check for `use vN.NN` or `use N.NNN`
            if looks_like_source_version(module) {
                source_version_declared = true;
            }
            if let Some(version) = parse_perl_version(module) {
                // Take the highest declared version if multiple appear
                match declared_version {
                    None => declared_version = Some(version),
                    Some(existing) if version > existing => declared_version = Some(version),
                    _ => {}
                }
            }
            if module == "builtin" {
                if args.is_empty() {
                    builtin_bundle_declared = true;
                }

                for arg in args {
                    for name in builtin_import_names(arg) {
                        if name.is_empty() {
                            continue;
                        }

                        if name.starts_with(':') {
                            builtin_bundle_declared = true;
                            continue;
                        }

                        if !builtin_imports.contains(&name) {
                            builtin_imports.push(name);
                        }
                    }
                }
            }
        }
    }

    // Validate the configured target independently of source authority. An invalid
    // project value is still actionable configuration state even when a valid
    // source declaration supplies the effective PL900 target.
    if let Some(project_version) = project_version
        && parse_configured_project_version(project_version).is_none()
    {
        diagnostics.push(invalid_project_version_diagnostic(project_version));
    }

    // A project version is only a fallback. Source declarations remain authoritative.
    let using_project_version = declared_version.is_none() && !source_version_declared;
    let declared_version = match declared_version {
        Some(v) => v,
        None if source_version_declared => return,
        None => {
            let Some(project_version) = project_version else {
                return;
            };
            if contains_source_version_declaration(node) {
                return;
            }
            let Some(version) = parse_configured_project_version(project_version) else {
                return;
            };
            version
        }
    };

    let pragma_map = PragmaTracker::build(node);
    let mut pragma_cursor = PragmaQueryCursor::new();

    // Second pass: walk AST for version-gated constructs.
    let diagnostics_before_walk = diagnostics.len();
    walk_node(node, &mut |n| {
        let pragma_state = pragma_cursor.state_for_offset(&pragma_map, n.location.start);
        let postfix_deref = postfix_deref_spelling(n, source);

        match &n.kind {
            // `class Foo { }` — the `class` feature shipped in v5.38 and is still
            // experimental, so it is in NO version bundle (`BUNDLE_5_40_FEATURES`
            // and `BUNDLE_5_42_FEATURES` both omit it). See
            // [`make_experimental_feature_diagnostic`] for why both halves are
            // required and why the version bump is never an alternative.
            NodeKind::Class { .. } => {
                let min_version = CLASS_MIN_VERSION;
                let version_ok = declared_version >= min_version;
                let feature_ok = pragma_state.has_feature("class");
                if !version_ok || !feature_ok {
                    diagnostics.push(make_experimental_feature_diagnostic(
                        n,
                        "class",
                        "class",
                        declared_version,
                        min_version,
                        feature_ok,
                    ));
                }
            }

            // `__CLASS__` — introduced in Perl v5.40 as part of the `class` feature.
            //
            // `__CLASS__` yields the run-time class of the current instance inside a
            // method, ADJUST block, or field initializer. The parser accepts it
            // unconditionally (PR #5280), so this lint carries the version contract.
            //
            // Compiling `__CLASS__` needs BOTH conditions, and they are independent:
            //
            //   1. declared version >= v5.40 — the `class` feature shipped in v5.38 but
            //      the `__CLASS__` keyword did not arrive until v5.40, so
            //      `use feature 'class'` does not backport it to v5.38/v5.39.
            //   2. the `class` feature lexically enabled — `class` is still experimental,
            //      so it is absent from every version bundle including `:5.40`
            //      (see `BUNDLE_5_40_FEATURES`). `use v5.40;` alone leaves `__CLASS__`
            //      a bareword, rejected under `strict subs`.
            //
            // Gating on either condition alone is a false negative: version-only misses
            // `use v5.40;` without the pragma, feature-only misses
            // `use v5.38; use feature 'class';`. The remediation names whichever half
            // is actually missing, so following it always produces compiling code.
            NodeKind::FunctionCall { name, .. } | NodeKind::Identifier { name }
                if name == "__CLASS__" =>
            {
                let min_version = PerlVersion::new(5, 40);
                let version_ok = declared_version >= min_version;
                let feature_ok = pragma_state.has_feature("class");
                if !version_ok || !feature_ok {
                    let declared =
                        format!("v{}.{}", declared_version.major, declared_version.minor);
                    let suggestion = match (version_ok, feature_ok) {
                        // Version is new enough; only the experimental pragma is missing.
                        (true, false) => format!(
                            "Add 'use feature \"class\";' — 'use {declared}' does not enable \
                             the experimental 'class' feature that provides __CLASS__"
                        ),
                        // Pragma is present but the keyword predates the declared version.
                        (false, true) => format!(
                            "Update 'use {declared}' to 'use v5.40' — 'use feature \"class\";' \
                             does not provide __CLASS__ before v5.40"
                        ),
                        // Both halves missing.
                        _ => format!(
                            "Update 'use {declared}' to 'use v5.40' and add \
                             'use feature \"class\";' — __CLASS__ requires both"
                        ),
                    };
                    diagnostics.push(make_diagnostic_with_details(
                        n,
                        "__CLASS__",
                        declared_version,
                        (5, 40),
                        DiagnosticSeverity::Warning,
                        Some(suggestion),
                    ));
                }
            }

            // `given` / `when` / `default` need the `switch` feature (v5.10+),
            // are deprecated in v5.38, and became feature-gated (not removed) in v5.42.
            NodeKind::Given { .. } | NodeKind::When { .. } | NodeKind::Default { .. } => {
                let construct = if matches!(&n.kind, NodeKind::Given { .. }) {
                    "given"
                } else if matches!(&n.kind, NodeKind::When { .. }) {
                    "when"
                } else {
                    "default"
                };

                if declared_version >= GIVEN_WHEN_FEATURE_GATE_VERSION {
                    // Perl 5.42+: switch is no longer in the default bundle.
                    // Emit a Warning (never Error) when the feature is not enabled.
                    if !pragma_state.has_feature("switch") {
                        diagnostics.push(make_given_when_feature_diagnostic(n, declared_version));
                    }
                } else if declared_version >= GIVEN_WHEN_DEPRECATION_VERSION {
                    diagnostics.push(make_given_when_default_diagnostic(n, declared_version));
                } else if !pragma_state.has_feature("switch") {
                    let min = feature_min_version("switch");
                    // The pragma is `switch`, not the construct name: perl
                    // rejects `use feature "given";`.
                    diagnostics.push(make_diagnostic(
                        n,
                        construct,
                        Some("switch"),
                        declared_version,
                        min,
                    ));
                }
            }

            // `try { } catch { }` — the `try` feature shipped in v5.34. It is the
            // one construct here that a bundle does enable (`BUNDLE_5_40_FEATURES`
            // lists `try`), so `has_feature` already goes quiet at v5.40+; below
            // that the pragma is required. The pragma is spelled `try`, not the
            // construct's display name `try/catch` — perl rejects
            // `use feature 'try/catch';` outright.
            NodeKind::Try { .. } => {
                let min_version = TRY_MIN_VERSION;
                let version_ok = declared_version >= min_version;
                let feature_ok = pragma_state.has_feature("try");
                if !version_ok || !feature_ok {
                    diagnostics.push(make_experimental_feature_diagnostic(
                        n,
                        "try/catch",
                        "try",
                        declared_version,
                        min_version,
                        feature_ok,
                    ));
                }
            }

            // `say` function call — requires v5.10
            NodeKind::FunctionCall { name, .. }
                if name == "say" && !pragma_state.has_feature("say") =>
            {
                let min = feature_min_version("say");
                diagnostics.push(make_diagnostic(n, "say", Some("say"), declared_version, min));
            }

            // `defer { }` block — the `defer` feature shipped in v5.36 and is
            // still experimental, so like `class` it is in no version bundle.
            // Detected only when the AST matches the parser's `defer { ... }`
            // shape, not for arbitrary helpers/imports named `defer`.
            NodeKind::Defer { .. } => {
                let min_version = DEFER_MIN_VERSION;
                let version_ok = declared_version >= min_version;
                let feature_ok = pragma_state.has_feature("defer");
                if !version_ok || !feature_ok {
                    diagnostics.push(make_experimental_feature_diagnostic(
                        n,
                        "defer",
                        "defer",
                        declared_version,
                        min_version,
                        feature_ok,
                    ));
                }
            }

            NodeKind::FunctionCall { name, .. } if name.starts_with("builtin::") => {
                let builtin_name = name.trim_start_matches("builtin::");
                let min = builtin_min_version(builtin_name);
                let imported = builtin_imports.iter().any(|import| import == builtin_name);

                if declared_version < min && !builtin_bundle_declared && !imported {
                    // No `use feature` pragma enables a `builtin::` function:
                    // on v5.38 `use builtin 'inf';` fails with
                    // `'inf' is not recognised as a builtin function`.
                    diagnostics.push(make_diagnostic(
                        n,
                        name,
                        None,
                        declared_version,
                        (min.major, min.minor),
                    ));
                }
            }

            NodeKind::Use { module, args, .. } if module == "builtin" => {
                if args.is_empty() {
                    if declared_version < BUILTIN_BUNDLE_MIN_VERSION {
                        diagnostics.push(make_diagnostic(
                            n,
                            "use builtin",
                            None,
                            declared_version,
                            (BUILTIN_BUNDLE_MIN_VERSION.major, BUILTIN_BUNDLE_MIN_VERSION.minor),
                        ));
                    }
                    return;
                }

                for arg in args {
                    for name in builtin_import_names(arg) {
                        let min = builtin_import_min_version(&name);
                        if declared_version < min {
                            let display = format!("use builtin {}", arg);
                            diagnostics.push(make_diagnostic(
                                n,
                                &display,
                                None,
                                declared_version,
                                (min.major, min.minor),
                            ));
                        }
                    }
                }
            }

            // `state $x` declaration — requires v5.10
            NodeKind::VariableDeclaration { declarator, .. }
                if declarator == "state" && !pragma_state.has_feature("state") =>
            {
                let min = feature_min_version("state");
                diagnostics.push(make_diagnostic(n, "state", Some("state"), declared_version, min));
            }

            // Postfix dereference — every parser-emitted non-interpolated
            // spelling (issue #13761): `->$*`, `->$#*`, `->@*`, `->@[...]`,
            // `->@{...}`, `->%*`, `->%{...}`, `->&*`, `->**`, classified by
            // [`postfix_deref_spelling`].
            //
            // The `Unary` match this arm replaced also listed `->@[` and
            // `->@{`, but no parser path emits those op strings: the real
            // slice shapes are `Binary("->@[]")` / `Binary("->%{}")` and a
            // `HashSlice` with a postfix receiver, so those two strings were
            // unreachable phantom spellings and are deleted rather than
            // matched. Ordinary and legacy slices are kept silent by the
            // classifier, never by kind narrowing here.
            //
            // Two distinct features, and only one governs this construct:
            //
            //   postderef     the `$r->@*` syntax itself, outside strings —
            //                 what this arm matches;
            //   postderef_qq  extends it to double-quotish interpolation.
            //
            // Verified on perl v5.38.2:
            //
            //   no feature 'postderef_qq'; print "$r->@*"  -> ARRAY(0x..)->@*
            //   no feature 'postderef';    my @a = $r->@*  -> still works
            //
            // so `postderef_qq` is the *interpolation* switch and naming it
            // here would be advice about a different feature. It is also not
            // supported on the versions this arm fires for: perl's own
            // bundles gain `postderef_qq` only at `:5.24`, and neither
            // bundle ever lists `postderef` (it became unconditional in
            // v5.24). An author targeting v5.20–v5.23 needs `postderef`.
            //
            // Non-interpolated postfix dereference is unconditional from
            // v5.24 (see [`POSTDEREF_UNCONDITIONAL_VERSION`]), so above that
            // floor the syntax must stay silent regardless of lexical feature
            // state. Below it, both feature names are queried because
            // `has_feature("postfix_deref")` canonicalizes to `postderef_qq`
            // in `perl-pragma`, which would not see an explicit
            // `use feature 'postderef';` — the very remediation this arm
            // advises — and following it would leave the warning up.
            _ if postfix_deref.is_some() => {
                if let Some((_, expression_range)) = postfix_deref {
                    let enabled = declared_version >= POSTDEREF_UNCONDITIONAL_VERSION
                        || pragma_state.has_feature("postfix_deref")
                        || pragma_state.has_feature("postderef");
                    if !enabled {
                        let min = feature_min_version("postfix_deref");
                        let mut diagnostic = make_diagnostic(
                            n,
                            "postfix deref",
                            Some("postderef"),
                            declared_version,
                            min,
                        );
                        // The star-form `Unary` nodes span the receiver only;
                        // the classifier recovered the exact expression range
                        // from the source, so the diagnostic covers the whole
                        // postfix-dereference expression for every spelling.
                        diagnostic.range = expression_range;
                        diagnostics.push(diagnostic);
                    }
                }
            }

            // Subroutine with a signature — requires v5.20
            NodeKind::Subroutine { signature: Some(_), .. }
                if !pragma_state.has_feature("signatures") =>
            {
                let min = feature_min_version("signatures");
                diagnostics.push(make_diagnostic(
                    n,
                    "subroutine signatures",
                    Some("signatures"),
                    declared_version,
                    min,
                ));
            }

            // `$obj isa 'ClassName'` — infix operator; stable at v5.36
            NodeKind::Binary { op, .. } if op == "isa" && !pragma_state.has_feature("isa") => {
                let min = feature_min_version("isa");
                diagnostics.push(make_diagnostic(n, "isa", Some("isa"), declared_version, min));
            }

            // Smartmatch operator `~~` — enabled by `use feature 'switch'` in v5.10+,
            // deprecated in v5.38, and became feature-gated (not removed) in v5.42.
            // In 5.42+ the `smartmatch` feature (not `switch`) gates the operator.
            NodeKind::Binary { op, .. } if op == "~~" => {
                if declared_version >= SMARTMATCH_FEATURE_GATE_VERSION {
                    // Perl 5.42+: smartmatch is no longer in the default bundle.
                    // Emit a Warning (never Error) when the feature is not enabled.
                    if !pragma_state.has_feature("smartmatch") {
                        diagnostics
                            .push(make_smartmatch_feature_gate_diagnostic(n, declared_version));
                    }
                } else if declared_version >= SMARTMATCH_DEPRECATION_VERSION {
                    diagnostics.push(make_smartmatch_diagnostic(n, declared_version));
                } else if !pragma_state.has_feature("switch") {
                    diagnostics.push(make_smartmatch_feature_diagnostic(n, declared_version));
                }
            }

            _ => {}
        }
    });

    if using_project_version {
        for diagnostic in &mut diagnostics[diagnostics_before_walk..] {
            diagnostic.message.push_str(" (target from project [perl].version)");
            if let Some(suggestion) = diagnostic.suggestion.as_mut() {
                suggestion.push_str(&format!(
                    ". This file declares no `use VERSION`, so the PL900 target v{}.{} came from \
                     the project `[perl].version`; add an explicit `use VERSION` to this file or \
                     raise `[perl].version`",
                    declared_version.major, declared_version.minor
                ));
            }
        }
    }
}

fn contains_source_version_declaration(node: &Node) -> bool {
    let mut found = false;
    walk_node(node, &mut |n| {
        if let NodeKind::Use { module, .. } = &n.kind
            && looks_like_source_version(module)
        {
            found = true;
        }
    });
    found
}

fn looks_like_source_version(module: &str) -> bool {
    let value = module.strip_prefix('v').unwrap_or(module);
    value.chars().next().is_some_and(|character| character.is_ascii_digit())
}

/// Parse the documented `[perl].version` spelling into its effective target.
///
/// Project configuration accepts only a major/minor pair, optionally prefixed
/// with `v`; callers must not use the raw spelling as the effective target.
pub fn parse_configured_project_version(value: &str) -> Option<PerlVersion> {
    let version = value.strip_prefix('v').unwrap_or(value);
    let mut components = version.split('.');
    let (Some(major), Some(minor), None) =
        (components.next(), components.next(), components.next())
    else {
        return None;
    };
    if major.is_empty()
        || minor.is_empty()
        || !major.chars().all(|character| character.is_ascii_digit())
        || !minor.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }

    parse_perl_version(value)
}

fn invalid_project_version_diagnostic(value: &str) -> Diagnostic {
    Diagnostic {
        range: (0, 0),
        severity: DiagnosticSeverity::Error,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message: format!(
            "Invalid project [perl].version value {value:?}; expected a major.minor Perl version such as 5.38"
        ),
        related_information: vec![],
        tags: vec![],
        suggestion: Some(
            "Set [perl].version to a supported major.minor Perl version, for example \"5.38\"."
                .to_string(),
        ),
        fixable: false,
        critic_observation: None,
    }
}

/// Return the minimum (major, minor) for a named feature from the table.
fn feature_min_version(feature: &str) -> (u32, u32) {
    FEATURE_VERSIONS
        .iter()
        .find(|(name, _, _)| *name == feature)
        .map(|(_, maj, min)| (*maj, *min))
        .unwrap_or((5, 0))
}

/// The postfix-dereference spellings the parser emits as `Unary` operator
/// identities (issue #13761). Matched exactly, never inferred: `->$*`,
/// `->$#*`, `->@*`, `->%*`, `->&*`, `->**`.
const POSTFIX_DEREF_UNARY_OPS: &[&str] = &["->$*", "->$#*", "->@*", "->%*", "->&*", "->**"];

/// Classify a node as one of the non-interpolated postfix-dereference
/// spellings, returning the spelling identity that matched and the exact
/// source range of the whole postfix-dereference expression.
///
/// Admitted shapes and their parser-emitted identities:
///
/// ```text
/// ->$*       Unary("->$*")
/// ->$#*      Unary("->$#*")
/// ->@*       Unary("->@*")
/// ->@[...]   Binary("->@[]")
/// ->@{...}   HashSlice with a postfix receiver
/// ->%*       Unary("->%*")
/// ->%{...}   Binary("->%{}")
/// ->&*       Unary("->&*")
/// ->**       Unary("->**")
/// ```
///
/// `HashSlice` is shared by three syntax families — the ordinary
/// `@hash{...}`, the legacy prefix dereference `@$href{...}` /
/// `@{$href}{...}`, and the postfix `$href->@{...}` — and the node carries no
/// spelling discriminator. Only the postfix family has an arrow between the
/// receiver and the keys, so the classification reads the exact source gap
/// between the target's end and the keys' start: an arrow there (with
/// perlderef's permitted whitespace/`#`-comment trivia between the tokens)
/// means postfix, a bare `{` means one of the legacy families. Kind-only
/// matching would flag slices that predate postfix dereference, so the source
/// text is load-bearing here.
///
/// The `Unary` and `Binary` arms match parser-emitted operator identities;
/// the parser normalizes inter-token trivia away from those identities
/// (`$r-> @*` still surfaces as `Unary("->@*")`), which the matrix tests pin.
/// The `Binary` slice ops `->@[]` / `->%{}` are produced only by the arrow
/// path in the parser, so the operator identity alone discriminates them.
/// Pre-5.20 arrow element access (`->[...]`, `->{...}`, `->(...)`) emits
/// different identities and never matches, and nothing here scans document
/// text outside these node-anchored ranges.
fn postfix_deref_spelling<'a>(node: &'a Node, source: &str) -> Option<(&'a str, (usize, usize))> {
    let spelling: &'a str = match &node.kind {
        NodeKind::Unary { op, .. } if POSTFIX_DEREF_UNARY_OPS.contains(&op.as_str()) => op,
        NodeKind::Binary { op, .. } if op == "->@[]" || op == "->%{}" => op,
        NodeKind::HashSlice { target, keys } => {
            let gap = source.get(target.location.end..keys.location.start)?;
            if !is_postfix_hash_slice_gap(gap) {
                return None;
            }
            "->@{"
        }
        _ => return None,
    };
    Some((spelling, postfix_deref_expression_range(node, source)))
}

/// The exact source range of a classified postfix-dereference expression.
///
/// The `Binary` slice and `HashSlice` nodes already span the full written
/// expression. The star-form `Unary` nodes end at the receiver — the parser's
/// arrow-chain tokens do not advance the node's end — so the exact end is
/// recovered from the source between the operand's end and the operator
/// spelling, skipping the trivia perlderef permits there. If the source does
/// not confirm the operator at that spot (a parser-surface change), the
/// node's own span is kept rather than a guessed range.
fn postfix_deref_expression_range(node: &Node, source: &str) -> (usize, usize) {
    if let NodeKind::Unary { op, operand } = &node.kind
        && let Some(tail) = op.strip_prefix("->")
    {
        let arrow = skip_perl_trivia(source, operand.location.end);
        if source.get(arrow..arrow + 2) == Some("->") {
            let sigil = skip_perl_trivia(source, arrow + 2);
            if source.get(sigil..sigil + tail.len()) == Some(tail) {
                return (node.location.start, sigil + tail.len());
            }
        }
    }
    (node.location.start, node.location.end)
}

/// Advance past the trivia Perl's tokenizer ignores between tokens —
/// whitespace and `#` line comments — starting at `offset`.
fn skip_perl_trivia(source: &str, mut offset: usize) -> usize {
    let bytes = source.as_bytes();
    loop {
        while offset < bytes.len() && bytes[offset].is_ascii_whitespace() {
            offset += 1;
        }
        if bytes.get(offset) == Some(&b'#') {
            match source[offset..].find('\n') {
                Some(newline) => offset += newline + 1,
                // A comment running to end-of-input; nothing follows it.
                None => return bytes.len(),
            }
        } else {
            return offset;
        }
    }
}

/// Does the source gap between a `HashSlice` target and its keys spell the
/// postfix form — `->`, then `@`, then `{`, with only trivia between them?
///
/// The ordinary (`@hash{...}`) and legacy (`@$href{...}`, `@{$href}{...}`)
/// families have no arrow in this gap, so they cannot match no matter how the
/// trivia falls. Only the receiver-adjacent gap is examined, never the
/// document at large.
fn is_postfix_hash_slice_gap(gap: &str) -> bool {
    let arrow = skip_perl_trivia(gap, 0);
    let Some(after_arrow) = gap.get(arrow..).and_then(|rest| rest.strip_prefix("->")) else {
        return false;
    };
    let sigil = skip_perl_trivia(after_arrow, 0);
    let Some(after_sigil) = after_arrow.get(sigil..).and_then(|rest| rest.strip_prefix('@')) else {
        return false;
    };
    let brace = skip_perl_trivia(after_sigil, 0);
    after_sigil.get(brace..).is_some_and(|rest| rest.starts_with('{'))
}

/// Return the minimum Perl version for a `builtin::name` call or named import.
fn builtin_min_version(name: &str) -> PerlVersion {
    BUILTIN_FUNCTION_VERSIONS
        .iter()
        .find(|(builtin_name, _, _)| *builtin_name == name)
        .map(|(_, maj, min)| PerlVersion::new(*maj, *min))
        .unwrap_or(BUILTIN_BUNDLE_MIN_VERSION)
}

fn builtin_import_min_version(name: &str) -> PerlVersion {
    if let Some(bundle) = name.strip_prefix(':') {
        return parse_perl_version(bundle).unwrap_or(BUILTIN_BUNDLE_MIN_VERSION);
    }

    builtin_min_version(name)
}

fn builtin_import_names(arg: &str) -> Vec<String> {
    let trimmed = arg.trim();

    if let Some(inner) = trimmed.strip_prefix("qw(").and_then(|s| s.strip_suffix(')')) {
        return inner
            .split_whitespace()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string())
            .collect();
    }

    vec![trimmed.trim_matches(|c| c == '\'' || c == '"').to_string()]
}

/// Build a PL900 diagnostic for a version-incompatible use of a *bundled*
/// feature — one a `use vX.Y` bundle really does turn on, so unlike the
/// pragma-gated constructs the version bump is a genuine remediation.
///
/// `feature` is the pragma name perl accepts, which is **not** the construct's
/// display name. This helper used to interpolate `display` into the
/// `use feature "..."` slot, so it emitted pragmas perl rejects outright and an
/// author who followed the advice traded the original diagnostic for a failed
/// `BEGIN` block. Verified on perl v5.38.2:
///
/// ```text
/// use feature "subroutine signatures";  Feature "..." is not supported
/// use feature "given";                  Feature "..." is not supported
/// use feature "postfix deref";          Feature "..." is not supported
/// use feature "builtin::inf";           Feature "..." is not supported
/// ```
///
/// Pass `None` when no `use feature` pragma can enable the construct at all.
/// That is the `builtin` namespace: on v5.38 even the correct-looking
/// `use builtin 'inf';` fails with `'inf' is not recognised as a builtin
/// function`, because `inf` arrived in v5.40 — the version bump is the only
/// remediation, so the pragma clause is dropped entirely.
///
/// The version clause is likewise dropped when the declared version already
/// meets the minimum. That is reachable through `no feature`, and it used to
/// produce the self-referential `Update 'use v5.36' to 'use v5.36'` and, for
/// `say`, the outright downgrade `Update 'use v5.36' to 'use v5.10'`.
fn make_diagnostic(
    node: &Node,
    display: &str,
    feature: Option<&str>,
    declared_version: PerlVersion,
    min_version: (u32, u32),
) -> Diagnostic {
    let declared = format!("v{}.{}", declared_version.major, declared_version.minor);
    let target = format!("v{}.{}", min_version.0, min_version.1);
    let version_ok = declared_version >= PerlVersion::new(min_version.0, min_version.1);

    // Message and suggestion are built from the same `(feature, version_ok)`
    // match so they cannot contradict each other: a message cites a minimum
    // version only when the declared version actually falls short of it.
    let (message, suggestion) = match (feature, version_ok) {
        // Bundle bump and pragma both work; either is a real fix.
        (Some(feature), false) => (
            default_version_message(display, declared_version, min_version),
            format!(
                "Update 'use {declared}' to 'use {target}' or add 'use feature \"{feature}\";'"
            ),
        ),
        // Version already satisfies the minimum, so only the pragma is left.
        // Citing the minimum here would state a satisfied condition as the
        // reason for the warning.
        (Some(feature), true) => (
            format!(
                "'{display}' requires the '{feature}' feature, which 'use {declared}' does not \
                 enable"
            ),
            format!(
                "Add 'use feature \"{feature}\";' — 'use {declared}' does not enable the \
                 '{feature}' feature"
            ),
        ),
        // No pragma can enable it; the version bump is the only remediation.
        (None, _) => (
            default_version_message(display, declared_version, min_version),
            format!("Update 'use {declared}' to 'use {target}'"),
        ),
    };

    Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message,
        related_information: vec![],
        tags: vec![],
        fixable: false,
        critic_observation: None,
        suggestion: Some(suggestion),
    }
}

/// The standard `'X' requires Perl vN.M+; declared version is vA.B` message,
/// used when the declared version genuinely falls short of the minimum.
fn default_version_message(
    display: &str,
    declared_version: PerlVersion,
    min_version: (u32, u32),
) -> String {
    format!(
        "'{}' requires Perl v{}.{}+; declared version is v{}.{}",
        display, min_version.0, min_version.1, declared_version.major, declared_version.minor,
    )
}

/// Build a PL900 for a construct that a `use feature` pragma gates.
///
/// `class`, `defer` and `try` all need TWO independent things, so the
/// remediation names whichever half is missing rather than offering them as
/// alternatives:
///
///   1. a declared version at or above `min_version` — perl rejects
///      `use feature 'NAME';` on an older interpreter, so the pragma cannot
///      backport the construct onto an older target;
///   2. the feature lexically enabled — `class` and `defer` are experimental
///      and appear in no version bundle (`BUNDLE_5_40_FEATURES` and
///      `BUNDLE_5_42_FEATURES` list neither), so no `use vX.Y` turns them on.
///
/// Consequently a version bump is never an *alternative* to the pragma, and at
/// or above `min_version` it is a no-op. The previous shared wording collapsed
/// into advice like `Update 'use v5.38' to 'use v5.38' or add 'use feature
/// "class";'` — telling the author to change a version to itself — and above
/// the minimum it advised an outright downgrade.
///
/// `display` names the construct as the author wrote it (`try/catch`);
/// `feature` is the pragma name perl actually accepts (`try`). They differ, and
/// conflating them produced `use feature "try/catch";`, which perl rejects with
/// `Feature "try/catch" is not supported`.
///
/// Message and suggestion are derived from the same `(version_ok, feature_ok)`
/// pair here so they cannot contradict each other. The shared
/// `'X' requires Perl vN.M+` wording is deliberately *not* used when the
/// declared version already meets the minimum: for `use v5.40; class Foo {}`
/// that read `'class' requires Perl v5.38+; declared version is v5.40`, which
/// states a satisfied condition as the reason for the warning and reads as a
/// false positive even though the code genuinely does not compile.
fn make_experimental_feature_diagnostic(
    node: &Node,
    display: &str,
    feature: &str,
    declared_version: PerlVersion,
    min_version: PerlVersion,
    feature_ok: bool,
) -> Diagnostic {
    let declared = format!("v{}.{}", declared_version.major, declared_version.minor);
    let target = format!("v{}.{}", min_version.major, min_version.minor);
    let version_ok = declared_version >= min_version;

    let (message, suggestion) = match (version_ok, feature_ok) {
        // Declared version already supports the pragma; only the pragma is
        // missing. The version is not the problem, so the message must not
        // cite one.
        (true, false) => (
            format!(
                "'{display}' requires the '{feature}' feature, which 'use {declared}' does not \
                 enable"
            ),
            format!(
                "Add 'use feature \"{feature}\";' — 'use {declared}' does not enable the \
                 '{feature}' feature"
            ),
        ),
        // Pragma is present but perl would reject it on the declared version.
        (false, true) => (
            format!(
                "'{display}' requires Perl {target}+; declared version is {declared}, which does \
                 not support 'use feature \"{feature}\";'"
            ),
            format!(
                "Update 'use {declared}' to 'use {target}' — 'use feature \"{feature}\";' is not \
                 supported before {target}"
            ),
        ),
        // Both halves missing; neither alone compiles.
        _ => (
            format!(
                "'{display}' requires Perl {target}+ and the '{feature}' feature; declared \
                 version is {declared}"
            ),
            format!(
                "Update 'use {declared}' to 'use {target}' and add 'use feature \"{feature}\";' \
                 — '{display}' requires both"
            ),
        ),
    };

    Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message,
        related_information: vec![],
        tags: vec![],
        fixable: false,
        critic_observation: None,
        suggestion: Some(suggestion),
    }
}

/// Build a PL900 deprecation Warning for given/when/default (v5.38–v5.41).
///
/// In Perl 5.42+ these constructs are feature-gated, not deprecated — use
/// [`make_given_when_feature_diagnostic`] for that case.
fn make_given_when_default_diagnostic(node: &Node, declared_version: PerlVersion) -> Diagnostic {
    let message = format!(
        "'given/when/default' is deprecated starting in Perl v5.38; declared version is v{}.{}",
        declared_version.major, declared_version.minor
    );

    Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message,
        related_information: vec![],
        tags: vec![],
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Refactor `given` / `when` / `default` to `if` / `elsif` or another supported control-flow form; this feature is deprecated in v5.38."
                .to_string(),
        ),
    }
}

/// Build a PL900 feature-gating Warning for given/when/default (v5.42+).
///
/// Perl 5.42 did not remove these constructs — it made them feature-gated.
/// The `switch` feature is no longer in the `:5.42` bundle, so an explicit
/// `use feature 'switch';` is required.
fn make_given_when_feature_diagnostic(node: &Node, declared_version: PerlVersion) -> Diagnostic {
    let message = format!(
        "'given/when/default' requires the 'switch' feature, which is not enabled by the v{}.{} bundle; declared version is v{}.{}",
        declared_version.major,
        declared_version.minor,
        declared_version.major,
        declared_version.minor,
    );

    Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message,
        related_information: vec![],
        tags: vec![],
        fixable: false,
        critic_observation: None,
        suggestion: Some("Add `use feature 'switch';` to enable given/when/default.".to_string()),
    }
}

/// Build a PL900 deprecation Warning for smartmatch `~~` (v5.38–v5.41).
///
/// In Perl 5.42+ the operator is feature-gated, not deprecated — use
/// [`make_smartmatch_feature_gate_diagnostic`] for that case.
fn make_smartmatch_diagnostic(node: &Node, declared_version: PerlVersion) -> Diagnostic {
    let message = format!(
        "smartmatch operator `~~` is deprecated starting in Perl v5.38; declared version is v{}.{}",
        declared_version.major, declared_version.minor
    );

    Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message,
        related_information: vec![],
        tags: vec![],
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Replace smartmatch `~~` with `if` / `elsif`, `grep`, or `any` from List::Util; this operator is deprecated in v5.38."
                .to_string(),
        ),
    }
}

/// Build a PL900 feature-gating Warning for smartmatch `~~` (v5.42+).
///
/// Perl 5.42 did not remove smartmatch — it made it feature-gated.
/// The `smartmatch` feature is no longer in the `:5.42` bundle, so an
/// explicit `use feature 'smartmatch';` is required.
fn make_smartmatch_feature_gate_diagnostic(
    node: &Node,
    declared_version: PerlVersion,
) -> Diagnostic {
    let message = format!(
        "smartmatch operator `~~` requires the 'smartmatch' feature, which is not enabled by the v{}.{} bundle; declared version is v{}.{}",
        declared_version.major,
        declared_version.minor,
        declared_version.major,
        declared_version.minor,
    );

    Diagnostic {
        range: (node.location.start, node.location.end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message,
        related_information: vec![],
        tags: vec![],
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Add `use feature 'smartmatch';` to enable the smartmatch operator.".to_string(),
        ),
    }
}

fn make_smartmatch_feature_diagnostic(node: &Node, declared_version: PerlVersion) -> Diagnostic {
    // Routed through the shared helper so a declared version that already meets
    // v5.10 (reachable via `no feature 'switch'`) drops the version clause
    // instead of advising the downgrade `Update 'use v5.36' to 'use v5.10'`.
    make_diagnostic(node, "smartmatch operator `~~`", Some("switch"), declared_version, (5, 10))
}

fn make_diagnostic_with_details(
    node: &Node,
    display: &str,
    declared_version: PerlVersion,
    min_version: (u32, u32),
    severity: DiagnosticSeverity,
    suggestion: Option<String>,
) -> Diagnostic {
    let message = format!(
        "'{}' requires Perl v{}.{}+; declared version is v{}.{}",
        display, min_version.0, min_version.1, declared_version.major, declared_version.minor,
    );

    Diagnostic {
        range: (node.location.start, node.location.end),
        severity,
        code: Some(DiagnosticCode::VersionIncompatFeature.as_str().to_string()),
        message,
        related_information: vec![],
        tags: vec![],
        suggestion,
        fixable: false,
        critic_observation: None,
    }
}

#[cfg(test)]
mod tests {
    use perl_test_must::{must, must_some_with};

    use super::*;
    use perl_parser::Parser;

    fn version_compat_diags(source: &str) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let mut diags = Vec::new();
        check_version_compat(&ast, source, &mut diags);
        diags
    }

    fn version_compat_diags_with_project_version(
        source: &str,
        project_version: Option<&str>,
    ) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let mut diags = Vec::new();
        check_version_compat_with_project_version(&ast, source, &mut diags, project_version);
        diags
    }

    #[test]
    fn project_version_is_used_as_pl900_fallback() {
        let diags =
            version_compat_diags_with_project_version("sub f ($x) { return $x; }", Some("v5.20"));
        assert!(
            diags.iter().any(|diagnostic| diagnostic.message.contains("requires Perl v5.36+")),
            "v5.20 project fallback must remain the effective PL900 target: {diags:?}"
        );
    }

    #[test]
    fn nested_source_version_suppresses_project_fallback() {
        let nested = version_compat_diags_with_project_version(
            "{ use v5.36; sub f ($x) { return $x; } }\n",
            Some("v5.20"),
        );
        assert!(
            nested.iter().all(|diagnostic| !diagnostic
                .message
                .contains("target from project [perl].version")),
            "nested source version must defeat the project fallback: {nested:?}"
        );

        let control =
            version_compat_diags_with_project_version("sub f ($x) { return $x; }\n", Some("v5.20"));
        assert!(
            control.iter().any(|diagnostic| diagnostic
                .message
                .contains("target from project [perl].version")),
            "a file without a source version must use the project fallback: {control:?}"
        );
    }

    #[test]
    fn project_fallback_suggestion_explains_missing_source_version() {
        let project_diags =
            version_compat_diags_with_project_version("sub f ($x) { return $x; }\n", Some("v5.20"));
        let project_diagnostic = must_some_with(
            project_diags.iter().find(|diagnostic| {
                diagnostic.code.as_deref() == Some("PL900")
                    && diagnostic.message.contains("subroutine signatures")
            }),
            "project fallback must emit a signatures diagnostic",
        );
        let project_suggestion = must_some_with(
            project_diagnostic.suggestion.as_deref(),
            "project fallback diagnostic must have a suggestion",
        );
        assert!(project_suggestion.contains("This file declares no `use VERSION`"));
        assert!(project_suggestion.contains("PL900 target v5.20"));
        assert!(project_suggestion.contains("add an explicit `use VERSION`"));
        assert!(project_suggestion.contains("raise `[perl].version`"));

        let source_diags = version_compat_diags_with_project_version(
            "use v5.20;\nsub f ($x) { return $x; }\n",
            Some("v5.40"),
        );
        let source_diagnostic = must_some_with(
            source_diags.iter().find(|diagnostic| {
                diagnostic.code.as_deref() == Some("PL900")
                    && diagnostic.message.contains("subroutine signatures")
            }),
            "source version must emit a signatures diagnostic",
        );
        let source_suggestion = must_some_with(
            source_diagnostic.suggestion.as_deref(),
            "source diagnostic must have a suggestion",
        );
        assert!(!source_suggestion.contains("This file declares no `use VERSION`"));
    }

    #[test]
    fn source_version_wins_over_project_fallback() {
        let diags = version_compat_diags_with_project_version(
            "use v5.40; use builtin 'inf'; my $value = builtin::inf();",
            Some("5.20"),
        );
        assert!(
            !diags.iter().any(|diagnostic| diagnostic.message.contains("project [perl].version"))
        );
    }

    #[test]
    fn invalid_project_version_reports_one_actionable_diagnostic_and_fails_closed() {
        let diags = version_compat_diags_with_project_version(
            "use builtin 'inf'; builtin::inf();",
            Some("not-a-version"),
        );
        assert_eq!(diags.len(), 1, "invalid project values must report once: {diags:?}");
        let diagnostic = &diags[0];
        assert_eq!(diagnostic.code.as_deref(), Some("PL900"));
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert!(diagnostic.message.contains("Invalid project [perl].version"));
        assert!(diagnostic.suggestion.is_some());
        assert!(
            diags.iter().all(|diagnostic| !diagnostic.message.contains("requires Perl")),
            "invalid project values must not run PL900: {diags:?}"
        );
    }

    #[test]
    fn malformed_project_version_suffix_reports_once_without_runtime_fallback() {
        for value in ["5.20.garbage", "5.20.1", " 5.20"] {
            let diags = version_compat_diags_with_project_version(
                "use builtin 'inf'; builtin::inf();",
                Some(value),
            );
            assert_eq!(diags.len(), 1, "malformed value {value:?}: {diags:?}");
            assert!(diags[0].message.contains("Invalid project [perl].version"));
            assert!(
                !diags[0].message.contains("requires Perl"),
                "malformed value {value:?} must not use a runtime/implicit PL900 target"
            );
        }
    }

    #[test]
    fn project_version_rejects_trailing_components_and_patch_suffixes() {
        for value in ["5.20.1", "v5.20.1", "5.20_1", "v5.20_1", " 5.20", "5.20 ", " 5.20 "] {
            let diags =
                version_compat_diags_with_project_version("sub f ($x) { return $x; }", Some(value));
            assert_eq!(diags.len(), 1, "project version {value:?} must report once");
            assert!(diags[0].message.contains("Invalid project [perl].version"));
        }
    }

    #[test]
    fn invalid_project_version_is_ignored_when_source_declares_a_version() {
        let diags = version_compat_diags_with_project_version(
            "use v5.40; use builtin 'inf'; builtin::inf();",
            Some("not-a-version"),
        );
        assert_eq!(
            diags
                .iter()
                .filter(|diagnostic| diagnostic.message.contains("Invalid project [perl].version"))
                .count(),
            1,
            "invalid project values must remain actionable with source authority: {diags:?}"
        );
        assert!(
            diags.iter().all(|diagnostic| !diagnostic.message.contains("requires Perl")),
            "the valid source target should not receive a fallback PL900: {diags:?}"
        );
        assert!(
            diags.iter().all(|diagnostic| !diagnostic.message.contains("target from project")),
            "invalid project values must not become the effective fallback target: {diags:?}"
        );
    }

    #[test]
    fn malformed_source_version_does_not_fall_back_to_project_target() {
        let diags = version_compat_diags_with_project_version(
            "use v5.20.1; use builtin 'inf'; builtin::inf();",
            Some("v5.40"),
        );
        assert!(
            diags.iter().any(|diagnostic| diagnostic.code.as_deref() == Some("PL900")),
            "the parser-supported source spelling must remain authoritative: {diags:?}"
        );
        assert!(
            diags.iter().all(|diagnostic| !diagnostic.message.contains("target from project")),
            "source authority must prevent project fallback on a recovered spelling: {diags:?}"
        );
    }

    #[test]
    fn project_version_v5_40_fallback_is_applied() {
        let below = version_compat_diags_with_project_version(
            "use builtin 'inf'; builtin::inf();",
            Some("v5.38"),
        );
        assert!(below.iter().any(|diagnostic| diagnostic.code.as_deref() == Some("PL900")));

        let supported = version_compat_diags_with_project_version(
            "use builtin 'inf'; builtin::inf();",
            Some("v5.40"),
        );
        assert!(supported.iter().all(|diagnostic| diagnostic.code.as_deref() != Some("PL900")));
    }

    #[test]
    fn project_version_fallback_does_not_enable_lexical_features() {
        let diags =
            version_compat_diags_with_project_version("sub f ($x) { return $x; }", Some("v5.40"));
        assert!(
            diags.iter().any(|diagnostic| diagnostic.message.contains("subroutine signatures")),
            "a fallback target must not enable lexical signature features: {diags:?}"
        );
    }

    #[test]
    fn signatures_need_explicit_feature_before_v5_36() {
        let diags = version_compat_diags("use v5.20;\nsub foo ($x) { return $x; }\n");
        assert!(
            diags.iter().any(|d| d.message.contains("subroutine signatures")),
            "v5.20 without explicit signatures feature should emit PL900 for signature subs"
        );
    }

    #[test]
    fn signatures_explicit_feature_suppresses_version_compat_diagnostic() {
        let diags =
            version_compat_diags("use v5.20;\nuse feature 'signatures';\nsub foo ($x) { $x }\n");
        assert!(
            diags.iter().all(|d| !d.message.contains("subroutine signatures")),
            "explicit signatures feature must suppress PL900 for signature subs on v5.20"
        );
    }

    #[test]
    fn lexical_no_feature_signatures_re_enables_signature_diagnostic() {
        let diags =
            version_compat_diags("use v5.36;\nno feature 'signatures';\nsub foo ($x) { $x }\n");
        assert!(
            diags.iter().any(|d| d.message.contains("subroutine signatures")),
            "lexical no feature signatures must make signature subs warn again"
        );
    }

    #[test]
    fn conditional_no_feature_is_observable_to_version_compat() {
        let diags = version_compat_diags(
            "use v5.36;\nno if 1, 'feature', 'signatures';\nsub foo ($x) { $x }\n",
        );
        assert!(
            diags.iter().any(|d| d.message.contains("subroutine signatures")),
            "conditional no feature should disable signatures for downstream PL900 checks"
        );
    }

    #[test]
    fn eval_string_feature_enable_does_not_affect_version_compat_state() {
        let diags = version_compat_diags(
            "use v5.20;\neval \"use feature 'signatures'\";\nsub foo ($x) { $x }\n",
        );
        assert!(
            diags.iter().any(|d| d.message.contains("subroutine signatures")),
            "eval STRING should not be treated as compile-time signatures enablement"
        );
    }

    #[test]
    fn builtin_named_import_suppresses_call_diagnostic_for_that_symbol() {
        let import_diags =
            version_compat_diags("use v5.38;\nuse builtin 'inf';\nmy $x = builtin::inf();\n");
        assert!(
            import_diags.iter().all(|d| !d.message.contains("builtin::inf")),
            "named builtin import should suppress the separate builtin::inf call diagnostic"
        );

        let no_import_diags = version_compat_diags("use v5.38;\nmy $x = builtin::inf();\n");
        assert!(
            no_import_diags.iter().any(|d| d.message.contains("builtin::inf")),
            "without named import or bundle, builtin::inf should still be version-gated"
        );
    }

    #[test]
    fn builtin_bundle_suppresses_call_diagnostic_but_not_bundle_version_diagnostic() {
        let diags = version_compat_diags("use v5.36;\nuse builtin;\nmy $x = builtin::inf();\n");
        assert!(
            diags.iter().any(|d| d.message.contains("'use builtin' requires Perl v5.40+")),
            "use builtin on v5.36 should emit a bundle version diagnostic"
        );
        assert!(
            diags.iter().all(|d| !d.message.contains("builtin::inf")),
            "declaring the builtin bundle should suppress separate builtin::inf call diagnostics"
        );
    }

    // ---------------------------------------------------------------------------
    // PL900 / Perl 5.42 feature-gating tests (issue #4635)
    //
    // Perl 5.42 did NOT remove given/when/default or smartmatch. Their removal
    // was "indefinitely postponed" and they became feature-gated instead.
    // The deprecation warning was removed in 5.42. The emitter must never
    // produce an Error for these constructs — only Warning.
    //
    // Reference: https://perldoc.perl.org/5.42.0/perldelta — "After extensive
    // discussion their removal has been indefinitely postponed. Using them no
    // longer produces a deprecation warning."
    // ---------------------------------------------------------------------------

    #[test]
    fn given_when_default_v5_42_emits_warning_not_error() {
        let diags =
            version_compat_diags("use v5.42;\ngiven ($x) { when (1) { 1 } default { 0 } }\n");

        // Must emit at least one PL900 diagnostic.
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL900")),
            "v5.42 given/when/default without switch feature should emit PL900"
        );

        for d in &diags {
            if d.code.as_deref() == Some("PL900") {
                assert_ne!(
                    d.severity,
                    DiagnosticSeverity::Error,
                    "PL900 for given/when/default on v5.42 must NOT be Error (feature-gated, not removed)"
                );
                assert!(
                    !d.message.contains("removed"),
                    "PL900 message for given/when/default on v5.42 must not say 'removed': {}",
                    d.message
                );
                assert!(
                    d.message.contains("switch"),
                    "PL900 message for given/when/default on v5.42 should mention the 'switch' feature: {}",
                    d.message
                );
            }
        }
    }

    #[test]
    fn given_when_default_v5_42_with_switch_feature_suppressed() {
        let diags = version_compat_diags(
            "use v5.42;\nuse feature 'switch';\ngiven ($x) { when (1) { 1 } default { 0 } }\n",
        );
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL900")),
            "v5.42 with explicit 'use feature switch' should suppress PL900 for given/when/default"
        );
    }

    #[test]
    fn smartmatch_v5_42_emits_warning_not_error() {
        let diags = version_compat_diags("use v5.42;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n");

        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("PL900")),
            "v5.42 smartmatch without smartmatch feature should emit PL900"
        );

        for d in &diags {
            if d.code.as_deref() == Some("PL900") {
                assert_ne!(
                    d.severity,
                    DiagnosticSeverity::Error,
                    "PL900 for smartmatch on v5.42 must NOT be Error (feature-gated, not removed)"
                );
                assert!(
                    !d.message.contains("removed"),
                    "PL900 message for smartmatch on v5.42 must not say 'removed': {}",
                    d.message
                );
                assert!(
                    d.message.contains("smartmatch"),
                    "PL900 message for smartmatch on v5.42 should mention the 'smartmatch' feature: {}",
                    d.message
                );
            }
        }
    }

    #[test]
    fn smartmatch_v5_42_with_smartmatch_feature_suppressed() {
        let diags = version_compat_diags(
            "use v5.42;\nuse feature 'smartmatch';\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
        );
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL900")),
            "v5.42 with explicit 'use feature smartmatch' should suppress PL900 for ~~"
        );
    }

    #[test]
    fn given_when_default_v5_38_emits_deprecation_warning() {
        let diags =
            version_compat_diags("use v5.38;\ngiven ($x) { when (1) { 1 } default { 0 } }\n");
        let pl900: Vec<_> = diags.iter().filter(|d| d.code.as_deref() == Some("PL900")).collect();
        assert!(
            !pl900.is_empty(),
            "v5.38 given/when/default should emit PL900 deprecation warning"
        );
        for d in &pl900 {
            assert_eq!(
                d.severity,
                DiagnosticSeverity::Warning,
                "PL900 for given/when/default on v5.38 must be Warning"
            );
            assert!(
                d.message.contains("deprecated"),
                "PL900 message for given/when/default on v5.38 should say 'deprecated': {}",
                d.message
            );
        }
    }

    #[test]
    fn no_pl900_at_error_severity_for_any_declared_version() {
        // Regression: PL900 must never be emitted at Error severity.
        // Test across the full range of declared versions.
        let sources = [
            "use v5.10;\ngiven ($x) { when (1) { 1 } }\n",
            "use v5.36;\ngiven ($x) { when (1) { 1 } }\n",
            "use v5.38;\ngiven ($x) { when (1) { 1 } }\n",
            "use v5.40;\ngiven ($x) { when (1) { 1 } }\n",
            "use v5.42;\ngiven ($x) { when (1) { 1 } }\n",
            "use v5.10;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
            "use v5.36;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
            "use v5.38;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
            "use v5.40;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
            "use v5.42;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
        ];

        for source in &sources {
            let diags = version_compat_diags(source);
            for d in &diags {
                if d.code.as_deref() == Some("PL900") {
                    assert_ne!(
                        d.severity,
                        DiagnosticSeverity::Error,
                        "PL900 must never be Error (source: {:?}, message: {})",
                        source,
                        d.message
                    );
                }
            }
        }
    }

    #[test]
    fn pl900_emitted_severity_matches_catalog_severity() {
        // Cross-check: the severity emitted by the version_compat lint must
        // match DiagnosticCode::VersionIncompatFeature.severity() from the
        // catalog. This guards against the emitter and catalog disagreeing.
        let catalog_severity = DiagnosticCode::VersionIncompatFeature.severity();

        let sources = [
            "use v5.42;\ngiven ($x) { when (1) { 1 } default { 0 } }\n",
            "use v5.42;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
            "use v5.38;\ngiven ($x) { when (1) { 1 } default { 0 } }\n",
            "use v5.38;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
            "use v5.10;\ngiven ($x) { when (1) { 1 } }\n",
        ];

        for source in &sources {
            let diags = version_compat_diags(source);
            for d in &diags {
                if d.code.as_deref() == Some("PL900") {
                    assert_eq!(
                        d.severity, catalog_severity,
                        "emitted PL900 severity must match catalog severity (source: {:?})",
                        source
                    );
                }
            }
        }
    }

    // ---------------------------------------------------------------------------
    // `__CLASS__` PL900 version-compat tests (issue #5279)
    //
    // `__CLASS__` was introduced in Perl v5.40 as part of the `class` object
    // system. Compiling it needs BOTH a declared version >= v5.40 (the keyword
    // did not exist in v5.38/v5.39, where the `class` feature already did) and
    // the `class` feature lexically enabled (it is experimental, so no version
    // bundle — `:5.40` included — turns it on). The lint emits PL900 unless both
    // hold, and the remediation names whichever half is missing.
    // ---------------------------------------------------------------------------

    #[test]
    fn class_token_without_class_feature_emits_pl900() {
        let diags = version_compat_diags("use v5.36;\nmy $name = __CLASS__;\n");
        let class_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("PL900") && d.message.contains("__CLASS__"))
            .collect();

        // One occurrence of the token must produce exactly one diagnostic. A
        // duplicate would surface as two identical squiggles on the same span.
        assert_eq!(
            class_diags.len(),
            1,
            "one `__CLASS__` occurrence should emit exactly one PL900: {diags:#?}"
        );
        let d = class_diags[0];

        assert_eq!(
            d.severity,
            DiagnosticSeverity::Warning,
            "__CLASS__ PL900 must be Warning, not Error"
        );

        // The message is the whole UX contract: it must name the construct, the
        // minimum version that supports it, and the declared version that does not.
        assert!(
            d.message.contains("__CLASS__"),
            "__CLASS__ PL900 message should name the construct: {}",
            d.message
        );
        assert!(
            d.message.contains("v5.40"),
            "__CLASS__ PL900 message should name v5.40 as the minimum: {}",
            d.message
        );
        assert!(
            d.message.contains("v5.36"),
            "__CLASS__ PL900 message should name the declared version that lacks it: {}",
            d.message
        );

        // The remediation text is asserted by
        // `class_token_suggestion_does_not_offer_the_feature_pragma`, which owns
        // the `suggestion` field; this test owns severity, count, and `message`.
    }

    #[test]
    fn class_token_with_explicit_class_feature_still_emits_pl900_below_v5_40() {
        // The `class` feature shipped in v5.38; `__CLASS__` did not arrive until
        // v5.40. So `use feature 'class'` on v5.36 does NOT make __CLASS__ work,
        // and the lint must keep warning. Suppressing here would be a false
        // negative on code that genuinely fails to compile on the declared perl.
        let diags =
            version_compat_diags("use v5.36;\nuse feature 'class';\nmy $name = __CLASS__;\n");
        let class_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("PL900") && d.message.contains("__CLASS__"))
            .collect();
        assert!(
            !class_diags.is_empty(),
            "'use feature class' must not suppress PL900 for __CLASS__ below v5.40: {diags:#?}"
        );
        for d in class_diags {
            assert!(
                d.message.contains("v5.40"),
                "__CLASS__ PL900 must name v5.40 as the fix: {}",
                d.message
            );
        }
    }

    /// The `suggestion` field for a given source, or `""` when no `__CLASS__`
    /// PL900 was emitted.
    fn class_token_suggestion(source: &str) -> String {
        version_compat_diags(source)
            .iter()
            .find(|d| d.code.as_deref() == Some("PL900") && d.message.contains("__CLASS__"))
            .and_then(|d| d.suggestion.clone())
            .unwrap_or_default()
    }

    #[test]
    fn class_token_suggestion_never_offers_the_pragma_as_an_alternative() {
        // Asserted on `suggestion`, not `message`: `message` is built by
        // `make_diagnostic_with_details` and always reads "'__CLASS__' requires
        // Perl v5.40+; declared version is vX.Y", so a `message.contains("v5.40")`
        // check passes no matter what the remediation says. The suggestion is the
        // field the author acts on, and it is the field that used to steer them
        // into `use feature 'class'` as a substitute for the version bump.
        //
        // Below v5.40 with no pragma, BOTH halves are missing, so the remediation
        // must ask for both — never "v5.40 *or* the pragma", which would send the
        // author to a pragma that does not make the code compile.
        let suggestion = class_token_suggestion("use v5.36;\nmy $name = __CLASS__;\n");
        assert!(suggestion.contains("v5.40"), "suggestion must name the v5.40 bump: {suggestion}");
        assert!(
            suggestion.contains("use feature \"class\""),
            "suggestion must name the class pragma too: {suggestion}"
        );
        assert!(
            !suggestion.contains(" or "),
            "the pragma must not be offered as an alternative to the version bump: {suggestion}"
        );
    }

    #[test]
    fn class_token_on_v5_40_without_class_feature_emits_pl900() {
        // `class` is experimental, so it is in NO version bundle — `:5.40`
        // included (see `BUNDLE_5_40_FEATURES`). `use v5.40;` alone therefore
        // leaves `__CLASS__` a bareword, which perl rejects under `strict subs`.
        // Suppressing here would approve code that cannot compile.
        let diags = version_compat_diags("use v5.40;\nmy $name = __CLASS__;\n");
        let class_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.code.as_deref() == Some("PL900") && d.message.contains("__CLASS__"))
            .collect();
        assert_eq!(
            class_diags.len(),
            1,
            "'use v5.40' without the class feature should still emit PL900: {diags:#?}"
        );
        // The version half is already satisfied, so the remediation must ask only
        // for the pragma — telling the author to "update to v5.40" here would be
        // the same no-op advice the v5.38 `class` arm still gives.
        let suggestion = class_diags[0].suggestion.clone().unwrap_or_default();
        assert!(
            suggestion.contains("use feature \"class\""),
            "suggestion must ask for the pragma: {suggestion}"
        );
        assert!(
            !suggestion.contains("Update 'use v5.40' to 'use v5.40'"),
            "suggestion must not tell the author to update v5.40 to itself: {suggestion}"
        );
    }

    #[test]
    fn class_token_on_v5_40_with_class_feature_suppresses_pl900() {
        // Both halves satisfied: declared version >= v5.40 and the experimental
        // `class` feature explicitly enabled. This is the only source shape that
        // actually compiles, so it is the only one that must be silent.
        let diags =
            version_compat_diags("use v5.40;\nuse feature 'class';\nmy $name = __CLASS__;\n");
        assert!(
            diags
                .iter()
                .all(|d| !(d.code.as_deref() == Some("PL900") && d.message.contains("__CLASS__"))),
            "v5.40 + 'use feature class' compiles and must not emit PL900: {diags:#?}"
        );
    }

    #[test]
    fn class_token_without_declared_version_no_pl900() {
        let diags = version_compat_diags("my $name = __CLASS__;\n");
        assert!(
            diags
                .iter()
                .all(|d| !(d.code.as_deref() == Some("PL900") && d.message.contains("__CLASS__"))),
            "no declared version should suppress all PL900 checks including __CLASS__"
        );
    }

    // ---------------------------------------------------------------------------
    // PL900 remediation for never-bundled experimental features (issue #5279)
    //
    // `class` (v5.38), `defer` (v5.36) and `try` (v5.34) are experimental, so no
    // `use vX.Y` bundle turns them on — `:5.40` and `:5.42` included (see
    // `BUNDLE_5_40_FEATURES` / `BUNDLE_5_42_FEATURES` in `perl-pragma`, neither of
    // which lists `class` or `defer`). Compiling any of them needs BOTH a declared
    // version at or above the release that introduced the feature AND the pragma
    // lexically enabled, so a version bump is never an *alternative* to the pragma
    // and at or above the minimum it is a no-op.
    //
    // Verified against perl v5.38.2 (`perl -c`):
    //
    //   use v5.38; class Foo {}                             syntax error
    //   use v5.38; use feature 'class'; class Foo {}         syntax OK
    //   use v5.36; defer { 1 }                               syntax error
    //   use v5.36; use feature 'defer'; defer { 1 }          syntax OK
    //   use v5.34; try { 1 } catch ($e) { 2 }                syntax error
    //   use v5.34; use feature 'try'; try {1} catch($e){2}   syntax OK
    //   use feature 'try/catch'                              Feature "try/catch"
    //                                                        is not supported
    //   use feature 'field'                                  Feature "field"
    //                                                        is not supported
    //
    // The last two are why the remediation must name a *real* feature name, and
    // why the declared version gates the pragma as well: perl rejects
    // `use feature 'NAME'` outright when NAME postdates the running perl, so
    // `use feature 'class'` cannot backport `class` onto a v5.36 target any more
    // than `use feature 'field'` works on v5.38.2.
    //
    // These tests assert on `suggestion` — the field the author acts on. A test
    // that only checked "PL900 emitted" would pass with `Update 'use v5.38' to
    // 'use v5.38'` intact.
    // ---------------------------------------------------------------------------

    /// The `suggestion` of the first PL900 whose message names `construct`, or
    /// `""` when no such diagnostic was emitted.
    fn pl900_suggestion(source: &str, construct: &str) -> String {
        version_compat_diags(source)
            .iter()
            .find(|d| d.code.as_deref() == Some("PL900") && d.message.contains(construct))
            .and_then(|d| d.suggestion.clone())
            .unwrap_or_default()
    }

    /// Whether any PL900 naming `construct` was emitted for `source`.
    fn pl900_emitted(source: &str, construct: &str) -> bool {
        version_compat_diags(source)
            .iter()
            .any(|d| d.code.as_deref() == Some("PL900") && d.message.contains(construct))
    }

    #[test]
    fn class_on_v5_38_remediation_does_not_advise_updating_a_version_to_itself() {
        // The reported defect: `use v5.38;` already meets the `class` minimum, so
        // the version half of the old "Update X to Y or add pragma" remediation
        // collapsed into `Update 'use v5.38' to 'use v5.38'`.
        let suggestion = pl900_suggestion("use v5.38;\nclass Foo { }\n", "'class'");
        assert!(
            !suggestion.is_empty(),
            "`use v5.38;` + `class Foo {{}}` is a syntax error on perl v5.38.2 and must emit PL900"
        );
        assert!(
            !suggestion.contains("'use v5.38' to 'use v5.38'"),
            "remediation must not tell the author to update v5.38 to itself: {suggestion}"
        );
        assert!(
            suggestion.contains("use feature \"class\""),
            "remediation must name the pragma, the only change that makes it compile: {suggestion}"
        );
        assert!(
            !suggestion.contains(" or "),
            "a version bump must not be offered as an alternative to the pragma: {suggestion}"
        );
    }

    #[test]
    fn class_above_v5_38_remediation_does_not_advise_a_version_downgrade() {
        // At v5.40 the old remediation read `Update 'use v5.40' to 'use v5.38'` —
        // a downgrade that still would not enable the feature.
        let suggestion = pl900_suggestion("use v5.40;\nclass Foo { }\n", "'class'");
        assert!(!suggestion.is_empty(), "`use v5.40;` alone does not enable `class`; expect PL900");
        assert!(
            !suggestion.contains("to 'use v5.38'"),
            "remediation must not advise downgrading v5.40 to v5.38: {suggestion}"
        );
        assert!(
            suggestion.contains("use feature \"class\""),
            "remediation must name the pragma: {suggestion}"
        );
    }

    #[test]
    fn class_below_v5_38_remediation_asks_for_both_halves() {
        let suggestion = pl900_suggestion("use v5.36;\nclass Foo { }\n", "'class'");
        assert!(!suggestion.is_empty(), "`use v5.36;` + `class` must emit PL900");
        assert!(suggestion.contains("v5.38"), "remediation must name the v5.38 bump: {suggestion}");
        assert!(
            suggestion.contains("use feature \"class\""),
            "remediation must name the pragma too: {suggestion}"
        );
        assert!(
            !suggestion.contains(" or "),
            "neither half alone compiles, so they must not be offered as alternatives: \
             {suggestion}"
        );
    }

    #[test]
    fn class_below_v5_38_with_pragma_still_emits_pl900() {
        // perl rejects `use feature 'NAME'` when NAME postdates the running perl
        // (v5.38.2: `Feature "field" is not supported by Perl 5.38.2`), so the
        // pragma cannot backport `class` onto a v5.36 target. Staying silent here
        // would approve code that cannot compile on the declared perl.
        let source = "use v5.36;\nuse feature 'class';\nclass Foo { }\n";
        assert!(
            pl900_emitted(source, "'class'"),
            "`use feature 'class'` must not suppress PL900 below v5.38"
        );
        let suggestion = pl900_suggestion(source, "'class'");
        assert!(
            suggestion.contains("v5.38"),
            "the pragma is already present, so the remediation must name the version bump: \
             {suggestion}"
        );
    }

    #[test]
    fn class_on_v5_38_with_pragma_suppresses_pl900() {
        // The only shape that compiles — verified `syntax OK` on perl v5.38.2.
        assert!(
            !pl900_emitted("use v5.38;\nuse feature 'class';\nclass Foo { }\n", "'class'"),
            "v5.38 + `use feature 'class'` compiles and must be silent"
        );
    }

    #[test]
    fn defer_on_v5_36_remediation_does_not_advise_updating_a_version_to_itself() {
        let suggestion = pl900_suggestion("use v5.36;\ndefer { 1 }\n", "'defer'");
        assert!(
            !suggestion.is_empty(),
            "`use v5.36;` + `defer {{ }}` is a syntax error on perl v5.38.2 and must emit PL900"
        );
        assert!(
            !suggestion.contains("'use v5.36' to 'use v5.36'"),
            "remediation must not tell the author to update v5.36 to itself: {suggestion}"
        );
        assert!(
            suggestion.contains("use feature \"defer\""),
            "remediation must name the pragma: {suggestion}"
        );
        assert!(
            !suggestion.contains(" or "),
            "a version bump must not be offered as an alternative to the pragma: {suggestion}"
        );
    }

    #[test]
    fn defer_above_v5_36_remediation_does_not_advise_a_version_downgrade() {
        // `defer` is in no bundle, so `use v5.40;` alone still leaves it a syntax
        // error — verified on perl v5.38.2 for `use v5.38; defer { 1 }`.
        let suggestion = pl900_suggestion("use v5.40;\ndefer { 1 }\n", "'defer'");
        assert!(!suggestion.is_empty(), "`use v5.40;` alone does not enable `defer`; expect PL900");
        assert!(
            !suggestion.contains("to 'use v5.36'"),
            "remediation must not advise downgrading v5.40 to v5.36: {suggestion}"
        );
    }

    #[test]
    fn defer_on_v5_36_with_pragma_suppresses_pl900() {
        // Verified `syntax OK` on perl v5.38.2.
        assert!(
            !pl900_emitted("use v5.36;\nuse feature 'defer';\ndefer { 1 }\n", "'defer'"),
            "v5.36 + `use feature 'defer'` compiles and must be silent"
        );
    }

    #[test]
    fn try_catch_remediation_names_the_real_try_feature() {
        // The old remediation reused the *display* name `try/catch` as the pragma
        // name. `use feature 'try/catch';` is a hard error on perl v5.38.2
        // (`Feature "try/catch" is not supported`), so following the advice
        // replaced a syntax error with a failed BEGIN block. The real feature name
        // is `try` (see `ALL_KNOWN_FEATURES` in `perl-pragma`).
        let suggestion =
            pl900_suggestion("use v5.34;\ntry { 1 } catch ($e) { 2 }\n", "'try/catch'");
        assert!(!suggestion.is_empty(), "`use v5.34;` + try/catch must emit PL900");
        assert!(
            !suggestion.contains("use feature \"try/catch\""),
            "remediation must not name a feature perl rejects: {suggestion}"
        );
        assert!(
            suggestion.contains("use feature \"try\""),
            "remediation must name the real `try` feature: {suggestion}"
        );
        assert!(
            !suggestion.contains("'use v5.34' to 'use v5.34'"),
            "remediation must not tell the author to update v5.34 to itself: {suggestion}"
        );
    }

    #[test]
    fn try_catch_above_v5_34_remediation_does_not_advise_a_version_downgrade() {
        let suggestion =
            pl900_suggestion("use v5.38;\ntry { 1 } catch ($e) { 2 }\n", "'try/catch'");
        assert!(!suggestion.is_empty(), "`use v5.38;` alone does not enable `try`; expect PL900");
        assert!(
            !suggestion.contains("to 'use v5.34'"),
            "remediation must not advise downgrading v5.38 to v5.34: {suggestion}"
        );
    }

    #[test]
    fn try_catch_with_pragma_suppresses_pl900() {
        // Verified `syntax OK` on perl v5.38.2.
        assert!(
            !pl900_emitted(
                "use v5.34;\nuse feature 'try';\ntry { 1 } catch ($e) { 2 }\n",
                "'try/catch'"
            ),
            "v5.34 + `use feature 'try'` compiles and must be silent"
        );
    }

    #[test]
    fn try_catch_on_v5_40_bundle_suppresses_pl900() {
        // `try` is the one of the three that a version bundle does enable:
        // `BUNDLE_5_40_FEATURES` in `perl-pragma` lists it. Guards against the
        // version gate being tightened into a false positive at v5.40+.
        assert!(
            !pl900_emitted("use v5.40;\ntry { 1 } catch ($e) { 2 }\n", "'try/catch'"),
            "the v5.40 bundle enables `try`, so v5.40 alone must be silent"
        );
    }

    #[test]
    fn experimental_feature_remediation_never_advises_a_useless_version_change() {
        // One invariant over the whole never-bundled family and the full declared
        // version range: whenever a remediation says `Update 'use vA.B' to
        // 'use vC.D'`, the target must be strictly newer than the declared
        // version. Anything else is a no-op or a downgrade.
        //
        // Covers the pragma-gated arms and the bundled arms reachable through
        // `no feature`, which produce the same shape via the shared
        // `make_diagnostic` helper.
        for source in PL900_REMEDIATION_SOURCES {
            for d in version_compat_diags(source) {
                if d.code.as_deref() != Some("PL900") {
                    continue;
                }
                let Some(suggestion) = d.suggestion.as_deref() else {
                    continue;
                };
                if let Some((declared, target)) = parse_version_bump(suggestion) {
                    assert!(
                        target > declared,
                        "remediation advises a no-op or downgrading version change \
                         (source: {source:?}, suggestion: {suggestion})"
                    );
                }
            }
        }
    }

    /// Extract `(declared, target)` from a `Update 'use vA.B' to 'use vC.D'`
    /// clause, or `None` when the suggestion carries no version bump.
    fn parse_version_bump(suggestion: &str) -> Option<(PerlVersion, PerlVersion)> {
        let rest = suggestion.split_once("Update 'use ")?.1;
        let (declared, rest) = rest.split_once("' to 'use ")?;
        let (target, _) = rest.split_once('\'')?;
        Some((parse_perl_version(declared)?, parse_perl_version(target)?))
    }

    /// Sources driving the three pragma-gated arms this claim owns, across the
    /// declared-version range.
    ///
    /// Covers every PL900 arm that carries a remediation, through both the
    /// pragma-gated helper ([`make_experimental_feature_diagnostic`]) and the
    /// shared bundled-feature helper ([`make_diagnostic`]).
    ///
    /// The `no feature` rows are load-bearing: they are how a *bundled* feature
    /// reaches the shared helper with a declared version that already meets the
    /// minimum, which is what produced `Update 'use v5.36' to 'use v5.36'`
    /// there, and `Update 'use v5.36' to 'use v5.10'` — an outright downgrade —
    /// for `say`.
    const PL900_REMEDIATION_SOURCES: &[&str] = &[
        // Pragma-gated: never bundled (`class`, `defer`), or bundled only later
        // (`try`, at v5.40).
        "use v5.10;\nclass Foo { }\n",
        "use v5.36;\nclass Foo { }\n",
        "use v5.38;\nclass Foo { }\n",
        "use v5.40;\nclass Foo { }\n",
        "use v5.42;\nclass Foo { }\n",
        "use v5.36;\nuse feature 'class';\nclass Foo { }\n",
        "use v5.10;\ndefer { 1 }\n",
        "use v5.36;\ndefer { 1 }\n",
        "use v5.40;\ndefer { 1 }\n",
        "use v5.36;\nuse feature 'defer';\ndefer { 1 }\n",
        "use v5.10;\ntry { 1 } catch ($e) { 2 }\n",
        "use v5.34;\ntry { 1 } catch ($e) { 2 }\n",
        "use v5.38;\ntry { 1 } catch ($e) { 2 }\n",
        // Bundled features below their minimum: the version bump is a genuine
        // remediation here, so both halves of the advice must be usable.
        "use v5.8;\nsub f ($x) { $x }\n",
        "use v5.20;\nsub f ($x) { $x }\n",
        "use v5.8;\nsay 'hi';\n",
        "use v5.8;\nstate $x = 1;\n",
        "use v5.8;\nmy $r = [];\nmy @a = $r->@*;\n",
        "use v5.8;\nmy $o = bless {}, 'X';\nmy $b = $o isa 'X';\n",
        "use v5.8;\ngiven ($x) { when (1) { 1 } }\n",
        "use v5.8;\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
        // Bundled features switched off by `no feature` at or above the
        // minimum — the shared helper's self-referential / downgrading path.
        "use v5.36;\nno feature 'signatures';\nsub f ($x) { $x }\n",
        "use v5.36;\nno feature 'say';\nsay 'hi';\n",
        "use v5.36;\nno feature 'state';\nstate $x = 1;\n",
        "use v5.36;\nno feature 'isa';\nmy $o = bless {}, 'X';\nmy $b = $o isa 'X';\n",
        // NOTE: no `no feature 'postderef_qq'` row — non-interpolated postfix
        // dereference is unconditional from v5.24, so that source is now
        // required to be silent (pinned by
        // `lexical_no_feature_postderef_qq_keeps_v5_24_silent_for_every_postfix_spelling`).
        // smartmatch's own downgrade path: below v5.38 with `switch` off, the
        // arm routes through `make_smartmatch_feature_diagnostic`. Without this
        // row only its below-minimum path was covered, so the
        // `Update 'use v5.36' to 'use v5.10'` downgrade went unpinned.
        "use v5.36;\nno feature 'switch';\nmy $x = 1;\nmy $y = 2;\n$x ~~ $y;\n",
        // `builtin`: no `use feature` pragma enables these at all.
        "use v5.36;\nmy $x = builtin::inf();\n",
        "use v5.36;\nuse builtin;\n",
        "use v5.36;\nuse builtin 'inf';\n",
        "use v5.10;\nuse feature 'try';\ntry { 1 } catch ($e) { 2 }\n",
    ];

    /// Every feature name perl itself accepts in `use feature '...'`.
    ///
    /// Mirrors `ALL_KNOWN_FEATURES` in `perl-pragma`. Spot-checked against perl
    /// v5.38.2, which rejects anything outside this set with
    /// `Feature "NAME" is not supported by Perl 5.38.2` — including the three
    /// names this lint used to emit: `try/catch`, `subroutine signatures`, and
    /// `postfix_deref`.
    const REAL_PERL_FEATURE_NAMES: &[&str] = &[
        "say",
        "state",
        "smartmatch",
        "switch",
        "unicode_strings",
        "unicode_eval",
        "evalbytes",
        "current_sub",
        "fc",
        "lexical_subs",
        "postderef",
        "postderef_qq",
        "signatures",
        "refaliasing",
        "bitwise",
        "declared_refs",
        "isa",
        "indirect",
        "multidimensional",
        "bareword_filehandles",
        "try",
        "defer",
        "extra_paired_delimiters",
        "module_true",
        "class",
        "field",
        "method",
        "apostrophe_as_package_separator",
        "keyword_any",
        "keyword_all",
    ];

    #[test]
    fn pragma_gated_message_and_suggestion_never_contradict_each_other() {
        // Review finding (#5544, factory-droid P1): for `use v5.40; class Foo {}`
        // the message read `'class' requires Perl v5.38+; declared version is
        // v5.40` while the suggestion correctly asked only for the pragma. The
        // diagnostic simultaneously asserted a version problem and denied the
        // version half mattered, which reads as a false positive on code that
        // genuinely does not compile.
        //
        // The rule: a message may cite a minimum version only when the declared
        // version actually falls short of it. Message and suggestion are built
        // from one `(version_ok, feature_ok)` match so they cannot drift.
        for source in PL900_REMEDIATION_SOURCES {
            for d in version_compat_diags(source) {
                if d.code.as_deref() != Some("PL900") {
                    continue;
                }
                let Some(suggestion) = d.suggestion.as_deref() else {
                    continue;
                };
                let suggestion_asks_for_a_bump = suggestion.contains("Update 'use ");
                let message_cites_a_minimum = d.message.contains("requires Perl v");

                assert_eq!(
                    message_cites_a_minimum, suggestion_asks_for_a_bump,
                    "message and suggestion disagree about whether the declared version is the \
                     problem (source: {source:?}, message: {}, suggestion: {suggestion})",
                    d.message
                );
            }
        }
    }

    #[test]
    fn postfix_deref_remediation_names_postderef_and_actually_silences_the_lint() {
        // Review finding (codex P2 on #5559). `postderef` governs the `$r->@*`
        // syntax this arm matches; `postderef_qq` only extends it to
        // double-quotish interpolation. Verified on perl v5.38.2:
        //
        //   no feature 'postderef_qq'; print "$r->@*"  -> ARRAY(0x..)->@*
        //   no feature 'postderef';    my @a = $r->@*  -> still works
        //
        // and perl's own bundles gain `postderef_qq` only at `:5.24`, so naming
        // it to an author targeting v5.20 is advice about the wrong feature on a
        // version that does not have it.
        let source = "use v5.20;\nmy $r = [];\nmy @a = $r->@*;\n";
        let suggestion = pl900_suggestion(source, "postfix deref");
        assert!(!suggestion.is_empty(), "v5.20 postfix deref should emit PL900");
        assert!(
            suggestion.contains("use feature \"postderef\""),
            "remediation must name the feature that governs the operator: {suggestion}"
        );
        assert!(
            !suggestion.contains("postderef_qq"),
            "postderef_qq is the interpolation switch, not this construct: {suggestion}"
        );

        // Advice that is correct but leaves the warning up is still a defect:
        // the author follows it and nothing changes. Guards the `has_feature`
        // query, which canonicalizes `postfix_deref` to `postderef_qq` and so
        // would not otherwise see an explicit `use feature 'postderef';`.
        assert!(
            !pl900_emitted(
                "use v5.20;\nuse feature 'postderef';\nmy $r = [];\nmy @a = $r->@*;\n",
                "postfix deref"
            ),
            "following the remediation must silence the diagnostic"
        );

        // The bundle path must keep working: perl's `:5.24`+ bundles carry
        // `postderef_qq`, and the operator is unconditional from v5.24.
        assert!(
            !pl900_emitted("use v5.24;\nmy $r = [];\nmy @a = $r->@*;\n", "postfix deref"),
            "the v5.24 bundle enables postfix deref, so v5.24 alone must be silent"
        );
    }

    #[test]
    fn every_pl900_remediation_names_a_feature_perl_accepts() {
        // The second half of the defect: the shared helper interpolated the
        // construct's *display* name into the `use feature "..."` slot, so
        // following the advice replaced the original error with
        // `Feature "subroutine signatures" is not supported by Perl 5.38.2`.
        // Any pragma this lint tells an author to add must be a name perl will
        // actually accept.
        for source in PL900_REMEDIATION_SOURCES {
            for d in version_compat_diags(source) {
                if d.code.as_deref() != Some("PL900") {
                    continue;
                }
                let Some(suggestion) = d.suggestion.as_deref() else {
                    continue;
                };
                for named in named_features(suggestion) {
                    assert!(
                        REAL_PERL_FEATURE_NAMES.contains(&named.as_str()),
                        "remediation names '{named}', which perl rejects \
                         (source: {source:?}, suggestion: {suggestion})"
                    );
                }
            }
        }
    }

    /// Every `NAME` appearing in a `use feature "NAME"` clause of `suggestion`.
    fn named_features(suggestion: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = suggestion;
        while let Some((_, after)) = rest.split_once("use feature \"") {
            match after.split_once('"') {
                Some((name, tail)) => {
                    out.push(name.to_string());
                    rest = tail;
                }
                None => break,
            }
        }
        out
    }

    // ---------------------------------------------------------------------------
    // PL900 non-interpolated postfix-dereference spellings (issue #13761)
    //
    // The lint claimed the Perl 5.20 postfix-dereference family but matched
    // only three `Unary` spellings plus two phantom op strings the parser never
    // emits. The actual parser-emitted shapes are:
    //
    //   ->$*       Unary("->$*")
    //   ->$#*      Unary("->$#*")
    //   ->@*       Unary("->@*")
    //   ->@[...]   Binary("->@[]")
    //   ->@{...}   HashSlice (postfix receiver)
    //   ->%*       Unary("->%*")
    //   ->%{...}   Binary("->%{}")
    //   ->&*       Unary("->&*")
    //   ->**       Unary("->**")
    //
    // `HashSlice` is shared with the ordinary (`@hash{...}`) and legacy prefix
    // (`@$href{...}`, `@{$href}{...}`) slice families, so classification must be
    // source-aware over exact child ranges — never kind-only. These tests pin
    // both directions: every postfix spelling warns on v5.20–v5.23 and goes
    // quiet under the `postderef` remediation, while every legacy form stays
    // silent.
    // ---------------------------------------------------------------------------

    /// One entry per non-interpolated postfix-dereference spelling, written as
    /// an expression over a predeclared `$r` reference.
    const POSTFIX_DEREF_SPELLINGS: &[&str] = &[
        "$r->$*",
        "$r->$#*",
        "$r->@*",
        "$r->@[0, 1]",
        "$r->@{qw(a b)}",
        "$r->%*",
        "$r->%{a => 1}",
        "$r->&*",
        "$r->**",
    ];

    /// A v5.20 source using one postfix-dereference `spelling`.
    fn postfix_spelling_source(spelling: &str) -> String {
        format!("use v5.20;\nmy $r = [];\nmy $x = {spelling};\n")
    }

    /// The PL900 diagnostics whose message names the postfix-deref construct.
    fn postfix_pl900s(source: &str) -> Vec<Diagnostic> {
        version_compat_diags(source)
            .into_iter()
            .filter(|d| d.code.as_deref() == Some("PL900") && d.message.contains("postfix deref"))
            .collect()
    }

    #[test]
    fn every_non_interpolated_postfix_spelling_emits_one_pl900_on_v5_20() {
        for spelling in POSTFIX_DEREF_SPELLINGS {
            let source = postfix_spelling_source(spelling);
            let diags = postfix_pl900s(&source);
            assert_eq!(
                diags.len(),
                1,
                "`{spelling}` on v5.20 should emit exactly one PL900: {diags:#?}"
            );
            let d = &diags[0];
            let expr_start = must_some_with(source.find(spelling), "spelling present in source");
            let expr_end = expr_start + spelling.len();
            assert_eq!(
                d.severity,
                DiagnosticSeverity::Warning,
                "PL900 for `{spelling}` must be Warning, not Error"
            );
            // The diagnostic range is the exact postfix-dereference
            // expression for every row: the slice nodes already span the full
            // expression, and the classifier recovers the star forms' ends
            // from the source because their `Unary` nodes end at the
            // receiver.
            assert_eq!(
                d.range,
                (expr_start, expr_end),
                "the PL900 range must be the exact postfix-dereference expression \
                 `{spelling}`"
            );
        }
    }

    #[test]
    fn every_postfix_spelling_remediation_names_postderef() {
        for spelling in POSTFIX_DEREF_SPELLINGS {
            let source = postfix_spelling_source(spelling);
            for d in postfix_pl900s(&source) {
                let suggestion = d.suggestion.as_deref().unwrap_or_default();
                assert!(
                    suggestion.contains("use feature \"postderef\""),
                    "`{spelling}` remediation must name the `postderef` feature: {suggestion}"
                );
                assert!(
                    !suggestion.contains("postderef_qq"),
                    "`{spelling}` remediation must not name the interpolation switch: \
                     {suggestion}"
                );
            }
        }
    }

    #[test]
    fn explicit_postderef_pragma_silences_every_postfix_spelling() {
        for spelling in POSTFIX_DEREF_SPELLINGS {
            let source =
                format!("use v5.20;\nuse feature 'postderef';\nmy $r = [];\nmy $x = {spelling};\n");
            let diags = postfix_pl900s(&source);
            assert!(
                diags.is_empty(),
                "following the remediation must silence `{spelling}`: {diags:#?}"
            );
        }
    }

    #[test]
    fn v5_24_bundle_is_silent_for_every_postfix_spelling() {
        for spelling in POSTFIX_DEREF_SPELLINGS {
            let source = format!("use v5.24;\nmy $r = [];\nmy $x = {spelling};\n");
            let diags = version_compat_diags(&source);
            assert!(
                diags.iter().all(|d| d.code.as_deref() != Some("PL900")),
                "the v5.24 bundle enables postfix dereference; `{spelling}` must be silent: \
                 {diags:#?}"
            );
        }
    }

    #[test]
    fn no_declared_version_is_silent_for_every_postfix_spelling() {
        for spelling in POSTFIX_DEREF_SPELLINGS {
            let source = format!("my $r = [];\nmy $x = {spelling};\n");
            let diags = version_compat_diags(&source);
            assert!(
                diags.iter().all(|d| d.code.as_deref() != Some("PL900")),
                "an undeclared version is out of PL900's scope; `{spelling}` must be silent: \
                 {diags:#?}"
            );
        }
    }

    #[test]
    fn lexical_no_feature_postderef_qq_keeps_v5_24_silent_for_every_postfix_spelling() {
        // Non-interpolated postfix dereference became unconditional in v5.24
        // (perldelta 5.24), and `postderef_qq` governs only double-quotish
        // interpolation. Disabling that interpolation switch lexically must
        // not resurrect PL900 for any non-interpolated spelling.
        for spelling in POSTFIX_DEREF_SPELLINGS {
            let source = format!(
                "use v5.24;\nno feature 'postderef_qq';\nmy $r = [];\nmy $x = {spelling};\n"
            );
            let diags = version_compat_diags(&source);
            assert!(
                diags.iter().all(|d| d.code.as_deref() != Some("PL900")),
                "`no feature 'postderef_qq'` cannot disable unconditional non-interpolated \
                 syntax; `{spelling}` must stay silent on v5.24: {diags:#?}"
            );
        }
    }

    #[test]
    fn lexical_no_feature_postderef_re_enables_below_the_unconditional_floor() {
        // Inside the v5.20–v5.23 window the syntax is still feature-gated, so
        // a lexical `no feature 'postderef'` must switch PL900 back on at the
        // operator offset — while the occurrence outside that scope stays
        // silent under the pragma.
        let source = "use v5.20;\nuse feature 'postderef';\nmy $r = [];\n{\n    no feature \
                      'postderef';\n    my @a = $r->@*;\n}\nmy @b = $r->@*;\n";
        let diags = postfix_pl900s(source);
        assert_eq!(
            diags.len(),
            1,
            "only the occurrence inside the `no feature` scope warns: {diags:#?}"
        );
        let disabled_start = must_some_with(source.find("$r->@*"), "in-scope spelling present");
        assert_eq!(
            diags[0].range,
            (disabled_start, disabled_start + "$r->@*".len()),
            "the warning must sit on the in-scope-disabled occurrence"
        );
    }

    #[test]
    fn inter_token_trivia_spellings_emit_one_exact_pl900() {
        // perlderef permits whitespace and `#` line comments between `->` and
        // the dereference character, and the parser skips that trivia while
        // still producing the same node shapes. Every trivia variant must warn
        // exactly once, over the exact written expression.
        let variants = [
            "$r-> @{qw(a b)}",
            "$r ->@{qw(a b)}",
            "$r-> @ {qw(a b)}",
            "$r-># slice comment\n@{qw(a b)}",
            "$r-> @*",
            "$r ->@*",
            "$r-> @[0, 1]",
            "$r-> %{a => 1}",
        ];
        for spelling in variants {
            let source = postfix_spelling_source(spelling);
            let diags = postfix_pl900s(&source);
            assert_eq!(
                diags.len(),
                1,
                "trivia variant `{spelling}` should emit exactly one PL900: {diags:#?}"
            );
            let expr_start = must_some_with(source.find(spelling), "spelling present in source");
            assert_eq!(
                diags[0].range,
                (expr_start, expr_start + spelling.len()),
                "the PL900 range must cover the exact written expression `{spelling}`"
            );
        }
    }

    #[test]
    fn parser_emits_the_pinned_identity_for_every_postfix_spelling() {
        // The classifier matches parser-emitted operator identities exactly;
        // this test is the single point of failure if the parser ever changes
        // them — a normalization, a re-spelled op, a re-spanned arrow chain —
        // instead of the lint silently going quiet. The spaced rows pin the
        // parser's trivia normalization of the op identity.
        let expected: &[(&str, &str, &str)] = &[
            ("$r->$*", "Unary", "->$*"),
            ("$r->$#*", "Unary", "->$#*"),
            ("$r->@*", "Unary", "->@*"),
            ("$r-> @*", "Unary", "->@*"),
            ("$r->@[0, 1]", "Binary", "->@[]"),
            ("$r-> @[0, 1]", "Binary", "->@[]"),
            ("$r->@{qw(a b)}", "HashSlice", ""),
            ("$r-> @{qw(a b)}", "HashSlice", ""),
            ("$r->%*", "Unary", "->%*"),
            ("$r->%{a => 1}", "Binary", "->%{}"),
            ("$r->&*", "Unary", "->&*"),
            ("$r->**", "Unary", "->**"),
        ];
        for &(spelling, kind, op) in expected {
            let source = format!("my $r = [];\nmy $x = {spelling};\n");
            let ast = must(Parser::new(&source).parse());
            let mut found = false;
            walk_node(&ast, &mut |n| {
                found |= match (&n.kind, kind) {
                    (NodeKind::Unary { op: actual, .. }, "Unary") => actual == op,
                    (NodeKind::Binary { op: actual, .. }, "Binary") => actual == op,
                    (NodeKind::HashSlice { .. }, "HashSlice") => true,
                    _ => false,
                };
            });
            assert!(found, "the parser must emit {kind}({op:?}) for `{spelling}`");
        }
    }

    #[test]
    fn lexical_postderef_scopes_follow_the_operator_offset() {
        // The inner block enables the feature lexically; the postfix form after
        // the block closes is outside that scope and must still warn, with no
        // duplicate squiggles from the inner occurrence.
        let source = "use v5.20;\nmy $r = [];\n{\n    use feature 'postderef';\n    my @a = \
                      $r->@*;\n}\nmy @b = $r->@[0, 1];\n";
        let diags = postfix_pl900s(source);
        assert_eq!(diags.len(), 1, "only the out-of-scope occurrence warns: {diags:#?}");
        let expr_start = must_some_with(source.find("$r->@[0, 1]"), "outer spelling present");
        assert_eq!(
            diags[0].range,
            (expr_start, expr_start + "$r->@[0, 1]".len()),
            "the warning must sit on the out-of-scope occurrence"
        );
    }

    #[test]
    fn multiple_postfix_occurrences_emit_one_diagnostic_each() {
        let source = "use v5.20;\nmy $r = [];\nmy @a = $r->@*;\nmy @b = $r->@[0, 1];\nmy $c = \
                      $r->$#*;\n";
        let diags = postfix_pl900s(source);
        assert_eq!(
            diags.len(),
            3,
            "one diagnostic per occurrence, no duplicate squiggles: {diags:#?}"
        );
        let mut ranges: Vec<_> = diags.iter().map(|d| d.range).collect();
        ranges.sort_unstable();
        ranges.dedup();
        assert_eq!(ranges.len(), 3, "each occurrence squiggles its own range");
    }

    /// Legacy slice and pre-5.20 arrow forms that must stay silent. The slice
    /// rows share `HashSlice`/`KeyValueSlice`/`ArraySlice` node kinds with the
    /// postfix `->@{...}` spelling, so they are the kind-only-overmatch
    /// controls; the spaced slice rows additionally guard against a
    /// classifier that over-deletes gap trivia and starts seeing an arrow
    /// where none was written; the arrow rows pin the pre-5.20 element-access
    /// family.
    const LEGACY_SILENT_FORMS: &[&str] = &[
        "@hash{qw(a b)}",
        "@hash {qw(a b)}",
        "%hash{a => 1}",
        "@$href{qw(a b)}",
        "@$href {qw(a b)}",
        "@{$href}{qw(a b)}",
        "@{$href} {qw(a b)}",
        "%$href{a => 1}",
        "%{$href}{a => 1}",
        "$r->[0]",
        "$r->{k}",
        "$r->(1)",
    ];

    /// A v5.20 source using one legacy (non-postfix) `form` over predeclared
    /// variables.
    fn legacy_silent_source(form: &str) -> String {
        format!(
            "use v5.20;\nmy %hash = (a => 1, b => 2);\nmy $href = \\%hash;\nmy $r = [];\nmy $x \
             = {form};\n"
        )
    }

    #[test]
    fn legacy_slice_and_element_forms_emit_no_pl900() {
        for form in LEGACY_SILENT_FORMS {
            let source = legacy_silent_source(form);
            let diags = version_compat_diags(&source);
            assert!(
                diags.iter().all(|d| d.code.as_deref() != Some("PL900")),
                "`{form}` predates or falls outside postfix dereference and must stay silent: \
                 {diags:#?}"
            );
        }
    }

    #[test]
    fn text_mentions_of_postfix_spellings_do_not_trigger_pl900() {
        // Mutation control against whole-document substring search: the lint
        // classifies AST nodes over exact ranges, so spellings appearing inside
        // a string literal or a comment must not warn. This claim owns
        // non-interpolated syntax; interpolated postfix dereference is tracked
        // separately against #6730 / #8636.
        let source = "use v5.20;\nmy $s = '$r->@* $r->@[0,1] $r->@{x}';\n# $r->%*\n";
        let diags = version_compat_diags(source);
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("PL900")),
            "spellings inside strings or comments are not postfix dereference: {diags:#?}"
        );
    }
}
