# Context — self-delimited qw recovery

Issue: #4499

The lexer’s recovery seam handled unclosed parenthesized `qw(` input but did not exercise self-delimited `qw[...]` forms or identifier-led statement boundaries. This slice keeps recovery bounded to real statement starters and uses the existing local symbol table for configured user subroutines.
