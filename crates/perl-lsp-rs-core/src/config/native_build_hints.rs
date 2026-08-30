//! Conservative native build hint extraction.
//!
//! This module looks for literal `Makefile.PL` / `Build.PL` hints at the
//! workspace root and extracts the ExtUtils::MakeMaker-style keys relevant to
//! native builds: `INC`, `LIBS`, `DEFINE`, `OBJECT`, and `MYEXTLIB`, plus the
//! Module::Build equivalents `include_dirs` and `extra_compiler_flags`. It does
//! not execute Perl and does not model full build metadata.
//!
//! Only literal quoted strings and literal arrays of quoted strings are
//! understood, and a scalar value must end the assignment (`,` `;` `)` `]`
//! `}`, end-of-line/file, or a trailing comment). Interpolating double-quoted
//! strings (`"..."$var`, `"@list"`), concatenations such as `'a' . 'b'`,
//! barewords, `q()` forms on the right-hand side, function calls,
//! unterminated strings, and arrays containing non-literals all fail closed:
//! the offending occurrence contributes no hint values and is reported through
//! a named [`NativeBuildHintDiagnostic`] so callers can see why hints stayed
//! empty. Key-shaped text inside Perl quote-like operators (`q()`, `qw()`,
//! `s///`, ...) is skipped as string content, never treated as an assignment.
//!
//! Residual limitation: heredoc bodies and POD paragraphs are not modeled;
//! key-shaped text inside them can still be read as an assignment.

use std::fs;
use std::path::Path;

/// Native build hints derived from workspace-root build scripts.
///
/// Every vector holds de-duplicated entries in deterministic scan order
/// (`Makefile.PL` first, then `Build.PL`). Off-type literal tokens (for
/// example a definition flag inside `LIBS`) are conservatively ignored rather
/// than diagnosed: they are well-formed literals the static parser cannot
/// resolve to the concrete input kind the field promises.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeBuildHints {
    /// Native include directories discovered from static build-script hints.
    pub include_dirs: Vec<String>,
    /// Library link inputs (`-l*` / `-L*` flags, MSVC `*.lib` names) declared
    /// through the `LIBS` key.
    pub libs_flags: Vec<String>,
    /// Ordered alternative linker specifications from `LIBS` array entries.
    /// Each inner vector is one complete MakeMaker candidate.
    pub libs_alternatives: Vec<Vec<String>>,
    /// Preprocessor definition flags (`-D*`) declared through the `DEFINE`
    /// key.
    pub define_flags: Vec<String>,
    /// Object files (`*.o` / `*.obj`) declared through the `OBJECT` key.
    pub object_files: Vec<String>,
    /// Static archives or import libraries (`*.a` / `*.lib`) declared through
    /// the `MYEXTLIB` key.
    pub myextlib_files: Vec<String>,
    /// Malformed literals recorded while failing closed. Empty when every
    /// scanned assignment was either supported or off-type.
    pub diagnostics: Vec<NativeBuildHintDiagnostic>,
}

/// Named diagnostic for a build-script literal that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeBuildHintDiagnostic {
    /// Workspace-root build script containing the malformed assignment.
    pub script: NativeBuildScript,
    /// Literal assignment key whose value failed to parse (for example
    /// `"LIBS"`).
    pub key: &'static str,
    /// Why the literal value was rejected.
    pub reason: NativeBuildHintParseReason,
}

/// Workspace-root build script that produced native build hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBuildScript {
    /// `Makefile.PL` (ExtUtils::MakeMaker).
    MakefilePl,
    /// `Build.PL` (Module::Build or a hybrid authoring script).
    BuildPl,
}

impl NativeBuildScript {
    /// Workspace-root-relative file name of this build script.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::MakefilePl => "Makefile.PL",
            Self::BuildPl => "Build.PL",
        }
    }
}

/// Why a literal build-hint value could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeBuildHintParseReason {
    /// The value after `KEY =>` was not a quoted string or an array of quoted
    /// strings (for example a bareword, `q()` form, or function call).
    UnsupportedValueForm,
    /// A quoted string was never terminated: it ran past an unescaped newline
    /// or past the end of the file.
    UnterminatedStringLiteral,
    /// An array literal contained a non-literal element, lacked a separator
    /// comma, or was never closed.
    MalformedArrayLiteral,
}

/// Detect literal native build hints from workspace-root `Makefile.PL` /
/// `Build.PL`.
pub fn detect_native_build_hints(workspace_root: &Path) -> NativeBuildHints {
    let mut hints = NativeBuildHints::default();

    for script in [NativeBuildScript::MakefilePl, NativeBuildScript::BuildPl] {
        let script_path = workspace_root.join(script.file_name());
        if let Ok(source) = fs::read_to_string(&script_path) {
            merge_script_hints(&mut hints, script, &source);
        }
    }

    hints
}

