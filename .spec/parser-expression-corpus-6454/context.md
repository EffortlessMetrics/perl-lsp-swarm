# Context

Issue: #6454.

This slice measures parser-facing expression and statement NodeKinds that already exist in the public AST but were absent from the manifest-backed scorer. The fixture uses production-shaped assignment, unary, array/hash literal, array/hash/key-value slice, and ternary forms.

The branch adds no parser behavior change. It widens the scorer's measured-node set, binds the fixture into the public parser E2E selector, and records executable expectations.
