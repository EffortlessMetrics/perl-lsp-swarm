//! Workspace-rename criterion benchmark (#18031, first workspace-rename bench).
//!
//! Measures the cross-file rename computation the `textDocument/rename`
//! handler dispatches to: `perl_lsp::features::workspace_rename::
//! build_rename_edit` (handler call sites in
//! `src/runtime/language/rename.rs`), over a small multi-package Perl
//! workspace held in a `perl_workspace::workspace_index::WorkspaceIndex`.
//!
//! # Fixtures
//!
//! * Dispatch-heavy (`method_dispatch_heavy_greeter_greet`): `Greeter::greet`
//!   shares its bare name with `Base::greet` (parent of `Child`) and
//!   `Logger::greet`. Eight caller packages each hold a constructor-typed
//!   receiver (`my $obj = Greeter->new`), a foreign-typed look-alike receiver
//!   (`my $logger = Logger->new`), and a qualified call. Renaming
//!   `Greeter::greet` must run the receiver-authority logic
//!   (#17448/#9814/#18012) on every arrow site the bare-name method-call index
//!   returns: cross-package bare-ref scan, per-call-site AST parsing,
//!   constructor attribution, and inheritance checks — while excluding the
//!   foreign-typed and inherited look-alike sites.
//! * Inherited dispatch (`method_inherited_base_shared`): `Base::shared`
//!   renamed while `Child` (`use parent 'Base';`) dispatches
//!   `$self->shared`.
//! * Ambiguous refusal (`method_ambiguous_refusal`): unqualified cross-package
//!   bare calls (`name()` and `&name()`) force the fail-closed
//!   `RenameRefusal::AmbiguousIdentity` path instead of an edit set.
//!
//! Every fixture is validated once at setup, outside the timed region —
//! including the exact per-caller edit count and position for the
//! dispatch-heavy fixture — so a fixture drift or attribution regression
//! that stops exercising the intended authority path fails the run before
//! any timing happens.
//!
//! # Running
//!
//! ```bash
//! cargo bench -p perl-lsp-rs --bench workspace_rename_benchmark --locked
//! ```
//!
//! `cargo test -p perl-lsp-rs --bench workspace_rename_benchmark` runs the
//! binary in Criterion's test mode (each bench once, no timing), which is the
//! fast fixture-validation lane.

use criterion::{Criterion, criterion_group, criterion_main};
use perl_lsp::features::workspace_rename::{RenameRefusal, build_rename_edit};
use perl_workspace::workspace_index::{SymKind, SymbolKey, WorkspaceIndex};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::Arc;
use url::Url;

/// Number of dispatch-heavy caller packages in the dispatch fixture.
const CALLER_COUNT: u32 = 8;

fn sub_key(pkg: &str, name: &str) -> SymbolKey {
    SymbolKey { pkg: Arc::from(pkg), name: Arc::from(name), sigil: None, kind: SymKind::Sub }
}

/// Index one fixture document, failing the run on a broken fixture.
///
/// Fixture indexing is setup, not measurement: an indexing failure means the
/// fixture no longer exercises the rename paths the benches claim to measure,
/// so the run aborts before any timing happens.
fn index_fixture(idx: &WorkspaceIndex, uri: &str, text: &str) {
    let indexed = Url::parse(uri)
        .map_err(|err| err.to_string())
        .and_then(|url| idx.index_initial_file(url, text.to_string()));
    assert!(indexed.is_ok(), "fixture document {uri} failed to index: {:?}", indexed.err());
}

/// One dispatch-heavy caller package: a constructor-typed receiver for the
/// rename target, a foreign-typed look-alike receiver, and a qualified call.
fn caller_module(n: u32) -> String {
    format!(
        concat!(
            "package Caller{0:02};\n",
            "use Greeter;\n",
            "use Logger;\n",
            "sub process {{\n",
            "    my $obj = Greeter->new();\n",
            "    my $logger = Logger->new();\n",
            "    my $first = $obj->greet();\n",
            "    my $log = $logger->greet();\n",
            "    my $second = Greeter::greet($obj);\n",
            "    return ($first, $log, $second, $obj->greet());\n",
            "}}\n",
            "1;\n",
        ),
        n
    )
}

