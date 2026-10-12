//! Bind local source evidence at the executable owner, without invalidating
//! shared parser/LSP libraries or repository tooling on every source recheck.
use std::{env, path::Path};
#[path = "src/build_source_identity.rs"]
mod build_source_identity;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for key in ["PERL_LSP_BUILD_REVISION", "PERL_LSP_SOURCE_TREE_DIGEST"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let manifest = env::var("CARGO_MANIFEST_DIR")?;
    let root = Path::new(&manifest).parent().and_then(Path::parent);
    if let Some(root) = root.filter(|root| root.join(".git").exists()) {
        let observed =
            match (env::var("PERL_LSP_BUILD_REVISION"), env::var("PERL_LSP_SOURCE_TREE_DIGEST")) {
                (Ok(revision), Ok(digest)) => Some((revision, digest)),
                _ => match build_source_identity::source_identity(root) {
                    Ok(identity) => Some(identity),
                    Err(error) => {
                        println!("cargo:warning=local source identity not proven: {error}");
                        None
                    }
                },
            };
        for (key, automatic) in [
            ("PERL_LSP_BUILD_REVISION", observed.as_ref().map(|value| &value.0)),
            ("PERL_LSP_SOURCE_TREE_DIGEST", observed.as_ref().map(|value| &value.1)),
        ] {
            if let Some(value) = env::var(key).ok().or_else(|| automatic.cloned()) {
                println!("cargo:rustc-env={key}={value}");
            }
        }
        // Recheck dirty inputs and additions on every executable build. Only
        // this small facade is rebuilt; the shared implementation stays fresh.
        println!(
            "cargo:rerun-if-changed={}",
            Path::new(&env::var("OUT_DIR")?).join("source-identity-always-recheck").display()
        );
    }
    Ok(())
}
