//! Deterministic semantic digest for a prototype shape.
//!
//! Formatting whitespace is excluded. Slot order, optionality, kind, default
//! disposition, completeness, and syntax class are included. The encoding is
//! labelled and length-prefixed so a payload cannot shift across a field
//! boundary.

use super::{PrototypeCompleteness, PrototypeSlot, PrototypeSyntaxClass};

/// Stable semantic digest of a prototype shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeSemanticDigest(String);

impl PrototypeSemanticDigest {
    /// Digest text. Stable for a given semantic shape.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn from_shape(
        completeness: &PrototypeCompleteness,
        syntax_class: PrototypeSyntaxClass,
        slots: &[PrototypeSlot],
    ) -> Self {
        let mut out = String::from("prototype-shape.v1");
        fold(&mut out, "completeness", completeness.tag());
        fold(&mut out, "syntax", syntax_class.tag());
        match slots.iter().position(|slot| slot.is_optional()) {
            Some(index) => fold(&mut out, "optional_from", &index.to_string()),
            None => fold(&mut out, "optional_from", "-"),
        }
        fold(&mut out, "slot_count", &slots.len().to_string());
        for (index, slot) in slots.iter().enumerate() {
            let optional = if slot.is_optional() { "optional" } else { "required" };
            let payload =
                format!("{index}:{}:{optional}:{}", slot.kind().tag(), slot.default().tag());
            fold(&mut out, "slot", &payload);
        }
        Self(out)
    }
}

impl AsRef<str> for PrototypeSemanticDigest {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

fn fold(out: &mut String, label: &str, value: &str) {
    out.push('\u{1f}');
    out.push_str(label);
    out.push('=');
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
}
