use perl_parser_core::SourceRegionIndex;
use ropey::Rope;

#[test]
fn rope_and_str_build_identical_regions() -> Result<(), Box<dyn std::error::Error>> {
    let source = "my $x = \"# in string\"; # comment\n=pod\nbody\n=cut\n";
    let from_str = SourceRegionIndex::build(source);
    let rope = Rope::from_str(source);
    let from_rope = SourceRegionIndex::build(&rope.to_string());
    assert_eq!(from_str.regions(), from_rope.regions());
    Ok(())
}
