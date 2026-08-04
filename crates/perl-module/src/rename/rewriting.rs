/// Replace `old_module::` namespace prefixes in `line` with `new_module::`.
#[must_use]
pub fn replace_module_name_prefix(line: &str, old_module: &str, new_module: &str) -> String {
    if old_module.is_empty() || new_module.is_empty() || line.is_empty() {
        return line.to_string();
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with("package ")
        || trimmed.starts_with("use ")
        || trimmed.starts_with("require ")
        || trimmed.starts_with("no ")
    {
        return line.to_string();
    }

    let mut out = line.to_string();

    for separator in ["::", "'"] {
        let needle = format!("{old_module}{separator}");
        let replacement = format!("{new_module}{separator}");
        let needle_bytes = needle.as_bytes();
        let needle_len = needle_bytes.len();
        let line_bytes = out.as_bytes();

        if line_bytes.len() < needle_len {
            continue;
        }

        let mut replaced = String::with_capacity(out.len());
        let mut cursor = 0usize;

        while cursor + needle_len <= line_bytes.len() {
            let Some(rel) = out[cursor..].find(needle.as_str()) else {
                break;
            };
            let abs = cursor + rel;
            let after = abs + needle_len;

            let before_ok = abs == 0 || {
                // Check the byte directly — identifier boundary characters
                // (alphanumeric, _, :) are all ASCII, so we can safely check
                // the raw byte without a byte-to-char cast (#2371).
                let b = line_bytes[abs - 1];
                !b.is_ascii_alphanumeric() && b != b'_' && b != b':'
            };

            let after_ok = after < line_bytes.len() && {
                let b = line_bytes[after];
                b.is_ascii_alphabetic() || b == b'_'
            };

            if before_ok && after_ok && !index_is_in_quote_or_comment(&out, abs) {
                replaced.push_str(&out[cursor..abs]);
                replaced.push_str(&replacement);
                cursor = after;
            } else {
                replaced.push_str(&out[cursor..abs + 1]);
                cursor = abs + 1;
            }
        }

        replaced.push_str(&out[cursor..]);
        out = replaced;
    }

    out
}

pub(super) fn index_is_in_quote_or_comment(line: &str, index: usize) -> bool {
    let bytes = line.as_bytes();
    if index >= bytes.len() {
        return false;
    }

    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (i, &byte) in bytes.iter().enumerate() {
        if i == index {
            return in_single || in_double;
        }

        // Use ASCII byte comparison for quote/escape characters (#2371).
        // These are all ASCII so the raw byte check is correct and avoids
        // the invalid byte-to-char cast.
        if escaped {
            escaped = false;
            continue;
        }

        if in_single {
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'\'' {
                in_single = false;
            }
            continue;
        }

        if in_double {
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                in_double = false;
            }
            continue;
        }

        if byte == b'#' {
            return i < index;
        }

        if byte == b'\'' {
            in_single = true;
            continue;
        }

        if byte == b'"' {
            in_double = true;
        }
    }

    false
}
