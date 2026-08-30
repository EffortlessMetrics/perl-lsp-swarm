use perl_parser_core::error_recovery::SyncPoint;

#[test]
fn sync_point_equality() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(SyncPoint::Semicolon, SyncPoint::Semicolon.clone());
    assert_eq!(SyncPoint::CloseBrace, SyncPoint::CloseBrace.clone());
    assert_eq!(SyncPoint::Keyword, SyncPoint::Keyword.clone());
    assert_eq!(SyncPoint::Eof, SyncPoint::Eof.clone());
    assert_ne!(SyncPoint::Semicolon, SyncPoint::Eof);
    Ok(())
}
