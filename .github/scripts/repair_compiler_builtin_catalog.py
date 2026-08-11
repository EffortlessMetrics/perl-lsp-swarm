from pathlib import Path
import json
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


RUST = "xtask/src/bin/compiler-builtin-catalog.rs"
CATALOG = "contracts/compiler/perl_builtin_semantics.v1.toml"
SCHEMA = "schemas/perl_builtin_semantics.v1.schema.json"

replace_once(
    RUST,
    '''const DEFAULT_CATALOG: &str = "contracts/compiler/perl_builtin_semantics.v1.toml";
const DEFAULT_STATUS: &str = "docs/project/status/perl_builtin_semantics.md";
''',
    '''const DEFAULT_CATALOG: &str = "contracts/compiler/perl_builtin_semantics.v1.toml";
const DEFAULT_CONCEPT_LEDGER: &str = "contracts/compiler/perl_compiler_concepts.v1.toml";
const DEFAULT_STATUS: &str = "docs/project/status/perl_builtin_semantics.md";
''',
)

replace_once(
    RUST,
    '''    complete: bool,
    builtins: Vec<BuiltinEntry>,
''',
    '''    complete: bool,
    #[serde(default)]
    inventory_authority: Option<String>,
    builtins: Vec<BuiltinEntry>,
''',
)

replace_once(
    RUST,
    '''    recognition: Recognition,
    perl_version_min: String,
''',
    '''    recognition: Recognition,
    concept_ids: Vec<String>,
    perl_version_min: String,
    version_authority: VersionAuthority,
''',
)

replace_once(
    RUST,
    '''    owner_issue: String,
    evidence: Vec<String>,
    claim_boundary: String,
''',
    '''    owner_issue: String,
    evidence: Vec<String>,
    proof_evidence: Vec<String>,
    claim_boundary: String,
''',
)

replace_once(
    RUST,
    '''    access: ArgumentAccess,
    cardinality: Cardinality,
}
''',
    '''    access: ArgumentAccess,
    cardinality: Cardinality,
    #[serde(default)]
    callback_result_context: Option<ArgumentContext>,
    #[serde(default)]
    callback_result_cardinality: Option<Cardinality>,
}
''',
)

replace_once(
    RUST,
    '''enum Recognition {
    ParserBuiltinOrCoreQualified,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
''',
    '''enum Recognition {
    ParserBuiltinOrCoreQualified,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum VersionAuthority {
    MinimumOnly,
    Exact,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
''',
)

replace_once(
    RUST,
    '''enum ProofState {
    Missing,
    Proven,
}

trait StableName {
''',
    '''enum ProofState {
    Missing,
    Proven,
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptLedgerIndex {
    concepts: Vec<ConceptIndexRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConceptIndexRow {
    concept_id: String,
    body_hir: String,
    pir_a: String,
    eir_profile: String,
}

trait StableName {
''',
)

replace_once(
    RUST,
    '''stable_names!(Recognition, {
    Recognition::ParserBuiltinOrCoreQualified => "parser_builtin_or_core_qualified",
});
stable_names!(ArgumentContext, {
''',
    '''stable_names!(Recognition, {
    Recognition::ParserBuiltinOrCoreQualified => "parser_builtin_or_core_qualified",
});
stable_names!(VersionAuthority, {
    VersionAuthority::MinimumOnly => "minimum_only",
    VersionAuthority::Exact => "exact",
});
stable_names!(ArgumentContext, {
''',
)

replace_once(
    RUST,
    '''        validate_issue("controller_issue", &self.controller_issue)?;
        if self.builtins.is_empty() {
''',
    '''        validate_issue("controller_issue", &self.controller_issue)?;
        match self.inventory_authority.as_deref() {
            Some(authority) if authority.trim().is_empty() => {
                bail!("builtin catalog inventory authority must not be empty");
            }
            None if self.complete => {
                bail!("complete builtin catalog requires inventory_authority");
            }
            Some(_) | None => {}
        }
        if self.builtins.is_empty() {
''',
)

