# environment-sensitive-v1

Experience family: `environment_sensitive`.

A project whose resolution depends on `use lib` entries, a vendored
`local/lib/perl5` layout, and a `cpanfile`. It exercises include-path
sensitive module resolution and environment introspection without relying
on ambient `PERL5LIB`.

Observe during the study: definition from `script/report.pl` into
`lib/Local/Probe.pm`, whether the `local/lib/perl5` entry is treated as a
bounded include path rather than a workspace wildcard, and how `Config`
introspection is presented.

Do not edit files in this directory without refreshing `../manifest.json`
and any receipt that binds `environment-sensitive-v1`.
