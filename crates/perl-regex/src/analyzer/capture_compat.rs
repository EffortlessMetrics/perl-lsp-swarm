use super::{
    capture::{
        CaptureDiagnosticCode, CaptureGroup, CaptureLanguageProfile, CaptureNumberConfidence,
        CaptureProfileConfidence, CaptureSourceConfidence, analyze_captures,
        extract_named_captures as direct_projection,
    },
    modifier_analysis::EffectiveModifiers,
};

pub(crate) fn extract_named_captures(pattern: &str) -> Vec<CaptureGroup> {
    let analysis = analyze_captures(
        pattern,
        EffectiveModifiers::default(),
        CaptureLanguageProfile::unknown(),
    );
    let invalid_name_ranges = analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == CaptureDiagnosticCode::InvalidName)
        .map(|diagnostic| diagnostic.range)
        .collect::<Vec<_>>();

    if invalid_name_ranges.is_empty() {
        return direct_projection(pattern);
    }

    let mut fallback_next = 1usize;
    analysis
        .declarations
        .into_iter()
        .filter_map(|declaration| {
            let name = declaration.name?;
            if declaration.confidence.source != CaptureSourceConfidence::Exact
                || declaration.confidence.profile == CaptureProfileConfidence::Incompatible
            {
                return None;
            }

            let index = match declaration.number {
                Some(number) => {
                    let index = usize::try_from(number).ok()?;
                    fallback_next = fallback_next.max(index.saturating_add(1));
                    index
                }
                None
                    if declaration.confidence.number
                        == CaptureNumberConfidence::StructuralUnknown
                        && invalid_name_ranges
                            .iter()
                            .any(|range| range.start < declaration.group_range.start) =>
                {
                    let index = fallback_next;
                    fallback_next = fallback_next.saturating_add(1);
                    index
                }
                None => return None,
            };

            let subpattern = pattern
                .get(declaration.body_range.start..declaration.body_range.end)?
                .to_string();
            Some(CaptureGroup { name, index, pattern: subpattern })
        })
        .collect()
}
