use perl_parser_core::error_recovery::SyncPoint;

#[test]
fn sync_point_equality() -> Result<(), Box<dyn std::error::Error>> {
    assert_ne!(SyncPoint::Semicolon, SyncPoint::Eof);
    Ok(())
}
