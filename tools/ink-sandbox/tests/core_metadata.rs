//! R2a creation commitments survive the proposal-only V2↔V3 transition.
use generated_contracts::{provider_core::*, *};
use ink_v5_compatibility::{
    WHOLE_GAS,
    worker::{Authority, WholeConfiguration, Worker},
};
use parity_scale_codec::Encode;
use provider_execution::{ExecutionInputs, migration::CoreArchiveV3};
use sp_core::{Pair, sr25519};
fn account(seed: u8) -> AccountId32 {
    AccountId32::from_bytes(sr25519::Pair::from_seed(&[seed; 32]).public().0)
}
fn signed(
    core: &CoreArchiveV3,
    actor: u8,
    op: u8,
    command: ProviderCommand,
) -> SignedContractCallV1 {
    let call = UnsignedContractCallV1::new(
        core.checkpoint.info.provider_instance_id,
        OperationId::from_bytes([op; 16]),
        account(actor),
        *core.checkpoint.nonces.get(&account(actor)).unwrap_or(&0),
        UnixSeconds::new(u64::MAX),
        command,
    )
    .unwrap();
    SignedContractCallV1 {
        signature: Sr25519Signature::from_bytes(
            sr25519::Pair::from_seed(&[actor; 32])
                .sign(&call.signable_bytes().unwrap())
                .0,
        ),
        call,
    }
}
fn config(core: &CoreArchiveV3) -> WholeConfiguration {
    WholeConfiguration {
        root: core.checkpoint.info.root_account,
        generation: core.checkpoint.info.provider_instance_id,
        mock_funding: true,
        gas_limit: WHOLE_GAS,
    }
}
#[test]
fn creation_refs_large_preimages_recovery_and_replay_survive_proposal_cutover() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
    let previous:serde_json::Value=serde_json::from_str(include_str!("../../../services/mock-provider/src/domain/fixtures/proposal-predecessor.json")).unwrap();
    let source=previous["archives"][1]["archive"].as_str().unwrap().as_bytes();
    let (initial,records)=mock_provider::migration::from_predecessor(source,account(1)).unwrap();
    let records=records.into_iter().map(|r|(mock_provider::migration::source_hash(&r.hash_input()),r)).collect();
    let path=std::env::temp_dir().join(format!("creation-v3-{}.sqlite",std::process::id()));
    let worker=Worker::import_whole(path.clone(),initial.clone(),config(&initial),Authority::Sandbox).await.unwrap();
    let mut current=initial.clone();let mut calls=Vec::new();let mut receipts=Vec::new();
    let name=format!("Compact worker\0{}","名".repeat(8192));
    let raw=[
        (8,241,ProviderCommand::RegisterWorker(RegisterWorkerRequest{display_name:name,qualifications:Qualifications{role_ids:vec![3],skill_ids:vec![1]},calendar:CalendarDefinition{default_weekly_minutes:Minutes::new(2400),overrides:vec![]}})),
        (9,242,ProviderCommand::CreateProject(CreateProjectRequest{title:"Immutable\0Project".into(),description:"p".repeat(200_000)})),
    ];
    for (actor,op,command) in raw {
        let legacy=signed(&current,actor,op,command);
        let preimage=CreationMetadataRecordV1::from_creation_command(legacy.call.provider_instance_id,legacy.call.origin,legacy.call.operation_id,&legacy.call.command).unwrap();
        let compact=mock_provider::creation::compact_creation(&legacy.call).unwrap();
        let call=signed(&current,actor,op,compact);
        let receipt=worker.client().execute_directory_with_inputs(call.clone(),ExecutionInputs{version:1,now:UnixSeconds::new(500),seed:[op;32]}).unwrap().result().await.unwrap();
        assert_eq!(receipt.outcome,ExecutionOutcome::Success);
        let hash=mock_provider::migration::source_hash(&preimage.hash_input());
        current=worker.client().export_archive().unwrap().result().await.unwrap();
        let reference=if actor==8 {
            let selected=&current.checkpoint.workers[&account(8)];assert!(selected.encoded_size()<16*1024);assert!(serde_json::to_value(selected).unwrap().get("displayName").is_none());selected.content.clone()
        } else {
            let selected=&current.checkpoint.projects[&receipt.created_entity_id.unwrap()];assert!(selected.encoded_size()<16*1024);assert!(serde_json::to_value(selected).unwrap().get("description").is_none());selected.content.clone()
        };
        assert_eq!(reference.hash(),hash);calls.push(call);receipts.push(receipt);
    }
    let legacy=ProviderCommand::RegisterWorker(RegisterWorkerRequest{display_name:"Recoverable legacy\0worker".into(),qualifications:Qualifications{role_ids:vec![3],skill_ids:vec![1]},calendar:CalendarDefinition{default_weekly_minutes:Minutes::new(600),overrides:vec![]}});
    for (op,expected) in [(243,ExecutionOutcome::Success),(244,ExecutionOutcome::Failed("worker_exists".into()))] {
        let call=signed(&current,10,op,legacy.clone());
        let receipt=worker.client().execute_directory_with_inputs(call.clone(),ExecutionInputs{version:1,now:UnixSeconds::new(501),seed:[op;32]}).unwrap().result().await.unwrap();assert_eq!(receipt.outcome,expected);
        calls.push(call);receipts.push(receipt);current=worker.client().export_archive().unwrap().result().await.unwrap();
    }
    let recovered=mock_provider::migration::recover_legacy_creations(&current).unwrap();
    assert!(recovered.iter().any(|r|matches!(r.provenance,MetadataProvenanceV1::Creation{operation_id,..} if operation_id==OperationId::from_bytes([243;16]))));
    assert!(!recovered.iter().any(|r|matches!(r.provenance,MetadataProvenanceV1::Creation{operation_id,..} if [241,242,244].iter().any(|v|operation_id==OperationId::from_bytes([*v;16])))));
    // V2 already owns creation refs: proposal rollback never needs their preimages.
    let old=mock_provider::migration::to_predecessor(current.clone(),&records).unwrap();
    assert_eq!(old.checkpoint.workers[&account(8)],current.checkpoint.workers[&account(8)]);
    for (id,project) in &current.checkpoint.projects {assert_eq!(old.checkpoint.projects[id].content,project.content);}
    assert_eq!(serde_json::to_vec(&old.checkpoint.receipts).unwrap(),serde_json::to_vec(&current.checkpoint.receipts).unwrap());
    let mut swapped=current.clone();let id=*swapped.checkpoint.projects.last_key_value().unwrap().0;
    swapped.checkpoint.projects.get_mut(&id).unwrap().content=current.checkpoint.workers[&account(8)].content.clone();
    assert!(mock_provider::ReferenceState::from_archive(account(1),swapped).is_err());
    worker.shutdown().await.unwrap();
    let reopened=Worker::reopen_whole(path.clone(),config(&current),Authority::Sandbox).await.unwrap();
    for (call,receipt) in calls.into_iter().zip(receipts) {assert_eq!(reopened.client().execute_directory_with_inputs(call,ExecutionInputs{version:99,now:UnixSeconds::new(u64::MAX),seed:[0;32]}).unwrap().result().await.unwrap(),receipt);}
    assert_eq!(serde_json::to_vec(&reopened.client().export_archive().unwrap().result().await.unwrap()).unwrap(),serde_json::to_vec(&current).unwrap());
    reopened.shutdown().await.unwrap();std::fs::remove_file(path).unwrap();
 });
}
