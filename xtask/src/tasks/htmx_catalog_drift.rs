//! Maintainer drift report for the reviewed htmx catalog snapshot.
//!
//! `perl-lsp-rs-core` owns the catalog and its provenance; this task owns only
//! the comparison. Nothing here fetches. The maintainer obtains a copy of the
//! htmx reference document deliberately and passes its path, so ordinary PR CI
//! has no network path to take even by accident, and refreshing the catalog
//! stays a reviewed source change rather than an automatic update.

use color_eyre::eyre::{Context, Result, bail};
use perl_lsp_rs_core::providers::{
    HTMX_ATTRIBUTES, HTMX_CATALOG_PROVENANCE, HTMX_HEADERS, HtmxHeaderDirection,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// One table section of the htmx reference document.
struct SectionSpec {
    /// Heading anchor id. htmx's own documentation links to these ids, so they
    /// survive prose edits to the heading text around them.
    anchor: &'static str,
    /// Human-facing name used in failure messages.
    label: &'static str,
}

static CORE_ATTRIBUTES: SectionSpec =
    SectionSpec { anchor: "{#attributes}", label: "core attributes" };
static ADDITIONAL_ATTRIBUTES: SectionSpec =
    SectionSpec { anchor: "{#attributes-additional}", label: "additional attributes" };
static REQUEST_HEADERS: SectionSpec =
    SectionSpec { anchor: "{#request_headers}", label: "request headers" };
static RESPONSE_HEADERS: SectionSpec =
    SectionSpec { anchor: "{#response_headers}", label: "response headers" };

/// A header whose direction differs between the catalog and the reference.
#[derive(Debug, PartialEq, Eq)]
struct DirectionChange {
    name: String,
    catalog: HtmxHeaderDirection,
    reference: HtmxHeaderDirection,
}

/// The proposed change from the committed catalog to the supplied reference.
#[derive(Debug, Default, PartialEq, Eq)]
struct DriftReport {
    added_attributes: Vec<String>,
    removed_attributes: Vec<String>,
    added_headers: Vec<String>,
    removed_headers: Vec<String>,
    direction_changes: Vec<DirectionChange>,
}

impl DriftReport {
    /// Does the committed catalog already match the supplied reference?
    fn is_clean(&self) -> bool {
        self.added_attributes.is_empty()
            && self.removed_attributes.is_empty()
            && self.added_headers.is_empty()
            && self.removed_headers.is_empty()
            && self.direction_changes.is_empty()
    }
}

/// Report htmx catalog drift against a maintainer-supplied reference document.
pub fn run(reference: &Path) -> Result<()> {
    let document = fs::read_to_string(reference)
        .with_context(|| format!("reading htmx reference document {}", reference.display()))?;

    let report = compare_snapshot(
        &reference_attributes(&document)?,
        &reference_headers(&document)?,
        &catalog_attributes(),
        &catalog_headers(),
    );

    print_provenance(reference);

    if report.is_clean() {
        println!("\nno drift: the committed catalog matches the supplied reference document");
        return Ok(());
    }

    print_report(&report);
    bail!("htmx catalog drift detected against {}", reference.display())
}

/// Print which snapshot is committed and which document it is compared against.
///
/// The report is only meaningful next to the identity of the two sides.
fn print_provenance(reference: &Path) {
    let provenance = HTMX_CATALOG_PROVENANCE;
    println!("committed snapshot");
    println!("  htmx version : {}", provenance.htmx_version);
    println!("  contract     : {}.{}", provenance.contract_major, provenance.contract_minor);
    println!("  reviewed on  : {}", provenance.reviewed_on);
    println!("  commit       : {}", provenance.reference_commit);
    println!("  reference    : {}", provenance.reference_url);
    println!("compared against");
    println!("  local file   : {}", reference.display());
}

/// Print the proposed catalog change, one line per differing name or direction.
fn print_report(report: &DriftReport) {
    println!("\nproposed catalog change");
    for name in &report.added_attributes {
        println!("  + attribute {name}");
    }
    for name in &report.removed_attributes {
        println!("  - attribute {name}");
    }
    for name in &report.added_headers {
        println!("  + header    {name}");
    }
    for name in &report.removed_headers {
        println!("  - header    {name}");
    }
    for change in &report.direction_changes {
        println!(
            "  ~ header    {} direction {:?} -> {:?}",
            change.name, change.catalog, change.reference
        );
    }
    println!(
        "\nrefreshing the catalog is a reviewed source change: update \
         crates/perl-lsp-rs-core/src/providers/htmx/catalog.rs together with its provenance."
    );
}

/// The committed catalog's attribute names.
fn catalog_attributes() -> BTreeSet<String> {
    HTMX_ATTRIBUTES.iter().map(|attribute| attribute.name.to_string()).collect()
}

/// The committed catalog's header names with the direction each is used in.
fn catalog_headers() -> BTreeMap<String, HtmxHeaderDirection> {
    HTMX_HEADERS.iter().map(|header| (header.name.to_string(), header.direction)).collect()
}

/// Diff a reference snapshot against the committed catalog.
///
/// Names are compared with exact casing, so a capitalization-only upstream
/// change surfaces as an addition and a removal rather than vanishing through
/// normalization. Attribute metadata (deprecation, family) is deliberately not
/// compared: it lives in upstream prose, and inferring it would reintroduce the
/// fragile parsing this task otherwise avoids.
fn compare_snapshot(
    reference_attributes: &BTreeSet<String>,
    reference_headers: &BTreeMap<String, HtmxHeaderDirection>,
    catalog_attributes: &BTreeSet<String>,
    catalog_headers: &BTreeMap<String, HtmxHeaderDirection>,
) -> DriftReport {
    let mut report = DriftReport {
        added_attributes: reference_attributes.difference(catalog_attributes).cloned().collect(),
        removed_attributes: catalog_attributes.difference(reference_attributes).cloned().collect(),
        ..DriftReport::default()
    };

    for (name, reference) in reference_headers {
        match catalog_headers.get(name) {
            None => report.added_headers.push(name.clone()),
            // A name-only comparison would accept a header that moved between
            // the request and response tables, which changes what the server
            // may claim about it.
            Some(catalog) if catalog != reference => {
                report.direction_changes.push(DirectionChange {
                    name: name.clone(),
                    catalog: *catalog,
                    reference: *reference,
                })
            }
            Some(_) => {}
        }
    }

    for name in catalog_headers.keys() {
        if !reference_headers.contains_key(name) {
            report.removed_headers.push(name.clone());
        }
    }

    report
}

/// Attribute names from the reference document's two attribute sections.
fn reference_attributes(document: &str) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for section in [&CORE_ATTRIBUTES, &ADDITIONAL_ATTRIBUTES] {
        for name in section_names(document, section)? {
            names.insert(canonical_attribute_name(&name));
        }
    }
    Ok(names)
}

