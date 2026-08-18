#!/usr/bin/env bash
# Deep actual Vim + vim-lsp receipt for #7691.
#
# This wrapper owns exact subject identity, retained wire/process evidence, and
# the optional workspace-folder observation. The Vim driver owns behavior in the
# real editor/client event loop. Missing subjects/evidence fail closed.

set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
vim_bin=${VIM:-vim}
: "${VIM_LSP_DIR:?VIM_LSP_DIR must point at a pinned vim-lsp checkout}"
: "${PERLLSP:?PERLLSP must point at the exact perllsp candidate}"
expected_vim_lsp_ref=e10d186452743beb7b43d2b3427020832f930c2b
driver="${repo_root}/scripts/ux/vim_vim_lsp_driver.vim"
activation_contract="${repo_root}/.ci/editor-clients/vim-vim-lsp-activation-root.v1.json"
out=${RECEIPT_DIR:-"${repo_root}/target/receipts/vim-vim-lsp"}
receipt=${RECEIPT:-"${out}/actual-client.json"}
mkdir -p "${out}" "$(dirname "${receipt}")"

for required in "${driver}" "${activation_contract}" "${VIM_LSP_DIR}/plugin/lsp.vim"; do
  [[ -f ${required} ]] || { echo "vim/vim-lsp smoke FAILED: missing ${required}" >&2; exit 1; }
done
if ! command -v "${vim_bin}" >/dev/null 2>&1; then
  echo "vim/vim-lsp smoke FAILED: Vim executable not found: ${vim_bin}" >&2
  exit 1
fi
if ! command -v git >/dev/null 2>&1; then
  echo "vim/vim-lsp smoke FAILED: git is required to bind the vim-lsp checkout" >&2
  exit 1
fi
if ! command -v perl >/dev/null 2>&1; then
  echo "vim/vim-lsp smoke FAILED: Perl is required for receipt parsing" >&2
  exit 1
fi
if [[ ! -x ${PERLLSP} ]]; then
  echo "vim/vim-lsp smoke FAILED: perllsp is not executable: ${PERLLSP}" >&2
  exit 1
fi
if [[ ! -d ${VIM_LSP_DIR}/.git ]]; then
  echo "vim/vim-lsp smoke FAILED: VIM_LSP_DIR must be a real git checkout" >&2
  exit 1
fi

vim_lsp_ref=$(git -C "${VIM_LSP_DIR}" rev-parse HEAD)
if [[ ${ALLOW_VIM_LSP_DRIFT:-0} != 1 && ${vim_lsp_ref} != "${expected_vim_lsp_ref}" ]]; then
  echo "vim/vim-lsp smoke FAILED: expected vim-lsp ${expected_vim_lsp_ref}, got ${vim_lsp_ref}" >&2
  exit 1
fi
if [[ ${ALLOW_VIM_LSP_DIRTY:-0} != 1 ]]; then
  git -C "${VIM_LSP_DIR}" diff --quiet --ignore-submodules -- || {
    echo "vim/vim-lsp smoke FAILED: vim-lsp checkout has unstaged changes" >&2; exit 1;
  }
  git -C "${VIM_LSP_DIR}" diff --cached --quiet --ignore-submodules -- || {
    echo "vim/vim-lsp smoke FAILED: vim-lsp checkout has staged changes" >&2; exit 1;
  }
fi

hash_file() {
  local path=$1
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    perl -MDigest::SHA=sha256_hex -0777 -ne 'print sha256_hex($_), "\n"' "${path}"
  fi
}

