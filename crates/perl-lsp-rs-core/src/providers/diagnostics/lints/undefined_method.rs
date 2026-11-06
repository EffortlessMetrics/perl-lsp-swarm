//! Undefined method-call diagnostic (PL306).
//!
//! Warn when a static `->method` call targets a receiver class whose known
//! method table does not contain that name. The table has two tiers, tried in
//! order:
//!
//! 1. **In-file tier** — the file's own package/class tables (including
//!    declared parents and roles). Fires on a definite miss, stays silent
//!    when an ancestor is not in this file (its MRO is not computable here).
//! 2. **Workspace tier** — reached when the in-file tier cannot answer: the
//!    workspace index (`known_package` + `method_candidates`, the same MRO
//!    table hover/definition and completion use) is consulted, and the lint
//!    fires only when the index *knows* the receiver class and still lacks
//!    the method. An absent or unindexed receiver stays silent, because an
//!    empty candidate list there means "no data", not "missing".
//!
//! Receivers are inferred in-file (constructor assignments, `$self`, static
//! packages) first, then through the completion-grade receiver facts when a
//! type environment is available — accepted only at High confidence, Fresh
//! evidence, Exact fallback, and no dynamic boundary, exactly like
//! completion. Dynamic dispatch is acknowledged in the message; AUTOLOAD,
//! UNIVERSAL methods, unions, and medium-confidence receivers stay silent.

use std::collections::{HashMap, HashSet};

use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser_core::ast::{Node, NodeKind};
use perl_semantic_analyzer::class_model::{
    AccessorType, ClassModel, ClassModelBuilder, Framework, merge_reopened_class_models,
};
use perl_semantic_analyzer::receiver_facts::{
    ReceiverFactContext, ReceiverFactFreshness, ReceiverFallbackState,
    receiver_fact_for_method_call,
};
use perl_semantic_analyzer::symbol::is_universal_method;
use perl_semantic_analyzer::type_inference::TypeInferenceEngine;
use perl_semantic_facts::{Confidence, DefinitionCandidate};

use super::super::internal_types::{Diagnostic, RelatedInformation};

struct ClassInfo {
    methods: HashSet<String>,
    autoload: bool,
    parents: Vec<String>,
    roles: Vec<String>,
    framework: Framework,
}

struct WalkState<'a, 'b> {
    package: String,
    locals: HashMap<String, String>,
    classes: &'a HashMap<String, ClassInfo>,
    source: &'a str,
    /// Lazily materialized type environment: the deferred pass is expensive,
    /// and only receiver-fact fallbacks ever read it, so the accessor runs at
    /// most once per evaluation and only when a call site needs it (#19051
    /// review).
    type_environment: &'b (dyn Fn() -> Option<&'b TypeInferenceEngine> + 'b),
    known_package: &'b (dyn Fn(&str) -> Option<bool> + 'b),
    method_candidates: &'b (dyn Fn(&str, &str) -> Vec<DefinitionCandidate> + 'b),
}

/// Warn when a method call is absent from the receiver class's known method
/// table (in-file tables first, then the workspace index).
pub fn check_undefined_methods<'a, 'b>(
    root: &'a Node,
    source: &'a str,
    type_environment: &'b (dyn Fn() -> Option<&'b TypeInferenceEngine> + 'b),
    known_package: &'b (dyn Fn(&str) -> Option<bool> + 'b),
    method_candidates: &'b (dyn Fn(&str, &str) -> Vec<DefinitionCandidate> + 'b),
    diagnostics: &mut Vec<Diagnostic>,
) {
    let raw_models = ClassModelBuilder::new().build(root);
    let models = merge_reopened_class_models(&raw_models);
    let defined = collect_package_methods(root);
    let classes = class_info_by_name(&models, &raw_models, defined);
    let mut state = WalkState {
        package: "main".to_string(),
        locals: HashMap::new(),
        classes: &classes,
        source,
        type_environment,
        known_package,
        method_candidates,
    };
    visit(root, &mut state, diagnostics);
}

