//! Migration admission must cover valid existing signed payloads.
use generated_contracts::*;
use ink_v5_compatibility::{
    GAS,
    worker::{DirectoryFixture, MarketplaceProjection, Worker},
};
use provider_execution::ExecutionInputs;
use sp_core::{Pair, sr25519};
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
#[test]
fn valid_large_worker_profile_matches_native_success() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            assert_eq!(
                ink_v5_compatibility::Schedule::get().limits.payload_len,
                16 * 1024
            );
            assert_eq!(
                ink_v5_compatibility::Schedule::get().limits.memory_pages,
                16
            );
            for length in [17 * 1024, MAX_SIGNABLE_BYTES - 200] {
                let trace: serde_json::Value = serde_json::from_str(include_str!(
                    "../../../services/mock-provider/src/domain/fixtures/financial-signed.json"
                ))
                .unwrap();
                let initial: MarketplaceProjection =
                    serde_json::from_value(trace["initial"][0].clone()).unwrap();
                let path = std::env::temp_dir()
                    .join(format!("ink-import-capacity-{}.sqlite", std::process::id()));
                let worker = Worker::start_directory_fixture(
                    path.clone(),
                    DirectoryFixture {
                        root: account(1),
                        funded_accounts: vec![account(2)],
                        catalog: (initial.0.roles, initial.0.skills, initial.0.skill_roles),
                        reservation_controller: account(4),
                        reference_controller: account(4),
                        generation: Some(ProviderInstanceId::from_bytes([109; 16])),
                        gas_limit: GAS,
                        projects: true,
                        money: Some(true),
                    },
                )
                .await
                .unwrap();
                let command = ProviderCommand::RegisterWorker(RegisterWorkerRequest {
                    display_name: "n".repeat(length),
                    qualifications: Qualifications {
                        role_ids: vec![],
                        skill_ids: vec![],
                    },
                    calendar: CalendarDefinition {
                        default_weekly_minutes: Minutes::new(600),
                        overrides: vec![],
                    },
                });
                let call = UnsignedContractCallV1::new(
                    ProviderInstanceId::from_bytes([109; 16]),
                    OperationId::from_bytes([1; 16]),
                    account(2),
                    0,
                    UnixSeconds::new(10_000),
                    command,
                )
                .unwrap();
                let ProviderCommand::RegisterWorkerCore(compact) =
                    mock_provider::creation::compact_creation(&call).unwrap()
                else {
                    panic!("expected compact registration");
                };
                let expected = generated_contracts::provider_core::MetadataReferenceV1::Creation(
                    compact.content,
                );
                let signature = Sr25519Signature::from_bytes(
                    sr25519::Pair::from_seed(&[2; 32])
                        .sign(&call.signable_bytes().unwrap())
                        .0,
                );
                let result = worker
                    .client()
                    .execute_directory_with_inputs(
                        SignedContractCallV1 { call, signature },
                        ExecutionInputs {
                            version: 1,
                            now: UnixSeconds::new(100),
                            seed: [1; 32],
                        },
                    )
                    .unwrap()
                    .result()
                    .await;
                let projection = worker
                    .client()
                    .directory_projection(account(1))
                    .unwrap()
                    .result()
                    .await;
                worker.shutdown().await.unwrap();
                std::fs::remove_file(path).unwrap();
                let projected = projection.unwrap();
                assert_eq!(projected.3.len(), 1);
                assert_eq!(projected.3[0].content, expected);
                assert!(parity_scale_codec::Encode::encoded_size(&projected.3[0]) < 16 * 1024);
                let result = result.unwrap();
                assert!(
                    matches!(result.outcome, ExecutionOutcome::Success),
                    "native accepts this valid profile: {:?}",
                    result.outcome
                );
            }
        });
}
