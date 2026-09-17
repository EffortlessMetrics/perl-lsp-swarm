use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use toml::Value;

type TestResult = Result<(), Box<dyn Error>>;

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}

fn table_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

#[test]
fn package_declares_external_corpus_root_without_shipping_repository_assets() -> TestResult {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let contents = fs::read_to_string(&manifest_path)?;
    let manifest = toml::from_str::<Value>(&contents)?;

    let asset_root = table_value(&manifest, &["package", "metadata", "perl-corpus", "asset-root"])
        .and_then(Value::as_str);
    let authority_env =
        table_value(&manifest, &["package", "metadata", "perl-corpus", "authoritative-env"])
            .and_then(Value::as_str);
    let packaged_assets =
        table_value(&manifest, &["package", "metadata", "perl-corpus", "packaged-assets"])
            .and_then(Value::as_array);
    let include = table_value(&manifest, &["package", "include"]).and_then(Value::as_array);

    if asset_root != Some("external") || authority_env != Some("PERL_CORPUS_ROOT") {
        return Err(failure(format!(
            "unexpected root metadata: asset_root={asset_root:?}, authority_env={authority_env:?}"
        )));
    }

    let packaged_assets = packaged_assets
        .ok_or_else(|| failure("missing package.metadata.perl-corpus.packaged-assets"))?;
    let packaged_asset_names: Vec<_> = packaged_assets.iter().filter_map(Value::as_str).collect();
    if packaged_asset_names != ["src", "concepts"] {
        return Err(failure(format!(
            "unexpected packaged asset declaration: {packaged_asset_names:?}"
        )));
    }

    let include = include.ok_or_else(|| failure("missing package include allowlist"))?;
    let include_entries: Vec<_> = include.iter().filter_map(Value::as_str).collect();
    let ships_external_corpus = include_entries.iter().any(|entry| {
        entry.starts_with("test_corpus")
            || entry.starts_with("crates/perl-corpus/fuzz")
            || entry.starts_with("fuzz")
    });
    if include_entries.contains(&"src/**")
        && include_entries.contains(&"concepts/**")
        && !ships_external_corpus
    {
        Ok(())
    } else {
        Err(failure(format!(
            "package include allowlist violates external-root contract: {include_entries:?}"
        )))
    }
}
