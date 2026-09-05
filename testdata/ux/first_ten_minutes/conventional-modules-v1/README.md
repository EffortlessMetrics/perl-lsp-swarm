# conventional-modules-v1

Experience family: `conventional_modules`.

A conventional multi-file project: two `lib/` packages, a `bin/` entry
script, and one Test::More file. It exercises package navigation, import
resolution from `use` sites, and go-to-definition across files.

Observe during the study: completion after `App::`, hover on
`App::Registry`, definition from `bin/app.pl` into `lib/App/Registry.pm`,
workspace symbols for the two packages, and diagnostics after editing a
declaration.

Do not edit files in this directory without refreshing `../manifest.json`
and every receipt that binds `conventional-modules-v1`.
