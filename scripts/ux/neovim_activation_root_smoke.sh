#!/usr/bin/env bash
# Actual Neovim activation/root envelope for #10502 (parent acceptance #7743).
#
# Builds a deterministic fixture tree and runs the canonical Neovim config
# against a real perllsp so the emitted envelope records, per row, the native
# filetype, whether the canonical config actually activated, and whether the
# selected workspace root controlled an observable semantic result.
#
# Usage:
#   ./scripts/ux/neovim_activation_root_smoke.sh /path/to/perllsp
#
# Environment:
#   PERLLSP - exact perllsp binary (defaults to target/release/perllsp)
#   NEOVIM  - nvim executable (defaults to nvim)
#
# Exit codes:
#   0 - required activation/root assertions passed and an envelope was emitted
#   1 - required executable missing (NOT_PROVEN: instrument unavailable)
#   2 - actual Neovim journey failed
#
# Every root that claims project isolation carries the same `RootProbe` module
# spelling in a parent or sibling root with different content, so a passing row
# has to name the selected root's own facts rather than an equivalent fact from
# the wrong root.

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

if ! command -v git >/dev/null 2>&1; then
  echo "NOT_PROVEN: git not found; the linked-worktree row cannot be executed" >&2
  exit 1
fi

repo_root=$(cd "${repo_root}" && pwd)
perllsp_bin=$(cd "$(dirname "${perllsp_bin}")" && pwd)/$(basename "${perllsp_bin}")
fixture_root=$(mktemp -d)
trap 'rm -rf "${fixture_root}"' EXIT

# ---------------------------------------------------------------------------
# File-family denominator.
#
# The families live inside a Perl project so that "did the canonical config
# actually attach?" is a real observation for every row rather than an artifact
# of there being no selectable root.
# ---------------------------------------------------------------------------

mkdir -p "${fixture_root}/filetypes/bin" "${fixture_root}/filetypes/script"
touch "${fixture_root}/filetypes/.perl-lsp.toml"

write_perl_source() {
  cat >"$1" <<'EOF'
use strict;
use warnings;
my $value = 1;
EOF
}

write_perl_script() {
  cat >"$1" <<'EOF'
#!/usr/bin/env perl
use strict;
use warnings;
print "ok\n";
EOF
}

for file in sample.pl Sample.pm app.psgi basic.t legacy.PL; do
  write_perl_source "${fixture_root}/filetypes/${file}"
done

# Suffixes whose native detection depends on file content rather than the
# extension. `plain.cgi` is the negative control for exactly that claim: same
# suffix, no interpreter line.
for file in handler.cgi handler.fcgi; do
  write_perl_script "${fixture_root}/filetypes/${file}"
done
cat >"${fixture_root}/filetypes/plain.cgi" <<'EOF'
print "no interpreter line here\n";
EOF

write_perl_script "${fixture_root}/filetypes/bin/tool"
write_perl_script "${fixture_root}/filetypes/script/tool"

cat >"${fixture_root}/filetypes/cpanfile" <<'EOF'
requires 'strict';
EOF

cat >"${fixture_root}/filetypes/Doc.pod" <<'EOF'
=head1 NAME

Doc - fixture

=cut
EOF

cat >"${fixture_root}/filetypes/Native.xs" <<'EOF'
MODULE = Native PACKAGE = Native
EOF

for file in template.tt template.tt2; do
  cat >"${fixture_root}/filetypes/${file}" <<'EOF'
[% value %]
EOF
done

for file in view.ep view.mason; do
  cat >"${fixture_root}/filetypes/${file}" <<'EOF'
<% $value %>
EOF
done

# ---------------------------------------------------------------------------
# Root matrix.
# ---------------------------------------------------------------------------

