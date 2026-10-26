//! Opt-in engine timing; transport polling is deliberately excluded.
use generated_contracts::*;
use ink_sandbox::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use mock_provider::ReferenceState;
use provider_execution::{
    ExecutionInputs, VerifiedCall,
    migration::{CoreArchiveV4, CoreArchiveV5},
};
use serde_json::Value;
use sp_core::{Pair, sr25519};
use std::time::Instant;
fn signed(
    state: &ReferenceState,
    origin: u8,
    serial: u32,
    command: ProviderCommand,
) -> SignedContractCallV1 {
    let key = sr25519::Pair::from_seed(&[origin; 32]);
    let origin = AccountId32::from_bytes(key.public().0);
    let mut operation = [249; 16];
    operation[..4].copy_from_slice(&serial.to_le_bytes());
    let call = UnsignedContractCallV1::new(
        state.info().provider_instance_id,
        OperationId::from_bytes(operation),
        origin,
        state.nonce(origin).nonce,
        UnixSeconds::new(10_000),
        command,
    )
    .unwrap();
    SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes().unwrap()).0),
        call,
    }
}
fn inputs(serial: u8) -> ExecutionInputs {
    ExecutionInputs {
        version: 1,
        now: UnixSeconds::new(500),
        seed: [serial; 32],
    }
}
fn prepare(
    mut state: ReferenceState,
    origin: u8,
    serial: u8,
    command: ProviderCommand,
) -> (ReferenceState, OperationReceipt) {
    let call = signed(&state, origin, u32::from(serial), command);
    let (next, receipt) = state
        .execute(VerifiedCall::verify(call).unwrap(), inputs(serial))
        .unwrap();
    state = next;
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    (state, receipt)
}
fn fixture() -> (CoreArchiveV5, SignedContractCallV1) {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed.json"
    ))
    .unwrap();
    let archive: CoreArchiveV4 = serde_json::from_str(
        fixture["archives"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["beforeStep"] == 35)
            .unwrap()["archive"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let archive = CoreArchiveV5::from_v4(archive).unwrap().0;
    let root = archive.checkpoint.info.root_account;
    assert_eq!(
        root,
        AccountId32::from_bytes(sr25519::Pair::from_seed(&[1; 32]).public().0)
    );
    let state = ReferenceState::from_archive(root, archive).unwrap();
    let client = AccountId32::from_bytes(sr25519::Pair::from_seed(&[4; 32]).public().0);
    let (state, _) = prepare(
        state,
        1,
        200,
        ProviderCommand::FundAccount(FundAccountRequest {
            account: client,
            amount: Money::new(10_000),
        }),
    );
    let (state, project) = prepare(
        state,
        4,
        201,
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Four milestone timing".into(),
            description: "Fixed current-app engine benchmark".into(),
        }),
    );
    let project_id = project.created_entity_id.unwrap();
    let trace: Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
    ))
    .unwrap();
    let mut proposal: ProposalDefinition = serde_json::from_value(
        trace["steps"][28]["signed"]["call"]["command"]["data"]["proposal"].clone(),
    )
    .unwrap();
    let template = proposal.milestones[0].clone();
    proposal.milestones = (1..=4)
        .map(|index| {
            let mut milestone = template.clone();
            milestone.key = u32::from(index);
            let week = Week::new(2026, index).unwrap();
            milestone.window = WeekWindow::new(week, week).unwrap();
            milestone
        })
        .collect();
    let (state, receipt) = prepare(
        state,
        2,
        202,
        ProviderCommand::CreateProposal {
            project_id,
            proposal,
        },
    );
    let proposal_id = receipt.created_entity_id.unwrap();
    let (state, _) = prepare(
        state,
        2,
        203,
        ProviderCommand::SubmitProposal {
            project_id,
            proposal_id,
        },
    );
    let (state, _) = prepare(
        state,
        4,
        204,
        ProviderCommand::AcceptPlanningDelivery {
            project_id,
            expected_revision: 1,
        },
    );
    let approval = signed(
        &state,
        4,
        205,
        ProviderCommand::ApproveExecution {
            project_id,
            proposal_id,
            expected_revision: 1,
        },
    );
    let archive = state.archive().unwrap();
    let (_, receipt) = state
        .execute(VerifiedCall::verify(approval.clone()).unwrap(), inputs(205))
        .unwrap();
    assert_eq!(receipt.outcome, ExecutionOutcome::Success);
    (archive, approval)
}
#[test]
fn four_milestone_timing_fixture_is_native_admitted() {
    fixture();
}
#[test]
#[ignore = "Run explicitly on a controlled idle host; timings are not a normal test gate"]
fn whole_provider_engine_timings() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (initial, approval) = fixture();
        let root = initial.checkpoint.info.root_account;
        let configuration = WholeConfiguration { root, generation: initial.checkpoint.info.provider_instance_id, mock_funding: true, gas_limit: WHOLE_GAS };
        for authority in [Authority::Rust, Authority::Shadow, Authority::Sandbox] {
            let path = std::env::temp_dir().join(format!("ink-engine-timing-{}-{authority:?}.sqlite",std::process::id()));
            assert!(!path.exists());
            let start = Instant::now();
            let worker = Worker::import_whole(path.clone(),initial.clone(),configuration.clone(),authority).await.unwrap();
            let fresh_ms = start.elapsed().as_secs_f64()*1000.0;
            worker.shutdown().await.unwrap();
            let start = Instant::now();
            let worker = Worker::reopen_whole(path.clone(),configuration.clone(),authority).await.unwrap();
            let reopen_ms = start.elapsed().as_secs_f64()*1000.0;
            let client = worker.client();
            let mut state = ReferenceState::from_archive(root,initial.clone()).unwrap();
            let mut ordinary = Vec::new();
            for index in 0..32u8 {
                let call = signed(&state,2,1000+u32::from(index),ProviderCommand::SetWorkerMode(SetWorkerModeRequest { mode: if index % 2 == 0 { WorkerMode::Worker } else { WorkerMode::Coordinator } }));
                let captured = inputs(index);
                let (next, expected) = state.execute(VerifiedCall::verify(call.clone()).unwrap(),captured.clone()).unwrap(); state = next;
                let start = Instant::now();
                let receipt = client.execute_directory_with_inputs(call,captured).unwrap().result().await.unwrap();
                let elapsed = start.elapsed().as_secs_f64()*1000.0;
                assert_eq!(receipt,expected);
                if index >= 2 { ordinary.push(elapsed); }
            }
            let start = Instant::now();
            let receipt = client.execute_directory_with_inputs(approval.clone(),inputs(205)).unwrap().result().await.unwrap();
            let approval_ms = start.elapsed().as_secs_f64()*1000.0;
            assert_eq!(receipt.outcome, ExecutionOutcome::Success);
            let diagnostics = client.whole_diagnostics().unwrap().result().await.unwrap();
            assert_eq!(diagnostics.comparison,if authority == Authority::Rust {"disabled"} else {"matched"});
            ordinary.sort_by(f64::total_cmp);
            println!("{authority:?}: fresh_import_ms={fresh_ms:.2} reopen_ms={reopen_ms:.2} ordinary_n={} ordinary_p95_ms={:.2} four_milestone_approval_ms={approval_ms:.2}",ordinary.len(),ordinary[(ordinary.len()*95).div_ceil(100)-1]);
            worker.shutdown().await.unwrap(); std::fs::remove_file(path).unwrap();
        }
    });
}
