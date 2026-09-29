//! Literal `Sub::Exporter -setup` export lowering (#16865 / #14530).
//!
//! Owns parser-proven `-setup` structure → HIR export declarations. Group
//! members are recorded with the same vocabulary #14530 already used; further
//! group/export-set modeling is #16866. Importer materialization and provider
//! projection are #16868 / #16869.

use std::collections::BTreeSet;

use crate::SourceLocation;

use super::model::{
    ExportDeclaration, ExportDeclarationKind, HirId, StashConfidence, StashDynamicBoundary,
    StashDynamicBoundaryKind, StashGraph, StashProvenance,
};

/// Record export declarations for `use Sub::Exporter -setup => { ... };`.
///
/// Sub::Exporter replaces the `@EXPORT`/`@EXPORT_OK`/`%EXPORT_TAGS` stash
/// variables with a configuration hash, so the classic stash-variable path
/// never sees these packages. The mapping onto the existing export model
/// follows Sub::Exporter's documented semantics:
///
/// - every name in `exports` is available on request (`Optional`);
/// - the `default` group is what a bare `use Module;` installs (`Default`);
/// - every other group is a tag (`Tag`);
/// - the implicit `all` group holds every `exports` name.
///
/// These `Tag` declarations are reachable from an importer through the
/// `:name` spelling only. Sub::Exporter's documentation leads with
/// `-name`, but `collect_qw_import_words` (`hir/model.rs`) strips only
/// `:`, so a `-name` request lands in the explicit-name list and never
/// reaches tag expansion. Repairing that classifier is #14532: a blanket
/// `-word` → tag rule would break `use parent qw(-norequire Foo::Base)`,
/// which `hir/lower.rs` still relies on, so it needs the target module's
/// group names to disambiguate.
///
/// A configuration this function cannot enumerate statically records a
/// dynamic export boundary instead of a partial or unproven export list:
/// a computed `exports` or `groups` value, a group built from other group
/// references, a group naming something the `exports` configuration does
/// not declare, a setup that redirects installation away from the
/// importing package, or `Sub::Exporter` used with no literal `-setup`
/// hash.
pub(super) fn record_setup(
    stash_graph: &mut StashGraph,
    package: String,
    args: &[String],
    range: SourceLocation,
    item_id: HirId,
) {
    // Any `-setup` installs a new importer over an earlier one, so the
    // earlier configuration's names become stale — including when this
    // setup's own value cannot be read, which is why this runs before the
    // readability check below. A bare `use Sub::Exporter;` carries no
    // `-setup` and establishes no configuration, so it leaves an earlier
    // one standing. Boundaries are kept either way: they record what could
    // not be read, which stays true, and they claim no exports.
    if args.iter().any(|arg| unquote_literal(arg) == "-setup") {
        stash_graph.export_declarations.retain(|declaration| {
            declaration.package != package
                || declaration.provenance != StashProvenance::DesugaredAst
        });
    }

    let Some(setup) = sub_exporter_setup_body(args) else {
        record_dynamic_export_boundary(
            stash_graph,
            Some(package),
            None,
            range,
            Some(item_id),
            StashDynamicBoundaryKind::DynamicExportDeclaration,
            "Sub::Exporter export configuration is not a static -setup hash",
        );
        return;
    };

    // The `-setup` collector uses `build_exporter`, not `setup_exporter`,
    // so `into` and `as` "are not accepted here" as plain keys. The
    // documentation's own `-setup` example spells the rename as the dashed
    // `-as => 'do_import'` *inside* the setup hash — which this scan
    // catches, since the parser splits `-as` into `-` and `as` and the
    // `as` token sits at the hash's top level. Either spelling leaves an
    // ordinary `use` of the module unable to prove it installs these
    // symbols: the dashed form renames the exporter away from `import`,
    // and the plain form is a key the `-setup` path does not accept. A
    // custom `installer` may well install normally, but nothing here
    // proves it does.
    //
    // The same keys also redirect from a configuration hashref passed
    // *beside* the `-setup` pair — `use Sub::Exporter { into => 'Target' },
    // -setup => {...}` is the documented spelling, and the hash may sit
    // after the pair just as well. That hash is outside the setup body
    // this pass reads, so it has to be looked for separately or a
    // redirected module publishes its exports as if they arrived.
    if let Some(key) = SUB_EXPORTER_INSTALL_REDIRECT_KEYS
        .iter()
        .map(|key| (*key).to_string())
        .find(|key| hash_entry_value(setup, key).is_some())
        .or_else(|| sub_exporter_redirect_beside_setup(args))
    {
        record_dynamic_export_boundary(
            stash_graph,
            Some(package),
            Some((*key).to_string()),
            range,
            Some(item_id),
            StashDynamicBoundaryKind::DynamicExportDeclaration,
            "Sub::Exporter setup is not shown to install exports \
             into the importing package",
        );
        return;
    }

    // A key outside Sub::Exporter's documented set means this pass does not
    // know what the configuration does. The four keys above are the ones
    // documented to redirect installation, but that set is closed only for
    // the documented vocabulary: an unrecognized key may be a newer
    // release's installation-affecting option, or may make Sub::Exporter
    // reject the setup outright. Either way the `exports` list read below
    // is not shown to be what a consumer receives, so it is recorded as a
    // boundary rather than published.
    if let Some(key) = sub_exporter_unknown_setup_key(setup) {
        record_dynamic_export_boundary(
            stash_graph,
            Some(package),
            Some(key),
            range,
            Some(item_id),
            StashDynamicBoundaryKind::DynamicExportDeclaration,
            "Sub::Exporter setup carries a key this pass does not recognize, \
             so its exports are not shown to be what a consumer receives",
        );
        return;
    }

    let export_names = match hash_entry_value(setup, "exports") {
        None => None,
        Some(value) => match sub_exporter_export_names(value) {
            Some(names) => Some(names),
            None => {
                record_dynamic_export_boundary(
                    stash_graph,
                    Some(package.clone()),
                    Some("exports".to_string()),
                    range,
                    Some(item_id),
                    StashDynamicBoundaryKind::DynamicExportDeclaration,
                    "Sub::Exporter exports list is not statically enumerable",
                );
                None
            }
        },
    };

    // `generator` is documented as "a callback used to produce the code
    // that will be installed", defaulting to `Sub::Exporter`'s own
    // generator — the one that turns a plain `exports` name into that
    // package's sub of the same name. A setup that replaces it produces
    // every export's code itself, so a plain name no longer has to name a
    // sub in this source. That is the same weakening a per-name generator
    // causes, so it takes the same confidence, over the whole setup.
    let custom_generator = hash_entry_value(setup, "generator").is_some();

    let declared: &[String] = export_names.as_ref().map_or(&[], |exports| &exports.names);
    let generated_names: BTreeSet<String> = if custom_generator {
        declared.iter().cloned().collect()
    } else {
        export_names.as_ref().map(|exports| exports.generated.clone()).unwrap_or_default()
    };
    let generator_backed_in = |symbols: &[String]| -> Vec<String> {
        symbols.iter().filter(|symbol| generated_names.contains(*symbol)).cloned().collect()
    };
    let confidence_of = |symbols: &[String]| {
        if generator_backed_in(symbols).is_empty() {
            StashConfidence::High
        } else {
            StashConfidence::Medium
        }
    };

    if !declared.is_empty() {
        stash_graph.export_declarations.push(ExportDeclaration {
            package: package.clone(),
            kind: ExportDeclarationKind::Optional,
            tag_name: None,
            symbols: declared.to_vec(),
            generator_backed: generator_backed_in(declared),
            range,
            declaration_item: Some(item_id),
            provenance: StashProvenance::DesugaredAst,
            confidence: confidence_of(declared),
        });
    }

    let mut groups_are_static = true;
    let mut declared_all_group = false;

    if let Some(value) = hash_entry_value(setup, "groups") {
        match sub_exporter_group_bodies(value) {
            Some(groups) => {
                for (name, members) in groups {
                    if name == "all" {
                        declared_all_group = true;
                    }
                    let Some(members) = members else {
                        record_dynamic_export_boundary(
                            stash_graph,
                            Some(package.clone()),
                            Some(name),
                            range,
                            Some(item_id),
                            StashDynamicBoundaryKind::DynamicExportDeclaration,
                            "Sub::Exporter group is not a static member list",
                        );
                        continue;
                    };

                    // A group may only name declared exports. Publishing a
                    // member the `exports` configuration does not declare
                    // would offer a symbol whose import Sub::Exporter
                    // rejects — and with no enumerable `exports` there is
                    // nothing to check against, so nothing is publishable.
                    let mut symbols = members.names;
                    let named = symbols.len();
                    symbols.retain(|member| declared.contains(member));
                    let complete = members.complete && symbols.len() == named;

                    if !symbols.is_empty() {
                        let (kind, tag_name) = if name == "default" {
                            (ExportDeclarationKind::Default, None)
                        } else {
                            (ExportDeclarationKind::Tag, Some(name.clone()))
                        };
                        let confidence = confidence_of(&symbols);
                        stash_graph.export_declarations.push(ExportDeclaration {
                            package: package.clone(),
                            kind,
                            tag_name,
                            symbols: symbols.clone(),
                            generator_backed: generator_backed_in(&symbols),
                            range,
                            declaration_item: Some(item_id),
                            provenance: StashProvenance::DesugaredAst,
                            confidence,
                        });
                    }

                    // Members that did not resolve are recorded rather than
                    // dropped silently, so the group reads as incomplete
                    // instead of as an exact short list.
                    if !complete {
                        record_dynamic_export_boundary(
                            stash_graph,
                            Some(package.clone()),
                            Some(name),
                            range,
                            Some(item_id),
                            StashDynamicBoundaryKind::DynamicExportDeclaration,
                            "Sub::Exporter group has members that do not resolve \
                             to a declared export name",
                        );
                    }
                }
            }
            None => {
                groups_are_static = false;
                record_dynamic_export_boundary(
                    stash_graph,
                    Some(package.clone()),
                    Some("groups".to_string()),
                    range,
                    Some(item_id),
                    StashDynamicBoundaryKind::DynamicExportDeclaration,
                    "Sub::Exporter groups value is not a static list",
                );
            }
        }
    }

    // Sub::Exporter creates an implicit `all` group over the `exports`
    // list. Synthesize it only when the export list was fully enumerated
    // and the configuration could not have redefined `all` itself — a
    // computed `groups` value may well carry its own.
    if groups_are_static && !declared_all_group && !declared.is_empty() {
        stash_graph.export_declarations.push(ExportDeclaration {
            package,
            kind: ExportDeclarationKind::Tag,
            tag_name: Some("all".to_string()),
            symbols: declared.to_vec(),
            generator_backed: generator_backed_in(declared),
            range,
            declaration_item: Some(item_id),
            provenance: StashProvenance::DesugaredAst,
            confidence: confidence_of(declared),
        });
    }
}