/// Header names from the reference document, with direction from section membership.
fn reference_headers(document: &str) -> Result<BTreeMap<String, HtmxHeaderDirection>> {
    let request: BTreeSet<String> =
        section_names(document, &REQUEST_HEADERS)?.into_iter().collect();
    let response: BTreeSet<String> =
        section_names(document, &RESPONSE_HEADERS)?.into_iter().collect();

    // Direction is decided by section membership, not by insertion order. A
    // name repeated inside one table must not promote itself to bidirectional:
    // merging row by row would let a duplicated response row report a false
    // direction change against a catalog that is actually correct.
    //
    // A header listed in both tables is genuinely bidirectional, which is how
    // the catalog spells `HX-Trigger`.
    Ok(request
        .union(&response)
        .map(|name| {
            let direction = match (request.contains(name), response.contains(name)) {
                (true, true) => HtmxHeaderDirection::RequestAndResponse,
                (true, false) => HtmxHeaderDirection::Request,
                _ => HtmxHeaderDirection::Response,
            };
            (name.clone(), direction)
        })
        .collect())
}

/// Translate an upstream attribute spelling into the catalog's spelling.
///
/// Driven by the recorded provenance rather than a literal here, so the
/// transcription stays owned by the catalog that made it.
fn canonical_attribute_name(upstream: &str) -> String {
    if upstream == HTMX_CATALOG_PROVENANCE.upstream_event_handler_name {
        HTMX_CATALOG_PROVENANCE.catalog_event_handler_name.to_string()
    } else {
        upstream.to_string()
    }
}

