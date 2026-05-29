#![no_main]

use libfuzzer_sys::fuzz_target;
use perl_uri::{
    fs_path_to_uri, is_file_uri, is_special_scheme, normalize_uri, source_path_from_uri_or_path,
    uri_extension, uri_key, uri_to_fs_path,
};
use std::path::PathBuf;

const MAX_INPUT_BYTES: usize = 1024;
const MAX_URI_CHARS: usize = 192;

fn bounded_utf8_lossy(data: &[u8]) -> std::borrow::Cow<'_, str> {
    if data.is_empty() {
        return std::borrow::Cow::Borrowed("");
    }

    let capped = if data.len() <= MAX_INPUT_BYTES { data } else { &data[..MAX_INPUT_BYTES] };
    String::from_utf8_lossy(capped)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn percentish(input: &str) -> String {
    let mut encoded = String::new();
    for ch in input.chars().take(64) {
        match ch {
            ' ' => encoded.push_str("%20"),
            '#' => encoded.push_str("%23"),
            '?' => encoded.push_str("%3F"),
            '%' => encoded.push_str("%25"),
            '/' | '\\' | ':' => encoded.push('_'),
            ch if ch.is_control() => encoded.push('_'),
            ch => encoded.push(ch),
        }
    }
    encoded
}

fn exercise_uri(input: &str) {
    let normalized = normalize_uri(input);
    let key = uri_key(input);
    let normalized_key = uri_key(&normalized);

    let _ = is_file_uri(input);
    let _ = is_special_scheme(input);
    let _ = uri_extension(input);
    let _ = uri_extension(&normalized);
    let _ = uri_to_fs_path(input);
    let _ = uri_to_fs_path(&normalized);
    let _ = source_path_from_uri_or_path(input);
    let _ = source_path_from_uri_or_path(&normalized);

    // Idempotency-style probes: these should not panic even when the original
    // text is malformed or when normalization returns a non-URI fallback.
    let _ = normalize_uri(&normalized);
    let _ = uri_key(&key);
    let _ = uri_key(&normalized_key);
}

fuzz_target!(|data: &[u8]| {
    let input = bounded_utf8_lossy(data);
    let short = truncate_chars(&input, MAX_URI_CHARS);
    let encoded = percentish(&short);

    let variants = [
        short.clone(),
        format!("file:///tmp/{encoded}.pl"),
        format!("file://localhost/tmp/{encoded}.pm"),
        format!("file:///C:/{encoded}.t"),
        format!("untitled:{short}"),
        format!("perl-lsp://workspace/{encoded}"),
        format!(" {short}\n"),
        format!("/tmp/{encoded}.pl"),
        format!("./{encoded}.pm"),
        format!("https://example.invalid/{encoded}?q={encoded}#frag"),
    ];

    for variant in &variants {
        exercise_uri(variant);
    }

    let paths = [
        PathBuf::from(format!("/tmp/{encoded}.pl")),
        PathBuf::from(format!("relative/{encoded}.pm")),
        PathBuf::from(format!("C:/{encoded}.t")),
    ];

    for path in &paths {
        if let Ok(uri) = fs_path_to_uri(path) {
            exercise_uri(&uri);
            let _ = uri_to_fs_path(&uri);
        }
    }
});
