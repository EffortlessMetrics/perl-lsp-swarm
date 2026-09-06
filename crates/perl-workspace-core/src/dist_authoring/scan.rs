//! Comment- and string-aware scan of Perl authoring files.
//!
//! Ranges always point into the original source. The scanner never evaluates
//! expressions, interpolates, or shells out.

/// A statically recovered Perl value, or a typed dynamic hole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScanValue {
    /// A quoted string, bareword, number, or v-string.
    String(String),
    /// An anonymous hash of fat-comma pairs.
    Hash(Vec<ScanPair>),
    /// An anonymous array.
    List(Vec<ScanValue>),
    /// Not a literal; keep a short snippet for evidence.
    Dynamic { snippet: String },
}

/// One `key => value` pair recovered from a hash or argument list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScanPair {
    pub key: String,
    pub key_start: usize,
    pub key_end: usize,
    pub value: ScanValue,
    pub value_start: usize,
    pub value_end: usize,
}

impl ScanValue {
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    pub(crate) fn as_hash(&self) -> Option<&[ScanPair]> {
        match self {
            Self::Hash(pairs) => Some(pairs),
            _ => None,
        }
    }

    pub(crate) fn is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic { .. })
    }
}

pub(crate) fn skip_ws_comments(source: &str, idx: &mut usize) {
    let bytes = source.as_bytes();
    loop {
        while *idx < bytes.len() && bytes[*idx].is_ascii_whitespace() {
            *idx += 1;
        }
        if bytes.get(*idx) == Some(&b'#') {
            while *idx < bytes.len() && bytes[*idx] != b'\n' {
                *idx += 1;
            }
            continue;
        }
        if at_pod_directive(source, *idx) {
            skip_pod(source, idx);
            continue;
        }
        break;
    }
}

pub(crate) fn find_ident(source: &str, name: &str, mut from: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let needle = name.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    while from < bytes.len() {
        let byte = bytes[from];
        if in_single {
            from += 1;
            if byte == b'\\' && from < bytes.len() {
                from += 1;
            } else if byte == b'\'' {
                in_single = false;
            }
            continue;
        }
        if in_double {
            from += 1;
            if byte == b'\\' && from < bytes.len() {
                from += 1;
            } else if byte == b'"' {
                in_double = false;
            }
            continue;
        }
        match byte {
            b'\'' => {
                in_single = true;
                from += 1;
            }
            b'"' => {
                in_double = true;
                from += 1;
            }
            b'#' => {
                while from < bytes.len() && bytes[from] != b'\n' {
                    from += 1;
                }
            }
            _ => {
                if bytes[from..].starts_with(needle) && is_ident_boundary(bytes, from, needle.len())
                {
                    return Some(from);
                }
                from += 1;
            }
        }
    }
    None
}

pub(crate) fn call_open_paren(source: &str, ident_start: usize, ident_len: usize) -> Option<usize> {
    let mut idx = ident_start + ident_len;
    skip_ws_comments(source, &mut idx);
    if source.as_bytes().get(idx) == Some(&b'(') { Some(idx) } else { None }
}

pub(crate) fn parse_paren_hash(source: &str, open_idx: usize) -> Option<(Vec<ScanPair>, usize)> {
    let close = matching_pair(source, open_idx)?;
    let mut idx = open_idx + 1;
    let pairs = parse_pairs(source, &mut idx, close);
    Some((pairs, close + 1))
}

/// Parse a `( ... )` or recover pairs through EOF when the closer is missing.
pub(crate) fn parse_paren_hash_recovering(source: &str, open_idx: usize) -> (Vec<ScanPair>, bool) {
    let closed = matching_pair(source, open_idx);
    let stop = closed.unwrap_or(source.len());
    let mut idx = open_idx + 1;
    (parse_pairs(source, &mut idx, stop), closed.is_some())
}

pub(crate) fn parse_value(source: &str, idx: &mut usize) -> ScanValue {
    parse_value_in(source, idx, false)
}

