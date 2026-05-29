//! Fixture-style UX coverage for deterministic inline completion.
//!
//! These tests keep the visible ghost-text contract readable: the provider
//! should suggest project/style-aware text where it has context and stay silent
//! in zones where automatic ghost text would be noisy or unsafe.

use std::error::Error;

use lsp_types::Range;
use perl_lsp_rs_core::providers::inline_completion::{
    InlineCompletionEnvironment, InlineCompletionList, InlineCompletionProvider,
};
use perl_parser_core::position::{offset_to_utf16_line_col, utf16_line_col_to_offset};

type TestResult = Result<(), Box<dyn Error>>;

const CURSOR: &str = "<<CURSOR>>";

struct InlineCompletionScenario {
    text: String,
    line: u32,
    character: u32,
}

impl InlineCompletionScenario {
    fn from_fixture(fixture: &str) -> Result<Self, String> {
        let byte = fixture
            .find(CURSOR)
            .ok_or_else(|| "fixture must include <<CURSOR>> marker".to_string())?;
        let text = fixture.replacen(CURSOR, "", 1);
        let (line, character) = offset_to_utf16_line_col(&text, byte);

        Ok(Self { text, line, character })
    }

    fn completions(&self) -> InlineCompletionList {
        InlineCompletionProvider::new().get_inline_completions(
            self.text.as_str(),
            self.line,
            self.character,
        )
    }

    fn completions_with_environment(
        &self,
        environment: &InlineCompletionEnvironment,
    ) -> InlineCompletionList {
        InlineCompletionProvider::new().get_inline_completions_with_environment(
            self.text.as_str(),
            self.line,
            self.character,
            environment,
        )
    }
}

struct SuggestionFixture {
    name: &'static str,
    source: &'static str,
    first: Option<&'static str>,
    expected: &'static [&'static str],
    not_expected: &'static [&'static str],
}

struct SilentFixture {
    name: &'static str,
    source: &'static str,
}

struct RangeFixture {
    name: &'static str,
    source: &'static str,
    insert_text: &'static str,
    replaces: &'static str,
}

#[test]
fn inline_completion_fixture_corpus_returns_expected_ghost_text() -> TestResult {
    let fixtures = [
        SuggestionFixture {
            name: "use_pragmas",
            source: "use <<CURSOR>>",
            first: Some("strict;"),
            expected: &["strict;", "warnings;"],
            not_expected: &["done_testing();"],
        },
        SuggestionFixture {
            name: "use_pragmas_after_format_terminator",
            source: "format STDOUT =\n@<<<<\n$name\n.\nuse <<CURSOR>>",
            first: Some("strict;"),
            expected: &["strict;", "warnings;"],
            not_expected: &["done_testing();"],
        },
        SuggestionFixture {
            name: "test_more_assertion_prefers_visible_actual_expected",
            source: "use Test::More;\n\nmy $got = compute();\nmy $expected = 42;\n\n<<CURSOR>>",
            first: Some("is($got, $expected, 'test description');"),
            expected: &["is($got, $expected, 'test description');"],
            not_expected: &["done_testing();"],
        },
        SuggestionFixture {
            name: "test2_assertion_uses_visible_result",
            source: "use Test2::V0;\n\nmy $result = compute();\n\n<<CURSOR>>",
            first: Some("ok($result, 'test description');"),
            expected: &["ok($result, 'test description');"],
            not_expected: &["done_testing();"],
        },
        SuggestionFixture {
            name: "blank_line_in_sub_uses_visible_lexical",
            source: "sub compute {\n    my $result = build();\n    <<CURSOR>>\n}\n",
            first: Some("return $result;"),
            expected: &["return $result;"],
            not_expected: &["return $ghost;"],
        },
        SuggestionFixture {
            name: "self_receiver_prefers_current_package_methods",
            source: "package Other;\nsub external {}\n\npackage Demo;\nsub save {}\nsub display_name {}\nsub caller {\n    my $self = shift;\n    $self-><<CURSOR>>\n}\n",
            first: Some("save()"),
            expected: &["save()", "display_name()"],
            not_expected: &["external()", "new()"],
        },
        SuggestionFixture {
            name: "constructor_completion_keeps_shift_style",
            source: "sub helper {\n    my $self = shift;\n}\n\nsub new<<CURSOR>>",
            first: Some(
                " {\n    my $class = shift;\n    my $self = bless {}, $class;\n    return $self;\n}",
            ),
            expected: &[
                " {\n    my $class = shift;\n    my $self = bless {}, $class;\n    return $self;\n}",
            ],
            not_expected: &[
                " ($class, %args) {\n    my $self = bless {}, $class;\n    return $self;\n}",
            ],
        },
        SuggestionFixture {
            name: "constructor_completion_keeps_at_underscore_style",
            source: "sub helper {\n    my ($self, %args) = @_;\n}\n\nsub new<<CURSOR>>",
            first: Some(
                " {\n    my ($class, %args) = @_;\n    my $self = bless {}, $class;\n    return $self;\n}",
            ),
            expected: &[
                " {\n    my ($class, %args) = @_;\n    my $self = bless {}, $class;\n    return $self;\n}",
            ],
            not_expected: &[
                " {\n    my $class = shift;\n    my $self = bless {}, $class;\n    return $self;\n}",
            ],
        },
        SuggestionFixture {
            name: "constructor_completion_keeps_signature_style",
            source: "sub helper ($self, %args) {\n}\n\nsub new<<CURSOR>>",
            first: Some(
                " ($class, %args) {\n    my $self = bless {}, $class;\n    return $self;\n}",
            ),
            expected: &[
                " ($class, %args) {\n    my $self = bless {}, $class;\n    return $self;\n}",
            ],
            not_expected: &["my $class = shift;"],
        },
    ];

    for fixture in fixtures {
        assert_suggestions(fixture)?;
    }

    Ok(())
}