absolute_path() {
  local path=$1
  if [[ ${path} != */* ]]; then
    path=$(command -v "${path}")
  fi
  local dir
  dir=$(cd "$(dirname "${path}")" && pwd -P)
  printf '%s/%s\n' "${dir}" "$(basename "${path}")"
}

perllsp_bin=$(absolute_path "${PERLLSP}")
perllsp_sha=$(hash_file "${perllsp_bin}")
driver_sha=$(hash_file "${driver}")
activation_contract_sha=$(hash_file "${activation_contract}")

identity_path="${out}/perllsp.identity.json"
features_path="${out}/perllsp.features.json"
"${perllsp_bin}" --identity-json >"${identity_path}"
"${perllsp_bin}" --features-json >"${features_path}"
IDENTITY_PATH="${identity_path}" perl -MJSON::PP -0777 -e '
  open my $fh, "<", $ENV{IDENTITY_PATH} or die $!;
  local $/;
  my $id = decode_json(<$fh>);
  die "wrong identity schema\n" unless $id->{schema_version} eq "perl_lsp.binary_identity.v1";
  die "wrong executable identity\n" unless $id->{binary}{executable} eq "perllsp";
  die "wrong binary role\n" unless $id->{binary}{role} eq "server";
'

vim_version_path="${out}/vim-version.txt"
"${vim_bin}" --version >"${vim_version_path}"
vim_version_sha=$(hash_file "${vim_version_path}")

capture_matching_processes() {
  local output=$1
  if ! command -v ps >/dev/null 2>&1; then
    echo "vim/vim-lsp smoke FAILED: ps is required for process-cleanup evidence" >&2
    return 1
  fi
  PERLLSP_NEEDLE="${perllsp_bin}" ps -eo pid=,args= | perl -ne '
    my $needle = $ENV{PERLLSP_NEEDLE};
    next unless index($_, $needle) >= 0;
    s/^\s+//;
    print;
  ' | sort -n >"${output}"
}

# Run one Vim stage with stdin closed and a wall-clock bound, so a Vim that
# blocks before any s:WaitFor guard fails the stage instead of wedging the job.
# Mirrors the invocation already landed in vim_vimspector_dap_smoke.sh.
run_vim_stage() {
  local stage=$1
  local -a vim_cmd=("${vim_bin}" -Nu NONE -n -es -S "${driver}")
  if command -v timeout >/dev/null 2>&1; then
    vim_cmd=(timeout --signal=TERM --kill-after=30s "${VIM_STAGE_TIMEOUT:-300}" "${vim_cmd[@]}")
  else
    echo "vim/vim-lsp smoke: timeout(1) unavailable; ${stage} stage runs unbounded" >&2
  fi
  "${vim_cmd[@]}" </dev/null
}

extract_wire_evidence() {
  local log=$1
  local prefix=$2
  : >"${prefix}.didchange.jsonl"
  : >"${prefix}.initialized.jsonl"
  : >"${prefix}.shutdown.jsonl"
  : >"${prefix}.exit.jsonl"
  : >"${prefix}.workspace-folders.jsonl"
  : >"${prefix}.stderr.jsonl"
  rm -f "${prefix}.initialize-request.json" "${prefix}.client-capabilities.json"
  LOG_PATH="${log}" PREFIX="${prefix}" perl -MJSON::PP -e '
    my $prefix = $ENV{PREFIX};
    my $wrote_init = 0;
    open my $in, "<", $ENV{LOG_PATH} or die $!;
    open my $did, ">>", "$prefix.didchange.jsonl" or die $!;
    open my $initialized, ">>", "$prefix.initialized.jsonl" or die $!;
    open my $shutdown, ">>", "$prefix.shutdown.jsonl" or die $!;
    open my $exit, ">>", "$prefix.exit.jsonl" or die $!;
    open my $folders, ">>", "$prefix.workspace-folders.jsonl" or die $!;
    open my $stderr, ">>", "$prefix.stderr.jsonl" or die $!;
    my $json = JSON::PP->new->canonical(1);
    sub strings_contain_stderr {
      my ($x) = @_;
      if (ref($x) eq "ARRAY") { for (@$x) { return 1 if strings_contain_stderr($_) } }
      elsif (ref($x) eq "HASH") { for (values %$x) { return 1 if strings_contain_stderr($_) } }
      elsif (defined($x) && !ref($x)) { return 1 if $x =~ /stderr/i }
      return 0;
    }
    sub walk {
      my ($x) = @_;
      if (ref($x) eq "HASH") {
        if (exists $x->{method}) {
          my $method = $x->{method};
          if ($method eq "initialize" && !$wrote_init) {
            open my $req, ">", "$prefix.initialize-request.json" or die $!;
            print {$req} $json->pretty(1)->encode($x); close $req;
            open my $caps, ">", "$prefix.client-capabilities.json" or die $!;
            print {$caps} $json->pretty(1)->encode($x->{params}{capabilities} // {}); close $caps;
            $wrote_init = 1;
          } elsif ($method eq "initialized") {
            print {$initialized} $json->encode($x), "\n";
          } elsif ($method eq "textDocument/didChange") {
            print {$did} $json->encode($x), "\n";
          } elsif ($method eq "shutdown") {
            print {$shutdown} $json->encode($x), "\n";
          } elsif ($method eq "exit") {
            print {$exit} $json->encode($x), "\n";
          } elsif ($method eq "workspace/didChangeWorkspaceFolders") {
            print {$folders} $json->encode($x), "\n";
          }
        }
        walk($_) for values %$x;
      } elsif (ref($x) eq "ARRAY") {
        walk($_) for @$x;
      }
    }
    while (my $line = <$in>) {
      my $i = index($line, "[");
      next if $i < 0;
      my $payload = substr($line, $i);
      my $decoded = eval { decode_json($payload) };
      next unless $decoded;
      print {$stderr} $json->encode($decoded), "\n" if strings_contain_stderr($decoded);
      walk($decoded);
    }
  '
}

combine_trace() {
  local prefix=$1
  local output=$2
  : >"${output}"
  for piece in "${prefix}"*; do
    [[ -f ${piece} ]] || continue
    cat "${piece}" >>"${output}"
  done
}

tmpdir=$(mktemp -d)
cleanup() { rm -rf "${tmpdir}"; }
trap cleanup EXIT
workspace="${tmpdir}/workspace"
sibling="${tmpdir}/sibling"
mkdir -p "${workspace}/lib" "${sibling}"
: >"${workspace}/.perl-lsp.toml"
: >"${sibling}/.perl-lsp.toml"
cat >"${workspace}/lib/Widget.pm" <<'PERL'
package Widget;
use strict;
use warnings;
sub answer { return 42; }
sub greet { my ($name) = @_; return "hello $name"; }
1;
PERL
cat >"${workspace}/main.pl" <<'PERL'
use strict;
use warnings;
use Widget;

my $value = Widget::answer();
my $copy = $val
my $unicode = "😀"; my $unicode_value = Widget::answer();
print Widget::greet("world"), $value, $unicode_value;
PERL
cat >"${sibling}/other.pl" <<'PERL'
use strict;
my $other = 1;
print $other;
PERL

initial_main="${out}/fixture-main.initial.pl"
fixture_widget="${out}/fixture-Widget.pm"
cp "${workspace}/main.pl" "${initial_main}"
cp "${workspace}/lib/Widget.pm" "${fixture_widget}"
fixture_manifest="${out}/fixture-manifest.txt"
{
  printf '%s  %s\n' "$(hash_file "${initial_main}")" "fixture-main.initial.pl"
  printf '%s  %s\n' "$(hash_file "${fixture_widget}")" "fixture-Widget.pm"
  printf '%s  %s\n' "$(hash_file "${sibling}/other.pl")" "sibling-other.pl"
} | sort >"${fixture_manifest}"
fixture_sha=$(hash_file "${fixture_manifest}")

process_before="${tmpdir}/processes.before"
process_after="${tmpdir}/processes.after"
capture_matching_processes "${process_before}"

activation_receipt="${out}/activation-root.json"
RECEIPT="${activation_receipt}" VIM="${vim_bin}" VIM_LSP_DIR="${VIM_LSP_DIR}" PERLLSP="${perllsp_bin}" \
  "${repo_root}/scripts/ux/vim_activation_root_smoke.sh" --integration >/dev/null

baseline_log="${tmpdir}/vim-lsp.baseline.log"
baseline_caps="${out}/server-capabilities.json"
baseline_trace_prefix="${tmpdir}/perllsp-baseline"
export PERLLSP_VIM_LSP_DIR="${VIM_LSP_DIR}"
export PERLLSP_VIM_BIN="${perllsp_bin}"
export PERLLSP_VIM_WORKSPACE="${workspace}"
export PERLLSP_VIM_SIBLING="${sibling}"
export PERLLSP_VIM_RECEIPT="${receipt}"
export PERLLSP_VIM_LOG="${baseline_log}"
export PERLLSP_VIM_ACTIVATION_RECEIPT="${activation_receipt}"
export PERLLSP_VIM_SERVER_CAPABILITIES="${baseline_caps}"
export PERLLSP_VIM_SERVER_TRACE="${baseline_trace_prefix}"
export PERLLSP_VIM_MODE=baseline

baseline_rc=0
# The receipt directory persists between runs, so a receipt left by an earlier
# invocation would satisfy the -f check below even if this Vim run wrote nothing.
# Remove it first so the check is genuinely fail-closed.
rm -f "${receipt}"
run_vim_stage baseline || baseline_rc=$?
if [[ ${baseline_rc} -ne 0 ]]; then
  echo "vim/vim-lsp smoke FAILED: baseline driver exited ${baseline_rc}" >&2
  [[ -f ${baseline_log} ]] && cat "${baseline_log}" >&2
fi
if [[ ! -f ${receipt} ]]; then
  echo "vim/vim-lsp smoke FAILED: baseline receipt was not written" >&2
  [[ -f ${baseline_log} ]] && cat "${baseline_log}" >&2
  exit 2
fi
cp "${workspace}/main.pl" "${out}/fixture-main.final.pl"
client_log="${out}/vim-lsp.log"
cp "${baseline_log}" "${client_log}"
wire_prefix="${out}/wire"
extract_wire_evidence "${client_log}" "${wire_prefix}"
if [[ ! -s ${wire_prefix}.initialize-request.json || ! -s ${wire_prefix}.client-capabilities.json ]]; then
  echo "vim/vim-lsp smoke FAILED: initialize/client-capability evidence missing from real client log" >&2
  exit 2
fi
server_trace="${out}/perllsp.trace.log"
combine_trace "${baseline_trace_prefix}" "${server_trace}"
if [[ ! -s ${server_trace} ]]; then
  echo "vim/vim-lsp smoke FAILED: retained perllsp trace is empty" >&2
  exit 2
fi
server_stderr="${out}/perllsp.stderr.jsonl"
cp "${wire_prefix}.stderr.jsonl" "${server_stderr}"

wire_summary="${out}/wire-summary.json"
CAPS_PATH="${baseline_caps}" DID_PATH="${wire_prefix}.didchange.jsonl" \
INITIALIZED_PATH="${wire_prefix}.initialized.jsonl" SHUTDOWN_PATH="${wire_prefix}.shutdown.jsonl" \
EXIT_PATH="${wire_prefix}.exit.jsonl" OUT_PATH="${wire_summary}" perl -MJSON::PP -e '
  sub read_json {
    my ($path) = @_;
    open my $fh, "<", $path or die $!; local $/; return decode_json(<$fh>);
  }
  sub count_lines {
    my ($path) = @_;
    open my $fh, "<", $path or die $!; my $n = 0; $n++ while <$fh>; return $n;
  }
  my $caps = read_json($ENV{CAPS_PATH});
  my $sync = $caps->{textDocumentSync};
  my $change = ref($sync) eq "HASH" ? $sync->{change} : $sync;
  open my $did, "<", $ENV{DID_PATH} or die $!;
  my ($total, $ranged, $accepted) = (0, 0, 0);
  while (<$did>) {
    next unless /\S/;
    my $msg = decode_json($_); $total++;
    for my $change_event (@{$msg->{params}{contentChanges} // []}) {
      $ranged++ if ref($change_event) eq "HASH" && exists $change_event->{range};
      $accepted = 1 if ref($change_event) eq "HASH"
        && defined($change_event->{text})
        && index($change_event->{text}, q{my $copy = $value;}) >= 0;
    }
  }
  die "no real didChange traffic\n" if $total == 0;
  die "accepted edit not present in didChange evidence\n" unless $accepted;
  die "server advertised incremental sync but vim-lsp sent no ranged change\n"
    if defined($change) && $change == 2 && $ranged == 0;
  my $summary = {
    sync_change => $change,
    did_change_count => $total,
    ranged_change_count => $ranged,
    accepted_edit_seen => $accepted ? JSON::PP::true : JSON::PP::false,
    initialized_count => count_lines($ENV{INITIALIZED_PATH}),
    shutdown_count => count_lines($ENV{SHUTDOWN_PATH}),
    exit_count => count_lines($ENV{EXIT_PATH}),
  };
  die "initialized notification missing\n" if $summary->{initialized_count} == 0;
  die "shutdown request missing\n" if $summary->{shutdown_count} == 0;
  die "exit notification missing\n" if $summary->{exit_count} == 0;
  open my $out, ">", $ENV{OUT_PATH} or die $!;
  print {$out} JSON::PP->new->canonical(1)->pretty(1)->encode($summary);
'

workspace_receipt="${out}/workspace-folders.json"
workspace_log="${tmpdir}/vim-lsp.workspace-folders.log"
workspace_caps="${out}/workspace-folder-server-capabilities.json"
workspace_trace_prefix="${tmpdir}/perllsp-workspace-folders"
export PERLLSP_VIM_RECEIPT="${workspace_receipt}"
export PERLLSP_VIM_LOG="${workspace_log}"
export PERLLSP_VIM_SERVER_CAPABILITIES="${workspace_caps}"
export PERLLSP_VIM_SERVER_TRACE="${workspace_trace_prefix}"
export PERLLSP_VIM_MODE=workspace_folders
workspace_rc=0
# Same fail-closed requirement as the baseline receipt above: clear any receipt
# left by a previous run so this existence check proves the current run wrote it.
rm -f "${workspace_receipt}"
run_vim_stage workspace_folders || workspace_rc=$?
if [[ ${workspace_rc} -ne 0 ]]; then
  echo "vim/vim-lsp smoke FAILED: workspace-folder driver exited ${workspace_rc}" >&2
  [[ -f ${workspace_log} ]] && cat "${workspace_log}" >&2
fi
if [[ ! -f ${workspace_receipt} ]]; then
  echo "vim/vim-lsp smoke FAILED: workspace-folder observation receipt missing" >&2
  [[ -f ${workspace_log} ]] && cat "${workspace_log}" >&2
  exit 2
fi
workspace_client_log="${out}/vim-lsp.workspace-folders.log"
cp "${workspace_log}" "${workspace_client_log}"
workspace_wire_prefix="${out}/workspace-wire"
extract_wire_evidence "${workspace_client_log}" "${workspace_wire_prefix}"
workspace_notifications=$(grep -cve '^$' "${workspace_wire_prefix}.workspace-folders.jsonl" || true)
workspace_trace="${out}/perllsp.workspace-folders.trace.log"
combine_trace "${workspace_trace_prefix}" "${workspace_trace}"

capture_matching_processes "${process_after}"
process_ledger="${out}/process-ledger.txt"
{
  echo "perllsp=${perllsp_bin}"
  echo "--- before ---"
  cat "${process_before}"
  echo "--- after ---"
  cat "${process_after}"
} >"${process_ledger}"
# `comm` compares byte-wise, so both inputs must use the default collation.
# Numeric sort (e.g. 812 before 1490) makes `comm` report bogus differences and
# exit non-zero, which under `set -e` aborts the run before the
# os_process_cleanup evidence is ever written. Matches the resolution already
# landed in scripts/ux/vim_vimspector_dap_smoke.sh.
awk '{print $1}' "${process_before}" | LC_ALL=C sort >"${tmpdir}/before.pids"
awk '{print $1}' "${process_after}" | LC_ALL=C sort >"${tmpdir}/after.pids"
leaked_pids=$(LC_ALL=C comm -13 "${tmpdir}/before.pids" "${tmpdir}/after.pids" | LC_ALL=C sort -n | tr '\n' ' ' | sed 's/[[:space:]]*$//')
os_cleanup=true
[[ -z ${leaked_pids} ]] || os_cleanup=false

subject_path="${out}/subject.txt"
cat >"${subject_path}" <<EOF
schema_version=2
platform=$(uname -s 2>/dev/null || echo unknown)
os_version=$(uname -r 2>/dev/null || echo unknown)
architecture=$(uname -m 2>/dev/null || echo unknown)
vim_executable=${vim_bin}
vim_version_sha256=${vim_version_sha}
vim_lsp_dir=${VIM_LSP_DIR}
vim_lsp_ref=${vim_lsp_ref}
perllsp=${perllsp_bin}
perllsp_sha256=${perllsp_sha}
perllsp_identity_sha256=$(hash_file "${identity_path}")
perllsp_features_sha256=$(hash_file "${features_path}")
activation_contract_sha256=${activation_contract_sha}
activation_receipt_sha256=$(hash_file "${activation_receipt}")
driver_sha256=${driver_sha}
fixture_sha256=${fixture_sha}
EOF

RECEIPT_PATH="${receipt}" WIRE_SUMMARY="${wire_summary}" WORKSPACE_RECEIPT="${workspace_receipt}" \
WORKSPACE_NOTIFICATIONS="${workspace_notifications}" OS_CLEANUP="${os_cleanup}" LEAKED_PIDS="${leaked_pids}" \
SUBJECT_PATH="${subject_path}" VIM_VERSION="${vim_version_path}" CLIENT_LOG="${client_log}" \
SERVER_STDERR="${server_stderr}" SERVER_TRACE="${server_trace}" SERVER_CAPS="${baseline_caps}" \
CLIENT_CAPS="${wire_prefix}.client-capabilities.json" INIT_REQUEST="${wire_prefix}.initialize-request.json" \
IDENTITY_PATH="${identity_path}" FEATURES_PATH="${features_path}" ACTIVATION_RECEIPT="${activation_receipt}" \
FIXTURE_MANIFEST="${fixture_manifest}" FINAL_FIXTURE="${out}/fixture-main.final.pl" PROCESS_LEDGER="${process_ledger}" \
WORKSPACE_CLIENT_LOG="${workspace_client_log}" WORKSPACE_TRACE="${workspace_trace}" DRIVER="${driver}" \
perl -MJSON::PP -0777 -e '
  sub load_json {
    my ($path) = @_; open my $fh, "<", $path or die "open $path: $!"; local $/; return decode_json(<$fh>);
  }
  my $path = $ENV{RECEIPT_PATH};
  my $r = load_json($path);
  my $wire = load_json($ENV{WIRE_SUMMARY});
  my $wf = load_json($ENV{WORKSPACE_RECEIPT});
  my $cleanup = $ENV{OS_CLEANUP} eq "true" ? JSON::PP::true : JSON::PP::false;
  $r->{wire} = $wire;
  $r->{cells}{wire_lifecycle} = ($wire->{initialized_count} > 0 && $wire->{shutdown_count} > 0 && $wire->{exit_count} > 0) ? JSON::PP::true : JSON::PP::false;
  $r->{cells}{did_change_currentness} = $wire->{accepted_edit_seen};
  $r->{cells}{workspace_folders} = {
    driver => $wf->{cells}{workspace_folders},
    did_change_workspace_folders_count => 0 + $ENV{WORKSPACE_NOTIFICATIONS},
    outcome => (0 + $ENV{WORKSPACE_NOTIFICATIONS}) > 0 ? "observed" : "observed_no_notification",
  };
  $r->{cells}{os_process_cleanup} = $cleanup;
  $r->{process_cleanup} = {
    ok => $cleanup,
    leaked_pids => [grep { length } split /\s+/, $ENV{LEAKED_PIDS}],
  };
  $r->{subject_file} = $ENV{SUBJECT_PATH};
  $r->{artifacts} = {
    vim_version => $ENV{VIM_VERSION},
    driver => $ENV{DRIVER},
    client_log => $ENV{CLIENT_LOG},
    server_stderr => $ENV{SERVER_STDERR},
    server_trace => $ENV{SERVER_TRACE},
    server_capabilities => $ENV{SERVER_CAPS},
    client_capabilities => $ENV{CLIENT_CAPS},
    initialize_request => $ENV{INIT_REQUEST},
    binary_identity => $ENV{IDENTITY_PATH},
    feature_catalog => $ENV{FEATURES_PATH},
    activation_receipt => $ENV{ACTIVATION_RECEIPT},
    fixture_manifest => $ENV{FIXTURE_MANIFEST},
    final_fixture => $ENV{FINAL_FIXTURE},
    process_ledger => $ENV{PROCESS_LEDGER},
    workspace_folder_client_log => $ENV{WORKSPACE_CLIENT_LOG},
    workspace_folder_server_trace => $ENV{WORKSPACE_TRACE},
  };
  if (!$cleanup) {
    push @{$r->{failures}}, "perllsp process leaked after Vim exit";
    $r->{ok} = JSON::PP::false;
  }
  if (!$r->{cells}{wire_lifecycle}) {
    push @{$r->{failures}}, "initialize/initialized or shutdown/exit wire lifecycle incomplete";
    $r->{ok} = JSON::PP::false;
  }
  open my $out, ">", $path or die $!;
  print {$out} JSON::PP->new->canonical(1)->encode($r), "\n";
'

cat "${receipt}"
echo

if [[ ${baseline_rc} -ne 0 ]]; then
  echo "--- vim-lsp baseline log ---" >&2
  cat "${client_log}" >&2 || true
  exit "${baseline_rc}"
fi
if [[ ${workspace_rc} -ne 0 ]]; then
  echo "--- vim-lsp workspace-folder log ---" >&2
  cat "${workspace_client_log}" >&2 || true
  exit "${workspace_rc}"
fi
if [[ ${os_cleanup} != true ]]; then
  echo "vim/vim-lsp smoke FAILED: leaked perllsp PIDs: ${leaked_pids}" >&2
  exit 2
fi
