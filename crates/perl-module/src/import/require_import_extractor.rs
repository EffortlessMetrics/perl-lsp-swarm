/// A single symbol extracted from a literal `require Module; Module->import(...)` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequireImportEntry {
    /// The fully qualified module name (e.g. `Foo::Bar`).
    pub module: String,
    /// The symbol name imported from the module.
    pub symbol: String,
    /// Byte offset of the `require` statement start in the source string.
    pub require_byte_offset: usize,
    /// Byte offset of the `Module->import(...)` statement start in the source string.
    pub import_byte_offset: usize,
}

#[must_use]
pub fn extract_require_import_symbols(source: &str) -> Vec<RequireImportEntry> {
    let mut entries = Vec::new();
    let lines: Vec<(usize, &str)> = {
        let mut v = Vec::new();
        let mut offset = 0usize;
        for line in source.split('\n') {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let leading = line.len().saturating_sub(line.trim_start().len());
                v.push((offset + leading, trimmed));
            }
            offset += line.len() + 1;
        }
        v
    };

    for (i, &(req_offset, req_line)) in lines.iter().enumerate() {
        let parsed_require = match parse_literal_require_line(req_line) {
            Some(parsed_require) => parsed_require,
            None => continue,
        };
        let module = parsed_require.module;

        if collect_literal_import_entries(
            &mut entries,
            module,
            req_offset,
            req_offset + parsed_require.tail_start,
            parsed_require.tail,
        ) {
            continue;
        }

        let window_end = (i + 1 + 5).min(lines.len());
        for &(imp_offset, imp_line) in &lines[i + 1..window_end] {
            if collect_literal_import_entries(
                &mut entries,
                module,
                req_offset,
                imp_offset,
                imp_line,
            ) {
                break;
            }
            if is_statement_terminator(imp_line) {
                break;
            }
        }
    }

    entries
}

struct ParsedLiteralRequire<'a> {
    module: &'a str,
    tail_start: usize,
    tail: &'a str,
}

fn parse_literal_require_line(line: &str) -> Option<ParsedLiteralRequire<'_>> {
    let rest = line.strip_prefix("require")?;
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let leading_after_keyword = rest.len().saturating_sub(rest.trim_start().len());
    let rest = rest.trim_start();
    if rest.starts_with('$') || rest.starts_with('"') || rest.starts_with('\'') {
        return None;
    }

    let module_end = rest.find(|c: char| c == ';' || c.is_whitespace()).unwrap_or(rest.len());
    let module = &rest[..module_end];
    if !is_valid_bareword_module_name(module) {
        return None;
    }

    let after_module = &rest[module_end..];
    let semicolon_offset = after_module.find(';')?;
    let tail_start = "require".len() + leading_after_keyword + module_end + semicolon_offset + 1;
    Some(ParsedLiteralRequire { module, tail_start, tail: &line[tail_start..] })
}

fn is_valid_bareword_module_name(module: &str) -> bool {
    if module.is_empty() {
        return false;
    }

    module.split("::").all(|part| {
        !part.is_empty()
            && part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn collect_literal_import_entries(
    entries: &mut Vec<RequireImportEntry>,
    module: &str,
    require_byte_offset: usize,
    import_byte_offset: usize,
    candidate: &str,
) -> bool {
    let leading = candidate.len().saturating_sub(candidate.trim_start().len());
    let candidate = candidate.trim_start();

    if let Some(symbols) = parse_literal_import_call(candidate, module) {
        for symbol in symbols {
            entries.push(RequireImportEntry {
                module: module.to_string(),
                symbol,
                require_byte_offset,
                import_byte_offset: import_byte_offset + leading,
            });
        }
        return true;
    }

    false
}

fn parse_literal_import_call(line: &str, expected_module: &str) -> Option<Vec<String>> {
    let after_module = line.strip_prefix(expected_module)?.trim_start();
    let after_arrow = after_module.strip_prefix("->")?.trim_start();
    let after_method = after_arrow.strip_prefix("import")?.trim_start();
    let after_open = after_method.strip_prefix('(')?;
    let close_idx = after_open.rfind(')')?;
    let args_src = &after_open[..close_idx];

    if args_src.contains('@') || args_src.contains('$') {
        return None;
    }

    let symbols = parse_literal_arg_list(args_src)?;
    Some(symbols)
}

fn parse_literal_arg_list(args: &str) -> Option<Vec<String>> {
    let trimmed = args.trim();

    if trimmed.is_empty() {
        return Some(Vec::new());
    }

    if let Some(words) = parse_qw_arg_list(trimmed) {
        return Some(words);
    }

    let mut symbols = Vec::new();
    for part in trimmed.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let Some(inner) = p.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            if inner.is_empty() {
                continue;
            }
            symbols.push(inner.to_string());
            continue;
        }
        if let Some(inner) = p.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            if inner.is_empty() {
                continue;
            }
            symbols.push(inner.to_string());
            continue;
        }
        return None;
    }

    Some(symbols)
}

fn parse_qw_arg_list(trimmed: &str) -> Option<Vec<String>> {
    let after_operator = trimmed.strip_prefix("qw")?;
    let delimiter = after_operator.chars().next()?;
    if delimiter.is_ascii_alphanumeric() || delimiter == '_' || delimiter.is_whitespace() {
        return None;
    }

    let closing = match delimiter {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '<' => '>',
        other => other,
    };

    let inner_start = "qw".len() + delimiter.len_utf8();
    let inner_end = trimmed.len().checked_sub(closing.len_utf8())?;
    if inner_start > inner_end || !trimmed.ends_with(closing) {
        return None;
    }

    let inner = &trimmed[inner_start..inner_end];
    Some(inner.split_whitespace().filter(|word| !word.is_empty()).map(str::to_string).collect())
}

fn is_statement_terminator(line: &str) -> bool {
    line.starts_with("use ")
        || line.starts_with("require ")
        || line.starts_with("sub ")
        || line.starts_with("package ")
        || line.starts_with("my ")
        || line.starts_with("our ")
        || line.starts_with("local ")
}
