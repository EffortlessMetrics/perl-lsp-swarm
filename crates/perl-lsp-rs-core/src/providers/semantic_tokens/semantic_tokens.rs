//! Semantic token analysis for LSP syntax highlighting in Perl script processing
//!
//! This module provides semantic token extraction and classification for Perl scripts
//! within the LSP workflow. It generates precise syntax highlighting information that helps
//! developers understand complex Perl code during the Complete stage.
//!
//! # LSP Workflow Integration
//!
//! - **Parse**: Receives parsed AST from Perl script parsing
//! - **Index**: Uses semantic information for symbol indexing
//! - **Navigate**: Applies semantic analysis for cross-file navigation
//! - **Complete**: Primary consumer - provides syntax highlighting for code presentation
//! - **Analyze**: Uses semantic classification for enhanced search and analysis
//!
//! # Client capability requirements
//!
//! Requires client capability support for `textDocument/semanticTokens` and
//! `semanticTokens/legend` registration to enable semantic highlighting.
//!
//! # Protocol compliance
//!
//! Implements the semanticTokens protocol (full and delta) with LSP 3.17+
//! data layout and delta encoding expectations.
//!
//! # Related Modules
//!
//! This module integrates with symbol indexing, semantic analysis, and code completion.
//!
//! # Performance Characteristics
//!
//! - Memory usage: O(n) where n is token count in Perl script
//! - Time complexity: O(n) linear scanning with lexer integration
//! - Optimized for large Perl codebase processing with efficient token classification
//! - Thread-safe semantic token generation for concurrent script processing
//!
//! # Usage Examples
//!
//! ## Basic Semantic Token Generation
//!
//! ```ignore
//! use perl_lsp_providers::{Parser, ide::lsp_compat::semantic_tokens::collect_semantic_tokens};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let code = "package MyModule; sub greet { my $name = shift; print \"Hello, $name!\"; }";
//! let mut parser = Parser::new(code);
//! let ast = parser.parse()?;
//!
//! // Generate semantic tokens for syntax highlighting
//! let to_pos16 = |byte_pos: usize| {
//!     // Simple line/column calculation for demonstration
//!     let line = code[..byte_pos].matches('\n').count() as u32;
//!     let last_line = code[..byte_pos].rfind('\n').map_or(0, |pos| pos + 1);
//!     let col = (byte_pos - last_line) as u32;
//!     (line, col)
//! };
//! let tokens = collect_semantic_tokens(&ast, code, &to_pos16);
//! for token in tokens {
//!     println!("Token: [{}, {}, {}, {}, {}]",
//!              token[0], token[1], token[2], token[3], token[4]);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## LSP Semantic Tokens Provider
//!
//! ```ignore
//! use perl_lsp_providers::ide::lsp_compat::semantic_tokens::{collect_semantic_tokens, legend};
//! use perl_lsp_providers::Parser;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let code = "my @array = (1, 2, 3); for my $item (@array) { print $item; }";
//! let mut parser = Parser::new(code);
//! let ast = parser.parse()?;
//!
//! // Get encoded tokens for LSP response
//! let to_pos16 = |byte_pos: usize| {
//!     let line = code[..byte_pos].matches('\n').count() as u32;
//!     let last_line = code[..byte_pos].rfind('\n').map_or(0, |pos| pos + 1);
//!     let col = (byte_pos - last_line) as u32;
//!     (line, col)
//! };
//! let encoded_tokens = collect_semantic_tokens(&ast, code, &to_pos16);
//! let legend = legend();
//!
//! println!("Generated {} semantic tokens", encoded_tokens.len());
//! println!("Token types: {:?}", legend.token_types);
//! println!("Token modifiers: {:?}", legend.modifiers);
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom Token Classification
//!
//! ```ignore
//! use perl_lsp_providers::ide::lsp_compat::semantic_tokens::{EncodedToken, TokensLegend, legend};
//!
//! // Create custom semantic tokens
//! let custom_token: EncodedToken = [0, 0, 5, 1, 0];
//! // Structure: [delta_line, delta_start, length, token_type, token_modifiers]
//!
//! // Use with existing legend
//! let legend = legend();
//! println!("Token type: {:?}", legend.token_types.get(custom_token[3] as usize));
//! ```

use perl_lexer::{PerlLexer, StringPart, TokenType};
use perl_parser_core::ast::{Node, NodeKind};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::sync::LazyLock;

/// Undelta-encoded semantic token `(line, character, length, type, modifiers)`.
pub type RawSemanticToken = (u32, u32, u32, u32, u32);

/// Maximum source bytes admitted to the opaque lexer in a bounded traversal.
///
/// The compatibility collector remains unlimited. A caller that supplies a
/// cancellation callback or budget receives `SourceLimitExceeded` before
/// lexing a larger source, so one `PerlLexer::next_token` call can never hide
/// more than 256 KiB of source scanning from the caller's polling authority.
pub const MAX_BOUNDED_LEXER_SOURCE_BYTES: usize = 256 * 1024;

/// LSP semantic token encoding format for client transmission
///
/// Represents a semantic token as [deltaLine, deltaStartChar, length, tokenTypeIndex, tokenModBits]
/// following the LSP specification for efficient delta-encoded token streams.
pub type EncodedToken = [u32; 5];

/// Semantic token legend mapping token types and modifiers to indices
///
/// Provides the mapping between semantic token names and their numeric indices
/// for LSP client consumption. Used to establish a contract between the server
/// and client for semantic highlighting interpretation.
pub struct TokensLegend {
    /// List of token type names in index order
    pub token_types: Vec<String>,
    /// List of modifier names in index order
    pub modifiers: Vec<String>,
    /// Fast lookup map from token type names to indices
    pub map: FxHashMap<String, u32>,
}

/// Create the standard semantic token legend for Perl script highlighting
///
/// Returns a configured legend with all supported token types and modifiers
/// for comprehensive Perl script syntax highlighting. Optimized for common
/// Perl constructs found in Perl parsing workflows.
///
/// # Returns
///
/// A TokensLegend containing all token types, modifiers, and lookup mappings
/// ready for LSP client registration and semantic token classification.
///
/// # Examples
///
/// ```rust,ignore
/// use perl_lsp_providers::ide::lsp_compat::semantic_tokens::legend;
///
/// let legend = legend();
/// assert!(legend.token_types.contains(&"function".to_string()));
/// assert!(legend.token_types.contains(&"keyword".to_string()));
/// ```
pub fn legend() -> TokensLegend {
    // IMPORTANT: this ordering must exactly match the token_types vec in
    // `perl-lsp-protocol/src/capabilities.rs` `capabilities_for()`.
    // Clients decode emitted tokenType indices using the advertised legend;
    // any ordering mismatch renders every token with the wrong colour.
    let types = vec![
        "namespace",           // 0
        "type",                // 1
        "class",               // 2
        "interface",           // 3
        "enum",                // 4
        "enumMember",          // 5
        "typeParameter",       // 6
        "function",            // 7
        "method",              // 8
        "property",            // 9
        "macro",               // 10
        "variable",            // 11
        "parameter",           // 12
        "keyword",             // 13
        "modifier",            // 14
        "comment",             // 15
        "string",              // 16
        "number",              // 17
        "regexp",              // 18
        "operator",            // 19
        "sql_string",          // 20 — DBI/SQL string context (Issue #2337)
        "sql_heredoc_keyword", // 21 — SQL keyword inside <<SQL heredoc (Issue #2059)
        "json_heredoc_key",    // 22 — JSON object key inside <<JSON heredoc (Issue #2059)
        "label",               // 23 — Perl statement/control labels (LSP 3.18)
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();

    // IMPORTANT: this ordering must exactly match the token_modifiers vec in
    // `perl-lsp-protocol/src/capabilities.rs` `capabilities_for()`.
    // Modifier bitmasks (1 << bit_position) are decoded using the advertised legend.
    // Each modifier's numeric value is 2^bit_position (e.g., defaultLibrary at bit 9 = 512).
    let modifiers = vec![
        "declaration",    // bit 0  → 1
        "definition",     // bit 1  → 2
        "readonly",       // bit 2  → 4
        "static",         // bit 3  → 8
        "deprecated",     // bit 4  → 16
        "abstract",       // bit 5  → 32
        "async",          // bit 6  → 64
        "modification",   // bit 7  → 128
        "documentation",  // bit 8  → 256
        "defaultLibrary", // bit 9  → 512
        "scalarVariable", // bit 10 → 1024
        "arrayVariable",  // bit 11 → 2048
        "hashVariable",   // bit 12 → 4096
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();

    let mut map = FxHashMap::default();
    for (i, t) in types.iter().enumerate() {
        map.insert(t.clone(), i as u32);
    }

    TokensLegend { token_types: types, modifiers, map }
}

#[inline]
fn kind_idx(leg: &TokensLegend, k: &str) -> u32 {
    *leg.map.get(k).unwrap_or(&0)
}

fn is_perl_identifier_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
}

fn method_declaration_name_offsets(
    text: &str,
    node_start: usize,
    node_end: usize,
    name: &str,
) -> Option<(usize, usize)> {
    let node_text = text.get(node_start..node_end)?;
    let relative_start = if node_text.starts_with("method") {
        let after_keyword = node_text.get("method".len()..)?;
        let whitespace_len = after_keyword.len() - after_keyword.trim_start().len();
        if whitespace_len == 0 {
            return None;
        }
        "method".len() + whitespace_len
    } else if node_text.starts_with(name) {
        0
    } else {
        return None;
    };
    let relative_end = relative_start.checked_add(name.len())?;
    if node_text.get(relative_start..relative_end)? != name {
        return None;
    }
    Some((node_start + relative_start, node_start + relative_end))
}

fn statement_label_offsets(
    text: &str,
    node_start: usize,
    node_end: usize,
    label: &str,
) -> Option<(usize, usize)> {
    let node_text = text.get(node_start..node_end)?;
    let relative_start = node_text.find(label)?;
    let relative_end = relative_start.checked_add(label.len())?;
    let before = node_text[..relative_start].chars().next_back();
    if before.is_some_and(is_perl_identifier_continue) {
        return None;
    }
    let after = node_text.get(relative_end..)?;
    if !after.trim_start().starts_with(':') {
        return None;
    }
    Some((node_start + relative_start, node_start + relative_end))
}

fn loop_control_label_offsets(
    text: &str,
    node_start: usize,
    node_end: usize,
    op: &str,
    label: &str,
) -> Option<(usize, usize)> {
    let node_text = text.get(node_start..node_end)?;
    let op_start = node_text.find(op)?;
    let mut search_start = op_start.checked_add(op.len())?;
    while search_start <= node_text.len() {
        let rel = node_text.get(search_start..)?.find(label)?;
        let label_start = search_start + rel;
        let label_end = label_start.checked_add(label.len())?;
        let before = node_text[..label_start].chars().next_back();
        let after = node_text[label_end..].chars().next();
        if before.is_none_or(|ch| !is_perl_identifier_continue(ch))
            && after.is_none_or(|ch| !is_perl_identifier_continue(ch))
        {
            return Some((node_start + label_start, node_start + label_end));
        }
        search_start = label_start.checked_add(1)?;
    }
    None
}

/// Locate the method-name span of a `MethodCall` node by scanning forward from the
/// receiver's end.
///
/// Perl treats whitespace (including newlines) around the `->` operator as
/// insignificant (perldoc perlop), so the method name does not necessarily abut the
/// receiver at `object_end + 2`. Hard-coding that offset mis-paints the token for
/// `$obj ->name`, `$obj-> name`, and leading-arrow chains. Instead, starting at
/// `object_end`: skip whitespace, require the `->` arrow, skip whitespace again, then
/// take `method.len()` bytes as the method-name span. Mirrors the text-scan approach
/// of `statement_label_offsets` / `loop_control_label_offsets`.
fn method_call_name_offsets(text: &str, object_end: usize, method: &str) -> Option<(usize, usize)> {
    let rest = text.get(object_end..)?;
    // Skip whitespace between the receiver and the arrow.
    let after_ws1 = rest.trim_start();
    let ws1_len = rest.len() - after_ws1.len();
    // Require the arrow operator.
    let after_arrow = after_ws1.strip_prefix("->")?;
    // Skip whitespace between the arrow and the method name.
    let after_ws2 = after_arrow.trim_start();
    let ws2_len = after_arrow.len() - after_ws2.len();
    let name_start = object_end + ws1_len + "->".len() + ws2_len;
    let name_end = name_start.checked_add(method.len())?;
    // Confirm the scanned span actually equals the method name before painting it.
    if text.get(name_start..name_end)? != method {
        return None;
    }
    Some((name_start, name_end))
}

// ---------------------------------------------------------------------------
// Heredoc language injection helpers (Issue #2059)
// ---------------------------------------------------------------------------

/// Determine the injection language for a heredoc start token.
///
/// `text` is the full heredoc start token text (e.g. `<<SQL`, `<<'SQL'`, `<<~SQL`,
/// `<<"sql"`). Returns `Some("sql")` or `Some("json")` for known language tags,
/// and `None` for everything else (including backtick command heredocs).
///
/// # Edge cases handled
///
/// - `<<~SQL` — indented heredoc, strip the `~`
/// - `<<'SQL'`, `<<"SQL"` — quoted delimiters, strip quotes
/// - Case-insensitive: `<<sql`, `<<Sql`, `<<SQL` all map to `"sql"`
/// - Backtick heredoc `<<\`SQL\`` — returns `None` (command exec, not injection)
fn heredoc_injection_language(text: &str) -> Option<&'static str> {
    let rest = text.strip_prefix("<<")?;
    // Strip optional indented-heredoc `~`
    let rest = rest.strip_prefix('~').unwrap_or(rest);
    // Backtick delimiter → command heredoc, never inject
    if rest.starts_with('`') {
        return None;
    }
    // Strip matching quote characters (single or double)
    let label = rest.trim_start_matches(['\'', '"']).trim_end_matches(['\'', '"']);
    match label.to_ascii_lowercase().as_str() {
        "sql" | "mysql" | "postgres" | "postgresql" | "sqlite" => Some("sql"),
        "json" => Some("json"),
        _ => None,
    }
}

