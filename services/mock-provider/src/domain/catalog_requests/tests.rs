use generated_contracts::{
    AccountId32, CalendarDefinition, CatalogKind, CreateSkillRequest, DecideSkillRequest,
    ExecutionOutcome, Minutes, OperationId, ProviderCommand, Qualifications, RegisterWorkerRequest,
    SkillRequestDecision, SkillRequestStatus, UnixSeconds, UnsignedContractCallV1,
    UpsertCatalogEntryRequest,
};

use super::{super::VerifiedCall, State};
use crate::Result;

const NOW: UnixSeconds = UnixSeconds::new(100);

fn worker(state: &mut State, account: AccountId32) -> Result<()> {
    state.apply(
        account,
        &ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "Worker".into(),
            qualifications: Qualifications {
                role_ids: vec![1],
                skill_ids: Vec::new(),
            },
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: Vec::new(),
            },
        }),
        NOW,
    )?;
    Ok(())
}

#[test]
fn request_remains_private_and_does_not_self_qualify() -> Result<()> {
    let root = AccountId32::from_bytes([1; 32]);
    let owner = AccountId32::from_bytes([2; 32]);
    let stranger = AccountId32::from_bytes([3; 32]);
    let mut state = State::new(root)?;
    worker(&mut state, owner)?;
    let initial_skills = state.skills.len();
    let effect = state.apply(
        owner,
        &ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "  New  Skill  ".into(),
            role_ids: vec![1],
        }),
        NOW,
    )?;
    let id = effect.entity_id.ok_or_else(crate::Error::internal)?;
    assert!(effect.recipients.contains(&owner));
    assert!(effect.recipients.contains(&root));
    assert_eq!(state.skills.len(), initial_skills);
    assert!(state.worker_mut(owner)?.qualifications.skill_ids.is_empty());
    assert!(state.skill_requests(stranger).is_empty());
    assert_eq!(state.skill_requests(owner).len(), 1);
    assert_eq!(state.skill_requests(root).len(), 1);
    assert!(
        state
            .apply(
                stranger,
                &ProviderCommand::DecideSkillRequest(DecideSkillRequest {
                    request_id: id,
                    decision: SkillRequestDecision::Approve,
                }),
                NOW,
            )
            .is_err()
    );

    state.apply(
        root,
        &ProviderCommand::DecideSkillRequest(DecideSkillRequest {
            request_id: id,
            decision: SkillRequestDecision::Approve,
        }),
        NOW,
    )?;
    let request = &state.skill_requests(owner)[0];
    assert_eq!(request.status, SkillRequestStatus::Approved);
    let skill_id = request.skill_id.ok_or_else(crate::Error::internal)?;
    assert_eq!(state.skills.len(), initial_skills + 1);
    assert_eq!(state.skill_metadata[&skill_id].role_ids, vec![1]);
    assert!(
        state
            .snapshot()
            .catalog
            .skill_roles
            .iter()
            .any(|association| association.skill_id == skill_id && association.role_ids == [1])
    );
    assert!(state.worker_mut(owner)?.qualifications.skill_ids.is_empty());
    assert!(
        state
            .apply(
                root,
                &ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
                    kind: CatalogKind::Skill,
                    id: skill_id + 1,
                    name: " new skill ".into(),
                }),
                NOW,
            )
            .is_err()
    );
    state.validate()?;
    Ok(())
}

#[test]
fn rejected_request_does_not_change_catalog() -> Result<()> {
    let root = AccountId32::from_bytes([1; 32]);
    let owner = AccountId32::from_bytes([2; 32]);
    let mut state = State::new(root)?;
    worker(&mut state, owner)?;
    let initial_skills = state.skills.len();
    let effect = state.apply(
        owner,
        &ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "Unneeded Skill".into(),
            role_ids: vec![1],
        }),
        NOW,
    )?;
    let id = effect.entity_id.ok_or_else(crate::Error::internal)?;
    state.apply(
        root,
        &ProviderCommand::DecideSkillRequest(DecideSkillRequest {
            request_id: id,
            decision: SkillRequestDecision::Reject,
        }),
        NOW,
    )?;
    assert_eq!(state.skills.len(), initial_skills);
    assert_eq!(
        state.skill_requests(owner)[0].status,
        SkillRequestStatus::Rejected
    );
    assert!(
        state
            .apply(
                root,
                &ProviderCommand::DecideSkillRequest(DecideSkillRequest {
                    request_id: id,
                    decision: SkillRequestDecision::Approve,
                }),
                NOW,
            )
            .is_err()
    );
    state.validate()?;
    Ok(())
}

#[test]
fn transition_records_a_durable_request_event() -> Result<()> {
    let root = AccountId32::from_bytes([1; 32]);
    let owner = AccountId32::from_bytes([2; 32]);
    let mut state = State::new(root)?;
    worker(&mut state, owner)?;
    let call = UnsignedContractCallV1::new(
        state.info().provider_instance_id,
        OperationId::from_bytes([9; 16]),
        owner,
        0,
        UnixSeconds::new(200),
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "Durable Skill".into(),
            role_ids: vec![1],
        }),
    )?;
    let bytes = call.signable_bytes()?;
    let (state, receipt) = state.execute(VerifiedCall { call, bytes }, NOW)?;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    assert_eq!(state.events(0, 10).events.len(), 1);
    assert_eq!(state.events(0, 10).events[0].recipients.len(), 2);
    #[cfg(feature = "storage-sqlite")]
    {
        let restored = State::restore(&state.encode()?, root)?;
        assert_eq!(restored.skill_requests(owner).len(), 1);
        assert_eq!(restored.events(0, 10).events.len(), 1);
    }
    Ok(())
}
