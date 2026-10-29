//! Focused go-to-definition tests for Dancer and Dancer2 route targets.

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(test)]
mod dancer_navigation_tests {
    use super::TestResult;
    use crate::common::test_utils::{TestServer, TestServerBuilder, semantic};

    fn document_barrier(server: &TestServer, uri: &str) -> TestResult {
        let response = server.get_symbols(uri);
        if response.get("error").is_some() || response.get("result").is_none() {
            return Err(format!("document request barrier failed at {uri}: {response}").into());
        }
        Ok(())
    }

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
        document_barrier(&server, uri)?;
        let (line, character) = semantic::find_pos(code, needle, target_line);
        let response = server.get_definition(uri, line, character);
        let locations = definition_locations(&response, uri, line, character)?;
        Ok((response, locations))
    }

    // Preserve the recorded Dancer1 static reference even when its program would
    // fail at dispatch. In the Dancer 1.3522 / Perl 5.40.1 minimal GET proof,
    // quoted handlers compile and register but dispatch 500 under strict refs;
    // the matching CodeRef dispatches 200 with "About". Navigation does not run
    // the application or claim that its callback succeeds.
    #[test]
    fn dancer_route_target_definitions_to_named_sub() -> TestResult {
        let code =
            "use Dancer;\nget '/about' => 'show_about';\nsub show_about { return 'About'; }\n";
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
    fn dancer_named_handler_references_require_exact_semantic_evidence() -> TestResult {
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
        // Positive controls also prevent empty/error navigation from satisfying
        // the plain-string and missing-handler negative controls.
        for (line, character, expected_line, label) in [
            (2, 0, Some(6), "ordinary call"),
            (1, 18, Some(6), "named CodeRef"),
            (3, 17, Some(6), "recorded Dancer1 quoted handler"),
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
                assert_eq!(locations[0].range.start.character, 4, "{label}: response={response}");
                assert_eq!(
                    locations[0].range.end.line, expected_line,
                    "{label}: response={response}"
                );
                assert_eq!(locations[0].range.end.character, 14, "{label}: response={response}");
            } else {
                assert!(locations.is_empty(), "{label}: response={response}");
            }
        }
        // Changing the import revokes and restores the existing reference fact;
        // ordinary Perl references stay live in the same process throughout.
        for (version, changed, expected) in [
            (2, code.replacen("use Dancer;", "           ", 1), false),
            (3, code.to_string(), true),
            (4, code.replacen("use Dancer;", "use Dancer2;", 1), false),
        ] {
            server.change_document(uri, &changed, version);
            document_barrier(&server, uri)?;
            for (line, character, resolves, label) in [
                (2, 0, true, "ordinary call after import change"),
                (1, 18, true, "CodeRef after import change"),
                (3, 17, expected, "quoted handler after import change"),
            ] {
                let response = server.get_definition(uri, line, character);
                let locations = definition_locations(&response, uri, line, character)?;
                if resolves {
                    assert_eq!(locations.len(), 1, "{label}, version={version}: {response}");
                    assert_eq!(locations[0].uri.as_str(), uri, "{label}: {response}");
                    assert_eq!(
                        locations[0].range,
                        lsp_types::Range::new(
                            lsp_types::Position::new(6, 4),
                            lsp_types::Position::new(6, 14),
                        ),
                        "{label}: {response}"
                    );
                } else {
                    assert!(locations.is_empty(), "{label}, version={version}: {response}");
                }
            }
        }
        server.shutdown();
        Ok(())
    }

    #[test]
    fn named_coderef_main_shorthand_preserves_definition_navigation() -> TestResult {
        let code =
            "sub handler { 1 }\nmy $r = \\&::handler;\n&::handler();\nmy $miss = \\&::missing;\n";
        let uri = "file:///dancer_main_shorthand_coderef.pl";
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);
        document_barrier(&server, uri)?;
        for (line, character) in [(1, 14), (2, 4)] {
            let response = server.get_definition(uri, line, character);
            let locations = definition_locations(&response, uri, line, character)?;
            assert_eq!(locations.len(), 1, "main shorthand: {response}");
            assert_eq!(locations[0].uri.as_str(), uri, "main shorthand: {response}");
            assert_eq!(
                locations[0].range,
                lsp_types::Range::new(
                    lsp_types::Position::new(0, 0),
                    lsp_types::Position::new(0, 17),
                ),
                "main shorthand must retain its real subroutine declaration: {response}"
            );
        }
        let response = server.get_definition(uri, 3, 17);
        let locations = definition_locations(&response, uri, 3, 17)?;
        assert!(locations.is_empty(), "missing main shorthand: {response}");
        server.shutdown();
        Ok(())
    }

    #[test]
    fn dancer_handler_misses_do_not_resolve_containers_or_other_scopes() -> TestResult {
        let server = TestServerBuilder::new().build();
        let uri = "file:///dancer_missing_handler_controls.pl";
        let cases = [
            (
                "use Dancer;\nsub enclosing {\nget '/x' => 'missing_handler';\nget '/y' => \\&missing_handler;\nmy $label = 'show_about';\n}\nsub show_about { 1 }\n",
                vec![(2, 13), (3, 14), (4, 14)],
            ),
            ("use Dancer;\nget '/x' => 'hidden';\n{ my sub hidden { 1 } }\n", vec![(1, 13)]),
            (
                "use Dancer;\nget '/x' => 'Missing::show_about';\nsub show_about { 1 }\n",
                vec![(1, 22)],
            ),
        ];
        server.open_document(uri, cases[0].0);
        document_barrier(&server, uri)?;
        for (index, (code, positions)) in cases.into_iter().enumerate() {
            if index > 0 {
                server.change_document(uri, code, index as i32 + 1);
                document_barrier(&server, uri)?;
            }
            for (line, character) in positions {
                let response = server.get_definition(uri, line, character);
                let locations = definition_locations(&response, uri, line, character)?;
                assert!(locations.is_empty(), "case={index} at {line}:{character}: {response}");
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
    // import-removal absence control below (a same-file word-name
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

    // Without a versioned workspace fixture these are absence controls; they do
    // not claim revocation of a previously active Dancer2 route (see scenario69).
    #[test]
    fn dancer2_unproved_activation_stays_absent_after_import_removal() -> TestResult {
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
        document_barrier(&server, uri)?;
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
