//! Build.PL authoring extraction (Module::Build).

use crate::id::FileId;
use crate::range::Utf8LineIndex;

use super::scan::{ScanPair, call_open_paren, contains_ident, find_ident, parse_paren_hash};
use super::{DistAuthoringBuildTool, DistAuthoringFacts, DistAuthoringSource, DistCollector};

/// Extract bounded static facts from `Build.PL` source.
#[must_use]
pub fn parse_build_pl(file_id: FileId, content: &str) -> DistAuthoringFacts {
    let index = Utf8LineIndex::new(content);
    let mut collector = DistCollector::new(
        file_id,
        DistAuthoringSource::BuildPl,
        DistAuthoringBuildTool::ModuleBuild,
        content,
        &index,
    );

    if let Some(pairs) = build_pairs(content) {
        collect_build_pairs(&mut collector, &pairs);
    } else {
        collector.limitation(
            "missing_module_build_new",
            "no static Module::Build->new argument list was recovered",
            None,
            None,
        );
    }

    if contains_ident(content, "system") || contains_ident(content, "eval") {
        let idx = find_ident(content, "system", 0).or_else(|| find_ident(content, "eval", 0));
        collector.limitation(
            "executable_construct",
            "ignored executable construct; authoring code is not executed",
            idx,
            idx.map(|i| i + 4),
        );
    }
    if contains_ident(content, "if") || contains_ident(content, "unless") {
        collector.limitation(
            "conditional_declaration",
            "conditional authoring remains a typed limitation; literals are still extracted",
            find_ident(content, "if", 0).or_else(|| find_ident(content, "unless", 0)),
            None,
        );
    }

    collector.finish()
}

fn build_pairs(content: &str) -> Option<Vec<ScanPair>> {
    let mut from = 0;
    while let Some(rel) = content.get(from..).and_then(|rest| rest.find("->new")) {
        let idx = from + rel;
        if ident_before_arrow(content, idx).is_some_and(is_module_build_constructor) {
            let method_start = idx + 2;
            if let Some(open) = call_open_paren(content, method_start, "new".len())
                && let Some((pairs, _)) = parse_paren_hash(content, open)
            {
                return Some(pairs);
            }
        }
        from = idx.saturating_add("->new".len());
        if from <= idx {
            break;
        }
    }
    if let Some(pairs) = method_new_pairs(content, "new") {
        return Some(pairs);
    }
    assigned_hash(content, "args")
}

fn ident_before_arrow(source: &str, arrow_idx: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut end = arrow_idx;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }
    let mut start = end;
    while start > 0 {
        let prev = bytes[start - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b':' {
            start -= 1;
        } else {
            break;
        }
    }
    if start == end { None } else { source.get(start..end) }
}

fn is_module_build_constructor(ident: &str) -> bool {
    ident == "Module::Build" || ident.starts_with("Module::Build::")
}

fn method_new_pairs(content: &str, method: &str) -> Option<Vec<ScanPair>> {
    let ident = find_ident(content, method, 0)?;
    let open = call_open_paren(content, ident, method.len())?;
    parse_paren_hash(content, open).map(|(pairs, _)| pairs)
}

fn assigned_hash(content: &str, name: &str) -> Option<Vec<ScanPair>> {
    let ident = find_ident(content, name, 0)?;
    let mut idx = ident + name.len();
    super::scan::skip_ws_comments(content, &mut idx);
    let bytes = content.as_bytes();
    if bytes.get(idx) != Some(&b'=') {
        return None;
    }
    idx += 1;
    super::scan::skip_ws_comments(content, &mut idx);
    if bytes.get(idx) == Some(&b'(') {
        return parse_paren_hash(content, idx).map(|(pairs, _)| pairs);
    }
    None
}

fn collect_build_pairs(collector: &mut DistCollector<'_>, pairs: &[ScanPair]) {
    for pair in pairs {
        match pair.key.as_str() {
            "module_name" => collector.set_name_from_package(pair),
            "dist_name" | "name" => collector.set_dist_name(pair),
            "dist_version" | "version" => collector.set_version(pair),
            "dist_version_from" | "version_from" => collector.set_version_from(pair),
            "dist_abstract" | "abstract" => collector.set_abstract(pair),
            "license" => collector.add_license(pair),
            "requires" => collector.add_prereq_hash(pair, "runtime", "requires"),
            "configure_requires" => collector.add_prereq_hash(pair, "configure", "requires"),
            "build_requires" => collector.add_prereq_hash(pair, "build", "requires"),
            "test_requires" => collector.add_prereq_hash(pair, "test", "requires"),
            "recommends" => collector.add_prereq_hash(pair, "runtime", "recommends"),
            "conflicts" => collector.add_prereq_hash(pair, "runtime", "conflicts"),
            "meta_add" | "meta_merge" => collector.add_meta_overlay(pair, pair.key.as_str()),
            "resources" => {
                collector.add_resources_value(&pair.value, pair.value_start, pair.value_end)
            }
            _ => {}
        }
    }
}