#[test]
fn inline_completion_fixture_corpus_covers_language_construct_prompts() -> TestResult {
    let fixtures = [
        SuggestionFixture {
            name: "empty_file_scaffold",
            source: "<<CURSOR>>",
            first: Some("#!/usr/bin/env perl\nuse strict;\nuse warnings;\n\n"),
            expected: &["#!/usr/bin/env perl\nuse strict;\nuse warnings;\n\n"],
            not_expected: &["done_testing();"],
        },
        SuggestionFixture {
            name: "shebang_interpreter",
            source: "#!<<CURSOR>>",
            first: Some("/usr/bin/env perl"),
            expected: &["/usr/bin/env perl"],
            not_expected: &["strict;"],
        },
        SuggestionFixture {
            name: "package_declaration",
            source: "package <<CURSOR>>",
            first: Some("MyPackage;\n\nuse strict;\nuse warnings;"),
            expected: &["MyPackage;\n\nuse strict;\nuse warnings;"],
            not_expected: &["strict;"],
        },
        SuggestionFixture {
            name: "bless_arguments",
            source: "sub new {\n    my $class = shift;\n    my $self = {};\n    bless <<CURSOR>>\n}\n",
            first: Some("$self, $class;"),
            expected: &["$self, $class;"],
            not_expected: &["return $self;"],
        },
    ];

    for fixture in fixtures {
        assert_suggestions(fixture)?;
    }

    Ok(())
}

