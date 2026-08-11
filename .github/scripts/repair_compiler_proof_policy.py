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


RUST = "xtask/src/bin/compiler-proof-policy.rs"
CONTRACT = "contracts/compiler/perl_compiler_proof_policy.v1.toml"
SCHEMA = "schemas/perl_compiler_proof_policy.v1.schema.json"

replace_once(
    RUST,
    '''    complete: bool,
    proof_classes: Vec<ProofClass>,
''',
    '''    complete: bool,
    #[serde(default)]
    closure_authority: Option<String>,
    proof_classes: Vec<ProofClass>,
''',
)

replace_once(
    RUST,
    '''        validate_issue("controller_issue", &self.controller_issue)?;
        if self.proof_classes.is_empty() || self.dimensions.is_empty() || self.campaigns.is_empty() {
''',
    '''        validate_issue("controller_issue", &self.controller_issue)?;
        match self.closure_authority.as_deref() {
            Some(authority) if authority.trim().is_empty() => {
                bail!("compiler proof policy closure authority must not be empty");
            }
            None if self.complete => {
                bail!("complete compiler proof policy requires closure_authority");
            }
            Some(_) | None => {}
        }
        if self.proof_classes.is_empty() || self.dimensions.is_empty() || self.campaigns.is_empty() {
''',
)

replace_once(
    RUST,
    '''        let mut dimension_ids = BTreeSet::new();
''',
    '''        let composition = self
            .proof_classes
            .iter()
            .find(|proof_class| proof_class.class_id == "composition_coverage")
            .ok_or_else(|| anyhow!("proof policy must define composition_coverage"))?;
        let expected_composition_stages = [
            ClaimStage::BodyHir,
            ClaimStage::PirA,
            ClaimStage::EffectsWorld,
            ClaimStage::Eir,
            ClaimStage::Provider,
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let actual_composition_stages = composition
            .claim_stages
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if composition.authority != Authority::CompositionHarness
            || composition.circular_output_allowed
            || composition.missing_effect != MissingEffect::BlocksClaim
            || actual_composition_stages != expected_composition_stages
        {
            bail!(
                "composition_coverage must retain composition_harness authority, non-circular output, blocks_claim semantics, and canonical claim stages"
            );
        }

        let mut dimension_ids = BTreeSet::new();
''',
)

replace_once(
    RUST,
    '''        for campaign in &self.campaigns {
            campaign.validate(&class_ids, &dimension_ids, &concept_families)?;
            if !campaign_ids.insert(campaign.campaign_id.as_str()) {
                bail!("duplicate proof campaign {:?}", campaign.campaign_id);
            }
        }
        Ok(())
''',
    '''        for campaign in &self.campaigns {
            campaign.validate(&class_ids, &dimension_ids, &concept_families)?;
            if !campaign_ids.insert(campaign.campaign_id.as_str()) {
                bail!("duplicate proof campaign {:?}", campaign.campaign_id);
            }
        }

        let referenced_classes = self
            .campaigns
            .iter()
            .flat_map(|campaign| campaign.proof_classes.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        let referenced_dimensions = self
            .campaigns
            .iter()
            .flat_map(|campaign| campaign.dimensions.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        let referenced_families = self
            .campaigns
            .iter()
            .flat_map(|campaign| campaign.concept_families.iter().map(String::as_str))
            .collect::<BTreeSet<_>>();
        if referenced_classes != class_ids {
            bail!(
                "proof policy has unexercised proof classes: {:?}",
                class_ids
                    .difference(&referenced_classes)
                    .copied()
                    .collect::<Vec<_>>()
            );
        }
        if referenced_dimensions != dimension_ids {
            bail!(
                "proof policy has unexercised dimensions: {:?}",
                dimension_ids
                    .difference(&referenced_dimensions)
                    .copied()
                    .collect::<Vec<_>>()
            );
        }
        if referenced_families != concept_families {
            bail!(
                "proof policy has unexercised concept families: {:?}",
                concept_families
                    .difference(&referenced_families)
                    .copied()
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
''',
)

replace_once(
    RUST,
    '''        line(&mut output, &format!("- Complete: `{}`", normalized.complete))?;
        line(&mut output, &format!("- Proof classes: `{}`", normalized.proof_classes.len()))?;
''',
    '''        line(
            &mut output,
            &format!("- Policy vocabulary closed: `{}`", normalized.complete),
        )?;
        if let Some(authority) = &normalized.closure_authority {
            line(&mut output, &format!("- Closure authority: `{authority}`"))?;
        } else {
            line(&mut output, "- Closure authority: —")?;
        }
        line(&mut output, &format!("- Proof classes: `{}`", normalized.proof_classes.len()))?;
''',
)

