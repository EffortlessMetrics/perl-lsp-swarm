# Benchmark Results

This directory contains published benchmark results for the Perl LSP project.

## File Naming Convention

```
<date>-<machine>-<component>.json
```

Examples:
- `2026-01-22-ryzen9-9950x3d-parser.json` - Parser benchmarks on AMD Ryzen 9 9950X3D
- `2026-01-22-ryzen9-9950x3d-incremental.json` - Incremental parsing benchmarks
- `2026-01-22-ryzen9-9950x3d-lsp.json` - LSP server performance benchmarks

## Published Results

### 2026-01-22: v0.9.1 Baseline (AMD Ryzen 9 9950X3D)

**Machine Configuration:**
- CPU: AMD Ryzen 9 9950X3D 16-Core Processor (32 threads)
- Memory: 196 GiB
- OS: Linux 6.6.87.2-microsoft-standard-WSL2
- Rust: rustc 1.91.1 (ed61e7d7e 2025-11-07)

**Results:**
- **Parser Benchmarks**: `/benchmarks/results/2026-01-22-ryzen9-9950x3d-parser.json`
  - `parse_simple_script`: 21.26μs mean

**Status:**
- Parser performance baseline established
- Incremental parsing benchmarks pending
- LSP server performance tests pending
- Workspace indexing data available in `/target/criterion/`

## Performance Targets

### Parser Performance
- **Target**: 1-150μs parsing time
- **v0.9.1 Baseline**: 21.26μs mean (ACHIEVED)

### Incremental Parsing
- **Target**: <1ms updates with 70-99% node reuse
- **v0.9.1 Baseline**: Pending

### LSP Server Performance
- **Behavioral Tests**: <1s execution (0.31s achieved)
- **User Story Tests**: <1s execution (0.32s achieved)
- **Workspace Navigation**: <1s individual tests (0.26s achieved)
- **v0.9.1 Baseline**: Pending

## Running Benchmarks

See `/benchmarks/BENCHMARK_FRAMEWORK.md` for comprehensive documentation.

**Quick reference:**
```bash
# Parser benchmarks
cargo bench -p perl-parser --bench parser_benchmark

# Incremental parsing
cargo bench -p perl-parser --bench incremental_benchmark --features incremental

# LSP performance (via release tests)
RUST_TEST_THREADS=2 cargo test --release -p perl-lsp
```

## Interpreting Results

All times in the JSON files are reported in nanoseconds unless otherwise specified.

**Conversion reference:**
- 1,000 nanoseconds = 1 microsecond (μs)
- 1,000 microseconds = 1 millisecond (ms)
- 1,000 milliseconds = 1 second (s)

**Confidence intervals:**
- All measurements include 95% confidence intervals
- Lower/upper bounds represent the range of likely true values

**Outliers:**
- Measurements significantly different from the mean
- Categorized as mild or severe
- High outliers may indicate GC pauses or system interference

## Historical Baselines

Future benchmark runs will be added to this directory for regression tracking.

**Regression detection:**
```bash
# Compare against v0.9.1 baseline
cargo bench -p perl-parser --bench parser_benchmark -- --baseline v0.9.1-baseline
```

---

Last Updated: 2026-01-22

## Receipt template

Every new committed benchmark result should carry enough metadata to reproduce
and interpret the measurement. Use the YAML shape below for an adjacent Markdown/YAML receipt. For a `.json`
file, use the valid JSON form that follows:

    schema: benchmark-result.v1
    change:
      issue: "#<issue>"
      pr: "#<pr>"
      before_sha: "<40-char SHA or null for a new baseline>"
      after_sha: "<40-char SHA>"
    command: "<exact command and arguments>"
    machine:
      os: "<name and version>"
      kernel: "<version>"
      cpu: "<model>"
      memory_gib: <number>
      architecture: "<target triple>"
    runtime:
      rustc: "<rustc -Vv summary>"
      cargo_target_dir: "<path policy or isolated>"
      relevant_env: []
    result:
      artifact: "<repository-relative path>"
      summary: "<what was measured>"
      comparison: "<baseline and delta, or not applicable>"
      status: "measured|not_comparable|not_proven"

Do not put a target number in a receipt unless the command and baseline make
the comparison meaningful. Link the result back to the initiating issue or
PR, and keep generated/raw output separate from the short interpretation.


### JSON form

```json
{
  "schema": "benchmark-result.v1",
  "change": {
    "issue": "#<issue>",
    "pr": "#<pr>",
    "before_sha": "<40-char SHA or null for a new baseline>",
    "after_sha": "<40-char SHA>"
  },
  "command": "<exact command and arguments>",
  "machine": {
    "os": "<name and version>",
    "kernel": "<version>",
    "cpu": "<model>",
    "memory_gib": 0,
    "architecture": "<target triple>"
  },
  "runtime": {
    "rustc": "<rustc -Vv summary>",
    "cargo_target_dir": "<path policy or isolated>",
    "relevant_env": []
  },
  "result": {
    "artifact": "<repository-relative path>",
    "summary": "<what was measured>",
    "comparison": "<baseline and delta, or not applicable>",
    "status": "measured"
  }
}
```
