//! Extract path arguments from `use lib` and `no lib` pragma arguments.

use super::{UseLibPath, byte_offset_within};

/// Extract path arguments from a pragma argument slice.
///
/// Returns the number of bytes of `args` consumed by the recognized argument
/// list — i.e. the offset just past the last path token that was pushed to
/// `out`. Returns `0` when no path was extracted.
///
/// The count lets a caller inspect what follows the argument list. That is how
/// [`super::UseLibOperation`] tells a properly terminated pragma from one whose
/// semicolon has not been typed yet, whose statement slice runs past unrelated
/// later code.
pub(super) fn extract_paths_from_args(args: &str, out: &mut Vec<UseLibPath>) -> usize {
    let body = args.trim_end_matches(';').trim();

    if let Some(rest) = body.strip_prefix("qw") {
        let rest = rest.trim_start();
        let consumed = extract_qw_paths(rest, out);
        return consumed_end(args, rest, consumed);
    }

    if let Some(inner) = strip_parens(body) {
        let consumed = extract_quoted_list(inner, out);
        if consumed == 0 {
            return 0;
        }
        // Consume through the closing parenthesis, which sits immediately after
        // `inner` by construction in `strip_parens`.
        return consumed_end(args, inner, inner.len() + ')'.len_utf8());
    }

    let consumed = extract_quoted_list(body, out);
    consumed_end(args, body, consumed)
}

fn consumed_end(args: &str, inner: &str, consumed: usize) -> usize {
    if consumed == 0 { 0 } else { byte_offset_within(args, inner) + consumed }
}

/// Returns the number of bytes of `rest` consumed by the `qw` list, or `0`
/// when `rest` does not open a recognized delimiter.
fn extract_qw_paths(rest: &str, out: &mut Vec<UseLibPath>) -> usize {
    let (open, close) = match rest.chars().next() {
        Some('(') => ('(', ')'),
        Some('/') => ('/', '/'),
        Some('{') => ('{', '}'),
        Some('[') => ('[', ']'),
        Some('<') => ('<', '>'),
        Some('!') => ('!', '!'),
        _ => return 0,
    };

    let inner = &rest[open.len_utf8()..];
    let (end, consumed) = match inner.find(close) {
        Some(end) => (end, open.len_utf8() + end + close.len_utf8()),
        None => (inner.len(), rest.len()),
    };
    let content = &inner[..end];

    for word in content.split_whitespace() {
        out.push(UseLibPath { path: word.to_string(), from_findbin: false });
    }

    consumed
}

fn strip_parens(s: &str) -> Option<&str> {
    let s = s.trim();
    let inner = s.strip_prefix('(')?;
    let inner = inner.trim_end().strip_suffix(')')?;
    Some(inner)
}

/// Returns the number of bytes of `s` consumed up to and including the last
/// quoted path pushed to `out`, or `0` when no path was extracted.
fn extract_quoted_list(s: &str, out: &mut Vec<UseLibPath>) -> usize {
    let mut remaining = s.trim();
    let mut consumed = 0;

    while !remaining.is_empty() {
        remaining = remaining.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        if remaining.is_empty() {
            break;
        }

        // Skip Perl line comments: # ... <newline>
        if remaining.starts_with('#') {
            remaining = match remaining.find('\n') {
                Some(nl) => &remaining[nl + 1..],
                None => "",
            };
            continue;
        }

        if let Some((path, from_findbin, rest)) = extract_one_quoted(remaining) {
            out.push(UseLibPath { path, from_findbin });
            consumed = byte_offset_within(s, rest);
            remaining = rest.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        } else {
            break;
        }
    }

    consumed
}

