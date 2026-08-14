# Acceptance — self-delimited qw recovery

- Self-delimited `qw` recovery stops before `warn` and `say` statements.
- A known local subroutine can act as a recovery boundary.
- Similar ordinary words remain quote-word content.
- The change preserves the existing parser/lexer recovery architecture.
