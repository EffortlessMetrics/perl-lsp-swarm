NL = chr(10)

# --- verifier: bounded kill-wait + pipe closing ---
p = 'scripts/verify_binary_identity.py'
s = open(p, encoding='utf-8').read()

old = '''    except (ProcessLookupError, PermissionError, OSError):
        process.kill()
    process.wait()'''
new = '''    except (ProcessLookupError, PermissionError, OSError):
        process.kill()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)'''
assert old in s
s = s.replace(old, new)

old = '''    for reader in readers:
        reader.join(timeout=5)
    if overflow.is_set():
        raise VerificationError(
            f"identity output exceeded its bounded pipe for {name}"
        )
    return returncode, bytes(stdout), bytes(stderr)'''
new = '''    try:
        for reader in readers:
            reader.join(timeout=5)
        if overflow.is_set():
            raise VerificationError(
                f"identity output exceeded its bounded pipe for {name}"
            )
        return returncode, bytes(stdout), bytes(stderr)
    finally:
        for stream in (process.stdout, process.stderr):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass'''
assert old in s
s = s.replace(old, new)
open(p, 'w', encoding='utf-8', newline=NL).write(s)

# --- tests: rename shadowing, fix overflow size, add mutation falsifier ---
p = 'scripts/test_verify_binary_identity.py'
s = open(p, encoding='utf-8').read()

for name in ('test_digest_mismatch_against_trusted_topology_row',
             'test_matching_trusted_digest_is_required_for_verified',
             'test_receipt_carries_only_the_closed_packet_projection'):
    old = '        observed = ObservedBinary('
    # rename only within these three tests: do it via their unique bodies
s = s.replace('        observed = ObservedBinary(', '        under_test = ObservedBinary(')
s = s.replace('            observed,', '            under_test,')
s = s.replace('verify(\n            self._observed(),', 'verify(\n            self._observed(),')
# the three tests used `observed` as the verify() first arg

old = '''                "print('x' * 4096, flush=True)",'''
new = '''                "import verify_binary_identity_ref",'''
# not used; overflow fix below uses the bound constant via formatted line count

# Fix overflow test to exceed MAX_PACKET_BYTES: rewrite its lines list
old_lines = '''            lines = [
                "#!/usr/bin/env python3",
                "import sys",
                "print('x' * 4096, flush=True)",
                "while True:",
                "    pass",
            ]'''
new_lines = '''            lines = [
                "#!/usr/bin/env python3",
                "import os, sys",
                "sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))",
                "limit = 128 * 1024",
                "print('x' * (limit + 4096), flush=True)",
                "while True:",
                "    pass",
            ]'''
assert old_lines in s
s = s.replace(old_lines, new_lines)

# ZpmZ7: mutation-during-observation falsifier (POSIX-only)
anchor = '    def test_malformed_packet_is_not_proven(self) -> None:'
addition = (
    '    @unittest.skipUnless(os.name == "posix", "shebang executables")' + NL
    + '    def test_self_mutating_staged_binary_is_rejected(self) -> None:' + NL
    + '        with tempfile.TemporaryDirectory() as directory:' + NL
    + '            executable = Path(directory, "perllsp")' + NL
    + '            payload = json.dumps(packet())' + NL
    + '            lines = [' + NL
    + '                "#!/usr/bin/env python3",' + NL
    + '                "import os, sys",' + NL
    + '                "with open(os.path.abspath(sys.argv[0]), 'a', encoding='utf-8') as handle:",' + NL
    + '                "    handle.write('# mutation' + 'x' * 64)",' + NL
    + '                f"payload = {payload!r}",' + NL
    + '                "print(payload)",' + NL
    + '            ]' + NL
    + '            executable.write_text(' + repr(NL).replace("'", '"') + '.join(lines) + ' + repr(NL).replace("'", '"') + ', encoding="utf-8")' + NL
    + '            executable.chmod(0o755)' + NL
    + '            with self.assertRaises(VerificationError) as caught:' + NL
    + '                observe(ExpectedBinary(executable, "perllsp", "perllsp", "server"), 5.0)' + NL
    + '            self.assertIn("changed during observation", str(caught.exception))' + NL
    + NL
)
assert anchor in s
s = s.replace(anchor, addition + anchor, 1)
open(p, 'w', encoding='utf-8', newline=NL).write(s)

# --- shell adapter: reject option-looking values ---
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

# --- adapter tests: pair-binding, honest name, require-dap negative ---
p = 'scripts/test_verify_staged_binaries.py'
s = open(p, encoding='utf-8').read()
old = '''            self.assertIn("--expected-candidate", arguments)
            self.assertIn("rc1", arguments)'''
new = '''            self.assertIn("--expected-candidate", arguments)
            self.assertIn("rc1", arguments)
            # Pair binding, not mere membership: a swapped --server/--dap
            # forwarding must fail here.
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
addition = (
    '    def test_server_only_invocation_omits_dap_coupling(self) -> None:' + NL
    + '        with tempfile.TemporaryDirectory() as directory:' + NL
    + '            root = Path(directory)' + NL
    + '            capture = root / "args.json"' + NL
    + '            fake_python = root / "python3"' + NL
    + '            fake_python.write_text(' + NL
    + '                "#!/bin/sh" + chr(10) +' + NL
    + '                "printf '%s' \\"$*\\" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().split()))' > ' + str(capture) + "'" + NL
    + '            )' + NL
    + NL
)
# simpler: build via the same pattern as the existing test but with chr(10) joins
addition = (
    '    def test_server_only_invocation_omits_dap_coupling(self) -> None:' + NL
    + '        with tempfile.TemporaryDirectory() as directory:' + NL
    + '            root = Path(directory)' + NL
    + '            capture = root / "solo-args.json"' + NL
    + '            fake_python = root / "python3"' + NL
    + '            fake_python.write_text(' + NL
    + '                "#!/bin/sh' + repr(NL).replace("'", '"') + '"' + NL
    + '                "python_args=' + chr(36) + "*'" + repr(NL).replace("'", '"') + '"' + NL
    + '                "printf '%s' ' + chr(36) + 'python_args | "' + NL
    + '                "python3 -c 'import json,sys; print(json.dumps(sys.stdin.read().split()))' "' + NL
    + '                f"> {capture!s}' + repr(NL).replace("'", '"') + '"' + NL
    + '                ",' + NL
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
for path in ('scripts/verify_binary_identity.py', 'scripts/test_verify_binary_identity.py',
             'scripts/test_verify_staged_binaries.py'):
    ast.parse(open(path, encoding='utf-8').read())
print('all edits applied + python syntax OK')
