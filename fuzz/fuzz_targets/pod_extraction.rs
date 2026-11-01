#![no_main]

use libfuzzer_sys::fuzz_target;
use perl_pod::{extract_pod, PodDoc};

const MAX_INPUT_BYTES: usize = 4096;

fn bounded_utf8_lossy(data: &[u8]) -> std::borrow::Cow<'_, str> {
    let capped = if data.len() <= MAX_INPUT_BYTES { data } else { &data[..MAX_INPUT_BYTES] };
    String::from_utf8_lossy(capped)
}

/// Markdown wrapper `extract_link_display` puts around every rendered `L<>`.
const MARKDOWN_LINK_WRAPPER: &str = "[](perldoc://)";
/// Fewest source bytes that can emit one wrapper: unmatched `L<`.
const MIN_LINK_MARKUP: &str = "L<";

/// Upper bound on a Markdown-rendered POD field (`description` today).
///
/// Derived from `perl-pod`'s `strip_pod_formatting` / `extract_link_display`,
/// not chosen as a corpus-passing multiplier. Keep in lockstep with
/// `markdown_rendered_field_bound` in
/// `crates/perl-pod/tests/pod_extraction_tests.rs` (#15251):
///
/// - `encode_pod_link_target` emits at most 3 bytes per source byte (`%XX`).
/// - `escape_markdown_link_text` emits at most 2 bytes per display character
///   (`\\`, `[`, `]`). Display and target both come from the link inner text.
/// - Each rendered link adds a 14-byte wrapper (`[](perldoc://)`).
/// - A link consumes at least 2 source bytes (`L<`). The wrapper-dominated
///   empty unmatched link `L<` → `[](perldoc://)` is 14/2 = 7 output bytes
///   per source byte, which also covers `3n + 2n` content expansion plus
///   wrapper. Nested `L<>` is display-text-stripped then wrapped once
///   (#18135), so nesting cannot beat this linear ceiling.
///
/// Closed form: `bound(n) = n * (wrapper_len / min_link_markup_len) = 7n`.
fn markdown_rendered_field_bound(source_len: usize) -> usize {
    source_len.saturating_mul(MARKDOWN_LINK_WRAPPER.len() / MIN_LINK_MARKUP.len())
}

fn assert_doc_well_formed(doc: &PodDoc, source: &str) {
    // Display-text fields cannot exceed the source. DESCRIPTION is the sole
    // markdown field (#17984): percent-encoded L<> targets may grow past
    // source length, but not past the renderer-derived ceiling (#15251).
    let len = source.len();
    if let Some(name) = &doc.name {
        assert!(name.len() <= len, "name field longer than source");
    }
    if let Some(syn) = &doc.synopsis {
        assert!(syn.len() <= len, "synopsis field longer than source");
    }
    if let Some(desc) = &doc.description {
        assert!(
            desc.len() <= markdown_rendered_field_bound(len),
            "description field longer than renderer-derived markdown bound"
        );
    }
    // Method keys and bodies are plain display text (#17984). The same
    // expanding L<> input that DESCRIPTION renders as markdown cannot grow
    // a method body past source (fuzz pass 2 wraps input in both
    // `=head1 DESCRIPTION` and `=head2 method_a`), so these arms keep the
    // strong cap rather than the markdown ceiling.
    for (key, body) in &doc.methods {
        assert!(key.len() <= len, "method key longer than source");
        assert!(body.len() <= len, "method body longer than source");
    }
}

fuzz_target!(|data: &[u8]| {
    let input = bounded_utf8_lossy(data);

    // Pass 1: arbitrary input. Must not panic on any byte sequence - POD
    // extraction is a permissive line-oriented scanner that historically has
    // had off-by-one trouble with truncated directives like `=cut\0` or `=head`
    // without a following digit.
    let doc1 = extract_pod(&input);
    assert_doc_well_formed(&doc1, &input);
    let _ = doc1.is_empty();

    // Pass 2: extract from a well-formed POD wrapper around the input. This
    // exercises the *transition* code paths (`=pod` start, `=cut` end, body
    // accumulation across `=head` boundaries) that the raw input rarely hits.
    let wrapped = format!(
        "=pod\n\n=head1 NAME\n\nFuzz::Module - {input}\n\n=head1 SYNOPSIS\n\n  {input}\n\n=head1 DESCRIPTION\n\n{input}\n\n=head2 method_a\n\n{input}\n\n=cut\n"
    );
    let doc2 = extract_pod(&wrapped);
    assert_doc_well_formed(&doc2, &wrapped);

    // Pass 3: list-oriented POD (=over / =item / =back) is a separate state
    // machine inside the extractor; make sure arbitrary content inside lists
    // doesn't break it.
    let listy = format!(
        "=head1 DESCRIPTION\n\n=over 4\n\n=item *\n\n{input}\n\n=item *\n\n{input}\n\n=back\n\n=cut\n"
    );
    let doc3 = extract_pod(&listy);
    assert_doc_well_formed(&doc3, &listy);

    // Pass 4: multiple =cut/restart cycles. Some past bugs only surfaced when
    // POD reopened after closing.
    let restarting = format!(
        "=head1 NAME\n\n{input}\n\n=cut\n\nsub code {{}}\n\n=head1 NAME\n\n{input}\n\n=cut\n"
    );
    let doc4 = extract_pod(&restarting);
    assert_doc_well_formed(&doc4, &restarting);
});
