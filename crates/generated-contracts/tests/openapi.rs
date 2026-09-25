//! Public `OpenAPI` references, authentication boundaries, and Rust DTO examples.

use generated_contracts::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::BTreeSet;

fn document() -> Result<Value, serde_json::Error> {
    serde_json::from_str(include_str!("../../../contracts/openapi.json"))
}

fn check_references(document: &Value, value: &Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get("$ref") {
                assert!(
                    reference.starts_with("#/"),
                    "external reference {reference}"
                );
                assert!(
                    document.pointer(&reference[1..]).is_some(),
                    "unresolved reference {reference}"
                );
            }
            for child in object.values() {
                check_references(document, child);
            }
        }
        Value::Array(values) => {
            for child in values {
                check_references(document, child);
            }
        }
        _ => {}
    }
}

#[test]
fn all_public_operations_have_valid_references_and_security()
-> Result<(), Box<dyn std::error::Error>> {
    let doc = document()?;
    assert_eq!(doc["openapi"], "3.1.1");
    check_references(&doc, &doc);
    let paths = doc["paths"].as_object().ok_or("paths missing")?;
    let mut operation_ids = BTreeSet::new();
    for (path, item) in paths {
        assert!(path.starts_with("/api/"));
        assert!(!path.contains("internal"));
        for (method, operation) in item.as_object().ok_or("path operations missing")? {
            assert!(operation_ids.insert(operation["operationId"].as_str().ok_or("ID missing")?));
            assert!(operation["responses"].as_object().is_some());
            let public = matches!(
                path.as_str(),
                "/api/auth/register"
                    | "/api/auth/login"
                    | "/api/auth/passkeys/login/options"
                    | "/api/auth/passkeys/login/verify"
                    | "/api/openapi.json"
            ) || (matches!(
                path.as_str(),
                "/api/disputes/{disputeId}"
                    | "/api/profiles/{principalId}"
                    | "/api/profiles/{principalId}/{section}/image"
            ) && method == "get");
            let security = if operation["security"].is_null() {
                &doc["security"]
            } else {
                &operation["security"]
            };
            if public {
                assert_eq!(security, &serde_json::json!([]));
            } else {
                assert_eq!(security[0]["sessionCookie"], serde_json::json!([]));
                if method != "get" {
                    assert_eq!(security[0]["csrfToken"], serde_json::json!([]));
                }
            }
            if operation["responses"].get("202").is_some() {
                let response = &operation["responses"]["202"];
                let response = response["$ref"]
                    .as_str()
                    .and_then(|reference| doc.pointer(&reference[1..]))
                    .unwrap_or(response);
                assert_eq!(
                    response["content"]["application/json"]["schema"]["$ref"],
                    "#/components/schemas/OperationRef"
                );
                assert!(
                    operation["parameters"]
                        .as_array()
                        .ok_or("parameters missing")?
                        .iter()
                        .any(|parameter| parameter["$ref"]
                            == "#/components/parameters/IdempotencyKey")
                );
            }
            for parameter in path.split('{').skip(1) {
                let name = parameter.split('}').next().ok_or("invalid path template")?;
                let params = operation["parameters"]
                    .as_array()
                    .ok_or("path parameters missing")?;
                assert!(
                    params.iter().any(|parameter| {
                        parameter["$ref"]
                            .as_str()
                            .and_then(|reference| doc.pointer(&reference[1..]))
                            .is_some_and(|resolved| {
                                resolved["name"] == name
                                    && resolved["in"] == "path"
                                    && resolved["required"] == true
                            })
                    }),
                    "unbound path parameter {name}"
                );
            }
        }
    }
    assert_eq!(operation_ids.len(), 72);
    assert_eq!(
        doc["components"]["securitySchemes"]["sessionCookie"]["name"],
        "kunveno_session"
    );
    Ok(())
}

