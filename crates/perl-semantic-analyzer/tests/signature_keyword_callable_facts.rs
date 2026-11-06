//! End-to-end Function::Parameters / Method::Signatures callable-fact proof (#16808).
//!
//! Drives parse → lexical keyword enablement → declaration extraction →
//! registry detection → minted canonical facts, then queries those facts
//! through [`lookup_signature_keyword_callable`]. Parser node presence is not
//! counted as support.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::signature_keyword_declarations::extract_signature_keyword_units;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, AdapterDisposition, DetectionAbsenceReason,
    DetectionAuthorityError, DetectionEvidenceClass, DetectionOutcome, ModuleActivationIdentity,
    ModuleObservationReceipt, ModuleSelectorEvaluation, ModuleSelectorOutcome,
    ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::signature_keywords::{
    SignatureCallableKind, SignatureKeyword, SignatureKeywordCallableFact, SignatureKeywordFamily,
    SignatureParameterKind, lookup_signature_keyword_callable, signature_keyword_callable_facts,
    signature_keyword_descriptors,
};
use perl_semantic_facts::{FileId, SemanticFactKind, SemanticReasonCode, SourceGeneration};
use perl_tdd_support::{must, must_some};

fn matched(
    family: SignatureKeywordFamily,
    version: &str,
    generation: &str,
) -> ModuleSelectorEvaluation {
    let activation = ModuleActivationIdentity::new(
        family.module_name(),
        Some(FileId(7)),
        SourceGeneration::known(generation),
    )
    .with_observed_version(ModuleVersionEvidence::new(
        version,
        SourceGeneration::known(generation),
    ));
    ModuleSelectorEvaluation::new(
        family.module_name(),
        ModuleSelectorOutcome::Matched {
            activation,
            evidence_class: DetectionEvidenceClass::ResolvedImport,
        },
    )
}

fn detection_input(
    family: SignatureKeywordFamily,
    version: &str,
    generation: &str,
) -> AdapterDetectionInput {
    AdapterDetectionInput::new(
        family.descriptor(),
        ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "env:fixture",
            SourceGeneration::known(generation),
            "sha256:fixture",
            vec![matched(family, version, generation)],
        ),
        None,
        AdapterCancellation::active(),
    )
}

fn minted(
    code: &str,
    family: SignatureKeywordFamily,
    version: &str,
    generation: &str,
) -> Vec<SignatureKeywordCallableFact> {
    minted_with_input(code, family, &detection_input(family, version, generation))
}

fn minted_with_input(
    code: &str,
    family: SignatureKeywordFamily,
    input: &AdapterDetectionInput,
) -> Vec<SignatureKeywordCallableFact> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let extracted = extract_signature_keyword_units(
        &ast,
        code,
        FileId(1),
        input.module_observation.generation.clone(),
    );
    let detection = family.detect(input);
    let mut packages = Vec::new();
    for declaration in &extracted.declarations {
        if !packages.iter().any(|seen| seen == &declaration.package) {
            packages.push(declaration.package.clone());
        }
    }
    packages
        .into_iter()
        .flat_map(|package| {
            signature_keyword_callable_facts(
                &detection,
                family,
                package.as_deref(),
                &extracted.declarations,
            )
        })
        .collect()
}

fn query<'a>(
    facts: &'a [SignatureKeywordCallableFact],
    package: Option<&str>,
    name: &str,
) -> &'a SignatureKeywordCallableFact {
    must_some(lookup_signature_keyword_callable(facts, package, name))
}

#[test]
fn descriptors_are_shadow_and_distinct() {
    let descriptors = signature_keyword_descriptors();
    assert_eq!(descriptors[0].framework_name, "Function::Parameters");
    assert_eq!(descriptors[1].framework_name, "Method::Signatures");
    assert!(descriptors.iter().all(|item| item.disposition == AdapterDisposition::Shadow));
}

