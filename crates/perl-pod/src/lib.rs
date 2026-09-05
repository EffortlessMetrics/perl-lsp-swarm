//! POD documentation extractor for Perl `.pm` files.
//!
//! Parses POD (Plain Old Documentation) sections from Perl source files and
//! returns structured documentation suitable for hover display in an LSP.

#![deny(unsafe_code)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use std::collections::HashMap;
use std::io;
use std::path::Path;

/// Extracted POD documentation from a Perl module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PodDoc {
    /// Module name and optional one-line description from `=head1 NAME`.
    pub name: Option<String>,
    /// Usage example from `=head1 SYNOPSIS`.
    pub synopsis: Option<String>,
    /// First paragraph of `=head1 DESCRIPTION`.
    pub description: Option<String>,
    /// Method/function docs keyed by name, from `=head2 method_name`.
    pub methods: HashMap<String, String>,
    /// Parameters from `=head1 ARGUMENTS`.
    pub arguments: Option<String>,
    /// Return value documentation from `=head1 RETURN VALUES`.
    pub return_values: Option<String>,
    /// Usage examples from `=head1 EXAMPLES`.
    pub examples: Option<String>,
    /// Related modules from `=head1 SEE ALSO`.
    pub see_also: Option<String>,
}

impl PodDoc {
    /// Returns `true` if no documentation was extracted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.synopsis.is_none()
            && self.description.is_none()
            && self.methods.is_empty()
            && self.arguments.is_none()
            && self.return_values.is_none()
            && self.examples.is_none()
            && self.see_also.is_none()
    }
}

/// Read a file and extract its POD documentation.
///
/// # Errors
///
/// Returns an I/O error if the file cannot be read.
pub fn extract_pod_from_file(path: &Path) -> io::Result<PodDoc> {
    let content = std::fs::read_to_string(path)?;
    Ok(extract_pod(&content))
}

/// Extract POD documentation from a string of Perl source code.
///
/// Parses POD markup from the source string and extracts structured documentation
/// for the NAME, SYNOPSIS, DESCRIPTION sections, and method documentation (head2).
///
/// # Arguments
///
/// * `source` - Perl source code containing POD documentation
///
/// # Returns
///
/// A `PodDoc` containing the extracted documentation fields. Empty fields indicate
/// the corresponding POD section was not present in the source.
#[must_use]
pub fn extract_pod(source: &str) -> PodDoc {
    let mut doc = PodDoc::default();
    let mut current_section: Option<Section> = None;
    let mut body = String::new();
    // Whether body holds content accumulated before any `=head` was seen
    // (=over/=item lists land here) — such a flush must retain the content
    // under a synthetic Synopsis rather than dropping it (#2488).
    let mut leading_list = false;
    // Any `=head` seen yet — distinguishes genuinely leading content from
    // content under an unsupported heading (whose current_section is also
    // None but which must never synthesize or clobber a Synopsis).
    let mut saw_any_head = false;
    let mut in_pod = false;
    let mut in_over = false;

    for line in source.lines() {
        // Detect POD start directives. Use exact command-word matching to avoid
        // false positives like `=cutlery` matching `=cut` or `=headache`
        // matching `=head` (narrow command-map slice: #13575; broader POD
        // parser authority: #4971).
        if pod_command(line).is_some() {
            in_pod = true;
        }

        if !in_pod {
            continue;
        }

        // =cut ends POD
        if matches!(pod_command(line), Some("cut")) {
            flush_section(&mut doc, &current_section, &body, leading_list || in_over);
            current_section = None;
            body.clear();
            in_pod = false;
            in_over = false;
            continue;
        }

        // =over / =item / =back for lists
        if matches!(pod_command(line), Some("over")) {
            in_over = true;
            body.push('\n');
            continue;
        }
        if matches!(pod_command(line), Some("back")) {
            in_over = false;
            body.push('\n');
            continue;
        }
        if matches!(pod_command(line), Some("item")) {
            let item_text = pod_command_arg(line, "=item");
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str("- ");
            body.push_str(&strip_pod_formatting(item_text));
            body.push('\n');
            continue;
        }

        // New head1 section
        if matches!(pod_command(line), Some("head1")) {
            let heading = pod_command_arg(line, "=head1").trim();
            flush_section(&mut doc, &current_section, &body, leading_list);
            body.clear();
            saw_any_head = true;
            leading_list = false;
            if let Some(section) = match heading {
                "NAME" => Some(Section::Name),
                "SYNOPSIS" => Some(Section::Synopsis),
                "DESCRIPTION" => Some(Section::Description),
                "ARGUMENTS" => Some(Section::Arguments),
                "RETURN VALUES" => Some(Section::ReturnValues),
                "EXAMPLES" => Some(Section::Examples),
                "SEE ALSO" => Some(Section::SeeAlso),
                _ => None,
            } {
                current_section = Some(section);
            } else {
                current_section = None;
            }
            continue;
        }

        // New head2 section — treated as method documentation
        if matches!(pod_command(line), Some("head2")) {
            let heading = strip_pod_formatting(pod_command_arg(line, "=head2").trim());
            flush_section(&mut doc, &current_section, &body, leading_list);
            body.clear();
            saw_any_head = true;
            leading_list = false;
            current_section = Some(Section::Method(heading));
            continue;
        }

        // =head3–=head6: flush the current section and treat as sub-section
        // boundaries (no dedicated Section variant, but must not fall through
        // to body accumulation).
        if matches!(
            pod_command(line),
            Some("head3") | Some("head4") | Some("head5") | Some("head6")
        ) {
            flush_section(&mut doc, &current_section, &body, leading_list);
            current_section = None;
            body.clear();
            saw_any_head = true;
            leading_list = false;
            continue;
        }

        // Skip other directives
        if matches!(
            pod_command(line),
            Some("pod") | Some("encoding") | Some("begin") | Some("end") | Some("for")
        ) {
            continue;
        }

        // Accumulate body text — also capture =over/=item content even before
        // any =head section exists (#2488: these form an implicit "SYNOPSIS" or
        // "DESCRIPTION" block in many CPAN modules).
        if (!body.is_empty() || !line.is_empty()) && (current_section.is_some() || in_over) {
            if current_section.is_none() && !saw_any_head {
                leading_list = true;
            }
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line);
        }
    }

    // Flush any remaining section (POD can end at EOF without =cut)
    flush_section(&mut doc, &current_section, &body, leading_list || in_over);

    doc
}

