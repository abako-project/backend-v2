//! Explicit M4 component fixtures. Project participant authorization is M6.
use super::super::*;
use super::directory_parity::{digest, projection};
use crate::calendar;
use generated_contracts::*;
use serde_json::{Value, json};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Action {
    Command {
        origin: AccountId32,
        command: ProviderCommand,
        seed: u8,
    },
    Contributions {
        project_id: EntityId,
        author: AccountId32,
        votes: Vec<(AccountId32, u8, u8, u32)>,
    },
    Coordinator {
        client: AccountId32,
        week: Week,
        seed: u8,
    },
    Assign {
        project_id: EntityId,
        client: AccountId32,
        coordinator: AccountId32,
        milestones: Vec<(EntityId, MilestoneDefinition)>,
        seed: u8,
    },
}
fn account(n: u8) -> AccountId32 {
    AccountId32::from_bytes([n; 32])
}
fn reset(engine: &mut RustEngine, seed: u8) {
    engine.context.reset(&execution::ExecutionInputs {
        version: 1,
        now: UnixSeconds::new(100),
        seed: [seed; 32],
    });
}
fn apply(engine: &mut RustEngine, action: &Action) -> Value {
    let mut next = engine.clone();
    let result = transition(&mut next, action).and_then(|outcome| {
        next.validate()?;
        Ok(outcome)
    });
    match result {
        Ok(outcome) => {
            *engine = next;
            json!({"ok":outcome})
        }
        Err(error) => json!({"error":error.code()}),
    }
}
fn transition(engine: &mut RustEngine, action: &Action) -> crate::Result<Value> {
    match action {
        Action::Command {
            origin,
            command,
            seed,
        } => {
            command.validate()?;
            reset(engine, *seed);
            let effect = engine.apply(*origin, command, UnixSeconds::new(100))?;
            Ok(
                json!({"kind":effect.kind,"entityId":effect.entity_id,"recipients":effect.recipients}),
            )
        }
        Action::Contributions { votes, .. } => {
            for (account, kind, stars, minutes) in votes {
                let score = Score::new(*stars)?;
                let weight = match kind {
                    0 => 1,
                    1 => engine.score_policy.coordinator_client_vote_weight(),
                    2 => engine.score_policy.coordinator_worker_vote_weight(),
                    _ => return Err(Error::bad("invalid_contract")),
                };
                let worker = engine.worker_mut(*account)?;
                let target = if *kind == 0 {
                    &mut worker.worker_score
                } else {
                    &mut worker.coordinator_score
                };
                add_weighted_rating(
                    target,
                    u16::from(score.get()) * 100,
                    Minutes::new(*minutes),
                    weight,
                )?;
            }
            Ok(Value::Null)
        }
        Action::Coordinator { client, week, seed } => {
            reset(engine, *seed);
            let mut candidates = Vec::new();
            for worker in engine.workers.values() {
                if worker.mode == WorkerMode::Coordinator
                    && worker.coordinator_eligible
                    && worker.account != *client
                    && calendar::available(&worker.calendar, *week)? != Minutes::ZERO
                {
                    candidates.push(worker.account);
                }
            }
            let chosen = engine.select(&candidates, WorkerMode::Coordinator)?;
            Ok(json!([chosen, engine.context.random_id()]))
        }
        Action::Assign {
            project_id,
            client,
            coordinator,
            milestones,
            seed,
        } => {
            reset(engine, *seed);
            let project = project_fixture(*project_id, *client, *coordinator);
            let mut earlier = BTreeSet::new();
            let mut result = Vec::new();
            for (id, definition) in milestones {
                let mut milestone = generated_contracts::provider_core::MilestoneCore {
                    milestone_id: *id,
                    definition: generated_contracts::provider_core::MilestoneTerms::from(
                        definition,
                    ),
                    status: None,
                    assignments: vec![],
                    worker_ratings: vec![],
                    task_storage: TaskStorageView {
                        task_storage_id: *id,
                        milestone_id: *id,
                        tasks: vec![],
                    },
                    frozen: false,
                    submissions: vec![],
                };
                engine.assign(&project, &mut milestone, &earlier)?;
                earlier.extend(milestone.assignments.iter().map(|a| a.worker));
                result.push(
                    milestone
                        .assignments
                        .iter()
                        .map(|a| (a.requirement_key, a.worker))
                        .collect::<Vec<_>>(),
                );
            }
            Ok(json!(result))
        }
    }
}
#[test]
fn canonical_reputation_matching_trace() -> TestResult {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/matching-parity.json"))?;
    assert_eq!(fixture["schemaVersion"], 1);
    let mut engine = RustEngine::new(account(1))?;
    for (index, step) in fixture["steps"]
        .as_array()
        .ok_or("steps")?
        .iter()
        .enumerate()
    {
        let action = serde_json::from_value(step["action"].clone())?;
        assert_eq!(apply(&mut engine, &action), step["outcome"], "step {index}");
        assert_eq!(digest(&engine), step["stateDigest"], "step {index}");
        assert_eq!(json!(engine.score_policy), step["policy"], "step {index}");
    }
    assert_eq!(json!(projection(&engine)), fixture["final"]);
    Ok(())
}
#[test]
#[ignore = "explicit M4 trace authoring only"]
fn export_matching_trace() -> TestResult {
    let mut engine = RustEngine::new(account(1))?;
    let initial = json!(projection(&engine));
    let mut steps = Vec::new();
    for action in scenario()? {
        let outcome = apply(&mut engine, &action);
        steps.push(json!({"action":action,"outcome":outcome,"stateDigest":digest(&engine),"policy":engine.score_policy}));
    }
    std::fs::write(
        std::env::var("MATCHING_TRACE_OUTPUT")?,
        serde_json::to_vec_pretty(
            &json!({"schemaVersion":1,"initial":initial,"steps":steps,"final":projection(&engine)}),
        )?,
    )?;
    Ok(())
}
fn command(origin: u8, command: ProviderCommand, seed: u8) -> Action {
    Action::Command {
        origin: account(origin),
        command,
        seed,
    }
}
fn calendar(origin: u8, capacity: u32, overrides: Vec<WeekOverride>) -> Action {
    command(
        origin,
        ProviderCommand::SetCalendar(CalendarDefinition {
            default_weekly_minutes: Minutes::new(capacity),
            overrides,
        }),
        origin,
    )
}
fn promote(origin: u8) -> Vec<Action> {
    vec![
        command(
            1,
            ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
                account: account(origin),
            }),
            origin,
        ),
        command(
            origin,
            ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
                mode: WorkerMode::Coordinator,
            }),
            origin,
        ),
    ]
}
fn registration(origin: u8, skills: Vec<u32>, capacity: u32) -> Action {
    command(
        origin,
        ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: format!("Candidate {origin}"),
            qualifications: Qualifications {
                role_ids: vec![3],
                skill_ids: skills,
            },
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(capacity),
                overrides: vec![],
            },
        }),
        origin,
    )
}
fn votes(id: u8, votes: Vec<(u8, u8, u8, u32)>) -> Action {
    Action::Contributions {
        project_id: EntityId::from_bytes([id; 16]),
        author: account(1),
        votes: votes
            .into_iter()
            .map(|(a, k, s, m)| (account(a), k, s, m))
            .collect(),
    }
}
fn select(client: u8, week: Week, seed: u8) -> Action {
    Action::Coordinator {
        client: account(client),
        week,
        seed,
    }
}
fn requirement(key: u32, skills: Vec<u32>, minutes: u32) -> RequirementDefinition {
    RequirementDefinition {
        key,
        role_id: 2,
        skill_ids: skills,
        minutes: Minutes::new(minutes),
        budget: Money::ZERO,
    }
}
fn milestone(
    id: u8,
    week: Week,
    requirements: Vec<RequirementDefinition>,
) -> TestResult<(EntityId, MilestoneDefinition)> {
    Ok((
        EntityId::from_bytes([id; 16]),
        MilestoneDefinition {
            key: u32::from(id),
            title: "Work".into(),
            window: WeekWindow::new(week, week)?,
            coordinator_fee: Money::ZERO,
            coordinator_minutes: Minutes::new(30),
            requirements,
        },
    ))
}
fn assign(id: u8, milestones: Vec<(EntityId, MilestoneDefinition)>, seed: u8) -> Action {
    Action::Assign {
        project_id: EntityId::from_bytes([id; 16]),
        client: account(1),
        coordinator: account(2),
        milestones,
        seed,
    }
}
#[allow(clippy::too_many_lines)]
fn scenario() -> TestResult<Vec<Action>> {
    let w1 = Week::new(2026, 1)?;
    let w2 = Week::new(2026, 2)?;
    let w3 = Week::new(2026, 3)?;
    let mut actions = Vec::new();
    for n in 2..=10 {
        actions.push(registration(
            n,
            match n {
                5 => vec![1],
                6 => vec![2],
                _ => vec![1, 2],
            },
            600,
        ));
    }
    actions.extend(promote(2));
    actions.extend(promote(3));
    actions.push(votes(
        90,
        vec![
            (2, 0, 5, 10),
            (2, 1, 1, 10),
            (3, 0, 1, 10),
            (3, 1, 5, 10),
            (4, 0, 1, 11),
            (5, 0, 5, 9),
            (6, 0, 5, 9),
            (7, 0, 4, 37),
            (9, 0, 3, 53),
            (8, 0, 5, 13),
            (10, 0, 5, 13),
        ],
    ));
    for n in [8, 10] {
        actions.push(calendar(
            n,
            600,
            vec![WeekOverride {
                week: w1,
                capacity: Minutes::ZERO,
            }],
        ));
    }
    actions.push(select(1, w1, 1));
    actions.push(select(3, w1, 2));
    actions.push(calendar(3, 0, vec![]));
    actions.push(select(1, w1, 3));
    actions.push(calendar(3, 600, vec![]));
    let policy = ScorePolicy::new(Percentage::new(50)?, Percentage::new(50)?)?
        .with_coordinator_vote_weights(0, 2)?;
    actions.push(command(2, ProviderCommand::SetScorePolicy(policy), 20));
    actions.push(command(1, ProviderCommand::SetScorePolicy(policy), 20));
    actions.push(votes(91, vec![(2, 1, 5, u32::MAX), (2, 2, 5, 100)]));
    actions.push(assign(
        100,
        vec![
            milestone(
                101,
                w1,
                vec![
                    requirement(1, vec![1, 2], 40),
                    requirement(2, vec![1, 2], 40),
                ],
            )?,
            milestone(
                102,
                w2,
                vec![
                    requirement(1, vec![1, 2], 40),
                    requirement(2, vec![1, 2], 40),
                    requirement(3, vec![1, 2], 40),
                ],
            )?,
        ],
        13,
    ));
    actions.push(assign(
        103,
        vec![milestone(
            104,
            w3,
            vec![requirement(1, vec![1, 2], 10), requirement(2, vec![33], 10)],
        )?],
        14,
    ));
    actions.push(assign(
        112,
        vec![
            milestone(113, w3, vec![requirement(1, vec![1, 2], 10)])?,
            milestone(114, w3, vec![requirement(1, vec![33], 10)])?,
        ],
        14,
    ));
    actions.push(assign(
        105,
        vec![milestone(106, w3, vec![requirement(1, vec![], 10)])?],
        15,
    ));
    actions.push(command(
        5,
        ProviderCommand::UpdateQualifications(UpdateQualificationsRequest {
            qualifications: Qualifications {
                role_ids: vec![3],
                skill_ids: vec![1, 2],
            },
        }),
        16,
    ));
    actions.push(command(
        7,
        ProviderCommand::UpdateQualifications(UpdateQualificationsRequest {
            qualifications: Qualifications {
                role_ids: vec![3],
                skill_ids: vec![2],
            },
        }),
        17,
    ));
    actions.push(assign(
        107,
        vec![milestone(108, w3, vec![requirement(1, vec![1, 2], 10)])?],
        18,
    ));
    for n in [2, 3] {
        actions.push(command(
            n,
            ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
                mode: WorkerMode::Worker,
            }),
            n,
        ));
    }
    for n in [11, 12] {
        actions.push(registration(n, vec![1], 600));
        actions.extend(promote(n));
    }
    for seed in 0..64 {
        actions.push(select(1, w1, seed));
    }
    for n in 13..=80 {
        actions.push(registration(
            n,
            if n == 80 { vec![1, 33] } else { vec![1] },
            600,
        ));
    }
    actions.push(assign(
        110,
        vec![milestone(111, w3, vec![requirement(1, vec![1, 33], 10)])?],
        19,
    ));
    Ok(actions)
}

fn project_fixture(
    project_id: EntityId,
    client: AccountId32,
    coordinator: AccountId32,
) -> ProjectCore {
    ProjectCore {
        project_id,
        created_at: None,
        client,
        coordinator,
        content: generated_contracts::provider_core::MetadataReferenceV1::Migration(
            PayloadHash::from_bytes([0; 32]),
        ),
        planning: PlanningView {
            revision: 0,
            status: PlanningStatus::Accepted,
            quote: None,
            escrow: Money::ZERO,
            frozen: false,
        },
        proposals: vec![],
        execution_escrow: Money::ZERO,
        cancelled: false,
        completed: false,
        evaluations: vec![],
        active_dispute_id: None,
    }
}
