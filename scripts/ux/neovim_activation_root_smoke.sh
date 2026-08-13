#!/usr/bin/env bash
# Actual Neovim activation/root/filetype receipt for #7743.
#
# Usage:
#   ./scripts/ux/neovim_activation_root_smoke.sh /path/to/perllsp
#
# Environment:
#   PERLLSP - exact perllsp binary (defaults to target/release/perllsp)
#   NEOVIM  - nvim executable (defaults to nvim)
#
# Exit codes:
#   0 - required activation/root assertions passed and a receipt was emitted
#   1 - required executable missing
#   2 - actual Neovim journey failed

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
perllsp_bin=${PERLLSP:-${1:-"${repo_root}/target/release/perllsp"}}
nvim_bin=${NEOVIM:-nvim}

if ! command -v "${nvim_bin}" >/dev/null 2>&1; then
  echo "NOT_PROVEN: nvim not found (set NEOVIM=/path/to/nvim)" >&2
  exit 1
fi

if [[ ! -x "${perllsp_bin}" ]]; then
  echo "NOT_PROVEN: perllsp not executable at ${perllsp_bin}" >&2
  echo "build with: cargo build -p perllsp --release --bin perllsp" >&2
  exit 1
fi

repo_root=$(cd "${repo_root}" && pwd)
perllsp_bin=$(cd "$(dirname "${perllsp_bin}")" && pwd)/$(basename "${perllsp_bin}")
fixture_root=$(mktemp -d)
trap 'rm -rf "${fixture_root}"' EXIT

mkdir -p \
  "${fixture_root}/filetypes/bin" \
  "${fixture_root}/outer/sub/lib" \
  "${fixture_root}/gitroot/.git" \
  "${fixture_root}/gitroot/app/lib" \
  "${fixture_root}/nomarker"

cat >"${fixture_root}/filetypes/sample.pl" <<'EOF'
use strict;
my $value = 1;
EOF

cat >"${fixture_root}/filetypes/Sample.pm" <<'EOF'
package Sample;
use strict;
1;
EOF

cat >"${fixture_root}/filetypes/app.psgi" <<'EOF'
use strict;
my $app = sub { [200, [], ['ok']] };
$app;
EOF

cat >"${fixture_root}/filetypes/basic.t" <<'EOF'
use strict;
use Test::More tests => 1;
ok 1;
EOF

for file in legacy.PL handler.cgi handler.fcgi; do
  cat >"${fixture_root}/filetypes/${file}" <<'EOF'
#!/usr/bin/env perl
use strict;
print "ok\n";
EOF
done

cat >"${fixture_root}/filetypes/cpanfile" <<'EOF'
requires 'strict';
EOF

cat >"${fixture_root}/filetypes/bin/tool" <<'EOF'
#!/usr/bin/env perl
use strict;
print "tool\n";
EOF

cat >"${fixture_root}/filetypes/Doc.pod" <<'EOF'
=head1 NAME

Doc - fixture

=cut
EOF

cat >"${fixture_root}/filetypes/Native.xs" <<'EOF'
MODULE = Native PACKAGE = Native
EOF

cat >"${fixture_root}/filetypes/template.tt" <<'EOF'
[% value %]
EOF

# A farther outer .perl-lsp.toml must not beat the nearer Makefile.PL when
# Perl project markers are grouped at equal priority.
touch "${fixture_root}/outer/.perl-lsp.toml"
cat >"${fixture_root}/outer/sub/Makefile.PL" <<'EOF'
use ExtUtils::MakeMaker;
WriteMakefile(NAME => 'Nearest');
EOF
cat >"${fixture_root}/outer/sub/lib/Nearest.pm" <<'EOF'
package Nearest;
use strict;
1;
EOF

# A nearer Perl marker must beat the lower-priority .git fallback.
cat >"${fixture_root}/gitroot/app/cpanfile" <<'EOF'
requires 'strict';
EOF
cat >"${fixture_root}/gitroot/app/lib/App.pm" <<'EOF'
package App;
use strict;
1;
EOF

cat >"${fixture_root}/nomarker/single.pl" <<'EOF'
use strict;
my $single = 1;
EOF

receipt="${fixture_root}/receipt.json"
if ! REPO_ROOT="${repo_root}" \
  FIXTURE_ROOT="${fixture_root}" \
  PERLLSP="${perllsp_bin}" \
  "${nvim_bin}" --headless -u NONE -l \
    "${repo_root}/scripts/ux/neovim/neovim_activation_root_smoke.lua" \
    >"${receipt}" 2>"${fixture_root}/nvim.err"; then
  echo "neovim activation/root smoke FAILED" >&2
  cat "${fixture_root}/nvim.err" >&2
  [[ -s "${receipt}" ]] && cat "${receipt}" >&2
  exit 2
fi

if [[ ! -s "${receipt}" ]]; then
  echo "neovim activation/root smoke FAILED: receipt missing" >&2
  cat "${fixture_root}/nvim.err" >&2
  exit 2
fi

cat "${receipt}"
