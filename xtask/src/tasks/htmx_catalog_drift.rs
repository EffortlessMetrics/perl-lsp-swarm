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
use std::borrow::Cow;
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
    let mut inert = InertScanner::new();

    for line in document.lines().skip(heading_line + 1) {
        // An HTML comment inside the section can hold a line shaped like a
        // same-or-shallower heading. Markdown makes that line inert, so on the
        // specification's reading the section continues and the rows after the
        // comment are this section's. But if the comment was commenting out the
        // next section heading, those rows belong to the following section.
        //
        // Nothing in the document distinguishes an editorial `## TODO` note
        // from a commented-out section heading, and each guess has been
        // observed to drop a real upstream entry: reading on absorbs the
        // following section's table, and stopping truncates the section after
        // an ordinary note. Refuse rather than pick.
        if inert.commented_heading_depth(line).is_some_and(|heading_here| heading_here <= depth) {
            bail!(
                "the {} section ({}) has an HTML comment holding a line shaped like a \
                 same-or-shallower heading:\n  {}\nWhether the section continues past that \
                 comment or ends at that line cannot be decided from the document, and either \
                 reading has been observed to drop a real upstream entry",
                section.label,
                section.anchor,
                line.trim()
            );
        }

        // A fenced example or a commented-out table can contain pipe-table
        // shaped lines. Reading those as data would invent names, and their
        // separator rows would make the following sample lines look like
        // unreadable data.
        //
        // Illustration and a blank line both end any table in progress. Tables
        // are contiguous, so a later table in the same section must present its
        // own separator before its rows count as data.
        let Some(content) = inert.content(line) else {
            in_body = false;
            continue;
        };
        let trimmed = content.as_ref();
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

    if let Some(region) = inert.unterminated() {
        bail!(
            "the {} section ({}) has {} that is never closed; everything after it was skipped \
             as illustration, so a later upstream entry could vanish into a clean report",
            section.label,
            section.anchor,
            region
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
    let mut inert = InertScanner::new();

    for (index, line) in document.lines().enumerate() {
        // A fenced example that displays a heading is sample text, not a
        // section. Matching it would start the scan inside the fence with no
        // fence open, so the example's own rows would be read as authoritative
        // and the fence's real closer would be misread as an opener. With names
        // already collected the vacuity guard cannot fire, so a document whose
        // real section is gone reports the sample's names instead — the silent
        // wrong read this task exists to prevent. It also makes an ordinary
        // documentation example collide with the real heading and fail as
        // ambiguous, which refuses a document that is perfectly readable.
        let Some(content) = inert.content(line) else {
            continue;
        };
        let trimmed = content.trim();
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

/// A region of the document whose text is illustration rather than content.
enum InertRegion {
    /// A fenced code block, with its opening marker and run length.
    ///
    /// The opening marker is retained rather than a bare flag: a `~~~` inside a
    /// ``` fence would otherwise read as the close, so the example's own lines
    /// would be parsed and the real closing fence would re-open, silently
    /// swallowing every genuine row after it.
    Fence { marker: char, length: usize },
    /// An HTML comment.
    Comment,
}

/// Inert-region state for one pass over the document.
///
/// The section locator and the row scan have to agree on exactly which text is
/// illustration. Tracking that separately in each is how three of this task's
/// earlier silent defects arose, so the rule lives here once.
struct InertScanner {
    inside: Option<InertRegion>,
}

impl InertScanner {
    fn new() -> Self {
        Self { inside: None }
    }

    /// The ATX depth of a heading-shaped line currently inside an HTML
    /// comment, if this line is one.
    ///
    /// Used only by an already-located section scan, which refuses rather than
    /// deciding whether such a line ends the section; see [`section_names`].
    /// Section discovery must keep ignoring headings inside comments.
    fn commented_heading_depth(&self, line: &str) -> Option<usize> {
        matches!(self.inside, Some(InertRegion::Comment))
            .then(|| heading_depth(line.trim_start()))
            .filter(|depth| *depth > 0)
    }

    /// Advance over one raw line and return the document content in it.
    ///
    /// `None` for a fence delimiter, for everything inside a fence, for an
    /// indented code block, and for a line whose text is entirely commented
    /// out. Otherwise the line, trimmed and with its commented spans removed,
    /// which is the text the caller must read: a row carrying a trailing
    /// comment is still a row, and skipping the whole line would drop a name
    /// silently — the failure this task exists to prevent — while reading the
    /// commented span would invent one.
    fn content<'line>(&mut self, line: &'line str) -> Option<Cow<'line, str>> {
        let trimmed_line = line.trim_start();

        match self.inside {
            // Only the fence's own close ends it. An info-string line such as
            // ```` ```rust ```` is content inside an open fence: treating it as
            // the close would parse the sample and let the real closer re-open
            // the fence over genuine rows. A comment delimiter here is sample
            // text, so comment state is deliberately not advanced.
            Some(InertRegion::Fence { marker, length }) => {
                // A closing fence carries at most three columns of indent; at
                // four it is content inside the block. Closing on it ends the
                // fence early, so the example's own rows read as document text
                // and the real closer re-opens the fence over the genuine rows
                // after it — the same silent swallow an unmatched marker used
                // to cause, reached through indentation instead.
                if !indented_code_columns(line)
                    && fence_delimiter(trimmed_line).is_some_and(|delimiter| {
                        delimiter.can_close
                            && delimiter.marker == marker
                            && delimiter.length >= length
                    })
                {
                    self.inside = None;
                }
                None
            }
            // A fence marker inside a comment is commented-out text and must
            // not open a fence; only the comment's own close ends the region.
            Some(InertRegion::Comment) => self.visible_content(trimmed_line),
            // An indented code block is sample text with no delimiter to look
            // for. Reading it as document structure lets an indented heading
            // end the section early, dropping every row below it into a clean
            // report, and lets one stand in for a section that is gone.
            None if indented_code_columns(line) => None,
            None => match fence_delimiter(trimmed_line) {
                Some(delimiter) => {
                    self.inside = Some(InertRegion::Fence {
                        marker: delimiter.marker,
                        length: delimiter.length,
                    });
                    None
                }
                None => self.visible_content(trimmed_line),
            },
        }
    }

    /// This line's text with commented spans removed, if any remains.
    fn visible_content<'line>(&mut self, trimmed_line: &'line str) -> Option<Cow<'line, str>> {
        let visible = self.strip_commented_spans(trimmed_line);
        (!visible.trim().is_empty()).then_some(visible)
    }

    /// Remove this line's commented spans, advancing the comment state.
    ///
    /// A `<!--` inside an inline code span is an example of the delimiter, not
    /// the delimiter: opening a comment on it skips every row until a later
    /// example of `-->`, so a real addition between the two disappears into a
    /// clean report. Openers inside a code span are therefore ignored.
    ///
    /// Markdown is not parsed inside an HTML comment, so once a comment is open
    /// its `-->` closes it whatever backticks surround it, and code spans are
    /// consulted only while outside one.
    ///
    /// Both delimiters and the backtick are ASCII, so every byte offset here is
    /// a character boundary and the slicing cannot split a code point.
    fn strip_commented_spans<'line>(&mut self, line: &'line str) -> Cow<'line, str> {
        if !matches!(self.inside, Some(InertRegion::Comment)) && !line.contains("<!--") {
            return Cow::Borrowed(line);
        }

        let spans = code_span_ranges(line);
        let mut visible = String::new();
        let mut cursor = 0;
        loop {
            match self.inside {
                Some(InertRegion::Comment) => match line[cursor..].find("-->") {
                    Some(offset) => {
                        self.inside = None;
                        cursor += offset + "-->".len();
                    }
                    None => return Cow::Owned(visible),
                },
                None => match next_comment_opener(line, cursor, &spans) {
                    Some(at) => {
                        visible.push_str(&line[cursor..at]);
                        self.inside = Some(InertRegion::Comment);
                        cursor = at + "<!--".len();
                    }
                    None => {
                        visible.push_str(&line[cursor..]);
                        return Cow::Owned(visible);
                    }
                },
                Some(InertRegion::Fence { .. }) => return Cow::Owned(visible),
            }
        }
    }

    /// The name of the region still open at the end of the scan, if any.
    ///
    /// Everything after an unterminated region was skipped as illustration, so
    /// the caller must refuse rather than report on a partial read.
    fn unterminated(&self) -> Option<&'static str> {
        match self.inside {
            None => None,
            Some(InertRegion::Fence { .. }) => Some("a code fence"),
            Some(InertRegion::Comment) => Some("an HTML comment"),
        }
    }
}