/// Ordered section anchors used to slice the reference document.
const SLICED_SECTIONS: &[&SectionSpec] =
    &[&CORE_ATTRIBUTES, &ADDITIONAL_ATTRIBUTES, &REQUEST_HEADERS, &RESPONSE_HEADERS];

/// Extract htmx catalog names from one reference section using anchor slicing.
///
/// The slice runs from the line after this section's anchored heading until the
/// next anchored heading in [`SLICED_SECTIONS`]. Every backtick-quoted token in
/// the slice that matches the htmx name shape for that section is collected.
/// There is no block-level Markdown parsing: prose, comments, and code samples
/// in the slice may contribute false-positive names (#14602).
fn section_names(document: &str, section: &SectionSpec) -> Result<Vec<String>> {
    let slice = section_slice(document, section)?;
    let mut names = BTreeSet::new();
    for token in backtick_tokens_in_slice(slice).into_iter().chain(shaped_tokens_in_slice(slice)) {
        if let Some(name) = htmx_name_from_token(&token, section) {
            names.insert(name);
        }
    }

    if names.is_empty() {
        bail!(
            "the {} section ({}) yielded no names; the reference table shape has changed and a \
             drift report would be vacuous",
            section.label,
            section.anchor
        );
    }

    Ok(names.into_iter().collect())
}

/// Text after this section's anchored heading until the next sliced section heading.
fn section_slice<'doc>(document: &'doc str, section: &SectionSpec) -> Result<&'doc str> {
    let (start_line, heading_depth) = locate_section(document, section)?;
    let anchor_end = next_section_start_line(document, section);
    let structural_end = section_end_by_heading_depth(document, start_line, heading_depth);
    let end_line = anchor_end.map_or(structural_end, |anchor| anchor.min(structural_end));
    let (start, end) = line_byte_range(document, start_line + 1, end_line)?;
    document
        .get(start..end)
        .ok_or_else(|| color_eyre::eyre::eyre!("invalid slice bounds for {}", section.label))
}

/// Byte range covering lines `[start, end)` in `document`.
fn line_byte_range(document: &str, start: usize, end: usize) -> Result<(usize, usize)> {
    if start > end {
        bail!("internal line range {start}..{end} is inverted");
    }

    let mut byte_start = None;
    let mut byte_end = document.len();
    let mut line_index = 0usize;
    let mut offset = 0usize;
    for line in document.split_inclusive('\n') {
        if line_index == start {
            byte_start = Some(offset);
        }
        if line_index == end {
            byte_end = offset;
            break;
        }
        offset += line.len();
        line_index += 1;
    }

    match byte_start {
        Some(start) => Ok((start, byte_end)),
        None if start == end && end == line_index => Ok((document.len(), document.len())),
        None => bail!("internal line range start {start} out of bounds"),
    }
}

/// First line after `start_line` whose ATX heading is at the same or shallower depth.
fn section_end_by_heading_depth(document: &str, start_line: usize, heading_depth: usize) -> usize {
    for (index, line) in document.lines().enumerate().skip(start_line + 1) {
        if !is_atx_heading_line(line) {
            continue;
        }
        let depth = atx_heading_depth(line.trim());
        if depth > 0 && depth <= heading_depth {
            return index;
        }
    }
    document.lines().count()
}

fn is_atx_heading_line(line: &str) -> bool {
    !(line.starts_with('\t') || line.starts_with("    ")) && atx_heading_depth(line.trim()) > 0
}