fn parse_value_in(source: &str, idx: &mut usize, in_list: bool) -> ScanValue {
    skip_ws_comments(source, idx);
    let start = *idx;
    let bytes = source.as_bytes();
    if bytes.get(*idx) == Some(&b'+') {
        let mut next = *idx + 1;
        skip_ws_comments(source, &mut next);
        if bytes.get(next) == Some(&b'{') {
            *idx = next;
        }
    }
    if let Some(literal) = parse_string(source, idx) {
        return match literal {
            Quoted::Scalar(value) => ScanValue::String(value),
            Quoted::Words(words) if words.len() <= 1 => {
                ScanValue::String(words.into_iter().next().unwrap_or_default())
            }
            Quoted::Words(words) if in_list => {
                ScanValue::List(words.into_iter().map(ScanValue::String).collect())
            }
            Quoted::Words(words) => ScanValue::Dynamic { snippet: words.join(" ") },
            Quoted::Dynamic { snippet } => ScanValue::Dynamic { snippet },
        };
    }
    if bytes.get(*idx) == Some(&b'{') {
        return parse_hash_value(source, idx);
    }
    if bytes.get(*idx) == Some(&b'[') {
        return parse_list_value(source, idx);
    }
    if let Some(number) = parse_number_or_vstring(source, idx) {
        return ScanValue::String(number);
    }
    if let Some(bare) = parse_bareword_value(source, idx) {
        skip_ws_comments(source, idx);
        if bytes.get(*idx) == Some(&b'(') {
            *idx = start;
            return take_dynamic(source, idx);
        }
        return ScanValue::String(bare);
    }
    take_dynamic(source, idx)
}

pub(crate) fn snippet(source: &str, start: usize, end: usize) -> String {
    let end = end.min(source.len()).max(start);
    let slice = source.get(start..end).unwrap_or("");
    let trimmed = slice.trim();
    if trimmed.chars().count() <= 48 {
        trimmed.to_string()
    } else {
        let mut out = String::new();
        for (i, ch) in trimmed.chars().enumerate() {
            if i >= 48 {
                break;
            }
            out.push(ch);
        }
        out.push('…');
        out
    }
}

pub(crate) fn matching_pair(source: &str, open_idx: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let open = *bytes.get(open_idx)?;
    let close = match open {
        b'(' => b')',
        b'{' => b'}',
        b'[' => b']',
        _ => return None,
    };
    let mut depth = 0usize;
    let mut idx = open_idx;
    let mut in_single = false;
    let mut in_double = false;
    while idx < bytes.len() {
        if in_single {
            let byte = bytes[idx];
            idx += 1;
            if byte == b'\\' && idx < bytes.len() {
                idx += 1;
            } else if byte == b'\'' {
                in_single = false;
            }
            continue;
        }
        if in_double {
            let byte = bytes[idx];
            idx += 1;
            if byte == b'\\' && idx < bytes.len() {
                idx += 1;
            } else if byte == b'"' {
                in_double = false;
            }
            continue;
        }
        if let Some(after) = skip_quotelike_at(source, idx) {
            idx = after;
            continue;
        }
        match bytes[idx] {
            b'\'' => {
                in_single = true;
                idx += 1;
            }
            b'"' => {
                in_double = true;
                idx += 1;
            }
            b'#' => {
                while idx < bytes.len() && bytes[idx] != b'\n' {
                    idx += 1;
                }
            }
            b if b == open => {
                depth += 1;
                idx += 1;
            }
            b if b == close => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
                idx += 1;
            }
            _ => idx += 1,
        }
    }
    None
}

pub(crate) fn contains_ident(source: &str, name: &str) -> bool {
    find_ident(source, name, 0).is_some()
}

fn parse_pairs(source: &str, idx: &mut usize, stop: usize) -> Vec<ScanPair> {
    let mut pairs = Vec::new();
    while *idx < stop {
        skip_ws_comments(source, idx);
        if *idx >= stop {
            break;
        }
        let bytes = source.as_bytes();
        if matches!(bytes.get(*idx), Some(&b')' | &b'}' | &b']' | &b',')) {
            if bytes.get(*idx) == Some(&b',') {
                *idx += 1;
                continue;
            }
            break;
        }
        let Some((key, key_start, key_end)) = parse_key(source, idx) else {
            take_dynamic(source, idx);
            continue;
        };
        skip_ws_comments(source, idx);
        if bytes.get(*idx) == Some(&b'=') && bytes.get(*idx + 1) == Some(&b'>') {
            *idx += 2;
        } else if bytes.get(*idx) == Some(&b',') {
            *idx += 1;
            continue;
        }
        skip_ws_comments(source, idx);
        let value_start = *idx;
        let value = parse_value(source, idx);
        let value_end = (*idx).min(stop);
        pairs.push(ScanPair { key, key_start, key_end, value, value_start, value_end });
        skip_ws_comments(source, idx);
        if bytes.get(*idx) == Some(&b',') {
            *idx += 1;
        }
    }
    pairs
}

