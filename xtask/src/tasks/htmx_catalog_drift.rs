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

fn print_provenance(reference: &Path) {
    let provenance = HTMX_CATALOG_PROVENANCE;
    println!("committed snapshot");
    println!("  htmx version : {}", provenance.htmx_version);
    println!("  contract     : {}.{}", provenance.contract_major, provenance.contract_minor);
    println!("  reviewed on  : {}", provenance.reviewed_on);
    println!("  reference    : {}", provenance.reference_url);
    println!("compared against");
    println!("  local file   : {}", reference.display());
}

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

fn catalog_attributes() -> BTreeSet<String> {
    HTMX_ATTRIBUTES.iter().map(|attribute| attribute.name.to_string()).collect()
}

fn catalog_headers() -> BTreeMap<String, HtmxHeaderDirection> {
    HTMX_HEADERS.iter().map(|header| (header.name.to_string(), header.direction)).collect()
}

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

fn reference_attributes(document: &str) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for section in [&CORE_ATTRIBUTES, &ADDITIONAL_ATTRIBUTES] {
        for name in section_names(document, section)? {
            names.insert(canonical_attribute_name(&name));
        }
    }
    Ok(names)
}

fn reference_headers(document: &str) -> Result<BTreeMap<String, HtmxHeaderDirection>> {
    let mut headers: BTreeMap<String, HtmxHeaderDirection> = BTreeMap::new();

    for name in section_names(document, &REQUEST_HEADERS)? {
        headers.insert(name, HtmxHeaderDirection::Request);
    }
    // A header listed in both reference tables is bidirectional, which is how
    // the catalog spells `HX-Trigger`.
    for name in section_names(document, &RESPONSE_HEADERS)? {
        headers
            .entry(name)
            .and_modify(|direction| *direction = HtmxHeaderDirection::RequestAndResponse)
            .or_insert(HtmxHeaderDirection::Response);
    }

    Ok(headers)
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

/// Extract the backtick-quoted names from one reference table section.
///
/// Fails closed: a missing section or a section that yields no names is an
/// error rather than an empty result, because reporting "no drift" from a
/// document this task could not read is the primary defect risk here.
fn section_names(document: &str, section: &SectionSpec) -> Result<Vec<String>> {
    let mut lines = document.lines();

    if !lines.by_ref().any(|line| is_section_heading(line, section.anchor)) {
        bail!(
            "reference document has no {} section ({}); refusing to report a clean diff against \
             a document this task cannot read",
            section.label,
            section.anchor
        );
    }

    let mut names = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            break;
        }
        if let Some(name) = table_row_name(trimmed) {
            names.push(name);
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

    Ok(names)
}

fn is_section_heading(line: &str, anchor: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('#') && trimmed.ends_with(anchor)
}

/// Read the name out of a reference table row.
///
/// Names are backtick-quoted in the first cell, optionally wrapped in a link.
/// Header and separator rows carry no backticks and are skipped.
fn table_row_name(line: &str) -> Option<String> {
    let first_cell = line.strip_prefix('|')?.split('|').next()?;
    let mut spans = first_cell.split('`');
    spans.next()?;
    let name = spans.next()?;
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_ATTRIBUTES, DirectionChange, DriftReport, REQUEST_HEADERS, catalog_attributes,
        catalog_headers, compare_snapshot, reference_attributes, reference_headers, section_names,
    };
    use perl_lsp_rs_core::providers::{
        HTMX_ATTRIBUTES, HTMX_CATALOG_PROVENANCE, HTMX_HEADERS, HtmxHeaderDirection,
    };
    use std::collections::{BTreeMap, BTreeSet};

    /// A literal excerpt in the shape htmx actually publishes. Upstream is not
    /// uniform: attribute and response-header names are wrapped in links while
    /// request-header names are bare, and request-header rows have no trailing
    /// pipe. The fixture reproduces all three so extraction is proven against
    /// the real spread rather than one tidy shape.
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

    /// Render the committed catalog back into the reference document shape,
    /// using the upstream spelling of the dynamic family.
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

        // One section is enough to carry every attribute; the additional table
        // only has to exist and be non-empty for extraction to succeed.
        document.push_str("\n## Additional Attribute Reference {#attributes-additional}\n\n");
        document.push_str("| Attribute | Description |\n|---|---|\n");
        document.push_str("| [`hx-boost`](@/attributes/hx-boost.md) | described |\n");

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
    fn extracts_linked_and_unlinked_names_and_skips_table_furniture() {
        let core = section_names(EXCERPT, &CORE_ATTRIBUTES);
        let request = section_names(EXCERPT, &REQUEST_HEADERS);

        assert!(core.is_ok_and(|names| names == ["hx-get", "hx-on*"]));
        assert!(request.is_ok_and(|names| names == ["HX-Boosted", "HX-Trigger"]));
    }

    #[test]
    fn a_missing_section_fails_instead_of_reporting_no_drift() {
        let without_core = EXCERPT.replace("{#attributes}", "{#renamed-anchor}");
        let error = section_names(&without_core, &CORE_ATTRIBUTES);

        assert!(error.is_err_and(|error| error.to_string().contains("core attributes")));
    }

    #[test]
    fn an_empty_section_fails_instead_of_reporting_no_drift() {
        // The section is present but its rows no longer carry backticked names,
        // which is what a real upstream table restructure would look like.
        let flattened = EXCERPT
            .replace("[`hx-get`](@/attributes/hx-get.md)", "hx-get")
            .replace("[`hx-on*`](@/attributes/hx-on.md)", "hx-on*");
        let error = section_names(&flattened, &CORE_ATTRIBUTES);

        assert!(error.is_err_and(|error| error.to_string().contains("vacuous")));
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
        assert!(report.added_headers.is_empty() && report.removed_headers.is_empty());
    }

    #[test]
    fn the_committed_catalog_is_clean_against_its_own_reviewed_shape() {
        let document = document_from_catalog();
        let snapshot = reference_attributes(&document)
            .and_then(|attributes| reference_headers(&document).map(|h| (attributes, h)));

        assert!(snapshot.is_ok_and(|(attributes, headers)| {
            compare_snapshot(&attributes, &headers, &catalog_attributes(), &catalog_headers())
                .is_clean()
        }));
    }
}
