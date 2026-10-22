//! Frozen domain traces are independently replayed by the real WASM suite.
//! Domain and signed-operation fixtures also replay through the isolated VM.
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

fn signed_account(seed: u8) -> TestResult<AccountId32> {
    Ok(AccountId32::from_bytes(
        subxt_signer::sr25519::Keypair::from_secret_key([seed; 32])?
            .public_key()
            .0,
    ))
}
fn shell_digest(shell: &execution::Shell) -> TestResult<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(shell)?)))
}
fn signed_apply(
    state: &mut State,
    signed: SignedContractCallV1,
    inputs: execution::ExecutionInputs,
) -> Value {
    match execution::VerifiedCall::verify(signed)
        .map_err(Error::from)
        .and_then(|verified| state.clone().execute_with_inputs(verified, inputs))
    {
        Ok((after, receipt)) => {
            *state = after;
            json!({"receipt":receipt})
        }
        Err(error) => json!({"error":error.code()}),
    }
}
#[test]
fn canonical_signed_directory_trace() -> TestResult {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/directory-signed.json"))?;
    let root = serde_json::from_value(fixture["root"].clone())?;
    let mut state = State::new(root)?;
    state.shell.info.provider_instance_id = serde_json::from_value(fixture["generation"].clone())?;
    assert_eq!(json!(projection(&state.engine)), fixture["initial"]);
    for (index, step) in fixture["steps"]
        .as_array()
        .ok_or("steps")?
        .iter()
        .enumerate()
    {
        let call = serde_json::from_value(step["signed"].clone())?;
        let inputs = serde_json::from_value(step["inputs"].clone())?;
        assert_eq!(
            signed_apply(&mut state, call, inputs),
            step["outcome"],
            "step {index}"
        );
        assert_eq!(digest(&state.engine), step["stateDigest"], "step {index}");
        assert_eq!(
            shell_digest(&state.shell)?,
            step["shellDigest"],
            "step {index}"
        );
    }
    assert_eq!(json!(projection(&state.engine)), fixture["final"]);
    Ok(())
}
/// Freeze public calls signed by deterministic disposable keys. No private keys
/// or application data are exported; ordinary tests only replay the trace.
#[test]
#[ignore = "explicit signed trace authoring only"]
fn export_signed_directory_trace() -> TestResult {
    let root = signed_account(1)?;
    let generation = ProviderInstanceId::from_bytes([97; 16]);
    let mut state = State::new(root)?;
    state.shell.info.provider_instance_id = generation;
    let initial = json!(projection(&state.engine));
    let mut steps = Vec::new();
    let mut commands = scenario()?;
    // Force a calendar-ID collision after registration: native invariant failure
    // must abort without consuming a nonce or recording an accepted rejection.
    commands.push((4, commands[1].1.clone(), 10));
    for (index, (origin, mut command, seed)) in commands.into_iter().enumerate() {
        if let ProviderCommand::PromoteCoordinator(request) = &mut command {
            request.account = signed_account(2)?;
        }
        let origin_account = signed_account(origin)?;
        let call = UnsignedContractCallV1::new(
            generation,
            OperationId::from_bytes([u8::try_from(index + 1)?; 16]),
            origin_account,
            state.shell.nonce(origin_account),
            UnixSeconds::new(200),
            command,
        )?;
        let key = subxt_signer::sr25519::Keypair::from_secret_key([origin; 32])?;
        let signature = Sr25519Signature::from_bytes(key.sign(&call.signable_bytes()?).0);
        let signed = SignedContractCallV1 { call, signature };
        let inputs = execution::ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(100),
            seed: [seed; 32],
        };
        let outcome = signed_apply(&mut state, signed.clone(), inputs.clone());
        steps.push(json!({"signed":signed,"inputs":inputs,"outcome":outcome,
            "stateDigest":digest(&state.engine),"shellDigest":shell_digest(&state.shell)?}));
    }
    let accounts = (1..=4)
        .map(signed_account)
        .collect::<TestResult<Vec<_>>>()?;
    let fixture = json!({"schemaVersion":1,"root":root,"generation":generation,"accounts":accounts,
        "initial":initial,"steps":steps,"final":projection(&state.engine)});
    std::fs::write(
        std::env::var("DIRECTORY_SIGNED_TRACE_OUTPUT")?,
        serde_json::to_vec_pretty(&fixture)?,
    )?;
    Ok(())
}
