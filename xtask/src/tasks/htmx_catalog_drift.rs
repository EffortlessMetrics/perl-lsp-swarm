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

/// Extract the backtick-quoted names from one reference table section.
///
/// Fails closed: a missing section or a section that yields no names is an
/// error rather than an empty result, because reporting "no drift" from a
/// document this task could not read is the primary defect risk here.
fn section_names(document: &str, section: &SectionSpec) -> Result<Vec<String>> {
    let (heading_line, depth) = locate_section(document, section)?;

    let mut names = Vec::new();
    let mut in_body = false;
    let mut fence: Option<(char, usize)> = None;

    for line in document.lines().skip(heading_line + 1) {
        let trimmed = line.trim_start();

        // A fenced example can contain pipe-table-shaped lines. Reading those
        // as data would invent names, and a fenced separator row would make the
        // following sample lines look like unreadable data.
        //
        // Track the opening marker rather than toggling a flag: a `~~~` inside a
        // ``` fence would otherwise read as the close, so the example's own
        // lines would be parsed and the real closing fence would re-open,
        // silently swallowing every genuine row after it.
        if let Some(delimiter) = fence_delimiter(trimmed) {
            match fence {
                None => {
                    fence = Some((delimiter.marker, delimiter.length));
                    in_body = false;
                }
                Some((open_marker, open_length)) => {
                    // An info-string line such as ```` ```rust ```` is content
                    // inside an open fence, not its close. Treating it as the
                    // close would parse the sample and let the real closer
                    // re-open the fence over genuine rows.
                    if delimiter.can_close
                        && delimiter.marker == open_marker
                        && delimiter.length >= open_length
                    {
                        fence = None;
                    }
                }
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        // A deeper heading is a subsection, so this section continues; only a
        // heading at the same or shallower level ends it. Breaking at any
        // heading would silently drop every row after an inserted subsection —
        // hiding a real upstream addition behind a clean report, which is the
        // exact failure this task exists to prevent. `heading_depth` also
        // refuses hash-prefixed prose, which would truncate the scan the same
        // way.
        let heading_here = heading_depth(trimmed);
        if heading_here > 0 {
            if heading_here <= depth {
                break;
            }
            in_body = false;
            continue;
        }
        if trimmed.is_empty() {
            // Tables are contiguous, so a blank line ends one. A later table in
            // the same section must present its own separator before its rows
            // count as data.
            in_body = false;
            continue;
        }
        if is_separator_row(trimmed) {
            in_body = true;
            continue;
        }
        if !in_body {
            // The table's own header row, or prose above it.
            continue;
        }
        if !trimmed.contains('|') {
            // A line with no cell separator ends the table even without a blank
            // line before it. Requiring a leading pipe here instead would skip
            // it silently, and Markdown makes that pipe optional.
            in_body = false;
            continue;
        }

        // Every row below the separator is data. Skipping one this task cannot
        // read is the dangerous failure: a newly added upstream entry would
        // disappear into a clean report.
        match table_row_name(trimmed) {
            Some(name) => names.push(name),
            None => bail!(
                "the {} section ({}) has a table row this task cannot read:\n  {}\nA data row \
                 must carry a backtick-quoted name in its first cell; skipping it would let a \
                 new upstream entry vanish into a clean report",
                section.label,
                section.anchor,
                trimmed.trim_end()
            ),
        }
    }

    if fence.is_some() {
        bail!(
            "the {} section ({}) has a code fence that is never closed; everything after it was \
             skipped as sample text, so a later upstream entry could vanish into a clean report",
            section.label,
            section.anchor
        );
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

/// Is this the `|---|---|` rule separating a table's header from its body?
fn is_separator_row(line: &str) -> bool {
    // A cell separator is required, so a thematic break (`---`) does not open a
    // table body and turn the prose after it into unreadable data rows.
    line.contains('-')
        && line.contains('|')
        && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// Find the one heading carrying this section's anchor, with its ATX depth.
///
/// Requires exactly one. Two headings sharing an anchor make the section's
/// identity ambiguous, and taking the first would attribute a decoy's rows to
/// the real section — a wrong report presented with the same confidence as a
/// right one.
fn locate_section(document: &str, section: &SectionSpec) -> Result<(usize, usize)> {
    let mut located: Option<(usize, usize)> = None;

    for (index, line) in document.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.ends_with(section.anchor) {
            continue;
        }
        let depth = heading_depth(trimmed);
        if depth == 0 {
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
        located = Some((index, depth));
    }

    match located {
        Some(located) => Ok(located),
        None => bail!(
            "reference document has no {} section ({}); refusing to report a clean diff against \
             a document this task cannot read",
            section.label,
            section.anchor
        ),
    }
}

/// ATX heading depth, or 0 when the line is not a heading.
///
/// Markdown requires one to six `#` characters followed by whitespace or the
/// end of the line. A bare hash run is not enough: prose such as `#hashtag` or
/// `##css-selector` would otherwise read as a shallow heading and truncate the
/// section, dropping every row below it into a clean report.
fn heading_depth(trimmed_line: &str) -> usize {
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

/// A code-fence delimiter line.
struct FenceDelimiter {
    /// Marker character, `` ` `` or `~`. A fence closes only on its own.
    marker: char,
    /// Length of the marker run. A close is at least as long as its opener.
    length: usize,
    /// Whether this line can close a fence.
    ///
    /// An opening fence may carry an info string (```` ```markdown ````); a
    /// closing fence carries nothing but its run. Without this distinction an
    /// info-string line inside an open fence reads as the close.
    can_close: bool,
}

/// Read a code-fence delimiter line, if this line is one.
fn fence_delimiter(trimmed_line: &str) -> Option<FenceDelimiter> {
    let marker = trimmed_line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let length = trimmed_line.chars().take_while(|character| *character == marker).count();
    if length < 3 {
        return None;
    }

    // Both markers are ASCII, so the run's character count is also its byte
    // offset and this slice cannot split a code point.
    let can_close = match trimmed_line.get(length..) {
        Some(info) => info.trim().is_empty(),
        None => true,
    };

    Some(FenceDelimiter { marker, length, can_close })
}

/// Read the name out of a reference table row.
///
/// Names are backtick-quoted in the first cell, optionally wrapped in a link.
/// Header and separator rows carry no backticks and are skipped.
///
/// Both delimiters are required. Accepting an unterminated backtick would let a
/// malformed row yield the rest of the cell as a "name", which reports as
/// spurious drift instead of being skipped; a row with no closing backtick is
/// not a well-formed name row, and if every row in a section is malformed the
/// caller's zero-names check still fails closed.
fn table_row_name(line: &str) -> Option<String> {
    // Markdown makes the leading pipe optional, so a row may start at its first
    // cell. Requiring it would skip such a row silently.
    let first_cell = line.strip_prefix('|').unwrap_or(line).split('|').next()?;
    let (_, after_open) = first_cell.split_once('`')?;
    let (name, _) = after_open.split_once('`')?;
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

        // Upstream nests both header tables one level under a shared `##`
        // parent. Flattening them here would let the `##` attribute section run
        // straight into the header tables, so the fixture keeps the real depth.
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
    fn extracts_linked_and_unlinked_names_and_skips_table_furniture() {
        let core = section_names(EXCERPT, &CORE_ATTRIBUTES);
        let request = section_names(EXCERPT, &REQUEST_HEADERS);

        assert!(core.is_ok_and(|names| names == ["hx-get", "hx-on*"]));
        assert!(request.is_ok_and(|names| names == ["HX-Boosted", "HX-Trigger"]));
    }

    #[test]
    fn a_data_row_this_task_cannot_read_fails_instead_of_being_skipped() {
        // The dangerous case is one unreadable row among readable ones: a newly
        // added upstream entry would otherwise vanish into a clean report. The
        // unterminated backtick must also not be read as a name, since taking
        // the rest of the cell would report a bogus name as drift.
        let malformed = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| `hx-get | issues a GET to the specified URL |
| [`hx-post`](@/attributes/hx-post.md) | issues a POST |
";

        assert!(
            section_names(malformed, &CORE_ATTRIBUTES)
                .is_err_and(|error| error.to_string().contains("cannot read"))
        );
    }

    #[test]
    fn a_second_table_in_one_section_still_needs_its_own_separator() {
        // A blank line ends a table, so the following table's header row must
        // not be mistaken for data and rejected as unreadable.
        let two_tables = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

| Attribute | Description |
|-----------|-------------|
| [`hx-post`](@/attributes/hx-post.md) | issues a POST |
";

        assert!(
            section_names(two_tables, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );
    }

    #[test]
    fn a_subsection_does_not_truncate_the_section_and_hide_later_rows() {
        // The worst failure this task can have is a silent clean report. An
        // inserted subsection — a deprecation note, a "see also" — must not end
        // the scan, or a genuinely new entry below it disappears and the
        // comparison looks clean. Only a heading at the same or shallower level
        // ends the section.
        let with_subsection = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

### Deprecated Attributes

| Attribute | Description |
|-----------|-------------|
| [`hx-new-thing`](@/attributes/hx-new-thing.md) | a new upstream entry |

## Additional Attribute Reference {#attributes-additional}

| Attribute | Description |
|-----------|-------------|
| [`hx-boost`](@/attributes/hx-boost.md) | not part of the core section |
";

        assert!(
            section_names(with_subsection, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-new-thing"])
        );
    }

    #[test]
    fn a_duplicated_section_anchor_fails_instead_of_taking_the_first() {
        // Two headings sharing an anchor make the section ambiguous. Taking the
        // first would report every real row as removed and the decoy's row as
        // added — a confidently wrong report.
        let decoy = "\
## Deprecated, see below {#request_headers}

| Header | Description |
|--------|-------------|
| `HX-Decoy-Only` | not real |

### Request Headers Reference {#request_headers}

| Header | Description |
|--------|-------------|
| `HX-Request` | set to true on htmx requests |
";

        assert!(
            section_names(decoy, &REQUEST_HEADERS)
                .is_err_and(|error| error.to_string().contains("more than one"))
        );
    }

    #[test]
    fn a_fenced_example_is_not_read_as_table_data() {
        // A fenced sample can show a markdown table. Its rows are illustration,
        // not catalog entries: reading them would invent names, and a fenced
        // separator row would make the sample's own rows look like unreadable
        // data and fail the run.
        let fenced = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

```markdown
| Attribute | Description |
|-----------|-------------|
| `hx-phantom` | only an example |
| not even a name | still an example |
```
";

        assert!(section_names(fenced, &CORE_ATTRIBUTES).is_ok_and(|names| names == ["hx-get"]));
    }

    #[test]
    fn a_fence_closes_only_on_its_own_marker() {
        // A `~~~` inside a ``` fence is example text, not the close. Toggling on
        // either marker would end the fence early, parse the example's own
        // lines, and then treat the real closing fence as an opener — silently
        // swallowing every genuine row after it.
        let mixed = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

```markdown
~~~
| `hx-phantom` | only an example |
~~~
```

| Attribute | Description |
|-----------|-------------|
| [`hx-post`](@/attributes/hx-post.md) | issues a POST |
";

        assert!(
            section_names(mixed, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );
    }

    #[test]
    fn an_unclosed_fence_fails_instead_of_swallowing_the_rest() {
        // An unclosed fence consumes every following line as sample text. With
        // names already collected the vacuity check stays quiet, so a later
        // table would disappear into a clean report.
        let unclosed = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

```markdown
| `hx-phantom` | the fence is never closed |

| Attribute | Description |
|-----------|-------------|
| [`hx-post`](@/attributes/hx-post.md) | swallowed by the open fence |
";

        assert!(
            section_names(unclosed, &CORE_ATTRIBUTES)
                .is_err_and(|error| error.to_string().contains("never closed"))
        );
    }

    #[test]
    fn a_row_without_the_optional_leading_pipe_is_still_read() {
        // Markdown makes the leading pipe optional. Skipping such a row would
        // drop a genuine upstream entry silently; a thematic break must still
        // not be mistaken for a table separator.
        let no_leading_pipe = "\
## Core Attribute Reference {#attributes}

---

Attribute | Description
----------|------------
[`hx-get`](@/attributes/hx-get.md) | issues a GET
[`hx-new-thing`](@/attributes/hx-new-thing.md) | a new upstream entry
";

        assert!(
            section_names(no_leading_pipe, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-new-thing"])
        );
    }

    #[test]
    fn hash_prefixed_prose_does_not_truncate_the_section() {
        // Markdown headings need whitespace after the hash run, so `#hashtag`
        // and `##css-selector` are prose. Reading them as shallow headings
        // would end the scan and drop the table below into a clean report,
        // since the rows already collected keep the vacuity check quiet.
        // `#######` is seven hashes, which is not a heading either.
        let prose = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

Use #hashtag routing, and note that ##css-selector is not a heading.
####### also exceeds the six-level maximum.

| Attribute | Description |
|-----------|-------------|
| [`hx-post`](@/attributes/hx-post.md) | issues a POST |

## Additional Attribute Reference {#attributes-additional}

| Attribute | Description |
|-----------|-------------|
| [`hx-boost`](@/attributes/hx-boost.md) | not part of the core section |
";

        assert!(
            section_names(prose, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );
    }

    #[test]
    fn an_info_string_line_inside_a_fence_does_not_close_it() {
        // A nested ```` ```rust ```` line is content, not the close: only a bare
        // run closes a fence. Treating it as the close would parse the sample
        // and let the real closer re-open the fence over the rows after it.
        let nested = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

````markdown
```rust
| `hx-phantom` | only an example |
```
````

| Attribute | Description |
|-----------|-------------|
| [`hx-post`](@/attributes/hx-post.md) | issues a POST |
";

        assert!(
            section_names(nested, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );
    }

    #[test]
    fn a_header_repeated_within_one_table_keeps_that_table_s_direction() {
        // Merging row by row would let the second response row promote the name
        // to bidirectional, reporting a false direction change against a
        // catalog that is actually correct.
        let repeated = "\
### Request Headers Reference {#request_headers}

| Header | Description |
|--------|-------------|
| `HX-Request` | set to true on htmx requests |

### Response Headers Reference {#response_headers}

| Header | Description |
|--------|-------------|
| [`HX-Redirect`](@/headers/hx-redirect.md) | client-side redirect |
| [`HX-Redirect`](@/headers/hx-redirect.md) | listed twice upstream |
";

        assert!(reference_headers(repeated).is_ok_and(|headers| {
            headers.get("HX-Redirect") == Some(&HtmxHeaderDirection::Response)
                && headers.get("HX-Request") == Some(&HtmxHeaderDirection::Request)
        }));
    }

    #[test]
    fn a_missing_section_fails_instead_of_reporting_no_drift() {
        let without_core = EXCERPT.replace("{#attributes}", "{#renamed-anchor}");
        let error = section_names(&without_core, &CORE_ATTRIBUTES);

        assert!(error.is_err_and(|error| error.to_string().contains("core attributes")));
    }

    #[test]
    fn a_section_that_lost_its_table_fails_instead_of_reporting_no_drift() {
        // The section is present but carries no table at all — what an upstream
        // restructure into prose or per-page documentation would look like.
        // Zero names must be an error, never a clean comparison.
        let prose_only = "\
## Core Attribute Reference {#attributes}

The core attributes are now documented on their own pages.

## Additional Attribute Reference {#attributes-additional}
";

        assert!(
            section_names(prose_only, &CORE_ATTRIBUTES)
                .is_err_and(|error| error.to_string().contains("vacuous"))
        );
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

    /// Round-trip only: this renders the catalog into the reference shape and
    /// re-extracts it, so it proves the renderer and extractor agree on
    /// well-formed rows and that the event-handler transcription reverses
    /// cleanly. It reads no upstream document, so it cannot prove the catalog
    /// matches real htmx — that is established by running the command against
    /// the reviewed document recorded in `HTMX_CATALOG_PROVENANCE`.
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
