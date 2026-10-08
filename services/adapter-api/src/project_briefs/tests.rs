use super::models::{DeliveryPreference, ProjectBrief, PutProjectBriefRequest};
use crate::state::Error;
use serde_json::{Value, json};

fn example() -> Value {
    json!({
        "summary":"A focused brief", "projectType":"MVP", "link":null,
        "objectives":["First objective","Second objective"], "constraints":[],
        "indicativeBudget":{"currency":"USD","range":"From10000To50000"},
        "delivery":{"preference":"WithinOneMonth"}
    })
}

#[test]
fn validates_text_bounds_links_and_order() -> Result<(), Box<dyn std::error::Error>> {
    let mut brief: ProjectBrief = serde_json::from_value(example())?;
    brief.summary = "🦀".repeat(280);
    brief.validate()?;
    assert_eq!(brief.objectives, ["First objective", "Second objective"]);
    brief.summary.push('a');
    assert!(matches!(brief.validate(), Err(Error::Invalid)));
    brief.summary = "\0".into();
    assert!(brief.validate().is_err());
    brief.summary.clear();
    for link in [
        "javascript:alert(1)",
        "https:example.test",
        "https://user:secret@example.test",
        "https://example.test/ white",
        "",
        "https://example.test/\n",
    ] {
        brief.link = Some(link.into());
        assert!(
            matches!(brief.validate(), Err(Error::Invalid)),
            "accepted {link:?}"
        );
    }
    brief.link = Some("https://example.test/project?q=1#details".into());
    brief.validate()?;
    brief.link = Some(format!("https://example.test/{}", "a".repeat(2048)));
    assert!(brief.validate().is_err());
    brief.link = None;
    brief.objectives = vec!["á".repeat(2000); 50];
    brief.validate()?;
    brief.objectives.push("overflow".into());
    assert!(brief.validate().is_err());
    brief.objectives = vec![" ".into()];
    assert!(brief.validate().is_err());
    brief.objectives = vec!["invalid\0text".into()];
    assert!(brief.validate().is_err());
    brief.objectives = vec!["a".repeat(2001)];
    assert!(brief.validate().is_err());
    Ok(())
}

#[test]
fn rejects_invalid_dates_enums_unknown_fields_and_revisions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut brief: ProjectBrief = serde_json::from_value(example())?;
    for date in ["2028-02-29", "2000-02-29", "2026-12-31"] {
        brief.delivery = DeliveryPreference::SpecificDate { date: date.into() };
        brief.validate()?;
    }
    for date in [
        "2026-02-29",
        "1900-02-29",
        "0000-01-01",
        "2026-04-31",
        "2026-13-01",
        "2026-01-00",
        "2026-1-01",
        "🦀🦀🦀",
        "2026/01/01",
    ] {
        brief.delivery = DeliveryPreference::SpecificDate { date: date.into() };
        assert!(brief.validate().is_err(), "accepted {date}");
    }
    for (field, value) in [
        ("projectType", json!("Unknown")),
        ("principalId", json!("spoofed")),
        (
            "indicativeBudget",
            json!({"currency":"KVN","range":"Below10000"}),
        ),
        (
            "indicativeBudget",
            json!({"currency":"USD","range":"Unknown"}),
        ),
        (
            "indicativeBudget",
            json!({"currency":"USD","range":"Below10000","amount":1}),
        ),
        ("delivery", json!({"preference":"SpecificDate"})),
        (
            "delivery",
            json!({"preference":"WithinOneMonth","date":"2026-01-01"}),
        ),
    ] {
        let mut value_to_parse = example();
        value_to_parse[field] = value;
        assert!(serde_json::from_value::<ProjectBrief>(value_to_parse).is_err());
    }
    let brief: ProjectBrief = serde_json::from_value(example())?;
    for revision in [-1, i64::MAX] {
        assert!(
            PutProjectBriefRequest {
                expected_revision: revision,
                brief: brief.clone()
            }
            .validate()
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn openapi_examples_match_adapter_models() -> Result<(), Box<dyn std::error::Error>> {
    let doc: Value = serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
    let schemas = &doc["components"]["schemas"];
    for value in schemas["PutProjectBriefRequest"]["examples"]
        .as_array()
        .ok_or("request examples")?
    {
        let request: PutProjectBriefRequest = serde_json::from_value(value.clone())?;
        request.validate()?;
        assert_eq!(serde_json::to_value(request)?, *value);
    }
    for value in schemas["ProjectBriefView"]["examples"]
        .as_array()
        .ok_or("view examples")?
    {
        let view: super::models::ProjectBriefView = serde_json::from_value(value.clone())?;
        view.brief.validate()?;
        assert_eq!(serde_json::to_value(view)?, *value);
    }
    Ok(())
}
