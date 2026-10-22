//! Frozen domain traces are independently replayed by the real WASM suite.
//! Signed admission and generated-ID bridging remain a separate M3 slice.
use super::super::*;
use generated_contracts::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
type Projection = (
    Vec<CatalogEntry>,
    Vec<CatalogEntry>,
    Vec<SkillRoleAssociation>,
    Vec<WorkerView>,
    Vec<SkillRequestView>,
);
fn account(n: u8) -> AccountId32 {
    AccountId32::from_bytes([n; 32])
}
fn captured_id(seed: u8) -> EntityId {
    use rand::{Rng, SeedableRng, rngs::StdRng};
    let mut bytes = [0; 16];
    StdRng::from_seed([seed; 32]).fill_bytes(&mut bytes);
    EntityId::from_bytes(bytes)
}
fn projection(engine: &RustEngine) -> Projection {
    (
        engine.roles.values().cloned().collect(),
        engine.skills.values().cloned().collect(),
        engine
            .skill_metadata
            .iter()
            .map(|(id, metadata)| SkillRoleAssociation {
                skill_id: *id,
                role_ids: metadata.role_ids.clone(),
            })
            .collect(),
        engine.workers.values().cloned().collect(),
        engine.skill_requests.values().cloned().collect(),
    )
}
fn digest(engine: &RustEngine) -> String {
    use parity_scale_codec::Encode;
    format!("{:x}", Sha256::digest(projection(engine).encode()))
}
fn apply(
    engine: &mut RustEngine,
    origin: AccountId32,
    command: &ProviderCommand,
    seed: u8,
) -> Value {
    let mut candidate = engine.clone();
    candidate.context.reset(&execution::ExecutionInputs {
        version: 1,
        now: UnixSeconds::new(100),
        seed: [seed; 32],
    });
    match command
        .validate()
        .map_err(Error::from)
        .and_then(|()| candidate.apply(origin, command, UnixSeconds::new(100)))
        .and_then(|effect| {
            candidate.validate()?;
            Ok(effect)
        }) {
        Ok(effect) => {
            *engine = candidate;
            json!({"effect": {"kind":effect.kind,"entityId":effect.entity_id,"recipients":effect.recipients}})
        }
        Err(error) => json!({"error":error.code()}),
    }
}
#[test]
fn canonical_directory_parity_trace() -> TestResult {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/directory-parity.json"))?;
    assert_eq!(fixture["schemaVersion"], 1);
    let mut engine = RustEngine::new(account(1))?;
    assert_eq!(json!(projection(&engine)), fixture["initial"]);
    for step in fixture["steps"].as_array().ok_or("missing steps")? {
        let command: ProviderCommand = serde_json::from_value(step["command"].clone())?;
        let origin = serde_json::from_value(step["origin"].clone())?;
        let seed = serde_json::from_value(step["seed"].clone())?;
        assert_eq!(json!(captured_id(seed)), step["capturedId"]);
        assert_eq!(
            apply(&mut engine, origin, &command, seed),
            step["outcome"],
            "{step}"
        );
        assert_eq!(digest(&engine), step["stateDigest"], "{step}");
    }
    assert_eq!(json!(projection(&engine)), fixture["final"]);
    Ok(())
}

