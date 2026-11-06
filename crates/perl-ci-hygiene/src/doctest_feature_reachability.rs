//! Whether `compile_fail` contracts sit in library modules the doctest route's
//! feature selection actually compiles (#16005).
//!
//! The merge-gate route runs `cargo test --doc`, which enables `cfg(test)` and
//! the default feature set unless the route command names `--features` or
//! `--all-features`. A contract behind `#[cfg(feature = "…")]` with no matching
//! feature in that selection is inert; this module finds those sites.

use color_eyre::eyre::{Result, eyre};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::doctest_enforcement::{PackageFacts, Violation, package_name, workspace_members};
use crate::is_rust_source_file;

/// Feature selection implied by the doctest route's `cargo test` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteFeatureSelection {
    /// Every declared feature is enabled (`--all-features`).
    pub all_features: bool,
    /// Extra features named by `--features` / `-F` on the route command.
    pub explicit: BTreeSet<String>,
}

/// Build context matching `cargo test --doc` for the route command.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctestBuildContext {
    test: bool,
    enabled_features: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriState {
    Yes,
    No,
    Unknown,
}

impl TriState {
    fn is_yes(self) -> bool {
        matches!(self, Self::Yes)
    }

    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::No, _) | (_, Self::No) => Self::No,
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Yes, Self::Yes) => Self::Yes,
        }
    }
}

/// Parse `--features`, `-F`, and `--all-features` from the route command.
pub fn route_feature_selection(command: &str) -> RouteFeatureSelection {
    let mut explicit = BTreeSet::new();
    let mut all_features = false;
    let mut tokens = command.split_whitespace();
    while let Some(token) = tokens.next() {
        match token {
            "--all-features" => all_features = true,
            "--features" | "-F" => {
                if let Some(list) = tokens.next() {
                    for name in list.split(',').map(str::trim).filter(|name| !name.is_empty()) {
                        explicit.insert(name.to_owned());
                    }
                }
            }
            _ => {
                if let Some(list) = token.strip_prefix("--features=") {
                    for name in list.split(',').map(str::trim).filter(|name| !name.is_empty()) {
                        explicit.insert(name.to_owned());
                    }
                }
            }
        }
    }
    RouteFeatureSelection { all_features, explicit }
}

/// Contracts in selected packages that the route's feature selection cannot compile.
pub fn feature_reachability_violations(
    root: &Path,
    route_command: &str,
    route: &BTreeSet<String>,
    facts: &BTreeMap<String, PackageFacts>,
) -> Result<Vec<Violation>> {
    let selection = route_feature_selection(route_command);
    let mut violations = Vec::new();

    for package in route {
        let Some(facts) = facts.get(package) else {
            continue;
        };
        if facts.contracts.is_empty() {
            continue;
        }
        let manifest_path = member_manifest_path(root, package)?;
        let manifest = fs::read_to_string(&manifest_path).map_err(|error| {
            eyre!(
                "failed to read {}: {error}; cannot evaluate feature reachability for `{package}`",
                manifest_path.display()
            )
        })?;
        let ctx = doctest_build_context(&manifest, &selection)?;
        let src = manifest_path
            .parent()
            .ok_or_else(|| eyre!("{manifest_path:?} has no parent"))?
            .join("src");
        let reachable = library_files_reachable(root, &src, &ctx)?;
        let mut sites = Vec::new();
        for site in &facts.contracts {
            if !reachable.contains(&site.file) {
                sites.push(site.clone());
            }
        }
        if !sites.is_empty() {
            violations.push(Violation::ContractNotReachableUnderRouteFeatures {
                package: package.clone(),
                sites,
                enabled_features: ctx.enabled_features.iter().cloned().collect(),
            });
        }
    }

    violations.sort_by(|left, right| violation_package(left).cmp(violation_package(right)));
    Ok(violations)
}

fn violation_package(violation: &Violation) -> &str {
    match violation {
        Violation::ContractOutsideRoute { package, .. }
        | Violation::SelectedPackageUnknown { package }
        | Violation::ContractInUnexecutableTarget { package, .. }
        | Violation::ContractNotReachableUnderRouteFeatures { package, .. } => package,
    }
}

