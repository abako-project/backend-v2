//! V4 task replacement preserves the complete non-task archive and shell.
#![cfg(feature = "storage-memory")]
use generated_contracts::{ProviderInstanceId, provider_core::TaskStateV1};
use mock_provider::{ReferenceState, migration::restore_tasks};
use provider_execution::migration::CoreArchiveV4;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn fixture() -> Result<(CoreArchiveV4, TaskStateV1), Box<dyn std::error::Error>> {
    let trace: serde_json::Value =
        serde_json::from_str(include_str!("../src/domain/fixtures/migration-signed.json"))?;
    let archive: CoreArchiveV4 =
        serde_json::from_str(trace["archives"][2]["archive"].as_str().ok_or("archive")?)?;
    let tasks =
        ReferenceState::from_archive(archive.checkpoint.info.root_account, archive.clone())?
            .task_state();
    Ok((archive, tasks))
}
#[test]
fn restores_exact_values_deleted_gaps_and_exhaustion_without_rewriting_history() -> TestResult {
    let (archive, mut tasks) = fixture()?;
    let root = archive.checkpoint.info.root_account;
    let original = serde_json::to_value(&archive)?;
    tasks.next_task_id = u32::MAX;
    let task = tasks
        .storages
        .iter_mut()
        .find_map(|r| r.storage.tasks.first_mut())
        .ok_or("task")?;
    task.task_id = 999;
    task.task.description = "λ\0".repeat(20_000);
    task.updated_at = generated_contracts::UnixSeconds::new(u64::MAX);
    let restored = restore_tasks(root, &archive, tasks.clone())?;
    assert_eq!(
        ReferenceState::from_archive(root, restored.clone())?.task_state(),
        tasks
    );
    let mut before = original;
    let mut after = serde_json::to_value(&restored)?;
    for value in [&mut before, &mut after] {
        value["checkpoint"]["next_task_id"] = serde_json::Value::Null;
        for project in value["checkpoint"]["projects"]
            .as_object_mut()
            .ok_or("projects")?
            .values_mut()
        {
            for proposal in project["proposals"].as_array_mut().ok_or("proposals")? {
                for milestone in proposal["milestones"].as_array_mut().ok_or("milestones")? {
                    milestone["taskStorage"]["tasks"] = serde_json::Value::Null;
                }
            }
        }
    }
    assert_eq!(before, after);
    Ok(())
}
#[test]
fn task_restore_rejects_wrong_generation_counter_and_context_instead_of_guessing() -> TestResult {
    let (archive, tasks) = fixture()?;
    let root = archive.checkpoint.info.root_account;
    let mut invalid = tasks.clone();
    invalid.generation = ProviderInstanceId::from_bytes([1; 16]);
    assert!(restore_tasks(root, &archive, invalid).is_err());
    let mut invalid = tasks.clone();
    invalid.next_task_id = 1;
    for r in &mut invalid.storages {
        r.storage.tasks.clear();
    }
    assert!(restore_tasks(root, &archive, invalid).is_err());
    let mut invalid = tasks.clone();
    invalid.storages.pop();
    assert!(restore_tasks(root, &archive, invalid).is_err());
    let mut invalid = tasks.clone();
    invalid.storages[0].proposal_revision += 1;
    assert!(restore_tasks(root, &archive, invalid).is_err());
    let mut invalid = tasks.clone();
    invalid.storages.push(tasks.storages[0].clone());
    assert!(restore_tasks(root, &archive, invalid).is_err());
    let mut invalid = tasks.clone();
    invalid
        .storages
        .iter_mut()
        .find_map(|r| r.storage.tasks.first_mut())
        .ok_or("task")?
        .task
        .title
        .clear();
    assert!(restore_tasks(root, &archive, invalid).is_err());
    assert_eq!(
        serde_json::to_vec(&restore_tasks(root, &archive, tasks)?)?,
        serde_json::to_vec(&archive)?
    );
    Ok(())
}
