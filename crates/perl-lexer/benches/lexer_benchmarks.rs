//! Lexer throughput benchmarks. Reports the scorecard path on stdout, which is
//! how the harness surfaces it, so the workspace-wide `print_stdout = "deny"`
//! lint is opted out of here rather than worked around.
#![allow(clippy::print_stdout)]

use criterion::{BenchmarkId, Criterion, Throughput};
use perl_lexer::{LexerConfig, LocalSymbolTable, PerlLexer, Token};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const SCORECARD_FAMILIES: [&str; 8] = [
    "simple_tokens",
    "slash_disambiguation",
    "string_interpolation",
    "large_file",
    "whitespace_heavy",
    "operator_heavy",
    "number_parsing",
    "keyword_heavy",
];
const SCORECARD_ARTIFACT_PATH: &str = "benchmarks/results/lexer_scorecard.json";

#[derive(Default)]
struct BenchAccumulator {
    total_duration_ns: u128,
    total_iterations: u64,
    total_tokens: u64,
    sample_count: u64,
    min_sample_ns: u128,
    max_sample_ns: u128,
}

impl BenchAccumulator {
    fn update(&mut self, iterations: u64, elapsed: Duration, tokens_per_iteration: u64) {
        let sample_ns = elapsed.as_nanos();
        self.total_duration_ns = self.total_duration_ns.saturating_add(sample_ns);
        self.total_iterations = self.total_iterations.saturating_add(iterations);
        self.total_tokens =
            self.total_tokens.saturating_add(tokens_per_iteration.saturating_mul(iterations));
        self.sample_count = self.sample_count.saturating_add(1);

        if self.sample_count == 1 || sample_ns < self.min_sample_ns {
            self.min_sample_ns = sample_ns;
        }
        if sample_ns > self.max_sample_ns {
            self.max_sample_ns = sample_ns;
        }
    }

    fn to_json_value(&self) -> Value {
        let total_duration_ns = self.total_duration_ns as f64;
        let total_iterations = self.total_iterations as f64;
        let total_tokens = self.total_tokens as f64;

        let mean_sample_ns = if self.sample_count > 0 {
            self.total_duration_ns / u128::from(self.sample_count)
        } else {
            0
        };

        let ns_per_iteration =
            if self.total_iterations > 0 { total_duration_ns / total_iterations } else { 0.0 };

        let tokens_per_second = if self.total_duration_ns > 0 {
            total_tokens / (total_duration_ns / 1_000_000_000.0)
        } else {
            0.0
        };

        serde_json::json!({
            "sample_count": self.sample_count,
            "total_iterations": self.total_iterations,
            "total_time_ns": self.total_duration_ns,
            "mean_sample_ns": mean_sample_ns,
            "min_sample_ns": self.min_sample_ns,
            "max_sample_ns": self.max_sample_ns,
            "ns_per_iteration": ns_per_iteration,
            "total_tokens": self.total_tokens,
            "tokens_per_second": tokens_per_second,
        })
    }
}

static SCORECARD: LazyLock<Mutex<BTreeMap<String, BenchAccumulator>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn collect_all_tokens(mut lexer: PerlLexer) -> Vec<Token> {
    lexer.collect_tokens()
}

fn record_benchmark_sample(
    family: &'static str,
    iterations: u64,
    elapsed: Duration,
    tokens_per_iteration: u64,
) {
    if let Ok(mut scorecard) = SCORECARD.lock() {
        scorecard.entry(family.to_string()).or_default().update(
            iterations,
            elapsed,
            tokens_per_iteration,
        );
    }
}

fn bench_family(c: &mut Criterion, family: &'static str, input: &'static str) {
    let tokens_per_iteration = collect_all_tokens(PerlLexer::new(input)).len() as u64;

    c.bench_function(family, |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let lexer = PerlLexer::new(black_box(input));
                black_box(collect_all_tokens(lexer));
            }
            let elapsed = start.elapsed();
            record_benchmark_sample(family, iters, elapsed, tokens_per_iteration);
            elapsed
        });
    });
}

fn bench_simple_tokens(c: &mut Criterion) {
    bench_family(c, "simple_tokens", "my $x = 42; print $x;");
}

