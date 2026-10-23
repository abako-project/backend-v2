//! Compatibility at the public reader boundary before task ownership changes.
use generated_contracts::*;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn examples() -> Result<Value, serde_json::Error> {
    serde_json::from_str(include_str!("../../../contracts/openapi.json"))
}

#[test]
fn legacy_operation_and_provider_notification_shapes_are_preserved() -> TestResult {
    let legacy = r#"{"operationId":"0x01010101010101010101010101010101","status":"OutcomeUnknown","receipt":null,"errorCode":null}"#;
    let operation: OperationView = serde_json::from_str(legacy)?;
    assert!(operation.task_result.is_none());
    assert_eq!(serde_json::to_string(&operation)?, legacy);

    let example = &examples()?["components"]["schemas"]["NotificationView"]["examples"][0];
    let notification: NotificationView = serde_json::from_value(example.clone())?;
    let NotificationEvent::Provider(event) = &notification.event else {
        return Err("legacy event changed source".into());
    };
    // The union adds no tag/field around an existing provider event.
    assert_eq!(
        serde_json::to_string(&notification.event)?,
        serde_json::to_string(event)?
    );
    assert_eq!(serde_json::to_value(&notification)?, *example);
    assert!(serde_json::to_value(event)?.get("source").is_none());
    Ok(())
}

#[test]
fn api_task_events_are_distinct_and_contain_no_provider_execution_claim() -> TestResult {
    let example = &examples()?["components"]["schemas"]["AdapterTaskEvent"]["examples"][0];
    let event: NotificationEvent = serde_json::from_value(example.clone())?;
    let NotificationEvent::Task(task) = &event else {
        return Err("API task notification misclassified".into());
    };
    assert_eq!(event.kind(), DomainEventKind::TaskUpdated);
    assert_eq!(event.origin(), Some(task.origin));
    assert_eq!(event.project_id(), Some(task.project_id));
    assert_eq!(event.provider_instance_id(), task.provider_instance_id);
    assert_eq!(event.operation_id(), task.operation_id);
    assert_eq!(serde_json::to_value(&event)?, *example);
    for forbidden in [
        "cursor",
        "nonce",
        "receipt",
        "firstEventCursor",
        "lastEventCursor",
    ] {
        let mut mixed = example.clone();
        mixed[forbidden] = json!(1);
        assert!(serde_json::from_value::<NotificationEvent>(mixed).is_err());
    }
    for kind in ["ProjectCancelled", "AccountFunded", "Unknown"] {
        let mut invalid = example.clone();
        invalid["kind"] = json!(kind);
        assert!(serde_json::from_value::<NotificationEvent>(invalid).is_err());
    }
    let mut missing_source = example.clone();
    missing_source
        .as_object_mut()
        .ok_or("missing object")?
        .remove("source");
    assert!(serde_json::from_value::<NotificationEvent>(missing_source).is_err());
    for source in [json!("Provider"), json!("Unknown"), Value::Null] {
        let mut invalid = example.clone();
        invalid["source"] = source;
        assert!(serde_json::from_value::<NotificationEvent>(invalid).is_err());
    }
    let mut mixed_provider =
        examples()?["components"]["schemas"]["NotificationView"]["examples"][0]["event"].clone();
    mixed_provider["source"] = json!("AdapterTask");
    assert!(serde_json::from_value::<NotificationEvent>(mixed_provider).is_err());
    Ok(())
}
