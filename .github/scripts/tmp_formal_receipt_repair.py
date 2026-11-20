from pathlib import Path

path = Path("xtask/src/tasks/formal_receipt.rs")
source = path.read_text(encoding="utf-8")


def replace_once(old: str, new: str, label: str) -> None:
    global source
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{label} matched {count} times; expected exactly one")
    source = source.replace(old, new, 1)


replace_once(
    '''    fn validate_native(&self, errors: &mut Vec<String>) {
        match self.native.result {
''',
    '''    fn validate_native(&self, errors: &mut Vec<String>) {
        let declared_properties = self
            .obligations
            .positive
            .ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let observed_properties = self
            .native
            .properties
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let observed_count =
            u64::try_from(self.native.properties.len()).unwrap_or(u64::MAX);
        let satisfied_count = u64::try_from(
            self.native
                .properties
                .values()
                .filter(|outcome| **outcome == PropertyOutcome::Pass)
                .count(),
        )
        .unwrap_or(u64::MAX);

        if !observed_properties.is_subset(&declared_properties) {
            errors.push(
                "native property identities must be declared by obligations.positive.ids"
                    .to_string(),
            );
        }
        if observed_count != u64::from(self.obligations.positive.observed) {
            errors.push(
                "obligations.positive.observed must equal the native property row count"
                    .to_string(),
            );
        }
        if satisfied_count != u64::from(self.obligations.positive.satisfied) {
            errors.push(
                "obligations.positive.satisfied must equal the native pass row count"
                    .to_string(),
            );
        }

        match self.native.result {
''',
    "native observation accounting",
)

replace_once(
    '''                complete("positive", &self.obligations.positive, errors);
                complete("harness controls", &self.obligations.harness_controls, errors);
                if !self.artifacts.iter().any(|artifact| artifact.required) {
                    errors.push("native pass requires a required raw artifact".to_string());
                }
''',
    '''                if observed_properties != declared_properties {
                    errors.push(
                        "native pass property identities must exactly match obligations.positive.ids"
                            .to_string(),
                    );
                }
                complete("positive", &self.obligations.positive, errors);
                complete("harness controls", &self.obligations.harness_controls, errors);
                if !self.artifacts.iter().any(|artifact| {
                    artifact.required
                        && matches!(
                            artifact.kind,
                            ArtifactKind::Stdout
                                | ArtifactKind::Stderr
                                | ArtifactKind::StructuredReport
                        )
                }) {
                    errors.push(
                        "native pass requires a required stdout, stderr, or structured_report artifact"
                            .to_string(),
                    );
                }
                if self.disposition.status == DispositionStatus::Active
                    && self.freshness.status != FreshnessStatus::Current
                {
                    errors.push("an active native pass requires current freshness".to_string());
                }
                if self.claim.binding != ImplementationBinding::ModelOnly
                    && self.correspondence.status != CorrespondenceStatus::Established
                {
                    errors.push(
                        "a production-bound native pass requires established correspondence"
                            .to_string(),
                    );
                }
''',
    "native pass admission",
)

replace_once(
    '''                if !self.native.process_settled
                    && self.native.reason != Some(NonPassReason::ProcessUnsettled)
                {
                    errors.push(
                        "unsettled process requires reason process_unsettled".to_string(),
                    );
                }
''',
    '''                if !self.native.process_settled
                    && self.native.reason != Some(NonPassReason::ProcessUnsettled)
                {
                    errors.push(
                        "unsettled process requires reason process_unsettled".to_string(),
                    );
                }
                if self.native.process_settled
                    && self.native.reason == Some(NonPassReason::ProcessUnsettled)
                {
                    errors.push(
                        "settled process cannot use reason process_unsettled".to_string(),
                    );
                }
''',
    "process settlement consistency",
)