/// Line index and ATX depth of the one heading carrying this section's anchor.
fn locate_section(document: &str, section: &SectionSpec) -> Result<(usize, usize)> {
    let mut located = None;
    let mut in_fence = false;
    for (index, line) in document.lines().enumerate() {
        let trimmed = line.trim();
        if toggles_code_fence(trimmed) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if !is_locatable_section_heading(line, section.anchor) {
            continue;
        }
        if located.is_some() {
            bail!(
                "reference document has more than one {} heading ({}); the section's identity is \
                 ambiguous, and choosing one would attribute the wrong rows to it",
                section.label,
                section.anchor
            );
        }
        located = Some((index, atx_heading_depth(trimmed)));
    }

    located.ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "reference document has no {} section ({}); refusing to report a clean diff against \
             a document this task cannot read",
            section.label,
            section.anchor
        )
    })
}

/// A bare ``` or ~~~ fence delimiter line at column zero toggles illustration mode.
fn toggles_code_fence(trimmed_line: &str) -> bool {
    let Some(marker) = trimmed_line.chars().next() else {
        return false;
    };
    if marker != '`' && marker != '~' {
        return false;
    }
    trimmed_line.chars().take_while(|character| *character == marker).count() >= 3
}

/// Line index of the next section heading in slice order, if any.
fn next_section_start_line(document: &str, section: &SectionSpec) -> Option<usize> {
    let index_in_order =
        SLICED_SECTIONS.iter().position(|candidate| candidate.anchor == section.anchor)?;
    let next = SLICED_SECTIONS.get(index_in_order + 1)?;
    locate_section(document, next).ok().map(|(line, _depth)| line)
}

/// Does this line introduce a section heading carrying `anchor`?
///
/// This is not a Markdown parser: it only checks ATX heading prefix and anchor
/// suffix so prose ending in an anchor id does not locate a section.
fn is_locatable_section_heading(line: &str, anchor: &str) -> bool {
    if line.starts_with('\t') || line.starts_with("    ") {
        return false;
    }
    let trimmed = line.trim();
    if !trimmed.ends_with(anchor) {
        return false;
    }
    atx_heading_depth(trimmed) > 0
}

/// ATX heading depth for section location, or zero when the line is not a heading.
fn atx_heading_depth(trimmed_line: &str) -> usize {
    let hashes = trimmed_line.chars().take_while(|character| *character == '#').count();
    if !(1..=6).contains(&hashes) {
        return 0;
    }

    match trimmed_line.chars().nth(hashes) {
        None => hashes,
        Some(character) if character.is_whitespace() => hashes,
        Some(_) => 0,
    }
}

/// Every same-line backtick-delimited token in `slice`.
fn backtick_tokens_in_slice(slice: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for line in slice.lines() {
        let mut rest = line;
        while let Some(start) = rest.find('`') {
            let after_opener = &rest[start + 1..];
            let Some(end) = after_opener.find('`') else {
                break;
            };
            let token = &after_opener[..end];
            if !token.is_empty() {
                tokens.push(token.to_string());
            }
            rest = &after_opener[end + 1..];
        }
    }
    tokens
}

/// Scan slice text for bare `hx-*` / `HX-*` spellings outside backticks.
fn shaped_tokens_in_slice(slice: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < slice.len() {
        if slice[index..].starts_with("hx-") {
            let start = index;
            index += 3;
            while index < slice.len() {
                let Some(character) = slice[index..].chars().next() else {
                    break;
                };
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '*' | ':') {
                    index += character.len_utf8();
                } else {
                    break;
                }
            }
            if slice[index..].starts_with(".md") {
                continue;
            }
            tokens.push(slice[start..index].to_string());
            continue;
        }
        if slice[index..].starts_with("HX-") {
            let start = index;
            index += 3;
            while index < slice.len() {
                let Some(character) = slice[index..].chars().next() else {
                    break;
                };
                if character.is_ascii_alphanumeric() || character == '-' {
                    index += character.len_utf8();
                } else {
                    break;
                }
            }
            if slice[index..].starts_with(".md") {
                continue;
            }
            tokens.push(slice[start..index].to_string());
            continue;
        }
        index += 1;
    }
    tokens
}

