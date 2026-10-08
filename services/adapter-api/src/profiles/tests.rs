use super::models::{ClientProfile, ProfilesView, PublicProfilesView, WorkerProfile};
use crate::state::Error;
use generated_contracts::PrincipalId;
use serde_json::json;

#[test]
fn private_fields_never_appear_in_public_projection() -> Result<(), Box<dyn std::error::Error>> {
    let client: ClientProfile = serde_json::from_value(json!({
        "name": "Client",
        "company": "Example Ltd",
        "department": "Private department",
        "website": "https://example.test",
        "description": "A project",
        "location": "Madrid",
        "languages": ["es"]
    }))?;
    let worker: WorkerProfile = serde_json::from_value(json!({
        "name": "Worker",
        "contactEmail": "worker@example.test",
        "githubUsername": "example-worker",
        "portfolioUrl": "https://example.test/work",
        "biography": "Public biography",
        "background": "Private background",
        "proficiency": "senior",
        "location": "Madrid",
        "languages": ["es"]
    }))?;
    let full = ProfilesView {
        principal_id: PrincipalId::from_bytes([1; 16]),
        client: Some(client),
        worker: Some(worker),
    };
    let public = serde_json::to_value(PublicProfilesView::from(full))?;
    let serialized = serde_json::to_string(&public)?;
    assert!(!serialized.contains("department"));
    assert!(!serialized.contains("background"));
    assert!(!serialized.contains("proficiency"));
    assert!(!serialized.contains("email"));
    assert!(!serialized.contains("contactEmail"));
    assert!(!serialized.contains("worker@example.test"));
    assert!(serialized.contains("Public biography"));
    assert!(serialized.contains("Example Ltd"));
    Ok(())
}

#[test]
fn profile_validation_rejects_unbounded_or_unsafe_input() -> Result<(), Box<dyn std::error::Error>>
{
    let unknown = serde_json::from_value::<ClientProfile>(json!({
        "name": "Client", "company": null, "department": null,
        "website": null, "description": null, "location": null,
        "languages": [], "isAdmin": true
    }));
    assert!(unknown.is_err());

    let client: ClientProfile = serde_json::from_value(json!({
        "name": "Client", "company": null, "department": null,
        "website": "javascript:alert(1)", "description": null,
        "location": null, "languages": []
    }))?;
    assert!(matches!(client.validate(), Err(Error::Invalid)));

    let worker: WorkerProfile = serde_json::from_value(json!({
        "name": "Worker", "githubUsername": "invalid/username",
        "portfolioUrl": null, "biography": null, "background": null,
        "proficiency": null, "location": null, "languages": []
    }))?;
    assert!(matches!(worker.validate(), Err(Error::Invalid)));
    Ok(())
}

#[test]
fn contact_email_is_optional_bounded_and_validated() -> Result<(), Box<dyn std::error::Error>> {
    let base = json!({"name":"Worker", "githubUsername":null, "portfolioUrl":null,
        "biography":null, "background":null, "proficiency":null, "location":null, "languages":[]});
    let legacy: WorkerProfile = serde_json::from_value(base.clone())?;
    assert!(legacy.contact_email.is_none());
    for email in [json!(null), json!("worker@example.test")] {
        let mut value = base.clone();
        value["contactEmail"] = email;
        serde_json::from_value::<WorkerProfile>(value)?.validate()?;
    }
    for email in [
        String::new(),
        "@example.test".into(),
        "worker@".into(),
        "worker@@example.test".into(),
        "worker name@example.test".into(),
        "worker\n@example.test".into(),
        format!("{}@example.test", "a".repeat(255)),
    ] {
        let mut value = base.clone();
        value["contactEmail"] = json!(email);
        assert!(matches!(
            serde_json::from_value::<WorkerProfile>(value)?.validate(),
            Err(Error::Invalid)
        ));
    }
    Ok(())
}
