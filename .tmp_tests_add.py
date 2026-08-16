p = 'scripts/test_verify_binary_identity.py'
s = open(p, encoding='utf-8').read()

anchor = '    def test_malformed_packet_is_not_proven(self) -> None:'
shebang_line = '#!/usr/bin/env python3'
addition = '''    def test_digest_mismatch_against_trusted_topology_row(self) -> None:
        observed = ObservedBinary(
            expected=ExpectedBinary(
                Path("staged-perllsp"), "perllsp", "perllsp", "server", expected_sha256="a" * 64
            ),
            sha256="b" * 64,
            packet=packet(),
        )
        receipt = verify(
            observed,
            None,
            expected_version=packet()["binary"]["version"],
            expected_target=None,
            expected_candidate=None,
            require_dap=False,
        )
        self.assertIn("server_sha256_mismatch", receipt["reasons"])
        self.assertEqual(receipt["verdict"], "mismatch")

    def test_matching_trusted_digest_is_required_for_verified(self) -> None:
        digest = "c" * 64
        observed = ObservedBinary(
            expected=ExpectedBinary(
                Path("staged-perllsp"), "perllsp", "perllsp", "server", expected_sha256=digest
            ),
            sha256=digest,
            packet=packet(),
        )
        receipt = verify(
            observed,
            None,
            expected_version=packet()["binary"]["version"],
            expected_target=None,
            expected_candidate=None,
            require_dap=False,
        )
        self.assertEqual(receipt["verdict"], "verified")

    def test_receipt_carries_only_the_closed_packet_projection(self) -> None:
        raw = packet()
        raw["private_path"] = "/home/user/secret"
        raw["binary"]["oversized"] = "x" * 2048
        observed = ObservedBinary(
            expected=ExpectedBinary(Path("staged-perllsp"), "perllsp", "perllsp", "server"),
            sha256="d" * 64,
            packet=raw,
        )
        receipt = verify(
            observed,
            None,
            expected_version=raw["binary"]["version"],
            expected_target=None,
            expected_candidate=None,
            require_dap=False,
        )
        projected = receipt["binaries"][0]["packet_projection"]
        self.assertNotIn("packet", receipt["binaries"][0])
        self.assertNotIn("private_path", json.dumps(receipt))
        self.assertNotIn("oversized", json.dumps(receipt))
        self.assertEqual(projected["role"], "server")
        self.assertEqual(projected["version"], raw["binary"]["version"])

    @unittest.skipUnless(os.name == "posix", "shebang executables")
    def test_oversized_output_is_killed_while_running(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory, "perllsp")
            script = (
                shebang_line
                + "\n"
                + "import sys\n"
                + "print('x' * 4096, flush=True)\n"
                + "while True:\n"
                + "    pass\n"
            )
            executable.write_text(script, encoding="utf-8")
            executable.chmod(0o755)
            with self.assertRaises(VerificationError) as caught:
                observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 3.0)
            self.assertIn("exceeded its bounded pipe", str(caught.exception))

'''
assert anchor in s
s = s.replace(anchor, addition + anchor, 1)

# os import needed for the skip
if '\nimport os\n' not in s:
    s = s.replace('import json\n', 'import json\nimport os\n', 1)

open(p, 'w', encoding='utf-8', newline='\n').write(s)
import ast
ast.parse(s)
print('tests added + syntax OK')