fn member_manifest_path(root: &Path, package: &str) -> Result<PathBuf> {
    if package == "xtask" {
        return Ok(root.join("xtask/Cargo.toml"));
    }
    for member in workspace_members(root)? {
        let manifest_path = member.join("Cargo.toml");
        let manifest = fs::read_to_string(&manifest_path)
            .map_err(|error| eyre!("failed to read {}: {error}", manifest_path.display()))?;
        if package_name(&manifest).as_deref() == Some(package) {
            return Ok(manifest_path);
        }
    }
    Err(eyre!("workspace has no manifest for package `{package}`"))
}

fn doctest_build_context(
    manifest: &str,
    selection: &RouteFeatureSelection,
) -> Result<DoctestBuildContext> {
    let value: toml::Value =
        toml::from_str(manifest).map_err(|error| eyre!("failed to parse manifest: {error}"))?;
    let mut enabled = BTreeSet::new();
    let features =
        value.get("features").and_then(toml::Value::as_table).cloned().unwrap_or_default();
    if selection.all_features {
        enabled.extend(features.keys().filter(|name| *name != "default").cloned());
    } else {
        if let Some(default) = features.get("default").and_then(toml::Value::as_array) {
            for name in default.iter().filter_map(toml::Value::as_str) {
                enabled.insert(name.to_owned());
            }
        }
        enabled.extend(selection.explicit.iter().cloned());
    }
    Ok(DoctestBuildContext { test: true, enabled_features: enabled })
}

fn read_lines(path: &Path) -> Result<Vec<String>> {
    let contents = fs::read_to_string(path)
        .map_err(|error| eyre!("failed to read {}: {error}", path.display()))?;
    Ok(contents.lines().map(str::to_owned).collect())
}

fn library_files_reachable(
    root: &Path,
    src: &Path,
    ctx: &DoctestBuildContext,
) -> Result<BTreeSet<String>> {
    let lib = src.join("lib.rs");
    if !lib.is_file() {
        return Ok(BTreeSet::new());
    }

    let mut canonical_to_relative: BTreeMap<PathBuf, String> = BTreeMap::new();
    for path in walk_rs_files_or_fail(src)? {
        let relative =
            path.strip_prefix(root).unwrap_or(&path).display().to_string().replace('\\', "/");
        if let Ok(real) = path.canonicalize() {
            canonical_to_relative.insert(real, relative);
        }
    }

    let mut edges: BTreeMap<PathBuf, Vec<ModuleEdge>> = BTreeMap::new();
    for (real, relative) in &canonical_to_relative {
        let path = root.join(relative);
        let lines = read_lines(&path)?;
        edges.insert(real.clone(), module_edges(&path, &lines, ctx));
    }

    let lib_real = lib
        .canonicalize()
        .map_err(|error| eyre!("failed to canonicalize {}: {error}", lib.display()))?;
    let lib_lines = read_lines(&lib)?;
    let mut reachable: BTreeSet<PathBuf> = BTreeSet::new();
    if file_inner_cfg_allows(&lib_lines, ctx).is_yes() {
        let mut frontier = vec![lib_real.clone()];
        reachable.insert(lib_real);
        while let Some(path) = frontier.pop() {
            for edge in edges.get(&path).into_iter().flatten() {
                if !edge.active.is_yes() {
                    continue;
                }
                for child in &edge.files {
                    if reachable.contains(child) {
                        continue;
                    }
                    let child_lines =
                        read_lines_for_canonical(root, canonical_to_relative.get(child))?;
                    if file_inner_cfg_allows(&child_lines, ctx).is_yes() {
                        reachable.insert(child.clone());
                        frontier.push(child.clone());
                    }
                }
            }
        }
    }

    Ok(reachable.into_iter().filter_map(|real| canonical_to_relative.get(&real).cloned()).collect())
}

