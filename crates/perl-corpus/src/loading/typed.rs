use crate::metadata::{self, Section};
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File};
#[cfg(any(
    windows,
    all(
        any(target_os = "linux", target_os = "android"),
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "riscv32",
            target_arch = "riscv64"
        )
    ),
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Observed newline representation in a loaded UTF-8 source asset.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewlineStyle {
    /// The source contains no newline characters.
    None,
    /// Every newline is line feed (`\n`).
    Lf,
    /// Every newline is carriage-return plus line-feed (`\r\n`).
    CrLf,
    /// Every newline is carriage return (`\r`).
    Cr,
    /// The source mixes two or more newline representations.
    Mixed,
}

impl NewlineStyle {
    /// Stable machine-readable newline token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lf => "lf",
            Self::CrLf => "crlf",
            Self::Cr => "cr",
            Self::Mixed => "mixed",
        }
    }
}

/// Exact UTF-8 Perl source loaded through the plain-source path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainPerlSource {
    /// Stable parent asset identity supplied by topology or the caller.
    pub asset_id: String,
    /// Runtime source path used for this load.
    pub path: PathBuf,
    /// Exact UTF-8 source text, including any UTF-8 BOM and original newlines.
    pub source: String,
    /// Whether the source begins with a UTF-8 BOM.
    pub utf8_bom: bool,
    /// Observed newline representation.
    pub newline_style: NewlineStyle,
}

/// Stable identity of one case expanded from a sectioned corpus document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SectionCaseId {
    /// Stable identity of the parent sectioned document.
    pub asset_id: String,
    /// Explicit or generated section identity within that document.
    pub section_id: String,
}

/// One parsed case from a sectioned corpus document.
#[derive(Debug, Clone)]
pub struct SectionedCase {
    /// Parent-plus-section identity; fields remain separate to avoid delimiter ambiguity.
    pub id: SectionCaseId,
    /// Existing section metadata and source body.
    pub section: Section,
}

/// A strict sectioned corpus document and its expanded cases.
#[derive(Debug, Clone)]
pub struct SectionedCorpusDocument {
    /// Stable parent asset identity supplied by topology or the caller.
    pub asset_id: String,
    /// Runtime source path used for this load.
    pub path: PathBuf,
    /// Exact UTF-8 document text, including any UTF-8 BOM and original newlines.
    pub source: String,
    /// Whether the document begins with a UTF-8 BOM.
    pub utf8_bom: bool,
    /// Observed newline representation before normalization for section parsing.
    pub newline_style: NewlineStyle,
    /// Parsed cases in source order.
    pub cases: Vec<SectionedCase>,
}

/// Failure to load a declared plain or sectioned corpus asset.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorpusLoadError {
    /// The caller omitted the stable parent asset identity.
    EmptyAssetId,
    /// The selected asset does not exist.
    Missing {
        /// Missing asset path.
        path: PathBuf,
    },
    /// The selected asset is a symbolic link or reparse point.
    SymlinkUnsupported {
        /// Rejected asset path.
        path: PathBuf,
    },
    /// The selected asset exists but is not a regular file.
    NotRegularFile {
        /// Rejected asset path.
        path: PathBuf,
    },
    /// This target has no reviewed no-follow open contract.
    NoFollowUnsupported {
        /// Asset path that could not be opened safely.
        path: PathBuf,
    },
    /// The selected asset could not be opened, inspected, or read.
    Io {
        /// Asset path being inspected.
        path: PathBuf,
        /// Rendered operating-system error.
        message: String,
    },
    /// The selected asset is not strict UTF-8.
    InvalidUtf8 {
        /// Rejected asset path.
        path: PathBuf,
        /// Byte offset before the invalid sequence.
        valid_up_to: usize,
    },
    /// A declared sectioned document contained no section headers.
    NoSections {
        /// Parent asset identity.
        asset_id: String,
        /// Rejected document path.
        path: PathBuf,
    },
    /// A section delimiter candidate was structurally malformed.
    MalformedSection {
        /// Parent asset identity.
        asset_id: String,
        /// Rejected document path.
        path: PathBuf,
        /// One-based opening-delimiter line.
        line: usize,
        /// Stable machine-readable reason token.
        reason: &'static str,
    },
    /// Strict header validation and metadata parsing produced different populations.
    SectionPopulationMismatch {
        /// Parent asset identity.
        asset_id: String,
        /// Number of structurally declared sections.
        declared: usize,
        /// Number of parsed sections.
        parsed: usize,
    },
    /// Two sections in one document resolved to the same identity.
    DuplicateSectionId {
        /// Parent asset identity.
        asset_id: String,
        /// Duplicated section identity.
        section_id: String,
    },
}

