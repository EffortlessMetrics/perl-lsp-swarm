#!/usr/bin/env bash
# Real Vim + Vimspector + perl-dap launch receipt for #7702.
#
# This harness proves the debugger through the actual Vim/Vimspector host.
# Direct DAP tests, configured-but-unverified breakpoints, and a local binary
# relabelled as a public artifact cannot satisfy the receipt.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
perl_bin=${PERL:-perl}
: "${VIMSPECTOR_DIR:?VIMSPECTOR_DIR must point at pinned Vimspector}"
: "${PERL_DAP:?PERL_DAP must point at exact perl-dap}"
out=${RECEIPT_DIR:-"${repo_root}/target/receipts/vimspector-perl-dap"}
contract="${repo_root}/.ci/editor-clients/vimspector-perl-dap.v1.json"
driver="${repo_root}/scripts/ux/vim_vimspector_dap_driver.vim"
expected_vimspector_ref=34099d18d8957bb3db5f396c8ca993ffb246a437
mkdir -p "${out}"

# Several stage steps abort under `set -e` (identity mismatch, config generation,
# receipt rewrite). Without this the per-stage workspace leaked on every such exit.
stage_tmpdir=
cleanup_stage_tmpdir() {
  if [[ -n ${stage_tmpdir} && -d ${stage_tmpdir} ]]; then
    rm -rf "${stage_tmpdir}"
    stage_tmpdir=
  fi
}
trap cleanup_stage_tmpdir EXIT

if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "Vimspector DAP FAILED: Vim not found: ${vim_bin}" >&2
  exit 1
fi
if ! command -v "${perl_bin}" >/dev/null 2>&1; then
  echo "Vimspector DAP FAILED: Perl not found: ${perl_bin}" >&2
  exit 1
fi
if ! command -v git >/dev/null 2>&1; then
  echo "Vimspector DAP FAILED: git is required to bind the Vimspector checkout" >&2
  exit 1
fi
if [[ ! -d "${VIMSPECTOR_DIR}/.git" || ! -f "${VIMSPECTOR_DIR}/plugin/vimspector.vim" ]]; then
  echo "Vimspector DAP FAILED: VIMSPECTOR_DIR must be a real git checkout" >&2
  exit 1
fi
if [[ ! -x "${PERL_DAP}" ]]; then
  echo "Vimspector DAP FAILED: perl-dap not executable: ${PERL_DAP}" >&2
  exit 1
fi
if [[ ! -f "${contract}" || ! -f "${driver}" ]]; then
  echo "Vimspector DAP FAILED: contract/driver missing" >&2
  exit 1
fi
if ! "${vim_bin}" --version | grep -q '+python3'; then
  echo "Vimspector DAP FAILED: Vim must be compiled with +python3" >&2
  exit 1
fi

vimspector_ref=$(git -C "${VIMSPECTOR_DIR}" rev-parse HEAD)
if [[ ${ALLOW_VIMSPECTOR_DRIFT:-0} != 1 && ${vimspector_ref} != "${expected_vimspector_ref}" ]]; then
  echo "Vimspector DAP FAILED: expected Vimspector ${expected_vimspector_ref}, got ${vimspector_ref}" >&2
  exit 1
fi

hash_file() {
  local path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    echo "Vimspector DAP FAILED: no SHA-256 tool available" >&2
    return 1
  fi
}

driver_sha=$(hash_file "${driver}")

absolute_path() {
  "${perl_bin}" -MCwd=abs_path -e 'my $p = abs_path($ARGV[0]); die "cannot resolve $ARGV[0]\n" unless defined $p; print $p' "$1"
}

capture_matching_processes() {
  local output=$1
  local adapter=$2
  local debuggee=$3
  ADAPTER_NEEDLE="${adapter}" DEBUGGEE_NEEDLE="${debuggee}" \
    ps -eo pid=,args= | "${perl_bin}" -ne '
      my $adapter = $ENV{ADAPTER_NEEDLE};
      my $debuggee = $ENV{DEBUGGEE_NEEDLE};
      next unless index($_, $adapter) >= 0 || index($_, $debuggee) >= 0;
      s/^\s+//;
      print;
    ' | sort -n >"${output}"
}

