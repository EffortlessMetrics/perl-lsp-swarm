//! Regression pins for the #18765 dead-code convention false positives.
//!
//! The raw [`WorkspaceIndex::find_unused_symbols`] query keeps reporting every
//! unused non-lexical symbol; [`WorkspaceIndex::
//! find_unused_symbols_for_diagnostics`] is the diagnostic-facing edge that
//! drops the convention-shaped false positives the issue documents:
//!
//! - `sub BUILD` in a Moo module (invoked by Moo's constructor — the issue's
//!   Product.pm:37 "Remove" advice broke object construction);
//! - `our @EXPORT_OK` (and `@EXPORT`, `%EXPORT_TAGS`, `$VERSION`) read
//!   dynamically by Exporter/CPAN tooling — the issue's Util.pm:6 case;
//! - names listed in a module's own `@EXPORT_OK` / `@EXPORT` list (an export
//!   list is usage evidence — `Exporter::import` hands those names out);
//! - `sub DEMOLISH` / `sub BUILDARGS` under the same Moo-family gate.
//!
//! Everything else keeps the pre-existing behavior: a genuinely-dead
//! non-convention subroutine stays flagged (positive control), and the raw
//! query keeps returning the convention symbols for non-diagnostic callers.

use perl_workspace::workspace::workspace_index::WorkspaceIndex;
use url::Url;

fn unused_names(index: &WorkspaceIndex) -> Vec<String> {
    index.find_unused_symbols_for_diagnostics().iter().map(|s| s.name.clone()).collect()
}

fn raw_unused_names(index: &WorkspaceIndex) -> Vec<String> {
    index.find_unused_symbols().iter().map(|s| s.name.clone()).collect()
}

fn index_file(
    index: &WorkspaceIndex,
    uri: &str,
    code: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = Url::parse(uri)?;
    index.index_file(url, code.to_string())?;
    Ok(())
}

/// The issue's Product.pm case: a Moo module whose `sub BUILD` must not be
/// flagged, while a genuinely-dead helper in the same file still is.
#[test]
fn moo_build_is_not_flagged_but_dead_helper_still_is() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Store/Product.pm",
        r#"package Store::Product;
use Moo;

sub BUILD {
    my ($self, $args) = @_;
    $self->{name} //= $args->{name};
}

sub display_price {
    my ($self) = @_;
    return $self->{price};
}

sub dead_helper {
    return 42;
}
"#,
    )?;

    let names = unused_names(&index);
    assert!(
        !names.iter().any(|n| n == "BUILD"),
        "BUILD is invoked by Moo's constructor and must not be flagged unused; got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "dead_helper"),
        "a genuinely-dead non-convention sub must still be flagged; got: {:?}",
        names
    );
    assert!(
        names.iter().any(|n| n == "display_price"),
        "a Moo accessor-shaped sub with no callers is method-dispatch territory and \
         stays flagged (Hint, softened advice); got: {:?}",
        names
    );

    // The raw query keeps its raw semantics (non-diagnostic callers).
    let raw = raw_unused_names(&index);
    assert!(
        raw.iter().any(|n| n == "BUILD"),
        "raw find_unused_symbols must keep reporting BUILD for non-diagnostic callers; \
         got: {:?}",
        raw
    );
    Ok(())
}

/// DEMOLISH and BUILDARGS get the same Moo-family suppression.
#[test]
fn moo_demolish_and_buildargs_are_not_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Store/Receipt.pm",
        r#"package Store::Receipt;
use Moo;

sub BUILDARGS {
    my ($class, @rest) = @_;
    return {@rest};
}

sub DEMOLISH {
    my ($self) = @_;
    undef $self->{handle};
}

sub total_units { return 7; }
"#,
    )?;

    let names = unused_names(&index);
    assert!(
        !names.iter().any(|n| n == "DEMOLISH" || n == "BUILDARGS"),
        "DEMOLISH/BUILDARGS are Moo construction hooks and must not be flagged; got: \
         {:?}",
        names
    );
    Ok(())
}

/// The conventions are import-gated: `sub BUILD` in a file with NO OO
/// framework import has no implicit invoker, so it stays flagged.
#[test]
fn build_without_oo_framework_import_stays_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Plain.pm",
        "package Plain;\n\nsub BUILD {\n    return 'not an object hook';\n}\n",
    )?;

    let names = unused_names(&index);
    assert!(
        names.iter().any(|n| n == "BUILD"),
        "BUILD in a plain non-OO file is genuinely suspicious and stays flagged; got: \
         {:?}",
        names
    );
    Ok(())
}

