mod context;
mod dispatch;
mod interpolation;
mod test_frameworks;

use super::{
    CompletionItem, CompletionProvider,
    lexical_context::{InterpolationAdmission, interpolation_admission},
    sort,
};

pub(super) fn complete(
    provider: &CompletionProvider,
    source: &str,
    position: usize,
    filepath: Option<&str>,
    is_cancelled: &dyn Fn() -> bool,
) -> Vec<CompletionItem> {
    let Some(context) = context::prepare_context(provider, source, position) else {
        return vec![];
    };

    if is_cancelled() || context::rejects_dash_trigger(&context) {
        return vec![];
    }

    match interpolation_admission(source, position) {
        InterpolationAdmission::VariableSlot { .. } => {
            return interpolation::complete_slot(
                provider,
                &context,
                source,
                position,
                is_cancelled,
            );
        }
        InterpolationAdmission::Quiet
            if !context.in_string || prefix_starts_with_sigil(&context.prefix) =>
        {
            // Quiet interpolating/non-interpolating sigil slots and heredoc
            // bodies must not fall through to workspace package fallback.
            return vec![];
        }
        InterpolationAdmission::Quiet | InterpolationAdmission::NotOwned => {}
    }

    // POD still suppresses all completion. Interpolating heredocs are admitted
    // above; literal heredocs are Quiet.
    if context::rejects_lexical_block(source, position) {
        return vec![];
    }

    let mut completions = Vec::new();
    if let Some(regex_completions) =
        context::complete_regex_context(&mut completions, &context, source)
    {
        return regex_completions;
    }

    match dispatch::complete_dispatch(
        provider,
        &mut completions,
        &context,
        source,
        position,
        filepath,
        is_cancelled,
    ) {
        CompletionFlow::SortAndReturn => {
            test_frameworks::reconcile(&mut completions, provider, &context, source, filepath);
            sort::deduplicate_and_sort(completions)
        }
        CompletionFlow::Return(items) => items,
        CompletionFlow::Cancelled => vec![],
    }
}

fn prefix_starts_with_sigil(prefix: &str) -> bool {
    prefix.chars().next().is_some_and(|ch| matches!(ch, '$' | '@' | '%'))
}

pub(super) enum CompletionFlow {
    SortAndReturn,
    Return(Vec<CompletionItem>),
    Cancelled,
}
