// Build typed compile-directive syntax from parser-local facts (#8232).

use perl_ast::{
    CompileDirectiveSubject, CompileDirectiveSyntax, ImportArgumentItem, ImportArgumentList,
    ImportListPresence, ModuleNameSyntax, VersionRequirementSyntax,
};

/// Tracks import-list shape while parsing `use` / `no`.
#[derive(Debug, Default, Clone)]
pub struct DirectiveImportAcc {
    pub presence: ImportListPresence,
    pub items: Vec<ImportArgumentItem>,
}

impl DirectiveImportAcc {
    pub fn record_qw_words(&mut self, words: &[(String, SourceLocation)], projection: &str) {
        let list_span = span_covering(words.iter().map(|(_, span)| *span));
        self.presence = ImportListPresence::Present { list_span };
        self.items.push(ImportArgumentItem::NormalizedQwList {
            projection: projection.to_string(),
            span: list_span,
        });
        for (value, span) in words {
            self.items.push(ImportArgumentItem::QwWord { value: value.clone(), span: *span });
        }
    }

    pub fn record_string_literal(&mut self, raw: String, span: SourceLocation) {
        self.presence = ImportListPresence::Present { list_span: span };
        self.items.push(ImportArgumentItem::StringLiteral { raw, span });
    }

    pub fn record_explicit_empty(&mut self, paren_span: SourceLocation) {
        self.presence = ImportListPresence::ExplicitEmpty { paren_span };
    }
}

pub fn build_directive_syntax(
    source: &str,
    node_span: SourceLocation,
    legacy_module: &str,
    legacy_args: &[String],
    module_span: SourceLocation,
    version: Option<VersionRequirementSyntax>,
    import_acc: &DirectiveImportAcc,
) -> CompileDirectiveSyntax {
    let subject = classify_subject(legacy_module, module_span, version);
    let mut arguments = ImportArgumentList {
        presence: import_acc.presence.clone(),
        items: import_acc.items.clone(),
    };
    if arguments.items.is_empty() && !legacy_args.is_empty() {
        // Fallback: preserve legacy projections when only strings were collected.
        if legacy_args.len() == 1 && legacy_args[0].starts_with("qw(") {
            let span = infer_list_span(source, node_span, legacy_args[0].as_str());
            arguments.presence = ImportListPresence::Present { list_span: span };
            push_qw_projection_items(source, node_span, legacy_args[0].as_str(), span, &mut arguments.items);
        } else {
            for arg in legacy_args {
                let span = infer_arg_span(source, node_span, arg);
                if arg.starts_with('\'') || arg.starts_with('"') {
                    arguments.items.push(ImportArgumentItem::StringLiteral {
                        raw: arg.clone(),
                        span,
                    });
                } else {
                    arguments.items.push(ImportArgumentItem::NormalizedQwList {
                        projection: arg.clone(),
                        span,
                    });
                }
            }
            if !legacy_args.is_empty() {
                arguments.presence = ImportListPresence::Present {
                    list_span: span_covering(arguments.items.iter().filter_map(|item| item_span(item))),
                };
            }
        }
    }
    if matches!(arguments.presence, ImportListPresence::Absent)
        && legacy_args.is_empty()
        && node_slice(source, node_span).contains("()")
    {
        if let Some(paren_span) = find_empty_paren_span(source, node_span) {
            arguments.presence = ImportListPresence::ExplicitEmpty { paren_span };
        }
    }
    CompileDirectiveSyntax { subject, arguments }
}

fn classify_subject(
    legacy_module: &str,
    module_span: SourceLocation,
    version: Option<VersionRequirementSyntax>,
) -> CompileDirectiveSubject {
    if let Some(version) = version {
        let base = legacy_module.split_whitespace().next().unwrap_or(legacy_module);
        if base.starts_with('v') && legacy_module.split_whitespace().count() <= 1 {
            return CompileDirectiveSubject::VersionPragma(version);
        }
        return CompileDirectiveSubject::ModuleWithVersion {
            module: ModuleNameSyntax { name: base.to_string(), span: module_span },
            version,
        };
    }
    if legacy_module.starts_with('v')
        && legacy_module.len() > 1
        && legacy_module.chars().skip(1).all(|c| c.is_ascii_digit() || c == '.')
    {
        return CompileDirectiveSubject::VersionPragma(VersionRequirementSyntax::VString {
            raw: legacy_module.to_string(),
            normalized: legacy_module.to_string(),
            span: module_span,
        });
    }
    CompileDirectiveSubject::Module(ModuleNameSyntax {
        name: legacy_module.split_whitespace().next().unwrap_or(legacy_module).to_string(),
        span: module_span,
    })
}

fn push_qw_projection_items(
    source: &str,
    node_span: SourceLocation,
    projection: &str,
    list_span: SourceLocation,
    items: &mut Vec<ImportArgumentItem>,
) {
    items.push(ImportArgumentItem::NormalizedQwList {
        projection: projection.to_string(),
        span: list_span,
    });
    if let Some(inner) = projection.strip_prefix("qw(").and_then(|s| s.strip_suffix(')')) {
        let words: Vec<String> = inner.split_whitespace().map(ToString::to_string).collect();
        for (value, span) in infer_qw_word_spans(source, node_span, &words) {
            items.push(ImportArgumentItem::QwWord { value, span });
        }
    }
}

