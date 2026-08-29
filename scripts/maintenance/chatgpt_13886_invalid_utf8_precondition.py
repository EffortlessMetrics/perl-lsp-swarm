#!/usr/bin/env python3
"""Add the explicit invalid-UTF-8 preconditions retained from PR #13877."""

from pathlib import Path

path = Path("xtask/src/tasks/gates.rs")
text = path.read_text(encoding="utf-8")

replacements = [
    (
        """        let mut body = b"Compiling \\xff\\xfe garbage probe\\n".to_vec();
        body.extend_from_slice(b"running 3 tests\\n");
        body.extend_from_slice(b"test result: ok. 3 passed; 0 failed\\n");
        std::fs::write(&log_path, &body).expect("write gate log");
""",
        """        let mut body = b"Compiling \\xff\\xfe garbage probe\\n".to_vec();
        body.extend_from_slice(b"running 3 tests\\n");
        body.extend_from_slice(b"test result: ok. 3 passed; 0 failed\\n");
        assert!(
            std::str::from_utf8(&body).is_err(),
            "fixture must remain invalid UTF-8 so the continuation path is exercised"
        );
        std::fs::write(&log_path, &body).expect("write gate log");
""",
        "positive invalid-byte fixture",
    ),
    (
        """        let mut compile_only = b"\\xff\\xfe Compiling probe with invalid bytes\\n".to_vec();
        compile_only.extend_from_slice(b"warning: unused import\\n");
        std::fs::write(&log_path, &compile_only).expect("rewrite gate log");
""",
        """        let mut compile_only = b"\\xff\\xfe Compiling probe with invalid bytes\\n".to_vec();
        compile_only.extend_from_slice(b"warning: unused import\\n");
        assert!(
            std::str::from_utf8(&compile_only).is_err(),
            "negative control must also exercise the invalid-byte branch"
        );
        std::fs::write(&log_path, &compile_only).expect("rewrite gate log");
""",
        "negative invalid-byte fixture",
    ),
]

for old, new, label in replacements:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    text = text.replace(old, new, 1)

path.write_text(text, encoding="utf-8")
print(f"patched {path}")