/// Explicit fixture-authoring tool. Ordinary test runs only read the committed
/// trace. Set `DIRECTORY_TRACE_OUTPUT` to a temporary path and copy after review.
#[test]
#[ignore = "explicit trace authoring only"]
fn export_directory_trace() -> TestResult {
    let mut engine = RustEngine::new(account(1))?;
    let initial = json!(projection(&engine));
    let mut steps = Vec::new();
    for (origin, command, seed) in scenario()? {
        let outcome = apply(&mut engine, account(origin), &command, seed);
        steps.push(json!({"origin":account(origin),"command":command,"seed":seed,"capturedId":captured_id(seed),"outcome":outcome,"stateDigest":digest(&engine)}));
    }
    let fixture =
        json!({"schemaVersion":1,"initial":initial,"steps":steps,"final":projection(&engine)});
    std::fs::write(
        std::env::var("DIRECTORY_TRACE_OUTPUT")?,
        serde_json::to_vec_pretty(&fixture)?,
    )?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn scenario() -> TestResult<Vec<(u8, ProviderCommand, u8)>> {
    use CatalogKind::{Role, Skill};
    use SkillRequestDecision::{Approve, Reject};
    let id = captured_id;
    let qualifications = |role, skill| Qualifications {
        role_ids: vec![role],
        skill_ids: vec![skill],
    };
    let register = |role, skill| {
        ProviderCommand::RegisterWorker(RegisterWorkerRequest {
            display_name: "Candidate".into(),
            qualifications: qualifications(role, skill),
            calendar: CalendarDefinition {
                default_weekly_minutes: Minutes::new(600),
                overrides: vec![],
            },
        })
    };
    let upsert = |kind, id, name: &str| {
        ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
            kind,
            id,
            name: name.into(),
        })
    };
    let delete =
        |kind, id| ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest { kind, id });
    let request = |name: &str, role| {
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: name.into(),
            role_ids: vec![role],
        })
    };
    let decide = |seed, decision| {
        ProviderCommand::DecideSkillRequest(DecideSkillRequest {
            request_id: id(seed),
            decision,
        })
    };
    Ok(vec![
        (2, register(2, 999), 9),
        (2, register(2, 1), 10),
        (2, register(2, 999), 11),
        (
            4,
            ProviderCommand::UpdateQualifications(UpdateQualificationsRequest {
                qualifications: qualifications(2, 999),
            }),
            12,
        ),
        (
            4,
            ProviderCommand::UpdateQualifications(UpdateQualificationsRequest {
                qualifications: qualifications(2, 1),
            }),
            13,
        ),
        (
            2,
            ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
                mode: WorkerMode::Coordinator,
            }),
            14,
        ),
        (
            3,
            ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
                account: account(2),
            }),
            15,
        ),
        (
            1,
            ProviderCommand::PromoteCoordinator(PromoteCoordinatorRequest {
                account: account(2),
            }),
            16,
        ),
        (
            2,
            ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
                mode: WorkerMode::Coordinator,
            }),
            17,
        ),
        (3, register(3, 3), 18),
        (2, upsert(Skill, 80, "denied"), 19),
        (1, upsert(Role, 1, "renamed"), 20),
        (1, delete(Role, 1), 20),
        (1, delete(Role, 2), 20),
        (1, delete(Skill, 1), 20),
        (1, upsert(Skill, 80, " New\tSkill "), 20),
        (2, request("new  SKILL", 3), 20),
        (2, request("  DéCentral\tSpace  ", 3), 21),
        (3, request("décentral space", 3), 22),
        (2, request("RequestCollision", 3), 21),
        (3, decide(21, Approve), 22),
        (1, decide(21, Approve), 22),
        (1, decide(22, Approve), 22),
        (1, decide(22, Reject), 22),
        (1, decide(22, Reject), 22),
        (1, upsert(Skill, 81, "Renamed Approved Skill"), 22),
        (1, delete(Skill, 81), 22),
        (1, upsert(Role, 90, "temporary"), 22),
        (2, request("Pending Role", 90), 23),
        (1, delete(Role, 90), 23),
        (1, decide(23, Approve), 23),
        (1, decide(23, Reject), 23),
        (
            2,
            ProviderCommand::UpdateQualifications(UpdateQualificationsRequest {
                qualifications: qualifications(3, 2),
            }),
            23,
        ),
        (1, delete(Skill, 2), 23),
        (1, delete(Skill, 999), 23),
        (1, upsert(Skill, u32::MAX, "maximum catalog id"), 23),
        (2, request("Overflow Approval", 3), 24),
        (1, decide(24, Approve), 24),
        (1, decide(24, Reject), 24),
        (1, delete(Skill, u32::MAX), 24),
        (
            2,
            ProviderCommand::SetCalendar(CalendarDefinition {
                default_weekly_minutes: Minutes::new(37),
                overrides: vec![WeekOverride {
                    week: Week::new(2026, 53)?,
                    capacity: Minutes::ZERO,
                }],
            }),
            24,
        ),
        (
            2,
            ProviderCommand::SetWorkerMode(SetWorkerModeRequest {
                mode: WorkerMode::Worker,
            }),
            24,
        ),
        (1, upsert(Skill, 80, "Renamed Skill"), 24),
        (1, upsert(Skill, 82, "AΣ\u{a7ce}"), 24),
        (1, upsert(Skill, 83, "aσ\u{a7cf}"), 24),
        (1, upsert(Skill, 83, "\u{16ea0}"), 24),
        (1, upsert(Skill, 84, "\u{16ebb}"), 24),
        (1, upsert(Role, 91, "coordinator"), 24),
    ])
}