fn read_lines_for_canonical(root: &Path, relative: Option<&String>) -> Result<Vec<String>> {
    let Some(relative) = relative else {
        return Ok(Vec::new());
    };
    read_lines(&root.join(relative))
}

fn walk_rs_files_or_fail(src: &Path) -> Result<Vec<PathBuf>> {
    if !src.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(src).follow_links(false) {
        let entry = entry.map_err(|error| eyre!("failed to walk {}: {error}", src.display()))?;
        let path = entry.path();
        if entry.file_type().is_file() && is_rust_source_file(path) {
            paths.push(path.to_path_buf());
        }
    }
    Ok(paths)
}

#[derive(Debug)]
struct ModuleEdge {
    files: Vec<PathBuf>,
    active: TriState,
}

fn module_edges(path: &Path, lines: &[String], ctx: &DoctestBuildContext) -> Vec<ModuleEdge> {
    let Some(module_dir) = module_directory(path) else {
        return Vec::new();
    };
    let Some(file_dir) = path.parent().map(Path::to_path_buf) else {
        return Vec::new();
    };

    let mut edges = Vec::new();
    let mut item_cfgs = TriState::Yes;
    let mut redirect: Option<String> = None;
    let mut inline: Vec<(usize, String, TriState)> = Vec::new();

    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            index += 1;
            continue;
        }

        if trimmed.starts_with('}') {
            let indent = lines[index].len() - lines[index].trim_start().len();
            while inline.last().is_some_and(|(open, _, _)| *open >= indent) {
                inline.pop();
            }
            item_cfgs = TriState::Yes;
            redirect = None;
            index += 1;
            continue;
        }

        let item = guarded_item(lines, index);
        let mut edge_active = item_cfgs;
        for (_, attr) in &item.attributes {
            edge_active = edge_active.and(cfg_attribute_holds(attr, ctx));
            if let Some(value) = path_attribute_target(attr) {
                redirect = Some(value.to_string());
            }
        }
        let rest = item.text;
        index = item.line;
        if rest.is_empty() {
            index += 1;
            continue;
        }
        let indent = lines[item.line].len() - lines[item.line].trim_start().len();

        if let Some(name) = inline_module_name(rest) {
            let active = edge_active
                .and(inline.iter().fold(TriState::Yes, |acc, (_, _, gated)| acc.and(*gated)));
            inline.push((indent, name, active));
            item_cfgs = TriState::Yes;
            redirect = None;
            index += 1;
            continue;
        }

        if let Some(name) = declared_module_name(rest) {
            let base = if redirect.is_some() && inline.is_empty() {
                file_dir.clone()
            } else {
                let mut dir = module_dir.clone();
                for (_, segment, _) in &inline {
                    dir = dir.join(segment);
                }
                dir
            };
            let files: Vec<PathBuf> = module_files(&base, &name, redirect.as_deref())
                .into_iter()
                .filter_map(|path| path.canonicalize().ok())
                .collect();
            if !files.is_empty() {
                let active = edge_active
                    .and(inline.iter().fold(TriState::Yes, |acc, (_, _, gated)| acc.and(*gated)));
                edges.push(ModuleEdge { files, active });
            }
        }

        item_cfgs = TriState::Yes;
        redirect = None;
        index += 1;
    }
    edges
}

fn file_inner_cfg_allows(lines: &[String], ctx: &DoctestBuildContext) -> TriState {
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            index += 1;
            continue;
        }
        if !trimmed.starts_with("#![") {
            break;
        }
        let item = guarded_item(lines, index);
        for (_, attr) in &item.attributes {
            if attr.starts_with("#![cfg(") {
                let verdict = cfg_attribute_holds(attr, ctx);
                if !verdict.is_yes() {
                    return verdict;
                }
            }
        }
        if lines[index].trim_start().starts_with("#![") {
            index = item.line + 1;
            continue;
        }
        break;
    }
    TriState::Yes
}

