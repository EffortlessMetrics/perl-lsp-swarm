#![warn(missing_docs)]
//! Perl import-management helpers for LSP code actions.
//!
//! This crate intentionally focuses on a single responsibility:
//! collecting, classifying, and rewriting `use`/`require` statements.
//!
//! The former hard-coded function-to-module spelling table is withdrawn
//! (#10690): name affinity is not candidate identity and not import edit
//! authorization. Restoration requires #790/#8948 to land exact
//! unresolved-subject selection and insertion planning.

/// Collect import statements (`use` and `require`) from source lines.
#[must_use]
pub fn collect_imports(lines: &[String]) -> Vec<String> {
    let mut imports = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("use ") || trimmed.starts_with("require ") {
            imports.push(line.clone());
        }
    }

    imports
}

/// Sort imports by category: pragmas, core, CPAN-style, then local.
///
/// Duplicates are removed (keeping the first occurrence). Categories are
/// ordered: pragmas -> core -> CPAN -> local, each sorted alphabetically.
#[must_use]
pub fn sort_imports(imports: Vec<String>) -> Vec<String> {
    let mut pragmas = Vec::new();
    let mut core = Vec::new();
    let mut cpan = Vec::new();
    let mut local = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for import in imports {
        let trimmed = import.trim().to_string();
        if !seen.insert(trimmed.clone()) {
            continue;
        }

        if trimmed.contains("strict")
            || trimmed.contains("warnings")
            || trimmed.contains("utf8")
            || trimmed.contains("feature")
        {
            pragmas.push(trimmed);
        } else if trimmed.contains("::") {
            cpan.push(trimmed);
        } else if trimmed.starts_with("use lib") || trimmed.contains("./") {
            local.push(trimmed);
        } else {
            core.push(trimmed);
        }
    }

    pragmas.sort();
    core.sort();
    cpan.sort();
    local.sort();

    let mut result = Vec::new();
    result.extend(pragmas);
    result.extend(core);
    result.extend(cpan);
    result.extend(local);

    result
}

/// Find the byte range containing the contiguous import block boundaries.
#[must_use]
pub fn find_imports_range(source: &str, lines: &[String]) -> Option<(usize, usize)> {
    let imports = collect_imports(lines);
    if imports.is_empty() {
        return None;
    }

    let first = source.find(imports.first()?)?;
    let last_line = imports.last()?;
    let last = source.rfind(last_line)?;
    let last_end = last + last_line.len();

    Some((first, last_end))
}

/// Import management helper wrapper.
///
/// Wraps import management functions in a conventional provider interface.
#[derive(Debug, Default)]
pub struct ImportManager;

impl ImportManager {
    /// Create a new import manager.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Collect import statements from source lines.
    #[must_use]
    pub fn collect(&self, lines: &[String]) -> Vec<String> {
        collect_imports(lines)
    }

    /// Sort import statements by category.
    #[must_use]
    pub fn sort(&self, imports: Vec<String>) -> Vec<String> {
        sort_imports(imports)
    }
}