fn parse_key(source: &str, idx: &mut usize) -> Option<(String, usize, usize)> {
    skip_ws_comments(source, idx);
    let start = *idx;
    if let Some(literal) = parse_string(source, idx) {
        let key = match literal {
            Quoted::Scalar(value) => value,
            Quoted::Words(words) => words.into_iter().next().unwrap_or_default(),
            Quoted::Dynamic { .. } => return None,
        };
        return Some((key, start, *idx));
    }
    let bare = parse_bareword_value(source, idx)?;
    Some((bare, start, *idx))
}

fn parse_hash_value(source: &str, idx: &mut usize) -> ScanValue {
    let open = *idx;
    let Some(close) = matching_pair(source, open) else {
        return take_dynamic(source, idx);
    };
    *idx = open + 1;
    let pairs = parse_pairs(source, idx, close);
    *idx = close + 1;
    ScanValue::Hash(pairs)
}

fn parse_list_value(source: &str, idx: &mut usize) -> ScanValue {
    let open = *idx;
    let Some(close) = matching_pair(source, open) else {
        return take_dynamic(source, idx);
    };
    *idx = open + 1;
    let mut items = Vec::new();
    while *idx < close {
        skip_ws_comments(source, idx);
        if *idx >= close {
            break;
        }
        if source.as_bytes().get(*idx) == Some(&b',') {
            *idx += 1;
            continue;
        }
        skip_ws_comments(source, idx);
        let flatten_qw = at_qw(source, *idx);
        match parse_value_in(source, idx, flatten_qw) {
            ScanValue::List(inner) if flatten_qw => items.extend(inner),
            other => items.push(other),
        }
        skip_ws_comments(source, idx);
        if source.as_bytes().get(*idx) == Some(&b',') {
            *idx += 1;
        }
    }
    *idx = close + 1;
    ScanValue::List(items)
}

enum Quoted {
    Scalar(String),
    Words(Vec<String>),
    Dynamic { snippet: String },
}

fn parse_string(source: &str, idx: &mut usize) -> Option<Quoted> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(*idx)?;
    if quote == b'\'' {
        return parse_quoted(source, idx, quote).map(Quoted::Scalar);
    }
    if quote == b'"' {
        return parse_double_quoted(source, idx);
    }
    parse_quotelike(source, idx)
}

fn parse_double_quoted(source: &str, idx: &mut usize) -> Option<Quoted> {
    let inner_start = *idx + 1;
    let mut interpolating = false;
    let bytes = source.as_bytes();
    if bytes.get(*idx) != Some(&b'"') {
        return None;
    }
    let mut value = String::new();
    *idx += 1;
    let mut escaped = false;
    while *idx < source.len() {
        let ch = source[*idx..].chars().next()?;
        *idx += ch.len_utf8();
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            if interpolating {
                return Some(Quoted::Dynamic { snippet: snippet(source, inner_start, *idx - 1) });
            }
            return Some(Quoted::Scalar(value));
        }
        if ch == '$' || ch == '@' {
            interpolating = true;
        }
        value.push(ch);
    }
    if interpolating {
        Some(Quoted::Dynamic { snippet: snippet(source, inner_start, *idx) })
    } else {
        Some(Quoted::Scalar(value))
    }
}

fn parse_quoted(source: &str, idx: &mut usize, quote: u8) -> Option<String> {
    let bytes = source.as_bytes();
    if bytes.get(*idx) != Some(&quote) {
        return None;
    }
    let mut value = String::new();
    *idx += 1;
    let mut escaped = false;
    while *idx < source.len() {
        let ch = source[*idx..].chars().next()?;
        *idx += ch.len_utf8();
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch as u32 == u32::from(quote) {
            return Some(value);
        }
        value.push(ch);
    }
    Some(value)
}

