#![warn(missing_docs)]
//! Input validation and sanitization utilities for production hardening.

mod constants;
mod file_validation;
mod lsp_validation;
mod sanitize;
mod workspace_validation;

pub use file_validation::{validate_file_content, validate_file_path};
pub use lsp_validation::{
    validate_buffer_line_lengths, validate_document_uri, validate_request_admission,
};
pub use sanitize::sanitize_string;
pub use workspace_validation::validate_workspace_root;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_validate_file_path_valid() {
        use perl_tdd_support::must;
        let temp_dir = must(TempDir::new());
        let workspace_root = temp_dir.path();
        let file_path = workspace_root.join("test.pl");
        must(fs::write(&file_path, "print 'Hello';"));

        let result = validate_file_path(&file_path, workspace_root);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_path_traversal() {
        use perl_tdd_support::must;
        let temp_dir = must(TempDir::new());
        let workspace_root = temp_dir.path();
        let malicious_path = Path::new("../../etc/passwd");

        let result = validate_file_path(malicious_path, workspace_root);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_file_path_rejects_disallowed_extension() {
        use perl_tdd_support::must;
        let temp_dir = must(TempDir::new());
        let workspace_root = temp_dir.path();
        let file_path = workspace_root.join("notes.txt");
        must(fs::write(&file_path, "not perl"));

        let result = validate_file_path(&file_path, workspace_root);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_file_content_valid() {
        let content = "print 'Hello, World!';";
        let file_path = Path::new("test.pl");

        let result = validate_file_content(content, file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_content_too_large() {
        let max = crate::runtime::limits::max_file_size_bytes();
        let mut content = String::new();
        content.reserve(max + 1);
        content.extend(std::iter::repeat_n('x', max + 1));
        let file_path = Path::new("large.pl");

        let result = validate_file_content(&content, file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_file_content_null_bytes() {
        let content = "print 'Hello';\0";
        let file_path = Path::new("null.pl");

        let result = validate_file_content(content, file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_file_content_line_too_long() {
        let long_line = "x".repeat(100_001);
        let file_path = Path::new("long_line.pl");

        let result = validate_file_content(&long_line, file_path);
        assert!(result.is_err());
    }

    /// `validate_file_content` no longer has a production caller: it used to
    /// run on the LSP buffer path (`textDocument/didOpen` et al.) via generic
    /// preflight, but issue #8895 moved that boundary's policy into the sync
    /// sink, whose own configured size guard and binary-content guard now
    /// own those decisions. Content-pattern scanning was removed from this
    /// function earlier (issue #5256 follow-up: it made the server refuse to
    /// open Mason buffers, whose component blocks legitimately start with
    /// `<%`). This regression guard still asserts such content is accepted,
    /// not rejected, so a pattern scan cannot silently return here.
    #[test]
    fn test_validate_file_content_html_like_substrings_are_accepted() {
        let content = "print q{<ScRiPt>alert('xss')</script>};";
        let file_path = Path::new("suspicious.pl");

        let result = validate_file_content(content, file_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_content_exact_size_limit_allowed() {
        let max = crate::runtime::limits::max_file_size_bytes();
        let segment = "x".repeat(100_000);
        let repeats = max / 100_001;
        let remainder = max % 100_001;
        let mut content = (0..repeats).map(|_| format!("{segment}\n")).collect::<String>();
        if remainder > 0 {
            content.push_str(&"x".repeat(remainder));
        }

        let result = validate_file_content(&content, Path::new("exact_limit.pl"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_content_exact_line_limit_allowed() {
        let line = "x".repeat(100_000);
        let result = validate_file_content(&line, Path::new("line_limit.pl"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_sanitize_string() {
        let input = "Hello\x00World<script>alert('xss')</script>";
        let expected = "HelloWorld<script>alert('xss')</script>";

        let result = sanitize_string(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_validate_request_admission_accepts_known_methods() {
        let params = serde_json::json!({
            "textDocument": {
                "uri": "file:///test.pl",
                "text": "print 'Hello';"
            }
        });

        let result = validate_request_admission("textDocument/didOpen", &params);
        assert!(result.is_ok());
    }

    /// Method names are ordinary JSON-RPC strings: any punctuation is
    /// admissible as long as the name fits the length bound (issue #8895).
    #[test]
    fn test_validate_request_admission_accepts_punctuated_method_names() {
        let params = serde_json::json!({});
        for method in [
            "custom/fmt.v2:preview",
            "$/perl-lsp/clientResponse",
            "textDocument/<not-a-tag>",
            "weird method with spaces",
        ] {
            let result = validate_request_admission(method, &params);
            assert!(
                result.is_ok(),
                "method `{method}` must pass structural admission, got: {result:?}"
            );
        }
    }

    #[test]
    fn test_validate_request_admission_rejects_overlong_method() {
        let method = "a".repeat(101);
        let params = serde_json::json!({});

        let result = validate_request_admission(&method, &params);
        assert!(result.is_err());
    }

    /// Parameter *content* is never scanned by admission: browser-dangerous
    /// substrings are inert data unless a sink renders them (issue #8895).
    #[test]
    fn test_validate_request_admission_does_not_scan_param_content() {
        let params = serde_json::json!({
            "expression": "<script>alert('xss')</script>",
            "url": "javascript:void(0)"
        });

        let result = validate_request_admission("custom/eval", &params);
        assert!(result.is_ok(), "admission must not reject arbitrary param payloads by substring");
    }

    #[test]
    fn test_validate_document_uri_accepts_supported_schemes() {
        for uri in [
            "file:///test.pl",
            "untitled:Untitled-1",
            "opencode:/workspace/lib/My/Module.pm",
            "vscode-notebook-cell://wsl%2bubuntu/home/u/nb.ipynb#C1",
        ] {
            let result = validate_document_uri(uri);
            assert!(result.is_ok(), "{uri} must be accepted at the sync sink");
        }
    }

    #[test]
    fn test_validate_document_uri_rejects_unknown_scheme() {
        for uri in [
            "https://example.com/test.pl",
            "http://example.com/test.pl",
            "ftp://example.com/script.pl",
        ] {
            let result = validate_document_uri(uri);
            assert!(
                result.is_err(),
                "schemes the server cannot resolve into paths must be refused"
            );
        }
    }

    #[test]
    fn test_validate_document_uri_rejects_long_uri() {
        let uri = format!("file:///{}", "a".repeat(5000));

        let result = validate_document_uri(&uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_document_uri_boundary() {
        let max_uri = format!("file:///{}", "a".repeat(4096 - 8));
        assert_eq!(max_uri.len(), 4096);
        assert!(validate_document_uri(&max_uri).is_ok(), "URI of exactly MAX_URI_LENGTH is ok");
        let over_uri = format!("file:///{}", "a".repeat(4096 - 8 + 1));
        assert!(
            validate_document_uri(&over_uri).is_err(),
            "URI one byte over MAX_URI_LENGTH must be rejected"
        );
    }

    /// Per-line buffer bound (#8895 review): retained at the didOpen sink as
    /// an explicit parser-robustness resource bound.
    #[test]
    fn test_validate_buffer_line_lengths_boundary() {
        let max_line = "x".repeat(100_000);
        assert!(validate_buffer_line_lengths(&max_line).is_ok());
        let multi = format!("print 1;\n{max_line}\nprint 2;");
        assert!(validate_buffer_line_lengths(&multi).is_ok());

        let over_line = "x".repeat(100_001);
        assert!(
            validate_buffer_line_lengths(&over_line).is_err(),
            "a line over MAX_LINE_LENGTH must be rejected"
        );
        let second_long = format!("print 1;\n{over_line}");
        assert!(
            validate_buffer_line_lengths(&second_long).is_err(),
            "a long line on any line must be rejected"
        );
    }

    #[test]
    fn test_file_size_limit_sourced_from_lsp_limits() {
        let expected_limit = crate::runtime::limits::max_file_size_bytes();
        assert_eq!(expected_limit, 1_024 * 1_024);
    }
}
