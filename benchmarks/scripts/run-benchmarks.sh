#!/bin/bash
# Benchmark runner with structured JSON output
#
# Usage:
#   ./run-benchmarks.sh                    # Run all, output to stdout
#   ./run-benchmarks.sh --output out.json  # Save to file
#   ./run-benchmarks.sh --quick            # Quick smoke test
#   ./run-benchmarks.sh --category parser  # Run specific category

set -euo pipefail

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Defaults
OUTPUT_FILE=""
QUICK_MODE=false
CATEGORY=""
VERBOSE=false

# Space-separated categories whose cargo run failed. A failed category is
# marked explicitly in the JSON and fails the process at the end (#17218).
FAILED_CATEGORIES=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --output|-o)
            OUTPUT_FILE="$2"
            shift 2
            ;;
        --quick|-q)
            QUICK_MODE=true
            shift
            ;;
        --category|-c)
            CATEGORY="$2"
            shift 2
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --output, -o FILE    Save results to JSON file"
            echo "  --quick, -q          Run quick smoke benchmarks"
            echo "  --category, -c CAT   Run specific category (parser, lexer, lsp, index, ripr)"
            echo "  --verbose, -v        Show detailed output"
            echo "  --help, -h           Show this help"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

cd "$REPO_ROOT"

# Get environment info
GIT_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
GIT_DIRTY=$(git diff --quiet 2>/dev/null && echo "false" || echo "true")
RUST_VERSION=$(rustc --version | cut -d' ' -f2)
OS_NAME=$(uname -s)
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Criterion args
CRITERION_ARGS="--locked"
if $QUICK_MODE; then
    CRITERION_ARGS="$CRITERION_ARGS -- --warm-up-time 1 --measurement-time 2 --sample-size 20"
fi

log() {
    if $VERBOSE; then
        echo "[$(date +%H:%M:%S)] $*" >&2
    fi
}

# Convert Criterion timing values to integer nanoseconds.
# Uses awk for floating-point arithmetic to avoid requiring `bc`.
to_nanoseconds() {
    local value=$1
    local unit=$2
    local multiplier=1

    case $unit in
        ns) multiplier=1 ;;
        us|µs) multiplier=1000 ;;
        ms) multiplier=1000000 ;;
        s)  multiplier=1000000000 ;;
        *)
            echo ""
            return 1
            ;;
    esac

    awk -v value="$value" -v mult="$multiplier" 'BEGIN { printf "%.0f\n", value * mult }'
}

# Escape a single-line string for embedding in a JSON double-quoted value.
json_escape() {
    printf '%s' "$1" | tr '\t' ' ' | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' | tr -d '\001-\010\013\014\016-\037'
}

# Extract a one-line excerpt of cargo's stderr from a captured bench log so a
# failed category records the proximate error instead of only the exit code
# (#17425). Prefers the first `error` line plus its hint context; falls back
# to the last non-empty line when cargo printed no error line.
cargo_stderr_excerpt() {
    local log_file=$1
    local excerpt
    excerpt=$(grep -aA2 -m1 -E '^error' "$log_file" | tr '\n' ' ' | sed -e 's/  */ /g' -e 's/^ *//; s/ *$//')
    if [[ -z "$excerpt" ]]; then
        excerpt=$(grep -avE '^[[:space:]]*$' "$log_file" | tail -n 1)
    fi
    json_escape "$excerpt"
}