#[test]
fn function_parameters_fun_and_method_are_queryable_callables() {
    let code = r#"
package App;
use Function::Parameters;
fun add ($left, $right) { $left + $right }
method describe ($prefix = q{}) { $prefix }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert_eq!(facts.len(), 2);

    let add = query(&facts, Some("App"), "add");
    assert_eq!(add.keyword, SignatureKeyword::Fun);
    assert_eq!(add.callable_kind, SignatureCallableKind::Function);
    assert_eq!(add.envelope.kind, SemanticFactKind::Declaration);
    assert_eq!(add.parameters.len(), 2);
    assert!(
        add.parameters.iter().all(|parameter| parameter.kind == SignatureParameterKind::Positional)
    );
    assert!(add.signature_anchor.is_some());
    assert!(add.body_anchor.is_some());
    assert_eq!(add.profile_version, "function-parameters.2.v1");

    let describe = query(&facts, Some("App"), "describe");
    assert_eq!(describe.keyword, SignatureKeyword::Method);
    assert_eq!(describe.callable_kind, SignatureCallableKind::Method);
    assert_eq!(describe.parameters.len(), 1);
    assert_eq!(describe.parameters[0].kind, SignatureParameterKind::Optional);
}

#[test]
fn method_signatures_func_and_method_are_queryable_callables() {
    let code = r#"
package App;
use Method::Signatures;
func helper ($value) { $value }
method run ($arg) { $arg }
"#;
    let facts = minted(code, SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1");
    assert_eq!(facts.len(), 2);
    assert_eq!(query(&facts, Some("App"), "helper").keyword, SignatureKeyword::Func);
    assert_eq!(query(&facts, Some("App"), "run").keyword, SignatureKeyword::Method);
    assert_eq!(query(&facts, Some("App"), "run").callable_kind, SignatureCallableKind::Method);
}

#[test]
fn activation_absent_same_words_mint_no_callable_facts() {
    let code = r#"
package App;
fun add ($left, $right) { $left }
func helper ($value) { $value }
method run ($arg) { $arg }
"#;
    let fp = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let ms = minted(code, SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1");
    assert!(fp.is_empty());
    assert!(ms.is_empty());
    assert!(lookup_signature_keyword_callable(&fp, Some("App"), "add").is_none());
}

#[test]
fn empty_import_and_unsupported_version_fail_closed() {
    let empty = minted(
        "package App;\nuse Function::Parameters ();\nfun add ($x) { $x }\n",
        SignatureKeywordFamily::FunctionParameters,
        "2.002006",
        "gen-1",
    );
    assert!(empty.is_empty());

    let unsupported = minted_with_input(
        "package App;\nuse Function::Parameters;\nfun add ($x) { $x }\n",
        SignatureKeywordFamily::FunctionParameters,
        &detection_input(SignatureKeywordFamily::FunctionParameters, "1.000000", "gen-1"),
    );
    assert!(unsupported.is_empty());
    let detection = SignatureKeywordFamily::FunctionParameters.detect(&detection_input(
        SignatureKeywordFamily::FunctionParameters,
        "1.000000",
        "gen-1",
    ));
    assert_eq!(
        detection.outcome,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::VersionConstraintNotSatisfied }
    );
}

#[test]
fn custom_keyword_configuration_is_not_guessed() {
    let code = r#"
package App;
use Function::Parameters { fun => { defaults => 'function_strict' } };
fun add ($x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(facts.is_empty());
}

#[test]
fn qw_fun_does_not_mint_method() {
    let code = r#"
package App;
use Function::Parameters qw(fun);
fun add ($x) { $x }
method run ($arg) { $arg }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_some());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "run").is_none());
}

#[test]
fn same_name_in_two_packages_stays_distinct() {
    let code = r#"
package A;
use Function::Parameters;
fun add ($x) { $x }
package B;
use Function::Parameters;
fun add ($x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let a = query(&facts, Some("A"), "add");
    let b = query(&facts, Some("B"), "add");
    assert_ne!(a.envelope.entity_id, b.envelope.entity_id);
    assert_ne!(a.declaration_package(), b.declaration_package());
}

#[test]
fn positional_optional_and_slurpy_parameters_are_classified() {
    let code = r#"
package App;
use Function::Parameters;
fun scale ($factor, $offset = 0, @values) { 1 }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let scale = query(&facts, Some("App"), "scale");
    assert_eq!(
        scale.parameters.iter().map(|parameter| parameter.kind).collect::<Vec<_>>(),
        vec![
            SignatureParameterKind::Positional,
            SignatureParameterKind::Optional,
            SignatureParameterKind::Slurpy,
        ]
    );
}

#[test]
fn typed_parameters_are_not_flattened_into_exact_canonical_params() {
    let code = r#"
package App;
use Function::Parameters;
fun typed (Str $x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let typed = query(&facts, Some("App"), "typed");
    assert!(typed.parameters.is_empty());
    assert!(typed.envelope.boundary.is_some());
    assert_eq!(typed.envelope.reason_code, SemanticReasonCode::GeneratedFromSource);
}

#[test]
fn malformed_declaration_does_not_fabricate_an_exact_fact() {
    let code = "package App;\nuse Function::Parameters;\nfun add ($;\n";
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_none());
}

#[test]
fn nested_declaration_keeps_scope_boundaries() {
    let code = r#"
package App;
use Function::Parameters;
fun add ($left, $right) { fun nested ($x) { $x } }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_some());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "nested").is_some());
}

