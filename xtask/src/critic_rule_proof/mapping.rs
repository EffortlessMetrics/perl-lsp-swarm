//! Proof-contract spellings mapped onto native critic types.

use perl_lsp_rs_core::tooling::perl_critic::{
    CriticFindingOrigin, CriticFindingShape, CriticRemediationEligibility, NativeCriticProfile,
    Severity,
};

use super::model::{ProofProfile, ProofRemediation, ProofSeverity};

pub(crate) fn native_profile(profile: ProofProfile) -> NativeCriticProfile {
    match profile {
        ProofProfile::Recommended => NativeCriticProfile::Recommended,
        ProofProfile::Strict => NativeCriticProfile::Strict,
    }
}

pub(crate) fn proof_severity(severity: Severity) -> ProofSeverity {
    match severity {
        Severity::Gentle => ProofSeverity::Gentle,
        Severity::Stern => ProofSeverity::Stern,
        Severity::Harsh => ProofSeverity::Harsh,
        Severity::Cruel => ProofSeverity::Cruel,
        Severity::Brutal => ProofSeverity::Brutal,
    }
}

pub(crate) fn proof_remediation(eligibility: CriticRemediationEligibility) -> ProofRemediation {
    match eligibility {
        CriticRemediationEligibility::None => ProofRemediation::None,
        CriticRemediationEligibility::Manual => ProofRemediation::Manual,
        CriticRemediationEligibility::PreviewCandidate => ProofRemediation::PreviewCandidate,
        CriticRemediationEligibility::AutomaticCandidate => ProofRemediation::AutomaticCandidate,
    }
}

pub(crate) fn origin_name(origin: CriticFindingOrigin) -> &'static str {
    match origin {
        CriticFindingOrigin::BuiltInDiagnostic => "built_in_diagnostic",
        CriticFindingOrigin::NativeCritic => "native_critic",
        CriticFindingOrigin::LegacyPolicy => "legacy_policy",
        CriticFindingOrigin::ExternalPerlCritic => "external_perl_critic",
    }
}

pub(crate) fn shape_name(shape: CriticFindingShape) -> &'static str {
    match shape {
        CriticFindingShape::General => "general",
        CriticFindingShape::LiteralUndefComparison => "literal_undef_comparison",
        CriticFindingShape::PotentiallyUndefComparison => "potentially_undef_comparison",
        CriticFindingShape::Backtick => "backtick",
        CriticFindingShape::Qx => "qx",
        CriticFindingShape::Readpipe => "readpipe",
        CriticFindingShape::SystemCall => "system_call",
        CriticFindingShape::ExecCall => "exec_call",
    }
}

#[cfg(test)]
mod tests {
    use super::{origin_name, shape_name};
    use perl_lsp_rs_core::tooling::perl_critic::{CriticFindingOrigin, CriticFindingShape};

    #[test]
    fn origin_and_shape_spellings_match_schema_enums() {
        assert_eq!(origin_name(CriticFindingOrigin::BuiltInDiagnostic), "built_in_diagnostic");
        assert_eq!(origin_name(CriticFindingOrigin::NativeCritic), "native_critic");
        assert_eq!(origin_name(CriticFindingOrigin::LegacyPolicy), "legacy_policy");
        assert_eq!(origin_name(CriticFindingOrigin::ExternalPerlCritic), "external_perl_critic");
        assert_eq!(shape_name(CriticFindingShape::General), "general");
        assert_eq!(
            shape_name(CriticFindingShape::LiteralUndefComparison),
            "literal_undef_comparison"
        );
        assert_eq!(
            shape_name(CriticFindingShape::PotentiallyUndefComparison),
            "potentially_undef_comparison"
        );
        assert_eq!(shape_name(CriticFindingShape::Backtick), "backtick");
        assert_eq!(shape_name(CriticFindingShape::Qx), "qx");
        assert_eq!(shape_name(CriticFindingShape::Readpipe), "readpipe");
        assert_eq!(shape_name(CriticFindingShape::SystemCall), "system_call");
        assert_eq!(shape_name(CriticFindingShape::ExecCall), "exec_call");
    }
}