/// Emit semantic tokens for SQL keyword matches inside a heredoc body.
fn tokenize_sql_body(
    body: &str,
    body_start: usize,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    leg: &TokensLegend,
    out: &mut Vec<RawSemanticToken>,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    let kind = kind_idx(leg, "sql_heredoc_keyword");
    let mut word_start = None;
    for (index, character) in body.char_indices().chain(std::iter::once((body.len(), ' '))) {
        traversal.admit_work()?;
        if is_regex_word_character(character) {
            word_start.get_or_insert(index);
            continue;
        }
        let Some(start) = word_start.take() else {
            continue;
        };
        let word = &body[start..index];
        if !is_sql_keyword(word) {
            continue;
        }
        let offset = body_start + start;
        let (sl, sc) = to_pos16(offset);
        let (el, ec) = to_pos16(body_start + index);
        let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
        if len > 0 {
            out.push((sl, sc, len, kind, 0));
        }
    }
    Ok(())
}

static WORD_CHARACTER_RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?u)^\w$").ok());

fn is_regex_word_character(character: char) -> bool {
    let mut encoded = [0u8; 4];
    let text = character.encode_utf8(&mut encoded);
    WORD_CHARACTER_RE.as_ref().is_some_and(|regex| regex.is_match(text))
}

fn is_sql_keyword(word: &str) -> bool {
    [
        "SELECT",
        "FROM",
        "WHERE",
        "AND",
        "OR",
        "NOT",
        "IN",
        "IS",
        "NULL",
        "LIKE",
        "BETWEEN",
        "JOIN",
        "INNER",
        "LEFT",
        "RIGHT",
        "OUTER",
        "FULL",
        "CROSS",
        "ON",
        "AS",
        "DISTINCT",
        "GROUP",
        "BY",
        "ORDER",
        "HAVING",
        "LIMIT",
        "OFFSET",
        "UNION",
        "ALL",
        "INSERT",
        "INTO",
        "VALUES",
        "UPDATE",
        "SET",
        "DELETE",
        "CREATE",
        "DROP",
        "ALTER",
        "TABLE",
        "INDEX",
        "VIEW",
        "RETURNING",
        "WITH",
        "CASE",
        "WHEN",
        "THEN",
        "ELSE",
        "END",
        "EXISTS",
        "EXCEPT",
        "INTERSECT",
    ]
    .iter()
    .any(|keyword| word.eq_ignore_ascii_case(keyword))
}

/// Emit semantic tokens for JSON key matches inside a heredoc body.
fn tokenize_json_body(
    body: &str,
    body_start: usize,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    leg: &TokensLegend,
    out: &mut Vec<RawSemanticToken>,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    let kind = kind_idx(leg, "json_heredoc_key");
    let mut cursor = 0usize;
    while cursor < body.len() {
        traversal.admit_work()?;
        let Some(character) = body[cursor..].chars().next() else {
            break;
        };
        if character != '"' {
            cursor = cursor.saturating_add(character.len_utf8());
            continue;
        }
        let key_start_offset = cursor;
        cursor = cursor.saturating_add(1);
        let mut escaped = false;
        let mut key_end_offset = None;
        while cursor < body.len() {
            traversal.admit_work()?;
            let Some(current) = body[cursor..].chars().next() else {
                break;
            };
            let current_len = current.len_utf8();
            if current == '"' && !escaped {
                key_end_offset = Some(cursor.saturating_add(current_len));
                cursor = cursor.saturating_add(current_len);
                break;
            }
            escaped = current == '\\' && !escaped;
            if current != '\\' {
                escaped = false;
            }
            cursor = cursor.saturating_add(current_len);
        }
        let Some(_closing_quote_end) = key_end_offset else {
            // No unescaped closing quote remains in the suffix. Resetting to
            // open+1 would only rescan the same escaped-quote suffix (O(n^2) on
            // bodies like `"\"\"...`) and cannot recover a later JSON key,
            // which itself requires an unescaped quote delimiter.
            break;
        };
        while cursor < body.len() {
            traversal.admit_work()?;
            let Some(current) = body[cursor..].chars().next() else {
                break;
            };
            if !current.is_whitespace() {
                break;
            }
            cursor = cursor.saturating_add(current.len_utf8());
        }
        if body.as_bytes().get(cursor) != Some(&b':') {
            cursor = key_start_offset.saturating_add(1);
            continue;
        }
        // Preserve the historical regex geometry: whitespace before the colon
        // belongs to the json_heredoc_key span, while the colon does not.
        let key_end_offset = cursor;
        let key_start = body_start + key_start_offset;
        let key_end = body_start + key_end_offset;
        let (sl, sc) = to_pos16(key_start);
        let (el, ec) = to_pos16(key_end);
        let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
        if len > 0 {
            out.push((sl, sc, len, kind, 0));
        }
    }
    Ok(())
}

/// Dispatch to the appropriate body tokenizer based on the injection language.
fn tokenize_heredoc_body(
    body: &str,
    body_start: usize,
    lang: &str,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    leg: &TokensLegend,
    out: &mut Vec<RawSemanticToken>,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    match lang {
        "sql" => tokenize_sql_body(body, body_start, to_pos16, leg, out, traversal),
        "json" => tokenize_json_body(body, body_start, to_pos16, leg, out, traversal),
        _ => Ok(()),
    }
}

/// Returns `true` when `full_name` is a well-known Perl built-in special variable.
///
/// `full_name` is the sigil concatenated with the bare name, e.g. `"$_"`, `"@_"`,
/// `"%ENV"`.  These variables exist in every Perl program and are never declared
/// with `my`/`our`, so they deserve the `defaultLibrary` semantic-token modifier
/// to let editors colour them distinctly from user-defined variables.
///
/// Note: hash elements accessed as `$ENV{KEY}` appear in the AST as
/// `Variable { sigil: "$", name: "ENV" }`.  We include both `$ENV` and `%ENV`
/// so that all access forms receive the modifier.
fn is_special_variable(full_name: &str) -> bool {
    matches!(
        full_name,
        "$_" | "@_"
            | "$!"
            | "$@"
            | "$?"
            | "$/"
            | "$\\"
            | "$$"
            | "$0"
            | "$;"
            | "$,"
            | "$."
            | "$&"
            | "$'"
            | "$`"
            | "$+"
            | "$^W"
            | "$^O"
            | "$^V"
            | "$^T"
            | "$^A"
            | "@ISA"
            | "@INC"
            | "@ARGV"
            | "%ENV"
            | "$ENV"  // hash element access: $ENV{KEY}
            | "%INC"
            | "$INC"  // hash element access: $INC{'Foo.pm'}
            | "%SIG"
            | "$SIG" // hash element access: $SIG{INT}
            | "$PL_sv_yes"
            | "$PL_sv_no"
            | "$PL_sv_undef"
    )
}

fn find_bytes_at_controlled(
    haystack: &[u8],
    from: usize,
    needle: &[u8],
    traversal: &mut TraversalState<'_, '_>,
) -> Result<Option<usize>, TraversalStop> {
    if needle.is_empty() {
        return Ok(Some(from));
    }
    let end = haystack.len().saturating_sub(needle.len());
    for index in from..=end {
        traversal.admit_work()?;
        if haystack[index..index + needle.len()] == *needle {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// Caller-owned limits for semantic-token traversal.
///
/// Cancellation is polled before every lexer step and every AST node. A work
/// unit is also charged for every interpolation part and substring candidate,
/// heredoc character, declaration-target node, declaration-index insertion or lookup, raw-token copy,
/// heapsort comparison or swap, overlap candidate, and encoded token.
/// Cancellation is checked before the budget, so
/// simultaneous cancellation and exhaustion reports `Cancelled`. A zero budget
/// admits no work. Once either stop is observed, no further work is performed;
/// overshoot is zero work units. The only opaque interval is one lexer call,
/// bounded to [`MAX_BOUNDED_LEXER_SOURCE_BYTES`] for controlled traversals.
#[non_exhaustive]
pub struct SemanticTokensTraversalControl<'a> {
    cancellation: Option<&'a (dyn Fn() -> bool + Send + Sync)>,
    work_budget: Option<usize>,
}

impl<'a> SemanticTokensTraversalControl<'a> {
    /// Traverse without cancellation or a work budget.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self { cancellation: None, work_budget: None }
    }

    /// Traverse with caller-owned cancellation and an optional deterministic work budget.
    #[must_use]
    pub const fn new(
        cancellation: &'a (dyn Fn() -> bool + Send + Sync),
        work_budget: Option<usize>,
    ) -> Self {
        Self { cancellation: Some(cancellation), work_budget }
    }

    const fn is_bounded(&self) -> bool {
        self.cancellation.is_some() || self.work_budget.is_some()
    }
}

/// Result of a controlled semantic-token traversal.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SemanticTokensTraversalOutcome {
    /// The complete token stream, identical to the compatibility collector output.
    Complete(Vec<EncodedToken>),
    /// The caller cancelled traversal before the next work unit was admitted.
    Cancelled {
        /// Exact number of admitted units described by the traversal control.
        work_done: usize,
    },
    /// The deterministic work budget was consumed before traversal completed.
    BudgetExhausted {
        /// Explicitly incomplete raw tokens collected before exhaustion.
        partial: PartialSemanticTokens,
        /// Exact number of admitted units described by the traversal control.
        work_done: usize,
    },
    /// No AST was available at the core collection boundary.
    NoAst,
    /// Collection failed before an AST could be supplied to traversal.
    CollectionFailure(SemanticTokensCollectionError),
    /// A bounded collector cannot safely admit an opaque lexer call for this source.
    SourceLimitExceeded {
        /// Source size presented by the caller.
        source_bytes: usize,
        /// Maximum source size supported by the bounded collector.
        limit_bytes: usize,
    },
}

/// Typed collection failure supplied to the core semantic-token boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SemanticTokensCollectionError {
    message: String,
}

impl SemanticTokensCollectionError {
    /// Create a collection failure without imposing an LSP wire error policy.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    /// Human-readable failure detail for logging or caller-owned policy.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// AST availability at the fallible core collection boundary.
#[non_exhaustive]
pub enum SemanticTokensCollectionInput<'a> {
    /// A parsed AST is available for traversal.
    Ast(&'a Node),
    /// Parsing completed without an AST.
    NoAst,
    /// Parsing or collection preparation failed.
    CollectionFailure(SemanticTokensCollectionError),
}

/// Explicitly incomplete semantic-token data.
///
/// This type intentionally does not expose LSP-encoded data: sorting, overlap
/// removal, and delta encoding may themselves be interrupted. Callers can
/// inspect the amount of collected raw data without mistaking it for a complete
/// token stream or paying unbounded finalization cost after a stop.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PartialSemanticTokens {
    ast_tokens: Vec<RawSemanticToken>,
    lexer_tokens: Vec<RawSemanticToken>,
}

impl PartialSemanticTokens {
    /// Number of raw tokens retained before traversal or finalization stopped.
    #[must_use]
    pub const fn raw_token_count(&self) -> usize {
        self.ast_tokens.len().saturating_add(self.lexer_tokens.len())
    }