fn class_info_by_name(
    models: &[ClassModel],
    raw_models: &[ClassModel],
    defined: HashMap<String, HashSet<String>>,
) -> HashMap<String, ClassInfo> {
    let mut classes = HashMap::new();
    for (name, methods) in defined {
        if name.is_empty() {
            continue;
        }
        let autoload = methods.contains("AUTOLOAD");
        classes.insert(
            name,
            ClassInfo {
                methods,
                autoload,
                parents: Vec::new(),
                roles: Vec::new(),
                framework: Framework::None,
            },
        );
    }
    for model in models {
        if model.name.is_empty() {
            continue;
        }
        let extra = provided_method_names(model);
        let entry = classes.entry(model.name.clone()).or_insert_with(|| ClassInfo {
            methods: HashSet::new(),
            autoload: false,
            parents: Vec::new(),
            roles: Vec::new(),
            framework: Framework::None,
        });
        entry.methods.extend(extra);
        entry.autoload |= entry.methods.contains("AUTOLOAD");
        if !model.parents.is_empty() {
            entry.parents = model.parents.clone();
        }
        if !model.roles.is_empty() {
            entry.roles = model.roles.clone();
        }
        if model.framework != Framework::None {
            entry.framework = model.framework;
        }
    }
    for model in raw_models {
        if model.name.is_empty() {
            continue;
        }
        let extra = provided_method_names(model);
        if let Some(entry) = classes.get_mut(&model.name) {
            entry.methods.extend(extra);
            entry.autoload |= entry.methods.contains("AUTOLOAD");
        }
    }
    classes
}

/// Every in-file `sub` / `method`, including plain packages that ClassModel omits.
fn collect_package_methods(root: &Node) -> HashMap<String, HashSet<String>> {
    let mut methods = HashMap::new();
    let mut package = "main".to_string();
    collect_package_methods_visit(root, &mut package, &mut methods);
    methods
}

fn collect_package_methods_visit(
    node: &Node,
    package: &mut String,
    methods: &mut HashMap<String, HashSet<String>>,
) {
    match &node.kind {
        NodeKind::Package { name, block: Some(block), .. } => {
            let previous = std::mem::replace(package, name.clone());
            methods.entry(package.clone()).or_default();
            collect_package_methods_visit(block, package, methods);
            *package = previous;
        }
        NodeKind::Package { name, block: None, .. } => {
            *package = name.clone();
            methods.entry(package.clone()).or_default();
        }
        NodeKind::Class { name, body, .. } => {
            let previous = std::mem::replace(package, name.clone());
            methods.entry(package.clone()).or_default();
            collect_package_methods_visit(body, package, methods);
            *package = previous;
        }
        NodeKind::Subroutine { name, body, declarator, .. } => {
            let lexical = is_lexical_sub_declarator(declarator.as_deref());
            if let Some(sub_name) = name
                && !sub_name.is_empty()
                && !lexical
            {
                methods.entry(package.clone()).or_default().insert(sub_name.clone());
            }
            collect_package_methods_visit(body, package, methods);
        }
        NodeKind::Method { name, body, .. } => {
            if !name.is_empty() {
                methods.entry(package.clone()).or_default().insert(name.clone());
            }
            collect_package_methods_visit(body, package, methods);
        }
        _ => {
            for child in node.children() {
                collect_package_methods_visit(child, package, methods);
            }
        }
    }
}

fn is_lexical_sub_declarator(declarator: Option<&str>) -> bool {
    matches!(declarator, Some("my") | Some("state"))
}

fn provided_method_names(model: &ClassModel) -> HashSet<String> {
    let mut names = HashSet::new();
    for method in model.methods.iter().chain(model.adjusts.iter()) {
        if is_lexical_sub_declarator(method.declarator.as_deref()) {
            continue;
        }
        if !method.name.is_empty() {
            names.insert(method.name.clone());
        }
    }
    for attribute in &model.attributes {
        if attribute.is != Some(AccessorType::Bare) && !attribute.accessor_name.is_empty() {
            names.insert(attribute.accessor_name.clone());
        }
        for extra in [&attribute.predicate, &attribute.clearer, &attribute.builder] {
            if let Some(name) = extra
                && !name.is_empty()
            {
                names.insert(name.clone());
            }
        }
    }
    for field in &model.fields {
        for extra in [&field.reader, &field.writer, &field.accessor, &field.mutator] {
            if let Some(name) = extra
                && !name.is_empty()
            {
                names.insert(name.clone());
            }
        }
    }
    names
}