impl fmt::Display for CorpusLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAssetId => formatter.write_str("corpus asset identity must not be empty"),
            Self::Missing { path } => {
                write!(formatter, "corpus asset does not exist: {}", path.display())
            }
            Self::SymlinkUnsupported { path } => write!(
                formatter,
                "corpus asset symlink or reparse point is unsupported: {}",
                path.display()
            ),
            Self::NotRegularFile { path } => {
                write!(formatter, "corpus asset is not a regular file: {}", path.display())
            }
            Self::NoFollowUnsupported { path } => write!(
                formatter,
                "target cannot safely open corpus asset without following links: {}",
                path.display()
            ),
            Self::Io { path, message } => {
                write!(formatter, "failed to read corpus asset {}: {message}", path.display())
            }
            Self::InvalidUtf8 {
                path,
                valid_up_to,
            } => write!(
                formatter,
                "corpus asset {} is not valid UTF-8 at byte {valid_up_to}",
                path.display()
            ),
            Self::NoSections { asset_id, path } => write!(
                formatter,
                "declared sectioned corpus asset {asset_id:?} contained no sections: {}",
                path.display()
            ),
            Self::MalformedSection {
                asset_id,
                path,
                line,
                reason,
            } => write!(
                formatter,
                "sectioned corpus asset {asset_id:?} is malformed at {}:{line}: {reason}",
                path.display()
            ),
            Self::SectionPopulationMismatch {
                asset_id,
                declared,
                parsed,
            } => write!(
                formatter,
                "sectioned corpus asset {asset_id:?} declared {declared} sections but parsed {parsed}"
            ),
            Self::DuplicateSectionId {
                asset_id,
                section_id,
            } => write!(
                formatter,
                "sectioned corpus asset {asset_id:?} contains duplicate section ID {section_id:?}"
            ),
        }
    }
}

impl std::error::Error for CorpusLoadError {}

/// Load one declared ordinary Perl source without interpreting section delimiters.
pub fn load_plain_perl_source(
    asset_id: impl Into<String>,
    path: impl AsRef<Path>,
) -> Result<PlainPerlSource, CorpusLoadError> {
    let asset_id = validate_asset_id(asset_id.into())?;
    let path = path.as_ref();
    let loaded = read_utf8_regular_file(path)?;

    Ok(PlainPerlSource {
        asset_id,
        path: path.to_path_buf(),
        source: loaded.source,
        utf8_bom: loaded.utf8_bom,
        newline_style: loaded.newline_style,
    })
}

/// Load one explicitly declared sectioned corpus document.
///
/// Newlines are normalized only for metadata/section parsing. The exact input
/// remains available in [`SectionedCorpusDocument::source`]. A `.txt` file is
/// not sectioned merely because of its extension; callers must choose this API.
pub fn load_sectioned_corpus_document(
    asset_id: impl Into<String>,
    path: impl AsRef<Path>,
) -> Result<SectionedCorpusDocument, CorpusLoadError> {
    let asset_id = validate_asset_id(asset_id.into())?;
    let path = path.as_ref();
    let loaded = read_utf8_regular_file(path)?;
    let without_bom = loaded
        .source
        .strip_prefix('\u{feff}')
        .unwrap_or(&loaded.source);
    let normalized = normalize_newlines(without_bom);
    let declared = validate_section_headers(&asset_id, path, &normalized)?;

    // The legacy parser continues to derive its compatibility ID from an
    // asset-shaped path. The public structured ID is assigned separately by
    // `sectioned_identity` and does not treat this field as global authority.
    let sections = metadata::parser::parse_sections(&normalized, Path::new(&asset_id));
    if sections.len() != declared {
        return Err(CorpusLoadError::SectionPopulationMismatch {
            asset_id,
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
            });
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
        path: path.to_path_buf(),
        source: loaded.source,
        utf8_bom: loaded.utf8_bom,
        newline_style: loaded.newline_style,
        cases,
    })
}

