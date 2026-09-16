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
    state.roles.get_mut(&1).ok_or_else(Error::internal)?.name = "replacement".into();
    assert!(State::restore(&state.encode()?, root).is_err());
    let mut state = State::new(root)?;
    state.balances.insert(root, Money::new(1));
    assert!(State::restore(&state.encode()?, root).is_err());
    Ok(())
}