/// The dispatch-heavy multi-package workspace.
///
/// Same-named methods on three packages (`Base`, `Greeter`, `Logger`), a
/// parent/child inheritance pair (`Base` -> `Child`), and `CALLER_COUNT`
/// caller packages mixing constructor-attributed, foreign-typed, and
/// qualified call sites for the bare method name.
fn dispatch_index() -> WorkspaceIndex {
    let idx = WorkspaceIndex::new();
    index_fixture(
        &idx,
        "file:///bench-rename/Base.pm",
        concat!(
            "package Base;\n",
            "sub new { return bless {}, shift }\n",
            "sub greet { return 'base-greet' }\n",
            "sub shared { return 'base-shared' }\n",
            "1;\n",
        ),
    );
    index_fixture(
        &idx,
        "file:///bench-rename/Child.pm",
        concat!(
            "package Child;\n",
            "use parent 'Base';\n",
            "sub run {\n",
            "    my ($self) = @_;\n",
            "    return $self->shared;\n",
            "}\n",
            "1;\n",
        ),
    );
    index_fixture(
        &idx,
        "file:///bench-rename/Greeter.pm",
        concat!(
            "package Greeter;\n",
            "sub new { return bless {}, shift }\n",
            "sub greet { return 'hi' }\n",
            "1;\n",
        ),
    );
    index_fixture(
        &idx,
        "file:///bench-rename/Logger.pm",
        concat!(
            "package Logger;\n",
            "sub new { return bless {}, shift }\n",
            "sub greet { return 'log-line' }\n",
            "1;\n",
        ),
    );
    for n in 1..=CALLER_COUNT {
        let uri = format!("file:///bench-rename/Caller{n:02}.pm");
        index_fixture(&idx, &uri, &caller_module(n));
    }
    idx
}

/// The ambiguous fixture: unqualified cross-package bare calls (plain and
/// `&`-sigil) that rename must refuse rather than rewrite.
fn ambiguous_index() -> WorkspaceIndex {
    let idx = WorkspaceIndex::new();
    index_fixture(
        &idx,
        "file:///bench-ambiguous/Foo.pm",
        "package Foo;\nsub process_data { return 1; }\n1;\n",
    );
    index_fixture(
        &idx,
        "file:///bench-ambiguous/Bar.pm",
        concat!(
            "package Bar;\n",
            "sub run { return process_data(); }\n",
            "sub audit { return &process_data(); }\n",
            "1;\n",
        ),
    );
    idx
}

/// Expected dispatch edit spans in every caller package, as sorted
/// `(start_line, start_col, end_line, end_col)` of the narrowed `greet`
/// token (0-based; the fixture is pure ASCII, so columns are UTF-16-stable).
///
/// Exactly the three attributed sites: the two constructor-attributed
/// `$obj->greet()` arrow calls (lines 6 and 9) and the qualified
/// `Greeter::greet($obj)` call (line 8). The foreign-typed
/// `$logger->greet()` site (line 7) must stay unedited.
const CALLER_EXPECTED_SPANS: [(u32, u32, u32, u32); 3] =
    [(6, 22, 6, 27), (8, 26, 8, 31), (9, 41, 9, 46)];

/// Expected dispatch edit span of the `Greeter::greet` definition name in
/// Greeter.pm (line 2, after the `sub ` prefix).
const GREETER_DEF_SPAN: (u32, u32, u32, u32) = (2, 4, 2, 9);

/// Setup-time validation of the dispatch-heavy fixture.
///
/// The rename must cover exactly the definition and every
/// constructor-attributed caller — never the foreign-typed Logger receiver,
/// the inherited Base receiver, or any other same-named package. Each caller
/// is pinned to exactly its three attributed edits by position — both
/// `$obj->greet()` arrow sites plus the qualified `Greeter::greet($obj)` —
/// so a caller that received only its qualified-call edit, or that gained a
/// wrongly included `$logger->greet()` site, fails before any timing.
fn validate_dispatch_fixture(idx: &WorkspaceIndex) {
    let renamed = build_rename_edit(idx, &sub_key("Greeter", "greet"), "salute");
    assert!(
        renamed.is_ok(),
        "dispatch fixture rename must succeed; refused: {:?}",
        renamed.as_ref().err()
    );
    let edits = renamed.unwrap_or_default();

    let mut renamed_uris: Vec<&str> = edits.iter().map(|edit| edit.uri.as_str()).collect();
    renamed_uris.sort_unstable();

    let mut expected: Vec<String> =
        (1..=CALLER_COUNT).map(|n| format!("file:///bench-rename/Caller{n:02}.pm")).collect();
    expected.push("file:///bench-rename/Greeter.pm".to_string());
    expected.sort_unstable();

    assert_eq!(
        renamed_uris, expected,
        "dispatch rename must edit exactly the definition + constructor-attributed callers"
    );

    let mut actual_spans: BTreeMap<String, Vec<(u32, u32, u32, u32)>> = BTreeMap::new();
    for edit in &edits {
        let mut spans: Vec<(u32, u32, u32, u32)> =
            edit.edits.iter().map(|te| (te.start.0, te.start.1, te.end.0, te.end.1)).collect();
        spans.sort_unstable();
        actual_spans.insert(edit.uri.clone(), spans);
    }
    let mut expected_spans: BTreeMap<String, Vec<(u32, u32, u32, u32)>> = BTreeMap::new();
    for n in 1..=CALLER_COUNT {
        expected_spans.insert(
            format!("file:///bench-rename/Caller{n:02}.pm"),
            CALLER_EXPECTED_SPANS.to_vec(),
        );
    }
    expected_spans.insert("file:///bench-rename/Greeter.pm".to_string(), vec![GREETER_DEF_SPAN]);

    assert_eq!(
        actual_spans, expected_spans,
        "dispatch rename must place exactly CALLER_EXPECTED_SPANS in each caller \
         (both $obj->greet() sites + the qualified Greeter::greet($obj) site, \
         never $logger->greet()) and exactly the definition-name edit in Greeter.pm"
    );

    assert!(
        edits.iter().all(|edit| edit.edits.iter().all(|te| te.new_text == "salute")),
        "every dispatch edit renames the method"
    );
}

