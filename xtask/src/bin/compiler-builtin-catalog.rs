//! Validate and render the bounded Perl builtin semantic catalog from #6679.
//!
//! ```text
//! cargo run -p xtask --bin compiler-builtin-catalog -- --check
//! cargo run -p xtask --bin compiler-builtin-catalog -- --write-status
//! ```

#![allow(clippy::print_stdout)]

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const CATALOG_SCHEMA: &str = "perl_builtin_semantics.v1";
const DEFAULT_CATALOG: &str = "contracts/compiler/perl_builtin_semantics.v1.toml";
const DEFAULT_STATUS: &str = "docs/project/status/perl_builtin_semantics.md";

#[derive(Debug, Parser)]
#[command(name = "compiler-builtin-catalog")]
#[command(about = "Validate and render the Perl builtin semantic catalog")]
struct Cli {
    #[arg(long, default_value = DEFAULT_CATALOG)]
    catalog: PathBuf,

    #[arg(long, default_value = DEFAULT_STATUS)]
    status: PathBuf,

    #[arg(long)]
    check: bool,

    #[arg(long)]
    write_status: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BuiltinCatalog {
    schema_version: String,
    catalog_id: String,
    controller_issue: String,
    coverage_scope: String,
    claim_boundary: String,
    complete: bool,
    builtins: Vec<BuiltinEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BuiltinEntry {
    builtin_id: String,
    name: String,
    forms: Vec<String>,
    classification: Classification,
    recognition: Recognition,
    perl_version_min: String,
    arguments: Vec<ArgumentContract>,
    implicit_operands: Vec<ImplicitOperand>,
    result_behavior: ResultBehavior,
    side_effects: Vec<SideEffect>,
    boundaries: Vec<Boundary>,
    capabilities: Vec<Capability>,
    control_effect: ControlEffect,
    hir_pir_lowering: HirPirLowering,
    eir_profile: EirProfile,
    proof: ProofState,
    owner_issue: String,
    evidence: Vec<String>,
    claim_boundary: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ArgumentContract {
    role: String,
    context: ArgumentContext,
    access: ArgumentAccess,
    cardinality: Cardinality,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Classification {
    Builtin,
    ListOperator,
    NamedUnary,
    SpecialSyntax,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Recognition {
    ParserBuiltinOrCoreQualified,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ArgumentContext {
    Scalar,
    List,
    Callback,
    Place,
    Filehandle,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ArgumentAccess {
    Read,
    ReadModifyWrite,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Cardinality {
    ExactlyOne,
    ZeroOrOne,
    ZeroOrMore,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ImplicitOperand {
    CallerContext,
    SelectedOutputHandle,
    TopicAliasPerIteration,
    TopicWhenOmitted,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ResultBehavior {
    ContextTriState,
    ScalarBoolean,
    ScalarCount,
    ScalarCountOrList,
    ScalarValue,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum SideEffect {
    CallbackInvocation,
    ContainerMutation,
    InterpreterStateRead,
    StreamWrite,
    TopicLocalization,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Boundary {
    DynamicCallback,
    InterpreterState,
    Io,
    OutputSeparators,
    Overload,
    SelectedHandleState,
    TieMagic,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Capability {
    InterpreterState,
    Io,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ControlEffect {
    Ordinary,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum HirPirLowering {
    CatalogOnly,
    ClassifiedCall,
    DedicatedOperation,
    Boundary,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum EirProfile {
    Absent,
    Boundary,
    Executable,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum ProofState {
    Missing,
    Proven,
}

trait StableName {
    fn stable_name(self) -> &'static str;
}

macro_rules! stable_names {
    ($ty:ty, {$($variant:path => $name:literal),+ $(,)?}) => {
        impl StableName for $ty {
            fn stable_name(self) -> &'static str {
                match self {
                    $($variant => $name,)+
                }
            }
        }
    };
}

stable_names!(Classification, {
    Classification::Builtin => "builtin",
    Classification::ListOperator => "list_operator",
    Classification::NamedUnary => "named_unary",
    Classification::SpecialSyntax => "special_syntax",
});
stable_names!(Recognition, {
    Recognition::ParserBuiltinOrCoreQualified => "parser_builtin_or_core_qualified",
});
stable_names!(ArgumentContext, {
    ArgumentContext::Scalar => "scalar",
    ArgumentContext::List => "list",
    ArgumentContext::Callback => "callback",
    ArgumentContext::Place => "place",
    ArgumentContext::Filehandle => "filehandle",
});
stable_names!(ArgumentAccess, {
    ArgumentAccess::Read => "read",
    ArgumentAccess::ReadModifyWrite => "read_modify_write",
});
stable_names!(Cardinality, {
    Cardinality::ExactlyOne => "exactly_one",
    Cardinality::ZeroOrOne => "zero_or_one",
    Cardinality::ZeroOrMore => "zero_or_more",
});
stable_names!(ImplicitOperand, {
    ImplicitOperand::CallerContext => "caller_context",
    ImplicitOperand::SelectedOutputHandle => "selected_output_handle",
    ImplicitOperand::TopicAliasPerIteration => "topic_alias_per_iteration",
    ImplicitOperand::TopicWhenOmitted => "topic_when_omitted",
});
stable_names!(ResultBehavior, {
    ResultBehavior::ContextTriState => "context_tri_state",
    ResultBehavior::ScalarBoolean => "scalar_boolean",
    ResultBehavior::ScalarCount => "scalar_count",
    ResultBehavior::ScalarCountOrList => "scalar_count_or_list",
    ResultBehavior::ScalarValue => "scalar_value",
});
stable_names!(SideEffect, {
    SideEffect::CallbackInvocation => "callback_invocation",
    SideEffect::ContainerMutation => "container_mutation",
    SideEffect::InterpreterStateRead => "interpreter_state_read",
    SideEffect::StreamWrite => "stream_write",
    SideEffect::TopicLocalization => "topic_localization",
});
stable_names!(Boundary, {
    Boundary::DynamicCallback => "dynamic_callback",
    Boundary::InterpreterState => "interpreter_state",
    Boundary::Io => "io",
    Boundary::OutputSeparators => "output_separators",
    Boundary::Overload => "overload",
    Boundary::SelectedHandleState => "selected_handle_state",
    Boundary::TieMagic => "tie_magic",
});
stable_names!(Capability, {
    Capability::InterpreterState => "interpreter_state",
    Capability::Io => "io",
});
stable_names!(ControlEffect, {
    ControlEffect::Ordinary => "ordinary",
});
stable_names!(HirPirLowering, {
    HirPirLowering::CatalogOnly => "catalog_only",
    HirPirLowering::ClassifiedCall => "classified_call",
    HirPirLowering::DedicatedOperation => "dedicated_operation",
    HirPirLowering::Boundary => "boundary",
});
stable_names!(EirProfile, {
    EirProfile::Absent => "absent",
    EirProfile::Boundary => "boundary",
    EirProfile::Executable => "executable",
});
stable_names!(ProofState, {
    ProofState::Missing => "missing",
    ProofState::Proven => "proven",
});

impl BuiltinCatalog {
    fn from_str(source: &str) -> Result<Self> {
        let catalog: Self = toml::from_str(source).context("parse builtin semantic catalog")?;
        catalog.validate()?;
        Ok(catalog)
    }

    fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("read builtin semantic catalog {}", path.display()))?;
        Self::from_str(&source)
            .with_context(|| format!("validate builtin semantic catalog {}", path.display()))
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != CATALOG_SCHEMA {
            bail!(
                "unsupported builtin catalog schema {:?}; expected {:?}",
                self.schema_version,
                CATALOG_SCHEMA
            );
        }
        for (name, value) in [
            ("catalog_id", self.catalog_id.as_str()),
            ("controller_issue", self.controller_issue.as_str()),
            ("coverage_scope", self.coverage_scope.as_str()),
            ("claim_boundary", self.claim_boundary.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("builtin catalog field {name} must not be empty");
            }
        }
        validate_issue("controller_issue", &self.controller_issue)?;
        if self.builtins.is_empty() {
            bail!("builtin semantic catalog must contain at least one row");
        }

        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for builtin in &self.builtins {
            builtin.validate()?;
            if !ids.insert(builtin.builtin_id.as_str()) {
                bail!("duplicate builtin id {:?}", builtin.builtin_id);
            }
            if !names.insert(builtin.name.as_str()) {
                bail!("duplicate builtin name {:?}", builtin.name);
            }
        }

        if self.complete
            && self.builtins.iter().any(|builtin| {
                builtin.proof != ProofState::Proven
                    || builtin.hir_pir_lowering == HirPirLowering::CatalogOnly
                    || builtin.eir_profile != EirProfile::Executable
            })
        {
            bail!(
                "complete builtin catalog cannot contain missing proof, catalog-only lowering, or non-executable EIR rows"
            );
        }
        Ok(())
    }

    fn canonicalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.builtins.sort_by(|left, right| left.builtin_id.cmp(&right.builtin_id));
        for builtin in &mut normalized.builtins {
            builtin.forms.sort();
            builtin.implicit_operands.sort();
            builtin.side_effects.sort();
            builtin.boundaries.sort();
            builtin.capabilities.sort();
            builtin.evidence.sort();
        }
        normalized
    }

    fn render_markdown(&self) -> Result<String> {
        self.validate()?;
        let normalized = self.canonicalized();
        let mut output = String::new();
        let mut counts = BTreeMap::<(&'static str, &'static str), usize>::new();
        for builtin in &normalized.builtins {
            for (dimension, state) in [
                ("classification", builtin.classification.stable_name()),
                ("hir_pir_lowering", builtin.hir_pir_lowering.stable_name()),
                ("eir_profile", builtin.eir_profile.stable_name()),
                ("proof", builtin.proof.stable_name()),
            ] {
                *counts.entry((dimension, state)).or_default() += 1;
            }
        }

        line(&mut output, "# Perl Builtin Semantics")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "> Generated by `cargo run -p xtask --bin compiler-builtin-catalog -- --write-status`.",
        )?;
        line(
            &mut output,
            "> Check with `cargo run -p xtask --bin compiler-builtin-catalog -- --check`.",
        )?;
        line(&mut output, "")?;
        line(&mut output, &normalized.coverage_scope)?;
        line(&mut output, "")?;
        line(&mut output, &format!("- Schema: `{}`", normalized.schema_version))?;
        line(&mut output, &format!("- Catalog: `{}`", normalized.catalog_id))?;
        line(&mut output, &format!("- Controller: {}", normalized.controller_issue))?;
        line(&mut output, &format!("- Complete: `{}`", normalized.complete))?;
        line(&mut output, &format!("- Seed rows: `{}`", normalized.builtins.len()))?;
        line(&mut output, "")?;
        line(&mut output, &format!("**Claim boundary:** {}", normalized.claim_boundary))?;
        line(&mut output, "")?;

        line(&mut output, "## State counts")?;
        line(&mut output, "")?;
        line(&mut output, "| Dimension | State | Count |")?;
        line(&mut output, "| --- | --- | ---: |")?;
        for ((dimension, state), count) in counts {
            line(&mut output, &format!("| `{dimension}` | `{state}` | {count} |"))?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Catalog rows")?;
        line(&mut output, "")?;
        line(
            &mut output,
            "| Builtin | Class | Recognition | Arguments | Implicit operands | Result | Effects | Boundaries | Capabilities | Control | HIR/PIR | EIR | Proof | Owner |",
        )?;
        line(
            &mut output,
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        )?;
        for builtin in &normalized.builtins {
            let arguments = builtin
                .arguments
                .iter()
                .map(ArgumentContract::render)
                .collect::<Vec<_>>()
                .join("; ");
            line(
                &mut output,
                &format!(
                    "| `{}` / `{}` | `{}` | `{}` | {} | {} | `{}` | {} | {} | {} | `{}` | `{}` | `{}` | `{}` | {} |",
                    builtin.builtin_id,
                    builtin.name,
                    builtin.classification.stable_name(),
                    builtin.recognition.stable_name(),
                    empty_dash(&arguments),
                    enum_list(&builtin.implicit_operands),
                    builtin.result_behavior.stable_name(),
                    enum_list(&builtin.side_effects),
                    enum_list(&builtin.boundaries),
                    enum_list(&builtin.capabilities),
                    builtin.control_effect.stable_name(),
                    builtin.hir_pir_lowering.stable_name(),
                    builtin.eir_profile.stable_name(),
                    builtin.proof.stable_name(),
                    builtin.owner_issue
                ),
            )?;
        }
        line(&mut output, "")?;

        line(&mut output, "## Claim boundaries")?;
        line(&mut output, "")?;
        for builtin in &normalized.builtins {
            line(
                &mut output,
                &format!(
                    "- **`{}` ({}):** {}",
                    builtin.builtin_id, builtin.owner_issue, builtin.claim_boundary
                ),
            )?;
        }
        Ok(output)
    }
}

impl BuiltinEntry {
    fn validate(&self) -> Result<()> {
        validate_builtin_id(&self.builtin_id)?;
        validate_name(&self.name)?;
        let expected_id = format!("core.{}", self.name);
        if self.builtin_id != expected_id {
            bail!(
                "builtin id {:?} must match canonical name-derived id {:?}",
                self.builtin_id,
                expected_id
            );
        }
        validate_issue("owner_issue", &self.owner_issue)?;
        validate_version(&self.perl_version_min)?;
        validate_unique_nonempty("forms", &self.builtin_id, &self.forms)?;
        validate_unique_nonempty("evidence", &self.builtin_id, &self.evidence)?;
        if self.forms.is_empty() || self.evidence.is_empty() {
            bail!("builtin {} must name forms and evidence", self.builtin_id);
        }
        if self.claim_boundary.trim().is_empty() {
            bail!("builtin {} has an empty claim boundary", self.builtin_id);
        }

        let mut roles = BTreeSet::new();
        for (index, argument) in self.arguments.iter().enumerate() {
            argument.validate(&self.builtin_id)?;
            if !roles.insert(argument.role.as_str()) {
                bail!(
                    "builtin {} contains duplicate argument role {:?}",
                    self.builtin_id,
                    argument.role
                );
            }
            if argument.cardinality == Cardinality::ZeroOrMore && index + 1 != self.arguments.len()
            {
                bail!(
                    "builtin {} zero_or_more argument {:?} must be final",
                    self.builtin_id,
                    argument.role
                );
            }
        }
        validate_unique_enum("implicit_operands", &self.builtin_id, &self.implicit_operands)?;
        validate_unique_enum("side_effects", &self.builtin_id, &self.side_effects)?;
        validate_unique_enum("boundaries", &self.builtin_id, &self.boundaries)?;
        validate_unique_enum("capabilities", &self.builtin_id, &self.capabilities)?;

        if self.side_effects.contains(&SideEffect::CallbackInvocation) {
            if !self.arguments.iter().any(|argument| argument.context == ArgumentContext::Callback)
            {
                bail!(
                    "builtin {} callback_invocation requires a callback argument",
                    self.builtin_id
                );
            }
            if !self.boundaries.contains(&Boundary::DynamicCallback) {
                bail!(
                    "builtin {} callback_invocation requires dynamic_callback boundary",
                    self.builtin_id
                );
            }
        }
        if self.side_effects.contains(&SideEffect::TopicLocalization)
            && !self.implicit_operands.contains(&ImplicitOperand::TopicAliasPerIteration)
        {
            bail!(
                "builtin {} topic_localization requires topic_alias_per_iteration",
                self.builtin_id
            );
        }
        if self.side_effects.contains(&SideEffect::ContainerMutation) {
            if !self.arguments.iter().any(|argument| {
                argument.context == ArgumentContext::Place
                    && argument.access == ArgumentAccess::ReadModifyWrite
            }) {
                bail!(
                    "builtin {} container_mutation requires a read_modify_write place argument",
                    self.builtin_id
                );
            }
            if !self.boundaries.contains(&Boundary::TieMagic) {
                bail!("builtin {} container_mutation requires tie_magic boundary", self.builtin_id);
            }
        }
        if self.arguments.iter().any(|argument| argument.context == ArgumentContext::Filehandle)
            && (!self.capabilities.contains(&Capability::Io)
                || !self.boundaries.contains(&Boundary::Io))
        {
            bail!(
                "builtin {} filehandle argument requires io capability and boundary",
                self.builtin_id
            );
        }
        if self.side_effects.contains(&SideEffect::StreamWrite)
            && (!self.capabilities.contains(&Capability::Io)
                || !self.boundaries.contains(&Boundary::Io))
        {
            bail!("builtin {} stream_write requires io capability and boundary", self.builtin_id);
        }
        if self.implicit_operands.contains(&ImplicitOperand::SelectedOutputHandle)
            && (!self.capabilities.contains(&Capability::Io)
                || !self.boundaries.contains(&Boundary::SelectedHandleState))
        {
            bail!(
                "builtin {} selected output handle requires io capability and selected_handle_state boundary",
                self.builtin_id
            );
        }
        if self.implicit_operands.contains(&ImplicitOperand::CallerContext)
            && (!self.capabilities.contains(&Capability::InterpreterState)
                || !self.boundaries.contains(&Boundary::InterpreterState))
        {
            bail!(
                "builtin {} caller context requires interpreter_state capability and boundary",
                self.builtin_id
            );
        }
        if self.proof == ProofState::Proven && self.evidence.len() < 2 {
            bail!(
                "builtin {} proven status requires implementation proof beyond one language reference",
                self.builtin_id
            );
        }
        if (self.hir_pir_lowering != HirPirLowering::CatalogOnly
            || self.eir_profile == EirProfile::Executable)
            && self.proof != ProofState::Proven
        {
            bail!(
                "builtin {} cannot advance lowering/EIR state without proven evidence",
                self.builtin_id
            );
        }
        Ok(())
    }
}

impl ArgumentContract {
    fn validate(&self, builtin_id: &str) -> Result<()> {
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
}

fn main() -> Result<()> {
    color_eyre::install().map_err(|error| anyhow!("install diagnostics: {error}"))?;
    let cli = Cli::parse();
    if cli.check && cli.write_status {
        bail!("--check and --write-status are mutually exclusive");
    }

    let catalog = BuiltinCatalog::load(&cli.catalog)?;
    let rendered = catalog.render_markdown()?;
    if cli.write_status {
        write_status(&cli.status, &rendered)?;
        println!("wrote {}", cli.status.display());
        return Ok(());
    }
    if cli.check {
        let current = fs::read_to_string(&cli.status)
            .with_context(|| format!("read generated status {}", cli.status.display()))?;
        if current != rendered {
            bail!(
                "generated builtin status is stale: run `cargo run -p xtask --bin compiler-builtin-catalog -- --write-status`"
            );
        }
        println!("builtin semantic catalog valid: {} catalog-only rows", catalog.builtins.len());
        return Ok(());
    }

    print!("{rendered}");
    Ok(())
}

fn write_status(path: &Path, rendered: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create status directory {}", parent.display()))?;
    }
    fs::write(path, rendered).with_context(|| format!("write status {}", path.display()))
}

fn enum_list<T: StableName + Copy>(values: &[T]) -> String {
    if values.is_empty() {
        return "—".to_string();
    }
    values.iter().map(|value| format!("`{}`", value.stable_name())).collect::<Vec<_>>().join(", ")
}

fn empty_dash(value: &str) -> &str {
    if value.is_empty() { "—" } else { value }
}

fn line(output: &mut String, value: &str) -> Result<()> {
    writeln!(output, "{value}").map_err(|_| anyhow!("render builtin semantic status"))
}

fn validate_issue(name: &str, value: &str) -> Result<()> {
    let digits = value.strip_prefix('#').unwrap_or_default();
    if digits.is_empty()
        || digits.starts_with('0')
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("{name} must be a GitHub issue reference like #6679; got {value:?}");
    }
    Ok(())
}

fn validate_builtin_id(value: &str) -> Result<()> {
    if !value.starts_with("core.")
        || value.ends_with('.')
        || value.contains("..")
        || value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        bail!("invalid builtin id {value:?}");
    }
    Ok(())
}

fn validate_name(value: &str) -> Result<()> {
    if value.is_empty()
        || !value.as_bytes()[0].is_ascii_lowercase()
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'))
    {
        bail!("invalid builtin name {value:?}");
    }
    Ok(())
}

fn validate_role(value: &str) -> Result<()> {
    validate_name(value).with_context(|| format!("invalid builtin argument role {value:?}"))
}

fn validate_version(value: &str) -> Result<()> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        bail!("invalid Perl version {value:?}; expected major.minor.patch");
    }
    Ok(())
}

fn validate_unique_nonempty(name: &str, owner: &str, values: &[String]) -> Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        bail!("{owner} {name} contains an empty entry");
    }
    let unique = values.iter().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        bail!("{owner} {name} contains duplicate entries");
    }
    Ok(())
}

fn validate_unique_enum<T: Ord>(name: &str, owner: &str, values: &[T]) -> Result<()> {
    let unique = values.iter().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        bail!("{owner} {name} contains duplicate entries");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str =
        include_str!("../../../contracts/compiler/perl_builtin_semantics.v1.toml");
    const STATUS: &str = include_str!("../../../docs/project/status/perl_builtin_semantics.md");

    #[test]
    fn committed_catalog_validates_and_status_is_current() -> Result<()> {
        let catalog = BuiltinCatalog::from_str(CATALOG)?;
        assert!(!catalog.complete);
        assert_eq!(catalog.render_markdown()?, STATUS);
        Ok(())
    }

    #[test]
    fn duplicate_builtin_name_fails_closed() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let mut duplicate = catalog
            .builtins
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("committed catalog unexpectedly empty"))?;
        duplicate.builtin_id = "core.defined_copy".to_string();
        catalog.builtins.push(duplicate);
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn callback_without_boundary_fails_closed() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let builtin = catalog
            .builtins
            .iter_mut()
            .find(|builtin| builtin.name == "map")
            .ok_or_else(|| anyhow!("committed catalog has no map row"))?;
        builtin.boundaries.clear();
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn stream_write_without_io_capability_fails_closed() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let builtin = catalog
            .builtins
            .iter_mut()
            .find(|builtin| builtin.name == "print")
            .ok_or_else(|| anyhow!("committed catalog has no print row"))?;
        builtin.capabilities.clear();
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn place_mutation_without_place_context_fails_closed() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let builtin = catalog
            .builtins
            .iter_mut()
            .find(|builtin| builtin.name == "push")
            .ok_or_else(|| anyhow!("committed catalog has no push row"))?;
        let target = builtin
            .arguments
            .first_mut()
            .ok_or_else(|| anyhow!("committed push row has no target"))?;
        target.context = ArgumentContext::List;
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn proven_without_implementation_evidence_fails_closed() -> Result<()> {
        let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
        let builtin = catalog
            .builtins
            .first_mut()
            .ok_or_else(|| anyhow!("committed catalog unexpectedly empty"))?;
        builtin.proof = ProofState::Proven;
        assert!(catalog.validate().is_err());
        Ok(())
    }

    #[test]
    fn rendering_is_independent_of_row_order() -> Result<()> {
        let catalog = BuiltinCatalog::from_str(CATALOG)?;
        let expected = catalog.render_markdown()?;
        let mut reversed = catalog.clone();
        reversed.builtins.reverse();
        assert_eq!(reversed.render_markdown()?, expected);
        Ok(())
    }
}