replace_once(
    '''                if self.freshness.reason.is_some()
                    || self.freshness.evaluated_source_commit != self.source.commit
                    || self.freshness.evaluated_closure_digest != self.closure.digest
                {
                    errors.push(
                        "current freshness must have no reason and bind source plus closure"
                            .to_string(),
                    );
                }
''',
    '''                if self.freshness.reason.is_some()
                    || self.freshness.evaluated_source_commit != self.source.commit
                    || self.freshness.evaluated_closure_digest != self.closure.digest
                    || self.source.state == SourceState::Unknown
                {
                    errors.push(
                        "current freshness must have no reason, bind source plus closure, and use a known source state"
                            .to_string(),
                    );
                }
''',
    "current freshness consistency",
)

replace_once(
    '''        let ids = self
            .artifacts
            .iter()
            .map(|artifact| artifact.id.as_str())
            .collect::<BTreeSet<_>>();
        for artifact_id in &self.correspondence.artifact_ids {
            if !ids.contains(artifact_id.as_str()) {
                errors.push(format!(
                    "correspondence artifact '{artifact_id}' is not present in artifacts"
                ));
            }
        }
''',
    '''        let artifacts = self
            .artifacts
            .iter()
            .map(|artifact| (artifact.id.as_str(), artifact))
            .collect::<BTreeMap<_, _>>();
        for artifact_id in &self.correspondence.artifact_ids {
            match artifacts.get(artifact_id.as_str()) {
                Some(artifact) if artifact.kind == ArtifactKind::Correspondence => {}
                Some(_) => errors.push(format!(
                    "correspondence artifact '{artifact_id}' must reference a correspondence artifact"
                )),
                None => errors.push(format!(
                    "correspondence artifact '{artifact_id}' is not present in artifacts"
                )),
            }
        }
        if self.correspondence.status == CorrespondenceStatus::Established
            && !self.correspondence.artifact_ids.iter().any(|artifact_id| {
                artifacts.get(artifact_id.as_str()).is_some_and(|artifact| {
                    artifact.required && artifact.kind == ArtifactKind::Correspondence
                })
            })
        {
            errors.push(
                "established correspondence requires a required correspondence artifact"
                    .to_string(),
            );
        }
''',
    "correspondence artifact binding",
)

replace_once(
    '''                if self.disposition.reason.is_some()
                    || self.disposition.superseded_by.is_some()
                    || matches!(
''',
    '''                if self.disposition.reason.is_some()
                    || self.disposition.superseded_by.is_some()
                    || self.claim.rollout == RolloutState::Retired
                    || matches!(
''',
    "active disposition consistency",
)

