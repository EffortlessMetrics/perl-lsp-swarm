//! Literal `fun` / `func` / `method` declaration extraction (#16808).
//!
//! Extraction is source observation plus reviewed keyword enablement. It does
//! not decide module/version detection: a declaration fact exists only after
//! [`signature_keyword_callable_facts`] mints it over an exact adapter
//! detection. Native `class { method ... }` bodies are skipped so Perl 5.38
//! method syntax is not claimed as Function::Parameters / Method::Signatures.
//!
//! `fun` / `func` currently parse as calls whose following block becomes a
//! `{}` binary. Name, parameter, and body ranges are recovered from that
//! accepted syntax geometry: the Binary node's AST end closes the body, so a
//! comment `}` cannot truncate the range. Parameter facts are emitted only at
//! the strength the canonical signature owner already admits (positional,
//! literal-default optional, slurpy). Type constraints, named `:$param` forms,
//! and invocant `:` syntax stay limitations.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::signature_keywords::{
    SignatureKeyword, SignatureKeywordDeclaration, SignatureKeywordFamily,
    SignatureKeywordImportDisposition, SignatureKeywordParameter, SignatureKeywordSet,
    SignatureKeywordSiteAnchor, SignatureParameterKind, classify_signature_keyword_use,
    signature_keyword_anchor,
};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor, SourceGeneration};

/// One source-backed activation observation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureKeywordActivationSite {
    /// File containing the import.
    pub file_id: FileId,
    /// Deterministic source anchor for the import statement.
    pub anchor_id: AnchorId,
    /// Module family derived only from the exact module name.
    pub family: SignatureKeywordFamily,
    /// Static source version requirement, when present.
    pub requested_version: Option<String>,
    /// Reviewed or unmodeled import arguments.
    pub import_disposition: SignatureKeywordImportDisposition,
    /// Load-bearing package, interval, and generation identity.
    pub anchor: SignatureKeywordSiteAnchor,
}

impl SignatureKeywordActivationSite {
    /// Whether this site can participate in exact keyword activation.
    ///
    /// Import-list Exact is not enough: a requested version outside the
    /// reviewed constraint records the observation but does not enable
    /// keywords and must not report as exact activation.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.import_disposition.is_exact()
            && requested_version_is_admitted(self.family, self.requested_version.as_deref())
    }
}

/// Combined activation sites and enabled declarations for one file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SignatureKeywordExtraction {
    /// Activation sites in source order.
    pub sites: Vec<SignatureKeywordActivationSite>,
    /// Declarations enabled by a reviewed exact import in scope.
    pub declarations: Vec<SignatureKeywordDeclaration>,
}

/// Extract activation sites and enabled declarations from `ast`.
#[must_use]
pub fn extract_signature_keyword_units(
    ast: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
) -> SignatureKeywordExtraction {
    let mut state = WalkState {
        file_id,
        source,
        generation,
        current_package: Some("main".to_string()),
        keywords: ScopeKeywords::default(),
        next_index: 0,
        sites: Vec::new(),
        declarations: Vec::new(),
    };
    walk_node(ast, &mut state);
    SignatureKeywordExtraction { sites: state.sites, declarations: state.declarations }
}

/// Extract every activation observation from `ast`, in source order.
#[must_use]
pub fn extract_signature_keyword_activation_sites(
    ast: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<SignatureKeywordActivationSite> {
    extract_signature_keyword_units(ast, source, file_id, generation).sites
}

/// Extract only the enabled declarations.
#[must_use]
pub fn extract_signature_keyword_declarations(
    ast: &Node,
    source: &str,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<SignatureKeywordDeclaration> {
    extract_signature_keyword_units(ast, source, file_id, generation).declarations
}

struct WalkState<'a> {
    file_id: FileId,
    source: &'a str,
    generation: SourceGeneration,
    current_package: Option<String>,
    keywords: ScopeKeywords,
    next_index: u32,
    sites: Vec<SignatureKeywordActivationSite>,
    declarations: Vec<SignatureKeywordDeclaration>,
}

impl WalkState<'_> {
    fn anchor(&self, start: usize, end: usize) -> SourceAnchor {
        signature_keyword_anchor(self.file_id, start, end)
    }

    fn push_declaration(&mut self, mut declaration: SignatureKeywordDeclaration) {
        declaration.declaration_index = self.next_index;
        declaration.source_generation = self.generation.clone();
        self.next_index = self.next_index.saturating_add(1);
        self.declarations.push(declaration);
    }
}