fn merge_script_hints(hints: &mut NativeBuildHints, script: NativeBuildScript, source: &str) {
    match script {
        NativeBuildScript::MakefilePl => {
            merge_include_dirs(hints, script, source, "INC", flatten_include_flags);
        }
        NativeBuildScript::BuildPl => {
            merge_include_dirs(hints, script, source, "include_dirs", keep_value_verbatim);
            merge_include_dirs(
                hints,
                script,
                source,
                "extra_compiler_flags",
                flatten_include_flags,
            );
        }
    }

    for key in ["LIBS", "DEFINE", "OBJECT", "MYEXTLIB"] {
        merge_typed_key(hints, script, source, key);
    }
}

fn merge_include_dirs(
    hints: &mut NativeBuildHints,
    script: NativeBuildScript,
    source: &str,
    key: &'static str,
    expand: fn(&str) -> Vec<String>,
) {
    let extraction = extract_key_literal_values(source, key);
    push_failures(hints, script, key, extraction.failures);
    collect_unique(
        &mut hints.include_dirs,
        extraction.values.iter().flat_map(|value| expand(value)),
    );
}

fn merge_typed_key(
    hints: &mut NativeBuildHints,
    script: NativeBuildScript,
    source: &str,
    key: &'static str,
) {
    let extraction = extract_key_literal_values(source, key);
    push_failures(hints, script, key, extraction.failures);

    if key == "LIBS" {
        for value in &extraction.values {
            let tokens = tokenize_flags(value)
                .into_iter()
                .filter(|token| is_library_link_input(token))
                .collect::<Vec<_>>();
            if !tokens.is_empty() {
                hints.libs_alternatives.push(tokens.clone());
                collect_unique(&mut hints.libs_flags, tokens.into_iter());
            }
        }
    } else {
        let target = match key {
            "DEFINE" => &mut hints.define_flags,
            "OBJECT" => &mut hints.object_files,
            _ => &mut hints.myextlib_files,
        };
        collect_unique(target, extraction.values.iter().flat_map(|value| hint_tokens(key, value)));
    }
}

fn push_failures(
    hints: &mut NativeBuildHints,
    script: NativeBuildScript,
    key: &'static str,
    reasons: Vec<NativeBuildHintParseReason>,
) {
    for reason in reasons {
        hints.diagnostics.push(NativeBuildHintDiagnostic { script, key, reason });
    }
}

/// Split one parsed literal value into the token kind promised by `key`.
///
/// Off-type tokens are dropped silently: they are well-formed literals the
/// static parser cannot resolve to the field's concrete input kind.
fn hint_tokens(key: &str, value: &str) -> Vec<String> {
    match key {
        "LIBS" => {
            tokenize_flags(value).into_iter().filter(|token| is_library_link_input(token)).collect()
        }
        "DEFINE" => value
            .split_whitespace()
            .filter(|token| is_definition_flag(token))
            .map(str::to_owned)
            .collect(),
        "OBJECT" => value
            .split_whitespace()
            .filter(|token| is_object_file_token(token))
            .map(str::to_owned)
            .collect(),
        // MYEXTLIB references archives verbatim, so each literal value is one
        // candidate archive path and is never whitespace-split.
        _ => {
            let archive = value.trim();
            if is_static_archive_reference(archive) { vec![archive.to_owned()] } else { Vec::new() }
        }
    }
}

