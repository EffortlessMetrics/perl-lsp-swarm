//! POD section detection helpers for native critic rules.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MissingPodSection {
    pub(super) name: &'static str,
    pub(super) range_start: usize,
    pub(super) range_end: usize,
}

pub(super) fn missing_pod_sections(source: &str) -> Vec<MissingPodSection> {
    const REQUIRED: &[&str] = &["NAME", "DESCRIPTION"];

    let mut has_pod = false;
    let mut sections = Vec::new();
    let mut first_pod_span = None;
    let mut byte_offset = 0;

    for line in source.split_inclusive('\n') {
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        let trimmed = line_without_newline.trim_start();

        if let Some(section) = trimmed.strip_prefix("=head1") {
            has_pod = true;
            first_pod_span.get_or_insert((byte_offset, byte_offset + line_without_newline.len()));

            let section_name = section
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches(|ch: char| !ch.is_alphanumeric())
                .to_ascii_uppercase();
            if !section_name.is_empty() {
                sections.push(section_name);
            }
        } else if trimmed.starts_with("=pod")
            || trimmed.starts_with("=over")
            || trimmed.starts_with("=item")
            || trimmed.starts_with("=begin")
        {
            has_pod = true;
            first_pod_span.get_or_insert((byte_offset, byte_offset + line_without_newline.len()));
        }

        byte_offset += line.len();
    }

    if !has_pod {
        return Vec::new();
    }

    let (range_start, range_end) = first_pod_span.unwrap_or((0, source.len().min(1)));

    REQUIRED
        .iter()
        .filter(|required| !sections.iter().any(|section| section == **required))
        .map(|name| MissingPodSection { name, range_start, range_end })
        .collect()
}
