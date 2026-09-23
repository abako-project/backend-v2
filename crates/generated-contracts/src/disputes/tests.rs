use super::*;

#[test]
fn evidence_validation_is_shared_by_json_and_scale() -> Result<(), Box<dyn std::error::Error>> {
    let hash = PayloadHash::from_bytes([7; 32]);
    for url in [
        "",
        "http://example.test/a",
        "https://user:secret@example.test/a",
        "https://example.test/\na",
    ] {
        assert!(EvidenceReference::new(url.to_owned(), hash).is_err());
        assert!(
            serde_json::from_value::<EvidenceReference>(
                serde_json::json!({"url":url,"sha256":hash})
            )
            .is_err()
        );
        let bytes = (url.to_owned(), hash).encode();
        assert!(EvidenceReference::decode(&mut bytes.as_slice()).is_err());
    }
    assert!(
        EvidenceReference::new(format!("https://example.test/{}", "x".repeat(2048)), hash).is_err()
    );
    let reference = EvidenceReference::new("https://example.test/artifact".to_owned(), hash)?;
    assert_eq!(
        reference,
        EvidenceReference::decode(&mut reference.encode().as_slice())?
    );
    assert_eq!(
        reference,
        serde_json::from_str(&serde_json::to_string(&reference)?)?
    );
    assert!(!format!("{reference:?}").contains("example.test"));
    Ok(())
}
