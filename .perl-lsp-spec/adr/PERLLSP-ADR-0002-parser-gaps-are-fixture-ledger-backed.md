# PERLLSP-ADR-0002: Parser gaps are fixture-ledger-backed

Decision:

Parser gaps are durable repo artifacts. A parser gap may not be considered
closed unless it has a ledger status, fixture or accepted-impossible rationale,
proof command, and claim boundary.

Consequence:

- prose-only gap closure is invalid;
- tests without ledger updates are incomplete;
- ledger updates without tests are incomplete unless status is `accepted-impossible`.
