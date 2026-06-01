//! Candidate ranking and semantic scoring for inline completions.

use super::{
    ExpectedSyntax, FileRole, InlineCompletionItem, ReceiverHint, SemanticInlineContext,
    VariableFact,
};

#[derive(Debug)]
pub(super) struct RankedCompletionItem {
    pub(super) score: InlineCandidateScore,
    pub(super) order: usize,
    pub(super) metadata: InlineCandidateMetadata,
    pub(super) item: InlineCompletionItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InlineCandidateMetadata {
    pub(super) source: InlineCandidateSourceKind,
    pub(super) reason: InlineCandidateReason,
    pub(super) confidence: InlineCandidateConfidence,
}

impl InlineCandidateMetadata {
    pub(super) fn for_candidate(
        source: InlineCandidateSourceKind,
        item: &InlineCompletionItem,
        semantic_context: &SemanticInlineContext,
    ) -> Self {
        let reason = InlineCandidateReason::for_candidate(source, item, semantic_context);
        let confidence = InlineCandidateConfidence::for_reason(reason);
        Self { source, reason, confidence }
    }

    pub(super) fn stable_tiebreak(self) -> u8 {
        self.source.stable_rank() * 32
            + self.reason.stable_rank() * 4
            + self.confidence.stable_rank()
    }

    #[cfg(test)]
    pub(super) fn test_fixture() -> Self {
        Self {
            source: InlineCandidateSourceKind::Syntax,
            reason: InlineCandidateReason::SourceSyntax,
            confidence: InlineCandidateConfidence::Medium,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InlineCandidateReason {
    CurrentPackageMethod,
    DbiReceiverMethod,
    EffectiveIncModule,
    VisibleLexical,
    SourceReceiver,
    SourceModule,
    SourceSyntax,
    SourceTest,
    SourceShebang,
    SourceContextualFallback,
}

impl InlineCandidateReason {
    pub(super) fn for_candidate(
        source: InlineCandidateSourceKind,
        item: &InlineCompletionItem,
        semantic_context: &SemanticInlineContext,
    ) -> Self {
        match source {
            InlineCandidateSourceKind::Receiver => {
                receiver_candidate_reason(item, semantic_context)
            }
            InlineCandidateSourceKind::Module => module_candidate_reason(item, semantic_context),
            InlineCandidateSourceKind::Syntax => syntax_candidate_reason(semantic_context),
            InlineCandidateSourceKind::Test => Self::SourceTest,
            InlineCandidateSourceKind::Shebang => Self::SourceShebang,
            InlineCandidateSourceKind::ContextualFallback => Self::SourceContextualFallback,
        }
    }

    pub(super) fn stable_rank(self) -> u8 {
        match self {
            Self::CurrentPackageMethod => 0,
            Self::DbiReceiverMethod => 1,
            Self::EffectiveIncModule => 2,
            Self::VisibleLexical => 3,
            Self::SourceReceiver => 4,
            Self::SourceModule => 5,
            Self::SourceSyntax => 6,
            Self::SourceTest => 7,
            Self::SourceShebang => 8,
            Self::SourceContextualFallback => 9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InlineCandidateConfidence {
    High,
    Medium,
    Low,
}

impl InlineCandidateConfidence {
    fn for_reason(reason: InlineCandidateReason) -> Self {
        match reason {
            InlineCandidateReason::CurrentPackageMethod
            | InlineCandidateReason::DbiReceiverMethod
            | InlineCandidateReason::EffectiveIncModule
            | InlineCandidateReason::VisibleLexical
            | InlineCandidateReason::SourceTest => Self::High,
            InlineCandidateReason::SourceSyntax | InlineCandidateReason::SourceShebang => {
                Self::Medium
            }
            InlineCandidateReason::SourceReceiver
            | InlineCandidateReason::SourceModule
            | InlineCandidateReason::SourceContextualFallback => Self::Low,
        }
    }

    pub(super) fn stable_rank(self) -> u8 {
        match self {
            Self::High => 0,
            Self::Medium => 1,
            Self::Low => 2,
        }
    }
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

impl InlineCandidateSourceKind {
    pub(super) fn stable_rank(self) -> u8 {
        match self {
            Self::Receiver => 0,
            Self::Module => 1,
            Self::Syntax => 2,
            Self::Test => 3,
            Self::Shebang => 4,
            Self::ContextualFallback => 5,
        }
    }
}

fn receiver_candidate_reason(
    item: &InlineCompletionItem,
    context: &SemanticInlineContext,
) -> InlineCandidateReason {
    let method_name = item.insert_text.trim_end_matches("()");
    if receiver_targets_current_package(context)
        && context.current_package_methods.iter().any(|method| method.name == method_name)
    {
        return InlineCandidateReason::CurrentPackageMethod;
    }

    if context.dbi_receiver_kind.is_some() {
        return InlineCandidateReason::DbiReceiverMethod;
    }

    InlineCandidateReason::SourceReceiver
}

pub(super) fn receiver_targets_current_package(context: &SemanticInlineContext) -> bool {
    match context.receiver_hint.as_ref() {
        Some(ReceiverHint::SelfReceiver) => true,
        Some(ReceiverHint::Package(package)) => context
            .package
            .as_deref()
            .is_some_and(|current_package| package == "__PACKAGE__" || package == current_package),
        _ => false,
    }
}

fn module_candidate_reason(
    item: &InlineCompletionItem,
    context: &SemanticInlineContext,
) -> InlineCandidateReason {
    let module_name = item.insert_text.trim_end_matches(';');
    if context.available_modules.iter().any(|module| module.name == module_name) {
        return InlineCandidateReason::EffectiveIncModule;
    }

    InlineCandidateReason::SourceModule
}

fn syntax_candidate_reason(context: &SemanticInlineContext) -> InlineCandidateReason {
    match context.expected_syntax {
        ExpectedSyntax::ReturnExpression
        | ExpectedSyntax::GuardCondition
        | ExpectedSyntax::ConditionExpression
        | ExpectedSyntax::LoopBinding => InlineCandidateReason::VisibleLexical,
        _ => InlineCandidateReason::SourceSyntax,
    }
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
        ExpectedSyntax::ConditionExpression if item.insert_text.ends_with(") {\n    \n}") => 20,
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

#[derive(Debug)]
pub(super) struct InlineCandidateSink<'a> {
    semantic_context: &'a SemanticInlineContext,
    items: Vec<RankedCompletionItem>,
    sequence: usize,
}

impl<'a> InlineCandidateSink<'a> {
    pub(super) fn new(semantic_context: &'a SemanticInlineContext) -> Self {
        Self { semantic_context, items: Vec::new(), sequence: 0 }
    }

    pub(super) fn push(
        &mut self,
        source: InlineCandidateSourceKind,
        priority: u8,
        item: InlineCompletionItem,
    ) {
        let score =
            InlineCandidateScore::for_candidate(source, priority, &item, self.semantic_context);
        let metadata = InlineCandidateMetadata::for_candidate(source, &item, self.semantic_context);
        self.items.push(RankedCompletionItem { score, order: self.sequence, metadata, item });
        self.sequence += 1;
    }

    pub(super) fn into_items(self) -> Vec<RankedCompletionItem> {
        self.items
    }
}
