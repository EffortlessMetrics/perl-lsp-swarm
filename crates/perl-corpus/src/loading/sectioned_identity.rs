use super::typed::{self, CorpusLoadError, SectionedCorpusDocument};
use std::collections::BTreeSet;
use std::path::Path;

/// Load one strict sectioned corpus document and assign structured case IDs.
///
/// The legacy metadata parser retains its existing leaf-derived `Section.id`
/// compatibility field. The public loader assigns a separate structured
/// section identity from source order and title, while the parent asset ID
/// remains an independent string field. No platform path parser interprets the
/// asset identity.
pub fn load_sectioned_corpus_document(
    asset_id: impl Into<String>,
    path: impl AsRef<Path>,
) -> Result<SectionedCorpusDocument, CorpusLoadError> {
    let asset_id = asset_id.into();
    let mut document =
        typed::load_sectioned_corpus_document(asset_id.clone(), path.as_ref())?;
    let mut structured_ids = BTreeSet::new();

    for (index, case) in document.cases.iter_mut().enumerate() {
        let section_id = case
            .section
            .explicit_id
            .clone()
            .unwrap_or_else(|| generated_section_id(index + 1, &case.section.title));
        if !structured_ids.insert(section_id.clone()) {
            return Err(CorpusLoadError::DuplicateSectionId {
                asset_id,
                section_id,
            });
        }

        case.id.asset_id.clone_from(&asset_id);
        case.id.section_id = section_id;
    }

    Ok(document)
}

fn generated_section_id(index: usize, title: &str) -> String {
    let title = slugify(title);
    if title.is_empty() {
        format!("generated-{index}")
    } else {
        format!("generated-{index}-{title}")
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in value.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            separator = false;
        } else if !slug.is_empty() && !separator {
            slug.push('-');
            separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const GENERATED_CASE: &str = concat!(
        "==========================================\n",
        "Generated case\n",
        "==========================================\n",
        "my $value = 1;\n",
    );

    #[test]
    fn structured_generated_identity_ignores_runtime_filename()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let first = root.path().join("first.txt");
        let second = root.path().join("second.txt");
        fs::write(&first, GENERATED_CASE)?;
        fs::write(&second, GENERATED_CASE)?;

        let first = load_sectioned_corpus_document("corpus/a\\b.txt", &first)?;
        let second = load_sectioned_corpus_document("corpus/a\\b.txt", &second)?;

        assert_eq!(first.cases[0].id, second.cases[0].id);
        assert_eq!(first.cases[0].id.asset_id, "corpus/a\\b.txt");
        assert_eq!(
            first.cases[0].id.section_id,
            "generated-1-generated-case"
        );
        Ok(())
    }

    #[test]
    fn explicit_section_id_remains_authoritative()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("case.txt");
        fs::write(
            &path,
            concat!(
                "==========================================\n",
                "Explicit case\n",
                "==========================================\n",
                "# @id: explicit.case\n",
                "my $value = 1;\n",
            ),
        )?;

        let document = load_sectioned_corpus_document("corpus/case.txt", &path)?;
        assert_eq!(document.cases[0].id.section_id, "explicit.case");
        Ok(())
    }

    #[test]
    fn explicit_and_generated_structured_ids_cannot_collide()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("collision.txt");
        fs::write(
            &path,
            concat!(
                "==========================================\n",
                "Explicit case\n",
                "==========================================\n",
                "# @id: generated-2-generated-case\n",
                "my $first = 1;\n",
                "==========================================\n",
                "Generated case\n",
                "==========================================\n",
                "my $second = 2;\n",
            ),
        )?;

        assert!(matches!(
            load_sectioned_corpus_document("corpus/collision.txt", &path),
            Err(CorpusLoadError::DuplicateSectionId {
                section_id,
                ..
            }) if section_id == "generated-2-generated-case"
        ));
        Ok(())
    }
}
