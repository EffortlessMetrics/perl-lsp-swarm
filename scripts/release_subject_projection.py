"""Shared candidate subject relationships; these describe sources, not execution proof."""

FIXED_SUBJECTS = (
    ("dist/SHA256SUMS", "checksum_set"),
    ("dist/sbom-spdx.json", "sbom"),
    ("dist/release-terminal-manifest.json", "terminal_manifest"),
)
SUBJECT_LIST = "attestation-subjects.sha256"


def terminal_subject_paths(archives, evidence, release_notes):
    """Preserve the producer's ordered inventory, including optional notes."""
    subjects = [*archives, *evidence, *(path for path, _ in FIXED_SUBJECTS)]
    if release_notes:
        subjects.append("release_notes.md")
    return subjects


def topology_subject_projection():
    """Describe dynamic classes separately from concrete local and external objects."""
    return {
        "sbom": {"path": FIXED_SUBJECTS[1][0], "format": "SPDX-2.3", "channel": "github_release"},
        "subject_list": {"path": SUBJECT_LIST, "role": "local_subject_list", "algorithm": "sha256"},
        "named_artifacts": {
            "dynamic_classes": ["release_archives", "validated_build_evidence"],
            "fixed_artifacts": [{"path": path, "class": subject_class} for path, subject_class in FIXED_SUBJECTS],
            "conditional_paths": [{"path": "release_notes.md", "class": "release_notes", "condition": "present"}],
        },
        "external_records": {"role": "external_attestation_records", "producer": "actions/attest", "subjects_from": SUBJECT_LIST, "execution": "not_proven"},
    }
