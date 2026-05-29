#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use perl_uri::{is_file_uri, is_special_scheme, normalize_uri, uri_extension, uri_key};
use perl_workspace::{
    discovery::is_perl_discovery_path,
    folder::{
        extract_workspace_folder_change, extract_workspace_folder_uris, root_path_to_file_uri,
        workspace_folder_to_path,
    },
    ignore::{is_skipped_dir_name, path_contains_skipped_component},
};
use serde_json::json;

const MAX_INPUT_BYTES: usize = 2048;
const MAX_SNIPPET_CHARS: usize = 256;

fn bounded_utf8_lossy(data: &[u8]) -> std::borrow::Cow<'_, str> {
    let capped = if data.len() <= MAX_INPUT_BYTES { data } else { &data[..MAX_INPUT_BYTES] };
    String::from_utf8_lossy(capped)
}

fn truncate_chars(input: &str) -> String {
    input.chars().take(MAX_SNIPPET_CHARS).filter(|ch| *ch != '\0').collect()
}

fn exercise_uri_surfaces(input: &str) {
    let candidates = [
        input.to_string(),
        format!("file:///{input}"),
        format!("file://localhost/{input}"),
        format!("file://127.0.0.1/{input}"),
        format!("file://[::1]/{input}"),
        format!("file://remote.example/{input}"),
        format!("untitled:{input}"),
        format!("git:{input}"),
        format!("vscode-vfs://authority/{input}"),
        format!("C:\\{input}\\lib\\Module.pm"),
        format!("file://C:\\{input}\\lib\\Module.pm"),
        format!("/tmp/{input}/lib/Module.pm"),
        format!("relative/{input}/script.pl"),
    ];

    for candidate in candidates {
        let normalized = normalize_uri(&candidate);
        let key = uri_key(&candidate);
        let normalized_key = uri_key(&normalized);

        let _ = is_file_uri(&candidate);
        let _ = is_special_scheme(&candidate);
        let _ = uri_extension(&candidate);
        let _ = uri_extension(&normalized);
        let _ = workspace_folder_to_path(&candidate);
        let _ = root_path_to_file_uri(&candidate);

        // Normalization and keying should be stable enough for repeated calls.
        assert_eq!(normalize_uri(&normalized), normalized, "normalize_uri should be idempotent");
        assert_eq!(uri_key(&key), key, "uri_key should be idempotent");
        assert_eq!(
            uri_key(&normalized_key),
            normalized_key,
            "normalized uri_key should be idempotent"
        );
    }
}

fn exercise_workspace_folder_json(input: &str) {
    let event = json!({
        "added": [
            input,
            { "uri": format!("file:///{input}") },
            { "path": format!("/tmp/{input}") },
            { "name": input },
            42,
            null
        ],
        "removed": [
            { "uri": format!("file://localhost/{input}") },
            { "path": format!("relative/{input}") },
            input,
            false
        ],
        "ignored": input
    });

    let folders =
        event.get("added").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    let extracted = extract_workspace_folder_uris(&folders);
    let change = extract_workspace_folder_change(&event);

    for uri in extracted.iter().chain(change.added.iter()).chain(change.removed.iter()) {
        let _ = workspace_folder_to_path(uri);
        let _ = normalize_uri(uri);
    }

    assert_eq!(
        extract_workspace_folder_change(&json!({})),
        Default::default(),
        "missing workspace folder change sections should be empty"
    );
}

fn exercise_path_surfaces(input: &str) {
    let path_candidates = [
        input.to_string(),
        format!("lib/{input}/Module.pm"),
        format!("lib/{input}/script.pl"),
        format!("t/{input}/basic.t"),
        format!("templates/{input}/page.html.ep"),
        format!("templates/{input}/layout.tt2"),
        format!("xs/{input}/Native.xs"),
        format!("swig/{input}/Native.i"),
        format!(".git/{input}/ignored.pm"),
        format!("target/{input}/ignored.pl"),
        format!("node_modules/{input}/ignored.t"),
    ];

    for path_string in path_candidates {
        let path = Path::new(&path_string);
        let contains_skipped = path_contains_skipped_component(path);
        let _ = is_perl_discovery_path(path);

        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                if name.to_str().is_some_and(is_skipped_dir_name) {
                    assert!(contains_skipped, "skipped component should mark containing path");
                }
            }
        }
    }

    for canonical in
        [".git", ".hg", ".svn", "target", "node_modules", ".cache", "blib", "local", "vendor"]
    {
        assert!(is_skipped_dir_name(canonical), "canonical skipped directory must stay skipped");
    }
}

fuzz_target!(|data: &[u8]| {
    let input = bounded_utf8_lossy(data);
    let snippet = truncate_chars(&input);

    exercise_uri_surfaces(&snippet);
    exercise_workspace_folder_json(&snippet);
    exercise_path_surfaces(&snippet);
});