#[test]
fn inline_completion_fixture_corpus_covers_semantic_statement_prompts() -> TestResult {
    let fixtures = [
        SuggestionFixture {
            name: "guard_condition_prefers_booleanish_scalar",
            source: "sub save {\n    my $result = compute();\n    my $is_ready = validate($result);\n    return unless <<CURSOR>>\n}\n",
            first: Some("$is_ready;"),
            expected: &["$is_ready;"],
            not_expected: &["$result;"],
        },
        SuggestionFixture {
            name: "loop_binding_singularizes_array_name",
            source: "sub render {\n    my @children = @_;\n    for <<CURSOR>>\n}\n",
            first: Some("my $child (@children) {\n    \n}"),
            expected: &["my $child (@children) {\n    \n}"],
            not_expected: &["my $item (@children) {\n    \n}"],
        },
        SuggestionFixture {
            name: "loop_binding_uses_hash_key_name",
            source: "sub render {\n    my %user_by_id = load_users();\n    foreach <<CURSOR>>\n}\n",
            first: Some("my $id (keys %user_by_id) {\n    \n}"),
            expected: &["my $id (keys %user_by_id) {\n    \n}"],
            not_expected: &["my $item (%user_by_id) {\n    \n}"],
        },
        SuggestionFixture {
            name: "test_more_ok_arguments_use_visible_actual",
            source: "use Test::More;\n\nmy $success = run_case();\n\nok(<<CURSOR>>",
            first: Some("$success, 'test description');"),
            expected: &["$success, 'test description');"],
            not_expected: &["done_testing();"],
        },
        SuggestionFixture {
            name: "test_more_is_arguments_use_actual_and_want",
            source: "use Test::More;\n\nmy $value = subject();\nmy $want = 7;\n\nis(<<CURSOR>>",
            first: Some("$value, $want, 'test description');"),
            expected: &["$value, $want, 'test description');"],
            not_expected: &["$value, $value, 'test description');"],
        },
    ];

    for fixture in fixtures {
        assert_suggestions(fixture)?;
    }

    Ok(())
}

#[test]
fn inline_completion_fixture_corpus_covers_project_and_receiver_facts() -> TestResult {
    let module_scenario = InlineCompletionScenario::from_fixture("use My::<<CURSOR>>")?;
    let environment = InlineCompletionEnvironment {
        available_modules: vec![
            "Other::Thing".to_string(),
            "My::App".to_string(),
            "My::App".to_string(),
            "My::Util".to_string(),
            String::new(),
        ],
    };
    let module_completions = module_scenario.completions_with_environment(&environment);
    let module_texts = completion_texts(&module_completions);

    if module_texts.first().copied() != Some("My::App;") {
        return Err(format!(
            "available modules should rank sorted project modules first, got {module_texts:?}"
        )
        .into());
    }
    if !module_texts.contains(&"My::Util;") || module_texts.contains(&"Other::Thing;") {
        return Err(format!(
            "available modules should filter by typed module fragment, got {module_texts:?}"
        )
        .into());
    }

    let dbh_scenario = InlineCompletionScenario::from_fixture(
        "use DBI;\nmy $dbh = DBI->connect($dsn);\n$dbh->pr<<CURSOR>>",
    )?;
    let dbh_completions = dbh_scenario.completions();
    let dbh_texts = completion_texts(&dbh_completions);
    if dbh_texts.first().copied() != Some("prepare()") || dbh_texts.contains(&"new()") {
        return Err(format!(
            "DBI database handles should suggest DBI methods before generic receivers, got {dbh_texts:?}"
        )
        .into());
    }

    let sth_scenario = InlineCompletionScenario::from_fixture(
        "use DBI;\nmy $dbh = DBI->connect($dsn);\nmy $sth = $dbh->prepare($sql);\n$sth->fe<<CURSOR>>",
    )?;
    let sth_completions = sth_scenario.completions();
    let sth_texts = completion_texts(&sth_completions);
    if sth_texts.first().copied() != Some("fetchrow_hashref()") {
        return Err(format!(
            "DBI statement handles should suggest statement fetch methods first, got {sth_texts:?}"
        )
        .into());
    }

    Ok(())
}

#[test]
fn inline_completion_fixture_corpus_stays_silent_in_reject_zones() -> TestResult {
    let fixtures = [
        SilentFixture { name: "line_comment", source: "# use <<CURSOR>>" },
        SilentFixture { name: "string_literal", source: "my $text = \"use <<CURSOR>>\";" },
        SilentFixture { name: "unterminated_string", source: "my $text = \"use <<CURSOR>>" },
        SilentFixture { name: "quote_single", source: "my $text = q{use <<CURSOR>>};" },
        SilentFixture { name: "quote_double", source: "my $text = qq{use <<CURSOR>>};" },
        SilentFixture { name: "quote_words", source: "my @words = qw(use <<CURSOR>>);" },
        SilentFixture { name: "quote_command", source: "my $output = qx(use <<CURSOR>>);" },
        SilentFixture { name: "heredoc_body", source: "print <<'EOF';\nuse <<CURSOR>>\nEOF\n" },
        SilentFixture {
            name: "format_body",
            source: "format STDOUT =\nuse <<CURSOR>>\n.\nwrite STDOUT;\n",
        },
        SilentFixture { name: "data_body", source: "__DATA__\nuse <<CURSOR>>\n" },
        SilentFixture { name: "pod_body", source: "=pod\nuse <<CURSOR>>\n=cut\n" },
        SilentFixture { name: "regex_literal", source: "if ($name =~ /use <<CURSOR>>/) {}" },
        SilentFixture { name: "substitution", source: "$name =~ s/use <<CURSOR>>/strict/;" },
        SilentFixture { name: "transliteration", source: "$name =~ tr/use <<CURSOR>>/abc/;" },
    ];

    for fixture in fixtures {
        let scenario = InlineCompletionScenario::from_fixture(fixture.source)?;
        let completions = scenario.completions();
        if !completions.items.is_empty() {
            return Err(format!(
                "{}: expected no inline completions, got {:?}",
                fixture.name,
                completion_texts(&completions)
            )
            .into());
        }
    }

    Ok(())
}

