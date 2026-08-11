from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    if new in text:
        return
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one exact replacement, found {count}")
    file.write_text(text.replace(old, new, 1))


RUST = "xtask/src/bin/compiler-concepts.rs"
SCHEMA = "schemas/perl_compiler_concepts.v1.schema.json"

replace_once(
    RUST,
    '''        let mut ids = BTreeSet::new();
        for concept in &self.concepts {
            concept.validate()?;
            if !ids.insert(concept.concept_id.as_str()) {
                bail!("duplicate compiler concept id {:?}", concept.concept_id);
            }
        }

        Ok(())
''',
    '''        let mut ids = BTreeSet::new();
        for concept in &self.concepts {
            concept.validate()?;
            if !ids.insert(concept.concept_id.as_str()) {
                bail!("duplicate compiler concept id {:?}", concept.concept_id);
            }
        }

        if self.complete {
            let unresolved_ownership = self
                .concepts
                .iter()
                .filter(|concept| {
                    matches!(
                        concept.ownership,
                        OwnershipState::Active | OwnershipState::Deferred
                    )
                })
                .map(|concept| concept.concept_id.as_str())
                .collect::<Vec<_>>();
            if !unresolved_ownership.is_empty() {
                bail!(
                    "complete compiler concept inventory has unresolved ownership: {:?}",
                    unresolved_ownership
                );
            }

            let missing_proof = self
                .concepts
                .iter()
                .filter(|concept| {
                    matches!(concept.gold, ProofState::Missing)
                        || matches!(concept.oracle, ProofState::Missing)
                        || matches!(concept.composition, ProofState::Missing)
                })
                .map(|concept| concept.concept_id.as_str())
                .collect::<Vec<_>>();
            if !missing_proof.is_empty() {
                bail!(
                    "complete compiler concept inventory has missing proof: {:?}",
                    missing_proof
                );
            }
        }

        Ok(())
''',
)

replace_once(
    RUST,
    '''    #[test]
    fn inventory_complete_requires_named_authority() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        assert!(ledger.validate().is_err());
        ledger.inventory_authority = Some("issue:#6657/full-taxonomy-v1".to_string());
        ledger.validate()?;
        Ok(())
    }
''',
    '''    #[test]
    fn inventory_complete_requires_named_authority() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        assert!(ledger.validate().is_err());
        Ok(())
    }

    #[test]
    fn inventory_authority_does_not_hide_unresolved_ownership() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        ledger.inventory_authority = Some("issue:#6657/full-taxonomy-v1".to_string());
        for concept in &mut ledger.concepts {
            concept.provider_eligibility = ProviderEligibility::Ineligible;
            if matches!(concept.gold, ProofState::Missing) {
                concept.gold = ProofState::NotApplicable;
            }
            if matches!(concept.oracle, ProofState::Missing) {
                concept.oracle = ProofState::NotApplicable;
            }
            if matches!(concept.composition, ProofState::Missing) {
                concept.composition = ProofState::NotApplicable;
            }
        }
        assert!(ledger.validate().is_err());
        for concept in &mut ledger.concepts {
            concept.ownership = OwnershipState::Boundary;
        }
        ledger.validate()?;
        Ok(())
    }

    #[test]
    fn inventory_authority_does_not_hide_missing_proof() -> Result<()> {
        let mut ledger = ConceptLedger::from_str(COMMITTED_LEDGER)?;
        ledger.complete = true;
        ledger.inventory_authority = Some("issue:#6657/full-taxonomy-v1".to_string());
        for concept in &mut ledger.concepts {
            concept.provider_eligibility = ProviderEligibility::Ineligible;
            concept.ownership = OwnershipState::Boundary;
        }
        assert!(ledger.validate().is_err());
        for concept in &mut ledger.concepts {
            if matches!(concept.gold, ProofState::Missing) {
                concept.gold = ProofState::NotApplicable;
            }
            if matches!(concept.oracle, ProofState::Missing) {
                concept.oracle = ProofState::NotApplicable;
            }
            if matches!(concept.composition, ProofState::Missing) {
                concept.composition = ProofState::NotApplicable;
            }
        }
        ledger.validate()?;
        Ok(())
    }
''',
)

replace_once(
    SCHEMA,
    '''      "then": {
        "required": ["inventory_authority"]
      }
''',
    '''      "then": {
        "required": ["inventory_authority"],
        "properties": {
          "concepts": {
            "items": {
              "type": "object",
              "properties": {
                "gold": {
                  "not": {
                    "const": "missing"
                  }
                },
                "oracle": {
                  "not": {
                    "const": "missing"
                  }
                },
                "composition": {
                  "not": {
                    "const": "missing"
                  }
                },
                "ownership": {
                  "enum": [
                    "boundary",
                    "not_applicable"
                  ]
                }
              }
            }
          }
        }
      }
''',
)
