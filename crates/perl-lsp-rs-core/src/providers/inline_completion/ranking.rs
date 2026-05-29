use super::{ExpectedSyntax, FileRole, InlineCompletionItem, SemanticInlineContext, VariableFact};

#[derive(Debug)]
pub(super) struct RankedCompletionItem {
    pub(super) score: InlineCandidateScore,
    pub(super) order: usize,
    pub(super) item: InlineCompletionItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InlineCandidateScore(pub(super) i16);

impl InlineCandidateScore {
    const LEGACY_PRIORITY_STEP: i16 = 100;

    pub(super) fn for_candidate(
        source: InlineCandidateSourceKind,
        priority: u8,
        item: &InlineCompletionItem,
        semantic_context: &SemanticInlineContext,
    ) -> Self {
        Self(Self::legacy_base(priority) + semantic_bonus(source, item, semantic_context))
    }

    pub(super) fn legacy_base(priority: u8) -> i16 {
        10_000 - i16::from(priority) * Self::LEGACY_PRIORITY_STEP
    }

    #[cfg(test)]
    pub(super) fn from_legacy_priority(priority: u8) -> Self {
        Self(Self::legacy_base(priority))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InlineCandidateSourceKind {
    Receiver,
    Module,
    Syntax,
    Test,
    Shebang,
    ContextualFallback,
}

fn semantic_bonus(
    source: InlineCandidateSourceKind,
    item: &InlineCompletionItem,
    context: &SemanticInlineContext,
) -> i16 {
    match source {
        InlineCandidateSourceKind::Receiver => receiver_candidate_bonus(item, context),
        InlineCandidateSourceKind::Module => module_candidate_bonus(item, context),
        InlineCandidateSourceKind::Syntax => syntax_candidate_bonus(item, context),
        InlineCandidateSourceKind::Test => test_candidate_bonus(context),
        InlineCandidateSourceKind::Shebang => shebang_candidate_bonus(context),
        InlineCandidateSourceKind::ContextualFallback => {
            contextual_fallback_candidate_bonus(item, context)
        }
    }
}

fn module_candidate_bonus(item: &InlineCompletionItem, context: &SemanticInlineContext) -> i16 {
    if context.expected_syntax != ExpectedSyntax::UseModule {
        return 0;
    }

    let module_name = item.insert_text.trim_end_matches(';');
    if context
        .available_modules
        .binary_search_by(|module| module.name.as_str().cmp(module_name))
        .is_ok()
    {
        return 35;
    }

    0
}

fn receiver_candidate_bonus(item: &InlineCompletionItem, context: &SemanticInlineContext) -> i16 {
    if context.expected_syntax != ExpectedSyntax::MethodName {
        return 0;
    }

    let method_name = item.insert_text.trim_end_matches("()");
    if context.current_package_methods.iter().any(|method| method.name == method_name) {
        return 30;
    }

    10
}

fn syntax_candidate_bonus(item: &InlineCompletionItem, context: &SemanticInlineContext) -> i16 {
    match context.expected_syntax {
        ExpectedSyntax::UseModule
            if matches!(
                item.insert_text.as_str(),
                "strict;" | "warnings;" | "feature ':5.36';"
            ) =>
        {
            20
        }
        ExpectedSyntax::ReturnExpression | ExpectedSyntax::GuardCondition
            if item.insert_text.ends_with(';') =>
        {
            20
        }
        ExpectedSyntax::LexicalVariableName
            if item.insert_text.starts_with("self =")
                && context.visible_variables.iter().any(VariableFact::is_scalar_self) =>
        {
            20
        }
        ExpectedSyntax::PackageName
        | ExpectedSyntax::BlessArguments
        | ExpectedSyntax::LoopBinding => 15,
        ExpectedSyntax::SubroutineBody if item.insert_text.starts_with(" {") => 15,
        _ => 0,
    }
}

fn test_candidate_bonus(context: &SemanticInlineContext) -> i16 {
    match context.expected_syntax {
        ExpectedSyntax::TestAssertionArguments => 30,
        _ if context.file_role == FileRole::Test => 20,
        _ => 0,
    }
}

fn shebang_candidate_bonus(context: &SemanticInlineContext) -> i16 {
    if context.expected_syntax == ExpectedSyntax::ShebangInterpreter { 20 } else { 0 }
}

fn contextual_fallback_candidate_bonus(
    item: &InlineCompletionItem,
    context: &SemanticInlineContext,
) -> i16 {
    if context.file_role == FileRole::Test
        && (item.insert_text.starts_with("is(") || item.insert_text.starts_with("ok("))
    {
        return 25;
    }

    if item.insert_text.starts_with("return ")
        && matches!(context.expected_syntax, ExpectedSyntax::EmptyStatement)
        && !context.visible_variables.is_empty()
    {
        return 15;
    }

    if item.insert_text == "done_testing();" && context.file_role == FileRole::Test {
        return 10;
    }

    0
}
