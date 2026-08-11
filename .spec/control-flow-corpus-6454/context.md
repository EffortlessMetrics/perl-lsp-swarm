# Context — control-flow parser corpus

Issue: #6454

The broad Perl corpus contains control-flow examples, but the parser-accuracy bank lacked executable AST anchors for core conditional, loop, return, and loop-control nodes. This fixture is intentionally small and isolated so failures identify parser projection or span drift directly.