/// Recognized POD commands. Returns the command name (without `=`) when `line`
/// starts with `=` followed by exactly one of the known command identifiers and
/// a word boundary (space, tab, or end-of-line). This prevents `=cutlery` from
/// matching `=cut` and `=headache` from matching `=head` (narrow command-map
/// slice: #13575; broader POD parser authority: #4971).
fn pod_command(line: &str) -> Option<&'static str> {
    let rest = line.strip_prefix('=')?;
    // The command is the leading alphanumeric run (e.g. `head1`, `head2`).
    let cmd_end = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
    let cmd = &rest[..cmd_end];
    // After the command, the next char must be whitespace or end-of-line.
    if cmd_end < rest.len() && !rest[cmd_end..].starts_with(char::is_whitespace) {
        return None;
    }
    match cmd {
        "pod" => Some("pod"),
        "cut" => Some("cut"),
        "head1" => Some("head1"),
        "head2" => Some("head2"),
        "head3" => Some("head3"),
        "head4" => Some("head4"),
        "head5" => Some("head5"),
        "head6" => Some("head6"),
        "over" => Some("over"),
        "back" => Some("back"),
        "item" => Some("item"),
        "begin" => Some("begin"),
        "end" => Some("end"),
        "for" => Some("for"),
        "encoding" => Some("encoding"),
        _ => None,
    }
}

/// Extract the argument text after a POD command prefix (e.g. the heading text
/// after `=head1` or the item text after `=item`).
fn pod_command_arg<'a>(line: &'a str, prefix: &str) -> &'a str {
    line.strip_prefix(prefix).map(str::trim).unwrap_or("")
}

#[derive(Debug)]
enum Section {
    Name,
    Synopsis,
    Description,
    Arguments,
    ReturnValues,
    Examples,
    SeeAlso,
    Method(String),
}

