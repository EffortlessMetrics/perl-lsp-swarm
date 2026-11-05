use anyhow::{Context, Result};
use perl_corpus::byte_fidelity::{ByteFidelity, Encoding, NewlineStyle};
use perl_corpus::{ConceptRegistry, SidecarValidationContext, load_and_validate_sidecar};
use std::path::{Path, PathBuf};

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(root) = manifest_dir.parent().and_then(|path| path.parent()) else {
        anyhow::bail!("unable to discover workspace root from {}", manifest_dir.display());
    };
    Ok(root.to_path_buf())
}

#[test]
fn seeded_sidecars_parse_and_validate() -> Result<()> {
    let root = workspace_root()?;
    let sidecar_root = root.join("tests/perl-corpus");
    let context = SidecarValidationContext::discover(&sidecar_root)
        .with_context(|| format!("binding sidecar root {}", sidecar_root.display()))?;
    let sidecars: Vec<PathBuf> = context.sidecars().map(Path::to_path_buf).collect();
    assert!(!sidecars.is_empty(), "expected seeded parser sidecars");

    let registry_path = sidecar_root.join("concepts.toml");
    let registry = if registry_path.exists() {
        Some(
            ConceptRegistry::load(&registry_path)
                .with_context(|| format!("loading registry {}", registry_path.display()))?,
        )
    } else {
        None
    };

    for sidecar in &sidecars {
        let validation = load_and_validate_sidecar(&context, sidecar, registry.as_ref())
            .with_context(|| format!("sidecar should load and validate: {}", sidecar.display()))?;

        assert!(
            !validation.errors.iter().any(|error| error.contains("fixture file does not exist")),
            "fixture file should exist for {}\nerrors: {:?}",
            sidecar.display(),
            validation.errors,
        );

        if registry.is_none() {
            assert!(
                validation
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("concept resolution pending")),
                "missing registry should report pending concept resolution for {}\nwarnings: {:?}",
                sidecar.display(),
                validation.warnings,
            );
        }
    }

    Ok(())
}

fn validate_crlf_utf16_seed(bytes: &[u8]) -> Result<()> {
    let expected = ByteFidelity {
        newline_style: NewlineStyle::CrLf,
        final_newline: true,
        byte_order_mark: false,
        encoding: Encoding::Utf8,
    };
    let actual = ByteFidelity::classify(bytes);
    anyhow::ensure!(actual == expected, "expected {expected}; found {actual}");

    let source = std::str::from_utf8(bytes)?;
    anyhow::ensure!(
        source.chars().any(|character| character.len_utf16() == 2),
        "expected an astral scalar to exercise UTF-16 code-unit positions",
    );
    Ok(())
}

#[test]
fn crlf_utf16_span_seeds_preserve_raw_bytes() -> Result<()> {
    let root = workspace_root()?;
    for relative_path in [
        "tests/perl-corpus/position/utf16_crlf_span.pl",
        "tests/perl-corpus/spans/utf16_crlf_positions.pl",
        "crates/perl-corpus/tests/perl-corpus/position/crlf_utf16_span.pl",
    ] {
        let bytes = std::fs::read(root.join(relative_path))
            .with_context(|| format!("reading CRLF/UTF-16 span seed {relative_path}"))?;
        validate_crlf_utf16_seed(&bytes)
            .with_context(|| format!("CRLF/UTF-16 span seed {relative_path}"))?;
    }
    Ok(())
}

#[test]
fn crlf_utf16_span_seed_guard_rejects_lossy_changes() -> Result<()> {
    let source = "my $emoji = \"🙂\";\r\nprint $emoji;\r\n";
    validate_crlf_utf16_seed(source.as_bytes())?;

    for (change, mutated) in [
        ("LF normalization", source.replace("\r\n", "\n")),
        ("mixed newlines", source.replacen("\r\n", "\n", 1)),
        ("BMP-only text", source.replace('🙂', "é")),
    ] {
        assert!(
            validate_crlf_utf16_seed(mutated.as_bytes()).is_err(),
            "guard should reject {change}",
        );
    }
    Ok(())
}