fn visit(node: &Node, state: &mut WalkState<'_, '_>, diagnostics: &mut Vec<Diagnostic>) {
    match &node.kind {
        NodeKind::Package { name, block: Some(block), .. } => {
            let previous_package = std::mem::replace(&mut state.package, name.clone());
            let previous_locals = std::mem::take(&mut state.locals);
            visit(block, state, diagnostics);
            state.package = previous_package;
            state.locals = previous_locals;
        }
        NodeKind::Package { name, block: None, .. } => {
            state.package = name.clone();
        }
        NodeKind::Class { name, body, .. } => {
            let previous_package = std::mem::replace(&mut state.package, name.clone());
            let previous_locals = std::mem::take(&mut state.locals);
            visit(body, state, diagnostics);
            state.package = previous_package;
            state.locals = previous_locals;
        }
        NodeKind::Subroutine { body, .. } | NodeKind::Method { body, .. } => {
            let previous_locals = std::mem::take(&mut state.locals);
            state.locals.insert("self".to_string(), state.package.clone());
            state.locals.insert("this".to_string(), state.package.clone());
            visit(body, state, diagnostics);
            state.locals = previous_locals;
        }
        NodeKind::VariableDeclaration { variable, initializer, .. } => {
            if let Some(init) = initializer {
                record_constructor(variable, init, state);
                visit(init, state, diagnostics);
            }
        }
        NodeKind::Assignment { lhs, rhs, .. } => {
            record_constructor(lhs, rhs, state);
            visit(lhs, state, diagnostics);
            visit(rhs, state, diagnostics);
        }
        NodeKind::MethodCall { object, method, args } => {
            maybe_warn_undefined_method(node, object, method, state, diagnostics);
            visit(object, state, diagnostics);
            for arg in args {
                visit(arg, state, diagnostics);
            }
        }
        NodeKind::Block { .. } => {
            let checkpoint = state.locals.clone();
            for child in node.children() {
                visit(child, state, diagnostics);
            }
            state.locals = checkpoint;
        }
        _ => {
            for child in node.children() {
                visit(child, state, diagnostics);
            }
        }
    }
}

fn record_constructor(lhs: &Node, rhs: &Node, state: &mut WalkState<'_, '_>) {
    let Some(variable) = scalar_name(lhs) else {
        return;
    };
    if let Some(package) = constructor_package(rhs) {
        state.locals.insert(variable, package);
    } else {
        state.locals.remove(&variable);
    }
}

fn constructor_package(rhs: &Node) -> Option<String> {
    let NodeKind::MethodCall { object, method, .. } = &rhs.kind else {
        return None;
    };
    if method != "new" {
        return None;
    }
    match &object.kind {
        NodeKind::Identifier { name } if !name.is_empty() => Some(name.clone()),
        NodeKind::String { value, .. } => {
            let normalized = value.trim().trim_matches('\'').trim_matches('"').trim();
            (!normalized.is_empty()).then(|| normalized.to_string())
        }
        _ => None,
    }
}

fn scalar_name(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Variable { sigil, name } if sigil == "$" => {
            let stripped = name.trim_start_matches('$');
            (!stripped.is_empty()).then(|| stripped.to_string())
        }
        _ => None,
    }
}

fn maybe_warn_undefined_method(
    call: &Node,
    object: &Node,
    method: &str,
    state: &WalkState<'_, '_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !is_static_method_name(method) {
        return;
    }
    let Some(receiver) =
        infer_receiver_package(object, state).or_else(|| receiver_fact_package(call, state))
    else {
        return;
    };
    match method_is_known(state.classes, &receiver, method, &mut HashSet::new()) {
        Some(true) => return,
        // Definite in-file miss: every table this file could answer from
        // lacks the method.
        Some(false) => {}
        // The in-file tier cannot answer (receiver class not in this file,
        // or an ancestor is unseen). Consult the workspace tier, firing only
        // when the index knows the class *and* still lacks the method — an
        // empty candidate list for an unknown package means "no data", not
        // "missing" (#18771's empty-is-not-absent gate).
        None => {
            if (state.known_package)(&receiver) != Some(true) {
                return;
            }
            if !(state.method_candidates)(&receiver, "AUTOLOAD").is_empty() {
                return;
            }
            if !(state.method_candidates)(&receiver, method).is_empty() {
                return;
            }
        }
    }

    let range = method_name_range(call, object, method, state.source);
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::UnresolvedMethodCall.as_str().to_string()),
        message: format!(
            "no known definition of {method} on {receiver}; may be resolved at runtime"
        ),
        related_information: vec![RelatedInformation {
            location: range,
            message: format!(
                "Define `{method}` on `{receiver}`, correct the call, or ignore this if the \
                 method is installed dynamically."
            ),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        parse_error_fix_kind: None,
        suggestion: Some(format!(
            "Rename the call to a known `{receiver}` method, or add `sub {method}`."
        )),
    });
}