replace_once(
    RUST,
    '''        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for builtin in &self.builtins {
            builtin.validate()?;
''',
    '''        let concept_states = load_concept_states()?;
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for builtin in &self.builtins {
            builtin.validate(&concept_states)?;
''',
)

replace_once(
    RUST,
    '''                builtin.proof != ProofState::Proven
                    || builtin.hir_pir_lowering == HirPirLowering::CatalogOnly
                    || builtin.eir_profile != EirProfile::Executable
''',
    '''                builtin.proof != ProofState::Proven
                    || builtin.hir_pir_lowering == HirPirLowering::CatalogOnly
                    || builtin.eir_profile != EirProfile::Executable
                    || builtin.version_authority != VersionAuthority::Exact
''',
)

replace_once(
    RUST,
    '''            builtin.forms.sort();
            builtin.implicit_operands.sort();
''',
    '''            builtin.forms.sort();
            builtin.concept_ids.sort();
            builtin.implicit_operands.sort();
''',
)
replace_once(
    RUST,
    '''            builtin.evidence.sort();
''',
    '''            builtin.evidence.sort();
            builtin.proof_evidence.sort();
''',
)

replace_once(
    RUST,
    '''        line(&mut output, &format!("- Complete: `{}`", normalized.complete))?;
        line(
''',
    '''        line(&mut output, &format!("- Complete: `{}`", normalized.complete))?;
        if let Some(authority) = &normalized.inventory_authority {
            line(&mut output, &format!("- Inventory authority: `{authority}`"))?;
        } else {
            line(&mut output, "- Inventory authority: —")?;
        }
        line(
''',
)

replace_once(
    RUST,
    '''            "| Builtin | Class | Recognition | Arguments | Implicit operands | Result | Effects | Boundaries | Capabilities | Control | HIR/PIR | EIR | Proof | Owner |",
''',
    '''            "| Builtin | Class | Recognition | Version authority | Concepts | Arguments | Implicit operands | Result | Effects | Boundaries | Capabilities | Control | HIR/PIR | EIR | Proof | Proof evidence | Owner |",
''',
)
replace_once(
    RUST,
    '''            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
''',
    '''            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
''',
)
replace_once(
    RUST,
    '''                    "| `{}` / `{}` | `{}` | `{}` | {} | {} | `{}` | {} | {} | {} | `{}` | `{}` | `{}` | `{}` | {} |",
''',
    '''                    "| `{}` / `{}` | `{}` | `{}` | `{}` | {} | {} | {} | `{}` | {} | {} | {} | `{}` | `{}` | `{}` | {} | {} |",
''',
)
replace_once(
    RUST,
    '''                    builtin.recognition.stable_name(),
                    empty_dash(&arguments),
''',
    '''                    builtin.recognition.stable_name(),
                    builtin.version_authority.stable_name(),
                    code_list(&builtin.concept_ids),
                    empty_dash(&arguments),
''',
)
replace_once(
    RUST,
    '''                    builtin.proof.stable_name(),
                    builtin.owner_issue
''',
    '''                    builtin.proof.stable_name(),
                    code_list(&builtin.proof_evidence),
                    builtin.owner_issue
''',
)

replace_once(
    RUST,
    '''impl BuiltinEntry {
    fn validate(&self) -> Result<()> {
''',
    '''impl BuiltinEntry {
    fn validate(&self, concepts: &BTreeMap<String, ConceptIndexRow>) -> Result<()> {
''',
)

