//! String-format helpers for native critic rules.

pub(super) fn count_format_specifiers(format: &str) -> usize {
    let bytes = format.as_bytes();
    let mut count = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }

        index += 1;
        if index >= bytes.len() {
            break;
        }
        if bytes[index] == b'%' {
            index += 1;
            continue;
        }

        while index < bytes.len() && matches!(bytes[index], b'-' | b'+' | b' ' | b'0' | b'#') {
            index += 1;
        }
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'*' {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            if index < bytes.len() && bytes[index] == b'*' {
                index += 1;
            }
        }
        if index < bytes.len()
            && matches!(bytes[index], b'h' | b'l' | b'L' | b'q' | b'v' | b'z' | b't')
        {
            index += 1;
            if index < bytes.len() && matches!(bytes[index], b'h' | b'l') {
                index += 1;
            }
        }
        if index < bytes.len()
            && matches!(
                bytes[index],
                b's' | b'd'
                    | b'i'
                    | b'u'
                    | b'o'
                    | b'x'
                    | b'X'
                    | b'e'
                    | b'E'
                    | b'f'
                    | b'F'
                    | b'g'
                    | b'G'
                    | b'c'
                    | b'p'
                    | b'n'
                    | b'b'
            )
        {
            count += 1;
        }
        index += 1;
    }

    count
}

pub(super) fn unquote_string(raw: &str) -> &str {
    if raw.len() >= 2 {
        let bytes = raw.as_bytes();
        let first = bytes[0];
        let last = bytes[raw.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &raw[1..raw.len() - 1];
        }
    }

    raw
}