write_stage_metadata() {
  local stage=$1
  local adapter=$2
  local adapter_sha=$3
  local identity_path=$4
  local config_sha=$5
  local fixture_sha=$6
  local vim_version_sha=$7
  local vimspector_log=$8
  local adapter_stderr=$9
  local debuggee_output=${10}
  local process_ledger=${11}
  local adapter_trace=${12}

  cat >"${out}/${stage}.subject.txt" <<EOF
schema_version=1
stage=${stage}
platform=$(uname -s 2>/dev/null || echo unknown)
os_version=$(uname -r 2>/dev/null || echo unknown)
architecture=$(uname -m 2>/dev/null || echo unknown)
vim_executable=${vim_bin}
vim_version_sha256=${vim_version_sha}
driver=${driver}
driver_sha256=${driver_sha}
vimspector_dir=${VIMSPECTOR_DIR}
vimspector_ref=${vimspector_ref}
adapter=${adapter}
adapter_sha256=${adapter_sha}
adapter_identity=${identity_path}
configuration_sha256=${config_sha}
fixture_sha256=${fixture_sha}
vimspector_log=${vimspector_log}
adapter_stderr=${adapter_stderr}
debuggee_output=${debuggee_output}
process_ledger=${process_ledger}
adapter_trace=${adapter_trace}
EOF
}

verify_required_cells() {
  local receipt=$1
  CONTRACT_PATH="${contract}" RECEIPT_PATH="${receipt}" "${perl_bin}" -MJSON::PP -0777 -e '
    sub load_json {
      my ($path) = @_;
      open my $fh, "<", $path or die "open $path: $!";
      local $/;
      return decode_json(<$fh>);
    }
    my $contract = load_json($ENV{CONTRACT_PATH});
    my $receipt = load_json($ENV{RECEIPT_PATH});
    die "receipt ok=false\n" unless $receipt->{ok};
    for my $cell (@{$contract->{required_cells}}) {
      die "required cell missing: $cell\n" unless exists $receipt->{cells}{$cell};
      die "required cell false: $cell\n" unless $receipt->{cells}{$cell};
    }
    for my $artifact (@{$contract->{required_artifacts}}) {
      die "required artifact missing from receipt: $artifact\n"
        unless exists $receipt->{artifacts}{$artifact};
      my $path = $receipt->{artifacts}{$artifact};
      die "required artifact path absent: $artifact => $path\n" unless -f $path;
    }
  '
}

