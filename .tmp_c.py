NL = chr(10)
Q = chr(39)  # single quote
p = 'scripts/test_verify_binary_identity.py'
s = open(p, encoding='utf-8').read()

anchor = '    def test_malformed_packet_is_not_proven(self) -> None:'
payload_line = 'f"payload = {payload!r}",'
mutation_lines = [
    '    @unittest.skipUnless(os.name == "posix", "shebang executables")',
    '    def test_self_mutating_staged_binary_is_rejected(self) -> None:',
    '        with tempfile.TemporaryDirectory() as directory:',
    '            executable = Path(directory, "perllsp")',
    '            payload = json.dumps(packet())',
    '            lines = [',
    '                "#!/usr/bin/env python3",',
    '                "import os, sys",',
    '                "with open(os.path.abspath(sys.argv[0]), ' + Q + 'a' + Q + ', encoding=' + Q + 'utf-8' + Q + ') as handle:",',
    '                "    handle.write(' + Q + '# mutation' + Q + ' + ' + Q + 'x' + Q + ' * 64)",',
    '                ' + payload_line,
    '                "print(payload)",',
    '            ]',
    '            executable.write_text(' + repr(NL) + '.join(lines) + ' + repr(NL) + ', encoding="utf-8")',
    '            executable.chmod(0o755)',
    '            with self.assertRaises(VerificationError) as caught:',
    '                observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 5.0)',
    '            self.assertIn("changed during observation", str(caught.exception))',
    '',
]
addition = NL.join(mutation_lines).replace('\\n', '\\n')
assert anchor in s
s = s.replace(anchor, addition + NL + anchor, 1)
open(p, 'w', encoding='utf-8', newline=NL).write(s)
import ast
ast.parse(s)
print('mutation falsifier added')
