#!/usr/bin/env bash
# Diagnose existing Cargo work (#16126/#9178); never reuse or adjudicate artifacts.
# Usage: observe-xtask-build.sh PHASE OUTPUT_DIR -- COMMAND [ARG...]
# GNU time measures the whole command (including analysis for cargo xtask), not
# aggregate simultaneous RSS. Cargo's own Finished lines isolate build duration.
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

hash_file() {
    if [[ -f "$1" ]]; then sha256sum "$1" | cut -d ' ' -f 1; else printf 'missing'; fi
}

snapshot() {
    # This is an environment/default hint, not Cargo metadata target resolution.
    local target=${CARGO_TARGET_DIR:-$PWD/target} home=${CARGO_HOME:-$HOME/.cargo}
    local hashes configs='' cursor=$PWD file value flags='' fingerprints='' size count=0 bytes=0 clipped=false
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
    # Retain only scalar fingerprint hashes as decimal strings; jq must not
    # round u64 values. Never upload local/rustflags fields with environment data.
    for file in "$target"/debug/.fingerprint/{xtask,serde,serde_json,proc-macro2,perl-parser-core}-*/*.json; do
        [[ -f "$file" && ! -L "$file" ]] || continue
        size=$(wc -c < "$file")
        if (( count >= 32 || bytes + size > 65536 )); then clipped=true; continue; fi
        hashes=$(LC_ALL=C grep -oE '"(rustc|target|profile|path|config|compile_kind)":[[:space:]]*[0-9]+' "$file" | jq -Rn '[inputs|split(":")|{key:(.[0]|gsub("\"";"")),value:(.[1]|gsub("\\s";""))}]|from_entries')
        fingerprints+=$(jq -cn --arg unit "$(basename "$(dirname "$file")")/$(basename "$file")" --arg sha256 "$(hash_file "$file")" --argjson hashes "$hashes" '{unit:$unit,sha256:$sha256,hashes:$hashes}')$'\n'
        count=$((count + 1)); bytes=$((bytes + size))
    done
    jq -n --arg head "$(git rev-parse HEAD)" --arg tree "$(git rev-parse 'HEAD^{tree}')" \
        --arg cwd "$PWD" --arg cargo_home "$home" --arg target_dir_hint "$target" \
        --arg rustc "$(rustc -vV)" --arg cargo "$(cargo -vV)" \
        --arg rustc_path "$(command -v rustc)" --arg cargo_path "$(command -v cargo)" \
        --arg sysroot "$(rustc --print sysroot)" \
        --arg lock_sha256 "$(hash_file Cargo.lock)" --arg manifest_sha256 "$(hash_file Cargo.toml)" \
        --arg xtask_manifest_sha256 "$(hash_file xtask/Cargo.toml)" --arg toolchain_sha256 "$(hash_file rust-toolchain.toml)" \
        --arg configs "$configs" --arg flags_sha256 "$(printf '%s' "$flags" | sha256sum | cut -d ' ' -f 1)" \
        --argjson fingerprints "$(printf '%s' "$fingerprints" | jq -s .)" --argjson fingerprints_clipped "$clipped" \
        '{head:$head,tree:$tree,cwd:$cwd,cargo_home:$cargo_home,target_dir_hint:$target_dir_hint,rustc:$rustc,cargo:$cargo,rustc_path:$rustc_path,cargo_path:$cargo_path,sysroot:$sysroot,lock_sha256:$lock_sha256,manifest_sha256:$manifest_sha256,xtask_manifest_sha256:$xtask_manifest_sha256,toolchain_sha256:$toolchain_sha256,configs:$configs,flags_sha256:$flags_sha256,fingerprints:$fingerprints,fingerprints_clipped:$fingerprints_clipped}'
}

snapshot > "$prefix.before.json" || printf '{"instrument_error":"before snapshot failed"}\n' > "$prefix.before.json"
# Fingerprint output is diagnostic only. Bound retained data and suppress those
# verbose trace lines from the job console; preserve all ordinary Cargo stderr.
collect_stderr() {
LC_ALL=C exec awk -v trace="$prefix.trace.log" -v reasons="$prefix.dirty.log" -v finished="$prefix.finished.log" -v summary="$prefix.stderr.json" '
    /cargo::core::compiler::fingerprint/ {
        traces++
        # Cargo Debug dirty reasons can contain old/new environment or flags.
        # Keep the native reason kind and package context, omit reason values.
        if ($0 ~ /dirty: /) {
            reason=$0; sub(/^.*dirty: /,"",reason); sub(/[^[:alnum:]_].*$/,"",reason)
            sub(/dirty: .*/,"dirty: " reason " (details omitted)")
        } else if ($0 ~ /fingerprint error for/) { errors++ }
        else if ($0 ~ /err:/) { sub(/err: .*/,"err: details omitted") }
        else if ($0 !~ /fingerprint at:|write fingerprint|fingerprint dirty for/) { filtered++; next }
        n=length($0)+1
        if (trace_bytes+n <= 1048576) { print > trace; trace_bytes+=n } else trace_clipped=1
        if ($0 ~ /fingerprint dirty for|dirty:|fingerprint error for|err:/) {
            dirty++; if (reason_bytes+n <= 262144) { print > reasons; reason_bytes+=n } else reasons_clipped=1
        }
        next
    }
    /Compiling / { compiling++ }
    /Finished .*target\(s\) in / { print > finished }
    { print > "/dev/stderr"; fflush("/dev/stderr") }
    END { printf "{\"fingerprint_lines\":%d,\"dirty_lines\":%d,\"compiling_messages\":%d,\"filtered_fingerprint_lines\":%d,\"fingerprint_errors\":%d,\"trace_clipped\":%s,\"dirty_clipped\":%s}\n", traces,dirty,compiling,filtered,errors,trace_clipped?"true":"false",reasons_clipped?"true":"false" > summary }
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
command_json=$(printf '%s\0' "$@" | jq -Rs 'split("\u0000")[:-1]')
jq -n --arg schema_version xtask_build_observation.v1 --arg phase "$phase" \
    --arg run_id "${GITHUB_RUN_ID:-local}" --arg run_attempt "${GITHUB_RUN_ATTEMPT:-local}" \
    --arg image_id "${MEASUREMENT_IMAGE_ID:-host}" --arg image_digest "${MEASUREMENT_IMAGE_DIGEST:-host}" \
    --argjson command "$command_json" --argjson exit_code "$status" --argjson reader_exit "$reader_status" --argjson reader_timed_out "$reader_timed_out" \
    --argjson wall_seconds "$((stop-start))" \
    --slurpfile before "$prefix.before.json" --slurpfile after "$prefix.after.json" --slurpfile stderr "$prefix.stderr.json" \
    '{schema_version:$schema_version,phase:$phase,run_id:$run_id,run_attempt:$run_attempt,image_id:$image_id,image_digest:$image_digest,command:$command,exit_code:$exit_code,wall_seconds:$wall_seconds,reader_exit:$reader_exit,reader_timed_out:$reader_timed_out,resource_scope:"whole command; GNU time maximum RSS is not concurrent aggregate",before:$before[0],after:$after[0],stderr:$stderr[0],identity_stable:((($before[0]|del(.fingerprints,.fingerprints_clipped)) == ($after[0]|del(.fingerprints,.fingerprints_clipped))) and (($before[0].head//"")|test("^[0-9a-f]{40}$")) and (($before[0].tree//"")|test("^[0-9a-f]{40}$")) and (($before[0].rustc//"")|length>0) and (($before[0].cargo//"")|length>0) and ($before[0].lock_sha256!="missing") and ($before[0]|has("instrument_error")|not)),selected_trace_complete:($reader_exit==0 and ($reader_timed_out|not) and ($stderr[0].fingerprint_errors//0)==0 and ($stderr[0].fingerprint_lines//0)>0 and ($stderr[0].trace_clipped|not) and ($stderr[0].dirty_clipped|not))}' > "$prefix.observation.json" \
    || echo '::warning::xtask build observation NOT_PROVEN: summary failed' >&2
echo "xtask build observation: phase=$phase cargo_exit=$status record=$prefix.observation.json" >&2
# Instrument failures cannot hide or replace the existing Cargo/proof verdict.
exit "$status"
