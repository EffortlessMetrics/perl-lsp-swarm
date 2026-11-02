# perl-code-intelligence

Transport-neutral workspace session and document source authority for Perl
code intelligence (#6957, #7218, #8582).

This crate is the emerging service boundary below LSP and MCP adapters. It
composes canonical source-identity and readiness vocabulary without wire
types, fabricated transport sessions, or filesystem mutation.

Current scope in this repository train:

- typed document source authority (open / caller-supplied / disk observation);
- session identity from workspace and configuration generations;
- fail-closed construction when supplied text or generations disagree.

Provider migration, public package publication proof, and cross-adapter parity
live in sibling issues (#7220–#7222, #8582, #6961).