fn parse_quotelike(source: &str, idx: &mut usize) -> Option<Quoted> {
    let bytes = source.as_bytes();
    let start = *idx;
    let kind = if bytes[start..].starts_with(b"qq") {
        *idx += 2;
        "qq"
    } else if bytes[start..].starts_with(b"qw") {
        *idx += 2;
        "qw"
    } else if bytes[start..].starts_with(b"q") {
        *idx += 1;
        "q"
    } else {
        return None;
    };
    if *idx < bytes.len() && (bytes[*idx].is_ascii_alphanumeric() || bytes[*idx] == b'_') {
        *idx = start;
        return None;
    }
    skip_ws_comments(source, idx);
    let open = *bytes.get(*idx)?;
    if open.is_ascii_alphanumeric() || open == b'_' {
        *idx = start;
        return None;
    }
    *idx += 1;
    let close = matching_quotelike_close(open);
    let mut value = String::new();
    let mut depth = 1usize;
    while *idx < source.len() {
        let ch = source[*idx..].chars().next()?;
        let byte = ch as u8;
        if ch.len_utf8() == 1 && byte == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                *idx += 1;
                if kind == "qw" {
                    return Some(Quoted::Words(
                        value.split_whitespace().map(ToOwned::to_owned).collect(),
                    ));
                }
                return Some(finish_quotelike(kind, value));
            }
        } else if ch.len_utf8() == 1 && byte == open && open != close {
            depth += 1;
        }
        value.push(ch);
        *idx += ch.len_utf8();
    }
    if kind == "qw" {
        Some(Quoted::Words(value.split_whitespace().map(ToOwned::to_owned).collect()))
    } else {
        Some(finish_quotelike(kind, value))
    }
}

fn finish_quotelike(kind: &str, value: String) -> Quoted {
    if kind == "qq" && has_interpolation(&value) {
        Quoted::Dynamic { snippet: value }
    } else {
        Quoted::Scalar(value)
    }
}

fn has_interpolation(raw: &str) -> bool {
    let mut escaped = false;
    for ch in raw.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '$' || ch == '@' {
            return true;
        }
    }
    false
}

fn matching_quotelike_close(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'{' => b'}',
        b'[' => b']',
        b'<' => b'>',
        other => other,
    }
}

fn skip_quotelike_at(source: &str, idx: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = idx;
    if bytes[idx..].starts_with(b"qq") || bytes[idx..].starts_with(b"qw") {
        cursor += 2;
    } else if bytes[idx..].starts_with(b"q") {
        cursor += 1;
    } else {
        return None;
    }
    if idx > 0 && is_ident_byte(bytes[idx - 1]) {
        return None;
    }
    if cursor < bytes.len() && is_ident_byte(bytes[cursor]) {
        return None;
    }
    let mut probe = cursor;
    skip_ws_comments(source, &mut probe);
    let open = *bytes.get(probe)?;
    if open.is_ascii_alphanumeric() || open == b'_' {
        return None;
    }
    let close = matching_quotelike_close(open);
    probe += 1;
    let mut depth = 1usize;
    while probe < bytes.len() {
        let byte = bytes[probe];
        if byte == close {
            depth = depth.saturating_sub(1);
            probe += 1;
            if depth == 0 {
                return Some(probe);
            }
        } else if byte == open && open != close {
            depth += 1;
            probe += 1;
        } else {
            probe += 1;
        }
    }
    None
}

fn parse_number_or_vstring(source: &str, idx: &mut usize) -> Option<String> {
    let bytes = source.as_bytes();
    let start = *idx;
    if bytes.get(*idx) == Some(&b'v') {
        let mut end = *idx + 1;
        if end < bytes.len() && bytes[end].is_ascii_digit() {
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                end += 1;
            }
            *idx = end;
            return Some(source[start..end].to_string());
        }
        return None;
    }
    if bytes.get(*idx).is_some_and(|b| b.is_ascii_digit()) {
        let mut end = *idx;
        while end < bytes.len()
            && (bytes[end].is_ascii_digit() || bytes[end] == b'.' || bytes[end] == b'_')
        {
            end += 1;
        }
        *idx = end;
        return Some(source[start..end].replace('_', ""));
    }
    None
}

