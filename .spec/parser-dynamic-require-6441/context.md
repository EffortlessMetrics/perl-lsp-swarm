# Context

Issue: #6441.

The parser-accuracy manifest already contains a dynamic-require fixture with executable AST and line expectations, but the public parser E2E selector did not exercise it. This slice promotes that existing evidence into the public parser path without changing parser behavior.