struct LoadedUtf8 {
    source: String,
    utf8_bom: bool,
    newline_style: NewlineStyle,
}

fn validate_asset_id(asset_id: String) -> Result<String, CorpusLoadError> {
    if asset_id.trim().is_empty() {
        return Err(CorpusLoadError::EmptyAssetId);
    }
    Ok(asset_id)
}

fn validate_section_headers(
    asset_id: &str,
    path: &Path,
    source: &str,
) -> Result<usize, CorpusLoadError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0usize;
    let mut declared = 0usize;

    while index < lines.len() {
        if !is_section_delimiter(lines[index]) {
            index += 1;
            continue;
        }

        let opening_line = index + 1;
        let Some(title) = lines.get(index + 1) else {
            return Err(CorpusLoadError::MalformedSection {
                asset_id: asset_id.to_string(),
                path: path.to_path_buf(),
                line: opening_line,
                reason: "missing_title",
            });
        };
        if title.trim().is_empty() || is_section_delimiter(title) {
            return Err(CorpusLoadError::MalformedSection {
                asset_id: asset_id.to_string(),
                path: path.to_path_buf(),
                line: opening_line,
                reason: "missing_title",
            });
        }

        let Some(closing) = lines.get(index + 2) else {
            return Err(CorpusLoadError::MalformedSection {
                asset_id: asset_id.to_string(),
                path: path.to_path_buf(),
                line: opening_line,
                reason: "missing_closing_delimiter",
            });
        };
        if !is_section_delimiter(closing) {
            return Err(CorpusLoadError::MalformedSection {
                asset_id: asset_id.to_string(),
                path: path.to_path_buf(),
                line: opening_line,
                reason: "missing_closing_delimiter",
            });
        }

        declared += 1;
        index += 3;
    }

    if declared == 0 {
        return Err(CorpusLoadError::NoSections {
            asset_id: asset_id.to_string(),
            path: path.to_path_buf(),
        });
    }

    Ok(declared)
}

fn is_section_delimiter(line: &str) -> bool {
    let without_trailing_whitespace = line.trim_end();
    !without_trailing_whitespace.is_empty()
        && without_trailing_whitespace
            .as_bytes()
            .iter()
            .all(|byte| *byte == b'=')
}

fn read_utf8_regular_file(path: &Path) -> Result<LoadedUtf8, CorpusLoadError> {
    read_utf8_regular_file_with_opener(path, open_readonly_no_follow)
}

