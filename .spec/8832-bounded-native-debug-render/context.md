# Bounded native debug S-expression rendering (#8832)

Status: current for this claim
Owner: perl-ast

## Authority

The native debug grammar and one-root projection are #8829
(`crates/perl-ast/src/ast/node_sexp.rs`, PR #12728). Child identity and order
come only from `Node::for_each_child_with_field` (#8424 / PR #12583). This
claim does not copy a second child-match table or invent a second projection
grammar.

Deep fixtures may be dropped ordinarily (#8836). `mem::forget` is not
completion proof.

Out of scope: #8044 (typed machine output), #8047 (Tree-sitter CST),
#7045 (AST equality), #6900 (Clone/Eq/Debug), pest #8419/#8427.

## Contracts

- `render_debug_sexp` is iterative over an explicit heap stack.
- Caller-selected node, depth, output-byte, and work limits are optional.
- Outcomes are `Complete | Truncated | InstrumentFailure`.
- Truncation is typed metadata, not a fake AST node such as
  `(depth_limit_exceeded)`.
- Output never exceeds a declared byte limit. Bytes are UTF-8 lengths charged
  before forwarding a complete token to the caller `fmt::Write`.
- `omitted` is `Unknown` unless remaining members are already known without
  walking the omitted subtree.
- No thread-local or process-global renderer state.
- `to_sexp()` is a String convenience over the one engine. A `String` cannot
  prove completeness. Incomplete debug output cannot satisfy #7045 or #8044.

## Proof

`crates/perl-ast/tests/bounded_native_debug_render.rs` plus unit tests in
`crates/perl-ast/src/ast/node_sexp.rs`.