fn parse_bareword_value(source: &str, idx: &mut usize) -> Option<String> {
    let bytes = source.as_bytes();
    let start = *idx;
    let first = *bytes.get(*idx)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let mut end = *idx + 1;
    while end < bytes.len() && (is_ident_byte(bytes[end]) || bytes[end] == b':') {
        end += 1;
    }
    *idx = end;
    Some(source[start..end].to_string())
}

fn take_dynamic(source: &str, idx: &mut usize) -> ScanValue {
    skip_ws_comments(source, idx);
    let start = *idx;
    skip_balanced_value(source, idx);
    ScanValue::Dynamic { snippet: snippet(source, start, *idx) }
}

fn skip_balanced_value(source: &str, idx: &mut usize) {
    let bytes = source.as_bytes();
    let mut depth_paren = 0usize;
    let mut depth_brace = 0usize;
    let mut depth_bracket = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut started = false;
    while *idx < bytes.len() {
        if in_single {
            let byte = bytes[*idx];
            *idx += 1;
            if byte == b'\\' && *idx < bytes.len() {
                *idx += 1;
            } else if byte == b'\'' {
                in_single = false;
            }
            continue;
        }
        if in_double {
            let byte = bytes[*idx];
            *idx += 1;
            if byte == b'\\' && *idx < bytes.len() {
                *idx += 1;
            } else if byte == b'"' {
                in_double = false;
            }
            continue;
        }
        match bytes[*idx] {
            b'\'' => {
                in_single = true;
                *idx += 1;
                started = true;
            }
            b'"' => {
                in_double = true;
                *idx += 1;
                started = true;
            }
            b'#' => {
                while *idx < bytes.len() && bytes[*idx] != b'\n' {
                    *idx += 1;
                }
            }
            b'(' => {
                depth_paren += 1;
                *idx += 1;
                started = true;
            }
            b')' if depth_paren > 0 => {
                depth_paren -= 1;
                *idx += 1;
                started = true;
            }
            b'{' => {
                depth_brace += 1;
                *idx += 1;
                started = true;
            }
            b'}' if depth_brace > 0 => {
                depth_brace -= 1;
                *idx += 1;
                started = true;
            }
            b'[' => {
                depth_bracket += 1;
                *idx += 1;
                started = true;
            }
            b']' if depth_bracket > 0 => {
                depth_bracket -= 1;
                *idx += 1;
                started = true;
            }
            b',' | b')' | b'}' | b']'
                if depth_paren == 0 && depth_brace == 0 && depth_bracket == 0 =>
            {
                return;
            }
            _ => {
                *idx += 1;
                started = true;
            }
        }
        if started && depth_paren == 0 && depth_brace == 0 && depth_bracket == 0 {
            let next = bytes.get(*idx).copied();
            if matches!(next, Some(b',') | Some(b')') | Some(b'}') | Some(b']') | None)
                || next.is_some_and(|b| b.is_ascii_whitespace())
            {
                return;
            }
        }
    }
}

fn at_qw(source: &str, idx: usize) -> bool {
    let bytes = source.as_bytes();
    if !bytes.get(idx..).is_some_and(|rest| rest.starts_with(b"qw")) {
        return false;
    }
    if idx > 0 && is_ident_byte(bytes[idx - 1]) {
        return false;
    }
    let after = idx + 2;
    after >= bytes.len() || !is_ident_byte(bytes[after])
}

fn is_ident_boundary(bytes: &[u8], start: usize, len: usize) -> bool {
    let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
    let after = start + len;
    let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
    before_ok && after_ok
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn at_pod_directive(source: &str, idx: usize) -> bool {
    let bytes = source.as_bytes();
    if bytes.get(idx) != Some(&b'=') {
        return false;
    }
    let at_line_start = idx == 0 || bytes.get(idx - 1) == Some(&b'\n');
    at_line_start && bytes[idx..].starts_with(b"=pod")
}

fn skip_pod(source: &str, idx: &mut usize) {
    let bytes = source.as_bytes();
    while *idx < bytes.len() {
        if at_line_start(bytes, *idx) && bytes[*idx..].starts_with(b"=cut") {
            while *idx < bytes.len() && bytes[*idx] != b'\n' {
                *idx += 1;
            }
            return;
        }
        *idx += 1;
    }
}

fn at_line_start(bytes: &[u8], idx: usize) -> bool {
    idx == 0 || bytes.get(idx.wrapping_sub(1)) == Some(&b'\n')
}