fn bench_slash_disambiguation(c: &mut Criterion) {
    bench_family(
        c,
        "slash_disambiguation",
        r#"
        my $x = 10 / 2;
        if ($str =~ /pattern/) {
            $str =~ s/foo/bar/g;
        }
        print 1/ /abc/;
    "#,
    );
}

fn bench_string_interpolation(c: &mut Criterion) {
    bench_family(
        c,
        "string_interpolation",
        r#"
        my $name = "World";
        print "Hello, $name!\n";
        print "The answer is ${count + 1}\n";
        print "Array: @items\n";
    "#,
    );
}

fn bench_large_file(c: &mut Criterion) {
    let mut input = String::new();
    for i in 0..1000 {
        input.push_str(&format!("my $var{} = {};\n", i, i));
        input.push_str(&format!("print \"Value: $var{}\n\";\n", i));
        if i % 10 == 0 {
            input.push_str(&format!("if ($var{} =~ /\\d+/) {{\n", i));
            input.push_str(&format!("    $var{} = $var{} / 2;\n", i, i));
            input.push_str("}\n");
        }
    }

    let leaked_input: &'static str = Box::leak(input.into_boxed_str());
    bench_family(c, "large_file", leaked_input);
}

fn bench_whitespace_heavy(c: &mut Criterion) {
    bench_family(
        c,
        "whitespace_heavy",
        r#"
    # This is a comment
    my   $x   =   42  ;  # Another comment

    print    $x    ;

    # More comments
    "#,
    );
}

fn bench_operator_heavy(c: &mut Criterion) {
    bench_family(
        c,
        "operator_heavy",
        "$a += $b -= $c *= $d /= $e %= $f **= $g &&= $h ||= $i //= $j",
    );
}

fn bench_number_parsing(c: &mut Criterion) {
    bench_family(c, "number_parsing", "123 456.789 1_234_567 1.23e45 0xFF 0377 0b1010");
}

fn bench_keyword_heavy(c: &mut Criterion) {
    let base =
        "if else while until for foreach return last next redo package require default continue";
    let leaked_input: &'static str = Box::leak(base.repeat(100).into_boxed_str());
    bench_family(c, "keyword_heavy", leaked_input);
}

/// Build the heredoc-prepass fixture family (#18031).
///
/// `heredoc_heavy` interleaves `<<~` heredoc blocks with `sub` declarations and
/// ordinary code. Both opener shapes that make the prepass consult the table
/// appear: a term-position initializer (`my $template = <<~TPL_n;`) and a
/// callable bareword (`finish_block_n(<<~CFG_n, $count_n);`) whose declaration
/// only lands *after* the call — the forward reference that exists only because
/// the prepass runs two passes. Each body carries `sub`-shaped prose the scan
/// must keep out of the table, and a declared nullary call (`counter_n << 4`)
/// pins the left-shift control so the nullary lookup path is paid in both
/// variants. The fixture is strict-clean Perl: each block hoists a top-level
/// `my $count_n` covering the top-level, heredoc-interpolation, and
/// `finish_block_n`-body references, and the bareword `strict` argument is
/// quoted (#18062 review). The control keeps the same declarations and code
/// shape with the heredoc blocks replaced by ordinary lines, so the
/// disambiguation cost is visible as the delta between the two cases.
fn heredoc_fixture_source(heredoc_heavy: bool) -> String {
    const BLOCKS: usize = 64;
    let mut source = String::from("use strict;\nuse warnings;\n\n");
    for i in 0..BLOCKS {
        source.push_str(&format!("my $count{i} = {i};\n"));
        source.push_str(&format!(
            "sub render_block_{i} {{\n    my ($self, $count{i}) = @_;\n    return $count{i} * {i};\n}}\n"
        ));
        source.push_str(&format!("sub counter_{i} () {{ {i} }}\n"));
        source.push_str(&format!("my $shifted_{i} = counter_{i} << 4;\n"));
        if heredoc_heavy {
            source.push_str(&format!("my $template_{i} = <<~TPL_{i};\n"));
            source.push_str(&format!("    block {i} header\n"));
            source.push_str(&format!("    sub fake_body_{i} {{ }}\n"));
            source.push_str(&format!("    value => $count{i}\n"));
            source.push_str(&format!("    TPL_{i}\n"));
            source.push_str(&format!("finish_block_{i}(<<~CFG_{i}, $count{i});\n"));
            source.push_str(&format!("    mode => 'strict',\n    sub fake_cfg_{i} {{ }}\n"));
            source.push_str(&format!("    CFG_{i}\n"));
        } else {
            source.push_str(&format!("my $template_{i} = \"block {i}: value => $count{i}\";\n"));
            source.push_str(&format!("finish_block_{i}('strict', $count{i});\n"));
        }
        source.push_str(&format!("sub finish_block_{i} {{ $count{i} + {i} }}\n"));
        source.push_str(&format!("print \"block {i} done\\n\";\n"));
        source.push_str(&format!("if ($count{i} > {i}) {{\n    $count{i} /= 2;\n}}\n\n"));
    }
    source
}

