use crate::runtime::input_validation::constants::{
    ALLOWED_EXTENSIONS, MAX_LINE_LENGTH, MAX_PATH_LENGTH,
};
use crate::runtime::limits::max_file_size_bytes as limits_max_file_size_bytes;
use anyhow::{Result, anyhow};
use perl_parser_core::path_security::validate_workspace_path;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Validates and sanitizes a file path to prevent path traversal attacks.
pub fn validate_file_path<P: AsRef<Path>>(path: P, workspace_root: &Path) -> Result<PathBuf> {
    let path = path.as_ref();

    if path.to_string_lossy().len() > MAX_PATH_LENGTH {
        return Err(anyhow!("Path too long: {}", path.display()));
    }

    let validated = validate_workspace_path(path, workspace_root)
        .map_err(|error| anyhow!("Invalid workspace path {}: {error}", path.display()))?;

    if let Some(extension) = validated.extension().and_then(OsStr::to_str)
        && !ALLOWED_EXTENSIONS.contains(&extension)
    {
        return Err(anyhow!(
            "File extension '{}' not allowed. Allowed: {:?}",
            extension,
            ALLOWED_EXTENSIONS
        ));
    }

    Ok(validated)
}

/// Validates file content before parsing to prevent resource exhaustion.
///
/// This guards the LSP text-synchronization path (`textDocument/didOpen` and
/// friends) where `content` is the user's own editor buffer, not a file read
/// off disk on the server's behalf — `validate_file_path` above (and any
/// future disk-ingestion caller) is responsible for that distinct threat
/// model. Buffer content is therefore checked only against resource-exhaustion
/// guards (size, null bytes, per-line length); it deliberately does NOT scan
/// for HTML/script-injection substrings, because ordinary Perl and templating
/// source legitimately contains them — e.g. Mason component blocks open with
/// `<%`, and CGI scripts commonly `print` HTML (including `<script>` tags)
/// from heredocs. Rejecting those on open made the server refuse to load
/// every Mason file (issue #5256 follow-up).
pub fn validate_file_content(content: &str, file_path: &Path) -> Result<()> {
    let max_file_size = limits_max_file_size_bytes();
    if content.len() > max_file_size {
        return Err(anyhow!(
            "File {} too large: {} bytes (max: {} bytes) â€” adjust perl.limits.maxFileSizeBytes to increase",
            file_path.display(),
            content.len(),
            max_file_size
        ));
    }

    if content.contains('\0') {
        return Err(anyhow!("File {} contains null bytes", file_path.display()));
    }

    for (index, line) in content.lines().enumerate() {
        if line.len() > MAX_LINE_LENGTH {
            return Err(anyhow!(
                "Line {} in file {} is too long: {} characters",
                index + 1,
                file_path.display(),
                line.len()
            ));
        }
    }

    Ok(())
}
