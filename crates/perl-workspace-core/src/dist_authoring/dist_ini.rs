//! dist.ini authoring extraction (Dist::Zilla INI, no plugin execution).

use crate::id::FileId;
use crate::range::Utf8LineIndex;

use super::{
    DistAuthoringBuildTool, DistAuthoringFacts, DistAuthoringSource, DistCollector,
    DistDeclarationKind,
};

/// Plugins whose configuration is not a static fact source; they generate or
/// mutate metadata at `dzil` time.
const DYNAMIC_PLUGINS: &[&str] = &[
    "AutoPrereqs",
    "AutoVersion",
    "VersionFromModule",
    "Git::NextVersion",
    "PkgVersion",
    "OurPkgVersion",
    "PodVersion",
    "RewriteVersion",
    "GitHub::Meta",
    "AutoMetaResources",
    "MetaProvides::Package",
    "MetaProvides::Class",
    "MetaProvides::FromFile",
    "Prereqs::FromCPANfile",
    "Prereqs::AuthorDeps",
    "DynamicPrereqs",
    "OSPrereqs",
    "OptionalFeature",
];

/// Extract bounded static facts from Dist::Zilla `dist.ini`.
#[must_use]
pub fn parse_dist_ini(file_id: FileId, content: &str) -> DistAuthoringFacts {
    let index = Utf8LineIndex::new(content);
    let mut collector = DistCollector::new(
        file_id,
        DistAuthoringSource::DistIni,
        DistAuthoringBuildTool::DistZilla,
        content,
        &index,
    );

    let mut section = Section::Root;
    let mut line_start = 0usize;
    for line in content.split_inclusive('\n') {
        let line_end = line_start + line.len();
        let trimmed = strip_ini_comment(line).trim();
        if trimmed.is_empty() {
            line_start = line_end;
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            flush_prereq_section(&mut collector, &mut section, line_start, line_end);
            let raw = trimmed.trim_matches(['[', ']']).trim();
            section = Section::from_heading(raw);
            collector.note_plugin(raw, line_start, line_end);
            apply_section_identity(&mut collector, &section, line_start, line_end);
            line_start = line_end;
            continue;
        }
        let Some((key, value)) = split_ini_key_value(trimmed) else {
            collector.limitation(
                "malformed_ini",
                format!("skipped unreadable dist.ini line `{trimmed}`"),
                Some(line_start),
                Some(line_end),
            );
            line_start = line_end;
            continue;
        };
        let value_start = line_start + line.find(value).unwrap_or(0);
        let value_end = value_start + value.len();
        let key_start = line_start + line.find(key).unwrap_or(0);
        apply_ini_entry(
            &mut collector,
            &mut section,
            key,
            value,
            key_start,
            value_start,
            value_end,
        );
        line_start = line_end;
    }
    flush_prereq_section(&mut collector, &mut section, line_start, line_start);

    collector.finish()
}