    /// Iterate the retained raw tokens without sorting or allocating.
    pub fn raw_tokens(&self) -> impl Iterator<Item = &RawSemanticToken> {
        self.ast_tokens.iter().chain(&self.lexer_tokens)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TraversalStop {
    Cancelled,
    BudgetExhausted,
    WorkCounterOverflow,
}

struct TraversalState<'control, 'callback> {
    control: &'control SemanticTokensTraversalControl<'callback>,
    work_done: usize,
}

#[cfg(test)]
thread_local! {
    static CONTROLLED_CHILD_EDGES_ENUMERATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DECLARATION_INDEX_INSERTIONS_ATTEMPTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DECLARATION_LOOKUPS_ATTEMPTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl TraversalState<'_, '_> {
    fn admit_work(&mut self) -> Result<(), TraversalStop> {
        if self.control.cancellation.is_some_and(|cancelled| cancelled()) {
            return Err(TraversalStop::Cancelled);
        }
        if self.control.work_budget.is_some_and(|budget| self.work_done >= budget) {
            return Err(TraversalStop::BudgetExhausted);
        }
        self.work_done = self.work_done.checked_add(1).ok_or(TraversalStop::WorkCounterOverflow)?;
        Ok(())
    }
}

fn interrupted_outcome(
    stop: TraversalStop,
    ast_tokens: Vec<RawSemanticToken>,
    lexer_tokens: Vec<RawSemanticToken>,
    work_done: usize,
) -> SemanticTokensTraversalOutcome {
    match stop {
        TraversalStop::Cancelled => SemanticTokensTraversalOutcome::Cancelled { work_done },
        TraversalStop::BudgetExhausted => SemanticTokensTraversalOutcome::BudgetExhausted {
            partial: PartialSemanticTokens { ast_tokens, lexer_tokens },
            work_done,
        },
        TraversalStop::WorkCounterOverflow => SemanticTokensTraversalOutcome::CollectionFailure(
            SemanticTokensCollectionError::new("semantic-token work counter overflow"),
        ),
    }
}

/// Collect semantic tokens for LSP highlighting in the Complete stage.
///
/// # Arguments
/// * `ast` - Parsed AST for the document.
/// * `text` - Original source text.
/// * `to_pos16` - Converts byte offsets to UTF-16 positions.
/// # Returns
/// Encoded semantic tokens sorted for LSP transmission.
/// # Examples
/// ```rust,ignore
/// use perl_lsp_providers::{Parser, ide::lsp_compat::semantic_tokens::collect_semantic_tokens};
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let code = "my $x = 1;";
/// let mut parser = Parser::new(code);
/// let ast = parser.parse()?;
/// let to_pos16 = |pos| (0u32, pos as u32);
/// let tokens = collect_semantic_tokens(&ast, code, &to_pos16);
/// assert!(!tokens.is_empty());
/// # Ok(())
/// # }
/// ```
pub fn collect_semantic_tokens(
    ast: &Node,
    text: &str,
    to_pos16: &impl Fn(usize) -> (u32, u32),
) -> Vec<EncodedToken> {
    match collect_semantic_tokens_controlled(
        ast,
        text,
        to_pos16,
        &SemanticTokensTraversalControl::unlimited(),
    ) {
        SemanticTokensTraversalOutcome::Complete(tokens) => tokens,
        SemanticTokensTraversalOutcome::Cancelled { .. }
        | SemanticTokensTraversalOutcome::BudgetExhausted { .. }
        | SemanticTokensTraversalOutcome::NoAst
        | SemanticTokensTraversalOutcome::CollectionFailure(_)
        | SemanticTokensTraversalOutcome::SourceLimitExceeded { .. } => Vec::new(),
    }
}

/// Collect from a boundary that preserves absent-AST and typed-failure states.
pub fn collect_semantic_tokens_from_input(
    input: SemanticTokensCollectionInput<'_>,
    text: &str,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    control: &SemanticTokensTraversalControl<'_>,
) -> SemanticTokensTraversalOutcome {
    match input {
        SemanticTokensCollectionInput::Ast(ast) => {
            collect_semantic_tokens_controlled(ast, text, to_pos16, control)
        }
        SemanticTokensCollectionInput::NoAst => SemanticTokensTraversalOutcome::NoAst,
        SemanticTokensCollectionInput::CollectionFailure(error) => {
            SemanticTokensTraversalOutcome::CollectionFailure(error)
        }
    }
}

/// Collect semantic tokens with caller-owned cancellation and deterministic work limits.
pub fn collect_semantic_tokens_controlled(
    ast: &Node,
    text: &str,
    to_pos16: &impl Fn(usize) -> (u32, u32),
    control: &SemanticTokensTraversalControl<'_>,
) -> SemanticTokensTraversalOutcome {
    let mut traversal = TraversalState { control, work_done: 0 };
    let leg = legend();
    // AST tokens are collected first so that when lexer tokens occupy the same
    // span (e.g. "method" keyword vs method-name token), the AST token wins
    // the stable-sort tie-break in remove_overlapping_tokens.
    let mut ast_tokens: Vec<RawSemanticToken> = Vec::new();
    let mut lexer_tokens: Vec<RawSemanticToken> = Vec::new();
    macro_rules! controlled_value {
        ($expression:expr) => {
            match $expression {
                Ok(value) => value,
                Err(stop) => {
                    return interrupted_outcome(
                        stop,
                        ast_tokens,
                        lexer_tokens,
                        traversal.work_done,
                    );
                }
            }
        };
    }

    if control.is_bounded() && text.len() > MAX_BOUNDED_LEXER_SOURCE_BYTES {
        if let Err(stop) = traversal.admit_work() {
            return interrupted_outcome(stop, ast_tokens, lexer_tokens, traversal.work_done);
        }
        return SemanticTokensTraversalOutcome::SourceLimitExceeded {
            source_bytes: text.len(),
            limit_bytes: MAX_BOUNDED_LEXER_SOURCE_BYTES,
        };
    }

    // 1) Fast path from lexer categories: conservative single-line emission
    // FIFO queue of pending heredoc injection languages, one entry per heredoc start token
    // encountered in source order. Multiple heredocs on the same line (`<<SQL, <<JSON`)
    // are handled correctly: we push on HeredocStart and pop on HeredocBody.
    let mut pending_heredoc_langs: VecDeque<Option<&'static str>> = VecDeque::new();
    // Use with_body_tokens so the lexer emits HeredocBody tokens (needed for injection).
    let mut lexer = PerlLexer::with_body_tokens(text);
    loop {
        if let Err(stop) = traversal.admit_work() {
            return interrupted_outcome(stop, ast_tokens, lexer_tokens, traversal.work_done);
        }
        let Some(tok) = lexer.next_token() else {
            break;
        };
        let (sl, sc) = to_pos16(tok.start);
        let (el, ec) = to_pos16(tok.end);
        let len = if sl == el { ec.saturating_sub(sc) } else { 0 };

        // Map token types to semantic token kinds
        // Note: The lexer's TokenType enum is simpler than what we're matching
        let kind = match &tok.token_type {
            TokenType::Keyword(kw) => {
                // Check if it's a known keyword
                match kw.as_ref() {
                    "my" | "our" | "local" | "state" | "sub" | "package" | "use" | "require"
                    | "if" | "else" | "elsif" | "for" | "foreach" | "while" | "until" | "do"
                    | "return" | "next" | "last" | "redo" | "goto" | "eval" | "given" | "when"
                    | "default" | "break" | "continue" | "unless" | "no" | "BEGIN" | "END"
                    | "CHECK" | "INIT" | "UNITCHECK" | "class" | "method" | "try" | "catch"
                    | "finally" | "await"
                    // Infix operator keywords (perlop) — `isa` added for Perl 5.32+ (issue #778)
                    | "isa" | "cmp" => "keyword",
                    _ => continue,
                }
            }

            TokenType::InterpolatedString(parts) => {
                // Split the string into literal fragments (string) and variable
                // interpolations (variable), pushing each as its own token.
                // This avoids the "longer wins" overlap-removal rule that would
                // silently discard variable sub-tokens if we also pushed a
                // whole-string token spanning the entire interpolated string.
                if len > 0 {
                    let text_bytes = tok.text.as_bytes();
                    let mut cursor: usize = 1; // skip opening quote char
                    for part in parts {
                        controlled_value!(traversal.admit_work());
                        match part {
                            StringPart::Literal(lit) => {
                                if let Some(rel) = controlled_value!(find_bytes_at_controlled(
                                    text_bytes,
                                    cursor,
                                    lit.as_bytes(),
                                    &mut traversal,
                                )) {
                                    let part_start = tok.start + rel;
                                    let part_end = part_start + lit.len();
                                    let (psl, psc) = to_pos16(part_start);
                                    let (pel, pec) = to_pos16(part_end);
                                    let plen = if psl == pel { pec.saturating_sub(psc) } else { 0 };
                                    if plen > 0 {
                                        lexer_tokens.push((
                                            psl,
                                            psc,
                                            plen,
                                            kind_idx(&leg, "string"),
                                            0,
                                        ));
                                    }
                                    cursor = rel + lit.len();
                                }
                            }
                            StringPart::Variable(var) => {
                                if let Some(rel) = controlled_value!(find_bytes_at_controlled(
                                    text_bytes,
                                    cursor,
                                    var.as_bytes(),
                                    &mut traversal,
                                )) {
                                    let part_start = tok.start + rel;
                                    let part_end = part_start + var.len();
                                    let (psl, psc) = to_pos16(part_start);
                                    let (pel, pec) = to_pos16(part_end);
                                    let plen = if psl == pel { pec.saturating_sub(psc) } else { 0 };
                                    if plen > 0 {
                                        lexer_tokens.push((
                                            psl,
                                            psc,
                                            plen,
                                            kind_idx(&leg, "variable"),
                                            0,
                                        ));
                                    }
                                    cursor = rel + var.len();
                                }
                            }
                            // Expression, ArraySlice, MethodCall: defined in StringPart but
                            // the current lexer never emits them — only Literal and Variable
                            // are populated. Skip silently.
                            _ => {}
                        }
                    }
                }
                continue;
            }

            TokenType::StringLiteral
            | TokenType::QuoteSingle
            | TokenType::QuoteDouble
            | TokenType::QuoteWords
            | TokenType::QuoteCommand => "string",

            TokenType::HeredocStart => {
                // Record the injection language for this heredoc's upcoming body token.
                pending_heredoc_langs.push_back(heredoc_injection_language(&tok.text));
                "string"
            }

            TokenType::HeredocBody(_) => {
                // Pop the queued injection language for the corresponding heredoc start.
                // NOTE: The Arc<str> inside HeredocBody is always empty_arc(); the actual
                // body text must be sliced from the source using tok.start..tok.end.
                if let Some(maybe_lang) = pending_heredoc_langs.pop_front()
                    && let Some(lang) = maybe_lang
                {
                    let body_end = tok.end.min(text.len());
                    let body = &text[tok.start.min(body_end)..body_end];
                    controlled_value!(tokenize_heredoc_body(
                        body,
                        tok.start,
                        lang,
                        to_pos16,
                        &leg,
                        &mut lexer_tokens,
                        &mut traversal,
                    ));
                }
                "string"
            }

            TokenType::Number(_) => "number",

            TokenType::RegexMatch
            | TokenType::Substitution
            | TokenType::Transliteration
            | TokenType::QuoteRegex => "regexp",

            TokenType::Division
            | TokenType::Operator(_)
            | TokenType::Arrow
            | TokenType::FatComma => "operator",

            TokenType::Comment(_) => "comment",

            // POD documentation blocks
            TokenType::Pod => "comment",
            _ => continue,
        };

        if len > 0 {
            lexer_tokens.push((sl, sc, len, kind_idx(&leg, kind), 0));
        }
    }

    let const_fast_enabled = controlled_value!(ast_uses_const_fast(ast, &mut traversal));
    let readonly_enabled = controlled_value!(ast_uses_readonly(ast, &mut traversal));

    // 2a) Collect variable declaration spans for modifier tagging
    let decl_spans = controlled_value!(declaration_readonly_flags(
        ast,
        const_fast_enabled,
        readonly_enabled,
        &mut traversal,
    ));

    // 2a-ii) Collect assignment LHS spans to apply the "modification" modifier (bit 7)
    let assignment_spans = controlled_value!(assignment_lhs_spans(ast, &mut traversal));

    // 2b) AST overlays: package/sub/variable with precise spans where available
    controlled_value!(walk_ast_full_controlled_with_state(
        ast,
        &mut traversal,
        &mut |node, traversal| {
            // For nodes with name_span, use the precise span for better highlighting
            match &node.kind {
                NodeKind::Package { name_span, .. } => {
                    let (sl, sc) = to_pos16(name_span.start);
                    let (el, ec) = to_pos16(name_span.end);
                    let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                    if len > 0 {
                        ast_tokens.push((
                            sl,
                            sc,
                            len,
                            kind_idx(&leg, "namespace"),
                            1, /*declaration*/
                        ));
                    }
                    return Ok(true);
                }
                NodeKind::Subroutine { name: Some(_), name_span: Some(span), .. } => {
                    let (sl, sc) = to_pos16(span.start);
                    let (el, ec) = to_pos16(span.end);
                    let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                    if len > 0 {
                        ast_tokens.push((
                            sl,
                            sc,
                            len,
                            kind_idx(&leg, "function"),
                            1 | 2, /*declaration|definition*/
                        ));
                    }
                    return Ok(true);
                }
                NodeKind::Subroutine { name: Some(_), .. } => {
                    let (sl, sc) = to_pos16(node.location.start);
                    let (el, ec) = to_pos16(node.location.end);
                    let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                    if len > 0 {
                        ast_tokens.push((
                            sl,
                            sc,
                            len,
                            kind_idx(&leg, "function"),
                            1, /*declaration*/
                        ));
                    }
                    return Ok(true);
                }
                NodeKind::Method { name, .. } => {
                    let (start, end) = method_declaration_name_offsets(
                        text,
                        node.location.start,
                        node.location.end,
                        name,
                    )
                    .unwrap_or((node.location.start, node.location.end));
                    let (sl, sc) = to_pos16(start);
                    let (el, ec) = to_pos16(end);
                    let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                    if len > 0 {
                        ast_tokens.push((
                            sl,
                            sc,
                            len,
                            kind_idx(&leg, "method"),
                            1 | 2, /*declaration|definition*/
                        ));
                    }
                    return Ok(true);
                }
                NodeKind::Class { .. } => {
                    let (sl, sc) = to_pos16(node.location.start);
                    let (el, ec) = to_pos16(node.location.end);
                    let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                    if len > 0 {
                        ast_tokens.push((
                            sl,
                            sc,
                            len,
                            kind_idx(&leg, "class"),
                            1, /*declaration*/
                        ));
                    }
                    return Ok(true);
                }
                NodeKind::PhaseBlock { phase_span: Some(span), .. } => {
                    let (sl, sc) = to_pos16(span.start);
                    let (el, ec) = to_pos16(span.end);
                    let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                    if len > 0 {
                        ast_tokens.push((sl, sc, len, kind_idx(&leg, "macro"), 0));
                    }
                    return Ok(true);
                }
                NodeKind::LabeledStatement { label, .. } => {
                    let Some(fallback_end) = node.location.start.checked_add(label.len()) else {
                        return Ok(true);
                    };
                    let (start, end) = statement_label_offsets(
                        text,
                        node.location.start,
                        node.location.end,
                        label,
                    )
                    .unwrap_or((node.location.start, fallback_end));
                    let (sl, sc) = to_pos16(start);
                    let (el, ec) = to_pos16(end);
                    let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                    if len > 0 {
                        ast_tokens.push((
                            sl,
                            sc,
                            len,
                            kind_idx(&leg, "label"),
                            1, /*declaration*/
                        ));
                    }
                    return Ok(true);
                }
                NodeKind::LoopControl { op, label: Some(label) } => {
                    if let Some((start, end)) = loop_control_label_offsets(
                        text,
                        node.location.start,
                        node.location.end,
                        op,
                        label,
                    ) {
                        let (sl, sc) = to_pos16(start);
                        let (el, ec) = to_pos16(end);
                        let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                        if len > 0 {
                            ast_tokens.push((sl, sc, len, kind_idx(&leg, "label"), 0));
                        }
                    }
                    return Ok(true);
                }
                NodeKind::MethodCall { object, method, args } => {
                    // Emit a narrow token for just the method name, not the entire
                    // expression. Whitespace/newlines may separate the receiver from
                    // `->method` (perldoc perlop), so scan forward for the span rather
                    // than assuming `->` abuts the receiver at object.location.end + 2.
                    if let Some((method_name_start, method_name_end)) =
                        method_call_name_offsets(text, object.location.end, method)
                    {
                        let (sl, sc) = to_pos16(method_name_start);
                        let (el, ec) = to_pos16(method_name_end);
                        let len = if sl == el { ec.saturating_sub(sc) } else { 0 };
                        if len > 0 {
                            ast_tokens.push((sl, sc, len, kind_idx(&leg, "method"), 0));
                        }
                    }
                    // If this is a SQL-bearing DBI method, classify the first string arg as sql_string.
                    let is_sql_method = matches!(
                        method.as_str(),
                        "prepare"
                            | "do"
                            | "query"
                            | "selectrow_array"
                            | "selectrow_arrayref"
                            | "selectrow_hashref"
                            | "selectall_arrayref"
                            | "selectall_hashref"
                            | "fetchall_arrayref"
                            | "fetchall_hashref"
                            | "fetchrow_arrayref"
                            | "fetchrow_hashref"
                            | "execute"
                    );
                    if is_sql_method
                        && let Some(first_arg) = args.first()
                        && matches!(first_arg.kind, NodeKind::String { .. })
                    {
                        let (asl, asc) = to_pos16(first_arg.location.start);
                        let (ael, aec) = to_pos16(first_arg.location.end);
                        let alen = if asl == ael { aec.saturating_sub(asc) } else { 0 };
                        if alen > 0 {
                            ast_tokens.push((asl, asc, alen, kind_idx(&leg, "sql_string"), 0));
                        }
                    }
                    return Ok(true);
                }
                _ => {}
            }

            let (s, e) = (node.location.start, node.location.end);
            let (sl, sc) = to_pos16(s);
            let (el, ec) = to_pos16(e);
            let len = if sl == el { ec.saturating_sub(sc) } else { 0 };

            let (kind, mods): (&str, u32) = match &node.kind {
                NodeKind::FunctionCall { name, .. } | NodeKind::AmperCall { name, .. } => {
                    if matches!(&node.kind, NodeKind::FunctionCall { .. })
                        && ((const_fast_enabled && name == "const")
                            || (readonly_enabled && name == "Readonly"))
                    {
                        return Ok(true);
                    }
                    // Skip builtins that should remain as keywords from the lexer pass,
                    // and synthetic names produced for coderef/deref calls (these don't
                    // start at node.location.start — painting them produces garbage).
                    match name.as_str() {
                        "eval" | "do" | "use" | "no" | "return" | "my" | "our" | "local"
                        | "state" | "next" | "last" | "redo" | "goto" => return Ok(true),
                        // Synthetic FunctionCall names from postfix.rs (coderef invocation)
                        // and variables.rs (deref).  The name is not a real identifier at
                        // node.location.start, so narrowing to name.len() bytes paints
                        // garbage on the receiver.
                        "->()" | "&{}" | "$" => return Ok(true),
                        _ => {
                            // Narrow the token to just the function name, not the entire
                            // call expression.  Previously the token spanned the whole call
                            // (name + args), which caused the arguments to inherit the
                            // function color and dropped inner string/number/variable tokens
                            // via overlap removal.  (#5077)
                            //
                            // For AmperCall the source is `&name(...)` — the name starts
                            // one byte after the node start (past the `&`).
                            let is_amper = matches!(&node.kind, NodeKind::AmperCall { .. });
                            let name_start = if is_amper { s + 1 } else { s };
                            let name_end = name_start + name.len();
                            let (nsl, nsc) = to_pos16(name_start);
                            let (nel, nec) = to_pos16(name_end);
                            if nsl == nel {
                                let nlen = nec.saturating_sub(nsc);
                                if nlen > 0 {
                                    ast_tokens.push((
                                        nsl,
                                        nsc,
                                        nlen,
                                        kind_idx(&leg, "function"),
                                        0,
                                    ));
                                }
                            }
                            return Ok(true); // already emitted with narrowed range
                        }
                    }
                }
                NodeKind::Variable { sigil, name } => {
                    let (vs, ve) = (node.location.start, node.location.end);
                    let decl_info = declaration_flag(&decl_spans, (vs, ve), traversal)?;
                    let full_name = format!("{sigil}{name}");
                    let special_mod = if is_special_variable(&full_name) { 512 } else { 0 }; // defaultLibrary bit 9
                    let sigil_mod: u32 = match sigil.as_str() {
                        "$" => 1024, // scalarVariable bit 10
                        "@" => 2048, // arrayVariable  bit 11
                        "%" => 4096, // hashVariable   bit 12
                        _ => 0,      // "&" (code ref), "*" (glob), others
                    };
                    let mods = match decl_info {
                        // `Const::Fast` / `Readonly` produce true read-only variables,
                        // so emit `declaration | readonly` (bits 0 | 2) (#4968).
                        Some(true) => 1 | 4 | special_mod | sigil_mod,
                        // `my`, `state`, `local`, and `our` create ordinary mutable
                        // lexical/package variables — `our` is an alias, not immutable
                        // — so the `readonly` modifier (bit 2) must not be applied
                        // (#4968). Use `declaration` only.
                        Some(false) => 1 | special_mod | sigil_mod,
                        None => {
                            // Apply "modification" modifier (bit 7 = 128) when the variable is
                            // the direct LHS of an assignment expression ($x = ...).
                            let mod_bit =
                                if assignment_spans.contains(&(vs, ve)) { 128 } else { 0 };
                            special_mod | sigil_mod | mod_bit
                        }
                    };
                    ("variable", mods)
                }
                _ => return Ok(true),
            };

            if len > 0 {
                ast_tokens.push((sl, sc, len, kind_idx(&leg, kind), mods));
            }
            Ok(true)
        },
    ));

    let tokens =
        controlled_value!(finalize_tokens_controlled(&ast_tokens, &lexer_tokens, &mut traversal,));
    SemanticTokensTraversalOutcome::Complete(tokens)
}

#[derive(Clone, Copy)]
struct IndexedRawToken {
    token: RawSemanticToken,
    order: usize,
}

fn indexed_token_key(token: &IndexedRawToken) -> (u32, u32, usize) {
    (token.token.0, token.token.1, token.order)
}

fn sift_down(
    tokens: &mut [IndexedRawToken],
    mut root: usize,
    end: usize,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    loop {
        let Some(left) = root.checked_mul(2).and_then(|value| value.checked_add(1)) else {
            return Ok(());
        };
        if left >= end {
            return Ok(());
        }
        let mut largest = root;
        traversal.admit_work()?;
        if indexed_token_key(&tokens[left]) > indexed_token_key(&tokens[largest]) {
            largest = left;
        }
        let right = left.saturating_add(1);
        if right < end {
            traversal.admit_work()?;
            if indexed_token_key(&tokens[right]) > indexed_token_key(&tokens[largest]) {
                largest = right;
            }
        }
        if largest == root {
            return Ok(());
        }
        traversal.admit_work()?;
        tokens.swap(root, largest);
        root = largest;
    }
}

fn controlled_sort_tokens(
    tokens: &mut [IndexedRawToken],
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    let length = tokens.len();
    for root in (0..length / 2).rev() {
        sift_down(tokens, root, length, traversal)?;
    }
    for end in (1..length).rev() {
        traversal.admit_work()?;
        tokens.swap(0, end);
        sift_down(tokens, 0, end, traversal)?;
    }
    Ok(())
}

fn finalize_tokens_controlled(
    ast_tokens: &[RawSemanticToken],
    lexer_tokens: &[RawSemanticToken],
    traversal: &mut TraversalState<'_, '_>,
) -> Result<Vec<EncodedToken>, TraversalStop> {
    let capacity = ast_tokens.len().saturating_add(lexer_tokens.len());
    let mut indexed = Vec::with_capacity(capacity);
    for token in ast_tokens.iter().chain(lexer_tokens) {
        traversal.admit_work()?;
        indexed.push(IndexedRawToken { token: *token, order: indexed.len() });
    }
    controlled_sort_tokens(&mut indexed, traversal)?;

    let mut dedup: Vec<RawSemanticToken> = Vec::with_capacity(indexed.len());
    for indexed_token in indexed {
        traversal.admit_work()?;
        let token = indexed_token.token;
        let (line, start_char, length, _, _) = token;
        if let Some(&(last_line, last_start, last_length, _, _)) = dedup.last()
            && line == last_line
            && start_char < last_start.saturating_add(last_length)
        {
            if length > last_length {
                dedup.pop();
                dedup.push(token);
            }
        } else {
            dedup.push(token);
        }
    }

    let mut encoded = Vec::with_capacity(dedup.len());
    let mut previous_line = 0u32;
    let mut previous_character = 0u32;
    for (line, character, length, kind, modifiers) in dedup {
        traversal.admit_work()?;
        let (delta_line, delta_character) = if line == previous_line {
            (0, character.saturating_sub(previous_character))
        } else {
            (line.saturating_sub(previous_line), character)
        };
        encoded.push([delta_line, delta_character, length, kind, modifiers]);
        previous_line = line;
        previous_character = character;
    }
    Ok(encoded)
}

/// Comprehensive AST walker for semantic token extraction.
#[cfg(test)]
fn walk_ast_full<F>(node: &Node, visitor: &mut F) -> bool
where
    F: FnMut(&Node) -> bool,
{
    let control = SemanticTokensTraversalControl::unlimited();
    let mut traversal = TraversalState { control: &control, work_done: 0 };
    walk_ast_full_controlled(node, &mut traversal, visitor).unwrap_or_default()
}

fn walk_ast_full_controlled<F>(
    node: &Node,
    traversal: &mut TraversalState<'_, '_>,
    visitor: &mut F,
) -> Result<bool, TraversalStop>
where
    F: FnMut(&Node) -> bool,
{
    walk_ast_full_controlled_with_state(node, traversal, &mut |node, _| Ok(visitor(node)))
}

fn walk_ast_full_controlled_with_state<F>(
    node: &Node,
    traversal: &mut TraversalState<'_, '_>,
    visitor: &mut F,
) -> Result<bool, TraversalStop>
where
    F: FnMut(&Node, &mut TraversalState<'_, '_>) -> Result<bool, TraversalStop>,
{
    traversal.admit_work()?;
    if !visitor(node, traversal)? {
        return Ok(false);
    }
    match node.try_for_each_child_with_field_observed(
        |_, _| {
            #[cfg(test)]
            CONTROLLED_CHILD_EDGES_ENUMERATED
                .with(|count| count.set(count.get().saturating_add(1)));
        },
        |_, child| match walk_ast_full_controlled_with_state(child, traversal, visitor) {
            Ok(true) => ControlFlow::Continue(()),
            Ok(false) => ControlFlow::Break(Ok(false)),
            Err(stop) => ControlFlow::Break(Err(stop)),
        },
    ) {
        ControlFlow::Continue(()) => Ok(true),
        ControlFlow::Break(result) => result,
    }
}

fn declaration_readonly_flags(
    ast: &Node,
    const_fast_enabled: bool,
    readonly_enabled: bool,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<FxHashMap<(usize, usize), bool>, TraversalStop> {
    let mut flags = FxHashMap::default();

    walk_ast_full_controlled_with_state(ast, traversal, &mut |node, traversal| {
        match &node.kind {
            NodeKind::VariableDeclaration { declarator, variable, .. } => {
                // `our` creates a package-variable alias in lexical scope; it
                // does not make the variable immutable. Only `Const::Fast` /
                // `Readonly` produce true readonly semantics (#4968).
                let is_readonly = false;
                let _ = declarator;
                mark_declaration_target_flags(variable, is_readonly, &mut flags, traversal)?;
            }
            NodeKind::VariableListDeclaration { declarator, variables, .. } => {
                let is_readonly = false;
                let _ = declarator;
                for variable in variables {
                    mark_declaration_target_flags(variable, is_readonly, &mut flags, traversal)?;
                }
            }
            NodeKind::FunctionCall { name, args } if const_fast_enabled && name == "const" => {
                mark_readonly_declaration_flags(args, &mut flags, traversal)?;
            }
            NodeKind::FunctionCall { name, args } if readonly_enabled && name == "Readonly" => {
                mark_readonly_declaration_flags(args, &mut flags, traversal)?;
            }
            _ => {}
        }
        Ok(true)
    })?;

    Ok(flags)
}

fn mark_declaration_target_flags(
    target: &Node,
    is_readonly: bool,
    flags: &mut FxHashMap<(usize, usize), bool>,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    walk_ast_full_controlled_with_state(target, traversal, &mut |node, traversal| {
        if matches!(&node.kind, NodeKind::Variable { .. }) {
            #[cfg(test)]
            DECLARATION_INDEX_INSERTIONS_ATTEMPTED
                .with(|count| count.set(count.get().saturating_add(1)));
            traversal.admit_work()?;
            flags
                .entry((node.location.start, node.location.end))
                .and_modify(|flag| *flag |= is_readonly)
                .or_insert(is_readonly);
        }
        Ok(true)
    })?;
    Ok(())
}

fn declaration_flag(
    flags: &FxHashMap<(usize, usize), bool>,
    span: (usize, usize),
    traversal: &mut TraversalState<'_, '_>,
) -> Result<Option<bool>, TraversalStop> {
    #[cfg(test)]
    DECLARATION_LOOKUPS_ATTEMPTED.with(|count| count.set(count.get().saturating_add(1)));
    traversal.admit_work()?;
    Ok(flags.get(&span).copied())
}

fn assignment_lhs_spans(
    ast: &Node,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<FxHashSet<(usize, usize)>, TraversalStop> {
    let mut spans = FxHashSet::default();
    walk_ast_full_controlled(ast, traversal, &mut |node| {
        if let NodeKind::Assignment { lhs, .. } = &node.kind {
            spans.insert((lhs.location.start, lhs.location.end));
        }
        true
    })?;
    Ok(spans)
}

fn ast_uses_const_fast(
    ast: &Node,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<bool, TraversalStop> {
    let mut enabled = false;
    walk_ast_full_controlled(ast, traversal, &mut |node| {
        if matches!(&node.kind, NodeKind::Use { module, .. } if module == "Const::Fast") {
            enabled = true;
            return false;
        }
        true
    })?;
    Ok(enabled)
}

fn ast_uses_readonly(
    ast: &Node,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<bool, TraversalStop> {
    let mut enabled = false;
    walk_ast_full_controlled(ast, traversal, &mut |node| {
        if matches!(&node.kind, NodeKind::Use { module, .. } if module == "Readonly") {
            enabled = true;
            return false;
        }
        true
    })?;
    Ok(enabled)
}

fn mark_readonly_declaration_flags(
    args: &[Node],
    flags: &mut FxHashMap<(usize, usize), bool>,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    for arg in args {
        mark_readonly_declaration_operand(arg, flags, traversal)?;
    }
    Ok(())
}

/// Mark only the `Readonly` / `const` declaration operand.
///
/// Walk transparent wrappers around that operand (`=>` / assignment LHS), but do
/// not descend into initializer / RHS subtrees — nested declarations there are
/// unrelated locals and must stay non-readonly.
fn mark_readonly_declaration_operand(
    node: &Node,
    flags: &mut FxHashMap<(usize, usize), bool>,
    traversal: &mut TraversalState<'_, '_>,
) -> Result<(), TraversalStop> {
    traversal.admit_work()?;
    match &node.kind {
        NodeKind::VariableDeclaration { variable, .. } => {
            mark_declaration_target_flags(variable, true, flags, traversal)
        }
        NodeKind::VariableListDeclaration { variables, .. } => {
            for variable in variables {
                mark_declaration_target_flags(variable, true, flags, traversal)?;
            }
            Ok(())
        }
        // `Readonly my $x => EXPR` / `const my $x = EXPR`: only the LHS operand
        // is frozen by the wrapper call.
        NodeKind::Binary { left, .. } => {
            mark_readonly_declaration_operand(left, flags, traversal)
        }
        NodeKind::Assignment { lhs, .. } => {
            mark_readonly_declaration_operand(lhs, flags, traversal)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser_core::Parser;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Helper to create token tuple
    fn tok(line: u32, start: u32, len: u32, kind: u32, mods: u32) -> (u32, u32, u32, u32, u32) {
        (line, start, len, kind, mods)
    }

    fn finalized_raw_tokens(input: Vec<RawSemanticToken>) -> Vec<RawSemanticToken> {
        let control = SemanticTokensTraversalControl::unlimited();
        let mut traversal = TraversalState { control: &control, work_done: 0 };
        let encoded = match finalize_tokens_controlled(&input, &[], &mut traversal) {
            Ok(tokens) => tokens,
            Err(_) => return Vec::new(),
        };
        let mut line = 0u32;
        let mut character = 0u32;
        encoded
            .into_iter()
            .map(|[delta_line, delta_character, length, kind, modifiers]| {
                if delta_line == 0 {
                    character = character.saturating_add(delta_character);
                } else {
                    line = line.saturating_add(delta_line);
                    character = delta_character;
                }
                (line, character, length, kind, modifiers)
            })
            .collect()
    }

    fn pos16(source: &str, byte_offset: usize) -> (u32, u32) {
        let mut line = 0u32;
        let mut col = 0u32;
        for (idx, ch) in source.char_indices() {
            if idx >= byte_offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += ch.len_utf16() as u32;
            }
        }
        (line, col)
    }

    fn lexer_work_units(source: &str) -> usize {
        let mut lexer = PerlLexer::with_body_tokens(source);
        let mut work = 0usize;
        while lexer.next_token().is_some() {
            work = work.saturating_add(1);
        }
        work.saturating_add(1)
    }

    #[test]
    fn controlled_complete_output_matches_compatibility_collector()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "package Demo; my $value = 42; sub answer { return $value; }";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let legacy = collect_semantic_tokens(&ast, source, &|offset| pos16(source, offset));
        let controlled = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &SemanticTokensTraversalControl::unlimited(),
        );

        assert_eq!(controlled, SemanticTokensTraversalOutcome::Complete(legacy));
        Ok(())
    }

    #[test]
    fn complete_output_matches_frozen_protocol_vector() -> Result<(), Box<dyn std::error::Error>> {
        let source = "package Demo;\nmy $x = 42;\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;

        let outcome = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &SemanticTokensTraversalControl::unlimited(),
        );

        assert_eq!(
            outcome,
            SemanticTokensTraversalOutcome::Complete(vec![
                [0, 0, 7, 13, 0],
                [0, 8, 4, 0, 1],
                [1, 0, 2, 13, 0],
                [0, 3, 2, 11, 1025],
                [0, 3, 1, 19, 0],
                [0, 2, 2, 17, 0],
            ])
        );
        Ok(())
    }

    #[test]
    fn collection_boundary_distinguishes_no_ast_and_failure() {
        let control = SemanticTokensTraversalControl::unlimited();
        let no_ast = collect_semantic_tokens_from_input(
            SemanticTokensCollectionInput::NoAst,
            "",
            &|_| (0, 0),
            &control,
        );
        let failure = collect_semantic_tokens_from_input(
            SemanticTokensCollectionInput::CollectionFailure(SemanticTokensCollectionError::new(
                "parse failed",
            )),
            "",
            &|_| (0, 0),
            &control,
        );

        assert_eq!(no_ast, SemanticTokensTraversalOutcome::NoAst);
        assert_eq!(
            failure,
            SemanticTokensTraversalOutcome::CollectionFailure(SemanticTokensCollectionError::new(
                "parse failed"
            ))
        );
    }

    #[test]
    fn complete_heredoc_vectors_preserve_injected_language_boundaries()
    -> Result<(), Box<dyn std::error::Error>> {
        let painted = |source: &str,
                       kind: &str|
         -> Result<Vec<String>, Box<dyn std::error::Error>> {
            let mut parser = Parser::new(source);
            let ast = parser.parse()?;
            let kind = *legend().map.get(kind).ok_or("semantic-token kind missing")?;
            let lines: Vec<&str> = source.split('\n').collect();
            let mut line = 0u32;
            let mut column = 0u32;
            let mut result = Vec::new();
            for [delta_line, delta_column, length, token_type, _modifiers] in
                collect_semantic_tokens(&ast, source, &|offset| pos16(source, offset))
            {
                if delta_line == 0 {
                    column = column.saturating_add(delta_column);
                } else {
                    line = line.saturating_add(delta_line);
                    column = delta_column;
                }
                if token_type == kind {
                    let source_line = lines.get(line as usize).ok_or("token line missing")?;
                    result.push(
                        source_line.chars().skip(column as usize).take(length as usize).collect(),
                    );
                }
            }
            Ok(result)
        };

        let sql = "my $sql = <<SQL;\nSELECT id FROM users WHERE id = 1;\nSQL\n";
        assert_eq!(painted(sql, "sql_heredoc_keyword")?, vec!["SELECT", "FROM", "WHERE"]);

        let json = "my $json = <<JSON;\n{\"name\": \"Ada\", \"nested-key\": 1, \"not a key\": true}\nJSON\n";
        assert_eq!(
            painted(json, "json_heredoc_key")?,
            vec!["\"name\"", "\"nested-key\"", "\"not a key\""]
        );
        assert!(!painted(json, "json_heredoc_key")?.iter().any(|token| token == "value"));
        Ok(())
    }

    #[test]
    fn large_traversal_budget_covers_finalization() -> Result<(), Box<dyn std::error::Error>> {
        let source =
            (0..512).map(|index| format!("my $value_{index} = {index};\n")).collect::<String>();
        let mut parser = Parser::new(&source);
        let ast = parser.parse()?;
        let complete_polls = AtomicUsize::new(0);
        let never_cancelled = || {
            complete_polls.fetch_add(1, Ordering::Relaxed);
            false
        };
        let complete = collect_semantic_tokens_controlled(
            &ast,
            &source,
            &|offset| pos16(&source, offset),
            &SemanticTokensTraversalControl::new(&never_cancelled, None),
        );
        assert!(
            matches!(complete, SemanticTokensTraversalOutcome::Complete(_)),
            "an unlimited traversal must complete, got {complete:?}"
        );
        let total_work = complete_polls.load(Ordering::Relaxed);
        let budget = total_work.checked_sub(1).ok_or("complete traversal recorded no work")?;
        let bounded_never_cancelled = || false;
        let control = SemanticTokensTraversalControl::new(&bounded_never_cancelled, Some(budget));

        let bounded = collect_semantic_tokens_controlled(
            &ast,
            &source,
            &|offset| pos16(&source, offset),
            &control,
        );

        assert!(
            matches!(
                bounded,
                SemanticTokensTraversalOutcome::BudgetExhausted { work_done, .. }
                    if work_done == budget
            ),
            "a budget of {budget} must exhaust with work_done == {budget}, got {bounded:?}"
        );
        Ok(())
    }

    #[test]
    fn inner_scans_stop_at_the_exact_budget() -> Result<(), Box<dyn std::error::Error>> {
        let never_cancelled = || false;
        let find_control = SemanticTokensTraversalControl::new(&never_cancelled, Some(7));
        let mut find_traversal = TraversalState { control: &find_control, work_done: 0 };
        let find_result =
            find_bytes_at_controlled(b"aaaaaaaaaaaaaaaa-target", 0, b"target", &mut find_traversal);
        assert_eq!(find_result, Err(TraversalStop::BudgetExhausted));
        assert_eq!(find_traversal.work_done, 7);

        let json_control = SemanticTokensTraversalControl::new(&never_cancelled, Some(9));
        let mut json_traversal = TraversalState { control: &json_control, work_done: 0 };
        let mut tokens = Vec::new();
        let json_result = tokenize_json_body(
            r#"{"a-very-long-key": 1}"#,
            0,
            &|offset| (0, offset as u32),
            &legend(),
            &mut tokens,
            &mut json_traversal,
        );
        assert_eq!(json_result, Err(TraversalStop::BudgetExhausted));
        assert_eq!(json_traversal.work_done, 9);
        Ok(())
    }

    #[test]
    fn heredoc_scanners_preserve_json_and_sql_geometry() -> Result<(), Box<dyn std::error::Error>> {
        let control = SemanticTokensTraversalControl::unlimited();
        let mut traversal = TraversalState { control: &control, work_done: 0 };
        let mut json_tokens = Vec::new();
        tokenize_json_body(
            r#""key"   : 1, "a\"b": 2, "not-key""#,
            0,
            &|offset| (0, offset as u32),
            &legend(),
            &mut json_tokens,
            &mut traversal,
        )
        .map_err(|_| "unexpected JSON traversal stop")?;
        assert_eq!(json_tokens, vec![tok(0, 0, 8, 22, 0), tok(0, 13, 6, 22, 0)]);

        let mut recovered_tokens = Vec::new();
        tokenize_json_body(
            "\"unterminated\n{\"valid\": 1}",
            0,
            &|offset| (0, offset as u32),
            &legend(),
            &mut recovered_tokens,
            &mut traversal,
        )
        .map_err(|_| "unexpected malformed JSON traversal stop")?;
        assert_eq!(recovered_tokens, vec![tok(0, 15, 7, 22, 0)]);

        // Discriminator for the unterminated-reset seam: a long run of
        // backslash-escaped quotes with no unescaped closer must stay linear.
        // Resume-at-open+1 would re-scan the remaining suffix from each escape
        // quote and push work_done into O(n^2).
        let escaped_quote_runs = 128usize;
        let mut escaped_bomb = String::from('"');
        for _ in 0..escaped_quote_runs {
            escaped_bomb.push_str("\\\"");
        }
        let never_cancelled = || false;
        let bomb_control = SemanticTokensTraversalControl::new(&never_cancelled, None);
        let mut bomb_traversal = TraversalState { control: &bomb_control, work_done: 0 };
        let mut bomb_tokens = Vec::new();
        tokenize_json_body(
            &escaped_bomb,
            0,
            &|offset| (0, offset as u32),
            &legend(),
            &mut bomb_tokens,
            &mut bomb_traversal,
        )
        .map_err(|_| "unexpected escaped-quote bomb traversal stop")?;
        assert!(bomb_tokens.is_empty(), "unterminated escaped quotes emit no keys");
        let linear_ceiling = escaped_quote_runs.saturating_mul(4).saturating_add(8);
        assert!(
            bomb_traversal.work_done <= linear_ceiling,
            "unterminated escaped-quote scan must stay linear: work_done={} ceiling={}",
            bomb_traversal.work_done,
            linear_ceiling
        );

        let mut sql_tokens = Vec::new();
        tokenize_sql_body(
            "éSELECT SELECTé select notselect",
            0,
            &|offset| (0, offset as u32),
            &legend(),
            &mut sql_tokens,
            &mut traversal,
        )
        .map_err(|_| "unexpected SQL traversal stop")?;
        assert_eq!(sql_tokens, vec![tok(0, 18, 6, 21, 0)]);
        Ok(())
    }

    #[test]
    fn ast_child_enumeration_stops_before_eager_sibling_work()
    -> Result<(), Box<dyn std::error::Error>> {
        CONTROLLED_CHILD_EDGES_ENUMERATED.with(|count| count.set(0));
        let source = (0..512).map(|index| format!("{index};\n")).collect::<String>();
        let mut parser = Parser::new(&source);
        let ast = parser.parse()?;
        let never_cancelled = || false;
        let control = SemanticTokensTraversalControl::new(&never_cancelled, Some(1));
        let mut traversal = TraversalState { control: &control, work_done: 0 };
        let mut visited = 0usize;

        let result = walk_ast_full_controlled(&ast, &mut traversal, &mut |_| {
            visited = visited.saturating_add(1);
            true
        });

        assert_eq!(result, Err(TraversalStop::BudgetExhausted));
        assert_eq!(traversal.work_done, 1);
        assert_eq!(visited, 1, "only the admitted root may reach the visitor");
        assert_eq!(
            CONTROLLED_CHILD_EDGES_ENUMERATED.with(std::cell::Cell::get),
            1,
            "the production walker must enumerate only the first rejected child edge"
        );
        Ok(())
    }

    #[test]
    fn ast_child_enumeration_observes_cancellation_before_next_sibling()
    -> Result<(), Box<dyn std::error::Error>> {
        CONTROLLED_CHILD_EDGES_ENUMERATED.with(|count| count.set(0));
        let source = (0..512).map(|index| format!("{index};\n")).collect::<String>();
        let mut parser = Parser::new(&source);
        let ast = parser.parse()?;
        let polls = AtomicUsize::new(0);
        let cancellation = || polls.fetch_add(1, Ordering::Relaxed) >= 1;
        let control = SemanticTokensTraversalControl::new(&cancellation, None);
        let mut traversal = TraversalState { control: &control, work_done: 0 };
        let mut visited = 0usize;

        let result = walk_ast_full_controlled(&ast, &mut traversal, &mut |_| {
            visited = visited.saturating_add(1);
            true
        });

        assert_eq!(result, Err(TraversalStop::Cancelled));
        assert_eq!(traversal.work_done, 1);
        assert_eq!(visited, 1, "cancellation must stop before visiting a sibling");
        assert_eq!(polls.load(Ordering::Relaxed), 2);
        assert_eq!(
            CONTROLLED_CHILD_EDGES_ENUMERATED.with(std::cell::Cell::get),
            1,
            "cancellation must prevent enumeration of later sibling edges"
        );
        Ok(())
    }

    #[test]
    fn declaration_lookup_consumes_one_exact_work_unit() {
        let never_cancelled = || false;
        let control = SemanticTokensTraversalControl::new(&never_cancelled, Some(0));
        let mut traversal = TraversalState { control: &control, work_done: 0 };
        let mut flags = FxHashMap::default();
        flags.insert((4, 6), true);

        let result = declaration_flag(&flags, (4, 6), &mut traversal);

        assert_eq!(result, Err(TraversalStop::BudgetExhausted));
        assert_eq!(traversal.work_done, 0);
    }

    #[test]
    fn declaration_lookup_observes_cancellation_before_hash_work() {
        let cancelled = || true;
        let control = SemanticTokensTraversalControl::new(&cancelled, None);
        let mut traversal = TraversalState { control: &control, work_done: 0 };
        let mut flags = FxHashMap::default();
        flags.insert((4, 6), true);

        let result = declaration_flag(&flags, (4, 6), &mut traversal);

        assert_eq!(result, Err(TraversalStop::Cancelled));
        assert_eq!(traversal.work_done, 0);
    }

    #[test]
    fn controlled_collector_stops_inside_indexed_declaration_lookups()
    -> Result<(), Box<dyn std::error::Error>> {
        let source =
            (0..256).map(|index| format!("my $value_{index} = {index};\n")).collect::<String>();
        let mut parser = Parser::new(&source);
        let ast = parser.parse()?;

        DECLARATION_LOOKUPS_ATTEMPTED.with(|count| count.set(0));
        let cancel_during_second_lookup =
            || DECLARATION_LOOKUPS_ATTEMPTED.with(std::cell::Cell::get) >= 2;
        let cancelled = collect_semantic_tokens_controlled(
            &ast,
            &source,
            &|offset| pos16(&source, offset),
            &SemanticTokensTraversalControl::new(&cancel_during_second_lookup, None),
        );
        let SemanticTokensTraversalOutcome::Cancelled { work_done } = cancelled else {
            return Err("collector did not cancel during declaration lookup".into());
        };
        assert_eq!(
            DECLARATION_LOOKUPS_ATTEMPTED.with(std::cell::Cell::get),
            2,
            "the production overlay must route each variable through the indexed lookup"
        );

        DECLARATION_LOOKUPS_ATTEMPTED.with(|count| count.set(0));
        let never_cancelled = || false;
        let exhausted = collect_semantic_tokens_controlled(
            &ast,
            &source,
            &|offset| pos16(&source, offset),
            &SemanticTokensTraversalControl::new(&never_cancelled, Some(work_done)),
        );
        assert!(
            matches!(
                exhausted,
                SemanticTokensTraversalOutcome::BudgetExhausted {
                    work_done: exhausted_work,
                    ..
                } if exhausted_work == work_done
            ),
            "the exact cancellation prefix must also exhaust the budget at the lookup boundary"
        );
        assert_eq!(
            DECLARATION_LOOKUPS_ATTEMPTED.with(std::cell::Cell::get),
            2,
            "budget exhaustion must occur on the second indexed lookup"
        );
        Ok(())
    }

    #[test]
    fn controlled_collector_stops_inside_wrapped_declaration_indexing()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "my ($first, ($nested_a, $nested_b), $tagged :shared);\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;

        DECLARATION_INDEX_INSERTIONS_ATTEMPTED.with(|count| count.set(0));
        let cancel_during_second_insert =
            || DECLARATION_INDEX_INSERTIONS_ATTEMPTED.with(std::cell::Cell::get) >= 2;
        let cancelled = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &SemanticTokensTraversalControl::new(&cancel_during_second_insert, None),
        );
        let SemanticTokensTraversalOutcome::Cancelled { work_done } = cancelled else {
            return Err("collector did not cancel during wrapped declaration indexing".into());
        };
        assert_eq!(
            DECLARATION_INDEX_INSERTIONS_ATTEMPTED.with(std::cell::Cell::get),
            2,
            "production indexing must reach the second wrapped variable insertion"
        );

        DECLARATION_INDEX_INSERTIONS_ATTEMPTED.with(|count| count.set(0));
        let never_cancelled = || false;
        let exhausted = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &SemanticTokensTraversalControl::new(&never_cancelled, Some(work_done)),
        );
        assert!(
            matches!(
                exhausted,
                SemanticTokensTraversalOutcome::BudgetExhausted {
                    work_done: exhausted_work,
                    ..
                } if exhausted_work == work_done
            ),
            "the cancellation prefix must exhaust the exact budget at wrapped indexing"
        );
        assert_eq!(
            DECLARATION_INDEX_INSERTIONS_ATTEMPTED.with(std::cell::Cell::get),
            2,
            "budget exhaustion must occur on the second wrapped variable insertion"
        );
        Ok(())
    }

    #[test]
    fn declaration_index_complete_run_preserves_frozen_modifier_geometry()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = concat!(
            "use Const::Fast;\n",
            "const my $fast => 1;\n",
            "use Readonly;\n",
            "Readonly my ($left, $right) => (2, 3);\n",
            "my ($plain, $other);\n",
            "$plain = $fast + $left;\n",
        );
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let outcome = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &SemanticTokensTraversalControl::unlimited(),
        );
        let SemanticTokensTraversalOutcome::Complete(encoded) = outcome else {
            return Err("unlimited declaration fixture did not complete".into());
        };
        let variable_kind = kind_idx(&legend(), "variable");
        let mut line = 0u32;
        let mut character = 0u32;
        let variables = encoded
            .into_iter()
            .filter_map(|[delta_line, delta_character, length, kind, modifiers]| {
                if delta_line == 0 {
                    character = character.saturating_add(delta_character);
                } else {
                    line = line.saturating_add(delta_line);
                    character = delta_character;
                }
                (kind == variable_kind).then_some((line, character, length, modifiers))
            })
            .collect::<Vec<_>>();

        assert_eq!(
            variables,
            vec![
                (1, 9, 5, 1029),
                (3, 13, 5, 1029),
                (3, 20, 6, 1029),
                (4, 4, 6, 1025),
                (4, 12, 6, 1025),
                (5, 0, 6, 1152),
                (5, 9, 5, 1024),
                (5, 17, 5, 1024),
            ],
            "exact declaration spans must not leak declaration/readonly bits to later uses"
        );
        Ok(())
    }

