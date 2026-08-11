Issue: #6454

The parser-accuracy manifest already contains AST expectations for the dynamic require boundary fixture, but the public parser E2E selector did not exercise it. This slice promotes that existing fixture into executable coverage without changing parser behavior or inventing new expectations.
