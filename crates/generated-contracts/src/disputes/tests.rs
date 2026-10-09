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
        assert!(EvidenceReference::new(url.to_owned()).is_err());
        assert!(
            serde_json::from_value::<EvidenceReference>(serde_json::json!({"url":url})).is_err()
        );
        let bytes = (url.to_owned(), hash).encode();
        assert!(EvidenceReference::decode(&mut bytes.as_slice()).is_err());
    }
    assert!(EvidenceReference::new(format!("https://example.test/{}", "x".repeat(2048))).is_err());
    let reference = EvidenceReference::new("https://example.test/artifact".to_owned())?;
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

#[test]
fn evidence_uses_url_only_and_preserves_old_signed_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let url = "https://example.test/delivery";
    let reference = EvidenceReference::new(url.to_owned())?;
    assert_eq!(
        serde_json::to_value(&reference)?,
        serde_json::json!({"url":url})
    );
    for hash in ["", "no longer required"] {
        assert_eq!(
            serde_json::from_value::<EvidenceReference>(
                serde_json::json!({"url":url,"sha256":hash})
            )?,
            reference
        );
    }
    // Preserve signed bytes and command equality for operations queued before this change.
    let hash = PayloadHash::from_bytes([7; 32]);
    let old_json =
        serde_json::from_value::<EvidenceReference>(serde_json::json!({"url":url,"sha256":hash}))?;
    let old_bytes = (url.to_owned(), hash).encode();
    let restored = EvidenceReference::decode(&mut old_bytes.as_slice())?;
    assert_eq!(restored, old_json);
    assert_eq!(restored.encode(), old_bytes);
    let restored_json = serde_json::to_value(&restored)?;
    assert_eq!(restored_json, serde_json::json!({"url":url,"sha256":hash}));
    let reloaded: EvidenceReference = serde_json::from_value(restored_json)?;
    assert_eq!(reloaded.encode(), old_bytes);
    assert!(
        serde_json::from_value::<EvidenceReference>(
            serde_json::json!({"url":url,"author":"untrusted"})
        )
        .is_err()
    );
    Ok(())
}
