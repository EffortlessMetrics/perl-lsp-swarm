from pathlib import Path

path = Path("xtask/src/bin/compiler-proof-policy.rs")
text = path.read_text()
old = '''#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum MissingEffect {
'''
new = '''#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[allow(clippy::enum_variant_names)]
#[serde(rename_all = "snake_case")]
enum MissingEffect {
'''
if new not in text:
    if text.count(old) != 1:
        raise SystemExit("expected one MissingEffect enum declaration")
    path.write_text(text.replace(old, new, 1))