#[test]
fn lexical_no_and_block_scope_do_not_leak_keywords() {
    let code = r#"
package App;
{
  use Function::Parameters;
  fun inner ($x) { $x };
}
fun outer ($x) { $x };
use Function::Parameters;
fun kept ($x) { $x };
{
  no Function::Parameters;
  fun disabled ($x) { $x };
}
fun after_no ($x) { $x };
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "inner").is_some());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "outer").is_none());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "kept").is_some());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "disabled").is_none());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "after_no").is_some());
}

#[test]
fn generation_change_invalidates_fact_identity() {
    let code = "package App;\nuse Function::Parameters;\nfun add ($x) { $x }\n";
    let first = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let second = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-2");
    assert_ne!(
        query(&first, Some("App"), "add").envelope.fact_id,
        query(&second, Some("App"), "add").envelope.fact_id
    );
}

#[test]
fn native_class_method_is_not_claimed_as_framework_declaration() {
    let code = r#"
package App;
use Function::Parameters;
class C {
  method native ($x) { $x }
}
fun add ($x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_some());
    assert!(lookup_signature_keyword_callable(&facts, Some("C"), "native").is_none());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "native").is_none());
}

#[test]
fn shadow_detection_cannot_become_publication_authority() {
    let input = detection_input(SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let detection = SignatureKeywordFamily::FunctionParameters.detect(&input);
    assert!(detection.is_detected());
    assert_eq!(
        detection.validate_authority_against(&input),
        Err(DetectionAuthorityError::NonProduction)
    );
}

#[test]
fn shared_query_is_the_consumer_not_symbol_table_presence() {
    let code = r#"
package App;
use Function::Parameters;
fun add ($left, $right) { $left }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let found = query(&facts, Some("App"), "add");
    assert_eq!(found.name, "add");
    assert_eq!(found.callable_kind, SignatureCallableKind::Function);
    assert!(found.signature_anchor.is_some(), "signature lookup consumes the signature range");
}

#[test]
fn optional_signature_without_parens_is_still_a_callable() {
    let code = r#"
package App;
use Function::Parameters;
fun add { 1 }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let add = query(&facts, Some("App"), "add");
    assert_eq!(add.keyword, SignatureKeyword::Fun);
    assert!(add.body_anchor.is_some());
    assert!(add.signature_anchor.is_none(), "bare fun NAME BLOCK has no signature range");
}

#[test]
fn body_range_comes_from_ast_not_a_later_comment_brace() {
    let code = "package App;\nuse Function::Parameters;\nfun add ($x) {\n  # }\n  $x\n}\n";
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let add = query(&facts, Some("App"), "add");
    let body = add.body_anchor.as_ref().and_then(|anchor| {
        let start = usize::try_from(anchor.start_byte).ok()?;
        let end = usize::try_from(anchor.end_byte).ok()?;
        code.get(start..end)
    });
    let body = must_some(body);
    assert!(body.contains("$x"), "body range was {body:?}");
    assert!(body.starts_with('{') && body.ends_with('}'), "body range was {body:?}");
}

#[test]
fn nested_module_and_require_do_not_activate_keywords() {
    let nested = minted(
        "package App;\nuse Function::Parameters::Strict;\nfun add ($x) { $x }\n",
        SignatureKeywordFamily::FunctionParameters,
        "2.002006",
        "gen-1",
    );
    let required = minted(
        "package App;\nrequire Function::Parameters;\nfun add ($x) { $x }\n",
        SignatureKeywordFamily::FunctionParameters,
        "2.002006",
        "gen-1",
    );
    assert!(nested.is_empty());
    assert!(required.is_empty());
}

#[test]
fn qw_method_does_not_mint_fun() {
    let code = r#"
package App;
use Function::Parameters qw(method);
fun add ($x) { $x }
method run ($arg) { $arg }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_none());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "run").is_some());
}

