#!/usr/bin/env python3
from pathlib import Path

source = Path("crates/perl-lsp-rs-core/src/config/mod.rs")
text = source.read_text(encoding="utf-8")

replacements = [
    (
        "remove panic sentinel",
        '''        config.ensure_system_inc_probe_with(|_, _| {
            panic!("a settled successful outcome must not launch a third probe")
        });

        assert_eq!(calls.get(), 2);
''',
        '''        config.ensure_system_inc_probe_with(|_, _| {
            calls.set(calls.get() + 1);
            SystemIncProbeOutcome::IoFailed
        });

        assert_eq!(
            calls.get(),
            2,
            "a settled successful outcome must not launch a third probe"
        );
''',
    ),
    (
        "make timeout warning true on both attempts",
        '''                    "startup @INC probe timed out; failing closed for this lookup. \\
                     The configuration cache permits at most one later retry. \\
                     Set perl.workspace.useSystemInc=false to disable probing, \\
                     or pin a faster perl interpreter."
''',
        '''                    "startup @INC probe timed out; failing closed for this lookup. \\
                     The configuration layer permits no more than two bounded attempts in total. \\
                     Set perl.workspace.useSystemInc=false to disable probing, \\
                     or pin a faster perl interpreter."
''',
    ),
]

for label, old, new in replacements:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one source match, found {count}")
    text = text.replace(old, new, 1)

source.write_text(text, encoding="utf-8")