/// Stores accumulated body text into the appropriate `PodDoc` field.
///
/// Called when a POD section ends (new section starts, `=cut`, or EOF).
/// The body text is cleaned of POD formatting and stored based on section type:
/// - `Name` → `PodDoc::name`
/// - `Synopsis` → `PodDoc::synopsis`
/// - `Description` → `PodDoc::description` (first paragraph only)
/// - `Arguments` → `PodDoc::arguments`
/// - `ReturnValues` → `PodDoc::return_values`
/// - `Examples` → `PodDoc::examples`
/// - `SeeAlso` → `PodDoc::see_also`
/// - `Method(name)` → `PodDoc::methods` entry
///
/// # Arguments
///
/// * `doc` - The `PodDoc` to store extracted content into
/// * `section` - The section type being flushed
/// * `body` - Accumulated raw text for the section
/// * `in_over` - Whether inside an `=over`/`=back` block; leading list content
///   flushed before any `=head` exists is stored under a synthetic Synopsis
///   section rather than discarded (#2488)
fn flush_section(doc: &mut PodDoc, section: &Option<Section>, body: &str, in_over: bool) {
    let section = match section {
        Some(s) => s,
        // A `=over`/`=item` list before the first `=head` forms an implicit
        // synopsis block in many CPAN modules — keep it instead of dropping
        // the accumulated body on the floor (#2488).
        None if in_over => &Section::Synopsis,
        None => return,
    };

    let trimmed = body.trim();
    if trimmed.is_empty() {
        return;
    }

    let cleaned = strip_pod_formatting(trimmed);

    match section {
        Section::Name => {
            // NAME renders links as plain display text, not markdown — its
            // consumer is plain perldoc text, and percent-encoded link
            // targets made a cleaned NAME longer than its source (#12824).
            doc.name = Some(strip_pod_formatting_display_text(trimmed));
        }
        Section::Synopsis => {
            // Synopsis feeds the same plain-text hover/virtual-content
            // surfaces as NAME, so links render as display text there too.
            // The markdown `L<>` rendering percent-encodes link targets and
            // made a cleaned synopsis longer than its source, tripping the
            // `pod_extraction` fuzz invariant (#12824 family).
            doc.synopsis = Some(strip_pod_formatting_display_text(trimmed));
        }
        Section::Description => {
            // Take only the first paragraph
            let first_para = first_paragraph(&cleaned);
            doc.description = Some(first_para);
        }
        Section::Arguments => {
            doc.arguments = Some(cleaned);
        }
        Section::ReturnValues => {
            doc.return_values = Some(cleaned);
        }
        Section::Examples => {
            doc.examples = Some(cleaned);
        }
        Section::SeeAlso => {
            doc.see_also = Some(cleaned);
        }
        Section::Method(name) => {
            doc.methods.insert(name.clone(), cleaned);
        }
    }
}

/// Extract the first paragraph (text before the first blank line).
fn first_paragraph(text: &str) -> String {
    let mut result = String::new();
    for line in text.lines() {
        if line.trim().is_empty() && !result.is_empty() {
            break;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
    }
    result
}

/// Maximum nesting depth for POD inline formatting codes.
///
/// POD formatting codes (`B<I<...>>`, `L<...>`, etc.) are stripped recursively,
/// one level per call. Pathological or malicious input with extreme nesting could
/// otherwise exhaust the stack. Past this cap, inner content is returned verbatim
/// (with delimiters already removed) rather than recursing further. Real-world POD
/// never nests anywhere near this deep.
const MAX_POD_FORMATTING_DEPTH: usize = 100;

/// Strip POD inline formatting codes: `B<bold>`, `I<italic>`, `C<code>`, `L<link>`,
/// and decode common `E<>` entities.
///
/// Handles simple (non-nested) formatting codes. Nested codes like `B<I<text>>`
/// are handled by stripping outer codes first.
pub fn strip_pod_formatting(text: &str) -> String {
    strip_pod_formatting_depth(text, 0)
}

/// Like [`strip_pod_formatting`], but renders `L<...>` links as their plain
/// display text only — no markdown `[text](url)` wrapper, no percent-encoded
/// target. Used for the NAME and SYNOPSIS fields (#12824, #14171): their
/// consumers render them as plain perldoc text, so link markup is noise, and
/// the percent-encoding expansion made a cleaned field longer than its source,
/// violating the extraction invariant the `pod_extraction` fuzz target asserts.
pub fn strip_pod_formatting_display_text(text: &str) -> String {
    strip_pod_formatting_depth_links(text, 0, LinkRendering::DisplayText)
}

/// How `L<...>` links render while stripping formatting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkRendering {
    /// `[text](perldoc://target)` with percent-encoded target.
    Markdown,
    /// Bare display text only — explicit `text|target` keeps `text`,
    /// `Module/section` keeps `Module`, `Module::Name` keeps `Module::Name`.
    DisplayText,
}

