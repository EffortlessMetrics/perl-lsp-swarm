from pathlib import Path

path = Path("xtask/src/tasks/agent_flow.rs")
text = path.read_text()


def replace_once(old: str, new: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one occurrence, found {count}: {old!r}")
    text = text.replace(old, new, 1)


replace_once(
    'const METASYNTACTIC_PLACEHOLDERS: &[&str] = &["skill", "skill_name", "skill-name"];\n',
    'const METASYNTACTIC_PLACEHOLDERS: &[&str] = &["skill", "skill_name", "skill-name"];\n'
    'const ROUTE_BEARING_LABELS: &[&str] = &[\n'
    '    "entry flow",\n'
    '    "entry route",\n'
    '    "next flow",\n'
    '    "next route",\n'
    '    "return flow",\n'
    '    "return route",\n'
    '    "fallback flow",\n'
    '    "fallback route",\n'
    '];\n',
)

replace_once(
    '    ImperativeInvocation,\n    ProseMention,\n',
    '    ImperativeInvocation,\n    LabeledTarget,\n    ProseMention,\n',
)

replace_once(
    '                | Self::ImperativeInvocation\n        )\n',
    '                | Self::ImperativeInvocation\n                | Self::LabeledTarget\n        )\n',
)

replace_once(
    '    if is_markdown_list_item(trimmed) && has_imperative_route_prefix(candidate, code_span) {\n'
    '        return RouteSyntax::ImperativeInvocation;\n'
    '    }\n'
    '    RouteSyntax::InlineCode\n'
    '}\n\n'
    'fn has_imperative_route_prefix(candidate: &str, code_span: &str) -> bool {\n',
    '    if is_markdown_list_item(trimmed) && has_route_label_prefix(candidate, code_span) {\n'
    '        return RouteSyntax::LabeledTarget;\n'
    '    }\n'
    '    if is_markdown_list_item(trimmed) && has_imperative_route_prefix(candidate, code_span) {\n'
    '        return RouteSyntax::ImperativeInvocation;\n'
    '    }\n'
    '    RouteSyntax::InlineCode\n'
    '}\n\n'
    'fn has_route_label_prefix(candidate: &str, code_span: &str) -> bool {\n'
    '    let Some(index) = candidate.find(code_span) else {\n'
    '        return false;\n'
    '    };\n'
    '    let prefix = candidate[..index].trim();\n'
    '    let label = if let Some(without_colon) = prefix.strip_suffix(\':\') {\n'
    '        strip_strong_emphasis(without_colon).trim()\n'
    '    } else {\n'
    '        let without_emphasis = strip_strong_emphasis(prefix);\n'
    '        let Some(without_colon) = without_emphasis.strip_suffix(\':\') else {\n'
    '            return false;\n'
    '        };\n'
    '        without_colon.trim()\n'
    '    };\n'
    '    let normalized = label.to_ascii_lowercase();\n'
    '    ROUTE_BEARING_LABELS.contains(&normalized.as_str())\n'
    '}\n\n'
    'fn strip_strong_emphasis(text: &str) -> &str {\n'
    '    text.strip_prefix("**")\n'
    '        .and_then(|inner| inner.strip_suffix("**"))\n'
    '        .unwrap_or(text)\n'
    '}\n\n'
    'fn has_imperative_route_prefix(candidate: &str, code_span: &str) -> bool {\n',
)

marker = """    #[test]
    fn existing_skill_name_in_prose_is_a_prose_mention() {
"""
tests = """    #[test]
    fn preserves_labeled_route_fields_as_edges() {
        let text = "## Routes\\n- Entry flow: `deliver-pr`\\n- **Next route:** `finish-pr`\\n";
        let observations = route_observations(text);
        assert_eq!(edge_targets(&observations), vec!["deliver-pr", "finish-pr"]);
        assert!(
            observations
                .iter()
                .all(|observation| observation.syntax == RouteSyntax::LabeledTarget)
        );
    }

    #[test]
    fn labeled_route_typos_remain_load_bearing() {
        let observations = route_line_observations("- Entry flow: `delver-pr`", 1, true);
        assert_eq!(edge_targets(&observations), vec!["delver-pr"]);
        assert_eq!(
            resolve_route_syntax(&observations[0], &BTreeSet::new()),
            RouteSyntax::LabeledTarget
        );
    }

    #[test]
    fn unrelated_labeled_code_remains_non_executable() {
        let observations = route_line_observations("- Cache key: `deliver-pr`", 1, true);
        assert!(edge_targets(&observations).is_empty());
        assert_eq!(observations[0].syntax, RouteSyntax::InlineCode);
    }

"""
replace_once(marker, tests + marker)

path.write_text(text)
