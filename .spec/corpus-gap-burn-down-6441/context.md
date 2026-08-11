# Context

The Perl corpus already contained specialized generators and two dormant parser-accuracy fixtures for format declarations and do/until control flow, but the public parser-accuracy E2E selector did not exercise those fixtures. The documented corpus gap inventory also identified continue/redo, glob expressions, and tie/untie as absent from the manifest-backed parser-accuracy surface.

This slice builds the parser-facing evidence boundary for those gaps. It does not claim dedicated AST node kinds for constructs the parser currently represents through existing call/control-flow nodes, and it does not change parser behavior or LSP providers.