    #[test]
    fn controlled_finalization_preserves_overlap_and_tie_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let control = SemanticTokensTraversalControl::unlimited();
        let mut traversal = TraversalState { control: &control, work_done: 0 };
        let ast = vec![tok(0, 0, 5, 7, 3), tok(0, 8, 2, 11, 0)];
        let lexer = vec![tok(0, 0, 5, 13, 0), tok(0, 4, 7, 16, 0), tok(1, 2, 3, 17, 0)];

        let encoded = finalize_tokens_controlled(&ast, &lexer, &mut traversal)
            .map_err(|_| "unexpected finalization stop")?;

        assert_eq!(encoded, vec![[0, 4, 7, 16, 0], [1, 2, 3, 17, 0]]);
        Ok(())
    }

    #[test]
    fn oversized_single_lexer_token_stops_before_opaque_lexing()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = format!("#{}", "x".repeat(MAX_BOUNDED_LEXER_SOURCE_BYTES));
        let mut parser = Parser::new("");
        let ast = parser.parse()?;
        let polls = AtomicUsize::new(0);
        let cancellation = || {
            polls.fetch_add(1, Ordering::Relaxed);
            false
        };
        let control = SemanticTokensTraversalControl::new(&cancellation, None);

        let outcome = collect_semantic_tokens_controlled(
            &ast,
            &source,
            &|offset| (0, offset as u32),
            &control,
        );

        assert!(matches!(
            outcome,
            SemanticTokensTraversalOutcome::SourceLimitExceeded {
                source_bytes,
                limit_bytes: MAX_BOUNDED_LEXER_SOURCE_BYTES,
            } if source_bytes == source.len()
        ));
        assert_eq!(polls.load(Ordering::Relaxed), 1);
        Ok(())
    }

    #[test]
    fn cancellation_stops_during_lexer_traversal_without_extra_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $first = 1; my $second = 2;";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let polls = AtomicUsize::new(0usize);
        let cancellation = || {
            let observed = polls.fetch_add(1, Ordering::Relaxed);
            observed >= 2
        };
        let control = SemanticTokensTraversalControl::new(&cancellation, None);

        let outcome = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &control,
        );

        assert_eq!(outcome, SemanticTokensTraversalOutcome::Cancelled { work_done: 2 });
        assert_eq!(polls.load(Ordering::Relaxed), 3);
        Ok(())
    }

    #[test]
    fn cancellation_stops_during_ast_traversal_without_extra_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "sub outer { my $x = 1; if ($x) { return $x; } }";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let lexer_work = lexer_work_units(source);
        let ast_work_before_cancel = 3usize;
        let cancel_after = lexer_work.saturating_add(ast_work_before_cancel);
        let polls = AtomicUsize::new(0usize);
        let cancellation = || {
            let observed = polls.fetch_add(1, Ordering::Relaxed);
            observed >= cancel_after
        };
        let control = SemanticTokensTraversalControl::new(&cancellation, None);

        let outcome = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &control,
        );

        assert_eq!(outcome, SemanticTokensTraversalOutcome::Cancelled { work_done: cancel_after });
        assert_eq!(polls.load(Ordering::Relaxed), cancel_after.saturating_add(1));
        Ok(())
    }

    #[test]
    fn budget_exhaustion_reports_exact_work_and_incomplete_partial_tokens()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $first = 1; my $second = 2;";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let never_cancelled = || false;
        let budget = 4usize;
        let control = SemanticTokensTraversalControl::new(&never_cancelled, Some(budget));

        let outcome = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &control,
        );

        match outcome {
            SemanticTokensTraversalOutcome::BudgetExhausted { partial, work_done } => {
                assert_eq!(work_done, budget);
                assert!(
                    partial.raw_token_count() > 0,
                    "the exhausted outcome should retain collected tokens"
                );
            }
            SemanticTokensTraversalOutcome::Complete(_) => {
                return Err("budget-exhausted output was incorrectly marked complete".into());
            }
            SemanticTokensTraversalOutcome::Cancelled { .. } => {
                return Err("budget exhaustion was incorrectly reported as cancellation".into());
            }
            SemanticTokensTraversalOutcome::NoAst
            | SemanticTokensTraversalOutcome::CollectionFailure(_)
            | SemanticTokensTraversalOutcome::SourceLimitExceeded { .. } => {
                return Err("budget exhaustion was incorrectly reported as input failure".into());
            }
        }
        Ok(())
    }

    #[test]
    fn zero_budget_admits_no_work() -> Result<(), Box<dyn std::error::Error>> {
        let source = "my $value = 1;";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let never_cancelled = || false;
        let control = SemanticTokensTraversalControl::new(&never_cancelled, Some(0));

        let outcome = collect_semantic_tokens_controlled(
            &ast,
            source,
            &|offset| pos16(source, offset),
            &control,
        );

        assert_eq!(
            outcome,
            SemanticTokensTraversalOutcome::BudgetExhausted {
                partial: PartialSemanticTokens { ast_tokens: Vec::new(), lexer_tokens: Vec::new() },
                work_done: 0,
            }
        );
        Ok(())
    }

    #[test]
    fn collect_semantic_tokens_emits_label_for_labeled_statement_and_control_reference()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = "OUTER: while ($x) {\n    last OUTER;\n}\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let tokens = collect_semantic_tokens(&ast, source, &|offset| pos16(source, offset));
        let label_idx = *legend().map.get("label").ok_or("label token type missing from legend")?;

        let mut line = 0u32;
        let mut col = 0u32;
        let mut labels = Vec::new();
        for [delta_line, delta_start, length, token_type, modifiers] in tokens {
            if delta_line == 0 {
                col = col.saturating_add(delta_start);
            } else {
                line = line.saturating_add(delta_line);
                col = delta_start;
            }
            if token_type == label_idx {
                labels.push((line, col, length, modifiers));
            }
        }

        assert!(labels.contains(&(0, 0, 5, 1)));
        assert!(labels.contains(&(1, 9, 5, 0)));
        Ok(())
    }

    #[test]
    fn statement_label_offsets_rejects_embedded_or_non_label_matches()
    -> Result<(), Box<dyn std::error::Error>> {
        let embedded = "MYOUTER: while ($x) {}\n";
        assert_eq!(statement_label_offsets(embedded, 0, embedded.len(), "OUTER"), None);

        let no_colon = "OUTER while ($x) {}\n";
        assert_eq!(statement_label_offsets(no_colon, 0, no_colon.len(), "OUTER"), None);

        let whitespace = "OUTER : while ($x) {}\n";
        assert_eq!(statement_label_offsets(whitespace, 0, whitespace.len(), "OUTER"), Some((0, 5)));

        Ok(())
    }

    #[test]
    fn loop_control_label_offsets_skips_embedded_matches() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = "last OUTERLY OUTER;\n";
        assert_eq!(
            loop_control_label_offsets(source, 0, source.len(), "last", "OUTER"),
            Some((13, 18))
        );

        let embedded_only = "last OUTERLY;\n";
        assert_eq!(
            loop_control_label_offsets(embedded_only, 0, embedded_only.len(), "last", "OUTER"),
            None
        );

        Ok(())
    }

    #[test]
    fn test_remove_overlapping_tokens_basic() {
        // No overlap
        let input = vec![tok(0, 0, 5, 0, 0), tok(0, 6, 5, 0, 0)];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result, input);
    }

    #[test]
    fn test_remove_overlapping_tokens_touching() {
        // Touching is NOT overlap
        // [0, 5) and [5, 10)
        let input = vec![tok(0, 0, 5, 0, 0), tok(0, 5, 5, 0, 0)];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result, input);
    }

    #[test]
    fn test_remove_overlapping_tokens_nested_keep_outer() {
        // Outer [0, 10), Inner [2, 5)
        // Inner length 3 < Outer length 10
        // Expect Outer kept
        let input = vec![tok(0, 0, 10, 0, 0), tok(0, 2, 3, 1, 0)];
        // Sorted: Outer, Inner
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], tok(0, 0, 10, 0, 0));
    }

    #[test]
    fn test_remove_overlapping_tokens_nested_keep_longer_inner_replacement() {
        // Functionally: A [0, 5), B [0, 10)
        // Sorted: A, B
        // Expect B (longer) replaces A
        let input = vec![tok(0, 0, 5, 0, 0), tok(0, 0, 10, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], tok(0, 0, 10, 1, 0));
    }

    #[test]
    fn test_remove_overlapping_tokens_overlap_tail_keep_longer() {
        // A [0, 5) len 5
        // B [4, 10) len 6
        // Overlap at 4. B is longer.
        // Expect A replaced by B.
        let input = vec![tok(0, 0, 5, 0, 0), tok(0, 4, 6, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], tok(0, 4, 6, 1, 0));
    }

    #[test]
    fn test_remove_overlapping_tokens_overlap_tail_keep_earlier_if_longer() {
        // A [0, 10) len 10
        // B [8, 15) len 7
        // Overlap at 8. A is longer.
        // Expect A kept, B dropped.
        let input = vec![tok(0, 0, 10, 0, 0), tok(0, 8, 7, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], tok(0, 0, 10, 0, 0));
    }

    #[test]
    fn test_remove_overlapping_tokens_equal_length_keep_first() {
        // A [0, 5) len 5
        // B [0, 5) len 5
        // Expect A kept (first one)
        let input = vec![tok(0, 0, 5, 1, 0), tok(0, 0, 5, 2, 0)];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], tok(0, 0, 5, 1, 0));
    }

    #[test]
    fn test_remove_overlapping_tokens_different_lines() {
        let input = vec![tok(0, 0, 5, 0, 0), tok(1, 0, 5, 0, 0)];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result, input);
    }

    // ==================== Mutation Hardening Tests (Issue #155) ====================
    // These tests target specific mutation survivors identified in mutation analysis
    // Focus: FnValue mutations (71%) and BinaryOperator mutations (25%)

    /// Test that empty input produces empty output
    /// Kills FnValue mutations on return statement
    #[test]
    fn mutation_hardening_empty_input() {
        let input = vec![];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 0, "Empty input must produce empty output");
    }

    /// Test single token passes through unchanged
    /// Kills FnValue mutations on result.push() at line 333
    #[test]
    fn mutation_hardening_single_token() {
        let input = vec![tok(0, 0, 5, 0, 0)];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result.len(), 1, "Single token must be preserved");
        assert_eq!(result[0], input[0], "Single token must match input exactly");
    }

    /// Test two non-overlapping tokens on same line
    /// Kills BinaryOperator mutations on `start_char < last_start + last_length` comparison
    #[test]
    fn mutation_hardening_adjacent_non_overlapping() {
        // Token A: [0, 5), Token B: [5, 10) - touching but not overlapping
        let input = vec![tok(0, 0, 5, 0, 0), tok(0, 5, 5, 1, 0)];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result.len(), 2, "Adjacent non-overlapping tokens must both be kept");
        assert_eq!(result[0], tok(0, 0, 5, 0, 0));
        assert_eq!(result[1], tok(0, 5, 5, 1, 0));
    }

    /// Test exact boundary case: token end equals next token start
    /// Kills BinaryOperator mutations on boundary comparisons
    #[test]
    fn mutation_hardening_exact_boundary() {
        // Token A: [10, 15), Token B: [15, 20) - exact boundary
        let input = vec![tok(0, 10, 5, 0, 0), tok(0, 15, 5, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 2, "Tokens with exact boundaries must not overlap");
    }

    /// Test one-character overlap triggers replacement
    /// Kills BinaryOperator mutations on overlap detection (< vs <=)
    #[test]
    fn mutation_hardening_single_char_overlap() {
        // Token A: [0, 6), Token B: [5, 10) - overlap by 1 char at position 5
        // A is kept because it comes first and B is not longer (A=6, B=5)
        let input = vec![tok(0, 0, 6, 0, 0), tok(0, 5, 5, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1, "Single char overlap must trigger deduplication");
        assert_eq!(result[0], tok(0, 0, 6, 0, 0), "First token kept (longer)");
    }

    /// Test partial overlap with length comparison
    /// Kills BinaryOperator mutations on `length > last_length` at line 324
    #[test]
    fn mutation_hardening_partial_overlap_length_determines_winner() {
        // Token A: [0, 5) len=5, Token B: [3, 10) len=7 - partial overlap, B longer
        let input = vec![tok(0, 0, 5, 0, 0), tok(0, 3, 7, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1, "Partial overlap must keep only one token");
        assert_eq!(result[0], tok(0, 3, 7, 1, 0), "Longer overlapping token must win");
    }

    /// Test equal length overlap keeps first token
    /// Kills BinaryOperator mutations on equality in length comparison
    #[test]
    fn mutation_hardening_equal_length_keeps_first() {
        // Token A: [0, 5) len=5, Token B: [2, 7) len=5 - equal length overlap
        let input = vec![tok(0, 0, 5, 0, 0), tok(0, 2, 5, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1, "Equal length overlap must keep first token");
        assert_eq!(result[0], tok(0, 0, 5, 0, 0), "First token must be kept when lengths equal");
    }

    #[test]
    fn walk_ast_full_matches_canonical_ast_children() -> Result<(), Box<dyn std::error::Error>> {
        let source = r#"
sub demo ($arg = 1) { return $arg if $arg = 5; }
print "ok" unless $x;
print "ok" while $y;
print "ok" until $z;
print "ok" for @xs;
print "ok" foreach @ys;
"#;
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;

        let mut visited = 0usize;
        let completed = walk_ast_full(&ast, &mut |_| {
            visited += 1;
            true
        });
        assert!(completed);
        assert_eq!(visited, ast.count_nodes());
        Ok(())
    }

    /// Test tokens on different lines never overlap
    /// Kills BinaryOperator mutations on `line == last_line` comparison at line 322
    #[test]
    fn mutation_hardening_different_lines_no_overlap() {
        let input = vec![
            tok(0, 0, 100, 0, 0), // Line 0, very long token
            tok(1, 0, 5, 1, 0),   // Line 1, early position
        ];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result.len(), 2, "Tokens on different lines must never overlap");
        assert_eq!(result[0], tok(0, 0, 100, 0, 0));
        assert_eq!(result[1], tok(1, 0, 5, 1, 0));
    }

    /// Test three tokens with cascading overlaps
    /// Kills FnValue mutations on multiple push operations
    #[test]
    fn mutation_hardening_three_tokens_cascading() {
        // A: [0, 5), B: [4, 9), C: [8, 12) - A overlaps B, B would overlap C
        let input = vec![
            tok(0, 0, 5, 0, 0), // len=5
            tok(0, 4, 5, 1, 0), // len=5 (overlaps A)
            tok(0, 8, 4, 2, 0), // len=4 (would overlap B)
        ];
        let result = finalized_raw_tokens(input);
        // A is kept (4 < 0+5, but 5 > 5 is false, so B is skipped)
        // C doesn't overlap A (8 < 0+5 is false), so C is kept
        assert_eq!(result.len(), 2, "First and third tokens kept");
        assert_eq!(result[0], tok(0, 0, 5, 0, 0));
        assert_eq!(result[1], tok(0, 8, 4, 2, 0));
    }

    /// Test zero-length token handling
    /// Kills FnValue mutations and edge case handling
    #[test]
    fn mutation_hardening_zero_length_token() {
        let input = vec![
            tok(0, 5, 0, 0, 0), // Zero-length token [5, 5)
            tok(0, 5, 5, 1, 0), // Normal token at same position [5, 10)
        ];
        let result = finalized_raw_tokens(input);
        // Zero-length token [5,5) doesn't overlap with [5,10) per < check (5 < 5+0 is false)
        assert_eq!(
            result.len(),
            2,
            "Zero-length token at same position doesn't technically overlap"
        );
        assert_eq!(result[0], tok(0, 5, 0, 0, 0));
        assert_eq!(result[1], tok(0, 5, 5, 1, 0));
    }

    /// Test multiple zero-length tokens
    /// Kills FnValue mutations in edge cases
    #[test]
    fn mutation_hardening_multiple_zero_length() {
        let input = vec![tok(0, 5, 0, 0, 0), tok(0, 5, 0, 1, 0), tok(0, 5, 0, 2, 0)];
        let result = finalized_raw_tokens(input);
        // Zero-length tokens at same position don't overlap each other (5 < 5+0 is false)
        assert_eq!(result.len(), 3, "Multiple zero-length tokens are all kept");
    }

    /// Test large position values don't cause arithmetic overflow
    /// Kills BinaryOperator mutations in arithmetic operations
    #[test]
    fn mutation_hardening_large_positions() {
        let input = vec![tok(1000, u32::MAX - 100, 50, 0, 0), tok(1000, u32::MAX - 40, 20, 1, 0)];
        let result = finalized_raw_tokens(input);
        // Overflow is prevented by saturating operations in the original code
        assert_eq!(result.len(), 2, "Large positions must not cause overflow issues");
    }

    /// Test sorting preserves token order correctly
    /// Kills BinaryOperator mutations in sort_by_key at line 310
    #[test]
    fn mutation_hardening_sort_order() {
        // Input in reverse order
        let input = vec![tok(2, 10, 5, 0, 0), tok(1, 10, 5, 1, 0), tok(0, 10, 5, 2, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 3, "Non-overlapping tokens must all be preserved");
        // Verify sorted by line
        assert_eq!(result[0].0, 0);
        assert_eq!(result[1].0, 1);
        assert_eq!(result[2].0, 2);
    }

    /// Test sort order within same line
    /// Kills BinaryOperator mutations in sort comparisons
    #[test]
    fn mutation_hardening_sort_order_same_line() {
        // Input with tokens in reverse order on same line
        let input = vec![tok(0, 30, 5, 0, 0), tok(0, 20, 5, 1, 0), tok(0, 10, 5, 2, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 3, "Non-overlapping tokens must all be preserved");
        // Verify sorted by start position
        assert_eq!(result[0].1, 10);
        assert_eq!(result[1].1, 20);
        assert_eq!(result[2].1, 30);
    }

    /// Test multiple overlaps where shorter tokens are systematically removed
    /// Kills FnValue mutations on conditional push operations
    #[test]
    fn mutation_hardening_systematic_removal() {
        // All tokens overlap at same position, increasing length
        let input =
            vec![tok(0, 0, 3, 0, 0), tok(0, 0, 5, 1, 0), tok(0, 0, 7, 2, 0), tok(0, 0, 9, 3, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1, "Longest token must survive multiple replacements");
        assert_eq!(result[0], tok(0, 0, 9, 3, 0), "Longest token must be the survivor");
    }

    /// Test interleaved tokens without overlap
    /// Kills FnValue mutations on else branch at line 330
    #[test]
    fn mutation_hardening_interleaved_no_overlap() {
        let input = vec![
            tok(0, 0, 3, 0, 0),  // [0, 3)
            tok(0, 5, 3, 1, 0),  // [5, 8)
            tok(0, 10, 3, 2, 0), // [10, 13)
            tok(0, 15, 3, 3, 0), // [15, 18)
        ];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result.len(), 4, "All non-overlapping tokens must be preserved");
        assert_eq!(result, input, "Token order and content must be unchanged");
    }

    /// Test overlap at exactly boundary minus one
    /// Kills off-by-one errors in BinaryOperator mutations
    #[test]
    fn mutation_hardening_boundary_minus_one() {
        // Token A: [0, 10), Token B: [9, 15) - overlap at position 9
        let input = vec![tok(0, 0, 10, 0, 0), tok(0, 9, 6, 1, 0)];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 1, "Boundary-1 overlap must be detected");
        assert_eq!(result[0], tok(0, 0, 10, 0, 0), "First longer token wins");
    }

    /// Test that token type and modifiers are preserved correctly
    /// Kills mutations that might affect non-position fields
    #[test]
    fn mutation_hardening_preserves_metadata() {
        let input = vec![
            tok(0, 0, 5, 42, 7), // Specific type and modifiers
        ];
        let result = finalized_raw_tokens(input.clone());
        assert_eq!(result[0].3, 42, "Token type must be preserved");
        assert_eq!(result[0].4, 7, "Token modifiers must be preserved");
    }

    /// Test mixed line and position sorting
    /// Kills complex BinaryOperator mutations in sort logic
    #[test]
    fn mutation_hardening_mixed_line_position_sort() {
        let input = vec![
            tok(2, 5, 3, 0, 0),
            tok(0, 15, 3, 1, 0),
            tok(1, 10, 3, 2, 0),
            tok(0, 5, 3, 3, 0),
            tok(2, 0, 3, 4, 0),
        ];
        let result = finalized_raw_tokens(input);
        assert_eq!(result.len(), 5);
        // Verify primary sort by line
        assert!(result[0].0 <= result[1].0);
        assert!(result[1].0 <= result[2].0);
        // Verify secondary sort by position within same line
        for i in 1..result.len() {
            if result[i].0 == result[i - 1].0 {
                assert!(
                    result[i].1 >= result[i - 1].1,
                    "Tokens on same line must be sorted by position"
                );
            }
        }
    }

    /// Regression: the `method` semantic token must paint exactly the method name
    /// regardless of whitespace/newlines around `->`. Perl treats such whitespace as
    /// insignificant (external oracle: perldoc perlop — "whitespace is insignificant"
    /// around the arrow operator). Pre-fix, the hard-coded `object.location.end + 2`
    /// mislocated the span: `$obj ->name` painted ">nam", `$obj-> name` painted " nam",
    /// and the leading-arrow chain painted the indent+arrow instead of the method name.
    #[test]
    fn repro_method_token_spacing_paints_method_name_regardless_of_arrow_whitespace()
    -> Result<(), Box<dyn std::error::Error>> {
        let method_idx =
            *legend().map.get("method").ok_or("method token type missing from legend")?;

        // Drive the real provider and decode the source substrings painted by
        // `method` tokens (inputs are ASCII, so utf16 columns == byte/char columns).
        let painted_methods = |source: &str| -> Result<Vec<String>, Box<dyn std::error::Error>> {
            let mut parser = Parser::new(source);
            let ast = parser.parse()?;
            let tokens = collect_semantic_tokens(&ast, source, &|offset| pos16(source, offset));
            let lines: Vec<&str> = source.split('\n').collect();
            let mut line = 0u32;
            let mut col = 0u32;
            let mut painted = Vec::new();
            for [delta_line, delta_start, length, token_type, _mods] in tokens {
                if delta_line == 0 {
                    col = col.saturating_add(delta_start);
                } else {
                    line = line.saturating_add(delta_line);
                    col = delta_start;
                }
                if token_type == method_idx {
                    let src_line = lines.get(line as usize).ok_or("token line out of range")?;
                    let start = col as usize;
                    let chars: String =
                        src_line.chars().skip(start).take(length as usize).collect();
                    painted.push(chars);
                }
            }
            Ok(painted)
        };

        assert_eq!(painted_methods("$obj->name;")?, vec!["name".to_string()]);
        assert_eq!(painted_methods("$obj ->name;")?, vec!["name".to_string()]);
        assert_eq!(painted_methods("$obj-> name;")?, vec!["name".to_string()]);
        assert_eq!(painted_methods("$obj -> name;")?, vec!["name".to_string()]);

        // Leading-arrow multi-line method chain: both method names paint exactly.
        let chain = "$dbh\n    ->prepare($sql)\n    ->execute;";
        let painted = painted_methods(chain)?;
        assert!(
            painted.contains(&"prepare".to_string()),
            "expected `prepare` painted, got {painted:?}"
        );
        assert!(
            painted.contains(&"execute".to_string()),
            "expected `execute` painted, got {painted:?}"
        );

        Ok(())
    }

    /// Regression guard for the `modification` semantic-token modifier (issue #2810).
    ///
    /// The modifier (bit 7 = 128) distinguishes write from read occurrences of a
    /// variable: the direct LHS of an assignment (`$x = ...`) is a write and must
    /// carry the bit, while a plain read (`my $y = $x`) must not. A `my $x = ...`
    /// declaration is tagged with declaration modifiers, not `modification`.
    ///
    /// The feature was implemented (legend bit 7 at :201, `assignment_lhs_spans`,
    /// application at :896) but had no test pinning the read-vs-write distinction,
    /// so a regression in the LHS-span detection would have gone unnoticed.
    #[test]
    fn assignment_lhs_variable_carries_modification_modifier()
    -> Result<(), Box<dyn std::error::Error>> {
        const MODIFICATION_BIT: u32 = 128; // bit 7

        let variable_idx =
            *legend().map.get("variable").ok_or("variable token type missing from legend")?;

        // Line 0: declaration (declaration modifiers, NOT modification)
        // Line 1: reassignment — LHS write, modification bit set
        // Line 2: `$x` read on the RHS of another declaration — no modification bit
        let source = "my $x = 10;\n$x = 20;\nmy $y = $x;\n";
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        let tokens = collect_semantic_tokens(&ast, source, &|offset| pos16(source, offset));

        let lines: Vec<&str> = source.split('\n').collect();
        let mut line = 0u32;
        let mut col = 0u32;
        // (line, painted substring, modifier bitfield) for each `variable` token.
        let mut vars: Vec<(u32, String, u32)> = Vec::new();
        for [delta_line, delta_start, length, token_type, mods] in tokens {
            if delta_line == 0 {
                col = col.saturating_add(delta_start);
            } else {
                line = line.saturating_add(delta_line);
                col = delta_start;
            }
            if token_type == variable_idx {
                let src_line = lines.get(line as usize).ok_or("token line out of range")?;
                let painted: String =
                    src_line.chars().skip(col as usize).take(length as usize).collect();
                vars.push((line, painted, mods));
            }
        }

        let var_on = |ln: u32| -> Result<u32, Box<dyn std::error::Error>> {
            vars.iter()
                .find(|(l, painted, _)| *l == ln && painted == "$x")
                .map(|(_, _, mods)| *mods)
                .ok_or_else(|| format!("no `$x` variable token on line {ln}; got {vars:?}").into())
        };

        let decl_mods = var_on(0)?;
        let write_mods = var_on(1)?;
        let read_mods = var_on(2)?;

        assert_eq!(
            write_mods & MODIFICATION_BIT,
            MODIFICATION_BIT,
            "assignment LHS `$x` (`$x = 20`) must carry the modification modifier (bit 7), got mods={write_mods}"
        );
        assert_eq!(
            read_mods & MODIFICATION_BIT,
            0,
            "read `$x` (`my $y = $x`) must NOT carry the modification modifier, got mods={read_mods}"
        );
        assert_eq!(
            decl_mods & MODIFICATION_BIT,
            0,
            "declaration `my $x` must NOT carry the modification modifier (it is a declaration), got mods={decl_mods}"
        );

        Ok(())
    }

    /// Helper: collect the modifier bits for the first `$name` variable token.
    fn first_var_mods(source: &str, name: &str) -> u32 {
        let mut parser = Parser::new(source);
        let ast = parser.parse().expect("parse");
        let tokens = collect_semantic_tokens(&ast, source, &|offset| pos16(source, offset));
        let lines: Vec<&str> = source.split('\n').collect();
        let mut line = 0u32;
        let mut col = 0u32;
        let variable_idx = *legend().map.get("variable").expect("variable in legend");
        let target = format!("${name}");
        for [delta_line, delta_start, length, token_type, mods] in tokens {
            if delta_line == 0 {
                col = col.saturating_add(delta_start);
            } else {
                line = line.saturating_add(delta_line);
                col = delta_start;
            }
            if token_type == variable_idx {
                let src_line = lines.get(line as usize).expect("line in range");
                let painted: String =
                    src_line.chars().skip(col as usize).take(length as usize).collect();
                if painted == target {
                    return mods;
                }
            }
        }
        panic!("no `${name}` variable token found in source: {source:?}");
    }

    const DECLARATION_BIT: u32 = 1; // bit 0
    const READONLY_BIT: u32 = 4; // bit 2

    /// #4968 Slice 1: `my`, `state`, `local`, `our` carry `declaration` but NOT
    /// `readonly` — they are mutable lexical/package variables.
    #[test]
    fn mutable_declarations_carry_declaration_but_not_readonly() {
        for (declarator, source) in [
            ("my", "my $x = 1;\n"),
            ("state", "use feature 'state'; state $x = 1;\n"),
            ("local", "local $x;\n"),
            ("our", "our $x;\n"),
        ] {
            let mods = first_var_mods(source, "x");
            assert_eq!(
                mods & DECLARATION_BIT,
                DECLARATION_BIT,
                "`{declarator} $x` must carry the declaration modifier (bit 0), got mods={mods}"
            );
            assert_eq!(
                mods & READONLY_BIT,
                0,
                "`{declarator} $x` must NOT carry the readonly modifier (it is mutable), got mods={mods}"
            );
        }
    }

    /// #4968 Slice 1: `Const::Fast` declarations carry both `declaration` and
    /// `readonly` modifiers.
    #[test]
    fn const_fast_declaration_carries_readonly_modifier() {
        let source = "use Const::Fast;\nconst my $x => 42;\n";
        let mods = first_var_mods(source, "x");
        assert_eq!(
            mods & DECLARATION_BIT,
            DECLARATION_BIT,
            "Const::Fast `const my $x` must carry declaration (bit 0), got mods={mods}"
        );
        assert_eq!(
            mods & READONLY_BIT,
            READONLY_BIT,
            "Const::Fast `const my $x` must carry readonly (bit 2), got mods={mods}"
        );
    }

    /// #4968 Slice 1: `Readonly` declarations carry both `declaration` and
    /// `readonly` modifiers. Uses the `Readonly my $x => ...` form that
    /// `mark_readonly_declaration_flags` detects (FunctionCall name == "Readonly").
    #[test]
    fn readonly_declaration_carries_readonly_modifier() {
        let source = "use Readonly;\nReadonly my $x => 42;\n";
        let mods = first_var_mods(source, "x");
        assert_eq!(
            mods & DECLARATION_BIT,
            DECLARATION_BIT,
            "Readonly declaration must carry declaration (bit 0), got mods={mods}"
        );
        assert_eq!(
            mods & READONLY_BIT,
            READONLY_BIT,
            "Readonly declaration must carry readonly (bit 2), got mods={mods}"
        );
    }

    #[test]
    fn wrapped_declarations_preserve_frozen_modifier_bits() {
        for (source, name, expected_modifiers) in [
            ("my ($a, ($nested, $deep));\n", "nested", 1025),
            ("my ($tagged :shared, $plain);\n", "tagged", 1025),
            (
                "use Const::Fast;\nconst my ($fast, ($nested_fast)) => (1, 2);\n",
                "nested_fast",
                1029,
            ),
        ] {
            assert_eq!(
                first_var_mods(source, name),
                expected_modifiers,
                "wrapped declaration `${name}` must preserve its frozen modifiers"
            );
        }

        let readonly_source =
            "use Readonly;\nReadonly my ($tagged_ro :shared, $plain_ro) => (1, 2);\n";
        assert_eq!(
            first_var_mods(readonly_source, "tagged_ro"),
            1029,
            "wrapped Readonly declaration must emit the attributed variable with frozen modifiers"
        );
        assert_eq!(
            first_var_mods(readonly_source, "plain_ro"),
            1029,
            "wrapped Readonly declaration must emit every variable with frozen modifiers"
        );

        // Nested locals inside the Readonly/const initializer must not inherit
        // the frozen modifier from the outer wrapped declaration.
        let nested_local_source =
            "use Readonly;\nReadonly my $outer => do { my $tmp = 1; $tmp };\n";
        assert_eq!(
            first_var_mods(nested_local_source, "outer"),
            1029,
            "outer Readonly declaration operand must stay frozen"
        );
        let tmp_mods = first_var_mods(nested_local_source, "tmp");
        assert_eq!(
            tmp_mods & READONLY_BIT,
            0,
            "nested local inside Readonly RHS must not inherit readonly, got mods={tmp_mods}"
        );
        assert_eq!(
            tmp_mods & DECLARATION_BIT,
            DECLARATION_BIT,
            "nested local inside Readonly RHS must still be a declaration, got mods={tmp_mods}"
        );
    }
}