replace_once(
    RUST,
    '''        validate_issue("owner_issue", &self.owner_issue)?;
        validate_version(&self.perl_version_min)?;
        validate_unique_nonempty("forms", &self.builtin_id, &self.forms)?;
        validate_unique_nonempty("evidence", &self.builtin_id, &self.evidence)?;
        if self.forms.is_empty() || self.evidence.is_empty() {
''',
    '''        validate_issue("owner_issue", &self.owner_issue)?;
        validate_version(&self.perl_version_min)?;
        validate_unique_nonempty("concept_ids", &self.builtin_id, &self.concept_ids)?;
        if self.concept_ids.is_empty() {
            bail!("builtin {} must link at least one compiler concept", self.builtin_id);
        }
        for concept_id in &self.concept_ids {
            if !concepts.contains_key(concept_id) {
                bail!(
                    "builtin {} references unknown compiler concept {:?}",
                    self.builtin_id,
                    concept_id
                );
            }
        }
        validate_unique_nonempty("forms", &self.builtin_id, &self.forms)?;
        validate_unique_nonempty("evidence", &self.builtin_id, &self.evidence)?;
        validate_unique_nonempty("proof_evidence", &self.builtin_id, &self.proof_evidence)?;
        if self.forms.is_empty() || self.evidence.is_empty() {
''',
)

replace_once(
    RUST,
    '''        for (index, argument) in self.arguments.iter().enumerate() {
            argument.validate(&self.builtin_id)?;
''',
    '''        for (index, argument) in self.arguments.iter().enumerate() {
            argument.validate(&self.builtin_id)?;
''',
)

replace_once(
    RUST,
    '''        if self.side_effects.contains(&SideEffect::CallbackInvocation) {
''',
    '''        let has_callback_argument = self
            .arguments
            .iter()
            .any(|argument| argument.context == ArgumentContext::Callback);
        if self.side_effects.contains(&SideEffect::CallbackInvocation) {
''',
)
replace_once(
    RUST,
    '''            if !self
                .arguments
                .iter()
                .any(|argument| argument.context == ArgumentContext::Callback)
            {
''',
    '''            if !has_callback_argument {
''',
)
replace_once(
    RUST,
    '''        if self.side_effects.contains(&SideEffect::TopicLocalization)
''',
    '''        if has_callback_argument
            && (!self.side_effects.contains(&SideEffect::CallbackInvocation)
                || !self.boundaries.contains(&Boundary::DynamicCallback))
        {
            bail!(
                "builtin {} callback argument requires callback_invocation and dynamic_callback",
                self.builtin_id
            );
        }
        if self.boundaries.contains(&Boundary::DynamicCallback)
            && (!has_callback_argument
                || !self.side_effects.contains(&SideEffect::CallbackInvocation))
        {
            bail!(
                "builtin {} dynamic_callback boundary requires callback argument and callback_invocation",
                self.builtin_id
            );
        }
        if self.side_effects.contains(&SideEffect::TopicLocalization)
''',
)
replace_once(
    RUST,
    '''        if self.side_effects.contains(&SideEffect::ContainerMutation) {
''',
    '''        if self
            .implicit_operands
            .contains(&ImplicitOperand::TopicAliasPerIteration)
            && !self.side_effects.contains(&SideEffect::TopicLocalization)
        {
            bail!(
                "builtin {} topic_alias_per_iteration requires topic_localization",
                self.builtin_id
            );
        }
        if self.side_effects.contains(&SideEffect::ContainerMutation) {
''',
)

