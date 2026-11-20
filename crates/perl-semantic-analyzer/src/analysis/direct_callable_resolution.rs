//! Conservative direct-call resolution assessment derivation (#11138, slice 1).
//!
//! Projects a final disposition from typed evidence axes. Legacy empty optional
//! or vector inputs and incomplete denominators must never become
//! `exact_unresolved`.

use perl_semantic_facts::direct_callable::{
    DirectCallableCallShapeDisposition, DirectCallableCallSyntaxClass, DirectCallableClaimCeiling,
    DirectCallableContractError, DirectCallableCoverageDisposition, DirectCallableDisposition,
    DirectCallableDynamicBoundary, DirectCallableEvidenceAxes, DirectCallableLegacyInputKind,
    DirectCallableResolutionAssessment, DirectCallableSubject, DirectCallableTargetResolution,
};

/// Map parser syntax class to the call-shape disposition axis.
#[must_use]
pub fn call_shape_for_syntax_class(
    syntax_class: DirectCallableCallSyntaxClass,
) -> DirectCallableCallShapeDisposition {
    match syntax_class {
        DirectCallableCallSyntaxClass::BareParenthesizedFunctionCall => {
            DirectCallableCallShapeDisposition::ExactBareParenthesized
        }
        DirectCallableCallSyntaxClass::BareUnparenthesizedCall => {
            DirectCallableCallShapeDisposition::ExactBareUnparenthesized
        }
        DirectCallableCallSyntaxClass::PackageQualifiedFunctionCall => {
            DirectCallableCallShapeDisposition::ExactQualified
        }
        DirectCallableCallSyntaxClass::ExplicitAmpersandNamedCall => {
            DirectCallableCallShapeDisposition::ExactNamedAmpersand
        }
        DirectCallableCallSyntaxClass::CoderefVariableCall => {
            DirectCallableCallShapeDisposition::CoderefVariable
        }
        DirectCallableCallSyntaxClass::MethodCall => DirectCallableCallShapeDisposition::Method,
        DirectCallableCallSyntaxClass::BuiltinOrFeatureKeyword => {
            DirectCallableCallShapeDisposition::Unsupported
        }
        DirectCallableCallSyntaxClass::IndirectObjectOrFilehandleAmbiguous => {
            DirectCallableCallShapeDisposition::AmbiguousIndirectOrFilehandle
        }
        DirectCallableCallSyntaxClass::OtherUnsupportedOrDynamicShape => {
            DirectCallableCallShapeDisposition::Unsupported
        }
        _ => DirectCallableCallShapeDisposition::Unsupported,
    }
}

/// Derive the final assessment from a validated subject and evidence axes.
pub fn derive_direct_callable_assessment(
    subject: DirectCallableSubject,
    mut axes: DirectCallableEvidenceAxes,
) -> Result<DirectCallableResolutionAssessment, DirectCallableContractError> {
    subject.validate()?;
    if axes.call_shape == DirectCallableCallShapeDisposition::RecoveredOrSynthetic {
        axes.call_shape = call_shape_for_syntax_class(subject.syntax_class);
    }
    axes.validate()?;

    let (disposition, claim_ceiling) = project_disposition(&subject, &axes)?;
    let assessment_digest = DirectCallableResolutionAssessment::fingerprint(&subject, &axes);
    let assessment = DirectCallableResolutionAssessment {
        subject,
        axes,
        disposition,
        assessment_digest,
        claim_ceiling,
    };
    assessment.validate()?;
    Ok(assessment)
}

fn project_disposition(
    subject: &DirectCallableSubject,
    axes: &DirectCallableEvidenceAxes,
) -> Result<(DirectCallableDisposition, DirectCallableClaimCeiling), DirectCallableContractError> {
    if subject.syntax_class == DirectCallableCallSyntaxClass::MethodCall
        || axes.call_shape == DirectCallableCallShapeDisposition::Method
    {
        return Ok((
            DirectCallableDisposition::UnsupportedCallShape,
            DirectCallableClaimCeiling::NonExactEvidence,
        ));
    }

    if axes.legacy_input != DirectCallableLegacyInputKind::None {
        return Ok((
            DirectCallableDisposition::NotReadyOrIncomplete,
            DirectCallableClaimCeiling::InstrumentOrReadiness,
        ));
    }

    match axes.coverage {
        DirectCallableCoverageDisposition::InstrumentFailure => {
            return Ok((
                DirectCallableDisposition::InstrumentFailure,
                DirectCallableClaimCeiling::InstrumentOrReadiness,
            ));
        }
        DirectCallableCoverageDisposition::Stale => {
            return Ok((
                DirectCallableDisposition::Stale,
                DirectCallableClaimCeiling::InstrumentOrReadiness,
            ));
        }
        DirectCallableCoverageDisposition::NotReady => {
            return Ok((
                DirectCallableDisposition::NotReadyOrIncomplete,
                DirectCallableClaimCeiling::InstrumentOrReadiness,
            ));
        }
        DirectCallableCoverageDisposition::ExternalOrDependencyUnindexed => {
            return Ok((
                DirectCallableDisposition::ExternalOrDependencyUnindexed,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
        DirectCallableCoverageDisposition::Partial => {
            return Ok((
                DirectCallableDisposition::NotReadyOrIncomplete,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
        DirectCallableCoverageDisposition::CompleteForCallClass => {}
        _ => {
            return Ok((
                DirectCallableDisposition::NotReadyOrIncomplete,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
    }

    match axes.dynamic {
        DirectCallableDynamicBoundary::Possible
        | DirectCallableDynamicBoundary::PresentAndRelevant => {
            return Ok((
                DirectCallableDisposition::DynamicCallablePossible,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
        DirectCallableDynamicBoundary::UnknownOrIncomplete => {
            return Ok((
                DirectCallableDisposition::NotReadyOrIncomplete,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
        DirectCallableDynamicBoundary::NotApplicable
        | DirectCallableDynamicBoundary::ProvenAbsentForSubject => {}
        _ => {
            return Ok((
                DirectCallableDisposition::NotReadyOrIncomplete,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
    }

    match axes.target {
        DirectCallableTargetResolution::ExactSeveral => {
            return Ok((
                DirectCallableDisposition::AmbiguousMultipleTargets,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
        DirectCallableTargetResolution::ExactOne => {
            return Ok((
                DirectCallableDisposition::ResolvedLocalOrPackageSub,
                DirectCallableClaimCeiling::ExactBinding,
            ));
        }
        DirectCallableTargetResolution::Contradicted
        | DirectCallableTargetResolution::NotComparable => {
            return Ok((
                DirectCallableDisposition::UnsupportedCallShape,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
        DirectCallableTargetResolution::NoneObserved => {}
        _ => {
            return Ok((
                DirectCallableDisposition::UnsupportedCallShape,
                DirectCallableClaimCeiling::NonExactEvidence,
            ));
        }
    }

    if subject.syntax_class == DirectCallableCallSyntaxClass::BuiltinOrFeatureKeyword {
        return Ok((
            DirectCallableDisposition::ResolvedBuiltinOrLanguageConstruct,
            DirectCallableClaimCeiling::ExactBinding,
        ));
    }

    if axes.call_shape == DirectCallableCallShapeDisposition::AmbiguousIndirectOrFilehandle
        || axes.call_shape == DirectCallableCallShapeDisposition::Unsupported
    {
        return Ok((
            DirectCallableDisposition::UnsupportedCallShape,
            DirectCallableClaimCeiling::NonExactEvidence,
        ));
    }

    Ok((DirectCallableDisposition::ExactUnresolved, DirectCallableClaimCeiling::ExactBinding))
}
