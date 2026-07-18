//! Tests for Moose/Moo class model integration with goto-definition and hover.
//!
//! Covers issue #2831: wire Moose/Moo class model to navigation providers.
//!
//! The SymbolExtractor synthesizes accessor symbols at the `has` declaration
//! location with documentation like "Moo/Moose accessor (isa: Str, ro)". This test
//! suite validates those end-to-end LSP behaviours.
//!
//! Additionally, hover on `$self->accessor_name` now renders a dedicated
//! "Moo/Moose Attribute Accessor" card instead of the misleading "Subroutine"
//! label, implemented via the `declaration == "has"` early-return in hover.rs.

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(test)]
mod moose_class_model_navigation_tests {
    use super::TestResult;
    use crate::common::test_utils::{TestServerBuilder, semantic};

    // ── helpers ──────────────────────────────────────────────────────────

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

    fn hover_at(
        code: &str,
        uri: &str,
        needle: &str,
        target_line: usize,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);
        let (line, character) = semantic::find_pos(code, needle, target_line);
        Ok(server.get_hover(uri, line, character))
    }

    // ── goto-definition: $self->accessor jumps to has declaration ──────

    /// Goto-definition on `$self->name` in a Moose class must return the
    /// source location of the `has 'name'` declaration, not an empty result.
    #[test]
    fn test_moose_accessor_goto_definition_jumps_to_has() -> TestResult {
        // Line 0: package Animal;
        // Line 1: use Moose;
        // Line 2: has 'name' => (is => 'ro', isa => 'Str');
        // Line 3: sub speak { my ($self) = @_; return $self->name; }
        let code = "package Animal;\nuse Moose;\nhas 'name' => (is => 'ro', isa => 'Str');\nsub speak { my ($self) = @_; return $self->name; }\n";
        let uri = "file:///animal.pl";

        let resp = goto_def(code, uri, "name", 3)?;

        let (def_uri, def_line, _) = semantic::first_location(&resp).ok_or(
            "Expected goto-definition to find the 'has' declaration for Moose accessor 'name'",
        )?;

        assert_eq!(def_uri, uri, "Definition should be in the same file");
        assert_eq!(
            def_line, 2,
            "Definition should point to line 2 (the 'has' declaration), got line {def_line}"
        );
        Ok(())
    }

    /// Goto-definition on `$self->age` with Moo must jump to `has 'age'`.
    #[test]
    fn test_moo_accessor_goto_definition_jumps_to_has() -> TestResult {
        // Line 0: package Person;
        // Line 1: use Moo;
        // Line 2: has 'age' => (is => 'rw', isa => 'Int');
        // Line 3: sub birthday { my ($self) = @_; $self->age($self->age + 1); }
        let code = "package Person;\nuse Moo;\nhas 'age' => (is => 'rw', isa => 'Int');\nsub birthday { my ($self) = @_; $self->age($self->age + 1); }\n";
        let uri = "file:///person.pl";

        let resp = goto_def(code, uri, "age", 3)?;

        let (def_uri, def_line, _) = semantic::first_location(&resp).ok_or(
            "Expected goto-definition to find the 'has' declaration for Moo accessor 'age'",
        )?;

        assert_eq!(def_uri, uri, "Definition should be in the same file");
        assert_eq!(
            def_line, 2,
            "Definition should point to line 2 (the 'has' declaration), got line {def_line}"
        );
        Ok(())
    }

    /// Goto-definition on `$self->color` with Mouse must jump to `has 'color'`.
    #[test]
    fn test_mouse_accessor_goto_definition_jumps_to_has() -> TestResult {
        let code = "package Car;\nuse Mouse;\nhas 'color' => (is => 'ro', isa => 'Str');\nsub describe { my ($self) = @_; return $self->color; }\n";
        let uri = "file:///car.pl";

        let resp = goto_def(code, uri, "color", 3)?;

        let (def_uri, def_line, _) = semantic::first_location(&resp)
            .ok_or("Expected goto-definition for Mouse accessor 'color'")?;

        assert_eq!(def_uri, uri);
        assert_eq!(def_line, 2, "Should point to 'has' declaration on line 2");
        Ok(())
    }

    /// Multiple attributes: goto-definition on `$self->email` must jump to its
    /// specific `has 'email'` declaration, not the other attributes.
    #[test]
    fn test_moose_multiple_attrs_each_goto_own_has() -> TestResult {
        // Line 0: package Contact;
        // Line 1: use Moose;
        // Line 2: has 'name' => (is => 'ro', isa => 'Str');
        // Line 3: has 'email' => (is => 'rw', isa => 'Str');
        // Line 4: sub info { my ($self) = @_; return $self->email; }
        let code = "package Contact;\nuse Moose;\nhas 'name' => (is => 'ro', isa => 'Str');\nhas 'email' => (is => 'rw', isa => 'Str');\nsub info { my ($self) = @_; return $self->email; }\n";
        let uri = "file:///contact.pl";

        let resp = goto_def(code, uri, "email", 4)?;

        let (def_uri, def_line, _) = semantic::first_location(&resp)
            .ok_or("Expected goto-definition to find the 'has email' declaration")?;

        assert_eq!(def_uri, uri);
        assert_eq!(
            def_line, 3,
            "Definition should point to line 3 (the 'has email' declaration), got line {def_line}"
        );
        Ok(())
    }

    /// Goto-definition on `$self->method` that IS a real sub (not a has attribute)
    /// must still work normally — accessor lookup must not break regular methods.
    #[test]
    fn test_moose_real_method_definition_not_broken() -> TestResult {
        let code = "package Dog;\nuse Moose;\nhas 'name' => (is => 'ro', isa => 'Str');\nsub bark { return 'woof'; }\nsub speak { my ($self) = @_; $self->bark; }\n";
        let uri = "file:///dog.pl";

        let resp = goto_def(code, uri, "bark", 4)?;

        // The result must be non-empty (some location found)
        let result_array = resp["result"].as_array();
        assert!(
            result_array.is_some_and(|arr| !arr.is_empty()),
            "Goto-definition on a real sub should still return a location, got: {resp:#}"
        );
        Ok(())
    }

    /// Goto-definition on method modifier targets should jump to the underlying method.
    #[test]
    fn test_moo_method_modifier_goto_definition_jumps_to_target_method() -> TestResult {
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifiers.pl";

        for target_line in [8_usize, 13, 18] {
            let resp = goto_def(code, uri, "save", target_line)?;
            let (def_uri, def_line, _) = semantic::first_location(&resp).ok_or(
                "Expected goto-definition to find the underlying method for a method modifier",
            )?;

            assert_eq!(def_uri, uri, "Definition should be in the same file");
            assert_eq!(
                def_line, 3,
                "Method modifier on line {target_line} should jump to the underlying method declaration on line 3"
            );
        }
        Ok(())
    }

    // ── hover: $self->accessor shows dedicated attribute accessor card ────

    /// Hovering `$self->name` on a Moose `ro` attribute must show a dedicated
    /// "Moo/Moose Attribute Accessor" card with isa type and accessor mode.
    #[test]
    fn test_moose_accessor_hover_shows_isa_and_mode() -> TestResult {
        let code = "package Animal;\nuse Moose;\nhas 'name' => (is => 'ro', isa => 'Str');\nsub speak { my ($self) = @_; return $self->name; }\n";
        let uri = "file:///animal_hover.pl";

        let resp = hover_at(code, uri, "name", 3)?;

        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for Moose accessor 'name'")?;

        // The hover card must say "Moo/Moose Attribute Accessor" and include type info
        assert!(
            content.contains("Moo/Moose Attribute Accessor"),
            "Hover for Moose accessor should show 'Moo/Moose Attribute Accessor', got: {content}"
        );
        assert!(
            content.contains("Str") || content.contains("ro"),
            "Hover for Moose accessor should show isa type or accessor mode, got: {content}"
        );
        Ok(())
    }

    /// Hovering `$self->age` on a Moo `rw` attribute must show the attribute accessor card.
    #[test]
    fn test_moo_rw_accessor_hover_shows_mode() -> TestResult {
        let code = "package Person;\nuse Moo;\nhas 'age' => (is => 'rw', isa => 'Int');\nsub birthday { my ($self) = @_; $self->age($self->age + 1); }\n";
        let uri = "file:///person_hover.pl";

        let resp = hover_at(code, uri, "age", 3)?;

        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for Moo accessor 'age'")?;

        assert!(
            content.contains("Moo/Moose Attribute Accessor"),
            "Hover for Moo accessor should show 'Moo/Moose Attribute Accessor', got: {content}"
        );
        assert!(
            content.contains("Int") || content.contains("rw"),
            "Hover for Moo accessor should show isa or mode, got: {content}"
        );
        Ok(())
    }

    /// Hovering on `$self->name` where `name` is a Moose attribute must NOT show
    /// "Subroutine" — it must specifically identify this as an attribute accessor.
    #[test]
    fn test_moose_accessor_hover_does_not_say_subroutine() -> TestResult {
        let code = "package Widget;\nuse Moose;\nhas 'label' => (is => 'ro', isa => 'Str', required => 1);\nsub render { my ($self) = @_; print $self->label; }\n";
        let uri = "file:///widget.pl";

        let resp = hover_at(code, uri, "label", 3)?;

        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for Moose accessor 'label'")?;

        // Must NOT say "Subroutine" for an attribute accessor
        assert!(
            !content.contains("**Subroutine**"),
            "Hover for Moose accessor should NOT show '**Subroutine**', got: {content}"
        );
        // Must identify as Moose/Moo accessor
        assert!(
            content.contains("Moo/Moose Attribute Accessor"),
            "Hover content should say 'Moo/Moose Attribute Accessor', got: {content}"
        );
        Ok(())
    }

    /// The hover documentation for a Moose accessor must include the isa type
    /// when the `has` declaration specifies one.
    #[test]
    fn test_moose_accessor_hover_contains_isa_type_in_doc() -> TestResult {
        let code = "package Order;\nuse Moose;\nhas 'total' => (is => 'rw', isa => 'Num');\nsub show { my ($self) = @_; print $self->total; }\n";
        let uri = "file:///order.pl";

        let resp = hover_at(code, uri, "total", 3)?;

        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for Moose accessor 'total'")?;

        // The accessor doc format is "Moo/Moose accessor (isa: Num, rw)"
        assert!(
            content.contains("Num"),
            "Hover content for 'total' should contain isa type 'Num', got: {content}"
        );
        Ok(())
    }

    /// Hovering an accessor with Moo/Moose metadata must surface the key fields
    /// from the attribute model in the hover markdown.
    #[test]
    fn test_moose_accessor_hover_surfaces_full_attribute_metadata() -> TestResult {
        let code = "package Widget;\nuse Moose;\nhas 'status' => (is => 'rw', isa => 'Str', required => 1, predicate => 1, builder => 1, clearer => 1);\nsub render { my ($self) = @_; print $self->status; }\n";
        let uri = "file:///widget_metadata.pl";

        let resp = hover_at(code, uri, "status", 3)?;
        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for Moose accessor 'status'")?;

        assert!(
            content.contains("Moo/Moose Attribute Accessor"),
            "Hover for Moose accessor should show the dedicated accessor card, got: {content}"
        );
        for expected in [
            "**Attribute**: `status`",
            "**Type**: `Str`",
            "**Access**: read-write",
            "**Required**: yes",
            "**Predicate**: `has_status`",
            "**Builder**: `_build_status`",
            "**Clearer**: `clear_status`",
        ] {
            assert!(
                content.contains(expected),
                "Hover for Moose accessor should include `{expected}`, got: {content}"
            );
        }
        Ok(())
    }

    /// Hovering on a bare method call (no Moose) must NOT break — must still
    /// return some content as before.
    #[test]
    fn test_non_moose_hover_not_broken() -> TestResult {
        let code = "package Greeter;\nsub new { bless {}, shift }\nsub greet { return 'hello'; }\nsub run { my ($self) = @_; $self->greet; }\n";
        let uri = "file:///greeter.pl";

        let resp = hover_at(code, uri, "greet", 3)?;

        // Either some hover content exists, or we get null — both are acceptable
        // as long as we don't get an error response
        assert!(
            resp.get("error").is_none(),
            "Hover on non-Moose $self->method should not return an error, got: {resp:#}"
        );
        Ok(())
    }

    // ── hover: method modifier declarations show modifier card ───────────
    //
    // Issue #1728: hover tests for before/after/around/override/augment.
    // The `method_modifier_hover` card is emitted when the cursor falls within a
    // modifier declaration's AST span — the symbol extractor creates a synthetic
    // subroutine symbol with `attributes = ["modifier=<kind>"]` for each modifier
    // statement, so `symbol_at(offset)` finds it and triggers the dedicated card.

    /// Hovering on the target name in `before 'save' => sub { }` must show the
    /// "Method Modifier (`before`)" card, not a generic "Subroutine" label.
    #[test]
    fn test_moo_before_modifier_hover_shows_before_modifier_card() -> TestResult {
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifier_hover_before.pl";

        // Line 8 (0-indexed): `before 'save' => sub {`
        let resp = hover_at(code, uri, "save", 8)?;
        let content = semantic::hover_content(&resp).ok_or(
            "Expected hover content when cursor is on 'save' in 'before 'save'' declaration",
        )?;

        assert!(
            content.contains("before"),
            "Hover on 'before' modifier declaration should mention 'before', got: {content}"
        );
        assert!(
            content.contains("Method Modifier"),
            "Hover on modifier declaration should show 'Method Modifier' card header, got: {content}"
        );
        Ok(())
    }

    /// Hovering on the target name in `after 'save' => sub { }` must show the
    /// "Method Modifier (`after`)" card.
    #[test]
    fn test_moo_after_modifier_hover_shows_after_modifier_card() -> TestResult {
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifier_hover_after.pl";

        // Line 13 (0-indexed): `after 'save' => sub {`
        let resp = hover_at(code, uri, "save", 13)?;
        let content = semantic::hover_content(&resp).ok_or(
            "Expected hover content when cursor is on 'save' in 'after 'save'' declaration",
        )?;

        assert!(
            content.contains("after"),
            "Hover on 'after' modifier declaration should mention 'after', got: {content}"
        );
        assert!(
            content.contains("Method Modifier"),
            "Hover on modifier declaration should show 'Method Modifier' card header, got: {content}"
        );
        Ok(())
    }

    /// Hovering on the target name in `around 'save' => sub { }` must show the
    /// "Method Modifier (`around`)" card.
    #[test]
    fn test_moo_around_modifier_hover_shows_around_modifier_card() -> TestResult {
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifier_hover_around.pl";

        // Line 18 (0-indexed): `around 'save' => sub {`
        let resp = hover_at(code, uri, "save", 18)?;
        let content = semantic::hover_content(&resp).ok_or(
            "Expected hover content when cursor is on 'save' in 'around 'save'' declaration",
        )?;

        assert!(
            content.contains("around"),
            "Hover on 'around' modifier declaration should mention 'around', got: {content}"
        );
        assert!(
            content.contains("Method Modifier"),
            "Hover on modifier declaration should show 'Method Modifier' card header, got: {content}"
        );
        Ok(())
    }

    /// Hovering on `override 'save'` in a Moose class must show the
    /// "Method Modifier (`override`)" card with the override description.
    #[test]
    fn test_moose_override_modifier_hover_shows_override_modifier_card() -> TestResult {
        // Line 0: package Demo::Override;
        // Line 1: use Moose;
        // Line 2: sub save { return 1; }
        // Line 3: override 'save' => sub { };
        let code = "package Demo::Override;\nuse Moose;\nsub save { return 1; }\noverride 'save' => sub { };\n";
        let uri = "file:///moose_modifier_hover_override.pl";

        let resp = hover_at(code, uri, "save", 3)?;
        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for 'override' modifier declaration")?;

        assert!(
            content.contains("override"),
            "Hover on 'override' modifier should mention 'override', got: {content}"
        );
        assert!(
            content.contains("Method Modifier"),
            "Hover on 'override' modifier should show 'Method Modifier' card, got: {content}"
        );
        Ok(())
    }

    /// Hovering on `augment 'save'` in a Moose class must show the
    /// "Method Modifier (`augment`)" card with the augment description.
    #[test]
    fn test_moose_augment_modifier_hover_shows_augment_modifier_card() -> TestResult {
        // Line 0: package Demo::Augment;
        // Line 1: use Moose;
        // Line 2: sub save { inner() }
        // Line 3: augment 'save' => sub { return 1; };
        let code = "package Demo::Augment;\nuse Moose;\nsub save { inner() }\naugment 'save' => sub { return 1; };\n";
        let uri = "file:///moose_modifier_hover_augment.pl";

        let resp = hover_at(code, uri, "save", 3)?;
        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for 'augment' modifier declaration")?;

        assert!(
            content.contains("augment"),
            "Hover on 'augment' modifier should mention 'augment', got: {content}"
        );
        assert!(
            content.contains("Method Modifier"),
            "Hover on 'augment' modifier should show 'Method Modifier' card, got: {content}"
        );
        Ok(())
    }

    /// When a method has multiple modifiers, hovering over each modifier declaration
    /// must produce DISTINCT content identifying the specific modifier kind.
    ///
    /// This guards against the case where all modifiers produce the same generic card
    /// that does not distinguish "before" from "after" from "around".
    #[test]
    fn test_moo_multiple_modifiers_hover_each_distinct() -> TestResult {
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifier_hover_distinct.pl";
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);

        let (bl, bc) = semantic::find_pos(code, "save", 8); // before
        let before_resp = server.get_hover(uri, bl, bc);
        let before_content = semantic::hover_content(&before_resp)
            .ok_or("Expected hover content for 'before' modifier")?;

        let (al, ac) = semantic::find_pos(code, "save", 13); // after
        let after_resp = server.get_hover(uri, al, ac);
        let after_content = semantic::hover_content(&after_resp)
            .ok_or("Expected hover content for 'after' modifier")?;

        let (rl, rc) = semantic::find_pos(code, "save", 18); // around
        let around_resp = server.get_hover(uri, rl, rc);
        let around_content = semantic::hover_content(&around_resp)
            .ok_or("Expected hover content for 'around' modifier")?;

        // Each card must name its own modifier kind
        assert!(
            before_content.to_lowercase().contains("before"),
            "before modifier hover must mention 'before', got: {before_content}"
        );
        assert!(
            after_content.to_lowercase().contains("after"),
            "after modifier hover must mention 'after', got: {after_content}"
        );
        assert!(
            around_content.to_lowercase().contains("around"),
            "around modifier hover must mention 'around', got: {around_content}"
        );

        // The three cards must not be identical (before ≠ after, before ≠ around)
        assert_ne!(
            before_content, after_content,
            "before and after modifier hover cards should produce distinct content"
        );
        assert_ne!(
            before_content, around_content,
            "before and around modifier hover cards should produce distinct content"
        );
        Ok(())
    }

    /// Hovering on a modifier declaration must NOT show "Subroutine" as the kind label —
    /// the dedicated "Method Modifier" card must replace the generic subroutine card.
    #[test]
    fn test_moo_modifier_hover_does_not_show_subroutine_label() -> TestResult {
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifier_hover_no_sub.pl";

        // Check all three modifier declarations
        for (needle, target_line) in [("save", 8), ("save", 13), ("save", 18)] {
            let resp = hover_at(code, uri, needle, target_line)?;
            let content = semantic::hover_content(&resp).ok_or_else(|| {
                format!("Expected hover content for modifier declaration on line {target_line}")
            })?;
            assert!(
                !content.contains("**Subroutine**"),
                "Modifier declaration hover on line {target_line} should not say '**Subroutine**', got: {content}"
            );
        }
        Ok(())
    }

    /// Hovering on a plain method (no modifiers) must NOT show "Method Modifier" —
    /// modifier detection must be scoped to actual modifier declarations.
    #[test]
    fn test_moo_plain_method_hover_not_labelled_as_modifier() -> TestResult {
        // The fixture has `plain_method` with no decorators.
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifier_hover_plain.pl";

        // Line 28 (0-indexed): `sub plain_method {`
        let resp = hover_at(code, uri, "plain_method", 28)?;

        let content = semantic::hover_content(&resp)
            .ok_or("Expected hover content for plain method declaration")?;
        assert!(
            !content.contains("Method Modifier"),
            "Plain method hover should not say 'Method Modifier', got: {content}"
        );
        Ok(())
    }

    /// Hovering on a call site `$self->save` must not crash or return an error
    /// response, even when `save` is decorated with multiple method modifiers.
    ///
    /// NOTE: The current implementation does not surface modifier information at
    /// call sites — that is a separate enhancement. This test guards robustness only.
    #[test]
    fn test_moo_modifier_decorated_method_call_site_hover_does_not_crash() -> TestResult {
        let code = include_str!("fixtures/frameworks/moo_method_modifiers.pl");
        let uri = "file:///moo_modifier_hover_callsite.pl";

        // Line 25 (0-indexed): `    $self->save;  # call site`
        let server = TestServerBuilder::new().build();
        server.open_document(uri, code);
        let (line, character) = semantic::find_pos(code, "save", 25);
        let resp = server.get_hover(uri, line, character);

        assert!(
            resp.get("error").is_none(),
            "Hover on $self->save call site should not return an error, got: {resp:#}"
        );
        assert!(
            resp.get("result").is_some(),
            "Hover on $self->save call site should return an LSP result field, got: {resp:#}"
        );
        Ok(())
    }

    // ── hover on the has declaration itself ──────────────────────────────

    /// Hovering directly on the `has 'name'` declaration line must show
    /// attribute documentation.
    #[test]
    fn test_moose_hover_on_has_declaration_itself() -> TestResult {
        let code = "package Animal;\nuse Moose;\nhas 'name' => (is => 'ro', isa => 'Str');\nsub speak { return 'roar'; }\n";
        let uri = "file:///animal_has_hover.pl";

        // Hover on "name" in the `has 'name'` declaration (line 2)
        let resp = hover_at(code, uri, "name", 2)?;

        // Must not error
        assert!(
            resp.get("error").is_none(),
            "Hover on 'has' declaration should not error, got: {resp:#}"
        );

        // If hover content exists, it should mention attribute info
        if let Some(content) = semantic::hover_content(&resp) {
            assert!(
                content.to_lowercase().contains("attribute")
                    || content.contains("Moo")
                    || content.contains("Str")
                    || content.contains("ro"),
                "Hover on 'has' declaration should show attribute info, got: {content}"
            );
        }
        Ok(())
    }
}
