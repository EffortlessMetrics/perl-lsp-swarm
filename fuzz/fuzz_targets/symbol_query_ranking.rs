#![no_main]

use libfuzzer_sys::fuzz_target;
use perl_lsp_rs_core::providers::symbol_query::matches_query;
use perl_workspace::workspace_symbol_query::{
    match_searchable_key, WorkspaceSymbolQueryProfile, WorkspaceSymbolSearchKeyRole,
};

const MAX_INPUT_BYTES: usize = 2048;
const MAX_TOKENS: usize = 64;
const MAX_TOKEN_CHARS: usize = 64;

fn bounded_utf8_lossy(data: &[u8]) -> std::borrow::Cow<'_, str> {
    if data.is_empty() {
        return std::borrow::Cow::Borrowed("");
    }

    let capped = if data.len() <= MAX_INPUT_BYTES {
        data
    } else {
        &data[..MAX_INPUT_BYTES]
    };

    String::from_utf8_lossy(capped)
}

fn split_tokens(input: &str) -> Vec<String> {
    input
        .split(['\n', '\0', '|', ',', ';'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.chars().take(MAX_TOKEN_CHARS).collect::<String>())
        .take(MAX_TOKENS)
        .collect()
}

// #10794 repair follow-up: the ranking half previously mutated the removed
// `symbol_query::compare_names_by_query` forwarding shim. The canonical owner
// (`perl_workspace::workspace_symbol_query`) exposes the same mutation
// surface — compile once, admit per key, total deterministic evidence
// comparator — so the harness now drives that API directly. Non-matches do
// not participate in ordering (they are `None` at the API boundary), matching
// every live consumer.
fuzz_target!(|data: &[u8]| {
    let input = bounded_utf8_lossy(data);
    let mut tokens = split_tokens(&input);

    if tokens.is_empty() {
        tokens.push("symbol".to_string());
    }

    for query in &tokens {
        for name in &tokens {
            let _ = matches_query(name, query);
        }
    }

    for query in &tokens {
        let profile = WorkspaceSymbolQueryProfile::compile(query);
        let mut ranked: Vec<_> = tokens
            .iter()
            .filter_map(|name| {
                match_searchable_key(&profile, name, WorkspaceSymbolSearchKeyRole::Other)
                    .map(|evidence| (name.as_str(), evidence))
            })
            .collect();
        ranked.sort_by(|a, b| a.1.compare(&b.1));
        ranked.dedup();

        let mut reversed = ranked.clone();
        reversed.reverse();
        reversed.sort_by(|a, b| a.1.compare(&b.1));
    }
});
