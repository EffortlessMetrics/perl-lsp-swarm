//! Structured hover card presentation below LSP `MarkupContent` and above semantic facts.
//!
//! Generic cards build a [`HoverCard`] here; Markdown and plaintext render independently.

use super::{Value, json};
use crate::util::escape_markdown_text;

/// Remaining hover card builders that still emit hard-coded Markdown JSON directly.
///
/// Denominator for the presentation migration train (#9047).
pub const UNMIGRATED_DIRECT_MARKDOWN_HOVER_BUILDERS: &[&str] = &[
    "hover_cards::unresolved_qualified_token_hover",
    "closed_file_hover",
    "live_compiler_hover",
    "regex_hover",
    "hover.rs (inline json! hover cards)",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverCard {
    blocks: Vec<HoverBlock>,
}

impl HoverCard {
    pub fn new(blocks: Vec<HoverBlock>) -> Self {
        Self { blocks }
    }

    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        for (idx, block) in self.blocks.iter().enumerate() {
            if idx > 0 {
                out.push_str("\n\n");
            }
            block.render_markdown(&mut out);
        }
        out
    }

    pub fn render_plaintext(&self) -> String {
        let mut out = String::new();
        for (idx, block) in self.blocks.iter().enumerate() {
            if idx > 0 {
                out.push_str("\n\n");
            }
            block.render_plaintext(&mut out);
        }
        out
    }

    pub fn to_lsp_hover(&self, markdown: bool) -> Value {
        if markdown {
            json!({
                "contents": {
                    "kind": "markdown",
                    "value": self.render_markdown(),
                },
            })
        } else {
            json!({
                "contents": {
                    "kind": "plaintext",
                    "value": self.render_plaintext(),
                },
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverBlock {
    PhaseBlockHeading { phase: String },
    TimingLabel { text: &'static str },
    AuthoredProse { text: &'static str },
    UserProse { text: String },
    CodeLine { code: String },
    BoldTitle { title: String },
    MethodModifierHeading { modifier_kind: String },
    MethodModifierSummary { method_name: String, kind_label: &'static str },
    PackageDefinedLine { package: String },
    PresentationLink { label: String, url: String },
}

impl HoverBlock {
    fn render_markdown(&self, out: &mut String) {
        match self {
            Self::PhaseBlockHeading { phase } => {
                out.push_str("**Phase Block: `");
                out.push_str(phase);
                out.push_str("`**");
            }
            Self::TimingLabel { text } => {
                out.push('_');
                out.push_str(text);
                out.push('_');
            }
            Self::AuthoredProse { text } => out.push_str(text),
            Self::UserProse { text } => out.push_str(&escape_markdown_text(text)),
            Self::CodeLine { code } => {
                out.push('`');
                out.push_str(code);
                out.push('`');
            }
            Self::BoldTitle { title } => {
                out.push_str("**");
                out.push_str(title);
                out.push_str("**");
            }
            Self::MethodModifierHeading { modifier_kind } => {
                out.push_str("**Method Modifier (`");
                out.push_str(modifier_kind);
                out.push_str("`)**");
            }
            Self::MethodModifierSummary { method_name, kind_label } => {
                out.push('`');
                out.push_str(method_name);
                out.push_str("` — ");
                out.push_str(kind_label);
            }
            Self::PackageDefinedLine { package } => {
                out.push_str("Defined in `");
                out.push_str(package);
                out.push('`');
            }
            Self::PresentationLink { label, url } => {
                out.push_str("    [");
                out.push_str(label);
                out.push_str("](");
                out.push_str(url);
                out.push(')');
            }
        }
    }

    fn render_plaintext(&self, out: &mut String) {
        match self {
            Self::PhaseBlockHeading { phase } => {
                out.push_str("Phase Block: ");
                out.push_str(phase);
            }
            Self::TimingLabel { text } => out.push_str(text),
            Self::AuthoredProse { text } => out.push_str(&strip_authored_markdown(text)),
            Self::UserProse { text } => out.push_str(text),
            Self::CodeLine { code } => out.push_str(code),
            Self::BoldTitle { title } => out.push_str(title),
            Self::MethodModifierHeading { modifier_kind } => {
                out.push_str("Method Modifier (");
                out.push_str(modifier_kind);
                out.push(')');
            }
            Self::MethodModifierSummary { method_name, kind_label } => {
                out.push_str(method_name);
                out.push_str(" — ");
                out.push_str(&strip_authored_markdown(kind_label));
            }
            Self::PackageDefinedLine { package } => {
                out.push_str("Defined in ");
                out.push_str(package);
            }
            Self::PresentationLink { label, .. } => out.push_str(label),
        }
    }
}

/// Strip presentation markdown from server-authored static prose only.
fn strip_authored_markdown(text: &str) -> String {
    text.replace("**", "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::must_some;

    fn phase_begin_card() -> HoverCard {
        HoverCard::new(vec![
            HoverBlock::PhaseBlockHeading { phase: "BEGIN".to_string() },
            HoverBlock::TimingLabel { text: "Compile-time execution" },
            HoverBlock::AuthoredProse {
                text: "Runs **as soon as the block is fully parsed**, at **compile time**, \
before the rest of the file compiles. \
Multiple `BEGIN` blocks run in declaration order (FIFO). \
Use for loading modules, setting up the environment, and other tasks that \
must complete before compilation continues.",
            },
            HoverBlock::PresentationLink {
                label: "perlmod — Special Subroutines".to_string(),
                url: "https://perldoc.perl.org/perlmod#BEGIN%2C-UNITCHECK%2C-CHECK%2C-INIT-and-END"
                    .to_string(),
            },
        ])
    }

    #[test]
    fn phase_block_markdown_matches_legacy_shape() {
        let card = phase_begin_card();
        let md = card.render_markdown();
        assert!(md.starts_with("**Phase Block: `BEGIN`**"));
        assert!(md.contains("_Compile-time execution_"));
        assert!(md.contains("as soon as the block is fully parsed"));
        assert!(md.contains("[perlmod — Special Subroutines]("));
    }

    #[test]
    fn phase_block_plaintext_preserves_perl_literals_in_prose() {
        let card = phase_begin_card();
        let plain = card.render_plaintext();
        assert!(plain.contains("Phase Block: BEGIN"));
        assert!(plain.contains("Compile-time execution"));
        assert!(plain.contains("Multiple `BEGIN` blocks"));
        assert!(!plain.contains("replace('*'"));
    }

    #[test]
    fn method_modifier_plaintext_preserves_user_perl_punctuation() {
        let card = HoverCard::new(vec![
            HoverBlock::MethodModifierHeading { modifier_kind: "before".to_string() },
            HoverBlock::MethodModifierSummary {
                method_name: "foo_bar".to_string(),
                kind_label: "runs **before** the method — use for preconditions and logging",
            },
            HoverBlock::UserProse {
                text: "Checks *main::name*, `$hash{key}`, `@arr`, `%h` and `qx`".to_string(),
            },
        ]);
        let plain = card.render_plaintext();
        assert!(plain.contains("foo_bar"));
        assert!(plain.contains("*main::name*"));
        assert!(plain.contains("`$hash{key}`"));
        assert!(plain.contains("@arr"));
        assert!(plain.contains("%h"));
    }

    #[test]
    fn method_modifier_markdown_escapes_user_structure() {
        let card = HoverCard::new(vec![
            HoverBlock::MethodModifierHeading { modifier_kind: "before".to_string() },
            HoverBlock::MethodModifierSummary {
                method_name: "validate_input".to_string(),
                kind_label: "runs **before** the method — use for preconditions and logging",
            },
            HoverBlock::UserProse { text: "Checks that *all* args are [valid]".to_string() },
        ]);
        let md = card.render_markdown();
        assert!(md.contains(r"\*all\*"));
        assert!(md.contains(r"\[valid\]"));
        assert!(md.contains("**before**"));
        assert!(md.contains("`validate_input`"));
    }

    #[test]
    fn qualified_callable_renders_signature_and_package() {
        let mut blocks = vec![
            HoverBlock::BoldTitle { title: "Subroutine".to_string() },
            HoverBlock::CodeLine { code: "sub Foo::Bar".to_string() },
            HoverBlock::PackageDefinedLine { package: "Foo".to_string() },
        ];
        blocks.push(HoverBlock::UserProse { text: "Does [not] break".to_string() });
        let card = HoverCard::new(blocks);
        let md = card.render_markdown();
        assert!(md.contains("**Subroutine**"));
        assert!(md.contains("`sub Foo::Bar`"));
        assert!(md.contains("Defined in `Foo`"));
        assert!(md.contains(r"\[not\]"));
        let plain = card.render_plaintext();
        assert!(plain.contains("sub Foo::Bar"));
        assert!(plain.contains("Defined in Foo"));
        assert!(plain.contains("[not]"));
    }

    #[test]
    fn presentation_link_plaintext_is_label_only() {
        let card = HoverCard::new(vec![HoverBlock::PresentationLink {
            label: "perlmod — Special Subroutines".to_string(),
            url: "https://perldoc.perl.org/perlmod".to_string(),
        }]);
        assert_eq!(card.render_plaintext(), "perlmod — Special Subroutines");
    }

    #[test]
    fn empty_optional_section_suppressed_in_card_builders() {
        let card = HoverCard::new(vec![
            HoverBlock::BoldTitle { title: "Subroutine".to_string() },
            HoverBlock::CodeLine { code: "sub main".to_string() },
            HoverBlock::PackageDefinedLine { package: "main".to_string() },
        ]);
        let md = card.render_markdown();
        assert!(!md.ends_with("\n\n\n"));
        assert_eq!(md.matches("\n\n").count(), 2);
    }

    #[test]
    fn unmigrated_inventory_lists_unresolved_qualified_builder() {
        assert!(
            UNMIGRATED_DIRECT_MARKDOWN_HOVER_BUILDERS
                .iter()
                .any(|entry| entry.contains("unresolved_qualified_token_hover"))
        );
    }

    #[test]
    fn to_lsp_hover_uses_structured_renderer_not_lossy_strip() {
        let card = phase_begin_card();
        let value = card.to_lsp_hover(false);
        assert_eq!(
            value["contents"]["kind"].as_str(),
            Some("plaintext"),
            "plaintext must be rendered directly"
        );
        let plain = must_some(value["contents"]["value"].as_str());
        assert!(plain.contains('`'), "plaintext must not strip backticks from prose");
    }
}