fn htmx_name_from_token(token: &str, section: &SectionSpec) -> Option<String> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    match section.anchor {
        anchor if anchor == CORE_ATTRIBUTES.anchor || anchor == ADDITIONAL_ATTRIBUTES.anchor => {
            is_htmx_attribute_token(token).then(|| token.to_string())
        }
        _ => is_htmx_header_token(token).then(|| token.to_string()),
    }
}

/// Narrow enough to keep ordinary prose (`GET`, `true`) out of the comparison.
fn is_htmx_attribute_token(token: &str) -> bool {
    let Some(rest) = token.strip_prefix("hx-") else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    rest.chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '*' | ':'))
}

/// Header tokens use the catalog's `HX-` spelling.
fn is_htmx_header_token(token: &str) -> bool {
    let Some(rest) = token.strip_prefix("HX-") else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    rest.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_ATTRIBUTES, DirectionChange, DriftReport, REQUEST_HEADERS, RESPONSE_HEADERS,
        catalog_attributes, catalog_headers, compare_snapshot, reference_attributes,
        reference_headers, section_names,
    };
    use perl_lsp_rs_core::providers::{
        HTMX_ATTRIBUTES, HTMX_CATALOG_PROVENANCE, HTMX_HEADERS, HtmxHeaderDirection,
    };
    use std::collections::{BTreeMap, BTreeSet};

    const EXCERPT: &str = "\
# htmx Reference

## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md)  | issues a `GET` to the specified URL |
| [`hx-on*`](@/attributes/hx-on.md)   | handle events with inline scripts   |

## Additional Attribute Reference {#attributes-additional}

| Attribute | Description |
|-----------|-------------|
| [`hx-boost`](@/attributes/hx-boost.md) | progressively enhances links |
| [`hx-vars`](@/attributes/hx-vars.md)   | deprecated, please use `hx-vals` |

## HTTP Header Reference {#headers}

### Request Headers Reference {#request_headers}

| Header | Description |
|--------|-------------|
| `HX-Boosted` | indicates the request is via an element using hx-boost
| `HX-Trigger` | the id of the triggered element

### Response Headers Reference {#response_headers}