fn cfg_attribute_holds(attr: &str, ctx: &DoctestBuildContext) -> TriState {
    let predicate = attr
        .strip_prefix("#[cfg(")
        .or_else(|| attr.strip_prefix("#![cfg("))
        .and_then(|rest| rest.strip_suffix(")]"));
    let Some(predicate) = predicate else {
        return TriState::Yes;
    };
    eval_cfg_predicate(predicate.trim(), ctx)
}

fn eval_cfg_predicate(predicate: &str, ctx: &DoctestBuildContext) -> TriState {
    let predicate = predicate.trim();
    if predicate == "test" {
        return ctx.test.into();
    }
    if predicate == "not(test)" {
        return (!ctx.test).into();
    }
    if let Some(inner) = predicate.strip_prefix("not(").and_then(|rest| rest.strip_suffix(')')) {
        let inner = eval_cfg_predicate(inner.trim(), ctx);
        return match inner {
            TriState::Yes => TriState::No,
            TriState::No => TriState::Yes,
            TriState::Unknown => TriState::Unknown,
        };
    }
    if let Some(inner) = predicate.strip_prefix("all(").and_then(|rest| rest.strip_suffix(')')) {
        return conjuncts(inner)
            .into_iter()
            .map(|arm| eval_cfg_predicate(arm, ctx))
            .fold(TriState::Yes, TriState::and);
    }
    if let Some(inner) = predicate.strip_prefix("any(").and_then(|rest| rest.strip_suffix(')')) {
        let mut any = TriState::No;
        for arm in conjuncts(inner) {
            match eval_cfg_predicate(arm, ctx) {
                TriState::Yes => return TriState::Yes,
                TriState::Unknown => any = TriState::Unknown,
                TriState::No => {}
            }
        }
        return any;
    }
    if let Some(name) = feature_name_from_predicate(predicate) {
        return ctx.enabled_features.contains(name).into();
    }
    TriState::Unknown
}

impl From<bool> for TriState {
    fn from(value: bool) -> Self {
        if value { Self::Yes } else { Self::No }
    }
}

fn feature_name_from_predicate(predicate: &str) -> Option<&str> {
    let rest = predicate.strip_prefix("feature")?.trim();
    let rest = rest.strip_prefix('=')?.trim();
    rest.strip_prefix('"').and_then(|value| value.strip_suffix('"'))
}

fn conjuncts(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut start = 0usize;
    let mut previous = '\0';
    for (index, ch) in inner.char_indices() {
        if in_string {
            if ch == '"' && previous != '\\' {
                in_string = false;
            }
        } else {
            match ch {
                '"' => in_string = true,
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    if let Some(part) = inner.get(start..index) {
                        parts.push(part.trim());
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
        previous = ch;
    }
    if let Some(part) = inner.get(start..) {
        parts.push(part.trim());
    }
    parts
}

fn path_attribute_target(attr: &str) -> Option<&str> {
    let rest = attr.strip_prefix("#[path(")?.strip_suffix(")]")?;
    parse_string_literal(rest.trim())
}

fn parse_string_literal(value: &str) -> Option<&str> {
    let body = value.strip_prefix('"')?;
    let mut escaped = false;
    for (index, character) in body.char_indices() {
        match character {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return body.get(..index),
            _ => {}
        }
    }
    None
}

struct GuardedItem<'a> {
    attributes: Vec<(usize, String)>,
    line: usize,
    text: &'a str,
}

fn guarded_item<'a>(lines: &'a [String], start: usize) -> GuardedItem<'a> {
    let mut attributes = Vec::new();
    let mut line = start;
    let mut col = lines[start].len() - lines[start].trim_start().len();
    while let Some(current) = lines.get(line) {
        let Some(rest) = current.get(col..) else {
            break;
        };
        let trimmed = rest.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            line += 1;
            col = 0;
            continue;
        }
        if !trimmed.starts_with("#[") {
            break;
        }
        let Some((end_line, end_col, text)) =
            attribute_extent(lines, line, col + (rest.len() - trimmed.len()))
        else {
            break;
        };
        attributes.push((line, text));
        line = end_line;
        col = end_col;
    }
    let text =
        lines.get(line).and_then(|current| current.get(col..)).map(str::trim_start).unwrap_or("");
    GuardedItem { attributes, line, text }
}