#[derive(Clone, Default)]
struct ScopeKeywords {
    fun: Option<SignatureKeywordFamily>,
    func: Option<SignatureKeywordFamily>,
    fp_method: bool,
    ms_method: bool,
}

impl ScopeKeywords {
    fn family_for(&self, keyword: SignatureKeyword) -> Option<SignatureKeywordFamily> {
        match keyword {
            SignatureKeyword::Fun => self.fun,
            SignatureKeyword::Func => self.func,
            SignatureKeyword::Method => match (self.fp_method, self.ms_method) {
                (true, false) => Some(SignatureKeywordFamily::FunctionParameters),
                (false, true) => Some(SignatureKeywordFamily::MethodSignatures),
                _ => None,
            },
            _ => None,
        }
    }

    fn apply_use(&mut self, family: SignatureKeywordFamily, keywords: SignatureKeywordSet) {
        if keywords.contains(SignatureKeyword::Fun) {
            self.fun = Some(family);
        }
        if keywords.contains(SignatureKeyword::Func) {
            self.func = Some(family);
        }
        if keywords.contains(SignatureKeyword::Method) {
            match family {
                SignatureKeywordFamily::FunctionParameters => self.fp_method = true,
                SignatureKeywordFamily::MethodSignatures => self.ms_method = true,
                _ => {}
            }
        }
    }

    fn apply_no(&mut self, family: SignatureKeywordFamily, keywords: SignatureKeywordSet) {
        let disable_all = keywords.is_empty();
        if (disable_all || keywords.contains(SignatureKeyword::Fun)) && self.fun == Some(family) {
            self.fun = None;
        }
        if (disable_all || keywords.contains(SignatureKeyword::Func)) && self.func == Some(family) {
            self.func = None;
        }
        if disable_all || keywords.contains(SignatureKeyword::Method) {
            match family {
                SignatureKeywordFamily::FunctionParameters => self.fp_method = false,
                SignatureKeywordFamily::MethodSignatures => self.ms_method = false,
                _ => {}
            }
        }
    }
}

fn walk_node(node: &Node, state: &mut WalkState<'_>) {
    match &node.kind {
        NodeKind::Program { statements } => walk_statements(statements, state),
        NodeKind::Block { statements } => {
            let saved_package = state.current_package.clone();
            let saved_keywords = state.keywords.clone();
            walk_statements(statements, state);
            state.current_package = saved_package;
            state.keywords = saved_keywords;
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            let saved_package = state.current_package.clone();
            let saved_keywords = state.keywords.clone();
            state.current_package = Some(name.clone());
            walk_node(block, state);
            state.current_package = saved_package;
            state.keywords = saved_keywords;
        }
        NodeKind::Package { name, block: None, .. } => {
            state.current_package = Some(name.clone());
        }
        NodeKind::Class { .. } => {}
        NodeKind::Use { module, args, .. } => apply_use(node, module, args, state),
        NodeKind::No { module, args, .. } => apply_no(node, module, args, state),
        NodeKind::ExpressionStatement { expression } => {
            walk_expression_statement(expression, state)
        }
        NodeKind::Method { name, .. } if name == "ADJUST" => {}
        NodeKind::Method { .. } => {
            try_extract_method(node, state);
            for child in node.children() {
                walk_node(child, state);
            }
        }
        NodeKind::FunctionCall { name, .. }
            if name == SignatureKeyword::Fun.as_str()
                || name == SignatureKeyword::Func.as_str() =>
        {
            if let Some(nested) = try_extract_call_declaration(node, state) {
                walk_node(nested, state);
            } else {
                for child in node.children() {
                    walk_node(child, state);
                }
            }
        }
        _ => {
            for child in node.children() {
                walk_node(child, state);
            }
        }
    }
}

fn walk_statements(statements: &[Node], state: &mut WalkState<'_>) {
    let mut index = 0;
    while index < statements.len() {
        let statement = &statements[index];
        if try_extract_two_statement_optional_signature(statements, index, state) {
            index += 2;
            continue;
        }
        walk_node(statement, state);
        index += 1;
    }
}