#[test]
fn qw_func_does_not_mint_method() {
    let code = r#"
package App;
use Method::Signatures qw(func);
func helper ($value) { $value }
method run ($arg) { $arg }
"#;
    let facts = minted(code, SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1");
    assert!(
        facts.is_empty(),
        "Method::Signatures qw(func) is not a reviewed import and must mint nothing"
    );
}

#[test]
fn function_parameters_does_not_mint_func() {
    let code = r#"
package App;
use Function::Parameters;
func helper ($value) { $value }
fun add ($x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_some());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "helper").is_none());
}

#[test]
fn overlapping_method_families_fail_closed() {
    let code = r#"
package App;
use Function::Parameters;
use Method::Signatures;
fun add ($x) { $x }
func helper ($x) { $x }
method run ($x) { $x }
"#;
    let fp = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let ms = minted(code, SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1");
    assert!(lookup_signature_keyword_callable(&fp, Some("App"), "add").is_some());
    assert!(lookup_signature_keyword_callable(&fp, Some("App"), "helper").is_none());
    assert!(lookup_signature_keyword_callable(&fp, Some("App"), "run").is_none());
    assert!(lookup_signature_keyword_callable(&ms, Some("App"), "helper").is_some());
    assert!(lookup_signature_keyword_callable(&ms, Some("App"), "add").is_none());
    assert!(lookup_signature_keyword_callable(&ms, Some("App"), "run").is_none());
}

#[test]
fn named_parameter_is_not_an_exact_canonical_param() {
    let code = r#"
package App;
use Function::Parameters;
fun named (:$x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(
        lookup_signature_keyword_callable(&facts, Some("App"), "named").is_none_or(|fact| {
            fact.parameters.is_empty()
                && fact.envelope.boundary.is_some()
                && fact.envelope.reason_code == SemanticReasonCode::GeneratedFromSource
        }),
        "named :$param must not flatten into an exact canonical parameter list"
    );
}

#[test]
fn method_signatures_empty_import_mints_nothing() {
    let facts = minted(
        "package App;\nuse Method::Signatures ();\nfunc helper ($x) { $x }\n",
        SignatureKeywordFamily::MethodSignatures,
        "20170211",
        "gen-1",
    );
    assert!(facts.is_empty());
}

#[test]
fn shared_query_misses_wrong_package() {
    let code = r#"
package App;
use Function::Parameters;
fun add ($x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("Other"), "add").is_none());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "missing").is_none());
}

#[test]
fn qw_method_on_method_signatures_does_not_mint_func() {
    let code = r#"
package App;
use Method::Signatures qw(method);
func helper ($value) { $value }
method run ($arg) { $arg }
"#;
    let facts = minted(code, SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1");
    assert!(
        facts.is_empty(),
        "Method::Signatures qw(method) is not a reviewed import and must mint nothing"
    );
}

#[test]
fn no_qw_fun_disables_only_fun() {
    let code = r#"
package App;
use Function::Parameters;
no Function::Parameters qw(fun);
fun add ($x) { $x }
method run ($arg) { $arg }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_none());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "run").is_some());
}

#[test]
fn typed_method_parameters_are_not_exact() {
    let code = r#"
package App;
use Function::Parameters;
method run (Str $arg) { $arg }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let run = query(&facts, Some("App"), "run");
    assert!(run.parameters.is_empty());
    assert!(run.envelope.boundary.is_some());
}

#[test]
fn semicolon_between_fun_and_name_is_not_a_declaration() {
    let code = r#"
package App;
use Function::Parameters;
fun;
add { 1 }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_none());
}

#[test]
fn no_parens_body_still_walks_nested_callables() {
    let code = r#"
package App;
use Function::Parameters;
fun outer { fun inner ($x) { $x } }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "outer").is_some());
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "inner").is_some());
}

#[test]
fn third_import_does_not_revive_conflicting_method() {
    let code = r#"
package App;
use Function::Parameters;
use Method::Signatures;
use Function::Parameters;
method run ($x) { $x }
"#;
    let fp = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let ms = minted(code, SignatureKeywordFamily::MethodSignatures, "20170211", "gen-1");
    assert!(lookup_signature_keyword_callable(&fp, Some("App"), "run").is_none());
    assert!(lookup_signature_keyword_callable(&ms, Some("App"), "run").is_none());
}

