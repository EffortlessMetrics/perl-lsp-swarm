//! `textDocument/hover` honesty for AUTOLOAD dynamic dispatch (#14256, parent #1763).
//!
//! `AUTOLOAD` is a `DynamicBoundary` under
//! `docs/specs/PLSP-SPEC-0017-fact-provenance-and-source-backing.md`: the handler is
//! source-backed, but the method name that reaches it is computed at runtime. The spec
//! allows such a fact to "explain fallback" and forbids it becoming "exact definition,
//! reference, symbol, token, or receiver proof".
//!
//! These are end-to-end tests through the real LSP request path, so they prove what a
//! user actually sees rather than what the analyzer returns internally. The
//! analyzer-level confidence assertions live in
//! `perl-semantic-analyzer/src/analysis/semantic/mod.rs`.

// Integration tests print diagnostic output for CI troubleshooting; this is
// not the LSP server's stdio transport, so print_stdout doesn't apply the
// way it does to production code.
#![allow(clippy::print_stdout)]

mod common;

#[cfg(test)]
mod autoload_hover_confidence {
    use crate::common::test_utils::TestServerBuilder;
    use serde_json::Value;

    /// Extract the markdown hover string from an LSP hover response.
    fn hover_content(resp: &Value) -> Option<String> {
        let result = resp.get("result")?;
        if result.is_null() {
            return None;
        }
        Some(contents_value(result)?.to_string())
    }

    fn contents_value(result: &Value) -> Option<&str> {
        result.get("contents")?.get("value")?.as_str()
    }

    /// Compute (line, character) of `needle` on `target_line`.
    fn find_pos(
        code: &str,
        needle: &str,
        target_line: usize,
    ) -> Result<(u32, u32), Box<dyn std::error::Error>> {
        let line = code
            .lines()
            .nth(target_line)
            .ok_or_else(|| format!("no line {target_line} in test code"))?;
        let col = line
            .find(needle)
            .ok_or_else(|| format!("could not find `{needle}` on line {target_line}"))?;
        Ok((target_line as u32, col as u32))
    }

    /// A class whose only match for `undefined_method` is its AUTOLOAD handler,
    /// plus a real method used as the opposite-direction control.
    ///
    /// Receivers are `$self` and the bare class name deliberately. Hover
    /// resolves a receiver to a package only for `$self`/`$this`/`$class` and
    /// for uppercase bare identifiers (`resolve_receiver_package_name`,
    /// `hover.rs:898`); an opaque `my $obj = Widget->new()` receiver falls
    /// through to generic token hover whether or not AUTOLOAD is involved.
    /// That receiver-inference limitation is pre-existing and outside this
    /// claim, so the fixture uses the forms the LSP can actually resolve.
    const AUTOLOAD_CLASS: &str = r#"package Widget;

sub new { my $c = shift; return bless {}, $c; }

sub real_method { return 1; }

sub caller_method {
    my $self = shift;
    $self->undefined_method();
    $self->real_method();
    return;
}

sub AUTOLOAD {
    our $AUTOLOAD;
    my $self = shift;
    return 0;
}

package main;

Widget->undefined_method();
"#;

    #[test]
    fn autoload_dispatch_hover_is_marked_dynamic() -> Result<(), Box<dyn std::error::Error>> {
        let uri = "file:///autoload_dynamic.pl";
        let server = TestServerBuilder::new().build();
        server.open_document(uri, AUTOLOAD_CLASS);

        let (line, character) = find_pos(AUTOLOAD_CLASS, "undefined_method", 8)?;
        let response = server.get_hover(uri, line, character);
        println!("AUTOLOAD HOVER RESPONSE: {response:#}");

        let content = hover_content(&response).ok_or("expected hover content for AUTOLOAD call")?;

        // The card must not read as a plain, exact method.
        assert!(
            content.contains("dynamic dispatch"),
            "AUTOLOAD hover must mark the dispatch as dynamic, got: {content}"
        );
        // And it must say why, so the reader knows the name is runtime-resolved.
        assert!(
            content.contains("resolved at runtime"),
            "AUTOLOAD hover must explain that the method name is runtime-resolved, got: {content}"
        );
        // The pre-existing provenance explanation must survive.
        assert!(
            content.contains("AUTOLOAD"),
            "AUTOLOAD hover must still name the handler, got: {content}"
        );
        Ok(())
    }

    /// Opposite-direction control: an exactly-resolved method on the *same*
    /// class must keep the plain card. An implementation that marks every hover
    /// from an AUTOLOAD-bearing class as dynamic fails here.
    #[test]
    fn exact_method_hover_is_not_marked_dynamic() -> Result<(), Box<dyn std::error::Error>> {
        let uri = "file:///autoload_exact.pl";
        let server = TestServerBuilder::new().build();
        server.open_document(uri, AUTOLOAD_CLASS);

        let (line, character) = find_pos(AUTOLOAD_CLASS, "real_method", 9)?;
        let response = server.get_hover(uri, line, character);
        println!("EXACT METHOD HOVER RESPONSE: {response:#}");

        let content =
            hover_content(&response).ok_or("expected hover content for the exact method call")?;

        assert!(
            !content.contains("dynamic dispatch"),
            "an exactly-resolved method must not be marked dynamic, got: {content}"
        );
        assert!(
            !content.contains("resolved at runtime"),
            "an exactly-resolved method must not claim runtime resolution, got: {content}"
        );
        Ok(())
    }

