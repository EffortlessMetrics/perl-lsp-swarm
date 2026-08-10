//! Alignment tests: DAP breakpoint validator respects `NodeKind::safe_for_breakpoint()`.
//!
//! These tests verify that the validator rejects lines whose only AST nodes have
//! `safe_for_breakpoint() == false` (compile-time pragmas, data sections, format
//! declarations) while still accepting ordinary runtime statements.
//!
//! Tracking issue: #914 (builder-8: migrate DAP breakpoint validator to classification flags).

use perl_dap::breakpoint::{AstBreakpointValidator, BreakpointValidator};
use perl_tdd_support::must;

// ---------------------------------------------------------------------------
// Regression guard: ordinary runtime statements remain executable
// ---------------------------------------------------------------------------

#[test]
fn assignment_statement_is_executable() -> Result<(), Box<dyn std::error::Error>> {
    let v = must(AstBreakpointValidator::new("$x = 1;\n"));
    let result = v.validate(1);
    assert!(result.verified, "assignment statement should be a valid breakpoint location");
    Ok(())
}

#[test]
fn variable_declaration_is_executable() -> Result<(), Box<dyn std::error::Error>> {
    let v = must(AstBreakpointValidator::new("my $x = 42;\n"));
    let result = v.validate(1);
    assert!(result.verified, "my-declaration should be a valid breakpoint location");
    Ok(())
}

#[test]
fn function_call_is_executable() -> Result<(), Box<dyn std::error::Error>> {
    let v = must(AstBreakpointValidator::new("print \"hello\";\n"));
    let result = v.validate(1);
    assert!(result.verified, "function call should be a valid breakpoint location");
    Ok(())
}

// ---------------------------------------------------------------------------
// Use/No: compile-time pragmas are not valid breakpoint locations
// ---------------------------------------------------------------------------

#[test]
fn use_statement_is_not_executable() -> Result<(), Box<dyn std::error::Error>> {
    // `use strict;` is a compile-time BEGIN pragma.
    // safe_for_breakpoint() == false; the validator must reject it.
    let v = must(AstBreakpointValidator::new("use strict;\nmy $x = 1;\n"));
    let result = v.validate(1);
    assert!(
        !result.verified,
        "use strict; is a compile-time pragma and must not be a valid breakpoint location"
    );
    Ok(())
}

#[test]
fn use_warnings_is_not_executable() -> Result<(), Box<dyn std::error::Error>> {
    let v = must(AstBreakpointValidator::new("use warnings;\nprint 1;\n"));
    let result = v.validate(1);
    assert!(
        !result.verified,
        "use warnings; is a compile-time pragma and must not be a valid breakpoint location"
    );
    Ok(())
}

#[test]
fn no_statement_is_not_executable() -> Result<(), Box<dyn std::error::Error>> {
    // `no strict;` is also a compile-time unimport pragma (safe_for_breakpoint == false).
    let v = must(AstBreakpointValidator::new("no strict;\nmy $x = 1;\n"));
    let result = v.validate(1);
    assert!(
        !result.verified,
        "no strict; is a compile-time unimport and must not be a valid breakpoint location"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// __DATA__ section: non-executable raw content block
// ---------------------------------------------------------------------------

#[test]
fn data_section_marker_is_not_executable() -> Result<(), Box<dyn std::error::Error>> {
    // The __DATA__ marker line is parsed as a DataSection node (safe_for_breakpoint == false).
    let source = "my $x = 1;\n__DATA__\nsome data here\n";
    let v = must(AstBreakpointValidator::new(source));
    // Line 2 is "__DATA__" — the DataSection node starts here.
    let result = v.validate(2);
    assert!(
        !result.verified,
        "__DATA__ line must not be a valid breakpoint location (DataSection: bp=false)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Format declaration: non-executable Perl report format header
// ---------------------------------------------------------------------------

#[test]
fn format_declaration_is_not_executable() -> Result<(), Box<dyn std::error::Error>> {
    // `format STDOUT =` introduces a Perl report format declaration (safe_for_breakpoint == false).
    let source = "format STDOUT =\n@\n$name\n.\n";
    let v = must(AstBreakpointValidator::new(source));
    let result = v.validate(1);
    assert!(
        !result.verified,
        "format declaration header must not be a valid breakpoint location (Format: bp=false)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Mixed-file: use + runtime statements — only runtime lines are executable
// ---------------------------------------------------------------------------

#[test]
fn mixed_file_use_then_statement() -> Result<(), Box<dyn std::error::Error>> {
    let source = "use strict;\nuse warnings;\nmy $x = 42;\nprint $x;\n";
    let v = must(AstBreakpointValidator::new(source));

    // Pragma lines (1, 2) must be rejected
    assert!(!v.validate(1).verified, "line 1 (use strict) must not be a valid breakpoint location");
    assert!(
        !v.validate(2).verified,
        "line 2 (use warnings) must not be a valid breakpoint location"
    );

    // Runtime statement lines (3, 4) must be accepted
    assert!(v.validate(3).verified, "line 3 (my $x = 42) must be a valid breakpoint location");
    assert!(v.validate(4).verified, "line 4 (print $x) must be a valid breakpoint location");
    Ok(())
}