fn attribute_extent(
    lines: &[String],
    mut line: usize,
    mut col: usize,
) -> Option<(usize, usize, String)> {
    if !lines.get(line)?.get(col..)?.starts_with("#[") {
        return None;
    }
    let mut depth = 0usize;
    let mut text = String::new();
    loop {
        let rest = lines.get(line)?.get(col..)?;
        let mut closing = None;
        for (offset, character) in rest.char_indices() {
            match character {
                '[' => depth += 1,
                ']' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        closing = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        match closing {
            Some(offset) => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(rest.get(..=offset)?.trim());
                return Some((line, col + offset + 1, text));
            }
            None => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(rest.trim());
                line += 1;
                col = 0;
            }
        }
    }
}

fn module_directory(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    match path.file_stem()?.to_str()? {
        "lib" | "main" | "mod" => Some(parent.to_path_buf()),
        stem => Some(parent.join(stem)),
    }
}

fn module_files(base: &Path, name: &str, redirect: Option<&str>) -> Vec<PathBuf> {
    if let Some(relative) = redirect {
        let redirected = base.join(relative);
        return if redirected.is_file() {
            redirected.canonicalize().into_iter().collect()
        } else {
            Vec::new()
        };
    }
    let mut files = Vec::new();
    let sibling = base.join(format!("{name}.rs"));
    let nested = base.join(name).join("mod.rs");
    if sibling.is_file() {
        files.extend(sibling.canonicalize());
    }
    if nested.is_file() {
        files.extend(nested.canonicalize());
    }
    files
}

fn inline_module_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.trim_end().strip_suffix('{')?.trim_end();
    module_name_after_visibility(rest)
}

fn declared_module_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.trim_end().strip_suffix(';')?;
    module_name_after_visibility(rest)
}