/// The issue's Util.pm case: `our @EXPORT_OK` itself, and every name it
/// lists, are consumed by Exporter and must not be flagged.
#[test]
fn exporter_variables_and_export_listed_subs_are_not_flagged()
-> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Store/Util.pm",
        r#"package Store::Util;
use Exporter 'import';

our $VERSION   = '0.01';
our @EXPORT_OK = qw(format_money format_date);
our %EXPORT_TAGS = (all => [@EXPORT_OK]);

sub format_money {
    my ($cents) = @_;
    return sprintf("$%.2f", $cents / 100);
}

sub format_date {
    my ($ts) = @_;
    return scalar localtime($ts);
}

sub dead_internal {
    return 'unused';
}
"#,
    )?;

    let names = unused_names(&index);
    for exported in ["@EXPORT_OK", "%EXPORT_TAGS", "$VERSION", "format_money", "format_date"] {
        assert!(
            !names.iter().any(|n| n == exported),
            "Exporter-consumed `{}` must not be flagged unused; got: {:?}",
            exported,
            names
        );
    }
    assert!(
        names.iter().any(|n| n == "dead_internal"),
        "a genuinely-dead non-exported sub must still be flagged; got: {:?}",
        names
    );
    Ok(())
}

/// `@EXPORT` (the default-export list) and the single-quoted list form also
/// count as usage evidence for the listed names.
#[test]
fn quoted_export_list_members_are_not_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Store/Quote.pm",
        r#"package Store::Quote;
use Exporter 'import';

our @EXPORT = ('render_invoice', "render_credit");

sub render_invoice { return 'invoice'; }
sub render_credit   { return 'credit'; }
"#,
    )?;

    let names = unused_names(&index);
    assert!(
        !names.iter().any(|n| n == "@EXPORT" || n == "render_invoice" || n == "render_credit"),
        "quoted @EXPORT list members are exported and must not be flagged; got: {:?}",
        names
    );
    Ok(())
}

/// Multi-line and `push`-accumulated export lists are harvested too.
#[test]
fn multiline_and_pushed_export_lists_are_harvested() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Store/Big.pm",
        r#"package Store::Big;
use Exporter 'import';

our @EXPORT_OK = (
    'alpha',
    'beta',
);

push @EXPORT_OK, qw(gamma);

sub alpha { return 1; }
sub beta  { return 2; }
sub gamma { return 3; }
"#,
    )?;

    let names = unused_names(&index);
    for exported in ["@EXPORT_OK", "alpha", "beta", "gamma"] {
        assert!(
            !names.iter().any(|n| n == exported),
            "export-listed `{}` must not be flagged unused; got: {:?}",
            exported,
            names
        );
    }
    Ok(())
}

/// Non-Exporter files are unaffected: an unused sub in a module without any
/// export list keeps the pre-existing behavior.
#[test]
fn module_without_export_list_keeps_existing_behavior() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Store/Island.pm",
        "package Store::Island;\n\nsub never_called { return 1; }\n",
    )?;

    let names = unused_names(&index);
    assert!(
        names.iter().any(|n| n == "never_called"),
        "an unused sub with no convention evidence stays flagged; got: {:?}",
        names
    );
    Ok(())
}

/// Identifier look-alikes must not trigger the conventions:
/// `@EXPORTER`, `%VERSION`, and a non-Moo `sub BUILD_like` are real symbols.
#[test]
fn identifier_lookalikes_are_not_convention_matched() -> Result<(), Box<dyn std::error::Error>> {
    let index = WorkspaceIndex::new();
    index_file(
        &index,
        "file:///lib/Store/Alike.pm",
        r#"package Store::Alike;

our @EXPORTER = ();
our %VERSION  = ();

sub BUILD_like { return 1; }
"#,
    )?;

    let names = unused_names(&index);
    for lookalike in ["@EXPORTER", "%VERSION", "BUILD_like"] {
        assert!(
            names.iter().any(|n| n == lookalike),
            "`{}` is not a convention symbol and must stay flagged; got: {:?}",
            lookalike,
            names
        );
    }
    Ok(())
}
