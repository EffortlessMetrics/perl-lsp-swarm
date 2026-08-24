//! Focused go-to-definition tests for Dancer and Dancer2 route targets.

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(test)]
mod dancer_navigation_tests {
    use super::TestResult;
    use crate::common::test_utils::{TestServerBuilder, semantic};

    fn goto_def(
        code: &str,
        uri: &str,
        needle: &str,
        target_line: usize,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);
        let (line, character) = semantic::find_pos(code, needle, target_line);
        Ok(server.get_definition(uri, line, character))
    }

    #[test]
    fn dancer_route_target_definitions_to_named_sub() -> TestResult {
        let code =
            "use Dancer;\nget '/about' => 'show_about';\nsub show_about { return 'About'; }\n";
        let uri = "file:///dancer_route_target.pl";

        let resp = goto_def(code, uri, "show_about", 1)?;
        let (def_uri, def_line, _) = semantic::first_location(&resp)
            .ok_or("Expected goto-definition to resolve the Dancer route target")?;

        assert_eq!(def_uri, uri, "Definition should stay in the same file");
        assert_eq!(def_line, 2, "Definition should point to the named sub handler");
        Ok(())
    }

    // The former `dancer2_route_target_definitions_to_named_sub` string-handler test
    // was removed per #8910: Dancer2 string targets are not exact subroutine
    // references. The analyzer-level containment contract is proven in
    // `perl-semantic-analyzer/tests/frameworks_web.rs`
    // (`dancer2_route_target_string_does_not_add_subroutine_reference`); the
    // navigation-level controls are the inline-handler containment test and the
    // activation-removal staleness test below (a same-file word-name
    // goto-definition fallback is generic Perl behavior, not a route fact).

    // Valid inline CodeRef handler stays navigable under exact Dancer2 activation.
    #[test]
    fn dancer2_inline_handler_body_resolves_named_subs() -> TestResult {
        let code = "use Dancer2;\nsub helper { return 1 }\nget '/status' => sub { helper() };\n";
        let uri = "file:///dancer2_inline_handler.pl";

        let resp = goto_def(code, uri, "helper", 2)?;
        let (def_uri, def_line, _) = semantic::first_location(&resp)
            .ok_or("Expected goto-definition to resolve `helper` from the inline handler")?;

        assert_eq!(def_uri, uri, "Definition should stay in the same file");
        assert_eq!(def_line, 1, "Definition should point to `sub helper`");
        Ok(())
    }

    // Removing the activation import must not leave stale exact route behavior.
    #[test]
    fn dancer2_activation_removal_drops_route_navigation() -> TestResult {
        let code = "use Dancer2;\nget '/status' => sub { 'ok' };\n";
        let uri = "file:///dancer2_staleness.pl";

        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);
        let (line, character) = semantic::find_pos(code, "/status", 1);
        let before = server.get_definition(uri, line, character);
        assert!(
            semantic::first_location(&before).is_some(),
            "route symbol should be navigable while `use Dancer2` is present"
        );

        let changed = "get '/status' => sub { 'ok' };\n";
        server.change_document(uri, changed, 2);
        let (line, character) = semantic::find_pos(changed, "/status", 0);
        let after = server.get_definition(uri, line, character);
        assert!(
            semantic::first_location(&after).is_none(),
            "removing `use Dancer2` must drop the stale route navigation"
        );
        server.shutdown();
        Ok(())
    }
}