fn read_utf8_regular_file_with_opener<F>(
    path: &Path,
    opener: F,
) -> Result<LoadedUtf8, CorpusLoadError>
where
    F: FnOnce(&Path) -> io::Result<File>,
{
    let mut file = opener(path).map_err(|error| classify_open_error(path, error))?;
    let metadata = file.metadata().map_err(|error| CorpusLoadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if metadata_is_link_like(&metadata) {
        return Err(CorpusLoadError::SymlinkUnsupported {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_file() {
        return Err(CorpusLoadError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| CorpusLoadError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    let source = String::from_utf8(bytes).map_err(|error| CorpusLoadError::InvalidUtf8 {
        path: path.to_path_buf(),
        valid_up_to: error.utf8_error().valid_up_to(),
    })?;
    let utf8_bom = source.starts_with('\u{feff}');
    let newline_style = detect_newline_style(&source);

    Ok(LoadedUtf8 {
        source,
        utf8_bom,
        newline_style,
    })
}

fn classify_open_error(path: &Path, error: io::Error) -> CorpusLoadError {
    if error.kind() == io::ErrorKind::NotFound {
        return CorpusLoadError::Missing {
            path: path.to_path_buf(),
        };
    }
    if error.kind() == io::ErrorKind::Unsupported {
        return CorpusLoadError::NoFollowUnsupported {
            path: path.to_path_buf(),
        };
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata_is_link_like(&metadata) => CorpusLoadError::SymlinkUnsupported {
            path: path.to_path_buf(),
        },
        Ok(metadata) if !metadata.is_file() => CorpusLoadError::NotRegularFile {
            path: path.to_path_buf(),
        },
        _ => CorpusLoadError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        },
    }
}

#[cfg(all(
    any(target_os = "linux", target_os = "android"),
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv32",
        target_arch = "riscv64"
    )
))]
const NOFOLLOW_OPEN_FLAGS: i32 = 0x0002_0000 | 0x0000_0800;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
const NOFOLLOW_OPEN_FLAGS: i32 = 0x0000_0100 | 0x0000_0004;

#[cfg(any(
    all(
        any(target_os = "linux", target_os = "android"),
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "riscv32",
            target_arch = "riscv64"
        )
    ),
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
fn open_readonly_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW_OPEN_FLAGS)
        .open(path)
}

#[cfg(all(
    unix,
    not(any(
        all(
            any(target_os = "linux", target_os = "android"),
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "arm",
                target_arch = "aarch64",
                target_arch = "riscv32",
                target_arch = "riscv64"
            )
        ),
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
fn open_readonly_no_follow(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no reviewed no-follow open flags for this Unix target ABI",
    ))
}

#[cfg(windows)]
fn open_readonly_no_follow(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_readonly_no_follow(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no reviewed no-follow open contract for this target",
    ))
}

#[cfg(windows)]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn detect_newline_style(source: &str) -> NewlineStyle {
    let bytes = source.as_bytes();
    let mut lf = 0usize;
    let mut crlf = 0usize;
    let mut cr = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                crlf += 1;
                index += 2;
            }
            b'\r' => {
                cr += 1;
                index += 1;
            }
            b'\n' => {
                lf += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }

    match (lf > 0, crlf > 0, cr > 0) {
        (false, false, false) => NewlineStyle::None,
        (true, false, false) => NewlineStyle::Lf,
        (false, true, false) => NewlineStyle::CrLf,
        (false, false, true) => NewlineStyle::Cr,
        _ => NewlineStyle::Mixed,
    }
}

fn normalize_newlines(source: &str) -> String {
    if !source.as_bytes().contains(&b'\r') {
        return source.to_string();
    }

    let mut normalized = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

#[cfg(all(
    test,
    any(
        windows,
        all(
            any(target_os = "linux", target_os = "android"),
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "arm",
                target_arch = "aarch64",
                target_arch = "riscv32",
                target_arch = "riscv64"
            )
        ),
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )
))]
mod tests {
    use super::*;

    fn sectioned_source(newline: &str) -> String {
        [
            "==========================================",
            "First case",
            "==========================================",
            "# @id: first.case",
            "my $value = 1;",
            "",
            "==========================================",
            "Second case",
            "==========================================",
            "# @id: second.case",
            "my $value = 2;",
            "",
        ]
        .join(newline)
    }

    #[test]
    fn plain_loader_preserves_delimiter_like_source_exactly()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("ordinary.pl");
        let source = "my $text = <<'END';\n====\n---\nEND\n";
        fs::write(&path, source)?;