| Header | Description |
|--------|-------------|
| [`HX-Redirect`](@/headers/hx-redirect.md) | can be used to do a client-side redirect |
| [`HX-Trigger`](@/headers/hx-trigger.md)   | allows you to trigger client-side events |
";

    fn attribute_set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    fn header_map(
        entries: &[(&str, HtmxHeaderDirection)],
    ) -> BTreeMap<String, HtmxHeaderDirection> {
        entries.iter().map(|(name, direction)| ((*name).to_string(), *direction)).collect()
    }

    fn document_from_catalog() -> String {
        let provenance = HTMX_CATALOG_PROVENANCE;
        let mut document = String::from("## Core Attribute Reference {#attributes}\n\n");
        document.push_str("| Attribute | Description |\n|---|---|\n");
        for attribute in HTMX_ATTRIBUTES {
            let name = if attribute.name == provenance.catalog_event_handler_name {
                provenance.upstream_event_handler_name
            } else {
                attribute.name
            };
            document.push_str(&format!("| [`{name}`](@/attributes/{name}.md) | described |\n"));
        }

        document.push_str("\n## Additional Attribute Reference {#attributes-additional}\n\n");
        document.push_str("| Attribute | Description |\n|---|---|\n");
        document.push_str("| [`hx-boost`](@/attributes/hx-boost.md) | described |\n");

        document.push_str("\n## HTTP Header Reference {#headers}\n");

        for (anchor, wanted) in [
            ("{#request_headers}", HtmxHeaderDirection::Request),
            ("{#response_headers}", HtmxHeaderDirection::Response),
        ] {
            document.push_str(&format!("\n### Headers Reference {anchor}\n\n"));
            document.push_str("| Header | Description |\n|---|---|\n");
            for header in HTMX_HEADERS {
                if header.direction == wanted
                    || header.direction == HtmxHeaderDirection::RequestAndResponse
                {
                    document.push_str(&format!("| `{}` | described |\n", header.name));
                }
            }
        }

        document
    }

    #[test]
    fn extracts_linked_and_unlinked_names_and_ignores_unrelated_backticks() {
        let core = section_names(EXCERPT, &CORE_ATTRIBUTES);
        let request = section_names(EXCERPT, &REQUEST_HEADERS);

        assert!(core.is_ok_and(|names| {
            names.len() == 2
                && names.iter().any(|name| name == "hx-get")
                && names.iter().any(|name| name == "hx-on*")
        }));
        assert!(request.is_ok_and(|names| {
            names.len() == 2
                && names.iter().any(|name| name == "HX-Boosted")
                && names.iter().any(|name| name == "HX-Trigger")
        }));
    }

    #[test]
    fn ordinary_prose_backticks_do_not_enter_the_comparison() {
        let core = section_names(EXCERPT, &CORE_ATTRIBUTES).expect("core section");
        assert!(!core.iter().any(|name| name == "GET"));
    }

    #[test]
    fn a_subsection_does_not_end_the_slice() {
        let document = "\
## Core Attribute Reference {#attributes}

| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

### Deprecated Attributes

| [`hx-new-thing`](@/attributes/hx-new-thing.md) | a new upstream entry |

## Additional Attribute Reference {#attributes-additional}

| [`hx-boost`](@/attributes/hx-boost.md) | not part of the core section |
";
        assert!(section_names(&document, &CORE_ATTRIBUTES).is_ok_and(|names| {
            names.len() == 2
                && names.iter().any(|name| name == "hx-get")
                && names.iter().any(|name| name == "hx-new-thing")
        }));
    }

    #[test]
    fn a_duplicated_section_anchor_fails_instead_of_taking_the_first() {
        let decoy = "\
## Deprecated, see below {#request_headers}

| `HX-Decoy-Only` | not real |

### Request Headers Reference {#request_headers}

| `HX-Request` | set to true on htmx requests |
";
        assert!(
            section_names(&decoy, &REQUEST_HEADERS)
                .is_err_and(|error| error.to_string().contains("more than one"))
        );
    }

    #[test]
    fn a_missing_section_fails_instead_of_reporting_no_drift() {
        let without_core = EXCERPT.replace("{#attributes}", "{#renamed-anchor}");
        let error = section_names(&without_core, &CORE_ATTRIBUTES);
        assert!(error.is_err_and(|error| error.to_string().contains("core attributes")));
    }

    #[test]
    fn a_section_with_no_htmx_names_fails_instead_of_reporting_no_drift() {
        let prose_only = "\
## Core Attribute Reference {#attributes}

The core attributes are now documented on their own pages.

## Additional Attribute Reference {#attributes-additional}
";
        assert!(
            section_names(&prose_only, &CORE_ATTRIBUTES)
                .is_err_and(|error| error.to_string().contains("vacuous"))
        );
    }

    #[test]
    fn prose_ending_in_an_anchor_is_not_a_section_heading() {
        let prose = "\
# Reference

See the core attribute reference {#attributes}

| [`hx-phantom`](@/attributes/hx-phantom.md) | not under a heading |
";
        let error = section_names(&prose, &CORE_ATTRIBUTES)
            .expect_err("prose ending in the anchor is not a heading");
        assert!(error.to_string().contains("has no core attributes section"));
    }

    #[test]
    fn a_backtick_quoted_name_in_a_code_sample_inside_a_slice_is_reported() {
        let document = "\
## Core Attribute Reference {#attributes}

| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

```markdown
| `hx-phantom-sample` | only an example |
```
";
        let names = section_names(&document, &CORE_ATTRIBUTES).expect("slice names");
        assert!(
            names.iter().any(|name| name == "hx-phantom-sample"),
            "traded false-positive direction must surface sample names: {names:?}"
        );
    }

    #[test]
    fn an_unquoted_attribute_row_is_still_collected() {
        let document = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |
| hx-new | added without backticks |

## Additional Attribute Reference {#attributes-additional}
";
        assert!(
            section_names(&document, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names.iter().any(|name| name == "hx-new")
                    && names.iter().any(|name| name == "hx-get"))
        );
    }

    #[test]
    fn crlf_line_endings_do_not_truncate_the_final_name() {
        let document = "## Core Attribute Reference {#attributes}\r\n\r\n\
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |\r\n\
| [`hx-new`](@/attributes/hx-new.md) | added upstream |\r\n\
## Additional Attribute Reference {#attributes-additional}\r\n";
        assert!(
            section_names(document, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names.iter().any(|name| name == "hx-new"))
        );
    }

    #[test]
    fn a_response_header_moved_below_a_shallower_heading_leaves_the_response_slice() {
        let document = "\
### Response Headers Reference {#response_headers}

| `HX-Other` | still in the response table |

## Other Headers

| `HX-Redirect` | moved out of the response table |
";
        let response = section_names(document, &RESPONSE_HEADERS).expect("response section");
        assert!(response.iter().any(|name| name == "HX-Other"));
        assert!(!response.iter().any(|name| name == "HX-Redirect"));
    }

    #[test]
    fn a_fenced_example_heading_does_not_mask_the_real_section() {
        let document = "\
```text
## Core Attribute Reference {#attributes}
| `hx-phantom` | sample only |
```

## Core Attribute Reference {#attributes}

| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

## Additional Attribute Reference {#attributes-additional}
";
        assert!(
            section_names(&document, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names.len() == 1 && names[0] == "hx-get")
        );
    }

    mod legacy_defect_class {
        //! Re-expressions of the #14573 silent-read fixtures under anchor slicing.
        //! Several now fail loudly via false-positive names rather than silent clean reads.

        use super::{CORE_ATTRIBUTES, section_names};

        fn core(document: &str) -> Vec<String> {
            section_names(document, &CORE_ATTRIBUTES).expect("core section names")
        }

        #[test]
        fn hash_prefixed_prose_does_not_end_the_slice() {
            let document = "\
## Core Attribute Reference {#attributes}

| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

Use #hashtag routing, and note that ##css-selector is not a heading.

| [`hx-post`](@/attributes/hx-post.md) | issues a POST |

## Additional Attribute Reference {#attributes-additional}
";
            assert_eq!(core(document), vec!["hx-get".to_string(), "hx-post".to_string()]);
        }

        #[test]
        fn rows_without_leading_pipes_still_contribute_backtick_names() {
            let document = "\
## Core Attribute Reference {#attributes}

[`hx-get`](@/attributes/hx-get.md) | issues a GET
[`hx-new-thing`](@/attributes/hx-new-thing.md) | a new upstream entry

## Additional Attribute Reference {#attributes-additional}
";
            assert_eq!(core(document), vec!["hx-get".to_string(), "hx-new-thing".to_string()]);
        }

        #[test]
        fn fence_and_comment_markers_do_not_silently_drop_later_names() {
            let document = "\
## Core Attribute Reference {#attributes}

| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

```markdown
~~~
| `hx-phantom` | only an example |
~~~
```

| [`hx-post`](@/attributes/hx-post.md) | issues a POST |

## Additional Attribute Reference {#attributes-additional}
";
            let names = core(&document);
            assert!(
                names.iter().any(|name| name == "hx-get")
                    && names.iter().any(|name| name == "hx-post")
            );
        }

        #[test]
        fn commented_tables_may_false_positive_instead_of_silent_clean() {
            let document = "\
## Core Attribute Reference {#attributes}

<!--
| [`hx-retired`](@/attributes/hx-retired.md) | removed upstream |
-->

| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

## Additional Attribute Reference {#attributes-additional}
";
            let names = core(&document);
            assert!(names.iter().any(|name| name == "hx-get"));
            assert!(
                names.iter().any(|name| name == "hx-retired"),
                "comment text stays in the slice and may report stale names loudly: {names:?}"
            );
        }

        #[test]
        fn indented_sample_headings_do_not_locate_a_missing_section() {
            let document = "\
# Reference

    ## Core Attribute Reference {#attributes}
    | [`hx-phantom`](@/attributes/hx-phantom.md) | sample |

## Something Else {#other}
";
            let error = section_names(&document, &CORE_ATTRIBUTES)
                .expect_err("indented sample headings are not locatable");
            assert!(error.to_string().contains("has no core attributes section"));
        }
    }

    #[test]
    fn a_header_repeated_within_one_table_keeps_that_table_s_direction() {
        let repeated = "\
### Request Headers Reference {#request_headers}

| `HX-Request` | set to true on htmx requests |

### Response Headers Reference {#response_headers}

| `HX-Redirect` | client-side redirect |
| `HX-Redirect` | listed twice upstream |
";
        assert!(reference_headers(repeated).is_ok_and(|headers| {
            headers.get("HX-Redirect") == Some(&HtmxHeaderDirection::Response)
                && headers.get("HX-Request") == Some(&HtmxHeaderDirection::Request)
        }));
    }

    #[test]
    fn the_upstream_dynamic_family_spelling_is_reconciled_to_the_catalog_spelling() {
        let attributes = reference_attributes(EXCERPT);
        assert!(attributes.is_ok_and(|names| {
            names.contains(HTMX_CATALOG_PROVENANCE.catalog_event_handler_name)
                && !names.contains(HTMX_CATALOG_PROVENANCE.upstream_event_handler_name)
        }));
    }

    #[test]
    fn a_header_in_both_reference_tables_is_bidirectional() {
        let headers = reference_headers(EXCERPT);
        assert!(headers.is_ok_and(|headers| {
            headers.get("HX-Trigger") == Some(&HtmxHeaderDirection::RequestAndResponse)
                && headers.get("HX-Boosted") == Some(&HtmxHeaderDirection::Request)
                && headers.get("HX-Redirect") == Some(&HtmxHeaderDirection::Response)
        }));
    }

    #[test]
    fn an_identical_snapshot_reports_no_drift() {
        let report = compare_snapshot(
            &attribute_set(&["hx-get"]),
            &header_map(&[("HX-Request", HtmxHeaderDirection::Request)]),
            &attribute_set(&["hx-get"]),
            &header_map(&[("HX-Request", HtmxHeaderDirection::Request)]),
        );
        assert!(report.is_clean());
    }

    #[test]
    fn added_and_removed_names_appear_in_the_proposed_change() {
        let report = compare_snapshot(
            &attribute_set(&["hx-get", "hx-new"]),
            &header_map(&[("HX-New", HtmxHeaderDirection::Response)]),
            &attribute_set(&["hx-get", "hx-retired"]),
            &header_map(&[("HX-Retired", HtmxHeaderDirection::Response)]),
        );
        assert_eq!(
            report,
            DriftReport {
                added_attributes: vec!["hx-new".to_string()],
                removed_attributes: vec!["hx-retired".to_string()],
                added_headers: vec!["HX-New".to_string()],
                removed_headers: vec!["HX-Retired".to_string()],
                direction_changes: Vec::new(),
            }
        );
    }

    #[test]
    fn a_header_that_changed_direction_is_not_accepted_on_a_name_match() {
        let report = compare_snapshot(
            &attribute_set(&["hx-get"]),
            &header_map(&[("HX-Trigger", HtmxHeaderDirection::RequestAndResponse)]),
            &attribute_set(&["hx-get"]),
            &header_map(&[("HX-Trigger", HtmxHeaderDirection::Request)]),
        );
        assert!(!report.is_clean());
        assert_eq!(
            report.direction_changes,
            vec![DirectionChange {
                name: "HX-Trigger".to_string(),
                catalog: HtmxHeaderDirection::Request,
                reference: HtmxHeaderDirection::RequestAndResponse,
            }]
        );
    }

    #[test]
    fn the_catalog_survives_a_render_and_re_extract_round_trip() {
        let document = document_from_catalog();
        let snapshot = reference_attributes(&document)
            .and_then(|attributes| reference_headers(&document).map(|h| (attributes, h)));
        assert!(snapshot.is_ok_and(|(attributes, headers)| {
            compare_snapshot(&attributes, &headers, &catalog_attributes(), &catalog_headers())
                .is_clean()
        }));
    }
}
