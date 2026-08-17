NL = chr(10)
D = chr(36)  # dollar

# shell adapter: reject option-looking values for value-taking options
p = 'scripts/verify-staged-binaries.sh'
s = open(p, encoding='utf-8').read()
old = '''        --server|--dap|--expected-version|--expected-target|--expected-candidate|--receipt)
            [ "$#" -ge 2 ] || usage'''
new = '''        --server|--dap|--expected-version|--expected-target|--expected-candidate|--receipt)
            [ "$#" -ge 2 ] || usage
            case "$2" in --*) usage ;; esac'''
assert old in s
s = s.replace(old, new)
open(p, 'w', encoding='utf-8', newline=NL).write(s)

# adapter tests: pair-binding assertions, honest rename, server-only negative
p = 'scripts/test_verify_staged_binaries.py'
s = open(p, encoding='utf-8').read()

old = '''            self.assertIn("--expected-candidate", arguments)
            self.assertIn("rc1", arguments)'''
new = '''            self.assertIn("--expected-candidate", arguments)
            self.assertIn("rc1", arguments)
            # Pair binding, not mere membership: swapped --server/--dap
            # forwarding fails here.
            self.assertEqual(arguments[arguments.index("--server") + 1], "/stage/perllsp")
            self.assertEqual(arguments[arguments.index("--dap") + 1], "/stage/perl-dap")
            self.assertEqual(
                arguments[arguments.index("--expected-version") + 1], "0.18.0"
            )'''
assert old in s
s = s.replace(old, new)

old = '    def test_missing_required_option_fails_before_verifier(self) -> None:'
new = '    def test_missing_required_option_exits_with_usage_before_verifier(self) -> None:'
assert old in s
s = s.replace(old, new)

anchor = '    def test_unknown_positional_argument_is_rejected(self) -> None:'
test_lines = [
    '    def test_server_only_invocation_omits_dap_coupling(self) -> None:',
    '        with tempfile.TemporaryDirectory() as directory:',
    '            root = Path(directory)',
    '            capture = root / "solo-args.json"',
    '            fake_python = root / "python3"',
    '            fake_python.write_text(',
    '                "#!/bin/sh' + repr(NL).replace(chr(39), chr(34)) + '"',
]
# Build the fake-python script body carefully with explicit quoting.
sh_lines = [
    '#!/bin/sh',
    'python_args=' + D + '*',
    'printf ' + chr(39) + '%s' + chr(39) + ' ' + D + 'python_args | '
    + 'python3 -c ' + chr(39) + 'import json,sys; '
    + 'print(json.dumps(sys.stdin.read().split()))' + chr(39) + ' '
    + '> ' + chr(39) + '{capture}' + chr(39),
]
body = NL.join(sh_lines).replace('{capture}', '" + str(capture) + "')
body_literal = '"' + body.replace(chr(92), chr(92) * 2).replace('"', chr(92) + '"') + '"'

addition = (
    '    def test_server_only_invocation_omits_dap_coupling(self) -> None:' + NL
    + '        with tempfile.TemporaryDirectory() as directory:' + NL
    + '            root = Path(directory)' + NL
    + '            capture = root / "solo-args.json"' + NL
    + '            fake_python = root / "python3"' + NL
    + '            fake_python.write_text(' + NL
    + '                ' + body_literal + ',' + NL
    + '                encoding="utf-8",' + NL
    + '            )' + NL
    + '            fake_python.chmod(0o755)' + NL
    + '            environment = os.environ.copy()' + NL
    + '            environment["PERL_LSP_PYTHON"] = str(fake_python)' + NL
    + '            completed = subprocess.run(' + NL
    + '                [' + NL
    + '                    "bash",' + NL
    + '                    str(WRAPPER),' + NL
    + '                    "--server", "/stage/perllsp",' + NL
    + '                    "--expected-version", "0.18.0",' + NL
    + '                    "--expected-target", "x86_64-unknown-linux-gnu",' + NL
    + '                    "--receipt", "/stage/identity.json",' + NL
    + '                ],' + NL
    + '                env=environment,' + NL
    + '                stdout=subprocess.PIPE,' + NL
    + '                stderr=subprocess.PIPE,' + NL
    + '                check=False,' + NL
    + '                text=True,' + NL
    + '            )' + NL
    + '            self.assertEqual(completed.returncode, 0, completed.stderr)' + NL
    + '            arguments = json.loads(capture.read_text(encoding="utf-8"))' + NL
    + '            self.assertNotIn("--dap", arguments)' + NL
    + '            self.assertNotIn("--require-dap", arguments)' + NL
    + NL
)
assert anchor in s
s = s.replace(anchor, addition + anchor, 1)
open(p, 'w', encoding='utf-8', newline=NL).write(s)
import ast
ast.parse(s)
print('adapter edits OK')
