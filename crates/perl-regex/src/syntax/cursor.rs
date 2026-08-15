pub(crate) fn quoted_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'\\') || bytes.get(start + 1) != Some(&b'Q') {
        return None;
    }

    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' && bytes[i + 1] == b'E' {
            return Some(i + 2);
        }
        i += 1;
    }
    Some(bytes.len())
}
