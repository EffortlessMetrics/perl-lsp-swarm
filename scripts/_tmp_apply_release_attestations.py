#!/usr/bin/env python3
"""One-shot exact edit for release artifact attestation hardening."""

from pathlib import Path


PATH = Path(".github/workflows/release.yml")


REPLACEMENTS = [
    (
        """on:\n  release:\n    types: [published]\n  workflow_dispatch:\n""",
        """on:\n  workflow_dispatch:\n""",
        "single manual release trigger",
    ),
    (
        """    permissions:\n      contents: write\n      id-token: write\n      attestations: write\n""",
        """    permissions:\n      contents: write\n      id-token: write\n      attestations: write\n      artifact-metadata: write\n""",
        "release attestation permissions",
    ),
    (
        """      - name: Install cargo-release\n        run: cargo install cargo-release --locked\n""",
        """      - name: Install cargo-sbom\n        run: cargo install cargo-sbom --version 0.10.0 --locked\n""",
        "pinned fail-closed SBOM tool",
    ),
    (
        """      - name: Create GitHub Release\n        if: github.event_name == 'workflow_dispatch'\n        uses: softprops/action-gh-release@718ea10b132b3b2eba29c1007bb80653f286566b  # v3.0.1\n        with:\n          files: |\n            artifacts/*/*\n            SHA256SUMS\n          body_path: release_notes.md\n          draft: false\n          prerelease: ${{ needs.release-metadata.outputs.prerelease == 'true' }}\n          tag_name: ${{ needs.release-metadata.outputs.tag }}\n          generate_release_notes: true\n        env:\n          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n\n      - name: Generate SBOM\n        run: |\n          cargo install cargo-sbom --locked || true\n          cargo sbom --output-format spdx_json_2_3 > sbom-spdx.json\n\n      - name: Upload SBOM as release asset\n        if: github.event_name == 'workflow_dispatch'\n        uses: softprops/action-gh-release@718ea10b132b3b2eba29c1007bb80653f286566b  # v3.0.1\n        with:\n          files: sbom-spdx.json\n          tag_name: ${{ needs.release-metadata.outputs.tag }}\n        env:\n          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n""",
        """      - name: Generate fail-closed SPDX SBOM\n        run: cargo sbom --output-format spdx_json_2_3 > sbom-spdx.json\n\n      - name: Attest release artifact provenance\n        uses: actions/attest@a1948c3f048ba23858d222213b7c278aabede763 # v4\n        with:\n          subject-path: |\n            artifacts/*/*\n            SHA256SUMS\n            sbom-spdx.json\n\n      - name: Attest release archives against the SBOM\n        uses: actions/attest@a1948c3f048ba23858d222213b7c278aabede763 # v4\n        with:\n          subject-checksums: SHA256SUMS\n          sbom-path: sbom-spdx.json\n\n      - name: Create GitHub Release after all assets and attestations succeed\n        uses: softprops/action-gh-release@718ea10b132b3b2eba29c1007bb80653f286566b  # v3.0.1\n        with:\n          files: |\n            artifacts/*/*\n            SHA256SUMS\n            sbom-spdx.json\n          body_path: release_notes.md\n          draft: false\n          prerelease: ${{ needs.release-metadata.outputs.prerelease == 'true' }}\n          tag_name: ${{ needs.release-metadata.outputs.tag }}\n          generate_release_notes: false\n          fail_on_unmatched_files: true\n        env:\n          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n""",
        "attest before one-shot public release",
    ),
]


def main() -> None:
    text = PATH.read_text(encoding="utf-8")
    for old, _, label in REPLACEMENTS:
        count = text.count(old)
        if count != 1:
            raise SystemExit(f"{label}: expected one exact match, found {count}")
    for old, new, _ in REPLACEMENTS:
        text = text.replace(old, new, 1)
    PATH.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