# Function to run benchmarks and extract results
run_criterion_bench() {
    local crate=$1
    local bench=$2
    local category=$3
    local features=${4:-}

    log "Running $crate::$bench..."

    # Create temp file for output
    local temp_output
    temp_output=$(mktemp)

    # Benches with required-features (e.g. the workspace index bench) need
    # those features passed explicitly; every other bench keeps its default
    # feature set (#17425). Mirrors the crate:bench:feature triples in
    # .github/workflows/ci-nightly.yml.
    local features_args=""
    if [[ -n "$features" ]]; then
        features_args="--features $features"
    fi

    # Run benchmark, capturing output
    if cargo bench -p "$crate" --bench "$bench" $features_args $CRITERION_ARGS 2>&1 | tee "$temp_output" > /dev/null; then
        # Parse criterion output for timing.
        # Single-line layout: "parse_simple  time:   [45.123 us 45.234 us 45.345 us]"
        # Long-name layout (#17219): the bench name stands alone on its own
        # line and the time triple follows on the next line:
        #   "packet_fingerprint/large"
        #   "  time:   [206.67 µs 213.06 µs 218.91 µs]"
        # Names are captured whole (spaces, parens, dots included): a pending
        # bare name is consumed only by an immediately following unit triple,
        # and any other line clears it, so diagnostic lines can never donate
        # a stale name. Lines with colons (Benchmarking/time/thrpt/change)
        # are never pending-name candidates.
        local pending_name=""
        while IFS= read -r line; do
            if [[ $line =~ time:[[:space:]]+\[([0-9.]+)[[:space:]]+(ns|us|µs|ms|s)[[:space:]]+([0-9.]+)[[:space:]]+(ns|us|µs|ms|s)[[:space:]]+([0-9.]+)[[:space:]]+(ns|us|µs|ms|s)\] ]]; then
                # Capture the triple first: the inline-name test below
                # overwrites BASH_REMATCH.
                local low="${BASH_REMATCH[1]}"
                local low_unit="${BASH_REMATCH[2]}"
                local mean="${BASH_REMATCH[3]}"
                local mean_unit="${BASH_REMATCH[4]}"
                local high="${BASH_REMATCH[5]}"
                local high_unit="${BASH_REMATCH[6]}"
                local bench_name=""
                if [[ $line =~ ^(.*[^[:space:]])[[:space:]]+time: ]]; then
                    bench_name="${BASH_REMATCH[1]}"
                elif [[ -n "$pending_name" ]]; then
                    bench_name="$pending_name"
                else
                    continue
                fi
                pending_name=""

                local mean_ns
                mean_ns=$(to_nanoseconds "$mean" "$mean_unit") || continue
                local low_ns
                low_ns=$(to_nanoseconds "$low" "$low_unit") || continue
                local high_ns
                high_ns=$(to_nanoseconds "$high" "$high_unit") || continue

                echo "      \"$bench_name\": {"
                echo "        \"mean_ns\": $mean_ns,"
                echo "        \"low_ns\": $low_ns,"
                echo "        \"high_ns\": $high_ns,"
                echo "        \"unit\": \"$mean_unit\","
                echo "        \"display\": \"$mean $mean_unit\""
                echo "      },"
            elif [[ $line =~ ^[^[:space:]:][^:]*$ ]]; then
                pending_name="$line"
            else
                pending_name=""
            fi
        done < "$temp_output"
    else
        # Cargo failed (wrong toolchain, compile break, missing target):
        # mark the category explicitly instead of rendering a silent empty
        # stub. Downstream counters skip underscore keys (#17218).
        local cargo_status=$?
        local cargo_err
        cargo_err=$(cargo_stderr_excerpt "$temp_output")
        echo "      \"_status\": \"failed\","
        echo "      \"_error\": \"cargo bench -p $crate --bench $bench failed (exit $cargo_status): $cargo_err\","
        FAILED_CATEGORIES="${FAILED_CATEGORIES:+$FAILED_CATEGORIES }$category"
        log "FAILED $crate::$bench (exit $cargo_status): $cargo_err"
    fi

    rm -f "$temp_output"
}

# Start JSON output
json_output() {
    echo "{"
    echo "  \"version\": \"0.9.0\","
    echo "  \"timestamp\": \"$TIMESTAMP\","
    echo "  \"git_sha\": \"$GIT_SHA\","
    echo "  \"git_dirty\": $GIT_DIRTY,"
    echo "  \"environment\": {"
    echo "    \"os\": \"$OS_NAME\","
    echo "    \"rust_version\": \"$RUST_VERSION\","
    echo "    \"quick_mode\": $QUICK_MODE"
    echo "  },"
    echo "  \"results\": {"

    # Parser benchmarks
    if [[ -z "$CATEGORY" || "$CATEGORY" == "parser" ]]; then
        echo "    \"parser\": {"
        run_criterion_bench "perl-parser" "parser_benchmark" "parser"
        # Remove trailing comma from last entry
        echo "      \"_category\": \"parser\""
        echo "    },"
    fi

    # Lexer benchmarks
    if [[ -z "$CATEGORY" || "$CATEGORY" == "lexer" ]]; then
        echo "    \"lexer\": {"
        run_criterion_bench "perl-lexer" "lexer_benchmarks" "lexer"
        echo "      \"_category\": \"lexer\""
        echo "    },"
    fi

    # LSP benchmarks
    if [[ -z "$CATEGORY" || "$CATEGORY" == "lsp" ]]; then
        echo "    \"lsp\": {"
        run_criterion_bench "perl-lsp-rs" "rope_performance_benchmark" "lsp"
        echo "      \"_category\": \"lsp\""
        echo "    },"
    fi

    # Workspace index benchmarks
    if [[ -z "$CATEGORY" || "$CATEGORY" == "index" ]]; then
        echo "    \"index\": {"
        # The index bench target declares required-features=["workspace"].
        run_criterion_bench "perl-workspace" "workspace_index_benchmark" "index" "workspace"
        echo "      \"_category\": \"index\""
        echo "    },"
    fi

    # RIPR facts benchmarks
    if [[ -z "$CATEGORY" || "$CATEGORY" == "ripr" ]]; then
        echo "    \"ripr\": {"
        run_criterion_bench "perl-ripr-facts" "ripr_facts_benchmark" "ripr"
        echo "      \"_category\": \"ripr\""
        echo "    }"
    else
        # Remove trailing comma if ripr was skipped
        echo "    \"_done\": true"
    fi

    echo "  }"
    echo "}"
}

# Run and output
if [[ -n "$OUTPUT_FILE" ]]; then
    log "Saving results to $OUTPUT_FILE"
    json_output > "$OUTPUT_FILE"
    echo "Results saved to $OUTPUT_FILE"
else
    json_output
fi

# A failed category is recorded in the JSON above; also fail the process so
# no caller mistakes a partial run for green (#17218).
if [[ -n "$FAILED_CATEGORIES" ]]; then
    echo "Error: benchmark categories failed: $FAILED_CATEGORIES" >&2
    exit 1
fi
