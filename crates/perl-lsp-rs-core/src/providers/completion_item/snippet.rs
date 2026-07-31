//! Insertion-format representation and TextMate snippet rendering.
//!
//! LSP treats `CompletionItem.kind` (what the item *is*) and
//! `CompletionItem.insertTextFormat` (how its text must be *interpreted*) as
//! independent fields. A function may legitimately insert a snippet, so the
//! format lives here as its own value rather than being derived from the kind.
//!
//! Every snippet also has to work on clients that do not advertise
//! `completionItem.snippetSupport`. Those clients insert `insertText`
//! verbatim, so a snippet body must never reach them: they would get literal
//! `${1:<}` in their buffer. [`InsertTextFormat::Snippet`] therefore *requires*
//! a plain-text fallback — it is impossible to declare snippet insertion
//! without also supplying the text a non-snippet client receives.

/// How a completion item's insert text must be interpreted by the client.
///
/// Maps to LSP `InsertTextFormat`: `PlainText` = 1, `Snippet` = 2.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InsertTextFormat {
    /// Insert the text verbatim. This is the default for every item.
    #[default]
    PlainText,
    /// The insert text is a TextMate snippet.
    Snippet {
        /// Literal text for clients without `snippetSupport`.
        ///
        /// Must be valid Perl on its own: no tab stops, no snippet escapes.
        /// Build it with [`InsertTextFormat::snippet`] so it stays in sync
        /// with the body rather than being hand-maintained.
        plain_fallback: String,
    },
}

impl InsertTextFormat {
    /// Snippet insertion for `body`, with the plain-text fallback rendered from it.
    #[must_use]
    pub fn snippet(body: &str) -> Self {
        Self::Snippet { plain_fallback: render_snippet_plaintext(body) }
    }

    /// The format for a body authored in this server's snippet tables.
    ///
    /// Returns `PlainText` when the body contains no snippet construct at all —
    /// the two interpretations are then identical, so there is no reason to ask
    /// the client to parse it — and `Snippet` otherwise.
    ///
    /// Only for text this server authors. Never pass user-derived text (a
    /// variable name, a workspace symbol, a file path): a `$` or `\` occurring
    /// naturally there is literal, and reinterpreting it as snippet grammar
    /// would corrupt the insertion. Such items are `PlainText` by default.
    #[must_use]
    pub fn for_authored_body(body: &str) -> Self {
        if has_snippet_constructs(body) { Self::snippet(body) } else { Self::PlainText }
    }

    /// Whether this item's text is a snippet.
    #[must_use]
    pub const fn is_snippet(&self) -> bool {
        matches!(self, Self::Snippet { .. })
    }

    /// The plain-text fallback, when this is snippet insertion.
    #[must_use]
    pub fn plain_fallback(&self) -> Option<&str> {
        match self {
            Self::PlainText => None,
            Self::Snippet { plain_fallback } => Some(plain_fallback),
        }
    }
}

/// Whether `body` was authored as a snippet: it contains a numbered tab stop
/// (`$1`), a placeholder (`${...}`), or a backslash escape.
///
/// A lone `$name` deliberately does *not* count. Plain insert texts like
/// `opendir(my $dh, )` contain a literal Perl variable and no snippet
/// construct; classifying them as snippets is what would make the client
/// swallow `$dh` as an unknown variable. Inside a body that *is* a snippet, an
/// unescaped `$name` is a defect — see [`snippet_body_defects`].
///
/// When this is false, `insertTextFormat` is immaterial and `PlainText` is the
/// honest answer. When it is true, sending the body as `PlainText` puts literal
/// `${1:...}` or stray backslashes into the user's buffer.
#[must_use]
pub fn has_snippet_constructs(body: &str) -> bool {
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => return true,
            '$' => match chars.get(i + 1) {
                Some(&next) if next.is_ascii_digit() || next == '{' => return true,
                _ => i += 1,
            },
            _ => i += 1,
        }
    }
    false
}

/// Render a TextMate snippet body as the literal text a non-snippet client
/// should receive.
///
/// Implements the grammar this server actually emits, rather than stripping
/// placeholders with regexes (which cannot see escapes or nesting):
///
/// - `\$`, `\}`, `\\` and friends — the escaped character, literally.
/// - `$N` / `${N}` — tab stop, contributes nothing.
/// - `${N:default}` — the default, rendered recursively (so nested tab stops
///   inside a default are handled).
/// - `${N|one,two|}` — the first choice.
/// - `${N/find/replace/flags}` — transform; contributes nothing, since there is
///   no captured text to transform at insertion time.
/// - `$name` / `${name}` — a snippet *variable*. Contributes nothing: an
///   unknown variable has no value. A literal Perl `$name` must be written
///   `\$name`, which [`snippet_body_defects`] enforces.
/// - A `$` that begins none of the above is literal.
#[must_use]
pub fn render_snippet_plaintext(body: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    render_until_brace(&chars, &mut i, &mut out, false);
    out
}