replace_once(
    RUST,
    '''        if self.side_effects.contains(&SideEffect::StreamWrite) {
''',
    '''        if self.capabilities.contains(&Capability::Io)
            != self.boundaries.contains(&Boundary::Io)
        {
            bail!(
                "builtin {} io capability and io boundary must be declared together",
                self.builtin_id
            );
        }
        if self.side_effects.contains(&SideEffect::StreamWrite) {
''',
)
replace_once(
    RUST,
    '''        if self
            .implicit_operands
            .contains(&ImplicitOperand::SelectedOutputHandle)
''',
    '''        if self.boundaries.contains(&Boundary::OutputSeparators)
            && !self.side_effects.contains(&SideEffect::InterpreterStateRead)
        {
            bail!(
                "builtin {} output_separators boundary requires interpreter_state_read",
                self.builtin_id
            );
        }
        if self
            .implicit_operands
            .contains(&ImplicitOperand::SelectedOutputHandle)
''',
)
replace_once(
    RUST,
    '''        if self.proof == ProofState::Proven && self.evidence.len() < 2 {
            bail!(
                "builtin {} proven status requires implementation proof beyond one language reference",
                self.builtin_id
            );
        }
''',
    '''        if self.proof == ProofState::Proven {
            if self.proof_evidence.is_empty() {
                bail!(
                    "builtin {} proven status requires typed implementation proof",
                    self.builtin_id
                );
            }
            for evidence in &self.proof_evidence {
                validate_proof_evidence(&self.builtin_id, evidence)?;
            }
        } else if !self.proof_evidence.is_empty() {
            bail!(
                "builtin {} carries proof evidence while proof is missing",
                self.builtin_id
            );
        }
''',
)
replace_once(
    RUST,
    '''        if (self.hir_pir_lowering != HirPirLowering::CatalogOnly
            || self.eir_profile == EirProfile::Executable)
            && self.proof != ProofState::Proven
''',
    '''        if (self.hir_pir_lowering != HirPirLowering::CatalogOnly
            || self.eir_profile == EirProfile::Executable)
            && self.proof != ProofState::Proven
''',
)
replace_once(
    RUST,
    '''        {
            bail!(
                "builtin {} cannot advance lowering/EIR state without proven evidence",
                self.builtin_id
            );
        }
        Ok(())
''',
    '''        {
            bail!(
                "builtin {} cannot advance lowering/EIR state without proven evidence",
                self.builtin_id
            );
        }
        if (self.hir_pir_lowering != HirPirLowering::CatalogOnly
            || self.eir_profile == EirProfile::Executable
            || self.proof == ProofState::Proven)
            && self.version_authority != VersionAuthority::Exact
        {
            bail!(
                "builtin {} cannot advance beyond catalog-only without exact Perl version authority",
                self.builtin_id
            );
        }
        if self.hir_pir_lowering != HirPirLowering::CatalogOnly
            && !self.concept_ids.iter().any(|concept_id| {
                concepts.get(concept_id).is_some_and(|concept| {
                    concept.body_hir == "modeled" && concept.pir_a == "modeled"
                })
            })
        {
            bail!(
                "builtin {} cannot advance HIR/PIR without a linked concept modeled in body HIR and PIR-A",
                self.builtin_id
            );
        }
        if self.eir_profile == EirProfile::Executable {
            if !matches!(
                self.hir_pir_lowering,
                HirPirLowering::ClassifiedCall | HirPirLowering::DedicatedOperation
            ) {
                bail!(
                    "builtin {} executable EIR requires implemented HIR/PIR lowering",
                    self.builtin_id
                );
            }
            if !self.concept_ids.iter().any(|concept_id| {
                concepts
                    .get(concept_id)
                    .is_some_and(|concept| concept.eir_profile == "executable")
            }) {
                bail!(
                    "builtin {} executable EIR requires a linked executable concept",
                    self.builtin_id
                );
            }
        }
        Ok(())
''',
)

