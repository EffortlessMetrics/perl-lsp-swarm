//! Makefile.PL authoring extraction (ExtUtils::MakeMaker).

use crate::id::FileId;
use crate::range::Utf8LineIndex;

use super::scan::{
    ScanPair, call_open_paren, contains_ident, find_ident, parse_paren_hash,
    parse_paren_hash_recovering,
};
use super::{
    DistAuthoringBuildTool, DistAuthoringFacts, DistAuthoringSource, DistCollector,
    IGNORE_MAKEFILE_KEYS,
};

/// Extract bounded static facts from `Makefile.PL` source.
#[must_use]
pub fn parse_makefile_pl(file_id: FileId, content: &str) -> DistAuthoringFacts {
    let index = Utf8LineIndex::new(content);
    let mut collector = DistCollector::new(
        file_id,
        DistAuthoringSource::MakefilePl,
        DistAuthoringBuildTool::ExtUtilsMakeMaker,
        content,
        &index,
    );

    if let Some(pairs) = makefile_pairs(content, &mut collector) {
        collect_makefile_pairs(&mut collector, &pairs);
    } else {
        collector.limitation(
            "missing_writemakefile",
            "no static WriteMakefile or %WriteMakefileArgs hash was recovered",
            None,
            None,
        );
    }

    record_executable_constructs(&mut collector, content);
    record_conditionals(&mut collector, content);
    collector.finish()
}

fn makefile_pairs(content: &str, collector: &mut DistCollector<'_>) -> Option<Vec<ScanPair>> {
    if let Some(ident) = find_ident(content, "WriteMakefile", 0)
        && let Some(open) = call_open_paren(content, ident, "WriteMakefile".len())
    {
        let (pairs, closed) = parse_paren_hash_recovering(content, open);
        if !closed {
            collector.limitation(
                "malformed",
                "WriteMakefile argument list is unclosed; recovered static literals anyway",
                Some(open),
                Some(content.len()),
            );
        }
        if !pairs.is_empty() {
            return Some(pairs);
        }
    }
    if let Some(pairs) = assigned_hash(content, "%WriteMakefileArgs") {
        collector.limitation(
            "helper_variable",
            "WriteMakefile arguments recovered from %WriteMakefileArgs without executing the helper",
            find_ident(content, "WriteMakefileArgs", 0),
            find_ident(content, "WriteMakefileArgs", 0).map(|idx| idx + "WriteMakefileArgs".len()),
        );
        return Some(pairs);
    }
    None
}

fn assigned_hash(content: &str, sigil_name: &str) -> Option<Vec<ScanPair>> {
    let name = sigil_name.trim_start_matches('%');
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
    if bytes.get(idx) == Some(&b'{') {
        let mut cursor = idx;
        let value = super::scan::parse_value(content, &mut cursor);
        return value.as_hash().map(<[ScanPair]>::to_vec);
    }
    None
}

fn collect_makefile_pairs(collector: &mut DistCollector<'_>, pairs: &[ScanPair]) {
    for pair in pairs {
        let key = pair.key.as_str();
        if IGNORE_MAKEFILE_KEYS.contains(&key) {
            continue;
        }
        match key {
            "NAME" => collector.set_name_from_package(pair),
            "DISTNAME" => collector.set_dist_name(pair),
            "VERSION" => collector.set_version(pair),
            "VERSION_FROM" => collector.set_version_from(pair),
            "ABSTRACT" => collector.set_abstract(pair),
            "ABSTRACT_FROM" => collector.set_abstract_from(pair),
            "LICENSE" => collector.add_license(pair),
            "PREREQ_PM" => collector.add_prereq_hash(pair, "runtime", "requires"),
            "CONFIGURE_REQUIRES" => collector.add_prereq_hash(pair, "configure", "requires"),
            "BUILD_REQUIRES" => collector.add_prereq_hash(pair, "build", "requires"),
            "TEST_REQUIRES" => collector.add_prereq_hash(pair, "test", "requires"),
            "META_ADD" => collector.add_meta_overlay(pair, "meta_add"),
            "META_MERGE" => collector.add_meta_overlay(pair, "meta_merge"),
            _ => {}
        }
    }
}

fn record_executable_constructs(collector: &mut DistCollector<'_>, content: &str) {
    for name in ["system", "exec", "eval", "qx"] {
        if let Some(idx) = find_ident(content, name, 0) {
            collector.limitation(
                "executable_construct",
                format!("ignored `{name}` construct; authoring code is not executed"),
                Some(idx),
                Some(idx + name.len()),
            );
        }
    }
    if content.contains('`') {
        collector.limitation(
            "executable_construct",
            "ignored backtick qx form; authoring code is not executed",
            content.find('`'),
            content.find('`').map(|idx| idx + 1),
        );
    }
}

fn record_conditionals(collector: &mut DistCollector<'_>, content: &str) {
    for name in ["if", "unless", "elsif"] {
        if contains_ident(content, name) {
            let idx = find_ident(content, name, 0);
            collector.limitation(
                "conditional_declaration",
                "conditional authoring remains a typed limitation; literals are still extracted",
                idx,
                idx.map(|i| i + name.len()),
            );
            break;
        }
    }
}