/// Depth-bounded implementation of [`strip_pod_formatting`].
///
/// `depth` tracks how many levels of formatting-code recursion have already
/// occurred. Once it reaches [`MAX_POD_FORMATTING_DEPTH`], inner content is
/// emitted verbatim instead of recursing, guarding against stack overflow on
/// adversarially deep input such as `B<I<B<I<...>>>>`.
fn strip_pod_formatting_depth(text: &str, depth: usize) -> String {
    strip_pod_formatting_depth_links(text, depth, LinkRendering::Markdown)
}

fn strip_pod_formatting_depth_links(text: &str, depth: usize, links: LinkRendering) -> String {
    if depth >= MAX_POD_FORMATTING_DEPTH {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Check for formatting code: X<...> or X<< ... >> where X is a POD code.
        if i + 2 < len
            && chars[i].is_ascii_alphabetic()
            && chars[i + 1] == '<'
            && is_pod_format_code(chars[i])
        {
            let code_char = chars[i];
            let delimiter_width = opening_delimiter_width(&chars, i + 1);
            i += 1 + delimiter_width; // skip X and the opening angle delimiter

            let start = i;
            let end = if delimiter_width == 1 {
                // Find matching > accounting for nested <> in classic X<...> codes.
                let mut depth = 1;
                while i < len && depth > 0 {
                    if chars[i] == '<' {
                        depth += 1;
                    } else if chars[i] == '>' {
                        depth -= 1;
                    }
                    if depth > 0 {
                        i += 1;
                    }
                }
                let end = i;
                if i < len {
                    i += 1; // skip >
                }
                end
            } else {
                // POD permits doubled (or wider) delimiters so content can contain raw
                // angle brackets, for example C<< $obj->method >>.
                while i < len && !has_closing_delimiter(&chars, i, delimiter_width) {
                    i += 1;
                }
                let end = i;
                if i < len {
                    i += delimiter_width;
                }
                end
            };

            let inner = &chars[start..end];
            let mut inner_str: String = inner.iter().collect();
            if delimiter_width > 1 {
                inner_str = trim_multidelimiter_padding(&inner_str).to_string();
            }

            let display = match code_char {
                'L' => match links {
                    LinkRendering::Markdown => extract_link_display(&inner_str, depth + 1),
                    LinkRendering::DisplayText => extract_link_display_text(&inner_str, depth + 1),
                },
                'E' => decode_pod_entity(&inner_str),
                _ => strip_pod_formatting_depth_links(&inner_str, depth + 1, links),
            };

            result.push_str(&display);
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

fn opening_delimiter_width(chars: &[char], start: usize) -> usize {
    chars[start..].iter().take_while(|ch| **ch == '<').count()
}

fn has_closing_delimiter(chars: &[char], start: usize, delimiter_width: usize) -> bool {
    chars
        .get(start..start + delimiter_width)
        .is_some_and(|candidate| candidate.iter().all(|ch| *ch == '>'))
}

fn trim_multidelimiter_padding(text: &str) -> &str {
    text.trim_matches(|ch: char| ch.is_ascii_whitespace())
}

/// Percent-encode characters that are invalid in a markdown link URL.
///
/// Encodes spaces (most common in POD section names like `L<Module/"Section Name">`)
/// and other characters that would break the markdown `[text](url)` parser.
fn encode_pod_link_target(target: &str) -> String {
    let mut encoded = String::with_capacity(target.len());
    for byte in target.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':' | b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn escape_markdown_link_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' | '[' | ']' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Extract a markdown link from a POD `L<>` formatting code.
///
/// Returns `[display](perldoc://target)` so LSP clients (VS Code) render
/// the link as clickable in hover tooltips.  Spaces in section names are
/// percent-encoded so the URL is well-formed.
///
/// Handles all standard POD link forms:
/// - `L<Module::Name>` → `[Module::Name](perldoc://Module::Name)`
/// - `L<text|Module::Name>` → `[text](perldoc://Module::Name)`
/// - `L<Module::Name/section>` → `[Module::Name](perldoc://Module::Name/section)`
/// - `L<text|Module::Name/section>` → `[text](perldoc://Module::Name/section)`
fn extract_link_display(link: &str, depth: usize) -> String {
    // L<text|target> — explicit display text before the pipe
    if let Some(pipe_pos) = link.find('|') {
        let display =
            escape_markdown_link_text(&strip_pod_formatting_depth(link[..pipe_pos].trim(), depth));
        let target = encode_pod_link_target(link[pipe_pos + 1..].trim());
        return format!("[{display}](perldoc://{target})");
    }
    // L<Module/section> — module + section, display is just the module part
    if let Some(slash_pos) = link.find('/') {
        let module =
            escape_markdown_link_text(&strip_pod_formatting_depth(link[..slash_pos].trim(), depth));
        let target = encode_pod_link_target(link.trim());
        return format!("[{module}](perldoc://{target})");
    }
    // L<Module::Name> — simple module reference
    let display = escape_markdown_link_text(&strip_pod_formatting_depth(link.trim(), depth));
    let target = encode_pod_link_target(link.trim());
    format!("[{display}](perldoc://{target})")
}

/// Display-text-only link rendering for the NAME field (#12824): keeps the
/// human-readable half of every link form and drops the target entirely —
/// `L<text|target>` → `text`, `L<Module/section>` → `Module`,
/// `L<Module::Name>` → `Module::Name`. No markdown wrapper, no
/// percent-encoding, so the result can never exceed the source link text by
/// more than the formatting codes it removes. URL links render verbatim and
/// local-section links render their section label — the unconditional
/// module/section split would otherwise reduce `L<https://example.com/a>` to
/// `https:` and `L</section>` to an empty name (#12824 review).
fn extract_link_display_text(link: &str, depth: usize) -> String {
    let trimmed = link.trim();
    let display = if let Some(pipe_pos) = trimmed.find('|') {
        // Explicit display text wins for every link form.
        &trimmed[..pipe_pos]
    } else if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("ftp://")
        || trimmed.starts_with("mailto:")
    {
        // URL link: render the URL itself, as Pod::Simple::Text does.
        trimmed
    } else if let Some(section) = trimmed.strip_prefix('/') {
        // Local-section link L</section>: the display is the section label.
        section.trim_matches('"')
    } else if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        // Quoted section link L<"Section With Spaces">.
        trimmed.trim_matches('"')
    } else {
        match trimmed.find('/') {
            // L<Module/section>: display is the module part.
            Some(slash_pos) => &trimmed[..slash_pos],
            None => trimmed,
        }
    };
    strip_pod_formatting_depth_links(display.trim(), depth, LinkRendering::DisplayText)
}

/// Decodes a POD E<> entity to its corresponding character.
///
/// Handles standard POD escape sequences:
/// - `E<lt>` → `<`
/// - `E<gt>` → `>`
/// - `E<amp>` → `&`
/// - `E<quot>` → `"`
/// - `E<apos>` → `'`
///
/// - `E<sol>` -> `/`
/// - `E<verbar>` -> `|`
/// - `E<number>`, `E<0xhex>`, and `E<0octal>` numeric codepoints
///
/// Unknown entities are returned as-is.
fn decode_pod_entity(entity: &str) -> String {
    match entity {
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "amp" => "&".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "sol" => "/".to_string(),
        "verbar" => "|".to_string(),
        _ => decode_numeric_pod_entity(entity).unwrap_or_else(|| entity.to_string()),
    }
}

fn decode_numeric_pod_entity(entity: &str) -> Option<String> {
    if entity.is_empty() {
        return None;
    }

    let codepoint =
        if let Some(hex) = entity.strip_prefix("0x").or_else(|| entity.strip_prefix("0X")) {
            u32::from_str_radix(hex, 16).ok()?
        } else if entity.starts_with('0') && entity.len() > 1 {
            u32::from_str_radix(entity, 8).ok()?
        } else {
            entity.parse::<u32>().ok()?
        };

    char::from_u32(codepoint).map(|ch| ch.to_string())
}

fn is_pod_format_code(c: char) -> bool {
    matches!(c, 'B' | 'I' | 'C' | 'L' | 'F' | 'S' | 'E' | 'X' | 'Z')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_paragraph_stops_at_first_blank_line() {
        let text = "first line\nsecond line\n\nthird line";

        assert_eq!(first_paragraph(text), "first line\nsecond line");
    }

    #[test]
    fn first_paragraph_skips_leading_blank_before_text() {
        let text = "\nfirst paragraph\n\nsecond paragraph";

        assert_eq!(first_paragraph(text), "first paragraph");
    }

    // ── decode_pod_entity ────────────────────────────────────────────────────

    #[test]
    fn decode_entity_unknown_returns_name_unchanged() {
        // Unknown entity names fall through to the `_` branch and are returned as-is.
        assert_eq!(decode_pod_entity("nbsp"), "nbsp");
        assert_eq!(decode_pod_entity("unknown"), "unknown");
        assert_eq!(decode_pod_entity("copy"), "copy");
    }

    #[test]
    fn decode_entity_empty_returns_empty() {
        // Empty entity name is unknown → returned unchanged (empty string).
        assert_eq!(decode_pod_entity(""), "");
    }

    #[test]
    fn decode_entity_numeric_codepoints() {
        assert_eq!(decode_pod_entity("32"), " ");
        assert_eq!(decode_pod_entity("0x20"), " ");
        assert_eq!(decode_pod_entity("0X3BB"), "λ");
        assert_eq!(decode_pod_entity("181"), "µ");
        assert_eq!(decode_pod_entity("0x201E"), "„");
        assert_eq!(decode_pod_entity("075"), "=");
    }

    #[test]
    fn decode_entity_invalid_numeric_returns_unchanged() {
        assert_eq!(decode_pod_entity("0x"), "0x");
        assert_eq!(decode_pod_entity("09"), "09");
        assert_eq!(decode_pod_entity("1114112"), "1114112");
        assert_eq!(decode_pod_entity("0x110000"), "0x110000");
    }

    #[test]
    fn decode_entity_known_entities() {
        assert_eq!(decode_pod_entity("lt"), "<");
        assert_eq!(decode_pod_entity("gt"), ">");
        assert_eq!(decode_pod_entity("amp"), "&");
        assert_eq!(decode_pod_entity("quot"), "\"");
        assert_eq!(decode_pod_entity("apos"), "'");
        assert_eq!(decode_pod_entity("sol"), "/");
        assert_eq!(decode_pod_entity("verbar"), "|");
    }

    // ── multi-angle POD formatting delimiters ────────────────────────────────

    #[test]
    fn strips_double_angle_code_formatting() {
        assert_eq!(strip_pod_formatting("C<< $obj->method >>"), "$obj->method");
    }

    #[test]
    fn double_angle_formatting_allows_single_angle_content() {
        assert_eq!(strip_pod_formatting("Use C<< <=> >> for comparison"), "Use <=> for comparison");
    }

    #[test]
    fn double_angle_link_renders_markdown() {
        assert_eq!(
            strip_pod_formatting("L<< display text|File::Find/The wanted function >>"),
            "[display text](perldoc://File::Find/The%20wanted%20function)"
        );
    }

    // ── L<> display-text trimming (issues #2480, #2482, #2485) ───────────────

    #[test]
    fn link_pipe_form_trims_display_and_target() {
        // L<text|target> — leading/trailing whitespace on both sides is trimmed
        // so neither the display text nor the target leaks padding (#2480).
        assert_eq!(strip_pod_formatting("L<  text  |  target  >"), "[text](perldoc://target)");
    }

    #[test]
    fn link_slash_form_trims_module_display() {
        // L<Module/section> — the module display part is trimmed so no trailing
        // space leaks into the rendered link text (#2482).
        assert_eq!(
            strip_pod_formatting("L<Module / Section>"),
            "[Module](perldoc://Module%20/%20Section)"
        );
    }

    #[test]
    fn link_simple_form_trims_display() {
        // L<Module::Name> — surrounding whitespace is trimmed from the display
        // text (#2485).
        assert_eq!(
            strip_pod_formatting("L< Module::Name >"),
            "[Module::Name](perldoc://Module::Name)"
        );
    }

    #[test]
    fn strip_pod_formatting_handles_nested_text_and_entities() {
        let text = "Use B<I<strict>> and C<$value E<lt> 10>";

        assert_eq!(strip_pod_formatting(text), "Use strict and $value < 10");
    }

    #[test]
    fn strip_pod_formatting_deeply_nested_does_not_overflow_stack() {
        // Regression for unbounded recursion: ~5000 nested B<I<...>> formatting
        // codes previously blew the stack. With MAX_POD_FORMATTING_DEPTH the call
        // returns without panicking; content past the cap is emitted verbatim.
        const NESTING: usize = 5000;
        let mut text = String::from("core");
        for _ in 0..NESTING {
            text = format!("B<I<{text}>>");
        }

        // Must return normally (no stack overflow / panic).
        let stripped = strip_pod_formatting(&text);

        // The innermost payload survives the strip.
        assert!(stripped.contains("core"), "expected innermost content to remain");
        // Past the depth cap, residual unstripped delimiters may remain, so the
        // result is not guaranteed to be exactly "core"; the contract here is
        // simply that the function terminates safely.
    }

    #[test]
    fn extract_pod_deeply_nested_head2_does_not_overflow_stack() {
        // Exercise the public reachability path (=head2 → strip_pod_formatting).
        const NESTING: usize = 5000;
        let mut heading = String::from("name");
        for _ in 0..NESTING {
            heading = format!("B<I<{heading}>>");
        }
        let source = format!("=head2 {heading}\n\nbody text\n\n=cut\n");

        // Must not overflow the stack.
        let doc = extract_pod(&source);

        assert_eq!(doc.methods.len(), 1, "expected exactly one method section");
    }

    // ── POD command-prefix matching (#13575; broader authority #4971) ─────

    #[test]
    fn pod_command_maps_complete_closed_vocabulary() {
        for command in [
            "pod", "cut", "head1", "head2", "head3", "head4", "head5", "head6", "over", "back",
            "item", "begin", "end", "for", "encoding",
        ] {
            let bare = format!("={command}");
            let spaced = format!("={command} value");
            let tabbed = format!("={command}\tvalue");

            assert_eq!(pod_command(&bare), Some(command), "bare directive {command}");
            assert_eq!(pod_command(&spaced), Some(command), "spaced directive {command}");
            assert_eq!(pod_command(&tabbed), Some(command), "tabbed directive {command}");
        }
    }

    #[test]
    fn pod_command_rejects_prefix_only_matches() {
        // #13575: lookalikes must not match a shorter recognized directive.
        for line in [
            "=cutlery",
            "=headache",
            "=head7",
            "=head10",
            "=head1:",
            "=cut!",
            "=heаd1 confusable",
            "=overboard",
            "=backspace",
            "=items",
            "=podcast",
            "=beginner",
            "=ending",
            "=forward",
            "=encodingx",
        ] {
            assert_eq!(pod_command(line), None, "lookalike directive {line}");
        }
    }

    #[test]
    fn extract_pod_does_not_treat_fake_commands_as_pod() {
        // `=cutlery` inside a POD block must NOT end POD — it's not a real
        // `=cut` directive. Use ARGUMENTS (which captures full body text, not
        // just the first paragraph) so we can verify text after the fake
        // command is retained.
        let source =
            "=head1 ARGUMENTS\n\nFirst argument.\n\n=cutlery\n\nSecond argument.\n\n=cut\n";
        let doc = extract_pod(source);
        let args = doc.arguments.as_deref().unwrap_or("");
        assert!(
            args.contains("Second argument"),
            "text after fake =cutlery must still be captured, got arguments: {args:?}"
        );
    }

    #[test]
    fn extract_pod_handles_head3_through_head6() {
        // #4971: =head3–=head6 must flush the current section rather than
        // falling through to body accumulation.
        let source = "=head1 NAME\n\nFoo\n\n=head2 method_a\n\nBody A\n\n=head3 Details\n\n=head2 method_b\n\nBody B\n\n=cut\n";
        let doc = extract_pod(source);
        assert_eq!(doc.methods.len(), 2, "expected 2 methods, got {}", doc.methods.len());
    }

    // ── encode_pod_link_target ───────────────────────────────────────────────

    #[test]
    fn encode_link_empty_string() {
        assert_eq!(encode_pod_link_target(""), "");
    }

    #[test]
    fn encode_link_pure_ascii_safe_chars_pass_through() {
        // The safe set includes alphanumerics and: - . _ ~ : /
        assert_eq!(encode_pod_link_target("-._~"), "-._~");
        assert_eq!(
            encode_pod_link_target("A::Module/section-name_v1.0~"),
            "A::Module/section-name_v1.0~"
        );
        assert_eq!(
            encode_pod_link_target("Module::Name/path-._~/Section"),
            "Module::Name/path-._~/Section"
        );
    }

    #[test]
    fn encode_link_percent_encodes_markdown_breakers() {
        assert_eq!(
            encode_pod_link_target("Module Name/[section](x)"),
            "Module%20Name/%5Bsection%5D%28x%29"
        );
    }

    #[test]
    fn encode_link_multibyte_utf8_cafe() {
        // "café" — 'é' is U+00E9, encoded in UTF-8 as 0xC3 0xA9.
        let result = encode_pod_link_target("café");
        assert_eq!(result, "caf%C3%A9");
    }

    #[test]
    fn encode_link_multibyte_utf8_japanese() {
        // "日本語" — each kanji is three UTF-8 bytes.
        let result = encode_pod_link_target("日本語");
        // 日 = E6 97 A5, 本 = E6 9C AC, 語 = E8 AA 9E
        assert_eq!(result, "%E6%97%A5%E6%9C%AC%E8%AA%9E");
    }

    #[test]
    fn encode_link_consecutive_special_chars() {
        // Multiple consecutive non-safe characters are each percent-encoded.
        assert_eq!(encode_pod_link_target("a  b"), "a%20%20b");
        assert_eq!(encode_pod_link_target("((()))"), "%28%28%28%29%29%29");
    }

    #[test]
    fn encode_link_control_chars_tab_and_newline() {
        // Tab (0x09) and newline (0x0A) are not in the safe set → percent-encoded.
        assert_eq!(encode_pod_link_target("\t"), "%09");
        assert_eq!(encode_pod_link_target("\n"), "%0A");
        assert_eq!(encode_pod_link_target("a\tb"), "a%09b");
    }

    #[test]
    fn markdown_link_text_escapes_only_link_delimiters() {
        let text = r"back\slash [label] (target)";

        assert_eq!(escape_markdown_link_text(text), r"back\\slash \[label\] (target)");
    }
}

// ── #2488: leading =over lists before the first =head ──────────────────

#[test]
fn leading_over_list_before_any_head_lands_in_synopsis() {
    // A `=over/=item` list before any `=head` is an implicit synopsis
    // block (common in CPAN modules); it must be kept, not discarded.
    let text = "=pod\n\n=over 4\n\n=item *\n\nFirst leading point\n\n=item *\n\nSecond leading point\n\n=back\n\n=head1 DESCRIPTION\n\nBody text.\n\n=cut\n";
    let doc = extract_pod(text);
    assert!(doc.synopsis.is_some(), "leading list must be retained");
    assert!(doc.synopsis.as_ref().is_some_and(|s| s.contains("First leading point")));
    assert!(doc.synopsis.as_ref().is_some_and(|s| s.contains("Second leading point")));
}

#[test]
fn leading_content_without_over_is_still_dropped() {
    // Bare prose before any `=head` (no =over context) keeps the historic
    // behavior: no section context means nothing is stored.
    let text = "=pod\n\nStray prose with no section context.\n\n=head1 DESCRIPTION\n\nBody text.\n\n=cut\n";
    let doc = extract_pod(text);
    assert!(doc.synopsis.is_none(), "synopsis should stay empty: {:?}", doc.synopsis);
}