# write_probe_module <dir> <marker>
#
# `probe_marker` keeps one spelling in every root so a wrong-root answer stays
# structurally plausible; `probe_<marker>` is unique per root so an indexed
# symbol names which root actually won.
write_probe_module() {
  local dir=$1 marker=$2
  mkdir -p "${dir}/lib"
  cat >"${dir}/lib/RootProbe.pm" <<EOF
package RootProbe;
use strict;
use warnings;
our \$marker = '${marker}';
sub probe_marker { return '${marker}'; }
sub probe_${marker} { return '${marker}'; }
1;
EOF
}

# write_probe_entry <dir>
write_probe_entry() {
  local dir=$1
  mkdir -p "${dir}/t"
  cat >"${dir}/t/probe.pl" <<'EOF'
use strict;
use warnings;
use RootProbe;
my $value = RootProbe::probe_marker();
print "$value\n";
EOF
}

# Each configured marker wins on its own.
mkdir -p "${fixture_root}/roots/marker-dot"
touch "${fixture_root}/roots/marker-dot/.perl-lsp.toml"
write_probe_module "${fixture_root}/roots/marker-dot" markerdot
write_probe_entry "${fixture_root}/roots/marker-dot"

mkdir -p "${fixture_root}/roots/marker-build"
cat >"${fixture_root}/roots/marker-build/Build.PL" <<'EOF'
use Module::Build;
Module::Build->new(module_name => 'BuildRoot')->create_build_script;
EOF
write_probe_module "${fixture_root}/roots/marker-build" markerbuild
write_probe_entry "${fixture_root}/roots/marker-build"

mkdir -p "${fixture_root}/roots/marker-dist"
cat >"${fixture_root}/roots/marker-dist/dist.ini" <<'EOF'
name = DistRoot
version = 0.001
EOF
write_probe_module "${fixture_root}/roots/marker-dist" markerdist
write_probe_entry "${fixture_root}/roots/marker-dist"

# A farther Perl marker must not beat a nearer Perl marker: the outer
# `.perl-lsp.toml` loses to the nearer `Makefile.PL`, and the outer copy of
# RootProbe must not answer the inner root's query.
mkdir -p "${fixture_root}/roots/nearest-perl/sub"
touch "${fixture_root}/roots/nearest-perl/.perl-lsp.toml"
write_probe_module "${fixture_root}/roots/nearest-perl" outerperl
cat >"${fixture_root}/roots/nearest-perl/sub/Makefile.PL" <<'EOF'
use ExtUtils::MakeMaker;
WriteMakefile(NAME => 'Nearest');
EOF
write_probe_module "${fixture_root}/roots/nearest-perl/sub" nearestperl
write_probe_entry "${fixture_root}/roots/nearest-perl/sub"

# A Perl project marker must beat the lower-priority `.git` fallback.
mkdir -p "${fixture_root}/roots/perl-beats-git/.git" "${fixture_root}/roots/perl-beats-git/app"
write_probe_module "${fixture_root}/roots/perl-beats-git" gitrootperl
cat >"${fixture_root}/roots/perl-beats-git/app/cpanfile" <<'EOF'
requires 'strict';
EOF
write_probe_module "${fixture_root}/roots/perl-beats-git/app" appperl
write_probe_entry "${fixture_root}/roots/perl-beats-git/app"

# `.git` is the fallback when no Perl project marker exists.
mkdir -p "${fixture_root}/roots/git-only/.git"
write_probe_module "${fixture_root}/roots/git-only" gitonly
write_probe_entry "${fixture_root}/roots/git-only"

# Two competing Perl markers at different depths: the deeper one owns the file.
mkdir -p "${fixture_root}/roots/depth-conflict/nested/deep"
cat >"${fixture_root}/roots/depth-conflict/Makefile.PL" <<'EOF'
use ExtUtils::MakeMaker;
WriteMakefile(NAME => 'Shallow');
EOF
write_probe_module "${fixture_root}/roots/depth-conflict" shallowdepth
cat >"${fixture_root}/roots/depth-conflict/nested/deep/cpanfile" <<'EOF'
requires 'strict';
EOF
write_probe_module "${fixture_root}/roots/depth-conflict/nested/deep" deepdepth
write_probe_entry "${fixture_root}/roots/depth-conflict/nested/deep"