/// Byte ranges of this line that sit inside an inline code span.
///
/// A code span is delimited by matching backtick runs of equal length; a run
/// with no match is literal text rather than an opener. Comment delimiters
/// inside a span are examples of markup, not markup, so the comment scan skips
/// these ranges.
///
/// Scoped to one line. A code span may in principle continue across a line
/// break, which this does not model. An over-broad range makes a real comment
/// opener look like an example, so the rows inside it read as data and produce
/// a false *addition* — noise a maintainer sees. An under-broad one is the
/// silent direction, and only an unmatched run produces it, which is correctly
/// literal text anyway.
fn code_span_ranges(line: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'`' {
            index += 1;
            continue;
        }

        let opener = index;
        while index < bytes.len() && bytes[index] == b'`' {
            index += 1;
        }
        let run = index - opener;

        let mut search = index;
        let mut closed = None;
        while search < bytes.len() {
            if bytes[search] != b'`' {
                search += 1;
                continue;
            }
            let candidate = search;
            while search < bytes.len() && bytes[search] == b'`' {
                search += 1;
            }
            if search - candidate == run {
                closed = Some(search);
                break;
            }
        }

        // An unmatched run is literal, so the scan resumes just after it.
        if let Some(end) = closed {
            ranges.push((opener, end));
            index = end;
        }
    }

    ranges
}