/// Render `chars` from `*i`, stopping before an unescaped `}` when `nested`.
fn render_until_brace(chars: &[char], i: &mut usize, out: &mut String, nested: bool) {
    while let Some(&c) = chars.get(*i) {
        match c {
            '\\' => match chars.get(*i + 1) {
                Some(&escaped) => {
                    out.push(escaped);
                    *i += 2;
                }
                None => {
                    out.push('\\');
                    *i += 1;
                }
            },
            '}' if nested => return,
            '$' => {
                if !render_dollar(chars, i, out) {
                    out.push('$');
                    *i += 1;
                }
            }
            _ => {
                out.push(c);
                *i += 1;
            }
        }
    }
}

/// Render a `$`-introduced construct at `*i`. Returns false when `$` is literal.
fn render_dollar(chars: &[char], i: &mut usize, out: &mut String) -> bool {
    match chars.get(*i + 1) {
        // `$N` — tab stop, renders empty.
        Some(&c) if c.is_ascii_digit() => {
            *i += 1;
            while chars.get(*i).is_some_and(char::is_ascii_digit) {
                *i += 1;
            }
            true
        }
        // `$name` — snippet variable, renders empty.
        Some(&c) if is_name_start(c) => {
            *i += 1;
            while chars.get(*i).copied().is_some_and(is_name_continue) {
                *i += 1;
            }
            true
        }
        Some('{') => render_braced(chars, i, out),
        _ => false,
    }
}

/// Render `${...}` at `*i`. Returns false when the construct is malformed, in
/// which case the caller emits a literal `$` and continues.
fn render_braced(chars: &[char], i: &mut usize, out: &mut String) -> bool {
    let mut j = *i + 2;
    let name_start = j;
    if chars.get(j).copied().is_some_and(is_name_start)
        || chars.get(j).is_some_and(char::is_ascii_digit)
    {
        j += 1;
        while chars.get(j).copied().is_some_and(is_name_continue) {
            j += 1;
        }
    }
    if j == name_start {
        // `${` with no name — not a placeholder.
        return false;
    }

    match chars.get(j) {
        // `${N}` / `${name}` — renders empty.
        Some('}') => {
            *i = j + 1;
            true
        }
        // `${N:default}` — render the default in place.
        Some(':') => {
            j += 1;
            render_until_brace(chars, &mut j, out, true);
            // Consume the closing brace when present; an unterminated
            // placeholder still consumes what it rendered.
            if chars.get(j) == Some(&'}') {
                j += 1;
            }
            *i = j;
            true
        }
        // `${N|one,two|}` — take the first choice.
        Some('|') => {
            j += 1;
            while let Some(&c) = chars.get(j) {
                match c {
                    '\\' => {
                        if let Some(&escaped) = chars.get(j + 1) {
                            out.push(escaped);
                            j += 2;
                        } else {
                            j += 1;
                        }
                    }
                    ',' | '|' => break,
                    _ => {
                        out.push(c);
                        j += 1;
                    }
                }
            }
            *i = skip_to_close(chars, j);
            true
        }
        // `${N/find/replace/flags}` — no captured text, renders empty.
        Some('/') => {
            *i = skip_to_close(chars, j);
            true
        }
        _ => false,
    }
}