    /// A class with no AUTOLOAD must not gain the dynamic marker at all.
    #[test]
    fn class_without_autoload_is_never_marked_dynamic() -> Result<(), Box<dyn std::error::Error>> {
        // Uses a resolvable `$self` receiver so the method-hover path is
        // actually entered; an opaque receiver would make this vacuous.
        let code = r#"package Plain;

sub new { my $c = shift; return bless {}, $c; }

sub only_method { return 1; }

sub caller_method {
    my $self = shift;
    $self->only_method();
    return;
}
"#;
        let uri = "file:///no_autoload.pl";
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);

        let (line, character) = find_pos(code, "only_method", 8)?;
        let response = server.get_hover(uri, line, character);
        println!("NO-AUTOLOAD HOVER RESPONSE: {response:#}");

        // Require content rather than tolerating None: a hover that resolved
        // nothing would satisfy the negative assertion vacuously, so a
        // regression that broke plain-class hover entirely would stay green.
        let content =
            hover_content(&response).ok_or("expected hover content for the plain-class method")?;

        assert!(
            content.contains("only_method"),
            "hover must resolve the plain-class method, got: {content}"
        );
        assert!(
            !content.contains("dynamic dispatch"),
            "a class without AUTOLOAD must never be marked dynamic, got: {content}"
        );
        Ok(())
    }

    /// The cross-file (phase-2 workspace-BFS) hover path builds its card
    /// independently of `HoverInfo`, so it needs its own proof: an AUTOLOAD
    /// handler in another file must be marked dynamic too. Without this, the
    /// claim would silently hold only for single-file classes.
    #[test]
    fn cross_file_autoload_hover_is_marked_dynamic() -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let lib_dir = workspace.join("lib");
        fs::create_dir_all(&lib_dir)?;
        fs::write(
            lib_dir.join("Remote.pm"),
            "package Remote;\n\nsub new { my $c = shift; return bless {}, $c; }\n\n\
             sub AUTOLOAD {\n    our $AUTOLOAD;\n    return 0;\n}\n\n1;\n",
        )?;

        let script = workspace.join("script.pl");
        let code = "use lib 'lib';\nuse Remote;\n\nRemote->undefined_method();\n";
        fs::write(&script, code)?;

        let workspace_path = workspace.to_str().ok_or("non-UTF-8 workspace path")?;
        let script_uri =
            url::Url::from_file_path(&script).map_err(|_| "invalid script file path")?.to_string();

        let server = TestServerBuilder::new().with_workspace(workspace_path).build();
        server.open_document(&script_uri, code);

        let (line, character) = find_pos(code, "undefined_method", 3)?;
        let response = server.get_hover(&script_uri, line, character);
        println!("CROSS-FILE AUTOLOAD HOVER RESPONSE: {response:#}");

        let content =
            hover_content(&response).ok_or("expected hover content for cross-file AUTOLOAD")?;

        assert!(
            content.contains("dynamic dispatch"),
            "cross-file AUTOLOAD hover must mark the dispatch as dynamic, got: {content}"
        );
        assert!(
            content.contains("resolved at runtime"),
            "cross-file AUTOLOAD hover must explain runtime resolution, got: {content}"
        );
        Ok(())
    }

    /// Ordering control for the cross-file path: Perl searches the whole
    /// resolution order for an exact method before consulting AUTOLOAD, so a
    /// subclass AUTOLOAD must not pre-empt an ancestor's real method. Resolving
    /// exact-then-AUTOLOAD per package would report an exact, source-backed
    /// inherited call as a dynamic boundary.
    #[test]
    fn cross_file_ancestor_exact_method_beats_subclass_autoload()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let lib_dir = workspace.join("lib");
        fs::create_dir_all(&lib_dir)?;
        fs::write(
            lib_dir.join("RemoteBase.pm"),
            "package RemoteBase;\n\nsub inherited_method { return 1; }\n\n1;\n",
        )?;
        fs::write(
            lib_dir.join("RemoteChild.pm"),
            "package RemoteChild;\n\nour @ISA = ('RemoteBase');\n\n\
             sub AUTOLOAD {\n    our $AUTOLOAD;\n    return 0;\n}\n\n1;\n",
        )?;

        let script = workspace.join("script.pl");
        let code = "use lib 'lib';\nuse RemoteChild;\n\nRemoteChild->inherited_method();\n";
        fs::write(&script, code)?;

        let workspace_path = workspace.to_str().ok_or("non-UTF-8 workspace path")?;
        let script_uri =
            url::Url::from_file_path(&script).map_err(|_| "invalid script file path")?.to_string();

        let server = TestServerBuilder::new().with_workspace(workspace_path).build();
        server.open_document(&script_uri, code);

        let (line, character) = find_pos(code, "inherited_method", 3)?;
        let response = server.get_hover(&script_uri, line, character);
        println!("CROSS-FILE INHERITED EXACT HOVER RESPONSE: {response:#}");

        // Require content rather than tolerating None: a hover that resolved
        // nothing would satisfy the negative assertions vacuously.
        let content = hover_content(&response)
            .ok_or("expected hover content for the inherited exact method")?;

        assert!(
            content.contains("RemoteBase::inherited_method"),
            "hover must resolve through the ancestor to the exact method, got: {content}"
        );
        assert!(
            !content.contains("dynamic dispatch"),
            "an exact inherited method must not be marked dynamic, got: {content}"
        );
        assert!(
            !content.contains("resolved at runtime"),
            "an exact inherited method must not claim runtime resolution, got: {content}"
        );
        Ok(())
    }
}
