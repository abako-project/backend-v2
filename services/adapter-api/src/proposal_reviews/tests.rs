use super::models::*;
use generated_contracts::ProposalView;
use serde_json::json;

#[test]
fn validates_keys_bounds_experience_and_unknown_fields() -> Result<(), Box<dyn std::error::Error>> {
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
    let proposal: ProposalView = serde_json::from_value(
        doc["components"]["schemas"]["ProjectView"]["examples"][0]["proposals"][0].clone(),
    )?;
    let key = proposal.milestones[0].definition.key;
    let request_key = proposal.milestones[0].definition.requirements[0].key;
    let mut request: PutPresentation = serde_json::from_value(
        json!({"expectedProposalRevision":proposal.revision,"expectedPresentationRevision":0,"milestones":[{"key":key,"description":"Detailed outcome","requirements":[{"key":request_key,"experience":"Senior"}]}]}),
    )?;
    request.validate(&proposal)?;
    request.milestones[0].description = "é".repeat(1001);
    assert!(request.validate(&proposal).is_err());
    request.milestones[0].description = "valid".into();
    request.milestones.push(request.milestones[0].clone());
    assert!(request.validate(&proposal).is_err());
    request.milestones.pop();
    request.milestones[0].requirements[0].key = u32::MAX;
    assert!(request.validate(&proposal).is_err());
    request.milestones[0].key = u32::MAX;
    assert!(request.validate(&proposal).is_err());
    let mut message: PostComment = serde_json::from_value(
        json!({"expectedProposalRevision":0,"requestId":"0x00000000000000000000000000000001","message":"Review comment"}),
    )?;
    message.validate()?;
    message.message = " ".into();
    assert!(message.validate().is_err());
    message.message = "é".repeat(5001);
    assert!(message.validate().is_err());
    message.message = "invalid\0value".into();
    assert!(message.validate().is_err());
    let mut spoofed = serde_json::to_value(&message)?;
    spoofed["authorAccount"] = json!("fake");
    assert!(serde_json::from_value::<PostComment>(spoofed).is_err());
    Ok(())
}
