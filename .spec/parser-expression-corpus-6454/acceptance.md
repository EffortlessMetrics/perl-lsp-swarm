# Acceptance

- The fixture is registered in the parser-accuracy manifest.
- Every expected expression NodeKind is included in the scorer's authoritative AST prediction set.
- The public parser E2E selector exercises the fixture.
- Manifest and source spans remain structurally consistent.
- Hosted formatting and corpus/parser checks determine whether the slice is mergeable.

Claim boundary: this measures the selected AST shapes; it does not claim complete expression coverage or parser correctness beyond the fixture.