/// The next real `<!--` at or after `cursor`, skipping examples in code spans.
fn next_comment_opener(line: &str, cursor: usize, spans: &[(usize, usize)]) -> Option<usize> {
    let mut search = cursor;
    loop {
        let at = search + line[search..].find("<!--")?;
        if spans.iter().any(|(start, end)| at >= *start && at < *end) {
            search = at + "<!--".len();
            continue;
        }
        return Some(at);
    }
}

/// Is this line indented far enough to be an indented code block?
///
/// Markdown allows a block construct up to three leading columns; at four it is
/// a code block instead, so a heading, fence or table row indented that far is
/// sample text. A tab advances to the next multiple of four, as Markdown counts
/// it. A whitespace-only line is not code — it is the blank line that ends a
/// table, and treating it as code would be the same skip by another name.
///
/// This is the one place the scan trusts the specification rather than refusing
/// what it cannot account for: a data row indented four columns is code, not a
/// row, so skipping it is correct rather than a silent drop. The reviewed
/// reference document has no indented line at all, and
/// `a_row_indented_less_than_a_code_block_is_still_read` pins the safe side of
/// the boundary.
fn indented_code_columns(line: &str) -> bool {
    // A whitespace-only line is the blank line that ends a table, not code.
    if line.trim().is_empty() {
        return false;
    }

    let mut columns = 0;
    for character in line.chars() {
        match character {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            _ => break,
        }
        if columns >= 4 {
            return true;
        }
    }

    false
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
    let info = trimmed_line.get(length..).unwrap_or("");

    // Markdown forbids a backtick in a backtick fence's info string, so such a
    // line is ordinary text carrying inline code, not a fence. Opening a fence
    // on it skips every row until the next bare run — a real upstream addition
    // in between would vanish into a clean report. A tilde fence has no such
    // restriction, so its info string may contain whatever it likes.
    if marker == '`' && info.contains('`') {
        return None;
    }

    Some(FenceDelimiter { marker, length, can_close: info.trim().is_empty() })
}

/// Read the name out of a reference table row.
///
/// The whole first cell is validated, not just the first name in it. Upstream
/// writes exactly one of two shapes:
///
/// ```text
/// `HX-Boosted`
/// [`hx-get`](@/attributes/hx-get.md)
/// ```
///
/// Anything else — an unterminated backtick, a second quoted token, trailing
/// text outside the link — is a cell this task cannot fully account for, and it
/// returns `None` so the caller fails closed. Reading the first token and
/// discarding the rest would drop a name silently, which is the one outcome
/// this whole task exists to prevent.
///
/// Header and separator rows carry no backticks and so are skipped, which is
/// what the caller wants above a table separator.
fn table_row_name(line: &str) -> Option<String> {
    // Markdown makes the leading pipe optional, so a row may start at its first
    // cell. Requiring it would skip such a row silently.
    let first_cell = line.strip_prefix('|').unwrap_or(line).split('|').next()?;

    let (before, rest) = first_cell.split_once('`')?;
    let (name, after) = rest.split_once('`')?;
    if name.is_empty() {
        return None;
    }

    let accounted_for = match before.trim() {
        // `name`
        "" => after.trim().is_empty(),
        // [`name`](destination)
        "[" => closes_a_link(after.trim()),
        _ => false,
    };

    accounted_for.then(|| name.to_string())
}