tests = r'''
    #[test]
    fn pass_requires_every_declared_positive_property() {
        let mut value = valid_receipt();
        value["native"]["properties"] = json!({
            "roundtrip": "pass"
        });
        value["obligations"]["positive"]["observed"] = json!(1);
        value["obligations"]["positive"]["satisfied"] = json!(1);

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("exactly match obligations.positive.ids"))
        );
    }

    #[test]
    fn pass_rejects_replaced_positive_property_identity() {
        let mut value = valid_receipt();
        value["native"]["properties"] = json!({
            "roundtrip": "pass",
            "unrelated": "pass"
        });

        let errors = validate_value(&value);
        assert!(errors.iter().any(|error| error.contains("must be declared")));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("exactly match obligations.positive.ids"))
        );
    }

    #[test]
    fn pass_rejects_extra_positive_property_identity() {
        let mut value = valid_receipt();
        value["native"]["properties"]["auxiliary"] = json!("pass");
        value["obligations"]["positive"]["observed"] = json!(3);
        value["obligations"]["positive"]["satisfied"] = json!(3);

        let errors = validate_value(&value);
        assert!(errors.iter().any(|error| error.contains("must be declared")));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("exactly match obligations.positive.ids"))
        );
    }

    #[test]
    fn native_rows_must_match_positive_observation_counts() {
        let mut value = valid_receipt();
        value["obligations"]["positive"]["observed"] = json!(1);

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("native property row count"))
        );
    }

    #[test]
    fn native_pass_rows_must_match_positive_satisfaction_counts() {
        let mut value = valid_receipt();
        value["obligations"]["positive"]["satisfied"] = json!(1);

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("native pass row count"))
        );
    }

    #[test]
    fn non_pass_rows_must_still_use_declared_property_ids() {
        let mut value = valid_receipt();
        value["native"] = json!({
            "result": "not_proven",
            "reason": "timeout",
            "process_settled": true,
            "properties": { "unrelated": "not_proven" }
        });
        value["obligations"]["positive"]["observed"] = json!(1);
        value["obligations"]["positive"]["satisfied"] = json!(0);

        let errors = validate_value(&value);
        assert!(errors.iter().any(|error| error.contains("must be declared")));
    }

    #[test]
    fn pass_requires_native_raw_evidence_not_metadata_only() {
        for kind in ["correspondence", "route", "process_receipt", "other"] {
            let mut value = valid_receipt();
            value["artifacts"] = json!([
                {
                    "id": "correspondence",
                    "kind": kind,
                    "digest": ALT,
                    "path": "target/formal/metadata.json",
                    "required": true
                }
            ]);
            let errors = validate_value(&value);
            assert!(errors.iter().any(|error| {
                error.contains("required stdout, stderr, or structured_report")
            }));
        }
    }

    #[test]
    fn active_native_pass_requires_current_freshness() {
        let mut value = valid_receipt();
        value["freshness"]["status"] = json!("stale");
        value["freshness"]["reason"] = json!("production changed");

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("active native pass requires current freshness"))
        );
    }

    #[test]
    fn active_disposition_rejects_retired_rollout() {
        let mut value = valid_receipt();
        value["claim"]["rollout"] = json!("retired");

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("active disposition cannot be retired"))
        );
    }

    #[test]
    fn settled_process_rejects_process_unsettled_reason() {
        let mut value = valid_receipt();
        value["native"] = json!({
            "result": "not_proven",
            "reason": "process_unsettled",
            "process_settled": true,
            "properties": {}
        });
        value["obligations"]["positive"]["observed"] = json!(0);
        value["obligations"]["positive"]["satisfied"] = json!(0);

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("settled process cannot"))
        );
    }

    #[test]
    fn current_freshness_rejects_unknown_source_state() {
        let mut value = valid_receipt();
        value["source"]["state"] = json!("unknown");

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("known source state"))
        );
    }

    #[test]
    fn production_bound_pass_requires_established_correspondence() {
        let mut value = valid_receipt();
        value["correspondence"]["status"] = json!("not_proven");
        value["correspondence"]["controls"]["observed"] = json!(0);
        value["correspondence"]["controls"]["satisfied"] = json!(0);
        value["correspondence"]["artifact_ids"] = json!([]);
        value["correspondence"]["reason"] = json!("wrapper changed");

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("requires established correspondence"))
        );
    }

    #[test]
    fn correspondence_ids_must_bind_correspondence_artifacts() {
        let mut value = valid_receipt();
        value["artifacts"][1]["kind"] = json!("structured_report");

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("must reference a correspondence artifact"))
        );
    }

    #[test]
    fn established_correspondence_requires_required_evidence() {
        let mut value = valid_receipt();
        value["artifacts"][1]["required"] = json!(false);

        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("requires a required correspondence artifact"))
        );
    }

'''
marker = '''    #[test]
    fn counterexample_requires_bound_trace() {
'''
if source.count(marker) != 1:
    raise SystemExit("test insertion anchor must occur exactly once")
source = source.replace(marker, tests + marker, 1)

path.write_text(source, encoding="utf-8")

for newline_path in [
    Path("xtask/src/tasks/gate_receipts.rs"),
    Path("xtask/src/tasks/mod.rs"),
]:
    content = newline_path.read_text(encoding="utf-8")
    newline_path.write_text(content.rstrip("\n") + "\n", encoding="utf-8")