fn walk_expression_statement(expression: &Node, state: &mut WalkState<'_>) {
    if let NodeKind::FunctionCall { name, .. } = &expression.kind
        && (name == SignatureKeyword::Fun.as_str() || name == SignatureKeyword::Func.as_str())
        && let Some(nested) = try_extract_call_declaration(expression, state)
    {
        walk_node(nested, state);
        return;
    }
    walk_node(expression, state);
}

fn apply_use(node: &Node, module: &str, args: &[String], state: &mut WalkState<'_>) {
    let source_span = state.source.get(node.location.start..node.location.end);
    let Some((family, requested_version, import_disposition)) =
        classify_signature_keyword_use(module, args, source_span)
    else {
        return;
    };
    state.sites.push(SignatureKeywordActivationSite {
        file_id: state.file_id,
        anchor_id: AnchorId(u64::try_from(node.location.start).unwrap_or(u64::MAX)),
        family,
        requested_version: requested_version.clone(),
        import_disposition: import_disposition.clone(),
        anchor: SignatureKeywordSiteAnchor::new(
            state.current_package.clone(),
            saturate_u32(node.location.start),
            saturate_u32(node.location.end),
            state.generation.clone(),
        ),
    });
    if let SignatureKeywordImportDisposition::Exact { keywords } = import_disposition
        && requested_version_is_admitted(family, requested_version.as_deref())
    {
        state.keywords.apply_use(family, keywords);
    }
}

fn requested_version_is_admitted(family: SignatureKeywordFamily, requested: Option<&str>) -> bool {
    let Some(requested) = requested else {
        return true;
    };
    perl_semantic_facts::framework::version_constraint_matches(
        family.version_constraint(),
        requested,
    ) == Some(true)
}

fn apply_no(node: &Node, module: &str, args: &[String], state: &mut WalkState<'_>) {
    let source_span = state.source.get(node.location.start..node.location.end);
    let Some((family, requested_version, disposition)) =
        classify_signature_keyword_use(module, args, source_span)
    else {
        return;
    };
    if !requested_version_is_admitted(family, requested_version.as_deref()) {
        return;
    }
    let keywords = match disposition {
        SignatureKeywordImportDisposition::Exact { keywords } => keywords,
        _ => SignatureKeywordSet::none(),
    };
    state.keywords.apply_no(family, keywords);
}

fn try_extract_method(node: &Node, state: &mut WalkState<'_>) {
    let NodeKind::Method { name, name_span, signature, body, .. } = &node.kind else {
        return;
    };
    if name.is_empty() || name == "ADJUST" {
        return;
    }
    let Some(family) = state.keywords.family_for(SignatureKeyword::Method) else {
        return;
    };
    let name_anchor = match name_span {
        Some(span) => state.anchor(span.start, span.end),
        None => state.anchor(node.location.start, node.location.start),
    };
    let signature_anchor =
        signature.as_ref().map(|sig| state.anchor(sig.location.start, sig.location.end));
    let body_anchor = Some(state.anchor(body.location.start, body.location.end));
    let (parameters, parameter_limitations) =
        parameters_from_signature(signature.as_deref(), state.source);
    state.push_declaration(SignatureKeywordDeclaration::new(
        state.current_package.clone(),
        state.file_id,
        0,
        family,
        SignatureKeyword::Method,
        name.clone(),
        name_anchor,
        state.anchor(node.location.start, node.location.end),
        signature_anchor,
        body_anchor,
        parameters,
        parameter_limitations,
    ));
}

