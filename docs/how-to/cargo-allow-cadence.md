# Optional upstream cargo-allow cadence capture

This advisory capture complements `cargo xtask policy cadence`. Native cadence JSON,
Markdown and classification rules stay unchanged. It captures cargo-allow's own
explicit-date report, including its 14-day horizons and source-exception ledger;
it does not promote that ledger or change policy dates. Adoption and baseline ownership
remain tracked in [#15304](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/15304).

Use Python 3 on a POSIX host (Linux, macOS or WSL), an already installed executable
and its independently qualified SHA256. The caller owns executable immutability.
There is no PATH search, installer, download or default date.

```sh
python3 scripts/cargo_allow_cadence.py \
  --executable '/absolute/path/to/cargo-allow' \
  --expected-sha256 '<qualified 64-hex executable SHA256>' \
  --as-of 2026-10-20 \
  --root '/absolute/path/to/consumer' \
  --output-dir '/absolute/existing/parent/capture-2026-10-20'
```

The root must contain `policy/allow.toml`. The output directory must be fresh, with
an existing parent, or contain exactly an identical prior capture. The thin recipe
`just cargo-allow-cadence EXE SHA256 DATE ROOT OUTPUT_DIR` passes the same arguments.
Paths with spaces are supported. Refusal leaves existing artifacts unchanged; for
a different executable, date or policy, choose a new output directory.

`cargo-allow-cadence.json` preserves upstream stdout byte for byte.
`cargo-allow-cadence.stderr` preserves bounded stderr, including successful warnings.
`execution-receipt.json` separately records the exact command, explicit date,
report digest and observed executable/policy hashes before and after execution.
Identical repeated captures do not rewrite any file. A wrong executable digest,
failed child, timeout, oversized output, malformed/mismatched report or observed
input change prevents publication. Execution is limited to 30 seconds, 1 MiB stdout
and 64 KiB stderr, with process-group cleanup.
Generated execution receipts must fit within 16 KiB; an oversized receipt prevents publication.

The receipt establishes observed input identity for this capture. Pre/post hashes
do not guarantee immutability or close TOCTOU races. The report's tracked-file
inventory is context; this is not a whole-worktree snapshot, installed-consumer
qualification, performance qualification or a no-new-finding gate. A sparse policy
fixture proves only that fixture, even when it uses the actual qualified producer.

Focused portable admission and POSIX process fixtures run with:

```sh
python3 -m unittest scripts.tests.test_cargo_allow_cadence
```
