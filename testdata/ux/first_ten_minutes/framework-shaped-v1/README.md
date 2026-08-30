# framework-shaped-v1

Experience family: `framework_shaped`.

One bounded Mojolicious::Lite-style routing app. The framework module is
intentionally not vendored: the project exercises how the language server
behaves when a `use` site names an uninstalled CPAN module, when symbols
come from a framework DSL, and when `get '/task/:id'` route placeholders
are not plain Perl variables.

Observe during the study: whether the missing framework module produces a
clear, bounded explanation rather than manufactured exactness; whether
route handlers still get ordinary Perl navigation; and whether completion
near the DSL stays quiet instead of noisy.

Do not edit files in this directory without refreshing `../manifest.json`
and any receipt that binds `framework-shaped-v1`.
