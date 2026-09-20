# Iterative AST read traversal (#8867)

Status: current for this claim
Owner: perl-ast

## Authority

Production child identity and order come only from
`Node::try_for_each_child_with_field` (`kind_schema/visit.rs`, #8424 / PR #12583).
This claim does not copy a second child-match table.

Deep fixtures may be dropped ordinarily (#8836 / PR #11875). `mem::forget` is
not completion proof.

Out of scope: #8832 (`to_sexp` / configured Debug), #8044, #6900.

## Contracts

- Exact helpers (`count_nodes`, `find_deepest_containing_offset`, and their
  `_exact` forms) have no ordinary depth-truncation path.
- Bounded helpers return `Complete` / `Truncated` / `InstrumentFailure`.
- Containment is half-open: `start <= offset < end`.
- Greatest structural depth wins; equal-depth overlap keeps the earliest
  canonical visit-table path.
- Work counts nodes entered and edges descended; it is not reconstructed from
  the product value.

## Proof

`crates/perl-ast/tests/iterative_ast_read_traversal.rs` plus unit tests in
`crates/perl-ast/src/ast/read_cursor.rs`.
