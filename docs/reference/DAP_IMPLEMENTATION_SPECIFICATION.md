# DAP implementation specification — historical pointer

This document records the original bridge-to-native implementation plan for DAP. It is not the current implementation or support contract: the native `perl-dap` package now exists, and current user-visible behavior, limitations, and verification status are maintained elsewhere.

Use these current authorities instead:

- [Debugging guide](../how-to/DEBUGGING.md) for user-facing run modes and limitations;
- [DAP status](../project/status/dap.md) for current verification and scorecard state;
- [perl-dap README](../../crates/perl-dap/README.md) for the package boundary and commands;
- [Architecture Overview](ARCHITECTURE.md) for the DAP relationship to the rest of the workspace;
- current tests, receipts, and issue-linked acceptance evidence for individual claims.

The former phase schedule, version, acceptance totals, coverage targets, latency targets, bridge/shim deliverables, and “specification complete” label are historical planning material. They do not establish current implementation completeness, interactive attach support, or release readiness.