replace_once(
    RUST,
    '''    fn validate(&self, builtin_id: &str) -> Result<()> {
        validate_role(&self.role)
            .with_context(|| format!("validate argument role for {builtin_id}"))
    }

    fn render(&self) -> String {
        format!(
            "`{}`:`{}`/`{}`/`{}`",
            self.role,
            self.context.stable_name(),
            self.access.stable_name(),
            self.cardinality.stable_name()
        )
    }
''',
    '''    fn validate(&self, builtin_id: &str) -> Result<()> {
        validate_role(&self.role)
            .with_context(|| format!("validate argument role for {builtin_id}"))?;
        if self.context == ArgumentContext::Callback {
            if self.callback_result_context.is_none()
                || self.callback_result_cardinality.is_none()
            {
                bail!(
                    "builtin {builtin_id} callback argument {} must declare callback result context and cardinality",
                    self.role
                );
            }
        } else if self.callback_result_context.is_some()
            || self.callback_result_cardinality.is_some()
        {
            bail!(
                "builtin {builtin_id} non-callback argument {} cannot declare callback result semantics",
                self.role
            );
        }
        Ok(())
    }

    fn render(&self) -> String {
        let callback_result = match (
            self.callback_result_context,
            self.callback_result_cardinality,
        ) {
            (Some(context), Some(cardinality)) => format!(
                " -> `{}`/`{}`",
                context.stable_name(),
                cardinality.stable_name()
            ),
            _ => String::new(),
        };
        format!(
            "`{}`:`{}`/`{}`/`{}`{}",
            self.role,
            self.context.stable_name(),
            self.access.stable_name(),
            self.cardinality.stable_name(),
            callback_result
        )
    }
''',
)

replace_once(
    RUST,
    '''fn write_status(path: &Path, rendered: &str) -> Result<()> {
''',
    '''fn load_concept_states() -> Result<BTreeMap<String, ConceptIndexRow>> {
    let source = fs::read_to_string(DEFAULT_CONCEPT_LEDGER)
        .with_context(|| format!("read compiler concept ledger {DEFAULT_CONCEPT_LEDGER}"))?;
    let ledger: ConceptLedgerIndex = toml::from_str(&source)
        .with_context(|| format!("parse compiler concept ledger {DEFAULT_CONCEPT_LEDGER}"))?;
    let mut concepts = BTreeMap::new();
    for concept in ledger.concepts {
        if concepts.insert(concept.concept_id.clone(), concept).is_some() {
            bail!("compiler concept ledger contains duplicate concept ids");
        }
    }
    Ok(concepts)
}

fn write_status(path: &Path, rendered: &str) -> Result<()> {
''',
)

replace_once(
    RUST,
    '''fn enum_list<T: StableName + Copy>(values: &[T]) -> String {
''',
    '''fn code_list(values: &[String]) -> String {
    if values.is_empty() {
        return "—".to_string();
    }
    values
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn enum_list<T: StableName + Copy>(values: &[T]) -> String {
''',
)

replace_once(
    RUST,
    '''fn validate_unique_nonempty(name: &str, owner: &str, values: &[String]) -> Result<()> {
''',
    '''fn validate_proof_evidence(owner: &str, value: &str) -> Result<()> {
    let (kind, path) = value
        .split_once(':')
        .ok_or_else(|| anyhow!("{owner} proof evidence {value:?} must be kind:path"))?;
    if !matches!(kind, "fixture" | "receipt") {
        bail!("{owner} proof evidence {value:?} has unsupported kind {kind:?}");
    }
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(component, std::path::Component::ParentDir)
        })
        || !path.is_file()
    {
        bail!("{owner} proof evidence {value:?} must name an existing repository-relative file");
    }
    Ok(())
}

fn validate_unique_nonempty(name: &str, owner: &str, values: &[String]) -> Result<()> {
''',
)

replace_once(
    RUST,
    '''    #[test]
    fn rendering_is_independent_of_row_order() -> Result<()> {
''',
    '''    #[test]
    fn callback_result_contract_is_required() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let builtin = catalog
            .builtins
            .iter_mut()
            .find(|builtin| builtin.name == "map")
            .ok_or_else(|| anyhow!("committed catalog has no map row"))?;
        builtin.arguments[0].callback_result_cardinality = None;
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn executable_eir_requires_implemented_lowering() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let builtin = catalog
            .builtins
            .first_mut()
            .ok_or_else(|| anyhow!("committed catalog unexpectedly empty"))?;
        builtin.eir_profile = EirProfile::Executable;
        builtin.proof = ProofState::Proven;
        builtin.version_authority = VersionAuthority::Exact;
        builtin.proof_evidence = vec![
            "receipt:contracts/compiler/perl_builtin_semantics.v1.toml".to_string(),
        ];
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn complete_catalog_requires_inventory_authority() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        catalog.complete = true;
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn reverse_callback_implication_fails_closed() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let builtin = catalog
            .builtins
            .iter_mut()
            .find(|builtin| builtin.name == "grep")
            .ok_or_else(|| anyhow!("committed catalog has no grep row"))?;
        builtin.side_effects.clear();
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn rendering_is_independent_of_row_order() -> Result<()> {
''',
)

