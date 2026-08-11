from pathlib import Path
import re


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    if new in text:
        return
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one exact replacement, found {count}")
    file.write_text(text.replace(old, new, 1))


def regex_once(path: str, pattern: str, replacement: str) -> None:
    file = Path(path)
    text = file.read_text()
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"{path}: expected one regex replacement, found {count}")
    file.write_text(updated)


RUST = "xtask/src/bin/compiler-concepts.rs"
CONTRACT = "contracts/compiler/perl_compiler_concepts.v1.toml"
SCHEMA = "schemas/perl_compiler_concepts.v1.schema.json"

replace_once(
    RUST,
    '''    complete: bool,
    concepts: Vec<ConceptRow>,
''',
    '''    complete: bool,
    #[serde(default)]
    inventory_authority: Option<String>,
    concepts: Vec<ConceptRow>,
''',
)

replace_once(
    RUST,
    '''    #[serde(default)]
    ast_kinds: Vec<String>,
''',
    '''    ast_kinds: Vec<String>,
''',
)

replace_once(
    RUST,
    '''        validate_issue("controller_issue", &self.controller_issue)?;

        if self.concepts.is_empty() {
''',
    '''        validate_issue("controller_issue", &self.controller_issue)?;

        match self.inventory_authority.as_deref() {
            Some(authority) if authority.trim().is_empty() => {
                bail!("compiler concept inventory authority must not be empty");
            }
            None if self.complete => {
                bail!("complete compiler concept inventory requires inventory_authority");
            }
            Some(_) | None => {}
        }

        if self.concepts.is_empty() {
''',
)

replace_once(
    RUST,
    '''        line(
            &mut output,
            &format!("- Complete: `{}`", normalized.complete),
        )?;
        line(
            &mut output,
            &format!("- Concepts in this seed: `{}`", normalized.concepts.len()),
        )?;
''',
    '''        line(
            &mut output,
            &format!("- Inventory complete: `{}`", normalized.complete),
        )?;
        if let Some(authority) = &normalized.inventory_authority {
            line(
                &mut output,
                &format!("- Inventory authority: `{authority}`"),
            )?;
        } else {
            line(&mut output, "- Inventory authority: —")?;
        }
        line(
            &mut output,
            &format!("- Concepts in this seed: `{}`", normalized.concepts.len()),
        )?;
''',
)

replace_once(
    RUST,
    '''        if matches!(self.ownership, OwnershipState::Active)
            && matches!(
                self.body_hir,
                RepresentationState::Opaque
                    | RepresentationState::Bridge
                    | RepresentationState::Boundary
                    | RepresentationState::Absent
            )
            && matches!(self.provider_eligibility, ProviderEligibility::Exact)
        {
            bail!(
                "compiler concept {} cannot be provider-exact while canonical body HIR is {}",
                self.concept_id,
                self.body_hir.stable_name()
            );
        }
''',
    '''        if matches!(self.provider_eligibility, ProviderEligibility::Exact)
            && (!matches!(self.body_hir, RepresentationState::Modeled)
                || !matches!(self.pir_a, RepresentationState::Modeled)
                || !matches!(self.eir_profile, EirState::Executable))
        {
            bail!(
                "compiler concept {} cannot be provider-exact without modeled body HIR, modeled PIR-A, and executable EIR",
                self.concept_id
            );
        }
''',
)

replace_once(
    RUST,
    '''        if matches!(self.eir_profile, EirState::Executable)
            && matches!(
                self.pir_a,
                RepresentationState::Absent
                    | RepresentationState::Opaque
                    | RepresentationState::Bridge
                    | RepresentationState::Boundary
            )
        {
            bail!(
                "compiler concept {} cannot be executable from unverified PIR-A state {}",
                self.concept_id,
                self.pir_a.stable_name()
            );
        }
''',
    '''        if matches!(self.eir_profile, EirState::Executable)
            && !matches!(self.pir_a, RepresentationState::Modeled)
        {
            bail!(
                "compiler concept {} cannot be executable from unverified PIR-A state {}",
                self.concept_id,
                self.pir_a.stable_name()
            );
        }
''',
)

replace_once(
    RUST,
    '''fn valid_concept_id(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-')
        })
}
''',
    '''fn valid_concept_id(value: &str) -> bool {
    value.split('.').all(valid_concept_segment)
}

fn valid_concept_segment(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(&first) = bytes.first() else {
        return false;
    };
    let last = bytes.last().copied().unwrap_or(first);
    let is_boundary = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    is_boundary(first)
        && is_boundary(last)
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'-')
        })
}
''',
)

replace_once(
    RUST,
    '''    #[test]
    fn rendering_is_independent_of_input_order() -> Result<()> {
''',
    '''    #[test]
    fn inventory_complete_requires_named_authority() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        assert!(ledger.validate().is_err());
        ledger.inventory_authority = Some("issue:#6657/full-taxonomy-v1".to_string());
        ledger.validate()?;
        Ok(())
    }

    #[test]
    fn exact_provider_requires_canonical_executable_pipeline_regardless_of_owner() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let first = ledger
            .concepts
            .first_mut()
            .ok_or_else(|| anyhow!("committed ledger unexpectedly empty"))?;
        first.provider_eligibility = ProviderEligibility::Exact;
        first.body_hir = RepresentationState::Opaque;
        first.pir_a = RepresentationState::Modeled;
        first.eir_profile = EirState::Executable;
        first.gold = ProofState::Proven;
        first.composition = ProofState::Proven;
        first.ownership = OwnershipState::Deferred;
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn executable_eir_requires_modeled_pir_a() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        let first = ledger
            .concepts
            .first_mut()
            .ok_or_else(|| anyhow!("committed ledger unexpectedly empty"))?;
        first.eir_profile = EirState::Executable;
        first.pir_a = RepresentationState::Skipped;
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn ast_kinds_is_required_by_typed_parser() {
        let source = COMMITTED_LEDGER.replacen("ast_kinds = [\\\"AmperCall\\\"]\\n", "", 1);
        assert!(toml::from_str::<ConceptLedger>(&source).is_err());
    }

    #[test]
    fn concept_id_grammar_matches_segment_contract() {
        for value in ["a", "calls.normalized_semantics", "x1.a-b_c"] {
            assert!(valid_concept_id(value));
        }
        for value in ["", ".calls", "calls.", "calls..ampersand", "-calls.x", "calls.x-"] {
            assert!(!valid_concept_id(value));
        }
    }

    #[test]
    fn rendering_is_independent_of_input_order() -> Result<()> {
''',
)

regex_once(
    CONTRACT,
    r'(concept_id = "source\.data_section".*?\nbody_hir = )"not_applicable"',
    r'\1"opaque"',
)

replace_once(
    SCHEMA,
    '''    "complete": {
      "type": "boolean"
    },
    "concepts": {
''',
    '''    "complete": {
      "type": "boolean"
    },
    "inventory_authority": {
      "type": "string",
      "minLength": 1,
      "pattern": "\\\\S"
    },
    "concepts": {
''',
)

replace_once(
    SCHEMA,
    '''  },
  "$defs": {
''',
    '''  },
  "allOf": [
    {
      "if": {
        "properties": {
          "complete": {
            "const": true
          }
        },
        "required": ["complete"]
      },
      "then": {
        "required": ["inventory_authority"]
      }
    }
  ],
  "$defs": {
''',
)

replace_once(
    SCHEMA,
    '''          "pattern": "^[a-z0-9][a-z0-9_.-]*[a-z0-9]$"
''',
    '''          "pattern": "^[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?(?:\\\\.[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?)*$"
''',
)