#[test]
fn inline_completion_fixture_corpus_uses_editor_safe_replacement_ranges() -> TestResult {
    let fixtures = [
        RangeFixture {
            name: "use_partial_token",
            source: "use str<<CURSOR>>",
            insert_text: "strict;",
            replaces: "str",
        },
        RangeFixture {
            name: "method_arrow_partial_token",
            source: "$obj->n<<CURSOR>>",
            insert_text: "new()",
            replaces: "n",
        },
    ];

    for fixture in fixtures {
        assert_replacement_range(fixture)?;
    }

    Ok(())
}

fn assert_suggestions(fixture: SuggestionFixture) -> TestResult {
    let scenario = InlineCompletionScenario::from_fixture(fixture.source)?;
    let completions = scenario.completions();

    if let Some(first) = fixture.first {
        let actual =
            completions.items.first().map(|item| item.insert_text.as_str()).ok_or_else(|| {
                format!("{}: expected first completion {first}, got none", fixture.name)
            })?;
        if actual != first {
            return Err(format!(
                "{}: expected first completion {first}, got {actual}",
                fixture.name
            )
            .into());
        }
    }

    for expected in fixture.expected {
        if !completions.items.iter().any(|item| item.insert_text == *expected) {
            return Err(format!(
                "{}: expected completion {expected}, got {:?}",
                fixture.name,
                completion_texts(&completions)
            )
            .into());
        }
    }

    for unexpected in fixture.not_expected {
        if completions.items.iter().any(|item| item.insert_text == *unexpected) {
            return Err(format!(
                "{}: unexpected completion {unexpected}, got {:?}",
                fixture.name,
                completion_texts(&completions)
            )
            .into());
        }
    }

    Ok(())
}

fn assert_replacement_range(fixture: RangeFixture) -> TestResult {
    let scenario = InlineCompletionScenario::from_fixture(fixture.source)?;
    let completions = scenario.completions();
    let item =
        completions.items.iter().find(|item| item.insert_text == fixture.insert_text).ok_or_else(
            || {
                format!(
                    "{}: expected completion {}, got {:?}",
                    fixture.name,
                    fixture.insert_text,
                    completion_texts(&completions)
                )
            },
        )?;
    let range = item
        .range
        .as_ref()
        .ok_or_else(|| format!("{}: expected replacement range", fixture.name))?;
    let replaced = slice_for_range(scenario.text.as_str(), range)?;

    if replaced != fixture.replaces {
        return Err(format!(
            "{}: expected replacement range to cover {:?}, got {:?}",
            fixture.name, fixture.replaces, replaced
        )
        .into());
    }

    Ok(())
}

fn completion_texts(completions: &InlineCompletionList) -> Vec<&str> {
    completions.items.iter().map(|item| item.insert_text.as_str()).collect()
}

fn slice_for_range<'a>(text: &'a str, range: &Range) -> Result<&'a str, String> {
    let start = utf16_line_col_to_offset(text, range.start.line, range.start.character);
    let end = utf16_line_col_to_offset(text, range.end.line, range.end.character);
    text.get(start..end).ok_or_else(|| format!("invalid UTF-8 boundaries for range {start}..{end}"))
}