fn try_extract_call_declaration<'a>(node: &'a Node, state: &mut WalkState<'_>) -> Option<&'a Node> {
    let NodeKind::FunctionCall { name: keyword_name, args } = &node.kind else {
        return None;
    };
    let keyword = match SignatureKeyword::from_source_token(keyword_name) {
        Some(keyword @ (SignatureKeyword::Fun | SignatureKeyword::Func)) => keyword,
        _ => return None,
    };
    let family = state.keywords.family_for(keyword)?;
    let [arg] = args.as_slice() else {
        return None;
    };
    let NodeKind::Binary { op, left, right } = &arg.kind else {
        return None;
    };
    if op != "{}" {
        return None;
    }
    let NodeKind::FunctionCall { name, args: param_args } = &left.kind else {
        return None;
    };
    if !is_declaration_name(name) {
        return None;
    }
    let (body_start, body_end) =
        block_operator_span(state.source, left.location.end, arg.location.end)?;
    let (parameters, parameter_limitations) = parameters_from_call_args(param_args, state.source);
    let name_anchor = name_anchor_from_call(left, name, state.file_id);
    let signature_anchor =
        Some(state.anchor(left.location.start.saturating_add(name.len()), left.location.end));
    state.push_declaration(SignatureKeywordDeclaration::new(
        state.current_package.clone(),
        state.file_id,
        0,
        family,
        keyword,
        name.clone(),
        name_anchor,
        state.anchor(node.location.start, body_end),
        signature_anchor,
        Some(state.anchor(body_start, body_end)),
        parameters,
        parameter_limitations,
    ));
    Some(right.as_ref())
}

fn try_extract_two_statement_optional_signature(
    statements: &[Node],
    index: usize,
    state: &mut WalkState<'_>,
) -> bool {
    let Some(first) = statements.get(index) else {
        return false;
    };
    let Some(second) = statements.get(index.saturating_add(1)) else {
        return false;
    };
    let first_expr = match &first.kind {
        NodeKind::ExpressionStatement { expression } => expression.as_ref(),
        _ => return false,
    };
    let NodeKind::Identifier { name: keyword_name } = &first_expr.kind else {
        return false;
    };
    let keyword = match SignatureKeyword::from_source_token(keyword_name) {
        Some(keyword @ (SignatureKeyword::Fun | SignatureKeyword::Func)) => keyword,
        _ => return false,
    };
    let Some(family) = state.keywords.family_for(keyword) else {
        return false;
    };
    let second_expr = match &second.kind {
        NodeKind::ExpressionStatement { expression } => expression.as_ref(),
        _ => return false,
    };
    let NodeKind::FunctionCall { name, args } = &second_expr.kind else {
        return false;
    };
    if !is_declaration_name(name) {
        return false;
    }
    let [first_arg] = args.as_slice() else {
        return false;
    };
    if !matches!(first_arg.kind, NodeKind::Block { .. }) {
        return false;
    }
    if state
        .source
        .get(first.location.end..second.location.start)
        .is_some_and(gap_contains_statement_semicolon)
    {
        return false;
    }
    let name_anchor = name_anchor_from_call(second_expr, name, state.file_id);
    state.push_declaration(SignatureKeywordDeclaration::new(
        state.current_package.clone(),
        state.file_id,
        0,
        family,
        keyword,
        name.clone(),
        name_anchor,
        state.anchor(first.location.start, second.location.end),
        None,
        Some(state.anchor(first_arg.location.start, first_arg.location.end)),
        Vec::new(),
        Vec::new(),
    ));
    walk_node(first_arg, state);
    true
}

fn is_declaration_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !matches!(name, "fun" | "func" | "method" | "sub" | "ADJUST")
}

fn name_anchor_from_call(call: &Node, name: &str, file_id: FileId) -> SourceAnchor {
    signature_keyword_anchor(
        file_id,
        call.location.start,
        call.location.start.saturating_add(name.len()),
    )
}

fn parameters_from_signature(
    signature: Option<&Node>,
    source: &str,
) -> (Vec<SignatureKeywordParameter>, Vec<String>) {
    let Some(signature) = signature else {
        return (Vec::new(), Vec::new());
    };
    let NodeKind::Signature { parameters } = &signature.kind else {
        return (Vec::new(), vec!["signature-form-unsupported".to_string()]);
    };
    classify_parameter_nodes(parameters, source)
}

fn parameters_from_call_args(
    args: &[Node],
    source: &str,
) -> (Vec<SignatureKeywordParameter>, Vec<String>) {
    classify_parameter_nodes(args, source)
}

fn classify_parameter_nodes(
    nodes: &[Node],
    source: &str,
) -> (Vec<SignatureKeywordParameter>, Vec<String>) {
    let mut parameters = Vec::new();
    let mut limitations = Vec::new();
    for node in nodes {
        match classify_one_parameter(node, source) {
            Ok(parameter) => parameters.push(parameter),
            Err(limitation) => {
                if !limitations.contains(&limitation) {
                    limitations.push(limitation);
                }
            }
        }
    }
    (parameters, limitations)
}