# Enrich the seven seed rows without changing their bounded implementation claim.
text = Path(CATALOG).read_text()
concepts = {
    "defined": ["calls.normalized_semantics", "context.value_context"],
    "grep": ["calls.normalized_semantics", "context.value_context"],
    "map": ["calls.normalized_semantics", "context.value_context"],
    "print": ["calls.normalized_semantics", "context.value_context"],
    "push": ["calls.normalized_semantics", "context.value_context", "places.array_slice"],
    "scalar": ["calls.normalized_semantics", "context.value_context"],
    "wantarray": ["calls.normalized_semantics", "context.value_context"],
}
for name, concept_ids in concepts.items():
    pattern = re.compile(
        rf'(name = "{name}".*?\nrecognition = "parser_builtin_or_core_qualified"\n)',
        re.S,
    )
    replacement = (
        rf'\1concept_ids = {json.dumps(concept_ids)}\n'
        'version_authority = "minimum_only"\n'
    )
    text, count = pattern.subn(replacement, text, count=1)
    if count != 1:
        raise SystemExit(f"missing builtin row {name}")

text = text.replace(
    '{ role = "predicate", context = "callback", access = "read", cardinality = "exactly_one" }',
    '{ role = "predicate", context = "callback", access = "read", cardinality = "exactly_one", callback_result_context = "scalar", callback_result_cardinality = "exactly_one" }',
)
text = text.replace(
    '{ role = "mapper", context = "callback", access = "read", cardinality = "exactly_one" }',
    '{ role = "mapper", context = "callback", access = "read", cardinality = "exactly_one", callback_result_context = "list", callback_result_cardinality = "zero_or_more" }',
)
text = re.sub(r'(evidence = \[[^\n]+\]\n)', r'\1proof_evidence = []\n', text)
Path(CATALOG).write_text(text)

schema = json.loads(Path(SCHEMA).read_text())
schema["properties"]["inventory_authority"] = {
    "type": "string",
    "minLength": 1,
    "pattern": r"\\S",
}
schema.setdefault("allOf", []).append(
    {
        "if": {"properties": {"complete": {"const": True}}, "required": ["complete"]},
        "then": {"required": ["inventory_authority"]},
    }
)
schema["$defs"]["version_authority"] = {"enum": ["minimum_only", "exact"]}
argument = schema["$defs"]["argument"]
argument["properties"]["callback_result_context"] = {"$ref": "#/$defs/argument_context"}
argument["properties"]["callback_result_cardinality"] = {"$ref": "#/$defs/cardinality"}
builtin = schema["$defs"]["builtin"]
for required in ["concept_ids", "version_authority", "proof_evidence"]:
    if required not in builtin["required"]:
        builtin["required"].append(required)
builtin["properties"]["concept_ids"] = {
    "type": "array",
    "minItems": 1,
    "uniqueItems": True,
    "items": {"$ref": "#/$defs/identifier"},
}
builtin["properties"]["version_authority"] = {"$ref": "#/$defs/version_authority"}
builtin["properties"]["proof_evidence"] = {
    "type": "array",
    "uniqueItems": True,
    "items": {
        "type": "string",
        "pattern": r"^(fixture|receipt):[^/\\].+$",
    },
}
Path(SCHEMA).write_text(json.dumps(schema, indent=2) + "\n")
