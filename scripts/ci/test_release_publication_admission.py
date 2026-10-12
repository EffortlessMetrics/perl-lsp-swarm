"""Discriminators for the pure, explicitly nonqualifying #14406 leaf."""
import copy
from pathlib import Path
import unittest

import release_publication_admission as leaf


def require(value, message):
    if not value:
        raise RuntimeError(message)


def rejects(call):
    try:
        call()
    except leaf.AdmissionError:
        return
    raise RuntimeError("invalid input was accepted")


def policy():
    return {
        "schema_version": "perl_lsp.release_publication_policy.v1",
        "profiles": {"public-beta-rc": {
            "required": ["github_release"], "conditional": ["open_vsx"],
            "forbidden": ["crates_io", "vscode_marketplace", "docker", "homebrew"],
            "requires_prerelease": True,
        }},
    }


def packet():
    return {
        "schema_version": "perl_lsp.release_publication_admission.v1",
        "subject": {"source_sha": "a" * 40, "prepared_sha": "b" * 40,
                    "tag": "v0.18.0-rc.1", "version": "0.18.0-rc.1",
                    "prerelease": True, "topology_sha256": "c" * 64},
        "policy": {"repository": "EffortlessMetrics/perl-lsp-swarm",
                   "workflow_revision": "d" * 40, "sha256": "e" * 64,
                   "profile_id": "public-beta-rc"},
        "producer": {"repository": "EffortlessMetrics/perl-lsp-swarm",
                     "workflow_id": 12, "ref": "refs/heads/release-candidate",
                     "head_sha": "b" * 40, "run_id": 34, "run_attempt": 2},
        "artifact": {"id": 56, "sha256": "f" * 64},
        "input_packet_sha256": "1" * 64, "terminal_manifest_sha256": "2" * 64,
        "channels": ["github_release"],
    }


def selection():
    return leaf.derive_selection(policy(), "public-beta-rc", True, ["github_release"])


class AdmissionTests(unittest.TestCase):
    def test_rc_law_is_nonqualifying_and_deterministic(self):
        selected = selection()
        require(selected == ("github_release",), "wrong derived permission")
        p = packet()
        result = leaf.validate_subject(p, copy.deepcopy(p), selected)
        require(result.qualification == "not_proven", "pure identity granted eligibility")
        require("complete_vsix" in result.missing_adapters, "missing VSIX proof hidden")
        require("producer_authentication" in result.missing_adapters, "provenance hidden")
        require(result == leaf.validate_subject(dict(reversed(list(p.items()))), p, selected),
                "map ordering changed result")

    def test_every_exact_subject_join_is_load_bearing(self):
        for section in ("subject", "policy", "producer", "artifact"):
            for key, value in packet()[section].items():
                changed = packet()
                changed[section][key] = (value + 1 if type(value) is int else
                                        not value if type(value) is bool else "9" + value[1:])
                rejects(lambda: leaf.validate_subject(changed, packet(), selection()))
        for key in ("input_packet_sha256", "terminal_manifest_sha256"):
            changed = packet()
            changed[key] = "9" * 64
            rejects(lambda: leaf.validate_subject(changed, packet(), selection()))

    def test_no_unknown_or_forbidden_publisher_alias(self):
        for channel in ("crates_io", "vscode_marketplace", "docker", "homebrew",
                        "Scoop", "Chocolatey", "WinGet", "github", "latest", "stable"):
            rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", True,
                                                  ["github_release", channel]))
        rejects(lambda: leaf.validate_subject(packet(), packet(), ("github_release", "docker")))

    def test_open_vsx_requires_real_missing_adapter(self):
        rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", True,
                                              ["github_release", "open_vsx"]))
        p = packet()
        p["open_vsx_evidence"] = {"status": "pass"}
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_missing_required_channel_and_duplicates_refuse(self):
        for channels in ([], ["github_release", "github_release"]):
            rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", True, channels))
        p = packet()
        p["channels"] = []
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_unknown_profile_and_prerelease_false_refuse(self):
        rejects(lambda: leaf.derive_selection(policy(), "stable", True, ["github_release"]))
        rejects(lambda: leaf.derive_selection(policy(), "public-beta-rc", False, ["github_release"]))
        p = packet()
        p["subject"]["prerelease"] = False
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_unknown_skip_fields_refuse_independently(self):
        p = packet()
        leaf.validate_subject(p, p, selection())
        p["skip_crates"] = p["skip_marketplace"] = p["skip_open_vsx"] = True
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_native_boolean_and_untrusted_policy_claim_do_not_grant(self):
        p = packet()
        p["native"] = True
        rejects(lambda: leaf.validate_subject(p, p, selection()))
        p = packet()
        p["policy"]["trusted"] = True
        rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_closed_json_and_malformed_utf8(self):
        require(leaf.parse_object(b'{"a":1}') == {"a": 1}, "valid JSON refused")
        for raw in (b'{"a":1,"a":2}', b'{"a":{"b":1,"b":2}}', b'{"a":NaN}',
                    b'{"a":Infinity}', b'{"a":1e400}', b'\xff', b'[]', b'{} trailing'):
            rejects(lambda: leaf.parse_object(raw))
        for section in (None, "subject", "policy", "producer", "artifact"):
            p = packet()
            target = p if section is None else p[section]
            target["unknown"] = "x"
            rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_malformed_exact_identity_and_boolean_numeric_refuse(self):
        for section, key, value in (("producer", "run_id", True),
                                    ("producer", "run_attempt", 0),
                                    ("artifact", "id", -1),
                                    ("subject", "source_sha", "A" * 40),
                                    ("artifact", "sha256", "short")):
            p = packet()
            p[section][key] = value
            rejects(lambda: leaf.validate_subject(p, p, selection()))

    def test_policy_partition_and_checked_in_source(self):
        path = Path(__file__).resolve().parents[2] / "policy/release-publication-admission.json"
        require(leaf.parse_object(path.read_bytes()) == policy(), "checked policy drift")
        p = policy()
        p["profiles"]["public-beta-rc"]["conditional"].append("github_release")
        rejects(lambda: leaf.derive_selection(p, "public-beta-rc", True, ["github_release"]))
        p = policy()
        p["profiles"]["public-beta-rc"]["forbidden"].append("scoop")
        rejects(lambda: leaf.derive_selection(p, "public-beta-rc", True, ["github_release"]))
