//! Ordinary Directory records must grow beyond one packed cell without changing
//! admission, financial facts, ordered lookup or restart behavior.
use generated_contracts::{
    provider_core::{CatalogEntryCore, CatalogReferenceV1, MetadataCommitmentV1},
    *,
};
use ink_sandbox::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use mock_provider::ReferenceState;
use provider_execution::{ExecutionInputs, VerifiedCall, migration::CoreArchiveV5};
use sp_core::{Pair, sr25519};

#[tokio::test]
async fn ordinary_catalog_index_growth_interior_mutation_and_restart_match_native() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed-v5.json"
    ))
    .unwrap();
    let mut archive: CoreArchiveV5 =
        serde_json::from_str(corpus["archives"][0]["archive"].as_str().unwrap()).unwrap();
    for id in 10_000..15_000 {
        assert!(
            archive
                .checkpoint
                .roles
                .insert(
                    id,
                    CatalogEntryCore {
                        id,
                        fixed: false,
                        content: CatalogReferenceV1::Migration(PayloadHash::from_bytes([99; 32])),
                    }
                )
                .is_none()
        );
    }
    assert!(archive.checkpoint.roles.len() * 4 > 16 * 1024);
    let root = archive.checkpoint.info.root_account;
    let mut reference = ReferenceState::from_archive(root, archive.clone()).unwrap();
    let key = (1..=10)
        .map(|seed| sr25519::Pair::from_seed(&[seed; 32]))
        .find(|key| AccountId32::from_bytes(key.public().0) == root)
        .unwrap();
    let configuration = WholeConfiguration {
        root,
        generation: archive.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    };
    let path = std::env::temp_dir().join(format!(
        "directory-ordinary-growth-{}.sqlite",
        std::process::id()
    ));
    assert!(!path.exists());
    let worker = Worker::import_whole(
        path.clone(),
        archive,
        configuration.clone(),
        Authority::Shadow,
    )
    .await
    .unwrap();
    let content = MetadataCommitmentV1 {
        operation_id: OperationId::from_bytes([230; 16]),
        hash: PayloadHash::from_bytes([98; 32]),
    };
    for (sequence, command) in [
        ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
            kind: CatalogKind::Role,
            id: 12_500,
        }),
        ProviderCommand::UpsertCatalogEntryCore {
            kind: CatalogKind::Role,
            id: 12_500,
            content,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let call = UnsignedContractCallV1::new(
            configuration.generation,
            OperationId::from_bytes([u8::try_from(sequence).unwrap() + 229; 16]),
            root,
            reference.nonce(root).nonce,
            UnixSeconds::new(1_900_000_000),
            command,
        )
        .unwrap();
        let signed = SignedContractCallV1 {
            signature: Sr25519Signature::from_bytes(key.sign(&call.signable_bytes().unwrap()).0),
            call,
        };
        let inputs = ExecutionInputs {
            version: 1,
            now: UnixSeconds::new(1_800_000_000),
            seed: [228; 32],
        };
        let (next, expected) = reference
            .execute(
                VerifiedCall::verify(signed.clone()).unwrap(),
                inputs.clone(),
            )
            .unwrap();
        assert_eq!(expected.outcome, ExecutionOutcome::Success);
        reference = next;
        let actual = worker
            .client()
            .execute_directory_with_inputs(signed, inputs)
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(actual, expected);
        let diagnostics = worker
            .client()
            .whole_diagnostics()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(diagnostics.comparison, "matched");
        assert_eq!(
            diagnostics.comparisons,
            i64::try_from(sequence).unwrap() + 1
        );
        let actual = worker
            .client()
            .export_archive()
            .unwrap()
            .result()
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(reference.archive().unwrap()).unwrap()
        );
    }
    worker.shutdown().await.unwrap();
    let worker = Worker::reopen_whole(path.clone(), configuration, Authority::Shadow)
        .await
        .unwrap();
    let diagnostics = worker
        .client()
        .whole_diagnostics()
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(diagnostics.comparison, "matched");
    assert_eq!(diagnostics.comparisons, 2);
    let actual = worker
        .client()
        .export_archive()
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(reference.archive().unwrap()).unwrap()
    );
    worker.shutdown().await.unwrap();
    std::fs::remove_file(path).unwrap();
}