        let loaded = load_plain_perl_source("test_corpus/ordinary.pl", &path)?;
        assert_eq!(loaded.source, source);
        assert_eq!(loaded.newline_style, NewlineStyle::Lf);
        assert!(!loaded.utf8_bom);
        Ok(())
    }

    #[test]
    fn sectioned_loader_binds_cases_to_parent_asset()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sections.txt");
        fs::write(&path, sectioned_source("\n"))?;

        let loaded =
            load_sectioned_corpus_document("tree_sitter/corpus/sections.txt", &path)?;
        assert_eq!(loaded.cases.len(), 2);
        assert_eq!(loaded.cases[0].id.section_id, "first.case");
        assert_eq!(loaded.cases[1].id.section_id, "second.case");
        Ok(())
    }

    #[test]
    fn sectioned_loader_records_bom_and_normalizes_cr_only_for_parsing()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("legacy.txt");
        let source = format!("\u{feff}{}", sectioned_source("\r"));
        fs::write(&path, &source)?;

        let loaded =
            load_sectioned_corpus_document("tree_sitter/corpus/legacy.txt", &path)?;
        assert_eq!(loaded.source, source);
        assert!(loaded.utf8_bom);
        assert_eq!(loaded.newline_style, NewlineStyle::Cr);
        assert_eq!(loaded.cases.len(), 2);
        Ok(())
    }

    #[test]
    fn sectioned_loader_rejects_duplicate_explicit_ids()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("duplicate.txt");
        let source = sectioned_source("\n").replace("second.case", "first.case");
        fs::write(&path, source)?;

        assert!(matches!(
            load_sectioned_corpus_document("corpus/duplicate.txt", &path),
            Err(CorpusLoadError::DuplicateSectionId {
                section_id,
                ..
            }) if section_id == "first.case"
        ));
        Ok(())
    }

    #[test]
    fn sectioned_loader_rejects_valid_then_malformed_section()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("partial.txt");
        let source = concat!(
            "==========================================\n",
            "Valid case\n",
            "==========================================\n",
            "my $value = 1;\n",
            "==========================================\n",
            "\n",
            "==========================================\n",
            "my $value = 2;\n",
        );
        fs::write(&path, source)?;

        assert!(matches!(
            load_sectioned_corpus_document("corpus/partial.txt", &path),
            Err(CorpusLoadError::MalformedSection {
                line: 5,
                reason: "missing_title",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn sectioned_loader_rejects_plain_source_without_sections()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("plain.pl");
        fs::write(&path, "my $value = 1;\n---\n")?;

        assert!(matches!(
            load_sectioned_corpus_document("test_corpus/plain.pl", &path),
            Err(CorpusLoadError::NoSections { .. })
        ));
        Ok(())
    }

    #[test]
    fn loaders_reject_invalid_utf8() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("invalid.pl");
        fs::write(&path, b"my $value = '\xff';\n")?;

        assert!(matches!(
            load_plain_perl_source("test_corpus/invalid.pl", &path),
            Err(CorpusLoadError::InvalidUtf8 { .. })
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn loader_reads_the_opened_handle_after_path_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir()?;
        let path = dir.path().join("case.pl");
        let moved = dir.path().join("opened.pl");
        let replacement = dir.path().join("replacement.pl");
        fs::write(&path, "my $original = 1;\n")?;
        fs::write(&replacement, "my $replacement = 1;\n")?;

        let loaded = read_utf8_regular_file_with_opener(&path, |selected| {
            let file = open_readonly_no_follow(selected)?;
            fs::rename(selected, &moved)?;
            symlink(&replacement, selected)?;
            Ok(file)
        })?;

        assert_eq!(loaded.source, "my $original = 1;\n");
        Ok(())
    }
}

#[cfg(all(
    test,
    not(any(
        windows,
        all(
            any(target_os = "linux", target_os = "android"),
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "arm",
                target_arch = "aarch64",
                target_arch = "riscv32",
                target_arch = "riscv64"
            )
        ),
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))
))]
mod unsupported_target_tests {
    use super::*;

    #[test]
    fn regular_file_fails_with_no_follow_unsupported()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("case.pl");
        fs::write(&path, "my $value = 1;\n")?;

        assert_eq!(
            load_plain_perl_source("test_corpus/case.pl", &path),
            Err(CorpusLoadError::NoFollowUnsupported { path })
        );
        Ok(())
    }
}
