# test-heavy-v1

Experience family: `test_heavy`.

A Test::More project with a plan, subtests, and two test files around one
small module. It exercises test discovery, navigation from test files to
the module under test, completion inside test code, and diagnostics that
distinguish test-file scope from module scope.

Observe during the study: definition from `t/inventory_edges.t` into
`lib/Inventory.pm`, hover on `ok`/`is`, and whether editing
`lib/Inventory.pm` refreshes results opened from the test files.

Do not edit files in this directory without refreshing `../manifest.json`
and any receipt that binds `test-heavy-v1`.
