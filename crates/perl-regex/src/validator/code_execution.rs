use crate::syntax::event::{RegexEmbeddedCodeKind, RegexEventKind, RegexEventStream};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmbeddedCodeKind {
    Immediate,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EmbeddedCodeFinding {
    pub(crate) offset: usize,
    pub(crate) kind: EmbeddedCodeKind,
}

pub(crate) fn find_code_executions(stream: &RegexEventStream) -> Vec<EmbeddedCodeFinding> {
    stream
        .events
        .iter()
        .filter_map(|event| match event.kind {
            RegexEventKind::EmbeddedCode { kind, opener_range, .. } => {
                let kind = match kind {
                    RegexEmbeddedCodeKind::Immediate => EmbeddedCodeKind::Immediate,
                    RegexEmbeddedCodeKind::Deferred => EmbeddedCodeKind::Deferred,
                };
                Some(EmbeddedCodeFinding { offset: opener_range.start, kind })
            }
            _ => None,
        })
        .collect()
}