run_stage() {
  local stage=$1
  local adapter_input=$2
  local expected_sha=${3:-}
  local receipt="${out}/${stage}.json"
  local tmpdir
  tmpdir=$(mktemp -d)
  stage_tmpdir="${tmpdir}"
  local workspace="${tmpdir}/workspace"
  local home="${tmpdir}/home"
  mkdir -p "${workspace}/lib" "${workspace}/shadow" "${home}"

  local adapter
  adapter=$(absolute_path "${adapter_input}")
  local adapter_sha
  adapter_sha=$(hash_file "${adapter}")
  if [[ -n ${expected_sha} && ${adapter_sha} != "${expected_sha}" ]]; then
    echo "Vimspector DAP FAILED: ${stage}: expected adapter SHA-256 ${expected_sha}, got ${adapter_sha}" >&2
    cleanup_stage_tmpdir
    return 1
  fi

  local identity_path="${out}/${stage}.perl-dap.identity.json"
  if ! "${adapter}" --identity-json >"${identity_path}"; then
    echo "Vimspector DAP FAILED: ${stage}: perl-dap --identity-json failed" >&2
    cleanup_stage_tmpdir
    return 1
  fi
  IDENTITY_PATH="${identity_path}" "${perl_bin}" -MJSON::PP -0777 -e '
    open my $fh, "<", $ENV{IDENTITY_PATH} or die $!;
    local $/;
    my $id = decode_json(<$fh>);
    die "wrong identity schema\n" unless $id->{schema_version} eq "perl_lsp.binary_identity.v1";
    die "wrong executable identity\n" unless $id->{binary}{executable} eq "perl-dap";
    die "wrong binary role\n" unless $id->{binary}{role} eq "dap";
  '

  cat >"${workspace}/debug_me.pl" <<'PERL'
use strict;
use warnings;
my $value = 41;
$value += 1;
my $message = "value=$value";
print "$message\n";
PERL

  cat >"${workspace}/shadow/debug_me.pl" <<'PERL'
use strict;
use warnings;
die "shadow debug_me.pl must never execute";
PERL

  local debuggee
  debuggee=$(absolute_path "${workspace}/debug_me.pl")
  local shadow
  shadow=$(absolute_path "${workspace}/shadow/debug_me.pl")
  local adapter_trace="${out}/${stage}.perl-dap.trace.log"
  local adapter_trace_prefix="${workspace}/perl-dap-trace"
  local vimspector_log="${out}/${stage}.vimspector.log"
  local adapter_stderr="${out}/${stage}.perl-dap.stderr.log"
  local debuggee_output="${out}/${stage}.debuggee-output.log"
  local process_ledger="${out}/${stage}.process-ledger.txt"
  : >"${adapter_trace}"
  : >"${adapter_stderr}"
  : >"${debuggee_output}"
  : >"${vimspector_log}"

  ADAPTER="${adapter}" \
  PERL_BIN="$(command -v "${perl_bin}")" \
  WORKSPACE="${workspace}" \
  ADAPTER_TRACE="${adapter_trace_prefix}" \
  "${perl_bin}" -MJSON::PP -e '
    my $cfg = {
      q{$schema} => q{https://puremourning.github.io/vimspector/schema/vimspector.schema.json},
      adapters => {
        q{perl-dap-under-test} => {
          command => [ $ENV{ADAPTER}, q{--stdio} ],
          env => {
            PERL_LSP_LOG_FILE => $ENV{ADAPTER_TRACE},
            RUST_LOG => q{info},
          },
        },
      },
      configurations => {
        q{Launch Perl} => {
          adapter => q{perl-dap-under-test},
          configuration => {
            request => q{launch},
            program => q{${workspaceRoot}/debug_me.pl},
            perlPath => $ENV{PERL_BIN},
            args => [],
            includePaths => [ q{${workspaceRoot}/lib} ],
            cwd => q{${workspaceRoot}},
            env => {},
          },
        },
      },
    };
    open my $fh, q{>}, "$ENV{WORKSPACE}/.vimspector.json" or die $!;
    print {$fh} JSON::PP->new->canonical(1)->pretty(1)->encode($cfg);
    close $fh;
  '

  local config_sha
  config_sha=$(hash_file "${workspace}/.vimspector.json")
  local fixture_manifest="${out}/${stage}.fixture-manifest.txt"
  (
    cd "${workspace}"
    {
      hash_file_local() {
        if command -v sha256sum >/dev/null 2>&1; then
          sha256sum "$1"
        else
          shasum -a 256 "$1"
        fi
      }
      hash_file_local debug_me.pl
      hash_file_local shadow/debug_me.pl
      hash_file_local .vimspector.json
    } | sort
  ) >"${fixture_manifest}"
  local fixture_sha
  fixture_sha=$(hash_file "${fixture_manifest}")

  local vim_version_path="${out}/${stage}.vim-version.txt"
  "${vim_bin}" --version >"${vim_version_path}"
  local vim_version_sha
  vim_version_sha=$(hash_file "${vim_version_path}")

  local before="${tmpdir}/processes.before"
  local after="${tmpdir}/processes.after"
  capture_matching_processes "${before}" "${adapter}" "${debuggee}"

  export PERLLSP_VIMSPECTOR_DIR="${VIMSPECTOR_DIR}"
  export PERLLSP_DAP_WORKSPACE="${workspace}"
  export PERLLSP_DAP_RECEIPT="${receipt}"
  export PERLLSP_DAP_ADAPTER="${adapter}"
  export PERLLSP_DAP_ADAPTER_SHA="${adapter_sha}"
  export PERLLSP_DAP_IDENTITY="${identity_path}"
  export PERLLSP_DAP_DRIVER_SHA="${driver_sha}"
  export PERLLSP_DAP_STAGE="${stage}"
  export PERLLSP_DAP_EXPECTED_SOURCE="${debuggee}"
  export PERLLSP_DAP_SHADOW_SOURCE="${shadow}"
  export PERLLSP_DAP_CONFIG_SHA="${config_sha}"
  export PERLLSP_DAP_FIXTURE_SHA="${fixture_sha}"
  export PERLLSP_DAP_STDERR_ARTIFACT="${adapter_stderr}"
  export PERLLSP_DAP_OUTPUT_ARTIFACT="${debuggee_output}"

  local rc=0
  local vim_stdout="${out}/${stage}.vim-stdout.log"
  # Vimspector drives real windows and terminal buffers, so silent Ex mode (-es)
  # is the wrong host mode: upstream's own CI runs `vim -N --clean --not-a-term
  # -S ...`, never -es. Match that invocation, keep stdin closed so a stray prompt
  # cannot block forever, and bound the run so a hung adapter fails instead of
  # wedging the job.
  local -a vim_cmd=("${vim_bin}" -N -u NONE -n --not-a-term -S "${driver}")
  if command -v timeout >/dev/null 2>&1; then
    vim_cmd=(timeout --signal=TERM --kill-after=30s "${VIM_STAGE_TIMEOUT:-300}" "${vim_cmd[@]}")
  fi
  HOME="${home}" "${vim_cmd[@]}" </dev/null >"${vim_stdout}" 2>&1 || rc=$?

  capture_matching_processes "${after}" "${adapter}" "${debuggee}"
  {
    echo "stage=${stage}"
    echo "adapter=${adapter}"
    echo "debuggee=${debuggee}"
    echo "--- before ---"
    cat "${before}"
    echo "--- after ---"
    cat "${after}"
  } >"${process_ledger}"

  local baseline_pids="${tmpdir}/baseline.pids"
  local after_pids="${tmpdir}/after.pids"
  # `comm` compares byte-wise, so both inputs must use the default collation.
  # Numeric sort (e.g. 812 before 1490) makes `comm` report bogus differences
  # and exit non-zero, which under `set -e` aborted the run before the
  # os_process_cleanup evidence was ever written.
  awk '{print $1}' "${before}" | LC_ALL=C sort >"${baseline_pids}"
  awk '{print $1}' "${after}" | LC_ALL=C sort >"${after_pids}"
  local leaked_pids
  leaked_pids=$(LC_ALL=C comm -13 "${baseline_pids}" "${after_pids}" | LC_ALL=C sort -n | tr '\n' ' ' | sed 's/[[:space:]]*$//')
  local os_cleanup=true
  if [[ -n ${leaked_pids} ]]; then
    os_cleanup=false
  fi

  local vimspector_log_retained=false
  if [[ -s "${home}/.vimspector.log" ]]; then
    cp "${home}/.vimspector.log" "${vimspector_log}"
    vimspector_log_retained=true
  fi
  : >"${adapter_trace}"
  for trace_piece in "${adapter_trace_prefix}"*; do
    if [[ -f ${trace_piece} ]]; then
      cat "${trace_piece}" >>"${adapter_trace}"
    fi
  done
  local adapter_trace_retained=false
  if [[ -s "${adapter_trace}" ]]; then
    adapter_trace_retained=true
  fi

  if [[ ! -f "${receipt}" ]]; then
    echo "Vimspector DAP FAILED: receipt missing for ${stage}" >&2
    cleanup_stage_tmpdir
    return 2
  fi

  RECEIPT_PATH="${receipt}" \
  OS_CLEANUP="${os_cleanup}" \
  LEAKED_PIDS="${leaked_pids}" \
  PROCESS_LEDGER="${process_ledger}" \
  VIMSPECTOR_LOG="${vimspector_log}" \
  VIMSPECTOR_LOG_RETAINED="${vimspector_log_retained}" \
  ADAPTER_STDERR="${adapter_stderr}" \
  DEBUGGEE_OUTPUT="${debuggee_output}" \
  ADAPTER_TRACE="${adapter_trace}" \
  ADAPTER_TRACE_RETAINED="${adapter_trace_retained}" \
  VIM_VERSION="${vim_version_path}" \
  VIM_STDOUT="${vim_stdout}" \
  ADAPTER_IDENTITY="${identity_path}" \
  FIXTURE_MANIFEST="${fixture_manifest}" \
  DRIVER="${driver}" \
  "${perl_bin}" -MJSON::PP -0777 -e '
    my $path = $ENV{RECEIPT_PATH};
    open my $in, "<", $path or die $!;
    local $/;
    my $r = decode_json(<$in>);
    close $in;
    my $cleanup = $ENV{OS_CLEANUP} eq "true" ? JSON::PP::true : JSON::PP::false;
    $r->{cells}{os_process_cleanup} = $cleanup;
    $r->{cells}{vimspector_log_retained} =
      $ENV{VIMSPECTOR_LOG_RETAINED} eq "true" ? JSON::PP::true : JSON::PP::false;
    $r->{cells}{adapter_trace_retained} =
      $ENV{ADAPTER_TRACE_RETAINED} eq "true" ? JSON::PP::true : JSON::PP::false;
    $r->{process_cleanup} = {
      ok => $cleanup,
      leaked_pids => [ grep { length } split /\s+/, $ENV{LEAKED_PIDS} ],
      ledger => $ENV{PROCESS_LEDGER},
    };
    $r->{artifacts} = {
      vim_version => $ENV{VIM_VERSION},
      vim_stdout => $ENV{VIM_STDOUT},
      driver => $ENV{DRIVER},
      adapter_identity => $ENV{ADAPTER_IDENTITY},
      fixture_manifest => $ENV{FIXTURE_MANIFEST},
      vimspector_log => $ENV{VIMSPECTOR_LOG},
      adapter_stderr => $ENV{ADAPTER_STDERR},
      debuggee_output => $ENV{DEBUGGEE_OUTPUT},
      adapter_trace => $ENV{ADAPTER_TRACE},
      process_ledger => $ENV{PROCESS_LEDGER},
    };
    if (!$cleanup) {
      push @{$r->{failures}}, "adapter/debuggee process leaked after Vim exit";
      $r->{ok} = JSON::PP::false;
    }
    if ($ENV{VIMSPECTOR_LOG_RETAINED} ne "true") {
      push @{$r->{failures}}, "Vimspector log was not retained";
      $r->{ok} = JSON::PP::false;
    }
    if ($ENV{ADAPTER_TRACE_RETAINED} ne "true") {
      push @{$r->{failures}}, "perl-dap trace log was not retained";
      $r->{ok} = JSON::PP::false;
    }
    open my $out, ">", $path or die $!;
    print {$out} JSON::PP->new->canonical(1)->encode($r), "\n";
    close $out;
  '

  write_stage_metadata "${stage}" "${adapter}" "${adapter_sha}" "${identity_path}" \
    "${config_sha}" "${fixture_sha}" "${vim_version_sha}" "${vimspector_log}" \
    "${adapter_stderr}" "${debuggee_output}" "${process_ledger}" "${adapter_trace}"

  if ! verify_required_cells "${receipt}"; then
    rc=2
  fi

  cat "${receipt}"
  echo
  cleanup_stage_tmpdir
  return "${rc}"
}

run_stage exact_source_local "${PERL_DAP}"

if [[ -n ${PUBLIC_PERL_DAP:-} ]]; then
  : "${PUBLIC_PERL_DAP_SHA256:?PUBLIC_PERL_DAP_SHA256 is required when PUBLIC_PERL_DAP is supplied}"
  run_stage public_artifact "${PUBLIC_PERL_DAP}" "${PUBLIC_PERL_DAP_SHA256}"
else
  cat >"${out}/public_artifact.json" <<EOF
{"schema_version":2,"kind":"vim_vimspector_perl_dap","stage":"public_artifact","ok":false,"state":"not_proven","reason":"PUBLIC_PERL_DAP_not_supplied"}
EOF
fi

cat >"${out}/subject.txt" <<EOF
schema_version=2
vimspector_ref=${vimspector_ref}
perl_dap=$(absolute_path "${PERL_DAP}")
perl_dap_sha256=$(hash_file "$(absolute_path "${PERL_DAP}")")
public_perl_dap=${PUBLIC_PERL_DAP:-not_supplied}
attach=not_proven
EOF
