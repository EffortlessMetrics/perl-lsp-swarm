from pathlib import Path
import re

path = Path("xtask/src/bin/compiler-proof-policy.rs")
text = path.read_text()
pattern = re.compile(
    r'''#\[derive\(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord\)\]\n#\[serde\(rename_all = "snake_case"\)\]\nenum MissingEffect \{\n    BlocksClaim,\n    BlocksStage,\n    BlocksExecutionClaim,\n    BlocksClaimWhenObservable,\n\}'''
)
replacement = '''#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum MissingEffect {
    #[serde(rename = "blocks_claim")]
    Claim,
    #[serde(rename = "blocks_stage")]
    Stage,
    #[serde(rename = "blocks_execution_claim")]
    ExecutionClaim,
    #[serde(rename = "blocks_claim_when_observable")]
    ClaimWhenObservable,
}'''
text, count = pattern.subn(replacement, text, count=1)
if count != 1:
    raise SystemExit(f"expected one MissingEffect enum, found {count}")
for old, new in [
    ("MissingEffect::BlocksClaimWhenObservable", "MissingEffect::ClaimWhenObservable"),
    ("MissingEffect::BlocksExecutionClaim", "MissingEffect::ExecutionClaim"),
    ("MissingEffect::BlocksClaim", "MissingEffect::Claim"),
    ("MissingEffect::BlocksStage", "MissingEffect::Stage"),
]:
    text = text.replace(old, new)
path.write_text(text)
