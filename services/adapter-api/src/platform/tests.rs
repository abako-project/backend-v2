use super::*;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn bundled_profile_matches_public_contract_and_keeps_mock_units() -> TestResult {
    let profile = load(None)?;
    assert_eq!(profile.name, "Abako");
    assert_eq!(profile.currency.decimals, 0);
    let doc: Value = serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
    let example = &doc["components"]["schemas"]["PlatformProfile"]["examples"][0];
    assert_eq!(serde_json::to_value(&profile)?, *example);
    assert_eq!(
        serde_json::from_value::<PlatformProfile>(example.clone())?,
        profile
    );
    assert_eq!(
        serde_json::to_value(generated_contracts::Money::new(1000))?,
        "1000"
    );
    for value in profile
        .project_types
        .iter()
        .map(String::as_str)
        .chain(LEGACY_PROJECT_TYPES)
    {
        assert!(allows_project_type(&profile, value));
    }
    assert!(!allows_project_type(&profile, "Unconfigured domain"));
    Ok(())
}

#[test]
fn explicit_profile_is_bounded_validated_and_never_silently_replaced() -> TestResult {
    let path = std::env::temp_dir().join(format!(
        "digesta-platform-{:032x}.json",
        u128::from_le_bytes(crate::state::random()?)
    ));
    assert!(matches!(load(Some(&path)), Err(Error::Config)));
    let mut alternative = serde_json::to_value(load(None)?)?;
    alternative["id"] = json!("architecture");
    alternative["name"] = json!("Architecture DAO");
    alternative["projectTypes"] = json!(["Building Design"]);
    std::fs::write(&path, serde_json::to_vec(&alternative)?)?;
    let profile = load(Some(&path))?;
    assert_eq!(profile.id, "architecture");
    assert!(allows_project_type(&profile, "Building Design"));
    assert!(!allows_project_type(&profile, "Full Stack"));
    assert!(allows_project_type(&profile, "SmartContract"));
    for invalid in [
        json!({"name":"incomplete"}),
        {
            let mut v = alternative.clone();
            v["serviceToken"] = json!("must not be served");
            v
        },
        {
            let mut v = alternative.clone();
            v["currency"]["decimals"] = json!(256);
            v
        },
        {
            let mut v = alternative.clone();
            v["terminology"]["externalProfile"]["icon"] = json!(" ");
            v
        },
        {
            let mut v = alternative.clone();
            v["description"] = json!("a".repeat(4001));
            v
        },
        {
            let mut v = alternative.clone();
            v["name"] = json!("bad\0name");
            v
        },
        {
            let mut v = alternative.clone();
            v["projectTypes"] = json!([]);
            v
        },
        {
            let mut v = alternative.clone();
            v["projectTypes"] = json!(["Duplicate", "Duplicate"]);
            v
        },
        {
            let mut v = alternative.clone();
            v["projectTypes"] = json!((0..101).map(|n| format!("Type {n}")).collect::<Vec<_>>());
            v
        },
    ] {
        std::fs::write(&path, serde_json::to_vec(&invalid)?)?;
        assert!(matches!(load(Some(&path)), Err(Error::Config)));
    }
    std::fs::write(&path, vec![b' '; usize::try_from(MAX_CONFIG_BYTES)? + 1])?;
    assert!(matches!(load(Some(&path)), Err(Error::Config)));
    std::fs::remove_file(path)?;
    Ok(())
}
