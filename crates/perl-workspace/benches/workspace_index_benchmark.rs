#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use perl_tdd_support::{must, must_some};
use perl_workspace::workspace_index::{SourceCommit, SourceCommitOutcome, WorkspaceIndex};
use std::fs;
use std::hint::black_box;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use url::Url;

// Shared cold scan+index sampler for the `index_real_corpus` group
// (issue #17159, matrix row 5). Also included by the receipt test via
// `#[path]`, so the module stays lint-clean under both target lint sets.
#[path = "support/index_real_corpus.rs"]
mod index_real_corpus;

/// Sample Perl code representing a typical module
const SAMPLE_MODULE: &str = r#"
package MyModule;
use strict;
use warnings;

our $VERSION = '1.00';

sub new {
    my $class = shift;
    return bless {}, $class;
}

sub process_data {
    my ($self, $data) = @_;

    for my $item (@$data) {
        my $result = $self->transform($item);
        print "Result: $result\n";
    }

    return 1;
}

sub transform {
    my ($self, $value) = @_;
    return $value * 2;
}

sub calculate {
    my ($self, $x, $y) = @_;
    return $x + $y;
}

1;
"#;

/// Sample Perl code with multiple packages
const MULTI_PACKAGE_MODULE: &str = r#"
package First::Module;
our $VERSION = '0.01';

sub first_sub {
    return "first";
}

package Second::Module;
our $VERSION = '0.02';

sub second_sub {
    return "second";
}

package Third::Module;
our $VERSION = '0.03';

sub third_sub {
    return "third";
}

1;
"#;

/// Sample script with various constructs
const SAMPLE_SCRIPT: &str = r#"
#!/usr/bin/env perl
use strict;
use warnings;
use MyModule;

my $obj = MyModule->new();

sub main {
    my @data = (1, 2, 3, 4, 5);
    $obj->process_data(\@data);
}

sub helper {
    my ($x) = @_;
    return $x * 3;
}

main();
"#;

/// Modern Perl features sample
const MODERN_PERL: &str = r#"
use v5.38;
use feature qw(signatures try);

sub add($x, $y) {
    return $x + $y;
}

sub safe_divide($a, $b) {
    try {
        die "Division by zero" if $b == 0;
        return $a / $b;
    }
    catch ($e) {
        warn "Error: $e";
        return undef;
    }
}

package Point {
    use feature 'class';

    field $x :param = 0;
    field $y :param = 0;

    method move($dx, $dy) {
        $x += $dx;
        $y += $dy;
    }
}

1;
"#;

/// Complex module with inheritance
const COMPLEX_MODULE: &str = r#"
package Complex::Module;
use strict;
use warnings;
use parent qw(Base::Class);

our $VERSION = '2.34';

sub new {
    my ($class, %args) = @_;
    my $self = $class->SUPER::new(%args);
    return bless $self, $class;
}

sub method_one {
    my ($self, $arg1, $arg2) = @_;
    return $self->helper($arg1) + $arg2;
}

sub method_two {
    my ($self, @items) = @_;
    my @results;
    for my $item (@items) {
        push @results, $self->process($item);
    }
    return @results;
}

sub helper {
    my ($self, $value) = @_;
    return $value * 2;
}

sub process {
    my ($self, $item) = @_;
    return $item->{data} // 0;
}

1;
"#;

