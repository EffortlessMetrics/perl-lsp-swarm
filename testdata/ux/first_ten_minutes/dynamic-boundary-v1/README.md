# dynamic-boundary-v1

Experience family: `dynamic_boundary_control`.

A project whose call graph is deliberately dynamic: an AUTOLOAD handler,
symbolic method dispatch through a computed method name, and a string
`eval`. It exercises whether the language server reports bounded fallback
or refusal with an explanation instead of manufacturing exact answers.

Observe during the study: hover and completion on `$dispatch->status`
(handled via AUTOLOAD), on `$dispatch->invoke('status')` (symbolic), and
on both `eval`-guarded calls (the block form and the runtime-compiled
string form with its explained fallback). Correct behavior is a bounded,
explained limitation; manufactured exactness or unexplained silence are
both trust-breaking observations.

Do not edit files in this directory without refreshing `../manifest.json`
and any receipt that binds `dynamic-boundary-v1`.
