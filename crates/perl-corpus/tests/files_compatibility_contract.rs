use perl_corpus::{
    get_corpus_files_from, get_test_files_from, CorpusLayer, CorpusPaths,
};
use std::fs;

#[test]
fn public_corpus_paths_struct_literal_and_layer_discovery_remain_compatible()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let test_dir = root.path().join("test_corpus");
    let fuzz_dir = root.path().join("crates/perl-corpus/fuzz");
    fs::create_dir_all(test_dir.join("nested"))?;
    fs::create_dir_all(&fuzz_dir)?;

    let paths = CorpusPaths {
        root: root.path().to_path_buf(),
        test_corpus: test_dir.clone(),
        fuzz: fuzz_dir.clone(),
    };

    let test_file = test_dir.join("nested/case.PL");
    let module_file = test_dir.join("Case.Pm");
    let fuzz_file = fuzz_dir.join("fuzz_case.pl");
    fs::write(&test_file, "print 1;\n")?;
    fs::write(&module_file, "package Case; 1;\n")?;
    fs::write(&fuzz_file, "print 2;\n")?;
    fs::write(test_dir.join("ignored.txt"), "not selected\n")?;
    fs::write(test_dir.join(".hidden.pl"), "not selected\n")?;
    fs::create_dir_all(test_dir.join("_hidden"))?;
    fs::write(test_dir.join("_hidden/case.pl"), "not selected\n")?;

    let test_files = get_test_files_from(&paths);
    assert_eq!(test_files, vec![module_file.clone(), test_file.clone()]);

    let files = get_corpus_files_from(&paths);
    assert_eq!(files.len(), 3);
    assert!(files.iter().any(|file| {
        file.layer == CorpusLayer::TestCorpus && file.path == module_file
    }));
    assert!(files.iter().any(|file| {
        file.layer == CorpusLayer::TestCorpus && file.path == test_file
    }));
    assert!(files.iter().any(|file| {
        file.layer == CorpusLayer::Fuzz && file.path == fuzz_file
    }));
    assert!(files.windows(2).all(|pair| pair[0].path < pair[1].path));
    Ok(())
}
