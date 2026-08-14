use crate::syntax::cursor::RegexCursor;

use super::analysis::RegexRange;

pub(crate) fn find_interpolations(
    pattern: &str,
    excluded_ranges: &[RegexRange],
) -> Vec<RegexRange> {
    let bytes = pattern.as_bytes();
    let mut cursor = RegexCursor::new(pattern);
    let mut ranges = Vec::new();

    while cursor.current().is_some() {
        if let Some(excluded) =
            excluded_ranges.iter().find(|range| range.contains(cursor.position()))
        {
            cursor.advance_to(excluded.end);
            continue;
        }
        if cursor.skip_quoted_literal()
            || cursor.skip_escape()
            || cursor.skip_char_class()
            || cursor.skip_comment()
        {
            continue;
        }

        let start = cursor.position();
        let Some(sigil @ (b'$' | b'@')) = cursor.current() else {
            cursor.bump();
            continue;
        };
        let Some(next) = bytes.get(start.saturating_add(1)).copied() else {
            cursor.bump();
            continue;
        };

        let end = if next == b'{' {
            braced_interpolation_end(bytes, start.saturating_add(1))
        } else if is_identifier_start(next) || next.is_ascii_digit() {
            identifier_interpolation_end(bytes, start.saturating_add(1))
        } else if is_special_variable(sigil, next) {
            start.saturating_add(2).min(bytes.len())
        } else {
            cursor.bump();
            continue;
        };

        if let Some(range) = RegexRange::new(start, end) {
            ranges.push(range);
        }
        cursor.advance_to(end);
    }

    ranges
}

fn braced_interpolation_end(bytes: &[u8], open: usize) -> usize {
    let mut cursor = open.saturating_add(1).min(bytes.len());
    let mut depth = 1usize;
    let mut escaped = false;

    while cursor < bytes.len() {
        let ch = bytes[cursor];
        if escaped {
            escaped = false;
        } else if ch == b'\\' {
            escaped = true;
        } else if ch == b'{' {
            depth = depth.saturating_add(1);
        } else if ch == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return cursor + 1;
            }
        }
        cursor += 1;
    }

    bytes.len()
}

fn identifier_interpolation_end(bytes: &[u8], start: usize) -> usize {
    let mut cursor = start;
    while let Some(ch) = bytes.get(cursor).copied() {
        if ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b':' | b'\'') {
            cursor += 1;
        } else {
            break;
        }
    }
    cursor
}

fn is_identifier_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_special_variable(sigil: u8, ch: u8) -> bool {
    match sigil {
        b'$' => matches!(
            ch,
            b'$' | b'@'
                | b'%'
                | b'&'
                | b'`'
                | b'\''
                | b'+'
                | b'-'
                | b'!'
                | b'?'
                | b'^'
                | b'/'
                | b'\\'
                | b'|'
                | b'~'
                | b'='
                | b':'
                | b'.'
                | b','
                | b';'
                | b'<'
                | b'>'
                | b'('
                | b')'
                | b'['
                | b']'
        ),
        b'@' => matches!(ch, b'+' | b'-' | b'_'),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_identifier_braced_and_special_forms() {
        let ranges = find_interpolations(r"pre$name${expr{nested}}@_$!post", &[]);
        assert_eq!(ranges.len(), 4);
        assert_eq!(&r"pre$name${expr{nested}}@_$!post"[ranges[0].start..ranges[0].end], "$name");
        assert_eq!(
            &r"pre$name${expr{nested}}@_$!post"[ranges[1].start..ranges[1].end],
            "${expr{nested}}"
        );
        assert_eq!(&r"pre$name${expr{nested}}@_$!post"[ranges[2].start..ranges[2].end], "@_");
        assert_eq!(&r"pre$name${expr{nested}}@_$!post"[ranges[3].start..ranges[3].end], "$!");
    }

    #[test]
    fn skips_escapes_comments_classes_and_excluded_ranges() {
        assert!(find_interpolations(r"(?# $skip )$live", &[]).len() == 1);
        assert_eq!(
            &r"(?# $skip )$live"[find_interpolations(r"(?# $skip )$live", &[])[0].start
                ..find_interpolations(r"(?# $skip )$live", &[])[0].end],
            "$live"
        );
        assert!(find_interpolations(r"\Q$name\E$live", &[]).len() == 1);
        assert!(find_interpolations(r"[$no]$live", &[]).len() == 1);
        let excluded = [RegexRange::new(0, 5).expect("prefix")];
        let ranges = find_interpolations(r"$hide$live", &excluded);
        assert_eq!(ranges.len(), 1);
        assert_eq!(&r"$hide$live"[ranges[0].start..ranges[0].end], "$live");
    }

    #[test]
    fn rejects_non_interpolating_sigil_neighbors() {
        assert!(find_interpolations("$ ", &[]).is_empty());
        assert!(find_interpolations(r"\$name", &[]).is_empty());
        assert!(find_interpolations("@!", &[]).is_empty());
    }
}
