# Parser-accuracy failure worklist

Source: `target/metrics/parser_accuracy.json` (50 failure packets)

| Family | Count | Likely layer | First fixture | Suggested PR |
|---|---:|---|---|---|
| ast_shape_mismatch | 45 | ast_projection | `dynamic_require_boundary` | `feat(parser-accuracy): tighten AST projection fixture expectations` |
| line_tag_mismatch | 4 | parser | `slash_ambiguity` | `fix(parser-core): resolve parser projection failure packet` |
| missing_symbol_reference | 1 | semantic_fact_extraction | `imports_exports` | `fix(semantic): resolve parser-accuracy semantic fact packet` |
