//! Go-to-definition for DBIx::Class relationship accessors (issue #2978).
//!
//! Generated relationship members are source-anchored framework facts; navigation
//! must resolve `$self->accessor` to the relationship declaration anchor.

mod common;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(test)]
mod dbix_class_relationship_navigation_tests {
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

    /// `$self->orders` on a has_many accessor must jump to the relationship name
    /// in the `has_many('orders', ...)` declaration.
    #[test]
    fn test_dbix_has_many_accessor_goto_definition() -> TestResult {
        let code = "\
package MyApp::Schema::Result::User;
use DBIx::Class::Core;
__PACKAGE__->has_many('orders', 'MyApp::Schema::Result::Order', 'user_id');
sub related_orders {
    my ($self) = @_;
    return $self->orders;
}
1;
";
        let uri = "file:///dbix_user_result.pl";

        let resp = goto_def(code, uri, "orders", 5)?;

        let (def_uri, def_line, _) = semantic::first_location(&resp).ok_or(
            "Expected goto-definition to find the has_many relationship declaration for 'orders'",
        )?;

        assert_eq!(def_uri, uri, "Definition should be in the same file");
        assert_eq!(
            def_line, 2,
            "Definition should point to line 2 (has_many 'orders'), got line {def_line}"
        );
        Ok(())
    }

    /// `$self->company` on a belongs_to accessor must jump to the belongs_to declaration.
    #[test]
    fn test_dbix_belongs_to_accessor_goto_definition() -> TestResult {
        let code = "\
package MyApp::Schema::Result::User;
use DBIx::Class::Core;
__PACKAGE__->belongs_to('company', 'MyApp::Schema::Result::Company', 'company_id');
sub employer {
    my ($self) = @_;
    return $self->company;
}
1;
";
        let uri = "file:///dbix_user_belongs_to.pl";

        let resp = goto_def(code, uri, "company", 5)?;

        let (def_uri, def_line, _) = semantic::first_location(&resp)
            .ok_or("Expected goto-definition to find the belongs_to declaration for 'company'")?;

        assert_eq!(def_uri, uri);
        assert_eq!(
            def_line, 2,
            "Definition should point to line 2 (belongs_to 'company'), got line {def_line}"
        );
        Ok(())
    }
}
