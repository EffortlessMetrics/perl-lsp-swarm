from pathlib import Path
import re

path = Path("xtask/src/bin/compiler-concepts.rs")
text = path.read_text()
pattern = re.compile(
    r"        let source = COMMITTED_LEDGER\.replacen\(.*?        assert!\(toml::from_str::<ConceptLedger>\(&source\)\.is_err\(\)\);",
    re.S,
)
replacement = '''        let source = COMMITTED_LEDGER.replacen(
            "ast_kinds = [\\"AmperCall\\"]",
            "",
            1,
        );
        assert_ne!(source, COMMITTED_LEDGER);
        assert!(toml::from_str::<ConceptLedger>(&source).is_err());'''
updated, count = pattern.subn(replacement, text, count=1)
if count != 1:
    raise SystemExit(f"expected one AST-field mutation test, found {count}")
path.write_text(updated)
