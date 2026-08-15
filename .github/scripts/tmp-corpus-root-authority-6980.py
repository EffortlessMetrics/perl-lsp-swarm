from pathlib import Path

files_path = Path("crates/perl-corpus/src/files.rs")
source = files_path.read_text()

old_discover = '''    /// Discover corpus paths through the legacy developer-convenience contract.
    ///
    /// This non-fallible compatibility path does not preserve provenance or
    /// validate the selected path. Use [`Self::try_discover`] for validated
    /// developer discovery and [`Self::resolve_authoritative`] for load-bearing
    /// work.
    pub fn discover() -> Self {
        if let Some(root) = env::var_os(CORPUS_ROOT_ENV) {
            return Self::from_root(PathBuf::from(root));
        }

        match CorpusRoot::resolve_for_development(None) {
            Ok(authority) => Self::from_root(authority.into_path()),
            Err(_) => Self::from_root(PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        }
    }
'''
new_discover = '''    /// Discover corpus paths through the historical unchecked compatibility contract.
    ///
    /// This path preserves the raw environment value or compile-time workspace
    /// path, including a symlinked workspace ancestor. It does not validate or
    /// retain provenance and must not be used as evidence authority. Use
    /// [`Self::try_discover`] for strict developer discovery and
    /// [`Self::resolve_authoritative`] for load-bearing work.
    pub fn discover() -> Self {
        if let Some(root) = env::var_os(CORPUS_ROOT_ENV) {
            return Self::from_root(PathBuf::from(root));
        }

        Self::from_root(find_compatibility_workspace_root())
    }
'''
if old_discover in source:
    source = source.replace(old_discover, new_discover)
elif new_discover not in source:
    raise SystemExit("could not identify compatibility discovery contract")

old_layout = '''    /// Require the checked-in repository layers owned by the current topology.
    ///
    /// Missing, linked, non-directory, unreadable, or recursively unreadable
    /// layers fail instead of becoming an empty or partial successful corpus.
    pub fn require_repository_layout(&self) -> Result<(), CorpusRootError> {
        let authority = CorpusRoot::explicit(&self.root)?;
        let test_corpus =
            authority.require_directory(Path::new("test_corpus"), "test_corpus")?;
        validate_readable_tree(&test_corpus, "test_corpus")?;
        authority.require_directory(Path::new("test_corpus"), "test_corpus")?;

        let fuzz = authority.require_directory(Path::new("crates/perl-corpus/fuzz"), "fuzz")?;
        validate_readable_tree(&fuzz, "fuzz")?;
        authority.require_directory(Path::new("crates/perl-corpus/fuzz"), "fuzz")?;
        Ok(())
    }
'''
new_layout = '''    /// Require the checked-in repository layer directories owned by the current topology.
    ///
    /// This validates root and required-directory authority only. Selected
    /// descendant traversal belongs to [`crate::CorpusTopology`], and exact
    /// opened-file bytes belong to the shared member reader tracked by #7693.
    pub fn require_repository_layout(&self) -> Result<(), CorpusRootError> {
        let authority = CorpusRoot::explicit(&self.root)?;
        authority.require_directory(Path::new("test_corpus"), "test_corpus")?;
        authority.require_directory(Path::new("crates/perl-corpus/fuzz"), "fuzz")?;
        Ok(())
    }
'''
if old_layout in source:
    source = source.replace(old_layout, new_layout)
elif new_layout not in source:
    raise SystemExit("could not identify repository layout contract")

compat_helpers = '''fn find_compatibility_workspace_root() -> PathBuf {
    find_compatibility_workspace_root_from(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn find_compatibility_workspace_root_from(start: &Path) -> PathBuf {
    for ancestor in start.ancestors() {
        let manifest = ancestor.join("Cargo.toml");
        let Ok(contents) = fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(parsed) = toml::from_str::<toml::Value>(&contents) else {
            continue;
        };
        if parsed
            .get("workspace")
            .and_then(toml::Value::as_table)
            .is_some()
        {
            return ancestor.to_path_buf();
        }
    }
    start.to_path_buf()
}

'''
if "fn validate_readable_tree(" in source:
    start = source.index("fn validate_readable_tree(")
    end = source.index("/// Return test corpus files", start)
    source = source[:start] + compat_helpers + source[end:]
elif compat_helpers not in source:
    raise SystemExit("could not identify traversal/helper contract")