# Sibling roots under one repository, same relative module path in both.
mkdir -p "${fixture_root}/roots/siblings/.git" \
  "${fixture_root}/roots/siblings/alpha" \
  "${fixture_root}/roots/siblings/beta"
for sibling in alpha beta; do
  cat >"${fixture_root}/roots/siblings/${sibling}/cpanfile" <<'EOF'
requires 'strict';
EOF
  write_probe_module "${fixture_root}/roots/siblings/${sibling}" "sibling${sibling}"
  write_probe_entry "${fixture_root}/roots/siblings/${sibling}"
done

# Linked Git worktree: `.git` is a file rather than a directory there, and the
# worktree carries no Perl project marker, so the row proves the `.git`
# fallback still selects the worktree itself.
mkdir -p "${fixture_root}/worktree-source"
(
  cd "${fixture_root}/worktree-source"
  git init --quiet --initial-branch=main .
  git config user.email 'fixture@example.invalid'
  git config user.name 'Fixture'
  echo 'source root' >README.md
  git add README.md
  git commit --quiet -m 'fixture'
  git branch --quiet linked
) >/dev/null 2>&1
write_probe_module "${fixture_root}/worktree-source" worktreesource
(
  cd "${fixture_root}/worktree-source"
  git worktree add --quiet "${fixture_root}/roots/worktree-linked" linked
) >/dev/null 2>&1
write_probe_module "${fixture_root}/roots/worktree-linked" worktreelinked
write_probe_entry "${fixture_root}/roots/worktree-linked"

if [[ ! -f "${fixture_root}/roots/worktree-linked/.git" ]]; then
  echo "fixture error: linked worktree did not produce a .git file" >&2
  exit 2
fi

# Single file with no marker at all.
mkdir -p "${fixture_root}/nomarker"
write_perl_source "${fixture_root}/nomarker/single.pl"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    echo "NOT_PROVEN: no sha256sum or shasum available for content identity" >&2
    exit 1
  fi
}

# Resolve the digests before the run. `sha256_of` exits from a command
# substitution's subshell only, so inlining these below would turn a missing
# digest tool into an empty value and surface it as a product failure (2)
# rather than the instrument-unavailable result (1) this script documents.
perllsp_sha256=$(sha256_of "${perllsp_bin}") || perllsp_sha256=''
config_sha256=$(sha256_of "${repo_root}/scripts/ux/neovim/perllsp.lua") || config_sha256=''
if [[ -z ${perllsp_sha256} || -z ${config_sha256} ]]; then
  echo "NOT_PROVEN: could not compute sha256 content identity" >&2
  exit 1
fi

envelope="${fixture_root}/envelope.json"
if ! REPO_ROOT="${repo_root}" \
  FIXTURE_ROOT="${fixture_root}" \
  PERLLSP="${perllsp_bin}" \
  PERLLSP_SHA256="${perllsp_sha256}" \
  CONFIG_SHA256="${config_sha256}" \
  "${nvim_bin}" --headless -u NONE -l \
    "${repo_root}/scripts/ux/neovim/neovim_activation_root_smoke.lua" \
    >"${envelope}" 2>"${fixture_root}/nvim.err"; then
  echo "neovim activation/root smoke FAILED" >&2
  cat "${fixture_root}/nvim.err" >&2
  [[ -s "${envelope}" ]] && cat "${envelope}" >&2
  exit 2
fi

if [[ ! -s "${envelope}" ]]; then
  echo "neovim activation/root smoke FAILED: envelope missing" >&2
  cat "${fixture_root}/nvim.err" >&2
  exit 2
fi

cat "${envelope}"