fn is_static_method_name(method: &str) -> bool {
    let mut chars = method.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {
            chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        }
        _ => false,
    }
}

fn infer_receiver_package(object: &Node, state: &WalkState<'_, '_>) -> Option<String> {
    match &object.kind {
        NodeKind::Identifier { name } if !name.is_empty() => Some(name.clone()),
        NodeKind::String { value, .. } => {
            let normalized = value.trim().trim_matches('\'').trim_matches('"').trim();
            (!normalized.is_empty()).then(|| normalized.to_string())
        }
        NodeKind::Variable { sigil, name } if sigil == "$" => {
            let stripped = name.trim_start_matches('$');
            state.locals.get(stripped).cloned()
        }
        _ => None,
    }
}

/// Completion-grade receiver inference through the type environment.
///
/// Accepted only at the same trust level completion requires (`workspace.rs`
/// `exact_receiver_fact_evidence`): High confidence, Fresh evidence, Exact
/// fallback, no dynamic boundary. Medium-confidence receivers (array index,
/// dynamic hash key) and union receivers stay silent — diagnostics must not
/// be braver than completion.
fn receiver_fact_package(call: &Node, state: &WalkState<'_, '_>) -> Option<String> {
    let type_environment = (state.type_environment)()?;
    let fact = receiver_fact_for_method_call(
        call,
        ReceiverFactContext::new(Some(type_environment.environment())).with_source(state.source),
    );

    let exact = fact.confidence == Confidence::High
        && fact.freshness == ReceiverFactFreshness::Fresh
        && fact.fallback_state == ReceiverFallbackState::Exact
        && fact.dynamic_boundary.is_none()
        && fact.source_range.is_some()
        && !fact.is_union_receiver();

    if exact { fact.package.clone() } else { None }
}

fn method_is_known(
    classes: &HashMap<String, ClassInfo>,
    package: &str,
    method: &str,
    seen: &mut HashSet<String>,
) -> Option<bool> {
    if is_universal_method(method) || matches!(method, "DESTROY" | "import" | "unimport") {
        return Some(true);
    }
    if !seen.insert(package.to_string()) {
        return Some(false);
    }
    let info = classes.get(package)?;
    if info.methods.contains(method) || info.autoload {
        return Some(true);
    }
    if method == "new" && framework_provides_constructor(info.framework) {
        return Some(true);
    }
    if method == "meta" && framework_provides_meta(info.framework) {
        return Some(true);
    }

    let mut saw_unknown_ancestor = false;
    for related in info.parents.iter().chain(info.roles.iter()) {
        match method_is_known(classes, related, method, seen) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => saw_unknown_ancestor = true,
        }
    }
    if saw_unknown_ancestor { None } else { Some(false) }
}

fn framework_provides_constructor(framework: Framework) -> bool {
    matches!(
        framework,
        Framework::Moose
            | Framework::Moo
            | Framework::Mouse
            | Framework::ObjectPad
            | Framework::NativeClass
            | Framework::ClassTiny
            | Framework::ClassAccessor
            | Framework::DbixClass
    )
}

fn framework_provides_meta(framework: Framework) -> bool {
    matches!(framework, Framework::Moose | Framework::Moo | Framework::Mouse)
}

fn method_name_range(call: &Node, object: &Node, method: &str, source: &str) -> (usize, usize) {
    let search_start = object.location.end.min(source.len());
    let search_end = call.location.end.min(source.len());
    if let Some(haystack) = source.get(search_start..search_end)
        && let Some(rel) = haystack.find(method)
    {
        let start = search_start.saturating_add(rel);
        let end = start.saturating_add(method.len()).min(source.len());
        if start < end {
            return (start, end);
        }
    }
    (call.location.start, call.location.end)
}
