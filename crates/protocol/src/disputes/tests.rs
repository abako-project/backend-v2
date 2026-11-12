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

#[test]
fn current_evidence_scale_is_explicit_and_public_json_stays_compatible()
-> Result<(), Box<dyn std::error::Error>> {
    let original = EvidenceReference::new("https://example.test/delivery".into())?;
    let legacy = EvidenceRecordReferenceV1::Url(original.clone());
    assert_eq!(legacy.encode()[0], 0);
    assert_eq!(&legacy.encode()[1..], &original.encode());
    assert_eq!(
        legacy,
        EvidenceRecordReferenceV1::decode(&mut &legacy.encode()[..])?
    );
    assert_eq!(
        serde_json::to_value(&legacy)?,
        serde_json::to_value(&original)?
    );
    let committed = EvidenceRecordReferenceV1::Committed(EvidenceCommitmentV1 {
        record_id: EntityId::from_bytes([9; 16]),
        hash: PayloadHash::from_bytes([7; 32]),
    });
    let bytes = committed.encode();
    assert_eq!(bytes.len(), 49);
    assert_eq!(bytes[0], 1);
    assert_eq!(
        committed,
        EvidenceRecordReferenceV1::decode(&mut &bytes[..])?
    );
    assert!(EvidenceReference::decode(&mut &bytes[..]).is_err());
    assert!(committed.url().is_none());
    for value in [
        serde_json::json!({"url":"https://example.test","hash":PayloadHash::from_bytes([7;32])}),
        serde_json::json!({"recordId":EntityId::from_bytes([9;16]),"hash":PayloadHash::from_bytes([7;32]),"url":"https://example.test"}),
    ] {
        assert!(serde_json::from_value::<EvidenceRecordReferenceV1>(value).is_err());
    }
    assert!(EvidenceRecordReferenceV1::decode(&mut &[0xff, 2][..]).is_err());
    assert!(EvidenceRecordReferenceV1::decode(&mut &[0xfd, 0xff][..]).is_err());
    assert!(EvidenceRecordReferenceV1::decode(&mut &bytes[..48]).is_err());
    Ok(())
}
