//! Numeric critic severity, independent of transport and legacy output.

use serde::{Deserialize, Serialize};

/// Severity levels for Perl::Critic violations.
///
/// # The variant names are threshold names, not severity names
///
/// Perl::Critic scores each violation from **1 (least severe) to 5 (most
/// severe)** -- see [`Perl::Critic::Violation`], which states the severity is
/// "an integer ranging from 1 to 5, where 5 is the 'most' severe".
///
/// The names below (`gentle`, `stern`, `harsh`, `cruel`, `brutal`) come from
/// `perlcritic`'s command-line *threshold* shortcuts, and they run in the
/// opposite direction: they describe how harsh the critic is being, i.e. how
/// far down the severity scale it will report. `--gentle` (5) reports only
/// the most severe violations; `--brutal` (1) reports everything.
///
/// So `Gentle` carries numeric severity **5 and is the most severe bucket**,
/// and `Brutal` carries numeric severity **1 and is the least severe bucket**.
/// This reads backwards at a glance and has been misread before -- do not
/// "correct" the numeric direction without re-reading the upstream
/// documentation first. Transport severity projection belongs to adapters.
///
/// [`Perl::Critic::Violation`]: https://metacpan.org/pod/Perl::Critic::Violation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Most severe issues -- perlcritic severity 5 (`--gentle` threshold).
    Gentle = 5,
    /// Severe issues -- perlcritic severity 4 (`--stern` threshold).
    Stern = 4,
    /// Important issues -- perlcritic severity 3 (`--harsh` threshold).
    Harsh = 3,
    /// Less severe issues -- perlcritic severity 2 (`--cruel` threshold).
    Cruel = 2,
    /// Least severe issues -- perlcritic severity 1 (`--brutal` threshold).
    Brutal = 1,
}

impl Severity {
    /// Converts a numeric severity (1-5) to a `Severity` variant.
    ///
    /// Values outside 1-5 default to `Harsh`.
    pub fn from_number(n: u8) -> Self {
        match n {
            1 => Self::Brutal,
            2 => Self::Cruel,
            3 => Self::Harsh,
            4 => Self::Stern,
            5 => Self::Gentle,
            _ => Self::Harsh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Severity;

    #[test]
    fn numeric_conversion_preserves_the_fallback_for_every_u8() {
        for number in u8::MIN..=u8::MAX {
            let expected = if (1..=5).contains(&number) { number } else { 3 };
            assert_eq!(
                Severity::from_number(number) as u8,
                expected,
                "numeric severity conversion must retain its original fallback for {number}",
            );
        }
    }

    #[test]
    fn serde_names_and_discriminants_remain_stable() -> Result<(), serde_json::Error> {
        for (severity, number, wire) in [
            (Severity::Gentle, 5, "\"Gentle\""),
            (Severity::Stern, 4, "\"Stern\""),
            (Severity::Harsh, 3, "\"Harsh\""),
            (Severity::Cruel, 2, "\"Cruel\""),
            (Severity::Brutal, 1, "\"Brutal\""),
        ] {
            assert_eq!(severity as u8, number);
            assert_eq!(serde_json::to_string(&severity)?, wire);
            assert_eq!(serde_json::from_str::<Severity>(wire)?, severity);
        }
        Ok(())
    }

    #[test]
    fn serde_does_not_silently_accept_numeric_or_unknown_input() {
        for raw in ["3", "null", "\"harsh\"", "\"unknown\""] {
            assert!(
                serde_json::from_str::<Severity>(raw).is_err(),
                "ownership relocation must not broaden severity deserialization: {raw}",
            );
        }
    }
}