replace_once(
    RUST,
    '''        if self.concept_families.is_empty() || self.dimensions.len() < 2 || self.proof_classes.is_empty() {
            bail!(
                "campaign {} needs concept families, at least two dimensions, and proof classes",
''',
    '''        if self.concept_families.len() < 2 || self.dimensions.len() < 2 || self.proof_classes.is_empty() {
            bail!(
                "campaign {} needs at least two concept families, at least two dimensions, and proof classes",
''',
)

replace_once(
    RUST,
    '''    #[test]
    fn rendering_is_independent_of_input_order() -> Result<()> {
''',
    '''    #[test]
    fn complete_policy_requires_named_closure_authority() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        policy.complete = true;
        assert!(policy.validate(&concepts()?).is_err());
        policy.closure_authority = Some("issue:#6689/policy-closure-v1".to_string());
        policy.validate(&concepts()?)?;
        Ok(())
    }

    #[test]
    fn single_family_campaign_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let campaign = policy
            .campaigns
            .first_mut()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no campaign"))?;
        campaign.concept_families.truncate(1);
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn orphaned_policy_vocabulary_fails_closed() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let mut dimension = policy
            .dimensions
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("committed policy unexpectedly has no dimension"))?;
        dimension.dimension_id = "unexercised_axis".to_string();
        dimension.values = vec!["first".to_string(), "second".to_string()];
        policy.dimensions.push(dimension);
        assert!(policy.validate(&concepts()?).is_err());

        let policy = ProofPolicy::from_str(POLICY)?;
        let mut concept_index = concepts()?;
        concept_index.concepts.push(ConceptIndexRow {
            family: "unexercised_family".to_string(),
        });
        assert!(policy.validate(&concept_index).is_err());
        Ok(())
    }

    #[test]
    fn composition_coverage_contract_is_canonical() -> Result<()> {
        let mut policy = ProofPolicy::from_str(POLICY)?;
        let composition = policy
            .proof_classes
            .iter_mut()
            .find(|proof_class| proof_class.class_id == "composition_coverage")
            .ok_or_else(|| anyhow!("committed policy has no composition class"))?;
        composition.authority = Authority::CompilerSnapshot;
        composition.circular_output_allowed = true;
        composition.claim_stages = vec![ClaimStage::Provider];
        composition.missing_effect = MissingEffect::BlocksStage;
        assert!(policy.validate(&concepts()?).is_err());
        Ok(())
    }

    #[test]
    fn rendering_is_independent_of_input_order() -> Result<()> {
''',
)

replace_once(
    CONTRACT,
    '''dimensions = ["value_context", "evaluation_demand", "access_mode", "evaluation_order", "evaluate_once", "operator_family", "dynamic_boundary"]
''',
    '''dimensions = ["value_context", "evaluation_demand", "access_mode", "evaluation_order", "evaluate_once", "operator_family", "dynamic_boundary", "result_shape", "perl_version", "value_shape"]
''',
)

replace_once(
    CONTRACT,
    '''dimensions = ["implicit_topic", "match_state", "value_context", "evaluation_demand", "access_mode", "dynamic_boundary"]
''',
    '''dimensions = ["implicit_topic", "match_state", "value_context", "evaluation_demand", "access_mode", "dynamic_boundary", "hidden_effect"]
''',
)

with Path(CONTRACT).open("a") as contract:
    text = Path(CONTRACT).read_text()
    campaign_id = 'campaign_id = "dynamic_places_hidden_effects"'
    if campaign_id not in text:
        contract.write('''

[[campaigns]]
campaign_id = "dynamic_places_hidden_effects"
concept_families = ["dynamic", "places"]
dimensions = ["storage_class", "access_mode", "hidden_effect", "lifecycle", "dynamic_boundary", "result_shape"]
proof_classes = ["positive_gold", "negative_gold", "boundary_gold", "hir_snapshot", "pir_snapshot", "verifier_mutation", "eir_differential", "real_perl_oracle", "composition_coverage"]
owner_issue = "#6689"
claim_boundary = "Typed dynamic and place effects remain distinct from independently proved execution."
''')

replace_once(
    SCHEMA,
    '''    "complete": {
      "type": "boolean"
    },
    "proof_classes": {
''',
    '''    "complete": {
      "type": "boolean"
    },
    "closure_authority": {
      "type": "string",
      "minLength": 1,
      "pattern": "\\\\S"
    },
    "proof_classes": {
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
        "required": ["closure_authority"]
      }
    }
  ],
  "$defs": {
''',
)

replace_once(
    SCHEMA,
    '''        "concept_families": {
          "type": "array",
          "minItems": 1,
''',
    '''        "concept_families": {
          "type": "array",
          "minItems": 2,
''',
)
