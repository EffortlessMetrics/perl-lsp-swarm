//! Focused go-to-definition tests for Dancer and Dancer2 route targets.

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(test)]
mod dancer_navigation_tests {
    use super::TestResult;
    use crate::common::test_utils::{TestServerBuilder, semantic};

    fn definition_locations(
        response: &serde_json::Value,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<lsp_types::Location>, Box<dyn std::error::Error>> {
        let context = || format!("definition at {uri}:{line}:{character}; response={response}");
        if response.get("error").is_some() {
            return Err(format!("definition RPC failed; {}", context()).into());
        }
        let result = response
            .get("result")
            .ok_or_else(|| format!("definition RPC omitted its result; {}", context()))?;
        let definition: Option<lsp_types::GotoDefinitionResponse> =
            serde_json::from_value(result.clone())
                .map_err(|error| format!("invalid definition result ({error}); {}", context()))?;
        match definition {
            None => Ok(Vec::new()),
            Some(lsp_types::GotoDefinitionResponse::Scalar(location)) => Ok(vec![location]),
            Some(lsp_types::GotoDefinitionResponse::Array(locations)) => Ok(locations),
            Some(lsp_types::GotoDefinitionResponse::Link(_)) => Err(format!(
                "definition links require client linkSupport, which this fixture omits; {}",
                context()
            )
            .into()),
        }
    }

    fn goto_def(
        code: &str,
        uri: &str,
        needle: &str,
        target_line: usize,
    ) -> Result<(serde_json::Value, Vec<lsp_types::Location>), Box<dyn std::error::Error>> {
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);
        let (line, character) = semantic::find_pos(code, needle, target_line);
        let response = server.get_definition(uri, line, character);
        let locations = definition_locations(&response, uri, line, character)?;
        Ok((response, locations))
    }

    #[test]
    fn dancer_route_target_definitions_to_named_sub() -> TestResult {
        let code =
            "use Dancer;\nget '/about' => \\&show_about;\nsub show_about { return 'About'; }\n";
        let uri = "file:///dancer_route_target.pl";

        let (resp, locations) = goto_def(code, uri, "show_about", 1)?;
        let location = locations.first().ok_or_else(|| {
            format!("Expected goto-definition to resolve the Dancer route target; response={resp}")
        })?;

        assert_eq!(location.uri.as_str(), uri, "Definition should stay in the same file");
        assert_eq!(
            location.range.start.line, 2,
            "Definition should point to the named sub handler; response={resp}"
        );
        Ok(())
    }

    #[test]
    fn dancer_named_handler_references_do_not_promote_string_spellings() -> TestResult {
        let code = concat!(
            "use Dancer;\n",
            "get '/about' => \\&show_about;\n",
            "show_about();\n",
            "get '/other' => 'show_about';\n",
            "my $label = 'show_about';\n",
            "get '/empty' => \\&missing_handler;\n",
            "sub show_about { return 'About'; }\n",
        );
        let uri = "file:///dancer_handler_controls.pl";
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);

        // Literal positions and declaration line are independent of the provider.
        // The two positives also prevent empty/error navigation from satisfying
        // the string and missing-handler controls.
        for (line, character, expected_line, label) in [
            (2, 0, Some(6), "ordinary call"),
            (1, 18, Some(6), "named CodeRef"),
            (3, 17, None, "quoted route spelling"),
            (4, 14, None, "ordinary quoted spelling"),
            (5, 18, None, "missing named CodeRef"),
        ] {
            let response = server.get_definition(uri, line, character);
            let locations = definition_locations(&response, uri, line, character)?;
            if let Some(expected_line) = expected_line {
                assert_eq!(locations.len(), 1, "{label}: response={response}");
                assert_eq!(locations[0].uri.as_str(), uri, "{label}: response={response}");
                assert_eq!(
                    locations[0].range.start.line, expected_line,
                    "{label}: response={response}"
                );
            } else {
                assert!(locations.is_empty(), "{label}: response={response}");
            }
        }
        server.shutdown();
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

        let (resp, locations) = goto_def(code, uri, "helper", 2)?;
        let location = locations.first().ok_or_else(|| {
            format!(
                "Expected goto-definition to resolve `helper` from the inline handler; response={resp}"
            )
        })?;

        assert_eq!(location.uri.as_str(), uri, "Definition should stay in the same file");
        assert_eq!(
            location.range.start.line, 1,
            "Definition should point to `sub helper`; response={resp}"
        );

        // #8928: the legacy route-path Subroutine synthesis is retired for
        // admitted forms. Route navigation now comes from the canonical
        // facts and requires exact activation evidence (a resolved Dancer2
        // module with version evidence); without it there is zero framework
        // navigation. The positive canonical path is proven over the
        // skeleton fixture in perl-lsp-ux-tests
        // (ux_scenario_69_dancer2_provider_cutover). In this unit server no
        // Dancer2 module is resolvable, so the route pattern must NOT
        // produce a framework navigation target.
        let (route_resp, route_locations) = goto_def(code, uri, "/status", 2)?;
        assert!(
            route_locations.is_empty(),
            "no activation evidence: no framework route navigation (#8928); response={route_resp}"
        );
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
        let before_locations = definition_locations(&before, uri, line, character)?;
        // #8928: without a resolvable versioned Dancer2 module the
        // activation is not exact, so the framework route navigation is
        // absent even while `use Dancer2` is present (zero output without
        // #8914 activation evidence). The positive canonical path is proven
        // over the skeleton fixture in perl-lsp-ux-tests
        // (ux_scenario_69_dancer2_provider_cutover).
        assert!(
            before_locations.is_empty(),
            "no activation evidence: no framework route navigation while `use Dancer2` is present (#8928); response={before}"
        );

        // Discriminating control: neither state may produce a framework
        // navigation target for the route pattern (the positive canonical
        // path is scenario 69), while ordinary Perl navigation keeps working
        // after the removal so the absence is the framework contract, not a
        // dead server.
        let changed = "get '/status' => sub { 'ok' };
sub route_helper { 1 }
route_helper();
";
        server.change_document(uri, changed, 2);
        let (line, character) = semantic::find_pos(changed, "/status", 0);
        let after = server.get_definition(uri, line, character);
        let after_locations = definition_locations(&after, uri, line, character)?;
        assert!(
            after_locations.is_empty(),
            "removing `use Dancer2` must not leave any route navigation; response={after}"
        );
        let (helper_line, helper_char) = semantic::find_pos(changed, "route_helper();", 2);
        let helper_after = server.get_definition(uri, helper_line, helper_char);
        let helper_locations = definition_locations(&helper_after, uri, helper_line, helper_char)?;
        assert!(
            !helper_locations.is_empty(),
            "ordinary Perl navigation keeps working after the activation removal; response={helper_after}"
        );
        server.shutdown();
        Ok(())
    }
}