#[derive(Debug, Clone)]
struct PendingPrereq {
    key: String,
    value: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

#[derive(Debug, Clone)]
enum Section {
    Root,
    Prereqs { phase: String, relation: String, recognized: bool, pending: Vec<PendingPrereq> },
    MetaResources,
    Other(String),
}

impl Section {
    fn from_heading(raw: &str) -> Self {
        if raw.starts_with('@') {
            return Self::Other(raw.to_string());
        }
        let (plugin, rest) = split_section_name(raw);
        let plugin_tail = plugin.rsplit("::").next().unwrap_or(plugin);
        if plugin_eq(plugin, "Prereqs") || plugin_tail.eq_ignore_ascii_case("Prereqs") {
            let (phase, relation, recognized) =
                phase_relation_from_label(rest.unwrap_or("RuntimeRequires"));
            return Self::Prereqs {
                phase,
                relation,
                recognized: rest.is_none() || recognized,
                pending: Vec::new(),
            };
        }
        if plugin_eq(plugin, "MetaResources") {
            return Self::MetaResources;
        }
        Self::Other(raw.to_string())
    }
}

fn apply_section_identity(
    collector: &mut DistCollector<'_>,
    section: &Section,
    start: usize,
    end: usize,
) {
    match section {
        Section::Other(name) if name.starts_with('@') => {
            collector.limitation(
                "bundle_plugin",
                format!("Dist::Zilla bundle `{name}` is recorded, not expanded"),
                Some(start),
                Some(end),
            );
        }
        Section::Other(name) if is_dynamic_plugin(name) => {
            collector.limitation(
                "plugin_generated",
                format!("Dist::Zilla plugin `{name}` can generate metadata and is not executed"),
                Some(start),
                Some(end),
            );
        }
        Section::Other(name) if plugin_eq(name, "MakeMaker") || name.ends_with("::MakeMaker") => {
            collector.set_generated_build_tool(
                DistAuthoringBuildTool::ExtUtilsMakeMaker,
                start,
                end,
            );
        }
        Section::Other(name)
            if plugin_eq(name, "ModuleBuild")
                || plugin_eq(name, "ModuleBuildTiny")
                || name.contains("ModuleBuild") =>
        {
            collector.set_generated_build_tool(DistAuthoringBuildTool::ModuleBuild, start, end);
        }
        _ => {}
    }
}

fn apply_ini_entry(
    collector: &mut DistCollector<'_>,
    section: &mut Section,
    key: &str,
    value: &str,
    key_start: usize,
    value_start: usize,
    value_end: usize,
) {
    let value = unquote(value);
    match section {
        Section::Root => match key {
            "name" => collector.set_literal_name(&value, key_start, value_end),
            "version" => collector.set_literal_version(&value, key_start, value_end),
            "abstract" => collector.set_literal_abstract(&value, key_start, value_end),
            "license" => collector.add_literal_license(&value, key_start, value_end),
            "author" => collector.add_author(&value, key_start, value_end),
            _ => collector.declaration(
                DistDeclarationKind::AuthoringMetadata,
                key,
                Some(value),
                key_start,
                value_end,
                false,
            ),
        },
        Section::Prereqs { phase, relation, recognized, pending } => {
            if key == "-phase" {
                *phase = value.to_ascii_lowercase();
                *recognized = true;
                return;
            }
            if key == "-relationship" || key == "-relation" {
                *relation = value.to_ascii_lowercase();
                *recognized = true;
                return;
            }
            if key.starts_with('-') {
                return;
            }
            if *recognized {
                add_prereq_ini_entry(
                    collector,
                    key,
                    &value,
                    phase,
                    relation,
                    key_start,
                    value_start,
                    value_end,
                );
            } else {
                pending.push(PendingPrereq {
                    key: key.to_string(),
                    value,
                    key_start,
                    value_start,
                    value_end,
                });
            }
        }
        Section::MetaResources => {
            collector.add_literal_resource(key, &value, key_start, value_end);
        }
        Section::Other(_) => {
            collector.declaration(
                DistDeclarationKind::Plugin,
                key,
                Some(value),
                key_start,
                value_end,
                false,
            );
        }
    }
}

fn flush_prereq_section(
    collector: &mut DistCollector<'_>,
    section: &mut Section,
    start: usize,
    end: usize,
) {
    let Section::Prereqs { phase, relation, recognized, pending } = section else {
        return;
    };
    if *recognized {
        let phase = phase.clone();
        let relation = relation.clone();
        for item in pending.drain(..) {
            add_prereq_ini_entry(
                collector,
                &item.key,
                &item.value,
                &phase,
                &relation,
                item.key_start,
                item.value_start,
                item.value_end,
            );
        }
        return;
    }
    if pending.is_empty() {
        return;
    }
    pending.clear();
    collector.limitation(
        "unknown_prereq_section",
        "unrecognized Dist::Zilla prereq section label; modules in it are not treated as runtime/requires",
        Some(start),
        Some(end),
    );
}

fn add_prereq_ini_entry(
    collector: &mut DistCollector<'_>,
    key: &str,
    value: &str,
    phase: &str,
    relation: &str,
    key_start: usize,
    value_start: usize,
    value_end: usize,
) {
    if looks_dynamic_ini_value(value) {
        collector.add_literal_prereq(key, None, phase, relation, key_start, value_end);
        collector.limitation(
            "dynamic_value",
            format!("prerequisite `{key}` version is not a static literal"),
            Some(value_start),
            Some(value_end),
        );
    } else {
        collector.add_literal_prereq(key, Some(value), phase, relation, key_start, value_end);
    }
}

fn split_section_name(raw: &str) -> (&str, Option<&str>) {
    if let Some((plugin, rest)) = raw.split_once('/') {
        (plugin.trim(), Some(rest.trim()))
    } else {
        (raw.trim(), None)
    }
}

fn phase_relation_from_label(label: &str) -> (String, String, bool) {
    let compact: String = label.chars().filter(|ch| !ch.is_whitespace()).collect();
    const PHASES: &[&str] = &["Runtime", "Test", "Build", "Configure", "Develop"];
    const RELATIONS: &[&str] = &["Requires", "Recommends", "Suggests", "Conflicts"];
    for phase in PHASES {
        for relation in RELATIONS {
            if compact.eq_ignore_ascii_case(&format!("{phase}{relation}")) {
                return (phase.to_ascii_lowercase(), relation.to_ascii_lowercase(), true);
            }
        }
    }
    ("runtime".to_string(), "requires".to_string(), false)
}

fn plugin_eq(name: &str, expected: &str) -> bool {
    let heading = canonical_plugin(name);
    heading.eq_ignore_ascii_case(expected)
        || heading.eq_ignore_ascii_case(expected.rsplit("::").next().unwrap_or(expected))
}

fn canonical_plugin(name: &str) -> &str {
    let heading = name.split('/').next().unwrap_or(name).trim();
    heading.strip_prefix("Dist::Zilla::Plugin::").unwrap_or(heading)
}

fn is_dynamic_plugin(name: &str) -> bool {
    DYNAMIC_PLUGINS.iter().any(|plugin| {
        let heading = canonical_plugin(name);
        heading.eq_ignore_ascii_case(plugin)
            || heading
                .rsplit("::")
                .next()
                .unwrap_or(heading)
                .eq_ignore_ascii_case(plugin.rsplit("::").next().unwrap_or(plugin))
    })
}

fn strip_ini_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ';' | '#' if !in_single && !in_double => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn split_ini_key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value.trim()))
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
        {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn looks_dynamic_ini_value(value: &str) -> bool {
    value.contains('%') || value.contains('$') || value.contains("{{")
}