/// Is `tail` exactly the `](destination)` that closes a linked name?
///
/// The check has to prove the final parenthesis is the link's own. Accepting any
/// tail that merely starts `](` and ends `)` reads
/// `` [`hx-get`](@/attributes/hx-get.md) trailing) `` as the bare name `hx-get`
/// and discards the rest of the cell — the same silent partial read
/// [`table_row_name`] exists to prevent, one level down.
///
/// Upstream destinations are single unbroken tokens (`@/attributes/hx-get.md`),
/// so requiring the destination to carry no whitespace, parenthesis or backtick
/// makes that final parenthesis unambiguous. A destination this rule cannot
/// account for — a link title, an angle-bracketed target, a target with balanced
/// parentheses — fails closed rather than being guessed at, because guessing is
/// what produces a clean report against a document that changed.
fn closes_a_link(tail: &str) -> bool {
    let Some(destination) = tail.strip_prefix("](").and_then(|rest| rest.strip_suffix(')')) else {
        return false;
    };

    !destination.is_empty()
        && !destination.contains(['`', '(', ')'])
        && !destination.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::{
        CORE_ATTRIBUTES, DirectionChange, DriftReport, REQUEST_HEADERS, catalog_attributes,
        catalog_headers, code_span_ranges, compare_snapshot, indented_code_columns,
        reference_attributes, reference_headers, section_names, table_row_name,
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
    fn a_first_cell_this_task_cannot_fully_account_for_fails_closed() {
        // Reading the first quoted token and discarding the rest of the cell is
        // a partial read: the discarded name never reaches the comparison, so a
        // changed reference row could report clean. Every one of these rows must
        // fail rather than yield its first token.
        for malformed in [
            "| [`hx-get`](@/attributes/hx-get.md) `hx-new` | described |",
            "| `hx-get` `hx-new` | described |",
            "| `hx-get` trailing prose | described |",
            "| [`hx-get`]@/attributes/hx-get.md | described |",
            "| prefix [`hx-get`](@/attributes/hx-get.md) | described |",
            // Trailing text that itself ends in a closing parenthesis: the
            // final `)` is the trailing text's, not the link's, so a check that
            // only looks at the two ends accepts this and drops the suffix.
            "| [`hx-get`](@/attributes/hx-get.md) trailing) | described |",
            "| [`hx-get`](@/attributes/hx-get.md) `hx-new`) | described |",
            "| [`hx-get`]() | described |",
            "| [`hx-get`](@/attributes/hx-get.md \"a title\") | described |",
        ] {
            let section = format!(
                "## Core Attribute Reference {{#attributes}}\n\n\
                 | Attribute | Description |\n|-----------|-------------|\n{malformed}\n"
            );
            assert!(
                section_names(&section, &CORE_ATTRIBUTES)
                    .is_err_and(|error| error.to_string().contains("cannot read")),
                "{malformed} must fail closed rather than yield a partial read"
            );
        }

        // The two shapes upstream actually writes must still be accepted, or the
        // rule is merely stricter rather than correct.
        assert_eq!(
            table_row_name("| `HX-Boosted` | indicates a boosted request"),
            Some("HX-Boosted".to_string())
        );
        assert_eq!(
            table_row_name("| [`hx-get`](@/attributes/hx-get.md)  | issues a `GET` |"),
            Some("hx-get".to_string())
        );
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
    fn a_heading_inside_a_fence_does_not_become_the_section() {
        // The silent case. A fenced example that displays the section heading is
        // sample text. Locating it starts the scan inside the fence with no
        // fence open, so the sample's rows read as authoritative and the real
        // closer reads as an opener; a later well-formed fence then restores the
        // parity the unclosed-fence guard relies on, so nothing fails and the
        // sample's names are reported as the document's own. The real section is
        // absent here, so the only honest answer is to refuse.
        let fenced_only = "\
# Reference

```text
## Core Attribute Reference {#attributes}
| Attribute | Description |
|-----------|-------------|
| [`hx-phantom`](@/attributes/hx-phantom.md) | only an example |
```

## Something Else {#other}

```rust
let example = 1;
```
";

        let error = section_names(fenced_only, &CORE_ATTRIBUTES)
            .expect_err("a heading shown inside a fence must not be read as the section");
        assert!(
            error.to_string().contains("has no core attributes section"),
            "the refusal must name the missing section, not a downstream symptom: {error}"
        );
    }

    #[test]
    fn a_comment_delimiter_inside_a_code_span_is_an_example() {
        // Prose documenting the delimiter is not the delimiter. Opening a
        // comment on `<!--` skips every row until a later example of `-->`, so
        // `hx-post` — a genuine addition between the two — disappears into a
        // clean report. This surface exists only because comment tracking was
        // added at all; before it, delimiters were ignored entirely.
        let documented = concat!(
            "## Core Attribute Reference {#attributes}\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-get`](@/attributes/hx-get.md) | issues a GET |\n",
            "\n",
            "Write `<!--` to open an HTML comment.\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-post`](@/attributes/hx-post.md) | added upstream |\n",
            "\n",
            "Write `-->` to close one.\n",
        );

        assert!(
            section_names(documented, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );

        // A real comment must still hide its rows, or the fix has simply
        // disabled comment tracking rather than corrected it.
        let real = concat!(
            "## Core Attribute Reference {#attributes}\n",
            "\n",
            "<!--\n",
            "| `hx-retired` | removed upstream |\n",
            "-->\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-get`](@/attributes/hx-get.md) | issues a GET |\n",
        );

        assert!(section_names(real, &CORE_ATTRIBUTES).is_ok_and(|names| names == ["hx-get"]));

        // Matching runs only: an unmatched backtick run is literal text, so a
        // delimiter after it is real and must still open a comment.
        assert_eq!(code_span_ranges("a `b` c"), vec![(2, 5)]);
        assert_eq!(code_span_ranges("a ` b c"), Vec::new());
        assert_eq!(code_span_ranges("``a `b` c``"), vec![(0, 11)]);
    }

    #[test]
    fn an_indented_delimiter_does_not_close_an_open_fence() {
        // A closing fence carries at most three columns of indent; at four it
        // is content inside the block. Closing on it ends the fence early, the
        // example's rows read as document text, and the real closer re-opens
        // the fence over the genuine rows after it. The trailing well-formed
        // fence restores the parity the unclosed-fence guard relies on, so
        // nothing fails and `hx-post` simply disappears.
        let indented_closer = concat!(
            "## Core Attribute Reference {#attributes}\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-get`](@/attributes/hx-get.md) | issues a GET |\n",
            "\n",
            "```text\n",
            "    ```\n",
            "| [`hx-fake`](@/attributes/hx-fake.md) | sample |\n",
            "```\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-post`](@/attributes/hx-post.md) | added upstream |\n",
            "\n",
            "```rust\n",
            "let x = 1;\n",
            "```\n",
        );

        assert!(
            section_names(indented_closer, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );

        // Up to three columns is a legitimate closer, so it must still close —
        // refusing it would leave the fence open and swallow every row after.
        let shallow_closer = concat!(
            "## Core Attribute Reference {#attributes}\n",
            "\n",
            "```text\n",
            "| `hx-phantom` | only an example |\n",
            "   ```\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-get`](@/attributes/hx-get.md) | issues a GET |\n",
        );

        assert!(
            section_names(shallow_closer, &CORE_ATTRIBUTES).is_ok_and(|names| names == ["hx-get"])
        );
    }

    #[test]
    fn a_backtick_fence_with_a_backtick_in_its_info_string_is_not_a_fence() {
        // Markdown forbids a backtick in a backtick fence's info string, so
        // this line is prose carrying inline code. Opening a fence on it skips
        // every row until the next bare run, and the addition in between —
        // `hx-post` here — vanishes into a clean report.
        let invalid = concat!(
            "## Core Attribute Reference {#attributes}\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-get`](@/attributes/hx-get.md) | issues a GET |\n",
            "\n",
            "``` see `hx-get` for details\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-post`](@/attributes/hx-post.md) | added upstream |\n",
            "\n",
            "```html\n",
            "<div hx-get=\"/x\"></div>\n",
            "```\n",
        );

        assert!(
            section_names(invalid, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );

        // A tilde fence carries no such restriction, so a backtick in its info
        // string must still open one — rejecting it would read the example's
        // own rows as data and invent names.
        let tilde = concat!(
            "## Core Attribute Reference {#attributes}\n",
            "\n",
            "| Attribute | Description |\n",
            "|-----------|-------------|\n",
            "| [`hx-get`](@/attributes/hx-get.md) | issues a GET |\n",
            "\n",
            "~~~ see `hx-get` for details\n",
            "| `hx-phantom` | only an example |\n",
            "~~~\n",
        );

        assert!(section_names(tilde, &CORE_ATTRIBUTES).is_ok_and(|names| names == ["hx-get"]));
    }

    #[test]
    fn an_indented_code_heading_does_not_truncate_the_section() {
        // Four columns of indent make a code block, so the heading inside it is
        // sample text. Reading it as a real heading ends the section early and
        // drops every row below — here a genuine upstream addition — into a
        // clean report.
        let indented = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

    ## Not really a heading {#sample}

| Attribute | Description |
|-----------|-------------|
| [`hx-post`](@/attributes/hx-post.md) | added upstream |
";

        assert!(
            section_names(indented, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );
    }

    #[test]
    fn an_indented_code_heading_does_not_become_the_section() {
        // The locator half: a section shown inside an indented example must not
        // stand in for one the document no longer has.
        let indented = "\
# Reference

    ## Core Attribute Reference {#attributes}
    | Attribute | Description |
    |-----------|-------------|
    | [`hx-phantom`](@/attributes/hx-phantom.md) | sample |

## Something Else {#other}
";

        let error = section_names(indented, &CORE_ATTRIBUTES)
            .expect_err("a heading inside an indented code block is not the section");
        assert!(
            error.to_string().contains("has no core attributes section"),
            "the refusal must name the missing section: {error}"
        );
    }

    #[test]
    fn a_row_indented_less_than_a_code_block_is_still_read() {
        // The safe side of the boundary. Markdown allows a block up to three
        // leading columns; treating those as code would skip a real row
        // silently, which is worse than the defect above. A tab reaches four
        // columns on its own and is code.
        let shallow = "\
## Core Attribute Reference {#attributes}

   | Attribute | Description |
   |-----------|-------------|
   | [`hx-get`](@/attributes/hx-get.md) | three columns is still a row |
";

        assert!(section_names(shallow, &CORE_ATTRIBUTES).is_ok_and(|names| names == ["hx-get"]));
        assert!(!indented_code_columns("   | still a row |"));
        assert!(indented_code_columns("    | code |"));
        assert!(indented_code_columns("\t| code |"));
        assert!(!indented_code_columns("  \t"), "a whitespace-only line is a blank line, not code");
    }

    #[test]
    fn a_commented_out_table_does_not_contribute_names() {
        // An obsolete table kept in an HTML comment beside its replacement. Its
        // rows are not upstream any more, so reading them keeps a name alive in
        // the comparison that upstream has removed — the catalog still carries
        // `hx-retired`, the removal is never reported, and the run says clean.
        let commented = "\
## Core Attribute Reference {#attributes}

<!--
| Attribute | Description |
|-----------|-------------|
| [`hx-retired`](@/attributes/hx-retired.md) | removed upstream |
-->

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |
";

        assert!(section_names(commented, &CORE_ATTRIBUTES).is_ok_and(|names| names == ["hx-get"]));
    }

    #[test]
    fn a_row_carrying_a_trailing_comment_is_still_read() {
        // The opposite error. Discarding a whole line because it opens a
        // comment would drop the row's name silently, which is worse than
        // reading the commented span. The row's own text is still content.
        let annotated = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET | <!-- keep this row -->
| [`hx-post`](@/attributes/hx-post.md) | issues a POST |
";

        assert!(
            section_names(annotated, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );
    }

    #[test]
    fn a_commented_out_heading_does_not_become_the_section() {
        // The same defect as the fenced heading, through a different syntax: a
        // section commented out upstream must not still be selectable, or its
        // stale rows report as current.
        let commented = "\
# Reference

<!--
## Core Attribute Reference {#attributes}
| Attribute | Description |
|-----------|-------------|
| [`hx-phantom`](@/attributes/hx-phantom.md) | commented out |
-->

## Something Else {#other}
";

        let error = section_names(commented, &CORE_ATTRIBUTES)
            .expect_err("a heading inside an HTML comment must not be read as the section");
        assert!(
            error.to_string().contains("has no core attributes section"),
            "the refusal must name the missing section: {error}"
        );
    }

    #[test]
    fn an_unclosed_comment_fails_instead_of_swallowing_the_rest() {
        // Mirrors the unclosed-fence guard: everything after an unterminated
        // comment was skipped, so a later table would vanish into a clean
        // report exactly as it would behind an open fence.
        let unclosed = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

<!-- the comment is never closed

| Attribute | Description |
|-----------|-------------|
| [`hx-post`](@/attributes/hx-post.md) | swallowed by the open comment |
";

        assert!(
            section_names(unclosed, &CORE_ATTRIBUTES).is_err_and(|error| error
                .to_string()
                .contains("HTML comment that is never closed"))
        );
    }

    #[test]
    fn a_comment_holding_a_section_heading_is_refused_rather_than_guessed() {
        // Fixture from `11daa212`, which fixed the leak this describes by
        // ending the scan at the commented heading. That reading drops a real
        // addition after an ordinary `## TODO` note (below), and the opposite
        // reading absorbs the following section's table. Neither is decidable
        // from the document, so the scan refuses.
        let spans_next_section = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

<!-- editorial note
## Following Attribute Reference {#following}
This heading is inside the note.
-->

| [`hx-post`](@/attributes/hx-post.md) | belongs to the following section |
";

        let error = section_names(spans_next_section, &CORE_ATTRIBUTES)
            .expect_err("a comment holding a section heading is not decidable");
        assert!(
            error.to_string().contains("cannot be decided from the document"),
            "the refusal must say why it refused: {error}"
        );
        // The rows it would otherwise have absorbed must not appear anywhere in
        // the message-bearing result: refusing is not a quiet partial read.
        assert!(!error.to_string().contains("hx-post"), "{error}");

        // The other reading's cost: an editorial note that merely starts with
        // `##` ends no section, and truncating there hides the addition after
        // it. This is what `11daa212` did, and it is silent.
        let ordinary_note = "\
## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |

<!-- editorial note
## TODO revisit the wording here
-->

| Attribute | Description |
|-----------|-------------|
| [`hx-brandnew`](@/attributes/hx-brandnew.md) | added upstream |
";

        assert!(
            section_names(ordinary_note, &CORE_ATTRIBUTES).is_err(),
            "an ordinary note must not silently truncate the section"
        );
    }

    #[test]
    fn a_comment_without_a_heading_shaped_line_is_still_read_normally() {
        // The refusal above must stay narrow. A comment carrying no
        // same-or-shallower heading is ordinary illustration: its rows are
        // skipped and the section continues, including a deeper heading, which
        // is a subsection marker rather than a boundary.
        let deeper_heading_note = "\
## Core Attribute Reference {#attributes}

<!-- editorial note
### a deeper heading is not this section's boundary
| `hx-retired` | removed upstream |
-->

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |
| [`hx-post`](@/attributes/hx-post.md) | issues a POST |
";

        assert!(
            section_names(deeper_heading_note, &CORE_ATTRIBUTES)
                .is_ok_and(|names| names == ["hx-get", "hx-post"])
        );
    }

    #[test]
    fn a_fenced_heading_does_not_make_the_real_section_ambiguous() {
        // The false-failure half. Upstream documenting its own heading shape in
        // an example must not collide with the real heading: refusing a document
        // this task can in fact read is its own defect.
        let both = "\
# Reference

```text
## Core Attribute Reference {#attributes}
| Attribute | Description |
|-----------|-------------|
| [`hx-phantom`](@/attributes/hx-phantom.md) | only an example |
```

## Core Attribute Reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-get`](@/attributes/hx-get.md) | issues a GET |
";

        assert!(section_names(both, &CORE_ATTRIBUTES).is_ok_and(|names| names == ["hx-get"]));
    }

    #[test]
    fn an_anchored_line_in_ordinary_prose_is_still_not_a_heading() {
        // Fence awareness must not smuggle in a second way to accept a
        // non-heading: a line merely ending in the anchor is prose, and reading
        // it as the section would start the scan at the wrong place.
        let prose = "\
# Reference

See the core attribute reference {#attributes}

| Attribute | Description |
|-----------|-------------|
| [`hx-phantom`](@/attributes/hx-phantom.md) | not under a heading |
";

        let error = section_names(prose, &CORE_ATTRIBUTES)
            .expect_err("prose ending in the anchor is not a heading");
        assert!(
            error.to_string().contains("has no core attributes section"),
            "the refusal must name the missing section: {error}"
        );
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