/// Index just past the next unescaped `}` at or after `from`.
fn skip_to_close(chars: &[char], from: usize) -> usize {
    let mut j = from;
    while let Some(&c) = chars.get(j) {
        match c {
            '\\' => j += 2,
            '}' => return j + 1,
            _ => j += 1,
        }
    }
    j
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Report snippet-body constructs that do not mean what their author intended.
///
/// This exists because the failure is silent and client-specific: a body
/// containing literal Perl `$self` is grammatically a *variable reference*, so
/// VS Code inserts an editable `self` placeholder while another client may
/// insert nothing. Both are wrong, and neither surfaces as an error.
///
/// A literal dollar must be written `\$`. Empty result means the body is clean.
#[must_use]
pub fn snippet_body_defects(body: &str) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut defects = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => i += 2,
            '$' => match chars.get(i + 1) {
                Some(&next) if next.is_ascii_digit() => {
                    i += 1;
                    while chars.get(i).is_some_and(char::is_ascii_digit) {
                        i += 1;
                    }
                }
                Some(&next) if is_name_start(next) => {
                    let start = i;
                    i += 1;
                    while chars.get(i).copied().is_some_and(is_name_continue) {
                        i += 1;
                    }
                    let name: String = chars[start..i].iter().collect();
                    defects.push(format!(
                        "`{name}` is a snippet variable reference, not literal text; \
                         write `\\{name}` to insert a literal Perl variable"
                    ));
                }
                Some('{') => {
                    let name_start = i + 2;
                    let mut j = name_start;
                    while chars.get(j).copied().is_some_and(is_name_continue) {
                        j += 1;
                    }
                    let name: String = chars[name_start..j].iter().collect();
                    if name.is_empty() {
                        defects.push("`${` does not open a placeholder".to_string());
                        i += 1;
                    } else {
                        if !name.chars().all(|c| c.is_ascii_digit()) {
                            defects.push(format!(
                                "`${{{name}}}` is a named snippet variable; only numbered \
                                 tab stops are supported"
                            ));
                        }
                        i = j;
                    }
                }
                _ => {
                    defects.push(
                        "bare `$` is only literal by accident of client grammar; write `\\$`"
                            .to_string(),
                    );
                    i += 1;
                }
            },
            _ => i += 1,
        }
    }
    defects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_stops_render_as_nothing() {
        assert_eq!(render_snippet_plaintext("use strict;\n$0"), "use strict;\n");
        assert_eq!(render_snippet_plaintext("if ($1) {\n    $0\n}"), "if () {\n    \n}");
        assert_eq!(render_snippet_plaintext("a${10}b"), "ab");
    }

    #[test]
    fn placeholder_defaults_are_kept() {
        assert_eq!(render_snippet_plaintext("sub ${1:name} { $0 }"), "sub name {  }");
        assert_eq!(render_snippet_plaintext("/${1:pattern}/m${0}"), "/pattern/m");
    }

    #[test]
    fn escaped_dollars_become_literal_dollars() {
        assert_eq!(render_snippet_plaintext("my \\$self = shift;\n$0"), "my $self = shift;\n");
        assert_eq!(render_snippet_plaintext("\\\\ and \\} and \\$"), "\\ and } and $");
    }

    #[test]
    fn nested_placeholders_render_recursively() {
        // The `${2:...}` default itself contains an escaped literal dollar.
        assert_eq!(
            render_snippet_plaintext("open(my \\$fh, '${1:<}', ${2:\\$file})"),
            "open(my $fh, '<', $file)"
        );
        assert_eq!(render_snippet_plaintext("${1:outer ${2:inner} tail}"), "outer inner tail");
    }

    #[test]
    fn choices_take_the_first_option() {
        assert_eq!(render_snippet_plaintext("is  => '${1|ro,rw|}'"), "is  => 'ro'");
    }

    #[test]
    fn transforms_render_as_nothing() {
        assert_eq!(render_snippet_plaintext("${1/[a-z]/\\u$0/}x"), "x");
    }

    #[test]
    fn unknown_variables_render_as_nothing() {
        assert_eq!(render_snippet_plaintext("my ($self) = @_;"), "my () = @_;");
    }

    #[test]
    fn a_dollar_that_starts_nothing_is_literal() {
        assert_eq!(render_snippet_plaintext("die \"oops: $!\""), "die \"oops: $!\"");
        assert_eq!(render_snippet_plaintext("local $/;"), "local $/;");
    }

    #[test]
    fn unterminated_placeholder_still_renders_its_default() {
        assert_eq!(render_snippet_plaintext("sub ${1:name"), "sub name");
    }

    #[test]
    fn snippet_format_carries_a_rendered_fallback() {
        let format = InsertTextFormat::snippet("my (\\$self${1:, @args}) = @_;\n$0");
        assert!(format.is_snippet());
        assert_eq!(format.plain_fallback(), Some("my ($self, @args) = @_;\n"));
    }

    #[test]
    fn plain_text_is_the_default_and_has_no_fallback() {
        let format = InsertTextFormat::default();
        assert_eq!(format, InsertTextFormat::PlainText);
        assert!(!format.is_snippet());
        assert_eq!(format.plain_fallback(), None);
    }

    #[test]
    fn defects_flag_unescaped_perl_variables() {
        let defects = snippet_body_defects("my ($self${1:, @args}) = @_;");
        assert_eq!(defects.len(), 1, "{defects:?}");
        assert!(defects[0].contains("$self"), "{defects:?}");
    }

    #[test]
    fn defects_flag_bare_dollars() {
        let defects = snippet_body_defects("die \"$@\";");
        assert_eq!(defects.len(), 1, "{defects:?}");
        assert!(defects[0].contains("bare `$`"), "{defects:?}");
    }

    #[test]
    fn defects_accept_escaped_dollars_and_tab_stops() {
        assert!(snippet_body_defects("my \\$self = shift;\n$0").is_empty());
        assert!(snippet_body_defects("open my \\$${1:fh}, '<', ${2:\\$file};").is_empty());
    }

    #[test]
    fn defects_flag_named_placeholders() {
        let defects = snippet_body_defects("${TM_FILENAME}");
        assert_eq!(defects.len(), 1, "{defects:?}");
        assert!(defects[0].contains("named snippet variable"), "{defects:?}");
    }
}