#[test]
fn versioned_and_spaced_empty_imports_fail_closed() {
    let versioned = minted(
        "package App;\nuse Function::Parameters 2.002006 ();\nfun add ($x) { $x }\n",
        SignatureKeywordFamily::FunctionParameters,
        "2.002006",
        "gen-1",
    );
    let spaced = minted(
        "package App;\nuse Function::Parameters ( );\nfun add ($x) { $x }\n",
        SignatureKeywordFamily::FunctionParameters,
        "2.002006",
        "gen-1",
    );
    assert!(versioned.is_empty());
    assert!(spaced.is_empty());
}

#[test]
fn unsupported_requested_version_does_not_enable_keywords() {
    let code = "package App;\nuse Function::Parameters 3.0;\nfun add ($x) { $x }\n";
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(facts.is_empty());
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let extracted =
        extract_signature_keyword_units(&ast, code, FileId(1), SourceGeneration::known("gen-1"));
    assert_eq!(extracted.sites.len(), 1);
    assert!(
        !extracted.sites[0].is_exact(),
        "an unadmitted version pin must not report exact activation"
    );
}

#[test]
fn unadmitted_versioned_no_does_not_disable_keywords() {
    let code = r#"
package App;
use Function::Parameters;
no Function::Parameters 3.0;
fun add ($x) { $x }
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_some());
}

#[test]
fn extra_two_statement_arguments_are_not_a_declaration() {
    let code = r#"
package App;
use Function::Parameters;
fun add { 1 }, 2
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_none());
}

#[test]
fn extra_two_statement_arguments_after_comment_split_are_not_a_declaration() {
    let code = r#"
package App;
use Function::Parameters;
fun # force the two-statement split
add { 1 }, 2
"#;
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_none());
}

#[test]
fn admitted_activation_sites_are_exact() {
    let cases = [
        (
            "package App;\nuse Function::Parameters;\nfun add ($x) { $x }\n",
            SignatureKeywordFamily::FunctionParameters,
        ),
        (
            "package App;\nuse Function::Parameters 2.002006;\nfun add ($x) { $x }\n",
            SignatureKeywordFamily::FunctionParameters,
        ),
        (
            "package App;\nuse Method::Signatures;\nfunc add ($x) { $x }\n",
            SignatureKeywordFamily::MethodSignatures,
        ),
    ];
    for (code, family) in cases {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let extracted = extract_signature_keyword_units(
            &ast,
            code,
            FileId(1),
            SourceGeneration::known("gen-1"),
        );
        assert_eq!(extracted.sites.len(), 1, "{code}");
        assert_eq!(extracted.sites[0].family, family, "{code}");
        assert!(extracted.sites[0].is_exact(), "an admitted activation must report exact: {code}");
    }
}

#[test]
fn comment_semicolon_between_fun_and_name_is_still_a_declaration() {
    let code = "package App;\nuse Function::Parameters;\nfun # note; still docs\nadd { 1 }\n";
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    assert!(lookup_signature_keyword_callable(&facts, Some("App"), "add").is_some());
}

#[test]
fn comment_between_signature_and_brace_still_recovers_the_body() {
    let code = "package App;\nuse Function::Parameters;\nfun add ($x) # note\n{ $x }\n";
    let facts = minted(code, SignatureKeywordFamily::FunctionParameters, "2.002006", "gen-1");
    let add = query(&facts, Some("App"), "add");
    let body = add.body_anchor.as_ref().and_then(|anchor| {
        let start = usize::try_from(anchor.start_byte).ok()?;
        let end = usize::try_from(anchor.end_byte).ok()?;
        code.get(start..end)
    });
    let body = must_some(body);
    assert!(body.contains("$x"), "body range was {body:?}");
    assert!(body.starts_with('{') && body.ends_with('}'), "body range was {body:?}");
}

trait DeclarationPackage {
    fn declaration_package(&self) -> Option<&str>;
}

impl DeclarationPackage for SignatureKeywordCallableFact {
    fn declaration_package(&self) -> Option<&str> {
        self.envelope.package.as_deref()
    }
}
