/// Markdown wrapper `extract_link_display` puts around every rendered `L<>`.
const MARKDOWN_LINK_WRAPPER: &str = "[](perldoc://)";
/// Markup prefix that starts a POD `L<>` code. The formatter also requires a
/// third source byte (`i + 2 < len` in `strip_pod_formatting_depth_links`),
/// so a two-byte `L<` is copied literally and never wraps.
const MIN_LINK_MARKUP: &str = "L<";

/// Conservative upper bound on a Markdown-rendered POD field (`description`).
///
/// Derived from `strip_pod_formatting` / `extract_link_display`, not chosen
/// as a corpus-passing multiplier:
///
/// - `encode_pod_link_target` emits at most 3 bytes per source byte (`%XX`).
/// - `escape_markdown_link_text` emits at most 2 bytes per display character
///   (`\\`, `[`, `]`). Display and target are both taken from the link inner
///   text, so a byte may contribute to both expansions.
/// - Each rendered link adds a 14-byte wrapper (`[](perldoc://)`).
/// - Entering the formatter requires at least 3 source bytes (`L` + `<` +
///   one more). Reached worst-case examples include matched empty `L<>` →
///   `[](perldoc://)` (14/3) and unmatched `L<\` → escaped/percent-encoded
///   wrap (19/3). Nested `L<>` is display-text-stripped then wrapped once
///   (#18135). `=item` injects `"- "` (2 bytes) in place of a `=item`
///   directive (≥5 bytes).
///
/// Using `wrapper_len / 2` as the per-byte factor is therefore a
/// **conservative** closed form (`7n`), not a tight ratio of a two-byte
/// `L<` (that input never wraps). It still sits below a 16n multiplier:
/// duplicating a reached `L<>` render (28B) exceeds `bound(3) = 21`.
///
/// Closed form: `bound(n) = n * (wrapper_len / min_link_markup_len) = 7n`.
fn markdown_rendered_field_bound(source_len: usize) -> usize {
    source_len.saturating_mul(MARKDOWN_LINK_WRAPPER.len() / MIN_LINK_MARKUP.len())
}
