//! Extract path arguments from `use lib` and `no lib` pragma arguments.

use super::UseLibPath;

pub(super) fn extract_paths_from_args(args: &str, out: &mut Vec<UseLibPath>) -> usize {
    let raw = args;
    let args = raw.trim_start();
    let leading = raw.len() - args.len();
    let args = args.trim_end_matches(';').trim_end();

    if let Some(rest) = args.strip_prefix("qw") {
        return leading + 2 + extract_qw_paths(rest, out);
    }

    if let Some(inner) = strip_parens(args) {
        let before = out.len();
        extract_quoted_list(inner, out);
        if out.len() != before {
            return leading + args.len();
        }
        return 0;
    }

    leading + extract_quoted_list(args, out)
}

fn extract_qw_paths(rest: &str, out: &mut Vec<UseLibPath>) -> usize {
    let trimmed = rest.trim_start();
    let leading = rest.len() - trimmed.len();
    let (open, close) = match trimmed.chars().next() {
        Some('(') => ('(', ')'),
        Some('/') => ('/', '/'),
        Some('{') => ('{', '}'),
        Some('[') => ('[', ']'),
        Some('<') => ('<', '>'),
        Some('!') => ('!', '!'),
        _ => return 0,
    };

    let inner = &trimmed[open.len_utf8()..];
    let Some(end) = inner.find(close) else {
        for word in inner.split_whitespace() {
            out.push(UseLibPath { path: word.to_string(), from_findbin: false });
        }
        return leading + open.len_utf8() + inner.len();
    };
    let content = &inner[..end];

    for word in content.split_whitespace() {
        out.push(UseLibPath { path: word.to_string(), from_findbin: false });
    }

    leading + open.len_utf8() + end + close.len_utf8()
}

fn strip_parens(s: &str) -> Option<&str> {
    let s = s.trim();
    let inner = s.strip_prefix('(')?;
    let inner = inner.trim_end().strip_suffix(')')?;
    Some(inner)
}

fn extract_quoted_list(s: &str, out: &mut Vec<UseLibPath>) -> usize {
    let mut remaining = s;
    let mut position = 0;
    let mut consumed = 0;

    while !remaining.is_empty() {
        let trimmed = remaining.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        let leading = remaining.len() - trimmed.len();
        let candidate_start = position + leading;
        remaining = trimmed;
        if remaining.is_empty() {
            break;
        }

        // Skip Perl line comments: # ... <newline>
        if remaining.starts_with('#') {
            let skipped = match remaining.find('\n') {
                Some(nl) => nl + 1,
                None => remaining.len(),
            };
            position = candidate_start + skipped;
            remaining = &remaining[skipped..];
            continue;
        }

        if let Some((path, from_findbin, rest)) = extract_one_quoted(remaining) {
            out.push(UseLibPath { path, from_findbin });
            let parsed = remaining.len() - rest.len();
            position = candidate_start + parsed;
            consumed = position;
            remaining = rest;
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
