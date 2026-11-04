#!/usr/bin/env bash
# Diagnose existing Cargo work (#16126/#9178); never reuse or adjudicate artifacts.
# Usage: observe-xtask-build.sh PHASE OUTPUT_DIR -- COMMAND [ARG...]
# GNU time measures the whole command (including analysis for cargo xtask), not
# aggregate simultaneous RSS or the observer snapshot/collector CPU.
# Cargo's own Finished lines isolate build duration.
# cargo-toolchain-guard: exempt -- this diagnostic wrapper only probes metadata
# and delegates the caller's exact command/verdict. Hosted callers establish the
# pinned toolchain first; observation must not introduce a new build admission.
set -uo pipefail

phase=${1:-}; output=${2:-}
if [[ ! "$phase" =~ ^[a-z][a-z0-9-]*$ || ${3:-} != -- || $# -lt 4 ]]; then
    echo 'usage: observe-xtask-build.sh PHASE OUTPUT_DIR -- COMMAND [ARG...]' >&2
    exit 2
fi
shift 3
for tool in jq awk sha256sum; do
    if ! command -v "$tool" >/dev/null; then
        echo "::warning::xtask build observation unavailable: missing $tool" >&2
        exec "$@"
    fi
done
if ! mkdir -p "$output" || ! prefix=$(mktemp "$output/$phase.XXXXXX"); then
    echo '::warning::xtask build observation unavailable: output creation failed' >&2
    exec "$@"
fi
rm -f -- "$prefix"
# Raw descriptors contain private native paths. Keep at most one bounded record
# in a private temporary directory outside the artifact/upload directory.
private=$(mktemp -d /tmp/cargo-storage-observation.XXXXXXXX) || private=''
if [[ -n "$private" ]]; then
    output_absolute=$(cd "$output" && pwd -P)
    private_absolute=$(cd "$private" && pwd -P)
    if command -v cygpath >/dev/null; then
        output_absolute=$(cygpath -am -- "$output_absolute")
        private_absolute=$(cygpath -am -- "$private_absolute")
        output_absolute=${output_absolute,,}; private_absolute=${private_absolute,,}
    fi
    if [[ "$private_absolute" == "$output_absolute" || "$private_absolute" == "$output_absolute/"* ]]; then
        rmdir -- "$private" 2>/dev/null || true
        private=''
    fi
fi
descriptor_file=${private:+$private/descriptor.json}
cleanup_descriptor() {
    if [[ -n "$private" ]]; then
        rm -f -- "$private/descriptor.json"
        rmdir -- "$private" 2>/dev/null || true
    fi
}
trap cleanup_descriptor EXIT
if command -v python3 >/dev/null; then python=python3; else python=python; fi
storage_helper="$(dirname "${BASH_SOURCE[0]}")/../storage_observation.py"

hash_file() {
    if [[ -f "$1" ]]; then sha256sum "$1" | cut -d ' ' -f 1; else printf 'missing'; fi
}
hash_text() { printf '%s' "$1" | sha256sum | cut -d ' ' -f 1; }

snapshot() {
    # This is an environment/default hint, not Cargo metadata target resolution.
    local target=${CARGO_TARGET_DIR:-$PWD/target} home=${CARGO_HOME:-$HOME/.cargo}
    local configs='' cursor=$PWD file value flags=''
    # Cargo searches ancestor and Cargo-home configs. Hash contents; don't expose
    # config/flag values, which can contain credentials or machine-local secrets.
    while :; do
        for file in "$cursor/.cargo/config" "$cursor/.cargo/config.toml"; do
            [[ -f "$file" ]] && configs+="$file $(hash_file "$file")"$'\n'
        done
        [[ "$cursor" == / ]] && break
        cursor=$(dirname "$cursor")
    done
    for file in "$home/config" "$home/config.toml"; do
        [[ -f "$file" ]] && configs+="$file $(hash_file "$file")"$'\n'
    done
    for value in RUSTFLAGS CARGO_ENCODED_RUSTFLAGS RUSTC RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_BUILD_TARGET CARGO_BUILD_RUSTFLAGS CARGO_PROFILE_DEV_OPT_LEVEL CC CFLAGS AR; do
        if [[ -v "$value" ]]; then flags+="$value=set:${!value}"$'\n'; else flags+="$value=unset"$'\n'; fi
    done
    # Admission emits its descriptor later, inside the wrapped command. Neither
    # snapshot may retroactively treat environment/default hints as that scope.
    jq -n --arg head "$(git rev-parse HEAD)" --arg tree "$(git rev-parse 'HEAD^{tree}')" \
        --arg cwd_sha256 "$(hash_text "$PWD")" --arg cargo_home_hint_sha256 "$(hash_text "$home")" --arg target_dir_hint_sha256 "$(hash_text "$target")" \
        --arg rustc "$(rustc -vV)" --arg cargo "$(cargo -vV)" \
        --arg rustc_path_sha256 "$(hash_text "$(command -v rustc)")" --arg cargo_path_sha256 "$(hash_text "$(command -v cargo)")" \
        --arg sysroot_sha256 "$(hash_text "$(rustc --print sysroot)")" \
        --arg lock_sha256 "$(hash_file Cargo.lock)" --arg manifest_sha256 "$(hash_file Cargo.toml)" \
        --arg xtask_manifest_sha256 "$(hash_file xtask/Cargo.toml)" --arg toolchain_sha256 "$(hash_file rust-toolchain.toml)" \
        --arg configs_sha256 "$(hash_text "$configs")" --arg flags_sha256 "$(hash_text "$flags")" \
        '{head:$head,tree:$tree,cwd_sha256:$cwd_sha256,cargo_home_hint_sha256:$cargo_home_hint_sha256,target_dir_hint_sha256:$target_dir_hint_sha256,rustc:$rustc,cargo:$cargo,rustc_path_sha256:$rustc_path_sha256,cargo_path_sha256:$cargo_path_sha256,sysroot_sha256:$sysroot_sha256,lock_sha256:$lock_sha256,manifest_sha256:$manifest_sha256,xtask_manifest_sha256:$xtask_manifest_sha256,toolchain_sha256:$toolchain_sha256,configs_sha256:$configs_sha256,flags_sha256:$flags_sha256,fingerprints:[],fingerprints_clipped:false,fingerprints_complete:false,resource_authority:"NOT_PROVEN: environment hints are not an admitted descriptor"}'
}

snapshot > "$prefix.before.json" || printf '{"instrument_error":"before snapshot failed"}\n' > "$prefix.before.json"
# Fingerprint output is diagnostic only. Bound retained data and suppress those
# verbose trace lines from the job console; preserve all ordinary Cargo stderr.
collect_stderr() {
    # mawk otherwise buffers sparse pipe input until EOF or a full buffer;
    # fflush below only flushes records already read. Its interactive mode
    # line-buffers stdin so diagnostics survive the bounded drain cutoff.
    # Probe without consuming the command stream; other awks keep their flags.
    local implementation
    local -a input_options=()
    if implementation=$(LC_ALL=C awk -W version </dev/null 2>&1) && [[ "$implementation" == "mawk "* ]]; then
        input_options=(-W interactive)
    fi
LC_ALL=C exec awk "${input_options[@]}" -v trace="$prefix.trace.log" -v reasons="$prefix.dirty.log" -v finished="$prefix.finished.log" -v descriptor="${descriptor_file:-/dev/null}" -v summary="$prefix.stderr.json" '
    BEGIN {
        # Cargo 0.96.0 DirtyReason variants (the pinned 1.95 toolchain).
        # No arbitrary token, package SourceId, path, flag or log prefix survives.
        kinds="RustcChanged FeaturesChanged DeclaredFeaturesChanged TargetConfigurationChanged PathToSourceChanged ProfileConfigurationChanged RustflagsChanged ConfigSettingsChanged CompileKindChanged LocalLengthsChanged PrecalculatedComponentsChanged ChecksumUseChanged DepInfoOutputChanged RerunIfChangedOutputFileChanged RerunIfChangedOutputPathsChanged EnvVarsChanged EnvVarChanged LocalFingerprintTypeChanged NumberOfDependenciesChanged UnitDependencyNameChanged UnitDependencyInfoChanged FsStatusOutdated NothingObvious Forced FreshBuild"
        split(kinds, names, " "); for (i in names) allowed_kind[names[i]]=1
    }
    /^cargo-admitted resources:/ {
        descriptors++
        if (descriptors == 1 && length($0) <= 65536) {
            payload=$0; sub(/^cargo-admitted resources: /,"",payload)
            print payload > descriptor; fflush(descriptor)
        } else if (descriptors == 2 || (descriptors == 1 && length($0) > 65536)) {
            print "invalid or multiple resource descriptors" > descriptor; fflush(descriptor)
        }
        next
    }
    /^cargo-admitted .* (launch|settlement):/ { next }
    /^cargo-admitted/ {
        # Preflight/refusal records can also contain private paths or overrides.
        print "cargo-admitted diagnostic (details omitted)" > "/dev/stderr"
        fflush("/dev/stderr"); next
    }
    {
        # anyhow continuations can themselves mention the module delimiter.
        # Check them before target matching. Native uptime/level headers may
        # be padded; projected events keep a pending chain until a real
        # ordinary stderr/status boundary, even if a payload spoofs a header.
        plain=$0; gsub(/\033\[[0-9;]*[[:alpha:]]/,"",plain)
        # Spell out nine digits for awk implementations without intervals.
        tagged=(plain ~ /^[[:space:]]*[0-9]+\.[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]s[[:space:]]+(TRACE|DEBUG|INFO|WARN|ERROR)[[:space:]]+/ && plain ~ /cargo::core::compiler::fingerprint/)
        if (error_chain && !tagged && plain !~ /^[[:space:]]*(Compiling |Checking |Finished |error(\[E[0-9]+\])?:|warning:)/ && (plain ~ /^[[:space:]]*$/ || plain ~ /^(Caused by:|Stack backtrace:|stack backtrace:)/ || plain ~ /^[[:space:]]/)) { continuations++; next }
    }
    /cargo::core::compiler::fingerprint/ {
        traces++
        marker="cargo::core::compiler::fingerprint: "
        probe=$0
        locations=gsub(/cargo::core::compiler::fingerprint: /,"",probe)
        location=index($0,marker)
        # Span fields precede the target, and paths/payloads can contain this
        # delimiter too. Ambiguous boundaries cannot certify complete capture
        # or safely release possible untagged error continuations.
        if (locations != 1) {
            ambiguous++; error_chain=1
            projected="ambiguous fingerprint event (context omitted)"
        } else {
            message=substr($0,location+length(marker)); sub(/^[[:space:]]*/,"",message)
            # Construct selected records from fixed event labels and known enum
            # variants, rather than retaining then trying to redact raw trace text.
            # PackageId/SourceId Display may contain credential-bearing Git URLs.
            if (message ~ /^dirty: /) {
                reason=message; sub(/^dirty: /,"",reason); sub(/[^[:alnum:]_].*$/,"",reason)
                if (!(reason in allowed_kind)) { unknown++; reason="UNKNOWN" }
                projected="dirty: " reason " (details omitted)"
            } else if (message ~ /^fingerprint error for([[:space:]]|$)/) { errors++; projected="fingerprint error for (context omitted)" }
            else if (message ~ /^err:/) { projected="err: details omitted"; error_chain=1 }
            else if (message ~ /^fingerprint dirty for([[:space:]]|$)/) { projected="fingerprint dirty for (context omitted)" }
            else if (message ~ /^fingerprint at:/) { projected="fingerprint at: (context omitted)" }
            else if (message ~ /^write fingerprint([[:space:]:]|$)/) { projected="write fingerprint (context omitted)" }
            else { filtered++; next }
        }
        $0="cargo::core::compiler::fingerprint: " projected
        n=length($0)+1
        if (trace_bytes+n <= 1048576) { print > trace; trace_bytes+=n } else trace_clipped=1
        if ($0 ~ /fingerprint dirty for|dirty:|fingerprint error for|err:/) {
            dirty++; if (reason_bytes+n <= 262144) { print > reasons; reason_bytes+=n } else reasons_clipped=1
        }
        next
    }
    { error_chain=0 }
    plain ~ /^[[:space:]]*Compiling / { compiling++ }
    plain ~ /^[[:space:]]*Finished .*target\(s\) in / { print > finished }
    { print > "/dev/stderr"; fflush("/dev/stderr") }
    END { printf "{\"fingerprint_lines\":%d,\"dirty_lines\":%d,\"compiling_messages\":%d,\"filtered_fingerprint_lines\":%d,\"fingerprint_errors\":%d,\"unknown_dirty_reasons\":%d,\"ambiguous_fingerprint_lines\":%d,\"filtered_error_continuations\":%d,\"trace_clipped\":%s,\"dirty_clipped\":%s}\n", traces,dirty,compiling,filtered,errors,unknown,ambiguous,continuations,trace_clipped?"true":"false",reasons_clipped?"true":"false" > summary }
'
}
exec {stderr_fd}> >(collect_stderr)
reader=$!
start=$(date +%s)
export CARGO_LOG=cargo::core::compiler::fingerprint=trace
if [[ -x /usr/bin/time ]]; then
    /usr/bin/time -v -o "$prefix.time.txt" "$@" 2>&"$stderr_fd"
    status=$?
else
    "$@" 2>&"$stderr_fd"
    status=$?
    printf 'GNU time unavailable; CPU and peak RSS NOT_PROVEN\n' > "$prefix.time.txt"
fi
stop=$(date +%s)
exec {stderr_fd}>&-
# A detached command descendant can retain stderr after Cargo exits. Observation
# must not extend that command indefinitely; an incomplete drain is NOT_PROVEN.
reader_timed_out=false
for ((attempt=0; attempt<20; attempt++)); do
    kill -0 "$reader" 2>/dev/null || break
    sleep 0.1
done
if kill -0 "$reader" 2>/dev/null; then
    reader_timed_out=true
    kill -TERM "$reader" 2>/dev/null || true
fi
wait "$reader"; reader_status=$?
[[ -s "$prefix.stderr.json" ]] || printf '{"instrument_error":"stderr capture incomplete"}\n' > "$prefix.stderr.json"
snapshot > "$prefix.after.json" || printf '{"instrument_error":"after snapshot failed"}\n' > "$prefix.after.json"
storage_unknown='{"schema_version":"cargo_storage_inventory.v1","status":"NOT_PROVEN","observation_kind":"historical_only","descriptor_valid":false,"inventory_complete":false,"reasons":["descriptor_or_instrument_unavailable"],"admitted_before_sample":false,"same_lease_settlement":false,"peak_bytes":null,"build_delta_bytes":null}'
printf '%s\n' "$storage_unknown" > "$prefix.storage.json"
if [[ "$reader_status" == 0 && "$reader_timed_out" == false && -n "$descriptor_file" && -s "$descriptor_file" && -f "$storage_helper" ]] && command -v timeout >/dev/null; then
    timeout -k 1 6 "$python" -B "$storage_helper" --resources-descriptor "$descriptor_file" --json > "$prefix.storage.json" 2>/dev/null || true
    if ! jq -es 'length == 1 and (.[0] |
        type == "object" and .schema_version == "cargo_storage_inventory.v1" and
        .status == "NOT_PROVEN" and .observation_kind == "historical_only" and
        (.descriptor_valid | type == "boolean") and (.inventory_complete | type == "boolean") and
        (.fingerprints | type == "array") and (.fingerprints_complete | type == "boolean") and
        .admitted_before_sample == false and .same_lease_settlement == false and
        has("peak_bytes") and .peak_bytes == null and has("build_delta_bytes") and .build_delta_bytes == null and
        ((.inventory_complete | not) or .descriptor_valid) and
        ((.fingerprints_complete | not) or .inventory_complete))' "$prefix.storage.json" >/dev/null 2>&1; then
        printf '%s\n' "$storage_unknown" > "$prefix.storage.json"
    fi
fi
cleanup_descriptor
private=''
# Keep a non-NUL sentinel after the last argument delimiter. jq 1.6 omits the
# trailing empty field from split(), while newer jq versions retain it; relying
# on that field and slicing it away drops a real final argument on jq 1.6.
# The sentinel makes the split's final field unambiguously nonempty on both.
command_json=$(
    { printf '%s\0' "$@"; printf x; } |
        jq -Rs 'split("\u0000")[:-1]'
)
jq -n --arg schema_version xtask_build_observation.v1 --arg phase "$phase" \
    --arg run_id "${GITHUB_RUN_ID:-local}" --arg run_attempt "${GITHUB_RUN_ATTEMPT:-local}" \
    --arg image_id "${MEASUREMENT_IMAGE_ID:-host}" --arg image_digest "${MEASUREMENT_IMAGE_DIGEST:-host}" \
    --argjson command "$command_json" --argjson exit_code "$status" --argjson reader_exit "$reader_status" --argjson reader_timed_out "$reader_timed_out" \
    --argjson wall_seconds "$((stop-start))" \
    --slurpfile before "$prefix.before.json" --slurpfile after "$prefix.after.json" --slurpfile stderr "$prefix.stderr.json" --slurpfile storage "$prefix.storage.json" \
    '{schema_version:$schema_version,phase:$phase,run_id:$run_id,run_attempt:$run_attempt,image_id:$image_id,image_digest:$image_digest,command:$command,exit_code:$exit_code,wall_seconds:$wall_seconds,reader_exit:$reader_exit,reader_timed_out:$reader_timed_out,resource_scope:"whole command; GNU time maximum RSS is not concurrent aggregate",before:$before[0],after:$after[0],stderr:$stderr[0],storage:$storage[0],identity_stable:((($before[0]|del(.fingerprints,.fingerprints_clipped)) == ($after[0]|del(.fingerprints,.fingerprints_clipped))) and (($before[0].head//"")|test("^[0-9a-f]{40}$")) and (($before[0].tree//"")|test("^[0-9a-f]{40}$")) and (($before[0].rustc//"")|length>0) and (($before[0].cargo//"")|length>0) and ($before[0].lock_sha256!="missing") and ($before[0]|has("instrument_error")|not)),selected_trace_complete:($reader_exit==0 and ($reader_timed_out|not) and ($stderr[0].fingerprint_errors//0)==0 and ($stderr[0].unknown_dirty_reasons//0)==0 and ($stderr[0].ambiguous_fingerprint_lines//0)==0 and ($stderr[0].fingerprint_lines//0)>0 and ($stderr[0].trace_clipped|not) and ($stderr[0].dirty_clipped|not))}' > "$prefix.observation.json" \
    || echo '::warning::xtask build observation NOT_PROVEN: summary failed' >&2
echo "xtask build observation: phase=$phase cargo_exit=$status record=$prefix.observation.json" >&2
# Instrument failures cannot hide or replace the existing Cargo/proof verdict.
exit "$status"