/// Setup-time validation of the inherited-dispatch fixture: the rename covers
/// exactly the parent definition and the child `$self->shared` dispatch site.
fn validate_inherited_fixture(idx: &WorkspaceIndex) {
    let renamed = build_rename_edit(idx, &sub_key("Base", "shared"), "renamed_shared");
    assert!(
        renamed.is_ok(),
        "inherited fixture rename must succeed; refused: {:?}",
        renamed.as_ref().err()
    );
    let edits = renamed.unwrap_or_default();
    let mut uris: Vec<&str> = edits.iter().map(|edit| edit.uri.as_str()).collect();
    uris.sort_unstable();
    assert_eq!(
        uris,
        vec!["file:///bench-rename/Base.pm", "file:///bench-rename/Child.pm"],
        "inherited rename must edit exactly the parent definition and the child dispatch site"
    );
}

/// Setup-time validation of the ambiguous fixture: the rename must refuse.
fn validate_ambiguous_fixture(idx: &WorkspaceIndex) {
    let renamed = build_rename_edit(idx, &sub_key("Foo", "process_data"), "process_records");
    assert!(
        matches!(&renamed, Err(RenameRefusal::AmbiguousIdentity(_))),
        "ambiguous fixture must refuse with AmbiguousIdentity; got: {:?}",
        renamed.as_ref().map(|edits| edits.len())
    );
}

fn benchmark_workspace_rename(c: &mut Criterion) {
    let dispatch = dispatch_index();
    validate_dispatch_fixture(&dispatch);
    validate_inherited_fixture(&dispatch);
    let ambiguous = ambiguous_index();
    validate_ambiguous_fixture(&ambiguous);

    let greeter_greet = sub_key("Greeter", "greet");
    let base_shared = sub_key("Base", "shared");
    let foo_process_data = sub_key("Foo", "process_data");

    let mut group = c.benchmark_group("workspace_rename");

    // The Ok path over the dispatch-heavy fixture: the cross-package bare-ref
    // scan, per-call-site AST parse + constructor attribution, same-name
    // exclusion, and edit-set grouping all run on every rename request.
    group.bench_function("method_dispatch_heavy_greeter_greet", |b| {
        b.iter(|| {
            black_box(build_rename_edit(
                black_box(&dispatch),
                black_box(&greeter_greet),
                black_box("salute"),
            ))
        })
    });

    // Parent/child inheritance: `$self->shared` in Child must join the edit
    // set through the explicit `use parent` inheritance check.
    group.bench_function("method_inherited_base_shared", |b| {
        b.iter(|| {
            black_box(build_rename_edit(
                black_box(&dispatch),
                black_box(&base_shared),
                black_box("renamed_shared"),
            ))
        })
    });

    // The fail-closed refusal path: unqualified cross-package bare call sites
    // force AmbiguousIdentity instead of an edit set.
    group.bench_function("method_ambiguous_refusal", |b| {
        b.iter(|| {
            black_box(build_rename_edit(
                black_box(&ambiguous),
                black_box(&foo_process_data),
                black_box("process_records"),
            ))
        })
    });

    group.finish();
}

criterion_group!(benches, benchmark_workspace_rename);
criterion_main!(benches);