/// Benchmark initial workspace indexing with a small workspace (5-10 files)
///
/// This verifies the <100ms initial indexing performance requirement from the roadmap.
fn bench_initial_index_small_workspace(c: &mut Criterion) {
    c.bench_function("initial index small workspace (5 files)", |b| {
        b.iter_batched(
            || {
                // Setup: create a temporary workspace with 5 files
                let temp_dir = must(TempDir::new());
                let base_path = temp_dir.path();

                // Create 5 different Perl files
                must(fs::write(base_path.join("module1.pm"), SAMPLE_MODULE));
                must(fs::write(base_path.join("module2.pm"), MULTI_PACKAGE_MODULE));
                must(fs::write(base_path.join("script.pl"), SAMPLE_SCRIPT));
                must(fs::write(base_path.join("modern.pm"), MODERN_PERL));
                must(fs::write(base_path.join("complex.pm"), COMPLEX_MODULE));

                (temp_dir, WorkspaceIndex::new())
            },
            |(temp_dir, index)| {
                // Benchmark: index all files
                let base_path = temp_dir.path();

                let uri1 = must(Url::from_file_path(base_path.join("module1.pm")));
                let uri2 = must(Url::from_file_path(base_path.join("module2.pm")));
                let uri3 = must(Url::from_file_path(base_path.join("script.pl")));
                let uri4 = must(Url::from_file_path(base_path.join("modern.pm")));
                let uri5 = must(Url::from_file_path(base_path.join("complex.pm")));

                index.index_file(uri1, SAMPLE_MODULE.to_string()).ok();
                index.index_file(uri2, MULTI_PACKAGE_MODULE.to_string()).ok();
                index.index_file(uri3, SAMPLE_SCRIPT.to_string()).ok();
                index.index_file(uri4, MODERN_PERL.to_string()).ok();
                index.index_file(uri5, COMPLEX_MODULE.to_string()).ok();

                black_box(&index);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark initial workspace indexing with a medium workspace (10 files)
fn bench_initial_index_medium_workspace(c: &mut Criterion) {
    c.bench_function("initial index medium workspace (10 files)", |b| {
        b.iter_batched(
            || {
                let temp_dir = must(TempDir::new());
                let base_path = temp_dir.path();

                // Create 10 files by duplicating samples with variations
                for i in 0..10 {
                    let filename = format!("module{}.pm", i);
                    let content = match i % 5 {
                        0 => SAMPLE_MODULE,
                        1 => MULTI_PACKAGE_MODULE,
                        2 => SAMPLE_SCRIPT,
                        3 => MODERN_PERL,
                        _ => COMPLEX_MODULE,
                    };
                    must(fs::write(base_path.join(&filename), content));
                }

                (temp_dir, WorkspaceIndex::new())
            },
            |(temp_dir, index)| {
                let base_path = temp_dir.path();

                for i in 0..10 {
                    let filename = format!("module{}.pm", i);
                    let uri = must(Url::from_file_path(base_path.join(&filename)));
                    let content = match i % 5 {
                        0 => SAMPLE_MODULE,
                        1 => MULTI_PACKAGE_MODULE,
                        2 => SAMPLE_SCRIPT,
                        3 => MODERN_PERL,
                        _ => COMPLEX_MODULE,
                    };
                    index.index_file(uri, content.to_string()).ok();
                }

                black_box(&index);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark incremental update (single file change in already-indexed workspace)
///
/// This verifies the <10ms incremental update performance requirement from the roadmap.
fn bench_incremental_update(c: &mut Criterion) {
    c.bench_function("incremental update single file", |b| {
        b.iter_batched(
            || {
                // Setup: create and index a workspace
                let temp_dir = must(TempDir::new());
                let base_path = temp_dir.path();

                must(fs::write(base_path.join("module1.pm"), SAMPLE_MODULE));
                must(fs::write(base_path.join("module2.pm"), MULTI_PACKAGE_MODULE));
                must(fs::write(base_path.join("script.pl"), SAMPLE_SCRIPT));
                must(fs::write(base_path.join("modern.pm"), MODERN_PERL));
                must(fs::write(base_path.join("complex.pm"), COMPLEX_MODULE));

                let index = WorkspaceIndex::new();

                // Initial indexing
                index
                    .index_file(
                        must(Url::from_file_path(base_path.join("module1.pm"))),
                        SAMPLE_MODULE.to_string(),
                    )
                    .ok();
                index
                    .index_file(
                        must(Url::from_file_path(base_path.join("module2.pm"))),
                        MULTI_PACKAGE_MODULE.to_string(),
                    )
                    .ok();
                index
                    .index_file(
                        must(Url::from_file_path(base_path.join("script.pl"))),
                        SAMPLE_SCRIPT.to_string(),
                    )
                    .ok();
                index
                    .index_file(
                        must(Url::from_file_path(base_path.join("modern.pm"))),
                        MODERN_PERL.to_string(),
                    )
                    .ok();
                index
                    .index_file(
                        must(Url::from_file_path(base_path.join("complex.pm"))),
                        COMPLEX_MODULE.to_string(),
                    )
                    .ok();

                let update_uri = must(Url::from_file_path(base_path.join("module1.pm")));

                (temp_dir, index, update_uri)
            },
            |(temp_dir, index, update_uri)| {
                // Benchmark: update a single file (simulating a user edit)
                let updated_content = r#"
package MyModule;
use strict;
use warnings;

our $VERSION = '2.00';  # Version changed

sub new {
    my $class = shift;
    return bless {}, $class;
}

sub process_data {
    my ($self, $data) = @_;

    for my $item (@$data) {
        my $result = $self->transform($item);
        print "Updated result: $result\n";  # Changed
    }

    return 1;
}

sub transform {
    my ($self, $value) = @_;
    return $value * 3;  # Changed multiplier
}

sub calculate {
    my ($self, $x, $y) = @_;
    return $x + $y;
}

sub new_method {  # New method added
    my ($self) = @_;
    return "new";
}

1;
"#;

                index.index_file(update_uri, updated_content.to_string()).ok();
                black_box(&index);
                black_box(temp_dir);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark symbol lookup (O(1) hash table lookup)
///
/// Tests the performance of find_definition, which should be very fast
/// due to hash table indexing.
fn bench_symbol_lookup(c: &mut Criterion) {
    // Setup: create an indexed workspace once for all iterations
    let temp_dir = must(TempDir::new());
    let base_path = temp_dir.path();

    must(fs::write(base_path.join("module1.pm"), SAMPLE_MODULE));
    must(fs::write(base_path.join("module2.pm"), MULTI_PACKAGE_MODULE));
    must(fs::write(base_path.join("complex.pm"), COMPLEX_MODULE));

    let index = WorkspaceIndex::new();

    index
        .index_file(
            must(Url::from_file_path(base_path.join("module1.pm"))),
            SAMPLE_MODULE.to_string(),
        )
        .ok();
    index
        .index_file(
            must(Url::from_file_path(base_path.join("module2.pm"))),
            MULTI_PACKAGE_MODULE.to_string(),
        )
        .ok();
    index
        .index_file(
            must(Url::from_file_path(base_path.join("complex.pm"))),
            COMPLEX_MODULE.to_string(),
        )
        .ok();

    c.bench_function("symbol lookup by name", |b| {
        b.iter(|| {
            // Benchmark: lookup various symbols
            let def1 = index.find_definition("MyModule::process_data");
            let def2 = index.find_definition("First::Module::first_sub");
            let def3 = index.find_definition("Complex::Module::method_one");

            black_box(def1);
            black_box(def2);
            black_box(def3);
        });
    });
}

/// Benchmark find_references (cross-file reference lookup)
fn bench_find_references(c: &mut Criterion) {
    // Setup: create an indexed workspace
    let temp_dir = must(TempDir::new());
    let base_path = temp_dir.path();

    must(fs::write(base_path.join("module1.pm"), SAMPLE_MODULE));
    must(fs::write(base_path.join("module2.pm"), MULTI_PACKAGE_MODULE));
    must(fs::write(base_path.join("complex.pm"), COMPLEX_MODULE));

    let index = WorkspaceIndex::new();

    index
        .index_file(
            must(Url::from_file_path(base_path.join("module1.pm"))),
            SAMPLE_MODULE.to_string(),
        )
        .ok();
    index
        .index_file(
            must(Url::from_file_path(base_path.join("module2.pm"))),
            MULTI_PACKAGE_MODULE.to_string(),
        )
        .ok();
    index
        .index_file(
            must(Url::from_file_path(base_path.join("complex.pm"))),
            COMPLEX_MODULE.to_string(),
        )
        .ok();

    c.bench_function("find references cross-file", |b| {
        b.iter(|| {
            // Benchmark: find all references to a symbol
            let refs = index.find_references("process_data");
            black_box(refs);
        });
    });
}

/// Benchmark workspace symbol search (fuzzy matching)
fn bench_workspace_symbol_search(c: &mut Criterion) {
    // Setup: create an indexed workspace
    let temp_dir = must(TempDir::new());
    let base_path = temp_dir.path();

    // Create multiple files with various symbols
    for i in 0..10 {
        let filename = format!("module{}.pm", i);
        let content = match i % 5 {
            0 => SAMPLE_MODULE,
            1 => MULTI_PACKAGE_MODULE,
            2 => SAMPLE_SCRIPT,
            3 => MODERN_PERL,
            _ => COMPLEX_MODULE,
        };
        must(fs::write(base_path.join(&filename), content));
    }

    let index = WorkspaceIndex::new();

    for i in 0..10 {
        let filename = format!("module{}.pm", i);
        let uri = must(Url::from_file_path(base_path.join(&filename)));
        let content = match i % 5 {
            0 => SAMPLE_MODULE,
            1 => MULTI_PACKAGE_MODULE,
            2 => SAMPLE_SCRIPT,
            3 => MODERN_PERL,
            _ => COMPLEX_MODULE,
        };
        index.index_file(uri, content.to_string()).ok();
    }

    c.bench_function("workspace symbol search", |b| {
        b.iter(|| {
            // Benchmark: search for symbols matching a query
            let results1 = index.search_symbols("process");
            let results2 = index.search_symbols("method");
            let results3 = index.search_symbols("sub");

            black_box(results1);
            black_box(results2);
            black_box(results3);
        });
    });
}

/// Benchmark file removal and re-indexing
fn bench_file_removal_and_reindex(c: &mut Criterion) {
    c.bench_function("file removal and re-index", |b| {
        b.iter_batched(
            || {
                // Setup: create and index a workspace
                let temp_dir = must(TempDir::new());
                let base_path = temp_dir.path();

                must(fs::write(base_path.join("module1.pm"), SAMPLE_MODULE));
                must(fs::write(base_path.join("module2.pm"), MULTI_PACKAGE_MODULE));
                must(fs::write(base_path.join("complex.pm"), COMPLEX_MODULE));

                let index = WorkspaceIndex::new();

                let uri1 = must(Url::from_file_path(base_path.join("module1.pm")));
                let uri2 = must(Url::from_file_path(base_path.join("module2.pm")));
                let uri3 = must(Url::from_file_path(base_path.join("complex.pm")));

                index.index_file(uri1.clone(), SAMPLE_MODULE.to_string()).ok();
                index.index_file(uri2.clone(), MULTI_PACKAGE_MODULE.to_string()).ok();
                index.index_file(uri3.clone(), COMPLEX_MODULE.to_string()).ok();

                (temp_dir, index, uri2)
            },
            |(temp_dir, index, uri_to_remove)| {
                // Benchmark: remove a file and re-index it
                index.remove_file_url(&uri_to_remove);
                index.index_file(uri_to_remove, MULTI_PACKAGE_MODULE.to_string()).ok();

                black_box(&index);
                black_box(temp_dir);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark state transitions in IndexCoordinator
///
/// Validates that state transitions are fast enough for LSP responsiveness (<1ms overhead)
fn bench_state_transitions(c: &mut Criterion) {
    use perl_workspace::workspace_index::IndexCoordinator;

    c.bench_function("state transitions", |b| {
        b.iter_batched(
            IndexCoordinator::new,
            |coordinator| {
                // Building → Ready
                coordinator.transition_to_ready(100, 5000);
                black_box(coordinator.state());

                // Ready → Building
                coordinator.transition_to_building(100);
                black_box(coordinator.state());

                // Building progress update
                coordinator.update_building_progress(50);
                black_box(coordinator.state());

                black_box(coordinator);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark state query operations (hot path)
///
/// Validates that state() calls are fast enough for every LSP request (<100ns)
fn bench_state_query(c: &mut Criterion) {
    use perl_workspace::workspace_index::IndexCoordinator;

    let coordinator = IndexCoordinator::new();
    coordinator.transition_to_ready(100, 5000);

    c.bench_function("state query (hot path)", |b| {
        b.iter(|| {
            let state = coordinator.state();
            black_box(state);
        });
    });
}

/// Benchmark parse storm detection and recovery
///
/// Validates that parse storm detection overhead is minimal (<10μs per notify)
fn bench_parse_storm_detection(c: &mut Criterion) {
    use perl_workspace::workspace_index::IndexCoordinator;

    c.bench_function("parse storm detection and recovery", |b| {
        b.iter_batched(
            || {
                let coordinator = IndexCoordinator::new();
                coordinator.transition_to_ready(100, 5000);
                coordinator
            },
            |coordinator| {
                // Trigger parse storm (15 changes, threshold is 10)
                for i in 0..15 {
                    coordinator.notify_change(&format!("file{}.pm", i));
                }

                // Check degradation
                black_box(coordinator.state());

                // Recover
                for i in 0..15 {
                    coordinator.notify_parse_complete(&format!("file{}.pm", i));
                }

                // Check recovery
                black_box(coordinator.state());
                black_box(coordinator);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark early-exit optimization (content hash check)
///
/// Validates that early-exit check is fast enough to be worth it (<1μs)
fn bench_early_exit_optimization(c: &mut Criterion) {
    c.bench_function("early exit content hash check", |b| {
        b.iter_batched(
            || {
                let temp_dir = must(TempDir::new());
                let base_path = temp_dir.path();
                must(fs::write(base_path.join("module1.pm"), SAMPLE_MODULE));

                let index = WorkspaceIndex::new();
                let uri = must(Url::from_file_path(base_path.join("module1.pm")));

                // Initial index
                index.index_file(uri.clone(), SAMPLE_MODULE.to_string()).ok();

                (temp_dir, index, uri)
            },
            |(temp_dir, index, uri)| {
                // Benchmark: re-index with same content (should early-exit)
                index.index_file(uri, SAMPLE_MODULE.to_string()).ok();

                black_box(&index);
                black_box(temp_dir);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark coordinator with resource limits enforcement
///
/// Validates that limit checking overhead is acceptable (<10μs per check)
fn bench_resource_limit_enforcement(c: &mut Criterion) {
    use perl_workspace::workspace_index::{IndexCoordinator, IndexResourceLimits};

    c.bench_function("resource limit enforcement", |b| {
        b.iter_batched(
            || {
                let limits = IndexResourceLimits {
                    max_files: 1000,
                    max_total_symbols: 50000,
                    ..Default::default()
                };
                let coordinator = IndexCoordinator::with_limits(limits);
                coordinator.transition_to_ready(500, 25000);
                coordinator
            },
            |coordinator| {
                // Check limits (this happens after every index operation)
                coordinator.enforce_limits();
                black_box(coordinator.state());
                black_box(coordinator);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark batch indexing 1000 files (CPAN-scale workload).
///
/// `generate_module` lives in `index_real_corpus` so the real-corpus slice
/// (issue #17159) shares the exact same synthetic shape.
fn bench_batch_index_1000_files(c: &mut Criterion) {
    c.bench_function("batch index 1000 files", |b| {
        b.iter_batched(
            || {
                let files: Vec<(Url, String)> = (0..1000)
                    .map(|i| {
                        let uri = must(Url::parse(&format!("file:///lib/Gen/Module{}.pm", i)));
                        (uri, index_real_corpus::generate_module(i))
                    })
                    .collect();
                (WorkspaceIndex::new(), files)
            },
            |(index, files)| {
                let errors = index.index_files_batch(files);
                black_box(errors);
                black_box(&index);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark symbol lookup at scale (after indexing 1000 files / ~5000 symbols).
fn bench_symbol_lookup_at_scale(c: &mut Criterion) {
    let index = WorkspaceIndex::new();
    let files: Vec<(Url, String)> = (0..1000)
        .map(|i| {
            let uri = must(Url::parse(&format!("file:///lib/Gen/Module{}.pm", i)));
            (uri, index_real_corpus::generate_module(i))
        })
        .collect();
    let _errors = index.index_files_batch(files);

    c.bench_function("symbol lookup at 1000-file scale", |b| {
        b.iter(|| {
            let d1 = index.find_definition("Gen::Module0::method_a_0");
            let d2 = index.find_definition("Gen::Module500::method_b_500");
            let d3 = index.find_definition("Gen::Module999::_private_999");
            black_box(d1);
            black_box(d2);
            black_box(d3);
        });
    });
}

/// Benchmark search_symbols at scale (substring match across ~5000 symbols).
fn bench_search_symbols_at_scale(c: &mut Criterion) {
    let index = WorkspaceIndex::new();
    let files: Vec<(Url, String)> = (0..1000)
        .map(|i| {
            let uri = must(Url::parse(&format!("file:///lib/Gen/Module{}.pm", i)));
            (uri, index_real_corpus::generate_module(i))
        })
        .collect();
    let _errors = index.index_files_batch(files);

    c.bench_function("search_symbols at 1000-file scale", |b| {
        b.iter(|| {
            let r = index.search_symbols("method_a");
            black_box(r);
        });
    });
}

fn populated_update_workspace() -> (WorkspaceIndex, Url) {
    let index = WorkspaceIndex::new();
    let files: Vec<(Url, String)> = (0..1000)
        .map(|i| {
            let uri = must(Url::parse(&format!("file:///lib/Gen/Module{}.pm", i)));
            (uri, index_real_corpus::generate_module(i))
        })
        .collect();
    let errors = index.index_initial_files_batch(files);
    assert!(errors.is_empty(), "update benchmark setup failed: {errors:?}");
    assert_eq!(index.file_count(), 1000);

    let update_uri = must(Url::parse("file:///lib/Gen/Module500.pm"));
    (index, update_uri)
}

/// Time only the live commit. Input ownership and the publication oracle stay
/// outside the timer, though the oracle still affects between-iteration caches.
fn checked_workspace_update(
    index: &WorkspaceIndex,
    uri: &Url,
    variant: &(String, &str, &str),
    generation: &mut u32,
    expected_outcome: SourceCommitOutcome,
) -> Duration {
    *generation = must_some(generation.checked_add(1));
    // DocumentStore uses i32 versions; exhaustion must refuse the benchmark.
    assert!(*generation <= i32::MAX as u32, "update benchmark generation exhausted");
    let commit = SourceCommit::new(must_some(NonZeroU32::new(*generation)));
    let candidate_uri = uri.clone();
    let candidate_source = variant.0.clone();

    let start = Instant::now();
    let outcome = index.index_live_file(candidate_uri, candidate_source, commit);
    let elapsed = start.elapsed();

    assert_eq!(outcome, expected_outcome);
    assert_eq!(index.file_count(), 1000);
    assert_eq!(index.indexed_generation(uri.as_str()), Some(*generation));
    let document = must_some(index.document_store().get(uri.as_str()));
    assert_eq!(document.text(), variant.0.as_str());
    let definition = must_some(index.find_definition(variant.1));
    assert_eq!(definition.uri, uri.as_str());
    assert!(index.find_definition(variant.2).is_none(), "obsolete declaration survived update");
    elapsed
}

/// Live workspace commit latency, excluding caller-side source preparation and
/// editor processing. These cases replace the old mostly-NoOp "incremental
/// update" timing; its historical numbers are not real-edit baselines.
fn bench_incremental_update_at_scale(c: &mut Criterion) {
    let module = index_real_corpus::generate_module(500);
    assert!(module.contains("sub method_a_500 "));
    let variants = [
        (
            module.replace("method_a_500", "edited_a_500"),
            "Gen::Module500::edited_a_500",
            "Gen::Module500::edited_b_500",
        ),
        (
            module.replace("method_a_500", "edited_b_500"),
            "Gen::Module500::edited_b_500",
            "Gen::Module500::edited_a_500",
        ),
    ];

    for (name, changes_content) in [
        ("accepted real update at 1000-file scale", true),
        ("unchanged live commit at 1000-file scale", false),
    ] {
        // Each case owns an independently populated index.
        let (index, update_uri) = populated_update_workspace();
        let mut generation = 0;
        // Preflight both edit directions and the unchanged-content control.
        // None of this fixture validation contributes to Criterion's duration.
        for variant in [0, 1, 0] {
            checked_workspace_update(
                &index,
                &update_uri,
                &variants[variant],
                &mut generation,
                SourceCommitOutcome::Accepted,
            );
        }
        assert!(index.find_definition("Gen::Module500::method_a_500").is_none());
        checked_workspace_update(
            &index,
            &update_uri,
            &variants[0],
            &mut generation,
            SourceCommitOutcome::NoOp,
        );

        // Preserve accepted state across warmup and sampling callbacks.
        let mut active_variant = 0;
        c.bench_function(name, |b| {
            b.iter_custom(|iters| {
                let mut elapsed = Duration::ZERO;
                for _ in 0..iters {
                    let next_variant =
                        if changes_content { 1 - active_variant } else { active_variant };
                    let expected = if changes_content {
                        SourceCommitOutcome::Accepted
                    } else {
                        SourceCommitOutcome::NoOp
                    };
                    elapsed += checked_workspace_update(
                        &index,
                        &update_uri,
                        &variants[next_variant],
                        &mut generation,
                        expected,
                    );
                    active_variant = next_variant;
                }
                elapsed
            });
        });
    }
}

/// Generate a dense Perl module with ~100 symbols for CPAN-scale 500K-symbol testing.
///
/// Each module defines `new` + 96 numbered methods + 3 accessors = ~100 symbols.
/// At 5K files this yields ~500K total symbols, matching the acceptance criterion.
fn generate_dense_module(index: usize) -> String {
    let mut src = format!(
        "package Gen::Dense{idx};\nuse strict;\nour $VERSION = '1.00';\n\nsub new {{ bless {{}}, shift }}\n",
        idx = index
    );
    for j in 0..96 {
        src.push_str(&format!("sub method_{idx}_{j} {{ return {j}; }}\n", idx = index, j = j));
    }
    src.push_str(&format!(
        "sub get_{idx} {{ return {idx}; }}\nsub set_{idx} {{ my ($self, $v) = @_; }}\nsub reset_{idx} {{ }}\n1;\n",
        idx = index
    ));
    src
}

/// Benchmark batch indexing 10K sparse files (~5 symbols each = ~50K total symbols).
///
/// Validates: index 10K files in <30s (serial parse-bound).
/// Acceptance criterion: startup time at true CPAN scale.
fn bench_batch_index_10k_files_sparse(c: &mut Criterion) {
    c.bench_function("batch index 10K sparse files (50K symbols)", |b| {
        b.iter_batched(
            || {
                (0..10_000)
                    .map(|i| {
                        let uri = must(Url::parse(&format!("file:///lib/Gen/Sparse{}.pm", i)));
                        (uri, index_real_corpus::generate_module(i))
                    })
                    .collect::<Vec<_>>()
            },
            |files| {
                let index = WorkspaceIndex::new();
                let errors = index.index_files_batch(files);
                black_box(errors);
                black_box(&index);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark batch indexing 5K dense files (~100 symbols each = ~500K total symbols).
///
/// Validates: index 5K dense files in <30s.
/// Acceptance criterion: throughput at 500K-symbol scale.
fn bench_batch_index_5k_files_dense(c: &mut Criterion) {
    c.bench_function("batch index 5K dense files (500K symbols)", |b| {
        b.iter_batched(
            || {
                (0..5_000)
                    .map(|i| {
                        let uri = must(Url::parse(&format!("file:///lib/Gen/Dense{}.pm", i)));
                        (uri, generate_dense_module(i))
                    })
                    .collect::<Vec<_>>()
            },
            |files| {
                let index = WorkspaceIndex::new();
                let errors = index.index_files_batch(files);
                black_box(errors);
                black_box(&index);
            },
            BatchSize::SmallInput,
        );
    });
}

/// Benchmark symbol lookup at 500K-symbol scale.
///
/// Validates: query latency <50ms at 500K symbols.
/// Acceptance criterion: find_definition response time stays O(1) regardless of corpus size.
fn bench_symbol_lookup_at_500k_scale(c: &mut Criterion) {
    let index = WorkspaceIndex::new();
    let files: Vec<(Url, String)> = (0..5_000)
        .map(|i| {
            let uri = must(Url::parse(&format!("file:///lib/Gen/Dense{}.pm", i)));
            (uri, generate_dense_module(i))
        })
        .collect();
    let _errors = index.index_files_batch(files);

    c.bench_function("symbol lookup at 500K-symbol scale", |b| {
        b.iter(|| {
            let d1 = index.find_definition("Gen::Dense0::method_0_0");
            let d2 = index.find_definition("Gen::Dense2500::method_2500_50");
            let d3 = index.find_definition("Gen::Dense4999::method_4999_95");
            black_box(d1);
            black_box(d2);
            black_box(d3);
        });
    });
}

/// Benchmark slice #5 (issue #17159, benchmark matrix row 5): cold scan+index
/// wallclock on a real corpus.
///
/// One COLD sample = stage a fresh corpus copy into a `TempDir` (no `.git`,
/// so discovery exercises its `WalkDir` fallback against the corpus rather
/// than the host repository index), then time, in production startup order,
/// discovery + per-file read/admit/decode/`index_file` with a fresh
/// `WorkspaceIndex`. See `index_real_corpus` for the precise contract.
///
/// The real skeletons are skipped with a diagnostic when
/// `test_corpus/real_projects` is absent (matrix: "skip if absent"); the
/// deterministic synthetic 400-file tree never skips.
fn bench_index_real_corpus(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_real_corpus");
    // A cold sample copies up to 400 files and re-parses every one of them;
    // the default 100-sample criterion run would dominate wall time.
    group.sample_size(10);

    let projects = index_real_corpus::real_corpus_projects();
    if projects.is_empty() {
        // Diagnostic for the bench operator; benches are not the LSP server's
        // stdio transport, so the workspace print_stderr deny is relaxed here.
        #[allow(clippy::print_stderr)]
        {
            eprintln!(
                "index_real_corpus: {} absent; skipping real-corpus entries (synthetic entry still runs)",
                index_real_corpus::REAL_PROJECTS_RELATIVE
            );
        }
    }

    for project in &projects {
        let Some(name) = project.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let source_root = project.clone();
        group.bench_function(format!("real_corpus/{name}"), |b| {
            b.iter_batched(
                || {
                    // Setup (untimed): fresh corpus copy per sample. The fresh
                    // WorkspaceIndex is created inside `cold_scan_index`,
                    // before its timer starts.
                    let temp_dir = must(TempDir::new());
                    must(index_real_corpus::stage_copy(&source_root, temp_dir.path()));
                    temp_dir
                },
                |temp_dir| {
                    // A failed cold sample invalidates the run: surface it via
                    // the bench's house `must` diagnostic, then consume it.
                    let sample = must(index_real_corpus::cold_scan_index(temp_dir.path()));
                    black_box(sample);
                    // Keep the tree alive until the sample completes (:361 pattern).
                    black_box(temp_dir);
                },
                BatchSize::PerIteration,
            );
        });
    }

    // Deterministic synthetic on-disk tree, never skipped: 40 package dirs x
    // 10 modules = 400 files from the shared generator.
    group.bench_function("real_corpus/synthetic_400_files", |b| {
        b.iter_batched(
            || {
                let temp_dir = must(TempDir::new());
                must(index_real_corpus::write_synthetic_tree(temp_dir.path()));
                temp_dir
            },
            |temp_dir| {
                let sample = must(index_real_corpus::cold_scan_index(temp_dir.path()));
                black_box(sample);
                black_box(temp_dir);
            },
            BatchSize::PerIteration,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_initial_index_small_workspace,
    bench_initial_index_medium_workspace,
    bench_incremental_update,
    bench_symbol_lookup,
    bench_find_references,
    bench_workspace_symbol_search,
    bench_file_removal_and_reindex,
    bench_state_transitions,
    bench_state_query,
    bench_parse_storm_detection,
    bench_early_exit_optimization,
    bench_resource_limit_enforcement,
    bench_batch_index_1000_files,
    bench_symbol_lookup_at_scale,
    bench_search_symbols_at_scale,
    bench_incremental_update_at_scale,
    bench_batch_index_10k_files_sparse,
    bench_batch_index_5k_files_dense,
    bench_symbol_lookup_at_500k_scale,
    bench_index_real_corpus,
);
criterion_main!(benches);