fn record_dynamic_export_boundary(
    stash_graph: &mut StashGraph,
    package: Option<String>,
    symbol: Option<String>,
    range: SourceLocation,
    boundary_item: Option<HirId>,
    kind: StashDynamicBoundaryKind,
    reason: &str,
) {
    stash_graph.dynamic_boundaries.push(StashDynamicBoundary {
        package,
        symbol,
        range,
        boundary_item,
        kind,
        reason: reason.to_string(),
        provenance: StashProvenance::DynamicBoundary,
        confidence: StashConfidence::Low,
    });
}

/// Index of the delimiter closing the opener at `open`, if the slice balances.
fn matching_delimiter(tokens: &[String], open: usize) -> Option<usize> {
    // The closer kind is tracked, not just the nesting depth: counting depth
    // alone would let `[ ... }` balance and hand callers a body that spans a
    // delimiter it never closed.
    let mut expected: Vec<&str> = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.as_str() {
            "{" => expected.push("}"),
            "[" => expected.push("]"),
            "(" => expected.push(")"),
            close @ ("}" | "]" | ")") => {
                if expected.pop()? != close {
                    return None;
                }
                if expected.is_empty() {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Strip one matching pair of Perl quotes from a literal token.
///
/// `{ 'exports' => ... }` is ordinary Perl and means exactly what
/// `{ exports => ... }` means. Comparing raw tokens would read the quoted
/// spelling as an absent key, which publishes nothing *and* records no
/// boundary — the silent partial answer this lowering exists to avoid.
fn unquote_literal(token: &str) -> &str {
    let token = token.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = token.strip_prefix(quote).and_then(|rest| rest.strip_suffix(quote)) {
            return inner;
        }
    }
    token
}

/// Names declared by a Sub::Exporter `exports` value.
struct SubExporterExports {
    /// Export names in declaration order.
    names: Vec<String>,
    /// Names whose value is a generator, so no `sub` of that name need exist
    /// in this source.
    generated: BTreeSet<String>,
}

/// Statically resolvable members of one Sub::Exporter group.
struct SubExporterGroupMembers {
    /// Member names in declaration order.
    names: Vec<String>,
    /// False when the group also holds members this pass cannot name: a
    /// reference to another group, or a member carrying import options whose
    /// installed name differs from the export name.
    complete: bool,
}

/// Sub::Exporter setup keys that leave an ordinary `use` of the module unable
/// to prove it installs the configured symbols into the importing package.
///
/// `as` covers the documented `-setup` rename `-as => 'do_import'`, which
/// installs the exporter under another name so the module has no `import` at
/// all; `into` and `into_level` name a different destination; `installer`
/// replaces the installation itself, which may or may not be equivalent.
/// `into` and `as` as *plain* keys are not accepted by the `-setup` path at
/// all, which is a second reason not to read past them.
const SUB_EXPORTER_INSTALL_REDIRECT_KEYS: &[&str] = &["as", "into", "into_level", "installer"];

/// Every top-level key Sub::Exporter's documentation defines for a setup hash.
///
/// The pod enumerates `exports`, `groups`, `collectors`, `into_level`, `into`,
/// `generator` and `installer`; `as` comes from the same document's
/// `setup_exporter` configuration. This is the vocabulary this pass can reason
/// about — anything else is a configuration whose meaning is unknown here.
const SUB_EXPORTER_KNOWN_SETUP_KEYS: &[&str] =
    &["as", "collectors", "exports", "generator", "groups", "installer", "into", "into_level"];

/// The first top-level setup key outside the documented vocabulary, if any.
///
/// An entry that is not a readable `key => value` pair counts as unknown too:
/// a setup body this pass cannot parse into keys is not one whose `exports` it
/// can claim to have understood.
fn sub_exporter_unknown_setup_key(setup: &[String]) -> Option<String> {
    comma_separated_entries(setup).into_iter().find_map(|entry| {
        let key = unquote_literal(entry.first()?).to_string();
        // A flattened hash or an expression is not a readable `key => value`
        // pair, so which keys it contributes is unknown — including whether
        // one of them redirects installation.
        if entry.get(1).map(String::as_str) != Some("=>") {
            return Some(key);
        }
        (!SUB_EXPORTER_KNOWN_SETUP_KEYS.contains(&key.as_str())).then_some(key)
    })
}

/// An install-redirecting key in a configuration hashref beside `-setup`.
///
/// `use Sub::Exporter { into => 'Target::Package' }, -setup => { ... }` is the
/// documented way to redirect installation, and the same hash is an ordinary
/// argument, so it is equally valid after the `-setup` pair. Either way it sits
/// outside the setup body, so the scan over that body cannot see it: without
/// this, a module whose exports are installed into another package publishes
/// them as though an ordinary `use` received them.
///
/// The documented leading spelling is stopped earlier, by the missing-`-setup`
/// boundary: a `use` whose arguments open with a hash records only that hash,
/// so no readable setup reaches this point. The trailing spelling does reach
/// it, which is what this scan is for.
///
/// Every `-setup` value's own token range is skipped, so a key inside a setup
/// hash is judged by the scan over that hash and not counted a second time
/// here. All of them are skipped, not just the one this pass reads: a repeated
/// `-setup` discards the earlier value entirely, so a key inside it redirects
/// nothing.
fn sub_exporter_redirect_beside_setup(args: &[String]) -> Option<String> {
    let setup_bodies: Vec<(usize, usize)> = args
        .iter()
        .enumerate()
        .filter(|(index, arg)| {
            unquote_literal(arg) == "-setup" && args.get(index + 1).map(String::as_str) == Some("{")
        })
        .filter_map(|(index, _)| {
            matching_delimiter(args, index + 1).map(|close| (index + 1, close))
        })
        .collect();

    // Only a key of the configuration hash itself counts. A redirect name can
    // occur arbitrarily deep inside an unrelated value — `{ generator => sub {
    // ...; return { as => $opt{as} }; } }` builds a runtime hash whose `as` has
    // nothing to do with installing this module's exporter — and matching it
    // would withhold a fully readable export list over a coincidence of naming.
    let mut depth = 0usize;
    let mut found = None;
    for (index, token) in args.iter().enumerate() {
        match token.as_str() {
            "{" | "[" | "(" => {
                depth = depth.saturating_add(1);
                continue;
            }
            "}" | "]" | ")" => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if depth > 1 || setup_bodies.iter().any(|(open, close)| index >= *open && index <= *close) {
            continue;
        }
        let key = unquote_literal(token);
        if SUB_EXPORTER_INSTALL_REDIRECT_KEYS.contains(&key)
            && args.get(index + 1).map(String::as_str) == Some("=>")
        {
            found = Some(key.to_string());
            break;
        }
    }
    found
}

/// Token slice inside a `use Sub::Exporter -setup => { ... }` configuration hash.
///
/// The parser drops the `=>` after a leading `-flag`, so the opening brace
/// follows `-setup` directly. Returns `None` when there is no literal hash.
fn sub_exporter_setup_body(args: &[String]) -> Option<&[String]> {
    // Perl keeps the last value for a repeated key, so a repeated `-setup`
    // resolves to the last one for the same reason `hash_entry_value` does.
    let setup = args.iter().rposition(|arg| unquote_literal(arg) == "-setup")?;
    let open = setup + 1;
    if args.get(open).map(String::as_str) != Some("{") {
        return None;
    }
    let close = matching_delimiter(args, open)?;
    args.get(open + 1..close)
}

/// Value tokens for `key => <value>` at the top level of a hash body.
///
/// A bracketed value includes its delimiters so callers can tell an arrayref
/// from a hashref; any other value is a single token. A repeated key resolves
/// to its last value, matching Perl's hash construction.
fn hash_entry_value<'a>(body: &'a [String], key: &str) -> Option<&'a [String]> {
    let mut found = None;
    let mut depth = 0usize;
    for index in 0..body.len() {
        match body[index].as_str() {
            "{" | "[" | "(" => depth += 1,
            "}" | "]" | ")" => depth = depth.saturating_sub(1),
            token if depth == 0 && unquote_literal(token) == key => {
                if body.get(index + 1).map(String::as_str) != Some("=>") {
                    continue;
                }
                let value = index + 2;
                // Perl keeps the last value for a repeated hash key, so keep
                // scanning rather than returning the first match.
                found = match body.get(value).map(String::as_str) {
                    Some("{" | "[" | "(") => {
                        matching_delimiter(body, value).and_then(|close| body.get(value..=close))
                    }
                    Some(_) => body.get(value..=value),
                    None => None,
                };
            }
            _ => {}
        }
    }
    found
}

/// Split a bracketed body into its top-level comma-separated entries.
fn comma_separated_entries(body: &[String]) -> Vec<&[String]> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, token) in body.iter().enumerate() {
        match token.as_str() {
            "{" | "[" | "(" => depth += 1,
            "}" | "]" | ")" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                if start < index {
                    entries.push(&body[start..index]);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < body.len() {
        entries.push(&body[start..]);
    }
    entries
}

/// Strip the outer delimiters from a bracketed token slice.
fn bracketed_body<'a>(value: &'a [String], open: &str, close: &str) -> Option<&'a [String]> {
    if value.first().map(String::as_str) != Some(open)
        || value.last().map(String::as_str) != Some(close)
    {
        return None;
    }
    value.get(1..value.len() - 1)
}

/// Expand one export/group list token into its literal names.
///
/// Unlike `static_names_from_arg` in `hir/lower.rs`, this keeps a leading `:`
/// or `-`, so a group reference such as `:all` stays distinguishable from a
/// symbol named `all` and is rejected by [`is_sub_exporter_name`].
fn sub_exporter_literal_names(token: &str) -> Vec<String> {
    let trimmed = token.trim();
    for (open, close) in [("qw(", ')'), ("qw[", ']'), ("qw{", '}'), ("qw<", '>')] {
        if let Some(inner) = trimmed.strip_prefix(open).and_then(|rest| rest.strip_suffix(close)) {
            return inner.split_whitespace().map(str::to_string).collect();
        }
    }
    if let Some(inner) = trimmed
        .strip_prefix("qw")
        .filter(|rest| rest.len() >= 2 && !rest.starts_with(char::is_alphanumeric))
    {
        let mut chars = inner.chars();
        let delimiter = chars.next().unwrap_or('/');
        if let Some(body) = inner.strip_prefix(delimiter).and_then(|r| r.strip_suffix(delimiter)) {
            return body.split_whitespace().map(str::to_string).collect();
        }
    }
    vec![trimmed.trim_matches(',').trim_matches('"').trim_matches('\'').to_string()]
}

/// Remove duplicate names while preserving first-declaration order.
///
/// `Vec::dedup` only collapses *consecutive* duplicates, so it would let
/// `exports => [qw(foo bar foo)]` through as two `foo` facts while collapsing
/// `qw(foo foo bar)` to one. Downstream `ExportSet` sorts and dedups, but
/// `FrameworkFactGraph` does not, so the duplicate would survive as a
/// duplicated compiler fact.
fn dedup_preserving_order(names: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    names.retain(|name| seen.insert(name.clone()));
}

/// One literal name from a token that must expand to exactly one valid name.
fn sub_exporter_single_name(token: &str) -> Option<String> {
    let names = sub_exporter_literal_names(token);
    let [name] = names.as_slice() else {
        return None;
    };
    is_sub_exporter_name(name).then(|| name.clone())
}

/// True when `name` can be a Sub::Exporter export or group member name.
///
/// Group references (`-all`, `:all`) and anything with a sigil, quote, or
/// punctuation are deliberately rejected so callers fall back to a boundary.
fn is_sub_exporter_name(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// Export names declared by a Sub::Exporter `exports` value.
///
/// Accepts both documented forms — `[ qw(foo), bar => \&gen ]` and
/// `{ foo => undef, bar => \&gen }` — and returns `None` for anything that
/// cannot be enumerated without running Perl.
fn sub_exporter_export_names(value: &[String]) -> Option<SubExporterExports> {
    let body = bracketed_body(value, "[", "]").or_else(|| bracketed_body(value, "{", "}"))?;

    let mut names = Vec::new();
    let mut generated = BTreeSet::new();
    for entry in comma_separated_entries(body) {
        // `name => <generator>`: the generator is dynamic, but the exported
        // name itself is declared here. `undef` is Sub::Exporter's spelling
        // for "no generator", so it stays source-anchored — but only when it
        // is the *whole* value. An expression that merely begins with `undef`
        // (`undef // \&build`) still yields a generator, and reading it as
        // source-backed would hand a runtime-only name to the live completion
        // gate at high confidence. `undef()` is that same value written as a
        // call, so it is accepted too; anything beyond those two exact shapes
        // may evaluate to a generator and is treated as one.
        if entry.get(1).map(String::as_str) == Some("=>") {
            let name = sub_exporter_single_name(&entry[0])?;
            let value = entry.get(2..).unwrap_or_default();
            let no_generator = matches!(value, [only] if only == "undef")
                || matches!(value, [word, open, close]
                    if word == "undef" && open == "(" && close == ")");
            if !no_generator {
                generated.insert(name.clone());
            }
            names.push(name);
            continue;
        }

        let [token] = entry else {
            return None;
        };
        let expanded = sub_exporter_literal_names(token);
        if expanded.is_empty() {
            return None;
        }
        for name in expanded {
            if !is_sub_exporter_name(&name) {
                return None;
            }
            names.push(name);
        }
    }

    dedup_preserving_order(&mut names);
    Some(SubExporterExports { names, generated })
}

/// Group name to member list for a Sub::Exporter `groups` value.
///
/// Both documented forms are accepted: Sub::Exporter says "the `groups` list
/// can be passed in the same forms as `exports`", and an `exports` list "may be
/// provided as an array reference or a hash reference". Either way the body is
/// a flat sequence of `name => members` pairs.
///
/// The outer `Option` is `None` when `groups` is not a literal list. One
/// group's members are `None` when that group's value is not a literal list.
fn sub_exporter_group_bodies(
    value: &[String],
) -> Option<Vec<(String, Option<SubExporterGroupMembers>)>> {
    let body = bracketed_body(value, "{", "}").or_else(|| bracketed_body(value, "[", "]"))?;

    let mut groups: Vec<(String, Option<SubExporterGroupMembers>)> = Vec::new();
    for entry in comma_separated_entries(body) {
        if entry.get(1).map(String::as_str) != Some("=>") {
            return None;
        }
        let name = sub_exporter_single_name(&entry[0])?;
        let members = sub_exporter_group_members(entry.get(2..).unwrap_or_default());
        // Perl keeps the last value for a repeated key, so a group defined
        // twice resolves to its last definition rather than the union. The
        // first position is kept so declaration order stays deterministic.
        match groups.iter_mut().find(|(existing, _)| *existing == name) {
            Some((_, existing)) => *existing = members,
            None => groups.push((name, members)),
        }
    }

    Some(groups)
}

/// True when a group member's option value cannot change the installed name.
///
/// Sub::Exporter spells its directives with a leading dash, and `-as` is the
/// one that renames what is installed — `reformat => { -as => 'email_format',
/// width => 72 }` shows both kinds side by side, where `width` is a generator
/// argument. An option hash whose keys are all literal bare names therefore
/// carries no directive at all, so the member installs under its own name.
/// Resting on "a rename directive is dashed" rather than on a list of which
/// dashed options exist keeps this sound without enumerating them.
fn sub_exporter_member_keeps_its_name(value: &[String]) -> bool {
    let Some(body) = bracketed_body(value, "{", "}") else {
        return false;
    };
    comma_separated_entries(body).iter().all(|entry| {
        // Every entry must be a readable `key => value` pair whose key is a
        // literal bare name. A computed key — `$option`, an interpolation, a
        // concatenation, a call — cannot be shown *not* to evaluate to `-as`,
        // so it is not proof of anything and leaves the member unresolved.
        entry.get(1).map(String::as_str) == Some("=>")
            && entry.first().is_some_and(|key| is_sub_exporter_name(unquote_literal(key)))
    })
}

/// Statically enumerable members of one Sub::Exporter group.
fn sub_exporter_group_members(value: &[String]) -> Option<SubExporterGroupMembers> {
    let body = bracketed_body(value, "[", "]")?;

    let mut names = Vec::new();
    let mut complete = true;
    for entry in comma_separated_entries(body) {
        // `rabbit => { -as => 'coney' }`: the group references an export but
        // installs it under a different name, so the export name is not what
        // a consumer of this group ends up with. The plain members beside it
        // are still exactly known, so record the gap instead of discarding
        // the whole group.
        //
        // `item => { width => 72 }` is the other half of the same syntax —
        // generator arguments, no rename — and that member does still install
        // under its own name, so withholding it would hide a valid import.
        if entry.get(1).map(String::as_str) == Some("=>") {
            let member = sub_exporter_single_name(&entry[0])
                .filter(|_| sub_exporter_member_keeps_its_name(entry.get(2..).unwrap_or_default()));
            match member {
                Some(name) => names.push(name),
                None => complete = false,
            }
            continue;
        }

        let [token] = entry else {
            complete = false;
            continue;
        };
        let expanded = sub_exporter_literal_names(token);
        if expanded.is_empty() {
            complete = false;
            continue;
        }
        for name in expanded {
            // A `-name`/`:name` member names another group, whose contents
            // this pass does not expand.
            if is_sub_exporter_name(&name) {
                names.push(name);
            } else {
                complete = false;
            }
        }
    }

    dedup_preserving_order(&mut names);
    Some(SubExporterGroupMembers { names, complete })
}