fn module_name_after_visibility(rest: &str) -> Option<String> {
    let rest = rest.trim().strip_prefix("pub").map_or(rest, |tail| tail);
    let rest = rest.trim_start();
    let rest = match (rest.starts_with('('), rest.find(')')) {
        (true, Some(close)) => rest.get(close + 1..)?.trim(),
        _ => rest,
    };
    let name = rest.trim().strip_prefix("mod ")?.trim();
    (!name.is_empty() && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
        .then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use perl_test_must::must_with;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::doctest_enforcement::{package_facts, route_command, route_packages, violations};

    fn temp_repo(label: &str) -> PathBuf {
        let nanos =
            must_with(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH), "clock")
                .as_nanos();
        let root = std::env::temp_dir()
            .join(format!("perl-ci-hygiene-feature-{label}-{}", std::process::id()))
            .join(nanos.to_string());
        must_with(fs::create_dir_all(&root), "mkdir");
        root
    }

    fn write(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            must_with(fs::create_dir_all(parent), "mkdir");
        }
        must_with(fs::write(&path, contents), "write");
    }

    #[test]
    fn route_feature_selection_reads_features_and_all_features() {
        let selection = route_feature_selection(
            "cargo test --locked --doc -p a --features one,two --all-features",
        );
        assert!(selection.all_features);
        assert_eq!(selection.explicit, BTreeSet::from(["one".to_owned(), "two".to_owned()]));
    }

    #[test]
    fn a_feature_gated_contract_in_a_selected_package_is_reported() {
        let root = temp_repo("gated-contract");
        write(&root, "Cargo.toml", "[workspace]\nmembers = [\"fixture\"]\n");
        write(
            &root,
            "fixture/Cargo.toml",
            "[package]\nname = \"fixture-crate\"\nversion = \"0.1.0\"\n\n[features]\ndefault = []\nhidden = []\n",
        );
        write(&root, "fixture/src/lib.rs", "#[cfg(feature = \"hidden\")]\nmod gated;\n");
        write(
            &root,
            "fixture/src/gated.rs",
            "/// ```compile_fail\n/// fn bad() {}\n/// ```\npub struct Gate;\n",
        );
        let policy = [
            "gates:",
            "  - name: doctest_contract_proof",
            "    tier: merge_gate",
            "    required: true",
            "    quarantine: false",
            "    command: cargo test --locked --doc -p fixture-crate",
            "",
        ]
        .join("\n");
        let route = must_with(route_packages(&policy), "route");
        let facts = must_with(package_facts(&root), "facts");
        let command = must_with(route_command(&policy), "command");
        let mut found = violations(&route, &facts);
        found.extend(must_with(
            feature_reachability_violations(&root, &command, &route, &facts),
            "reachability",
        ));
        assert_eq!(found.len(), 1, "got {found:?}");
        assert!(matches!(
            &found[0],
            Violation::ContractNotReachableUnderRouteFeatures { package, .. } if package == "fixture-crate"
        ));
    }

    #[test]
    fn an_ungated_contract_stays_clean_while_gated_one_fails() {
        let root = temp_repo("control");
        write(&root, "Cargo.toml", "[workspace]\nmembers = [\"good\", \"bad\"]\n");
        write(&root, "good/Cargo.toml", "[package]\nname = \"good-crate\"\nversion = \"0.1.0\"\n");
        write(
            &root,
            "good/src/lib.rs",
            "/// ```compile_fail\n/// fn ok() {}\n/// ```\npub struct Ok;\n",
        );
        write(
            &root,
            "bad/Cargo.toml",
            "[package]\nname = \"bad-crate\"\nversion = \"0.1.0\"\n\n[features]\ndefault = []\nhidden = []\n",
        );
        write(&root, "bad/src/lib.rs", "#[cfg(feature = \"hidden\")]\nmod gated;\n");
        write(
            &root,
            "bad/src/gated.rs",
            "/// ```compile_fail\n/// fn bad() {}\n/// ```\npub struct Gate;\n",
        );
        let policy = [
            "gates:",
            "  - name: doctest_contract_proof",
            "    tier: merge_gate",
            "    required: true",
            "    quarantine: false",
            "    command: cargo test --locked --doc -p good-crate -p bad-crate",
            "",
        ]
        .join("\n");
        let route = must_with(route_packages(&policy), "route");
        let facts = must_with(package_facts(&root), "facts");
        let command = must_with(route_command(&policy), "command");
        let found = must_with(
            feature_reachability_violations(&root, &command, &route, &facts),
            "reachability",
        );
        assert_eq!(found.len(), 1);
        assert!(
            matches!(&found[0], Violation::ContractNotReachableUnderRouteFeatures { package, .. } if package == "bad-crate")
        );
    }

    #[test]
    fn route_features_that_enable_the_module_clear_the_violation() {
        let root = temp_repo("enabled");
        write(&root, "Cargo.toml", "[workspace]\nmembers = [\"fixture\"]\n");
        write(
            &root,
            "fixture/Cargo.toml",
            "[package]\nname = \"fixture-crate\"\nversion = \"0.1.0\"\n\n[features]\ndefault = []\nhidden = []\n",
        );
        write(&root, "fixture/src/lib.rs", "#[cfg(feature = \"hidden\")]\nmod gated;\n");
        write(
            &root,
            "fixture/src/gated.rs",
            "/// ```compile_fail\n/// fn bad() {}\n/// ```\npub struct Gate;\n",
        );
        let policy = [
            "gates:",
            "  - name: doctest_contract_proof",
            "    tier: merge_gate",
            "    required: true",
            "    quarantine: false",
            "    command: cargo test --locked --doc -p fixture-crate --features hidden",
            "",
        ]
        .join("\n");
        let route = must_with(route_packages(&policy), "route");
        let facts = must_with(package_facts(&root), "facts");
        let command = must_with(route_command(&policy), "command");
        let found = must_with(
            feature_reachability_violations(&root, &command, &route, &facts),
            "reachability",
        );
        assert!(found.is_empty(), "got {found:?}");
    }
}