/// Benchmark the `LocalSymbolTable` heredoc-disambiguation prepass (#18031).
///
/// `PerlLexer::new` leaves `symbol_table` unset and never runs the prepass;
/// `scan_subs` is invoked by perl-parser-core's `ParserContext::new`. Each case
/// therefore drives the production path that constructs the table —
/// `LocalSymbolTable::scan_subs` followed by `PerlLexer::with_config` with the
/// table installed — and collects the full token stream, so the measured unit
/// covers the two-pass scan, the pending-heredoc bookkeeping, and the lexer's
/// per-word callable/nullary lookups. The `heredoc_heavy` versus
/// `no_heredoc_control` delta is the heredoc-disambiguation cost.
fn bench_heredoc_prepass(c: &mut Criterion) {
    let mut group = c.benchmark_group("heredoc_prepass");

    for (case, source) in [
        ("heredoc_heavy", heredoc_fixture_source(true)),
        ("no_heredoc_control", heredoc_fixture_source(false)),
    ] {
        let leaked_source: &'static str = Box::leak(source.into_boxed_str());
        group.throughput(Throughput::Bytes(leaked_source.len() as u64));
        group.bench_with_input(
            BenchmarkId::new(case, leaked_source.len() as u64),
            &leaked_source,
            |b, input| {
                let input = *input;
                b.iter(|| {
                    let table = LocalSymbolTable::scan_subs(black_box(input));
                    let config =
                        LexerConfig { symbol_table: Some(table), ..LexerConfig::default() };
                    black_box(collect_all_tokens(PerlLexer::with_config(black_box(input), config)));
                });
            },
        );
    }

    group.finish();
}

fn run_all_benchmarks(c: &mut Criterion) {
    bench_simple_tokens(c);
    bench_slash_disambiguation(c);
    bench_string_interpolation(c);
    bench_large_file(c);
    bench_whitespace_heavy(c);
    bench_operator_heavy(c);
    bench_number_parsing(c);
    bench_keyword_heavy(c);
    bench_heredoc_prepass(c);
}

fn benchmark_scorecard_output_path() -> PathBuf {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_root.parent().and_then(Path::parent).map_or_else(
        || PathBuf::from(SCORECARD_ARTIFACT_PATH),
        |root| root.join(SCORECARD_ARTIFACT_PATH),
    )
}

fn build_scorecard_payload() -> Value {
    let mut families = Map::new();

    if let Ok(scorecard) = SCORECARD.lock() {
        for family in SCORECARD_FAMILIES {
            let value = scorecard
                .get(family)
                .map(BenchAccumulator::to_json_value)
                .unwrap_or_else(|| BenchAccumulator::default().to_json_value());
            families.insert(family.to_string(), value);
        }
    }

    serde_json::json!({
        "schema_version": 1,
        "scorecard": "perl_lexer_performance",
        "artifact": "lexer_scorecard",
        "families": families,
    })
}

fn emit_benchmark_scorecard() {
    let output_path = benchmark_scorecard_output_path();
    if let Some(parent) = output_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let payload = build_scorecard_payload();
    if let Ok(body) = serde_json::to_string_pretty(&payload) {
        let _ = fs::write(&output_path, body);
    }

    println!("lexer benchmark scorecard: {}", output_path.display());
}

fn main() {
    let mut criterion = Criterion::default().configure_from_args();
    run_all_benchmarks(&mut criterion);
    criterion.final_summary();
    emit_benchmark_scorecard();
}
