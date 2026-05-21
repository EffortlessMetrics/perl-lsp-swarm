# Parser Differential Lane Implementation Plan

1. Establish parser target registry and labels (`ts-vendored-c`, `ts-upstream-c`, `pest-legacy`, `v3-native`).
2. Add optional upstream target integration without replacing vendored baseline.
3. Extend CLI to choose targets and emit machine-readable receipts.
4. Add gap ledger + benchmark receipts for correctness and latency governance.
