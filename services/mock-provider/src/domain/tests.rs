use std::cmp::Ordering;

#[cfg(feature = "storage-sqlite")]
use generated_contracts::Money;
use generated_contracts::{
    AccountId32, CalendarDefinition, Minutes, ProviderCommand, Qualifications,
    RegisterWorkerRequest, ReputationView, UnixSeconds, WorkerMode,
};

use super::{State, add_rating, compare_scores, empty_score};
#[cfg(feature = "storage-sqlite")]
use crate::Error;
use crate::Result;

#[test]
fn exact_scores_do_not_round_or_overflow() -> Result<()> {
    let low = ReputationView {
        weighted_score_sum: u128::from(u64::MAX) * 999 - 1,
        rated_minutes: u64::MAX,
    };
    let high = ReputationView {
        weighted_score_sum: u128::from(u64::MAX) * 999,
        rated_minutes: u64::MAX,
    };
    assert_eq!(compare_scores(&low, &high), Ordering::Less);
    assert_eq!(compare_scores(&high, &low), Ordering::Greater);
    assert_eq!(
        compare_scores(
            &empty_score(),
            &ReputationView {
                weighted_score_sum: 3500,
                rated_minutes: 7,
            },
        ),
        Ordering::Equal
    );
    let mut score = empty_score();
    add_rating(&mut score, 1000, Minutes::new(100))?;
    add_rating(&mut score, 100, Minutes::new(1000))?;
    assert_eq!(score.weighted_score_sum, 200_000);
    assert_eq!(score.rated_minutes, 1100);
    Ok(())
}

#[test]
fn selection_uses_only_the_reputation_for_the_requested_mode() -> Result<()> {
    let mut state = State::new(AccountId32::from_bytes([1; 32]))?;
    let first = AccountId32::from_bytes([2; 32]);
    let second = AccountId32::from_bytes([3; 32]);
    for account in [first, second] {
        state.apply(
            account,
            &ProviderCommand::RegisterWorker(RegisterWorkerRequest {
                display_name: "Candidate".into(),
                qualifications: Qualifications {
                    role_ids: Vec::new(),
                    skill_ids: Vec::new(),
                },
                calendar: CalendarDefinition {
                    default_weekly_minutes: Minutes::new(60),
                    overrides: Vec::new(),
                },
            }),
            UnixSeconds::new(100),
        )?;
    }
    state.worker_mut(first)?.worker_score = ReputationView {
        weighted_score_sum: 900,
        rated_minutes: 1,
    };
    state.worker_mut(first)?.coordinator_score = ReputationView {
        weighted_score_sum: 100,
        rated_minutes: 1,
    };
    state.worker_mut(second)?.worker_score = ReputationView {
        weighted_score_sum: 200,
        rated_minutes: 1,
    };
    state.worker_mut(second)?.coordinator_score = ReputationView {
        weighted_score_sum: 1000,
        rated_minutes: 1,
    };
    assert_eq!(state.select(&[first, second], WorkerMode::Worker)?, first);
    assert_eq!(
        state.select(&[first, second], WorkerMode::Coordinator)?,
        second
    );
    Ok(())
}

#[cfg(feature = "storage-sqlite")]
#[test]
fn persisted_state_cannot_bypass_catalog_or_supply_invariants() -> Result<()> {
    let root = AccountId32::from_bytes([1; 32]);
    let mut state = State::new(root)?;
    assert_eq!(
        State::restore(b"{}", root).err(),
        Some(Error::domain("state_configuration_mismatch"))
    );
    state.roles.get_mut(&1).ok_or_else(Error::internal)?.name = "replacement".into();
    assert!(State::restore(&state.encode()?, root).is_err());
    let mut state = State::new(root)?;
    state.balances.insert(root, Money::new(1));
    assert!(State::restore(&state.encode()?, root).is_err());
    Ok(())
}

#[cfg(feature = "storage-sqlite")]
#[test]
fn project_dates_are_recorded_and_recovered_from_legacy_events() -> Result<()> {
    use generated_contracts::{CreateProjectRequest, DomainEvent, DomainEventKind, OperationId};

    let root = AccountId32::from_bytes([1; 32]);
    let coordinator = AccountId32::from_bytes([2; 32]);
    let mut state = State::new(root)?;
    state.apply(
        coordinator,
        &ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "Coordinator".into(),
            qualifications: Qualifications {
                role_ids: Vec::new(),
                skill_ids: Vec::new(),
            },
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(60),
                overrides: Vec::new(),
            },
        }),
        UnixSeconds::new(100),
    )?;
    let worker = state.worker_mut(coordinator)?;
    worker.mode = WorkerMode::Coordinator;
    worker.coordinator_eligible = true;
    let created_at = UnixSeconds::new(500);
    let effect = state.apply(
        root,
        &ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Dated request".into(),
            description: String::new(),
        }),
        created_at,
    )?;
    let id = effect.entity_id.ok_or_else(Error::internal)?;
    assert_eq!(state.projects[&id].created_at, Some(created_at));
    state.events.push(DomainEvent {
        provider_instance_id: state.info.provider_instance_id,
        cursor: 1,
        operation_id: OperationId::from_bytes([1; 16]),
        kind: DomainEventKind::ProjectCreated,
        project_id: Some(id),
        entity_id: Some(id),
        recipients: vec![root, coordinator],
        occurred_at: created_at,
    });
    let mut old: serde_json::Value =
        serde_json::from_slice(&state.encode()?).map_err(|_| Error::internal())?;
    old["projects"][id.to_string()]
        .as_object_mut()
        .ok_or_else(Error::internal)?
        .remove("createdAt");
    let bytes = serde_json::to_vec(&old).map_err(|_| Error::internal())?;
    let restored = State::restore(&bytes, root)?;
    assert_eq!(restored.projects[&id].created_at, Some(created_at));
    assert_eq!(
        State::restore(&restored.encode()?, root)?.projects[&id].created_at,
        Some(created_at)
    );
    old["events"] = serde_json::json!([]);
    assert_eq!(
        State::restore(
            &serde_json::to_vec(&old).map_err(|_| Error::internal())?,
            root
        )?
        .projects[&id]
            .created_at,
        None
    );
    Ok(())
}