fn extract_one_quoted(s: &str) -> Option<(String, bool, &str)> {
    let s = s.trim();
    let quote = match s.chars().next()? {
        '\'' => '\'',
        '"' => '"',
        _ => return None,
    };

    // `quote` is always a 1-byte ASCII `'` or `"` (the match rejects everything
    // else), so byte 1 is a char boundary today. Slice by `quote.len_utf8()` rather
    // than a hardcoded 1 so the opening-quote length stays explicit and correct by
    // construction if a multi-byte quote arm is ever added (#2369).
    let inner = &s[quote.len_utf8()..];
    let end = find_unescaped_quote(inner, quote)?;
    let content = &inner[..end];
    let rest = &inner[end + quote.len_utf8()..];

    let (path, from_findbin) = resolve_findbin_in_string(content);
    Some((path, from_findbin, rest))
}

fn find_unescaped_quote(s: &str, quote: char) -> Option<usize> {
    let mut escaped = false;

    for (idx, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == quote {
            return Some(idx);
        }
    }

    None
}

fn resolve_findbin_in_string(s: &str) -> (String, bool) {
    // Fully-qualified FindBin variables — no word-boundary ambiguity because `::` terminates
    // the name and braced forms are unambiguous.
    let qualified_vars =
        ["$FindBin::Bin", "$FindBin::RealBin", "${FindBin::Bin}", "${FindBin::RealBin}"];

    for var in &qualified_vars {
        if let Some(rest) = s.strip_prefix(var) {
            let path = rest.strip_prefix('/').unwrap_or(rest);
            if path.is_empty() {
                return (".".to_string(), true);
            }
            return (path.to_string(), true);
        }
    }

    // Short exported forms: `$Bin`, `$RealBin`, `${Bin}`, `${RealBin}`.
    // Braced forms (`${Bin}`) are always unambiguous.  Bare forms (`$Bin`,
    // `$RealBin`) must be followed by `/`, end-of-string, or a non-identifier
    // character to avoid false-positives on variables like `$BinDir` or
    // `$RealBinPath`.
    let bare_short = ["$Bin", "$RealBin"];
    let braced_short = ["${Bin}", "${RealBin}"];

    for var in &bare_short {
        if let Some(rest) = s.strip_prefix(var) {
            // Word-boundary check: the character after the variable name must
            // not be a Perl identifier character (letter, digit, or `_`).
            // This prevents `$BinDir` or `$RealBinPath` from matching `$Bin`/`$RealBin`.
            let next = rest.chars().next();
            if next.is_none() || next.is_some_and(|c| !c.is_alphanumeric() && c != '_') {
                let path = rest.strip_prefix('/').unwrap_or(rest);
                if path.is_empty() {
                    return (".".to_string(), true);
                }
                return (path.to_string(), true);
            }
        }
    }

    for var in &braced_short {
        if let Some(rest) = s.strip_prefix(var) {
            let path = rest.strip_prefix('/').unwrap_or(rest);
            if path.is_empty() {
                return (".".to_string(), true);
            }
            return (path.to_string(), true);
        }
    }

    (s.to_string(), false)
}

#[cfg(test)]
mod extract_one_quoted_tests {
    use super::extract_one_quoted;
    use perl_tdd_support::must_some;

    #[test]
    fn single_and_double_quotes_parse() {
        let (path, findbin, rest) = must_some(extract_one_quoted("'lib/path'"));
        assert_eq!(path, "lib/path");
        assert!(!findbin);
        assert_eq!(rest, "");

        let (path, _, _) = must_some(extract_one_quoted("\"lib\""));
        assert_eq!(path, "lib");
    }

    #[test]
    fn multibyte_first_char_returns_none_and_never_slices() {
        // #2369 guard: a non-ASCII leading char is not a quote, so the function
        // returns None before the opening-quote slice runs. This is what keeps
        // `&s[quote.len_utf8()..]` on a char boundary; verify it holds so a future
        // refactor cannot reintroduce a mid-codepoint slice panic.
        assert_eq!(extract_one_quoted("«weird»"), None);
        assert_eq!(extract_one_quoted("😀'lib'"), None);
    }

    #[test]
    fn trailing_content_after_close_quote_is_returned() {
        let (path, _, rest) = must_some(extract_one_quoted("'a', 'b'"));
        assert_eq!(path, "a");
        assert_eq!(rest, ", 'b'");
    }
}