fn tokenize_flags(value: &str) -> Vec<String> {
    // Quote characters group tokens across spaces; every other character,
    // including backslashes, is preserved verbatim so Windows separators such
    // as `C:\vendor\foo.lib` survive unchanged.
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in value.chars() {
        if let Some(active) = quote {
            if ch == active {
                quote = None;
            } else {
                current.push(ch);
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn flatten_include_flags(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter_map(|token| token.strip_prefix("-I"))
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect()
}

fn keep_value_verbatim(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { Vec::new() } else { vec![trimmed.to_owned()] }
}

/// Unix linker library/search-path flags plus MSVC import-library names.
fn is_library_link_input(token: &str) -> bool {
    ((token.starts_with("-l") || token.starts_with("-L")) && token.len() > 2)
        || token.ends_with(".lib")
}

fn is_definition_flag(token: &str) -> bool {
    token.starts_with("-D") && token.len() > 2
}

fn is_object_file_token(token: &str) -> bool {
    token.ends_with(".o") || token.ends_with(".obj")
}

fn is_static_archive_reference(value: &str) -> bool {
    value.ends_with(".a") || value.ends_with(".lib")
}

fn collect_unique<I>(into: &mut Vec<String>, values: I)
where
    I: Iterator<Item = String>,
{
    for value in values {
        if !value.is_empty() && !into.contains(&value) {
            into.push(value);
        }
    }
}

/// Extraction result for one literal assignment key: accepted values plus one
/// named reason per occurrence that failed to parse.
struct KeyLiteralExtraction {
    values: Vec<String>,
    failures: Vec<NativeBuildHintParseReason>,
}

fn extract_key_literal_values(source: &str, key: &str) -> KeyLiteralExtraction {
    let mut values = Vec::new();
    let mut failures = Vec::new();
    let bytes = source.as_bytes();
    let mut search_from = 0;

    while let Some((_, value_start)) = find_key_assignment(bytes, key, search_from) {
        match parse_literal_value(source, value_start) {
            Ok((mut parsed, consumed)) => {
                values.append(&mut parsed);
                search_from = value_start + consumed;
            }
            Err(reason) => {
                failures.push(reason);
                search_from = value_start + rejected_value_len(source, value_start).max(1);
            }
        }
    }

    KeyLiteralExtraction { values, failures }
}

fn rejected_value_len(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let mut idx = start;
    let mut stack = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    while idx < bytes.len() {
        let ch = bytes[idx] as char;
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            idx += 1;
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' | '[' | '{' => stack.push(ch),
            ')' | ']' | '}' => {
                // Bind the pop before testing it. A mutating call reads as a
                // pure condition inside a match guard, so #12910 asks for the
                // `let` at every side-effecting site: the pop must happen
                // exactly once per closer whether or not the walk ends here.
                let closed_an_open_group = stack.pop().is_some();
                if !closed_an_open_group {
                    return idx.saturating_sub(start);
                }
            }
            ',' | ';' if stack.is_empty() => return idx.saturating_sub(start),
            _ => {}
        }
        idx += 1;
    }
    idx.saturating_sub(start)
}

fn find_key_assignment(bytes: &[u8], key: &str, start: usize) -> Option<(usize, usize)> {
    let key_bytes = key.as_bytes();
    let mut idx = start;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_comment = false;

    while idx < bytes.len() {
        let byte = bytes[idx];

        if in_comment {
            idx += 1;
            if byte == b'\n' {
                in_comment = false;
            }
            continue;
        }

        if in_single_quote {
            idx += 1;
            if byte == b'\\' && idx < bytes.len() {
                idx += 1;
                continue;
            }
            if byte == b'\'' {
                in_single_quote = false;
            }
            continue;
        }

        if in_double_quote {
            idx += 1;
            if byte == b'\\' && idx < bytes.len() {
                idx += 1;
                continue;
            }
            if byte == b'"' {
                in_double_quote = false;
            }
            continue;
        }

        match byte {
            b'#' => {
                in_comment = true;
                idx += 1;
                continue;
            }
            b'\'' => {
                in_single_quote = true;
                idx += 1;
                continue;
            }
            b'"' => {
                in_double_quote = true;
                idx += 1;
                continue;
            }
            _ => {}
        }

        // Skip the complete body of Perl quote-like operators (`q`, `qq`,
        // `qw`, `qr`, `m`, `s///`, `tr///`, `y///`): key-shaped text inside
        // those string bodies is literal content, never an assignment.
        if let Some(after) = quote_like_op_span(bytes, idx) {
            idx = after;
            continue;
        }

        if !bytes[idx..].starts_with(key_bytes) {
            idx += 1;
            continue;
        }

        let key_pos = idx;
        if !is_key_boundary(bytes, key_pos, key_bytes.len()) {
            idx = key_pos + key_bytes.len();
            continue;
        }

        let mut value_idx = key_pos + key_bytes.len();
        skip_ws_and_comments(bytes, &mut value_idx);
        if value_idx + 1 >= bytes.len()
            || bytes.get(value_idx) != Some(&b'=')
            || bytes.get(value_idx + 1) != Some(&b'>')
        {
            idx = value_idx.saturating_add(1);
            continue;
        }

        value_idx += 2;
        skip_ws_and_comments(bytes, &mut value_idx);
        return Some((key_pos, value_idx));
    }

    None
}

/// If `bytes[idx..]` starts a Perl quote-like operator invocation, return the
/// index just past its full expression so the scanner can jump over literal
/// string bodies that may contain key-shaped text.
///
/// Recognizes single-pattern operators (`q`, `qq`, `qw`, `qr`, `m`) and
/// double-pattern operators (`s`, `tr`, `y`), each followed by a non-word
/// delimiter — paired (`()[]{}<>`) or repeated. Returns `None` on any shape it
/// does not confidently recognize; scanning then continues normally, which is
/// the conservative failure direction.
fn quote_like_op_span(bytes: &[u8], idx: usize) -> Option<usize> {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    // Word-boundary guard: never fire in the middle of an identifier.
    if idx > 0 && bytes.get(idx - 1).is_some_and(|b| is_ident(*b)) {
        return None;
    }

    const TWO_PATTERN_WORDS: [&[u8]; 3] = [b"s", b"tr", b"y"];
    const ONE_PATTERN_WORDS: [&[u8]; 5] = [b"qw", b"qq", b"qr", b"q", b"m"];

    let mut after_word = None;
    for word in ONE_PATTERN_WORDS.iter().chain(TWO_PATTERN_WORDS.iter()) {
        if bytes[idx..].starts_with(word)
            && bytes.get(idx + word.len()).is_none_or(|b| !is_ident(*b))
        {
            after_word = Some((word.len(), TWO_PATTERN_WORDS.contains(word)));
            break;
        }
    }
    let (word_len, two_patterns) = after_word?;

    let mut cursor = idx + word_len;
    skip_ws_and_comments(bytes, &mut cursor);
    let open = *bytes.get(cursor)?;
    if is_ident(open) || matches!(open, b'=' | b'>' | b':' | b'-') {
        return None;
    }
    let close = match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        other => other,
    };

    let mut end = quote_like_body_end(bytes, cursor, open, close)?;

    if two_patterns {
        let mut second = end;
        skip_ws_and_comments(bytes, &mut second);
        let open2 = *bytes.get(second)?;
        if is_ident(open2) || matches!(open2, b'=' | b'>' | b':' | b'-') {
            return None;
        }
        let close2 = match open2 {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b'<' => b'>',
            other => other,
        };
        end = quote_like_body_end(bytes, second, open2, close2)?;
    }

    Some(end)
}

/// Scan one delimited pattern body, honoring backslash escapes and nesting
/// for paired delimiters. Returns the index just past the closing delimiter.
fn quote_like_body_end(
    bytes: &[u8],
    open_at: usize,
    open_delim: u8,
    close_delim: u8,
) -> Option<usize> {
    // Start past the opening delimiter so it is never counted toward nesting.
    let mut idx = open_at.saturating_add(1);
    let paired = open_delim != close_delim;
    let mut depth = 0usize;

    while idx < bytes.len() {
        let byte = bytes[idx];
        if byte == b'\\' {
            idx += 2;
            continue;
        }
        if paired && byte == open_delim {
            depth += 1;
        } else if byte == close_delim {
            if depth == 0 {
                return Some(idx + 1);
            }
            depth = depth.saturating_sub(1);
        }
        idx += 1;
    }

    None
}

fn is_key_boundary(bytes: &[u8], key_pos: usize, key_len: usize) -> bool {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    let before_ok =
        key_pos.checked_sub(1).and_then(|idx| bytes.get(idx)).is_none_or(|b| !is_ident(*b));
    let after_ok = bytes.get(key_pos + key_len).is_none_or(|b| !is_ident(*b));

    before_ok && after_ok
}

fn skip_ws_and_comments(bytes: &[u8], idx: &mut usize) {
    loop {
        while let Some(b) = bytes.get(*idx) {
            match b {
                b' ' | b'\t' | b'\r' | b'\n' => *idx += 1,
                _ => break,
            }
        }

        if bytes.get(*idx) == Some(&b'#') {
            while *idx < bytes.len() && bytes[*idx] != b'\n' {
                *idx += 1;
            }
            continue;
        }

        break;
    }
}

fn parse_literal_value(
    source: &str,
    start: usize,
) -> Result<(Vec<String>, usize), NativeBuildHintParseReason> {
    let bytes = source.as_bytes();
    match bytes.get(start) {
        Some(b'\'' | b'"') => {
            let (value, consumed, interpolating) = parse_quoted_string(source, start)
                .ok_or(NativeBuildHintParseReason::UnterminatedStringLiteral)?;
            // An interpolated string is not a static literal: its build-time
            // value cannot be resolved without executing Perl, so it fails
            // closed instead of emitting unverifiable flags.
            if interpolating {
                return Err(NativeBuildHintParseReason::UnsupportedValueForm);
            }
            ensure_value_is_whole(source, start + consumed)
                .ok_or(NativeBuildHintParseReason::UnsupportedValueForm)?;
            Ok((vec![value], consumed))
        }
        Some(b'[') => parse_quoted_string_array(source, start),
        _ => Err(NativeBuildHintParseReason::UnsupportedValueForm),
    }
}

/// Verify the byte just past a fully consumed scalar value closes the
/// assignment (`,`, `;`, a bracket closer, end of line/file, or a trailing
/// comment). Anything else — concatenation operators, arithmetic, further
/// terms — means the RHS was only a partial expression and must be rejected.
fn ensure_value_is_whole(source: &str, after_end: usize) -> Option<()> {
    let bytes = source.as_bytes();
    let mut idx = after_end;
    while let Some(&b) = bytes.get(idx) {
        match b {
            b' ' | b'\t' | b'\r' => idx += 1,
            b'#' => {
                while idx < bytes.len() && bytes[idx] != b'\n' {
                    idx += 1;
                }
            }
            _ => break,
        }
    }
    match bytes.get(idx) {
        None | Some(b'\n') => Some(()),
        Some(b',' | b';' | b')' | b']' | b'}') => Some(()),
        _ => None,
    }
}

/// Returns `(value, consumed, is_interpolating)`. Double-quoted strings that
/// reference `$` or `@` interpolate at Perl runtime; single-quoted strings are
/// always verbatim.
fn parse_quoted_string(source: &str, start: usize) -> Option<(String, usize, bool)> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(start)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }

    let mut value = String::new();
    let mut idx = start + 1;
    let mut escaped = false;
    let mut interpolating = false;

    while idx < bytes.len() {
        let ch = source[idx..].chars().next()?;
        let ch_len = ch.len_utf8();
        idx += ch_len;

        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' && quote == b'"' {
            escaped = true;
            continue;
        }

        if ch == '\\' && quote == b'\'' {
            let next = bytes.get(idx).copied();
            if next == Some(b'\\') || next == Some(b'\'') {
                escaped = true;
                continue;
            }
        }

        if ch as u8 == quote {
            return Some((value, idx - start, interpolating));
        }

        if ch == '\n' {
            return None;
        }

        if quote == b'"' && (ch == '$' || ch == '@') {
            interpolating = true;
        }

        value.push(ch);
    }

    None
}

fn parse_quoted_string_array(
    source: &str,
    start: usize,
) -> Result<(Vec<String>, usize), NativeBuildHintParseReason> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'[') {
        return Err(NativeBuildHintParseReason::MalformedArrayLiteral);
    }

    let mut idx = start + 1;
    let mut values = Vec::new();

    loop {
        skip_ws_and_comments(bytes, &mut idx);
        match bytes.get(idx) {
            None => return Err(NativeBuildHintParseReason::MalformedArrayLiteral),
            Some(b']') => return Ok((values, idx + 1 - start)),
            Some(b'\'' | b'"') => {
                let (value, consumed, interpolating) = parse_quoted_string(source, idx)
                    .ok_or(NativeBuildHintParseReason::MalformedArrayLiteral)?;
                if interpolating {
                    return Err(NativeBuildHintParseReason::UnsupportedValueForm);
                }
                values.push(value);
                idx += consumed;
                skip_ws_and_comments(bytes, &mut idx);
                match bytes.get(idx) {
                    Some(b',') => {
                        idx += 1;
                    }
                    Some(b']') => {}
                    _ => return Err(NativeBuildHintParseReason::MalformedArrayLiteral),
                }
            }
            _ => return Err(NativeBuildHintParseReason::MalformedArrayLiteral),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    fn ensure_eq<T>(actual: T, expected: T, context: &str) -> TestResult
    where
        T: std::fmt::Debug + PartialEq,
    {
        if actual == expected {
            Ok(())
        } else {
            Err(format!("{context}: expected {expected:?}, got {actual:?}").into())
        }
    }

    /// Fresh temporary workspace root accepting optional build scripts.
    struct HintRoot {
        tempdir: tempfile::TempDir,
    }

    impl HintRoot {
        fn new() -> TestResult<Self> {
            Ok(Self { tempdir: tempfile::tempdir()? })
        }

        fn path(&self) -> &Path {
            self.tempdir.path()
        }

        fn write_makefile(&self, source: &str) -> TestResult<()> {
            std::fs::write(self.path().join("Makefile.PL"), source)?;
            Ok(())
        }

        fn write_build_pl(&self, source: &str) -> TestResult<()> {
            std::fs::write(self.path().join("Build.PL"), source)?;
            Ok(())
        }

        fn hints(&self) -> NativeBuildHints {
            detect_native_build_hints(self.path())
        }
    }

    fn diagnostic(
        script: NativeBuildScript,
        key: &'static str,
        reason: NativeBuildHintParseReason,
    ) -> NativeBuildHintDiagnostic {
        NativeBuildHintDiagnostic { script, key, reason }
    }

    #[test]
    fn libs_key_yields_typed_library_link_flags() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(
            r#"
WriteMakefile(
    NAME => 'Sample',
    LIBS => ['-L/opt/native/lib', '-lcrypto'],
);
"#,
        )?;

        let hints = root.hints();

        assert_eq!(hints.libs_flags, vec!["-L/opt/native/lib".to_string(), "-lcrypto".to_string()]);
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn libs_accepts_flag_strings_and_msvc_import_libraries_across_scripts() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(r#"WriteMakefile(LIBS => '-lm -lnsl');"#)?;
        root.write_build_pl(r#"never_called_marker(LIBS => ['ws2_32.lib user32.lib']);"#)?;

        let hints = root.hints();

        assert_eq!(
            hints.libs_flags,
            vec![
                "-lm".to_string(),
                "-lnsl".to_string(),
                "ws2_32.lib".to_string(),
                "user32.lib".to_string(),
            ]
        );
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn define_key_yields_typed_definition_flags_and_skips_comments() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(
            "# DEFINE => '-DSKIPPED_BY_COMMENT';\nWriteMakefile(\n    DEFINE => '-DUSE_THREADS -DHAVE_EPOLL',\n);\n",
        )?;

        let hints = root.hints();

        assert_eq!(
            hints.define_flags,
            vec!["-DUSE_THREADS".to_string(), "-DHAVE_EPOLL".to_string()]
        );
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn object_key_yields_object_file_tokens_across_forms() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(
            r#"
WriteMakefile(
    OBJECT => 'init.o core.obj',
    dynamic_lib => { OTHERLDFLAGS => '' },
);
"#,
        )?;

        let hints = root.hints();

        assert_eq!(hints.object_files, vec!["init.o".to_string(), "core.obj".to_string()]);
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn myextlib_key_yields_archive_elements_and_deduplicates_across_scripts() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile("WriteMakefile(MYEXTLIB => 'mylib/libmylib.a');")?;
        root.write_build_pl(
            "marker_lib_call(MYEXTLIB => ['mylib/libmylib.a', 'vendor/libvendor.lib']);",
        )?;

        let hints = root.hints();

        assert_eq!(
            hints.myextlib_files,
            vec!["mylib/libmylib.a".to_string(), "vendor/libvendor.lib".to_string()]
        );
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn conservative_filtering_drops_off_type_tokens_silently() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(
            r#"
WriteMakefile(
    LIBS => '$(EXPANDED_AT_BUILD_TIME) plainword',
    DEFINE => '-UBADFLAG',
    OBJECT => 'README pod/dir.pod',
    MYEXTLIB => '$(MYEXTLIB_VAR)',
);
"#,
        )?;

        let hints = root.hints();

        assert!(hints.libs_flags.is_empty());
        assert!(hints.define_flags.is_empty());
        assert!(hints.object_files.is_empty());
        assert!(hints.myextlib_files.is_empty());
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn malformed_makefile_values_fail_closed_with_named_diagnostics_per_key() -> TestResult {
        let cases: Vec<(&'static str, &str, NativeBuildHintParseReason)> = vec![
            (
                "LIBS",
                "WriteMakefile(LIBS => not_a_literal);",
                NativeBuildHintParseReason::UnsupportedValueForm,
            ),
            (
                "LIBS",
                "WriteMakefile(LIBS => ['unterminated);",
                NativeBuildHintParseReason::MalformedArrayLiteral,
            ),
            (
                "DEFINE",
                "WriteMakefile(DEFINE => q(-DQUOTED_FORM_IS_UNSUPPORTED));",
                NativeBuildHintParseReason::UnsupportedValueForm,
            ),
            (
                "DEFINE",
                "WriteMakefile(DEFINE => 'unterminated_multiline,\n);",
                NativeBuildHintParseReason::UnterminatedStringLiteral,
            ),
            (
                "OBJECT",
                "WriteMakefile(OBJECT => ['first.o', sub { 'not_static' }]);",
                NativeBuildHintParseReason::MalformedArrayLiteral,
            ),
            (
                "OBJECT",
                "WriteMakefile(OBJECT => 'missing_close.o",
                NativeBuildHintParseReason::UnterminatedStringLiteral,
            ),
            (
                "MYEXTLIB",
                "WriteMakefile(MYEXTLIB => ('archives', 'unsupported'));",
                NativeBuildHintParseReason::UnsupportedValueForm,
            ),
            (
                "MYEXTLIB",
                "WriteMakefile(MYEXTLIB => [\"unterminated.a]);",
                NativeBuildHintParseReason::MalformedArrayLiteral,
            ),
        ];

        for (key, makefile_source, expected_reason) in cases {
            let root = HintRoot::new()?;
            root.write_makefile(makefile_source)?;

            let hints = root.hints();

            let expected = vec![diagnostic(NativeBuildScript::MakefilePl, key, expected_reason)];
            assert_eq!(hints.diagnostics, expected, "diagnostics mismatch for {key}");
            assert!(hints.libs_flags.is_empty(), "{key} leaked libs");
            assert!(hints.define_flags.is_empty(), "{key} leaked defines");
            assert!(hints.object_files.is_empty(), "{key} leaked objects");
            assert!(hints.myextlib_files.is_empty(), "{key} leaked myextlib");
            assert!(hints.include_dirs.is_empty(), "{key} leaked include dirs");
        }

        Ok(())
    }

    #[test]
    fn malformed_build_pl_value_names_build_script_in_diagnostic() -> TestResult {
        let root = HintRoot::new()?;
        root.write_build_pl("marker_call(MYEXTLIB => foo);")?;

        let hints = root.hints();

        assert_eq!(
            hints.diagnostics,
            vec![diagnostic(
                NativeBuildScript::BuildPl,
                "MYEXTLIB",
                NativeBuildHintParseReason::UnsupportedValueForm,
            )]
        );
        Ok(())
    }

    #[test]
    fn recovery_after_malformed_occurrence_keeps_later_literal() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(
            "WriteMakefile(\n    OBJECT => broken_placeholder,\n    OBJECT => 'later.o',\n);",
        )?;

        let hints = root.hints();

        assert_eq!(hints.object_files, vec!["later.o".to_string()]);
        assert_eq!(
            hints.diagnostics,
            vec![diagnostic(
                NativeBuildScript::MakefilePl,
                "OBJECT",
                NativeBuildHintParseReason::UnsupportedValueForm,
            )]
        );
        Ok(())
    }

    #[test]
    fn include_dir_hints_remain_supported_next_to_the_new_keys() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(
            r#"
WriteMakefile(
    INC => '-I/vendor/include -Ithirdparty/include',
    LIBS => ['-lz'],
);
"#,
        )?;
        root.write_build_pl(
            r#"
Module::Build->new(
    include_dirs => ['gen/include'],
    extra_compiler_flags => ['-Iextra/include'],
);
"#,
        )?;

        let hints = root.hints();

        assert_eq!(
            hints.include_dirs,
            vec![
                "/vendor/include".to_string(),
                "thirdparty/include".to_string(),
                "gen/include".to_string(),
                "extra/include".to_string(),
            ]
        );
        assert_eq!(hints.libs_flags, vec!["-lz".to_string()]);
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn absent_build_scripts_yield_default_hints() -> TestResult {
        let root = HintRoot::new()?;

        let hints = root.hints();

        assert_eq!(hints, NativeBuildHints::default());
        Ok(())
    }

    #[test]
    fn diagnostic_script_reports_workspace_file_names() {
        assert_eq!(NativeBuildScript::MakefilePl.file_name(), "Makefile.PL");
        assert_eq!(NativeBuildScript::BuildPl.file_name(), "Build.PL");
    }

    #[test]
    fn rejected_nested_assignment_is_not_rescanned() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile("WriteMakefile(LIBS => q(LIBS => '-levil'));\n")?;

        let hints = root.hints();

        assert!(hints.libs_flags.is_empty());
        assert_eq!(hints.diagnostics.len(), 1);
        Ok(())
    }

    #[test]
    fn single_quoted_windows_paths_preserve_backslashes() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile("WriteMakefile(MYEXTLIB => 'C:\\\\vendor\\\\foo.lib');")?;

        let hints = root.hints();

        assert_eq!(hints.myextlib_files, vec![r"C:\vendor\foo.lib".to_string()]);
        Ok(())
    }

    #[test]
    fn quoted_library_search_paths_remain_one_flag() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(r#"WriteMakefile(LIBS => '-L"/opt/vendor SDK/lib" -lfoo');"#)?;

        let hints = root.hints();

        assert_eq!(
            hints.libs_flags,
            vec!["-L/opt/vendor SDK/lib".to_string(), "-lfoo".to_string()]
        );
        Ok(())
    }

    #[test]
    fn libs_array_retains_ordered_alternatives() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile("WriteMakefile(LIBS => ['-lgdbm', '-ldbm -lfoo']);")?;

        let hints = root.hints();

        assert_eq!(
            hints.libs_alternatives,
            vec![vec!["-lgdbm".to_string()], vec!["-ldbm".to_string(), "-lfoo".to_string()]]
        );
        Ok(())
    }

    #[test]
    fn windows_backslashes_survive_inside_quoted_segments() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(r#"WriteMakefile(LIBS => '-L"C:\Program Files\SDK\lib" -lfoo');"#)?;

        let hints = root.hints();

        assert_eq!(
            hints.libs_flags,
            vec![r"-LC:\Program Files\SDK\lib".to_string(), "-lfoo".to_string()]
        );
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    #[test]
    fn interpolated_double_quoted_rhs_fails_closed() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile("WriteMakefile(LIBS => \"-l$libname\");")?;

        let hints = root.hints();

        assert!(hints.libs_flags.is_empty());
        assert_eq!(
            hints.diagnostics,
            vec![diagnostic(
                NativeBuildScript::MakefilePl,
                "LIBS",
                NativeBuildHintParseReason::UnsupportedValueForm,
            )]
        );
        Ok(())
    }

    #[test]
    fn concatenated_rhs_fails_closed_without_partial_capture() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile("WriteMakefile(LIBS => '-lfoo' . '-lbar');")?;

        let hints = root.hints();

        assert!(hints.libs_flags.is_empty());
        assert_eq!(hints.diagnostics.len(), 1);
        assert_eq!(hints.diagnostics[0].reason, NativeBuildHintParseReason::UnsupportedValueForm);
        // The rejected span must not be rescanned into a second occurrence.
        assert!(
            !hints
                .libs_alternatives
                .iter()
                .any(|candidate| candidate == &vec!["-lbar".to_string()])
        );
        Ok(())
    }

    #[test]
    fn quote_like_string_content_is_not_an_assignment() -> TestResult {
        let root = HintRoot::new()?;
        root.write_makefile(
            "my $doc = q(LIBS => '-levil');\n\
             WriteMakefile(LIBS => ['-lreal']);\n",
        )?;

        let hints = root.hints();

        // The real assignment survives; the q()-embedded text yields nothing.
        assert_eq!(hints.libs_flags, vec!["-lreal".to_string()]);
        assert_eq!(hints.libs_alternatives, vec![vec!["-lreal".to_string()]]);
        assert!(hints.diagnostics.is_empty());
        Ok(())
    }

    // ---------------------------------------------------------------------
    // `rejected_value_len` delimiter walk (#12910)
    //
    // These pin every exit of the scanner that advances `search_from` past a
    // value the literal parser rejected. They are characterization tests:
    // written against the pre-collapse nested `if`, so the guarded-arm
    // rewrite is proven behavior-preserving rather than assumed. The
    // `stack.pop()` inside the closer arm is a side effect, which is exactly
    // what makes the collapse worth pinning — a guard that tested
    // `stack.is_empty()` without popping would leave the stack undrained.
    // ---------------------------------------------------------------------

    /// An unbalanced closer at depth zero ends the value at its own offset.
    #[test]
    fn rejected_value_len_stops_at_an_unmatched_closer() -> TestResult {
        ensure_eq(rejected_value_len(")rest", 0), 0, "parenthesis closer offset")?;
        ensure_eq(rejected_value_len("ab]rest", 0), 2, "bracket closer offset")?;
        ensure_eq(rejected_value_len("ab}rest", 0), 2, "brace closer offset")?;
        Ok(())
    }

    /// A balanced group is consumed whole; the walk ends at the first
    /// top-level separator AFTER it. This is the discriminator for the pop
    /// side effect: without the pop the stack never drains, the trailing
    /// separator is never seen at depth zero, and the walk runs to the end.
    #[test]
    fn rejected_value_len_drains_balanced_groups_before_the_separator() -> TestResult {
        ensure_eq(rejected_value_len("(a), b", 0), 3, "parenthesis group length")?;
        ensure_eq(rejected_value_len("[a]; b", 0), 3, "bracket group length")?;
        ensure_eq(rejected_value_len("{[()]}, b", 0), 6, "nested group length")?;
        Ok(())
    }

    /// Separators nested inside a group do not end the value.
    #[test]
    fn rejected_value_len_ignores_separators_inside_groups() -> TestResult {
        ensure_eq(rejected_value_len("(a, b); tail", 0), 6, "nested comma length")?;
        ensure_eq(rejected_value_len("[a; b], tail", 0), 6, "nested semicolon length")?;
        Ok(())
    }

    /// A closer that merely returns the walk to depth zero does not end it —
    /// only a closer with nothing left on the stack does.
    #[test]
    fn rejected_value_len_ends_only_on_the_extra_closer() -> TestResult {
        // Two openers, three closers: the walk survives the first two and
        // stops exactly at the third.
        ensure_eq(rejected_value_len("([a])) tail", 0), 5, "extra closer offset")?;
        Ok(())
    }

    /// Delimiters and separators inside quotes are inert, and a backslash
    /// escapes the closing quote.
    #[test]
    fn rejected_value_len_treats_quoted_delimiters_as_inert() -> TestResult {
        ensure_eq(rejected_value_len("')', x", 0), 3, "quoted closer length")?;
        ensure_eq(rejected_value_len("\"a,b\", tail", 0), 5, "quoted comma length")?;
        ensure_eq(rejected_value_len("'a\\'b)', tail", 0), 7, "escaped quote length")?;
        Ok(())
    }

    /// With no closer and no separator the walk consumes the remainder.
    #[test]
    fn rejected_value_len_consumes_the_remainder_when_unterminated() -> TestResult {
        ensure_eq(rejected_value_len("abc", 0), 3, "plain remainder length")?;
        ensure_eq(rejected_value_len("(a, b", 0), 5, "unclosed group length")?;
        ensure_eq(rejected_value_len("'unclosed", 0), 9, "unclosed quote length")?;
        Ok(())
    }

    /// The returned length is relative to `start`, not to the buffer.
    #[test]
    fn rejected_value_len_is_relative_to_the_start_offset() -> TestResult {
        //            0123456789
        let source = "KEY => a, b";
        ensure_eq(rejected_value_len(source, 7), 1, "relative value length")?;
        ensure_eq(rejected_value_len(source, 0), 8, "full-prefix value length")?;
        Ok(())
    }
}
