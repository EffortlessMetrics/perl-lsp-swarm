use super::typed;
use crate::metadata;
use crate::loading::{
    CorpusLoadError, PlainPerlSource, SectionCaseId, SectionedCase, SectionedCorpusDocument,
};
use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

/// Failure to load a declared sectioned corpus document.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionedCorpusLoadError {
    /// The underlying source asset could not be loaded or represented.
    Source(CorpusLoadError),
    /// A delimiter introduced an incomplete or ambiguous section header.
    MalformedHeader {
        /// Stable parent asset identity.
        asset_id: String,
        /// Runtime path of the malformed document.
        path: PathBuf,
        /// One-based line containing the opening delimiter.
        line: usize,
        /// Stable machine-readable reason token.
        reason: &'static str,
    },
    /// Header validation and the legacy metadata parser disagreed about the case population.
    SectionCountMismatch {
        /// Stable parent asset identity.
        asset_id: String,
        /// Runtime path of the inconsistent document.
        path: PathBuf,
        /// Number of structurally declared sections.
        declared: usize,
        /// Number of sections returned by the metadata parser.
        parsed: usize,
    },
}

impl fmt::Display for SectionedCorpusLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => error.fmt(formatter),
            Self::MalformedHeader { asset_id, path, line, reason } => write!(
                formatter,
                "sectioned corpus asset {asset_id:?} has a malformed header at {}:{line}: {reason}",
                path.display()
            ),
            Self::SectionCountMismatch { asset_id, path, declared, parsed } => write!(
                formatter,
                "sectioned corpus asset {asset_id:?} declared {declared} sections but parsed {parsed}: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SectionedCorpusLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::MalformedHeader { .. } | Self::SectionCountMismatch { .. } => None,
        }
    }
}

impl From<CorpusLoadError> for SectionedCorpusLoadError {
    fn from(error: CorpusLoadError) -> Self {
        Self::Source(error)
    }
}

/// Load one declared ordinary Perl source without interpreting section delimiters.
///
/// Whitespace-only asset identities are rejected before filesystem access.
pub fn load_plain_perl_source(
    asset_id: impl Into<String>,
    path: impl AsRef<Path>,
) -> Result<PlainPerlSource, CorpusLoadError> {
    let asset_id = asset_id.into();
    if asset_id.trim().is_empty() {
        return Err(CorpusLoadError::EmptyAssetId);
    }

    typed::load_plain_perl_source(asset_id, path)
}

/// Load one explicitly declared sectioned corpus document.
///
/// This strict public path validates the complete delimiter/header population
/// before accepting the metadata parser's cases. A valid first section cannot
/// hide a malformed later section by making the parser return a smaller set.
pub fn load_sectioned_corpus_document(
    asset_id: impl Into<String>,
    path: impl AsRef<Path>,
) -> Result<SectionedCorpusDocument, SectionedCorpusLoadError> {
    let asset_id = asset_id.into();
    if asset_id.trim().is_empty() {
        return Err(CorpusLoadError::EmptyAssetId.into());
    }

    let source = typed::load_plain_perl_source(asset_id.clone(), path)?;
    let parser_source = source.source.strip_prefix('\u{feff}').unwrap_or(&source.source);
    let normalized = normalize_newlines(parser_source);
    let declared = validate_headers(&asset_id, &source.path, &normalized)?;
    let sections = metadata::parser::parse_sections(&normalized, &source.path);

    if sections.is_empty() {
        return Err(CorpusLoadError::NoSections {
            asset_id,
            path: source.path,
        }
        .into());
    }
    if sections.len() != declared {
        return Err(SectionedCorpusLoadError::SectionCountMismatch {
            asset_id,
            path: source.path,
            declared,
            parsed: sections.len(),
        });
    }

    let mut seen = BTreeSet::new();
    let mut cases = Vec::with_capacity(sections.len());
    for section in sections {
        if !seen.insert(section.id.clone()) {
            return Err(CorpusLoadError::DuplicateSectionId {
                asset_id,
                section_id: section.id,
            }
            .into());
        }
        cases.push(SectionedCase {
            id: SectionCaseId {
                asset_id: asset_id.clone(),
                section_id: section.id.clone(),
            },
            section,
        });
    }

    Ok(SectionedCorpusDocument {
        asset_id,
        path: source.path,
        source: source.source,
        utf8_bom: source.utf8_bom,
        newline_style: source.newline_style,
        cases,
    })
}

fn validate_headers(
    asset_id: &str,
    path: &Path,
    source: &str,
) -> Result<usize, SectionedCorpusLoadError> {
    let lines = source.split('\n').collect::<Vec<_>>();
    let mut declared = 0usize;
    let mut index = 0usize;

    while index < lines.len() {
        if !is_delimiter(lines[index]) {
            index += 1;
            continue;
        }

        let line = index + 1;
        let Some(title) = lines.get(index + 1) else {
            return Err(malformed(asset_id, path, line, "missing_title"));
        };
        if title.trim().is_empty() {
            return Err(malformed(asset_id, path, line, "empty_title"));
        }

        let Some(closing) = lines.get(index + 2) else {
            return Err(malformed(asset_id, path, line, "missing_closing_delimiter"));
        };
        if !is_delimiter(closing) {
            return Err(malformed(asset_id, path, line, "missing_closing_delimiter"));
        }

        declared += 1;
        index += 3;
    }

    Ok(declared)
}

fn malformed(
    asset_id: &str,
    path: &Path,
    line: usize,
    reason: &'static str,
) -> SectionedCorpusLoadError {
    SectionedCorpusLoadError::MalformedHeader {
        asset_id: asset_id.to_string(),
        path: path.to_path_buf(),
        line,
        reason,
    }
}

fn is_delimiter(line: &str) -> bool {
    let mut characters = line.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    first == '=' && characters.skip_while(|character| *character == '=').all(char::is_whitespace)
}

fn normalize_newlines(source: &str) -> String {
    if !source.as_bytes().contains(&b'\r') {
        return source.to_string();
    }

    let mut normalized = String::with_capacity(source.len());
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\r' {
            if characters.peek() == Some(&'\n') {
                characters.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn strict_loader_rejects_a_malformed_later_header()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("partial.txt");
        fs::write(
            &path,
            "====\nValid\n====\nmy $value = 1;\n====\nBroken\nmy $value = 2;\n",
        )?;

        assert!(matches!(
            load_sectioned_corpus_document("corpus/partial.txt", &path),
            Err(SectionedCorpusLoadError::MalformedHeader {
                line: 5,
                reason: "missing_closing_delimiter",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn strict_loader_rejects_whitespace_only_asset_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("case.txt");
        fs::write(&path, "====\nCase\n====\n1;\n")?;

        assert!(matches!(
            load_plain_perl_source("  ", &path),
            Err(CorpusLoadError::EmptyAssetId)
        ));
        assert!(matches!(
            load_sectioned_corpus_document("\t", &path),
            Err(SectionedCorpusLoadError::Source(CorpusLoadError::EmptyAssetId))
        ));
        Ok(())
    }

    #[test]
    fn strict_loader_accepts_crlf_headers_without_changing_exact_source()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("crlf.txt");
        let source = "====\r\nCase\r\n====\r\nmy $value = 1;\r\n";
        fs::write(&path, source)?;

        let document = load_sectioned_corpus_document("corpus/crlf.txt", &path)?;
        assert_eq!(document.source, source);
        assert_eq!(document.cases.len(), 1);
        assert_eq!(document.cases[0].section.body, "my $value = 1;");
        Ok(())
    }
}