fn classify_one_parameter(node: &Node, source: &str) -> Result<SignatureKeywordParameter, String> {
    if parameter_span_has_type_constraint(source, node) {
        return Err("typed-parameter-unsupported".to_string());
    }
    match &node.kind {
        NodeKind::MandatoryParameter { variable } => {
            variable_parameter(variable, SignatureParameterKind::Positional)
        }
        NodeKind::OptionalParameter { variable, default_value, .. } => {
            if is_literal_default(default_value) {
                variable_parameter(variable, SignatureParameterKind::Optional)
            } else {
                Err("optional-default-not-literal".to_string())
            }
        }
        NodeKind::SlurpyParameter { variable } => {
            variable_parameter(variable, SignatureParameterKind::Slurpy)
        }
        NodeKind::NamedParameter { .. } => Err("named-parameter-unsupported".to_string()),
        NodeKind::Variable { sigil, name } => Ok(SignatureKeywordParameter::new(
            name.clone(),
            sigil.clone(),
            if sigil == "@" || sigil == "%" {
                SignatureParameterKind::Slurpy
            } else {
                SignatureParameterKind::Positional
            },
        )),
        NodeKind::Assignment { op, lhs, rhs, .. } if op == "=" => {
            let NodeKind::Variable { sigil, name } = &lhs.kind else {
                return Err("optional-form-unsupported".to_string());
            };
            if is_literal_default(rhs) {
                Ok(SignatureKeywordParameter::new(
                    name.clone(),
                    sigil.clone(),
                    SignatureParameterKind::Optional,
                ))
            } else {
                Err("optional-default-not-literal".to_string())
            }
        }
        NodeKind::FunctionCall { .. } => Err("typed-parameter-unsupported".to_string()),
        _ => Err("parameter-form-unsupported".to_string()),
    }
}

fn variable_parameter(
    variable: &Node,
    kind: SignatureParameterKind,
) -> Result<SignatureKeywordParameter, String> {
    let NodeKind::Variable { sigil, name } = &variable.kind else {
        return Err("parameter-form-unsupported".to_string());
    };
    Ok(SignatureKeywordParameter::new(name.clone(), sigil.clone(), kind))
}

fn is_literal_default(node: &Node) -> bool {
    matches!(
        node.kind,
        NodeKind::Number { .. }
            | NodeKind::String { .. }
            | NodeKind::Undef
            | NodeKind::ArrayLiteral { .. }
            | NodeKind::HashLiteral { .. }
    )
}

/// Opening `{` after the callee, closed by the Binary `{}` node's AST end.
///
/// The closer is not found by scanning source for a matching `}`, so a
/// comment `}` cannot truncate the body range.
fn block_operator_span(
    source: &str,
    after_callee: usize,
    block_end: usize,
) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut index = after_callee.min(bytes.len());
    let end = block_end.min(bytes.len());
    index = skip_ascii_whitespace_and_line_comments(bytes, index, end);
    if bytes.get(index).copied() != Some(b'{') {
        return None;
    }
    Some((index, end))
}

fn skip_ascii_whitespace_and_line_comments(bytes: &[u8], mut index: usize, end: usize) -> usize {
    while index < end {
        if bytes[index].is_ascii_whitespace() {
            index = index.saturating_add(1);
            continue;
        }
        if bytes[index] == b'#' {
            while index < end && bytes[index] != b'\n' {
                index = index.saturating_add(1);
            }
            continue;
        }
        break;
    }
    index
}

fn gap_contains_statement_semicolon(gap: &str) -> bool {
    let mut in_comment = false;
    for character in gap.chars() {
        if in_comment {
            if character == '\n' {
                in_comment = false;
            }
            continue;
        }
        if character == '#' {
            in_comment = true;
            continue;
        }
        if character == ';' {
            return true;
        }
    }
    false
}

fn parameter_span_has_type_constraint(source: &str, node: &Node) -> bool {
    let Some(span) = source.get(node.location.start..node.location.end) else {
        return false;
    };
    let Some(sigil_at) = span.find(['$', '@', '%']) else {
        return false;
    };
    span.get(..sigil_at).is_some_and(|prefix| prefix.chars().any(|ch| ch.is_ascii_alphabetic()))
}

fn saturate_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