new_tests = r'''    #[cfg(unix)]
    #[test]
    fn compatibility_workspace_discovery_preserves_symlinked_ancestor()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = temp_root("perl_corpus_compat_symlink_workspace")?;
        let real_workspace = root.join("real-workspace");
        let crate_dir = real_workspace.join("crates/perl-corpus");
        fs::create_dir_all(&crate_dir)?;
        fs::write(
            real_workspace.join("Cargo.toml"),
            "[workspace]\nmembers = []\n",
        )?;
        let linked_workspace = root.join("linked-workspace");
        symlink(&real_workspace, &linked_workspace)?;
        let linked_crate = linked_workspace.join("crates/perl-corpus");

        assert_eq!(
            find_compatibility_workspace_root_from(&linked_crate),
            linked_workspace
        );
        assert!(matches!(
            CorpusRoot::explicit(&linked_workspace),
            Err(CorpusRootError::SymlinkUnsupported { .. })
        ));

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn required_layout_leaves_excluded_metadata_symlink_to_topology()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = temp_root("perl_corpus_excluded_metadata_link")?;
        fs::create_dir_all(root.join("test_corpus"))?;
        let fuzz = root.join("crates/perl-corpus/fuzz");
        fs::create_dir_all(&fuzz)?;
        let outside = root.join("outside-readme.md");
        fs::write(&outside, "metadata only\n")?;
        symlink(&outside, fuzz.join("README.md"))?;

        CorpusPaths::try_from_root(&root)?.require_repository_layout()?;

        fs::remove_dir_all(&root)?;
        Ok(())
    }
'''
if "fn nested_probe_failure_is_not_green()" in source:
    start = source.index("    #[test]\n    fn nested_probe_failure_is_not_green()")
    end = source.rfind("\n}")
    source = source[:start] + new_tests + source[end:]
elif "fn compatibility_workspace_discovery_preserves_symlinked_ancestor()" not in source:
    raise SystemExit("could not identify root contract tests")
files_path.write_text(source)

readme = Path("crates/perl-corpus/README.md")
text = readme.read_text()
old = '''`CorpusPaths::try_discover` adds validated compile-time workspace discovery for developer convenience. `CorpusPaths::discover` remains the historical non-fallible compatibility surface: it does not validate or retain provenance and is not evidence authority.

Required repository layers are traversed recursively before success. Missing, replaced, linked, non-directory, or unreadable nested populations cannot become an empty or partial green corpus. The existing binary and legacy convenience consumers are migrated separately under #7025 and #7033.
'''
new = '''`CorpusPaths::try_discover` adds strict compile-time workspace discovery for developer convenience. `CorpusPaths::discover` remains the historical non-fallible compatibility surface: it preserves the raw environment or compile-time workspace path, including a symlinked ancestor, and does not validate or retain provenance. It is not evidence authority and emits no validation verdict.

`require_repository_layout` proves only the bound root and required `test_corpus/` and fuzz directories. `CorpusTopology` owns selected descendant traversal and symlink/member policy; the shared opened-file authority tracked by #7693 owns the exact bytes consumed by load-bearing readers. Missing, linked, non-directory, or unreadable required layer directories fail, while excluded metadata leaves remain outside the selected denominator. The existing binary and legacy convenience consumers are migrated separately under #7025 and #7033.
'''
if old in text:
    text = text.replace(old, new)
elif new not in text:
    raise SystemExit("could not identify README root contract")
readme.write_text(text)

claude = Path("crates/perl-corpus/CLAUDE.md")
text = claude.read_text()
text = text.replace(
    "- `CorpusPaths::discover` and `from_root`: original unchecked compatibility shape; not evidence authority.\n",
    "- `CorpusPaths::discover` and `from_root`: original unchecked compatibility shape; raw environment/compile-time paths, including symlinked workspace ancestors, are preserved and are not evidence authority.\n",
)
text = text.replace(
    "- `require_repository_layout` recursively traverses `test_corpus/` and `crates/perl-corpus/fuzz/`, propagates nested enumeration/metadata failures, and rejects nested symbolic links.\n- The root and top-level layer are revalidated around traversal. This is still path-based authority, not a capability-safe directory handle.\n",
    "- `require_repository_layout` validates the bound root and required `test_corpus/` and `crates/perl-corpus/fuzz/` directories only.\n- `CorpusTopology` owns selected descendant traversal and symlink/member policy; #7693 owns no-follow opened-file bytes. Do not add a second whole-tree leaf policy to root resolution.\n",
)
text = text.replace(
    "| `CorpusPaths`, `ResolvedCorpusPaths` | `files.rs` | Preserved compatibility shape plus validated provenance wrapper and recursive layout proof |\n",
    "| `CorpusPaths`, `ResolvedCorpusPaths` | `files.rs` | Preserved compatibility shape plus validated provenance wrapper and required-layer directory proof |\n",
)
claude.write_text(text)