fn item_span(item: &ImportArgumentItem) -> Option<SourceLocation> {
    match item {
        ImportArgumentItem::QwWord { span, .. }
        | ImportArgumentItem::StringLiteral { span, .. }
        | ImportArgumentItem::NormalizedQwList { span, .. }
        | ImportArgumentItem::ExpressionBoundary { span } => Some(*span),
        _ => None,
    }
}

fn span_covering<I: Iterator<Item = SourceLocation>>(spans: I) -> SourceLocation {
    let mut iter = spans.peekable();
    let Some(first) = iter.next() else {
        return SourceLocation { start: 0, end: 0 };
    };
    let mut start = first.start;
    let mut end = first.end;
    for span in iter {
        start = start.min(span.start);
        end = end.max(span.end);
    }
    SourceLocation { start, end }
}

fn node_slice(source: &str, span: SourceLocation) -> &str {
    source.get(span.start..span.end.min(source.len())).unwrap_or("")
}

fn infer_arg_span(source: &str, node_span: SourceLocation, arg: &str) -> SourceLocation {
    let slice = node_slice(source, node_span);
    if let Some(idx) = slice.find(arg) {
        let start = node_span.start + idx;
        SourceLocation { start, end: start + arg.len() }
    } else {
        SourceLocation { start: node_span.start, end: node_span.start }
    }
}

fn infer_list_span(source: &str, node_span: SourceLocation, projection: &str) -> SourceLocation {
    infer_arg_span(source, node_span, projection)
}

fn find_empty_paren_span(source: &str, node_span: SourceLocation) -> Option<SourceLocation> {
    let slice = node_slice(source, node_span);
    let idx = slice.find("()")?;
    let start = node_span.start + idx;
    Some(SourceLocation { start, end: start + 2 })
}

pub fn numeric_version_from_token(text: &str, span: SourceLocation) -> VersionRequirementSyntax {
    VersionRequirementSyntax::Numeric {
        raw: text.to_string(),
        normalized: text.to_string(),
        span,
    }
}

pub fn vstring_version_from_token(text: &str, span: SourceLocation) -> VersionRequirementSyntax {
    VersionRequirementSyntax::VString {
        raw: text.to_string(),
        normalized: text.to_string(),
        span,
    }
}

pub fn extract_version_syntax(
    source: &str,
    node_span: SourceLocation,
    legacy_module: &str,
) -> Option<VersionRequirementSyntax> {
    let parts: Vec<&str> = legacy_module.split_whitespace().collect();
    if parts.len() >= 2 {
        let ver = parts[1];
        let slice = node_slice(source, node_span);
        if let Some(idx) = slice.find(ver) {
            let start = node_span.start + idx;
            return Some(numeric_version_from_token(
                ver,
                SourceLocation { start, end: start + ver.len() },
            ));
        }
    }
    None
}

pub fn infer_qw_word_spans(
    source: &str,
    node_span: SourceLocation,
    words: &[String],
) -> Vec<(String, SourceLocation)> {
    let mut out = Vec::new();
    let slice = node_slice(source, node_span);
    let mut search_from = slice.find("qw").unwrap_or(0);
    for word in words {
        if let Some(rel) = slice[search_from..].find(word.as_str()) {
            let idx = search_from + rel;
            let start = node_span.start + idx;
            out.push((word.clone(), SourceLocation { start, end: start + word.len() }));
            search_from = idx + word.len();
        } else {
            out.push((word.clone(), SourceLocation { start: node_span.start, end: node_span.start }));
        }
    }
    out
}

pub fn use_kind_from_legacy(
    source: &str,
    span: SourceLocation,
    module: String,
    args: Vec<String>,
    has_filter_risk: bool,
) -> NodeKind {
    use crate::ast::NodeKind;
    let module_span = module_span_after_use_keyword(source, span.start, &module);
    let version = extract_version_syntax(source, span, &module);
    let syntax = build_directive_syntax(
        source,
        span,
        &module,
        &args,
        module_span,
        version,
        &DirectiveImportAcc::default(),
    );
    NodeKind::Use { module, module_span, syntax, args, has_filter_risk }
}

pub fn no_kind_from_legacy(
    source: &str,
    span: SourceLocation,
    module: String,
    args: Vec<String>,
    has_filter_risk: bool,
) -> NodeKind {
    use crate::ast::NodeKind;
    let module_span = module_span_after_use_keyword(source, span.start, &module);
    let version = extract_version_syntax(source, span, &module);
    let syntax = build_directive_syntax(
        source,
        span,
        &module,
        &args,
        module_span,
        version,
        &DirectiveImportAcc::default(),
    );
    NodeKind::No { module, module_span, syntax, args, has_filter_risk }
}

pub fn module_span_after_use_keyword(source: &str, use_start: usize, module_name: &str) -> SourceLocation {
    let slice = source.get(use_start..).unwrap_or("");
    let needle = module_name.split_whitespace().next().unwrap_or(module_name);
    if let Some(rel) = slice.find(needle) {
        let start = use_start + rel;
        SourceLocation { start, end: start + needle.len() }
    } else {
        SourceLocation { start: use_start, end: use_start }
    }
}