fn check_examples<T: DeserializeOwned + Serialize>(
    doc: &Value,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let schema = &doc["components"]["schemas"][name];
    let examples = schema["examples"].as_array().ok_or("examples missing")?;
    assert!(!examples.is_empty(), "empty examples for {name}");
    for example in examples {
        let value: T = serde_json::from_value(example.clone())?;
        assert_eq!(
            serde_json::to_value(value)?,
            *example,
            "wire drift in {name}"
        );
    }
    Ok(())
}

#[test]
fn documented_examples_roundtrip_through_the_frozen_rust_dtos()
-> Result<(), Box<dyn std::error::Error>> {
    let doc = document()?;
    check_examples::<Money>(&doc, "Money")?;
    check_examples::<EvidenceReference>(&doc, "EvidenceReference")?;
    check_examples::<EvidenceRequest>(&doc, "EvidenceRequest")?;
    check_examples::<CompletionSubmission>(&doc, "CompletionSubmission")?;
    check_examples::<OpenDisputeRequest>(&doc, "OpenDisputeRequest")?;
    check_examples::<Week>(&doc, "Week")?;
    check_examples::<PlanningQuote>(&doc, "PlanningQuote")?;
    check_examples::<RevisionRequest>(&doc, "RevisionRequest")?;
    check_examples::<ScorePolicy>(&doc, "ScorePolicy")?;
    check_examples::<TeamRating>(&doc, "TeamRating")?;
    check_examples::<ProposalDefinition>(&doc, "ProposalDefinition")?;
    check_examples::<TaskDefinition>(&doc, "TaskDefinition")?;
    check_examples::<SessionView>(&doc, "SessionView")?;
    check_examples::<WorkerSummaryView>(&doc, "WorkerSummaryView")?;
    check_examples::<ProjectView>(&doc, "ProjectView")?;
    check_examples::<OperationRef>(&doc, "OperationRef")?;
    check_examples::<OperationView>(&doc, "OperationView")?;
    check_examples::<NotificationView>(&doc, "NotificationView")?;
    check_examples::<NotificationsPage>(&doc, "NotificationsPage")?;
    Ok(())
}

#[test]
fn approvals_privacy_and_sse_use_the_correct_public_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    let doc = document()?;
    for path in [
        "/api/projects/{projectId}/planning/accept",
        "/api/projects/{projectId}/planning/accept-delivery",
        "/api/projects/{projectId}/proposals/{proposalId}/approve",
    ] {
        assert_eq!(
            doc["paths"][path]["post"]["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/RevisionRequest"
        );
    }
    let schemas = &doc["components"]["schemas"];
    assert_eq!(schemas["Revision"]["maximum"].as_u64(), Some(u64::MAX));
    assert_eq!(
        schemas["RevisionRequest"]["required"],
        serde_json::json!(["expectedRevision"])
    );
    assert_eq!(schemas["RevisionRequest"]["additionalProperties"], false);
    assert_eq!(
        doc["paths"]["/api/workers"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
            ["items"]["$ref"],
        "#/components/schemas/WorkerSummaryView"
    );
    assert!(
        schemas["CalendarSummaryView"]["properties"]
            .get("reservations")
            .is_none()
    );
    assert!(
        schemas["WeeklyCommitment"]["properties"]
            .get("projectId")
            .is_none()
    );
    let stream =
        &doc["paths"]["/api/events"]["get"]["responses"]["200"]["content"]["text/event-stream"];
    assert_eq!(
        stream["x-event-schema"]["$ref"],
        "#/components/schemas/NotificationView"
    );
    let example = stream["example"].as_str().ok_or("SSE example missing")?;
    assert!(example.contains("event: notification\n"));
    let data = example
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .ok_or("SSE data missing")?;
    let notification: NotificationView = serde_json::from_str(data)?;
    assert_eq!(notification.notification_id, 1);
    assert_eq!(notification.read_at, None);
    Ok(())
}
