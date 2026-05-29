//! textDocument/documentLink handler - clickable module/file links
//!
//! This module creates clickable links for:
//! - Module names (use/require) -> MetaCPAN
//! - File paths in require/do -> local files

use lsp_types::{DocumentLink, Position, Range, Uri};
use std::path::PathBuf;
use url::Url;

fn to_range(content: &str, start: usize, end: usize) -> Range {
    // Simple byte->(line,col) translator
    let (mut line, mut col, mut i) = (0u32, 0u32, 0usize);
    let mut start_pos = Position::new(0, 0);
    let mut end_pos = Position::new(0, 0);
    for ch in content.chars() {
        if i == start {
            start_pos = Position::new(line, col);
        }
        if i == end {
            end_pos = Position::new(line, col);
            break;
        }
        i += ch.len_utf8();
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    if end_pos == Position::new(0, 0) {
        end_pos = Position::new(line, col);
    }
    Range::new(start_pos, end_pos)
}

/// Collects clickable document links from Perl source code.
///
/// Scans the document for `use`, `require`, and `do` statements, creating links for:
/// - Module names (e.g., `use Foo::Bar;`) → MetaCPAN URLs
/// - File paths in quotes (e.g., `require 'path/file.pl'`) → local file URLs
///
/// # Arguments
/// * `text` - The Perl source code to scan
/// * `uri` - The document's URI, used to resolve relative file paths
///
/// # Returns
/// A vector of `DocumentLink` objects with range, target URL, and tooltip.
pub fn collect_document_links(text: &str, uri: &Url) -> Result<Vec<DocumentLink>, String> {
    let mut links = Vec::new();

    for (line_idx, line) in text.lines().enumerate() {
        // `use Foo::Bar;`
        if let Some(idx) = line.find("use ") {
            let rest = &line[idx + 4..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == ':' || *c == '_')
                .collect();
            if !name.is_empty() && name.contains("::") {
                let s =
                    text[..].lines().take(line_idx).map(|l| l.len() + 1).sum::<usize>() + idx + 4;
                let e = s + name.len();
                links.push(DocumentLink {
                    range: to_range(text, s, e),
                    target: Url::parse(&format!("https://metacpan.org/pod/{}", name))
                        .map_err(|e| {
                            tracing::debug!(module = %name, error = %e, "document link: failed to build MetaCPAN URL");
                        })
                        .ok()
                        .and_then(|url| url.to_string().parse::<Uri>().map_err(|e| {
                            tracing::debug!(module = %name, error = %e, "document link: failed to parse MetaCPAN URI");
                        }).ok()),
                    tooltip: Some(format!("Open {} on MetaCPAN", name)),
                    data: None,
                });
            }
        }

        // `require Module::Name`
        if let Some(idx) = line.find("require ") {
            let rest = &line[idx + 8..];
            // Check if it's a module name (not a file path)
            if !rest.trim_start().starts_with(['\'', '"']) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == ':' || *c == '_')
                    .collect();
                if !name.is_empty() && name.contains("::") {
                    let s = text[..].lines().take(line_idx).map(|l| l.len() + 1).sum::<usize>()
                        + idx
                        + 8;
                    let e = s + name.len();
                    links.push(DocumentLink {
                        range: to_range(text, s, e),
                        target: Url::parse(&format!("https://metacpan.org/pod/{}", name))
                            .map_err(|e| {
                                tracing::debug!(module = %name, error = %e, "document link: failed to build MetaCPAN URL");
                            })
                            .ok()
                            .and_then(|url| url.to_string().parse::<Uri>().map_err(|e| {
                                tracing::debug!(module = %name, error = %e, "document link: failed to parse MetaCPAN URI");
                            }).ok()),
                        tooltip: Some(format!("Open {} on MetaCPAN", name)),
                        data: None,
                    });
                }
            }
        }

        // `require 'path'` / `do "path"`
        for kw in ["require ", "do "] {
            if let Some(idx) = line.find(kw) {
                let rest = &line[idx + kw.len()..];
                let quote = rest.chars().next().unwrap_or(' ');
                if quote == '\'' || quote == '"' {
                    if let Some(endq) = rest[1..].find(quote) {
                        let path = &rest[1..1 + endq];
                        let s = text[..].lines().take(line_idx).map(|l| l.len() + 1).sum::<usize>()
                            + idx
                            + kw.len()
                            + 1;
                        let e = s + path.len();
                        // Try to resolve relative to current file
                        let target = if PathBuf::from(path).is_absolute() {
                            // Absolute path - works on both Unix and Windows
                            Url::from_file_path(path)
                                .map_err(|()| {
                                    tracing::debug!(
                                        path,
                                        "document link: failed to convert absolute path to URL"
                                    );
                                })
                                .ok()
                        } else {
                            // Relative to current file's directory
                            uri.to_file_path().map_err(|()| {
                                tracing::debug!(uri = %uri, "document link: URI is not a file path");
                            }).ok().and_then(|base_path| {
                                base_path.parent().and_then(|parent| {
                                    let resolved = parent.join(path);
                                    // Normalize the path for the current OS
                                    Url::from_file_path(&resolved).map_err(|()| {
                                        tracing::debug!(path = %resolved.display(), "document link: failed to convert resolved path to URL");
                                    }).ok()
                                })
                            })
                        };
                        if let Some(target_url) = target {
                            // Get display path for tooltip
                            let display_path = if let Ok(file_path) = target_url.to_file_path() {
                                file_path.display().to_string()
                            } else {
                                path.to_string()
                            };
                            links.push(DocumentLink {
                                range: to_range(text, s, e),
                                target: target_url.to_string().parse::<Uri>().map_err(|e| {
                                    tracing::debug!(error = %e, "document link: failed to parse file URI");
                                }).ok(),
                                tooltip: Some(format!("Open {}", display_path)),
                                data: None,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::collect_document_links;
    use anyhow::{Result, anyhow};
    use lsp_types::Position;
    use perl_tdd_support::must_some;
    use std::path::Path;
    use url::Url;

    fn file_uri(path: &Path) -> Result<Url> {
        Url::from_file_path(path)
            .map_err(|()| anyhow!("could not build file URI for {}", path.display()))
    }

    #[test]
    fn use_and_require_modules_link_to_metacpan() -> Result<()> {
        let uri = file_uri(Path::new("/tmp/app/lib/App.pm"))?;
        let text = "use Local::Thing;\nrequire Remote::Widget;\n";

        let links = collect_document_links(text, &uri).map_err(anyhow::Error::msg)?;

        assert_eq!(links.len(), 2);
        let use_link = must_some(links.first());
        assert_eq!(use_link.range.start, Position::new(0, 4));
        assert_eq!(use_link.range.end, Position::new(0, 16));
        assert_eq!(
            use_link.target.as_ref().map(|uri| uri.as_str()),
            Some("https://metacpan.org/pod/Local::Thing")
        );
        assert_eq!(use_link.tooltip.as_deref(), Some("Open Local::Thing on MetaCPAN"));

        let require_link = must_some(links.get(1));
        assert_eq!(require_link.range.start, Position::new(1, 8));
        assert_eq!(require_link.range.end, Position::new(1, 22));
        assert_eq!(
            require_link.target.as_ref().map(|uri| uri.as_str()),
            Some("https://metacpan.org/pod/Remote::Widget")
        );
        assert_eq!(require_link.tooltip.as_deref(), Some("Open Remote::Widget on MetaCPAN"));
        Ok(())
    }

    #[test]
    fn quoted_require_and_do_paths_resolve_relative_to_document() -> Result<()> {
        let uri = file_uri(Path::new("/tmp/app/bin/script.pl"))?;
        let text = "require '../lib/bootstrap.pl';\ndo \"../share/config.pl\";\n";

        let links = collect_document_links(text, &uri).map_err(anyhow::Error::msg)?;

        assert_eq!(links.len(), 2);
        let require_link = must_some(links.first());
        assert_eq!(require_link.range.start, Position::new(0, 9));
        assert_eq!(require_link.range.end, Position::new(0, 28));
        assert_eq!(
            require_link.target.as_ref().map(|uri| uri.as_str()),
            Some("file:///tmp/app/bin/../lib/bootstrap.pl")
        );

        let do_link = must_some(links.get(1));
        assert_eq!(do_link.range.start, Position::new(1, 4));
        assert_eq!(do_link.range.end, Position::new(1, 22));
        assert_eq!(
            do_link.target.as_ref().map(|uri| uri.as_str()),
            Some("file:///tmp/app/bin/../share/config.pl")
        );
        Ok(())
    }

    #[test]
    fn link_ranges_use_utf16_columns_after_wide_characters() -> Result<()> {
        let uri = file_uri(Path::new("/tmp/app/lib/App.pm"))?;
        let text = "my $x = '🚀'; use Wide::Module;\n";

        let links = collect_document_links(text, &uri).map_err(anyhow::Error::msg)?;

        assert_eq!(links.len(), 1);
        let code_link = must_some(links.first());
        assert_eq!(code_link.range.start, Position::new(0, 18));
        assert_eq!(code_link.range.end, Position::new(0, 30));
        assert_eq!(
            code_link.target.as_ref().map(|uri| uri.as_str()),
            Some("https://metacpan.org/pod/Wide::Module")
        );
        Ok(())
    }

    #[test]
    fn quoted_module_like_require_is_treated_as_file_path() -> Result<()> {
        let uri = file_uri(Path::new("/tmp/app/script.pl"))?;
        let text = "require 'Local::Thing';\n";

        let links = collect_document_links(text, &uri).map_err(anyhow::Error::msg)?;

        assert_eq!(links.len(), 1);
        let link = must_some(links.first());
        assert_eq!(link.range.start, Position::new(0, 9));
        assert_eq!(link.range.end, Position::new(0, 21));
        assert_eq!(
            link.target.as_ref().map(|uri| uri.as_str()),
            Some("file:///tmp/app/Local::Thing")
        );
        Ok(())
    }
}
