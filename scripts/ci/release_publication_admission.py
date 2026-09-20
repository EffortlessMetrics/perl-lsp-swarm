"""Pure #14406 permission/identity laws; never authenticated release eligibility.

Policy and expected context must be supplied by future trusted adapters. This
module performs no IO and does not implement those adapters or native entry.
"""
from dataclasses import dataclass
import json
import math
import re

CHANNELS = frozenset(("github_release", "crates_io", "vscode_marketplace", "open_vsx",
                      "docker", "homebrew"))
PROFILE = "public-beta-rc"
SCHEMA = "perl_lsp.release_publication_admission.v1"
POLICY_SCHEMA = "perl_lsp.release_publication_policy.v1"


class AdmissionError(ValueError):
    """A permission or identity law was not established; no authority is emitted."""


@dataclass(frozen=True)
class SubjectComparison:
    """Local law result only, with permanently absent integration qualification."""
    channels: tuple[str, ...]
    qualification: str = "not_proven"
    missing_adapters: tuple[str, ...] = (
        "trusted_policy_source", "canonical_topology", "terminal_candidate_bytes",
        "complete_vsix", "producer_authentication", "predecessor_graph",
    )


def _object(value, keys, label):
    if type(value) is not dict or set(value) != set(keys):
        raise AdmissionError(f"{label}: exact object fields required")


def _text(value, label):
    if type(value) is not str or not value.strip():
        raise AdmissionError(f"{label}: nonempty string required")


def _hex(value, length, label):
    if type(value) is not str or re.fullmatch(rf"[0-9a-f]{{{length}}}", value) is None:
        raise AdmissionError(f"{label}: lowercase {length}-hex required")


def _positive(value, label):
    if type(value) is not int or value <= 0:
        raise AdmissionError(f"{label}: positive integer required")


def _channels(value, label):
    if type(value) not in (list, tuple) or any(type(item) is not str for item in value):
        raise AdmissionError(f"{label}: channel sequence required")
    if len(set(value)) != len(value) or not set(value) <= CHANNELS:
        raise AdmissionError(f"{label}: duplicate or unknown publisher")
    return frozenset(value)


def parse_object(raw: bytes) -> dict:
    """Decode duplicate-aware JSON bytes; callers retain and bind these same bytes."""
    if type(raw) is not bytes:
        raise AdmissionError("JSON input must be captured bytes")

    def pairs(rows):
        result = {}
        for key, value in rows:
            if key in result:
                raise AdmissionError(f"duplicate JSON field: {key}")
            result[key] = value
        return result

    def constant(value):
        raise AdmissionError(f"nonfinite JSON constant: {value}")

    def finite_float(value):
        parsed = float(value)
        if not math.isfinite(parsed):
            raise AdmissionError("nonfinite JSON number")
        return parsed

    try:
        result = json.loads(raw.decode("utf-8"), object_pairs_hook=pairs,
                            parse_constant=constant, parse_float=finite_float)
    except (UnicodeDecodeError, ValueError, RecursionError) as error:
        raise AdmissionError("malformed UTF-8 JSON object") from error
    if type(result) is not dict:
        raise AdmissionError("JSON root must be an object")
    return result


def derive_selection(policy, profile_id, prerelease, requested_channels):
    """Derive a permission subset; no evidence for conditional Open VSX exists here."""
    _object(policy, ("schema_version", "profiles"), "policy")
    if policy["schema_version"] != POLICY_SCHEMA or profile_id != PROFILE:
        raise AdmissionError("unknown policy schema or profile")
    _object(policy["profiles"], (PROFILE,), "profiles")
    profile = policy["profiles"][PROFILE]
    _object(profile, ("required", "conditional", "forbidden", "requires_prerelease"), "profile")
    required = _channels(profile["required"], "required")
    conditional = _channels(profile["conditional"], "conditional")
    forbidden = _channels(profile["forbidden"], "forbidden")
    if required & conditional or required & forbidden or conditional & forbidden:
        raise AdmissionError("policy channel categories overlap")
    if required | conditional | forbidden != CHANNELS:
        raise AdmissionError("policy does not classify the complete publisher domain")
    # This v1 law recognizes only the accepted initial RC profile. A future profile
    # needs a separately accepted contract, not permissive candidate policy data.
    if (required != {"github_release"} or conditional != {"open_vsx"}
            or forbidden != CHANNELS - required - conditional
            or profile["requires_prerelease"] is not True):
        raise AdmissionError("policy differs from accepted initial RC ceiling")
    if prerelease is not True:
        raise AdmissionError("RC publication requires prerelease; latest/stable unavailable")
    requested = _channels(requested_channels, "requested channels")
    if not required <= requested or requested & forbidden:
        raise AdmissionError("required publisher absent or forbidden publisher requested")
    if requested & conditional:
        raise AdmissionError("Open VSX concrete evidence adapter is absent")
    return tuple(sorted(required))


def _packet(value):
    _object(value, ("schema_version", "subject", "policy", "producer", "artifact",
                    "input_packet_sha256", "terminal_manifest_sha256", "channels"), "packet")
    if value["schema_version"] != SCHEMA:
        raise AdmissionError("unknown admission schema")
    subject = value["subject"]
    _object(subject, ("source_sha", "prepared_sha", "tag", "version", "prerelease",
                      "topology_sha256"), "subject")
    for field in ("source_sha", "prepared_sha"):
        _hex(subject[field], 40, field)
    _hex(subject["topology_sha256"], 64, "topology_sha256")
    for field in ("tag", "version"):
        _text(subject[field], field)
    # No second version parser or source/prepared correspondence assertion here.
    if subject["prerelease"] is not True:
        raise AdmissionError("prerelease must be true")
    policy = value["policy"]
    _object(policy, ("repository", "workflow_revision", "sha256", "profile_id"), "policy identity")
    _text(policy["repository"], "policy repository")
    _hex(policy["workflow_revision"], 40, "workflow revision")
    _hex(policy["sha256"], 64, "policy digest")
    if policy["profile_id"] != PROFILE:
        raise AdmissionError("unknown policy profile")
    producer = value["producer"]
    _object(producer, ("repository", "workflow_id", "ref", "head_sha", "run_id", "run_attempt"),
            "producer")
    for field in ("repository", "ref"):
        _text(producer[field], field)
    _hex(producer["head_sha"], 40, "producer head")
    for field in ("workflow_id", "run_id", "run_attempt"):
        _positive(producer[field], field)
    artifact = value["artifact"]
    _object(artifact, ("id", "sha256"), "artifact")
    _positive(artifact["id"], "artifact id")
    _hex(artifact["sha256"], 64, "artifact digest")
    for field in ("input_packet_sha256", "terminal_manifest_sha256"):
        _hex(value[field], 64, field)
    _channels(value["channels"], "packet channels")


def validate_subject(candidate, expected, selection):
    """Check exact independently expected fields, without authenticating either input."""
    _packet(candidate)
    _packet(expected)
    _channels(selection, "derived selection")
    if tuple(selection) != ("github_release",):
        raise AdmissionError("selection outside the currently implemented RC law")
    if candidate != expected:
        raise AdmissionError("candidate differs from independently expected exact subject")
    if candidate["channels"] != list(selection):
        raise AdmissionError("packet channels differ from derived canonical selection")
    return SubjectComparison(tuple(selection))
