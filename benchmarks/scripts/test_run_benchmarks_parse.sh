#!/usr/bin/env bash
# Fixture test for run-benchmarks.sh criterion parsing (#17219).
#
# Drives the real script with a fake `cargo` emitting a fixed transcript that
# exercises the single-line layout, the throughput two-line layout, `µs`
# units, and ignored noise (change blocks, outlier notes). Fails on any
# missing or mis-converted bench.
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cat > "$TMP/transcript.txt" <<'TRANSCRIPT'
Benchmarking packet_build/small
Benchmarking packet_build/small: Warming up for 3.0000 s
Benchmarking packet_build/small: Collecting 100 samples in estimated 9.4021 s (10k iterations)
Benchmarking packet_build/small: Analyzing
packet_build/small      time:   [1.1890 ms 1.2121 ms 1.2342 ms]
                        change:
                        time:   [-12.359% -8.8655% -5.4060%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 7 outliers among 100 measurements (7.00%)
Benchmarking packet_fingerprint/large
Benchmarking packet_fingerprint/large: Warming up for 3.0000 s
Benchmarking packet_fingerprint/large: Collecting 100 samples in estimated 5.3613 s (25k iterations)
Benchmarking packet_fingerprint/large: Analyzing
packet_fingerprint/large
                        time:   [206.67 µs 213.06 µs 218.91 µs]
                        thrpt:  [2.1604 GiB/s 2.2197 GiB/s 2.2883 GiB/s]
                 change:
                        time:   [+2.9840% +6.1621% +9.6521%] (p = 0.00 < 0.05)
                        Performance has regressed.
Benchmarking state transitions
Benchmarking state transitions: Warming up for 3.0000 s
Benchmarking state transitions: Collecting 100 samples in estimated 5.0000 s
Benchmarking state transitions: Analyzing
state transitions  time:   [1.0000 ms 1.1000 ms 1.2000 ms]
Gnuplot not found, using plotters backend
Benchmarking initial index small workspace (5 files)
Benchmarking initial index small workspace (5 files): Warming up for 3.0000 s
Benchmarking initial index small workspace (5 files): Collecting 100 samples in estimated 5.0000 s
Benchmarking initial index small workspace (5 files): Analyzing
initial index small workspace (5 files)
                        time:   [10.500 ms 10.700 ms 10.900 ms]
TRANSCRIPT

mkdir -p "$TMP/bin"
cat > "$TMP/bin/cargo" <<EOF
#!/usr/bin/env bash
# Fake cargo: ignore bench args, replay the fixed transcript.
cat "$TMP/transcript.txt"
EOF
chmod +x "$TMP/bin/cargo"

export PATH="$TMP/bin:$PATH"
"$SCRIPT_DIR/run-benchmarks.sh" --category ripr --output "$TMP/out.json" > /dev/null

python3 - "$TMP/out.json" <<'PYEOF'
import json
import sys

doc = json.load(open(sys.argv[1], encoding="utf-8"))
ripr = doc["results"]["ripr"]

small = ripr["packet_build/small"]
assert small["mean_ns"] == 1212100, small
assert small["low_ns"] == 1189000, small
assert small["high_ns"] == 1234200, small
assert small["unit"] == "ms", small

fp = ripr["packet_fingerprint/large"]
assert fp["mean_ns"] == 213060, fp
assert fp["low_ns"] == 206670, fp
assert fp["high_ns"] == 218910, fp

spaced = ripr["state transitions"]
assert spaced["mean_ns"] == 1100000, spaced
assert spaced["low_ns"] == 1000000, spaced
assert spaced["high_ns"] == 1200000, spaced

paren = ripr["initial index small workspace (5 files)"]
assert paren["mean_ns"] == 10700000, paren
assert paren["low_ns"] == 10500000, paren
assert paren["high_ns"] == 10900000, paren
assert fp["unit"] == "\u00b5s", fp

assert sorted(ripr.keys()) == ["_category", "initial index small workspace (5 files)", "packet_build/small", "packet_fingerprint/large", "state transitions"], sorted(ripr.keys())
print("run-benchmarks parse fixtures: OK (single-line, throughput two-line, µs, noise ignored)")
PYEOF
