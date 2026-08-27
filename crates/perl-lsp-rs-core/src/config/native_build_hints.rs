//! Conservative native build hint extraction.
//!
//! This module looks for literal `Makefile.PL` / `Build.PL` hints at the
//! workspace root and extracts the ExtUtils::MakeMaker-style keys relevant to
//! native builds: `INC`, `LIBS`, `DEFINE`, `OBJECT`, and `MYEXTLIB`, plus the
//! Module::Build equivalents `include_dirs` and `extra_compiler_flags`. It does
//! not execute Perl and does not model full build metadata.
//!
//! Only literal quoted strings and literal arrays of quoted strings are
//! understood. Anything else — barewords, `q()` forms, function calls,
//! unterminated strings, arrays containing code — fails closed: the offending
//! occurrence contributes no hint values and is reported through a named
//! [`NativeBuildHintDiagnostic`] so callers can see why hints stayed empty.

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
        "LIBS" => tokenize_flags(value)
            .into_iter()
            .filter(|token| is_library_link_input(token))
            .collect(),
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
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' && quote.is_some() {
            escaped = true;
        } else if let Some(active) = quote {
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
    if escaped {
        current.push('\\');
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
                if stack.pop().is_none() {
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
            let (value, consumed) = parse_quoted_string(source, start)
                .ok_or(NativeBuildHintParseReason::UnterminatedStringLiteral)?;
            Ok((vec![value], consumed))
        }
        Some(b'[') => parse_quoted_string_array(source, start)
            .ok_or(NativeBuildHintParseReason::MalformedArrayLiteral),
        _ => Err(NativeBuildHintParseReason::UnsupportedValueForm),
    }
}

fn parse_quoted_string(source: &str, start: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(start)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }

    let mut value = String::new();
    let mut idx = start + 1;
    let mut escaped = false;

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
            return Some((value, idx - start));
        }

        if ch == '\n' {
            return None;
        }

        value.push(ch);
    }

    None
}

fn parse_quoted_string_array(source: &str, start: usize) -> Option<(Vec<String>, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'[') {
        return None;
    }

    let mut idx = start + 1;
    let mut values = Vec::new();

    loop {
        skip_ws_and_comments(bytes, &mut idx);
        match bytes.get(idx).copied()? {
            b']' => return Some((values, idx + 1 - start)),
            b'\'' | b'"' => {
                let (value, consumed) = parse_quoted_string(source, idx)?;
                values.push(value);
                idx += consumed;
                skip_ws_and_comments(bytes, &mut idx);
                match bytes.get(idx).copied()? {
                    b',' => {
                        idx += 1;
                    }
                    b']' => {}
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

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
}
