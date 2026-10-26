//! Paused owner handoff against retained V4 facts and real `PostgreSQL`.
use super::*;
use generated_contracts::provider_core::*;

struct Source {
    compact: bool,
    snapshot: CoreSnapshotV4,
    tasks: TaskStateV1,
    cases: BTreeMap<EntityId, CoreDisputeViewV4>,
}
impl Source {
    fn fixture() -> Result<Self, Box<dyn std::error::Error>> {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../mock-provider/src/domain/fixtures/migration-signed.json"
        ))?;
        let archive = corpus["archives"]
            .as_array()
            .ok_or("archives")?
            .iter()
            .rev()
            .map(
                |entry| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
                    Ok(serde_json::from_str(
                        entry["archive"].as_str().ok_or("archive JSON")?,
                    )?)
                },
            )
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|value| {
                value["checkpoint"]["disputes"]
                    .as_object()
                    .is_some_and(|cases| !cases.is_empty())
            })
            .ok_or("case fixture")?;
        let checkpoint = &archive["checkpoint"];
        let projects: BTreeMap<EntityId, ProjectCoreV4> =
            serde_json::from_value(checkpoint["projects"].clone())?;
        let info: ProviderInfo = serde_json::from_value(checkpoint["info"].clone())?;
        let mut tasks = TaskStateV1 {
            schema_version: 1,
            generation: info.provider_instance_id,
            next_task_id: serde_json::from_value(checkpoint["next_task_id"].clone())?,
            storages: vec![],
        };
        for project in projects.values() {
            for proposal in &project.proposals {
                for milestone in &proposal.milestones {
                    tasks.storages.push(TaskStorageRecordV1 {
                        project_id: project.project_id,
                        proposal_id: proposal.proposal_id,
                        proposal_revision: proposal.revision,
                        storage: milestone.task_storage.clone(),
                    });
                }
            }
        }
        tasks.storages.sort_by_key(|r| r.storage.task_storage_id);
        let disputes: BTreeMap<EntityId, Dispute> =
            serde_json::from_value(checkpoint["disputes"].clone())?;
        let cases = disputes
            .into_iter()
            .map(|(id, dispute)| -> Result<_, Box<dyn std::error::Error>> {
                let milestone = projects[&dispute.project_id]
                    .proposals
                    .iter()
                    .flat_map(|p| &p.milestones)
                    .find(|m| m.milestone_id == dispute.milestone_id)
                    .ok_or("case milestone")?
                    .clone();
                Ok((id, CoreDisputeViewV4 { dispute, milestone }))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            compact: false,
            snapshot: CoreSnapshotV4 {
                schema_version: 4,
                info,
                catalog: fake()?.try_lock()?.snapshot.catalog.clone(),
                workers: vec![],
                projects: projects.into_values().collect(),
                balances: vec![],
            },
            tasks,
            cases,
        })
    }
    fn core(&self) -> CoreSnapshotV5 {
        CoreSnapshotV5 {
            schema_version: 5,
            info: self.snapshot.info.clone(),
            catalog: self.snapshot.catalog.clone(),
            workers: self.snapshot.workers.clone(),
            projects: self
                .snapshot
                .projects
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
            balances: self.snapshot.balances.clone(),
        }
    }
    fn task_core(&self) -> TaskStateV2 {
        TaskStateV2 {
            schema_version: 2,
            generation: self.tasks.generation,
            next_task_id: self.tasks.next_task_id,
            has_task_history: true,
            storages: self
                .tasks
                .storages
                .iter()
                .map(|r| TaskStorageRecordV2 {
                    project_id: r.project_id,
                    proposal_id: r.proposal_id,
                    proposal_revision: r.proposal_revision,
                    storage: TaskStorageCore::from(&r.storage),
                })
                .collect(),
        }
    }
}

async fn source(state: Arc<Mutex<Source>>) -> Result<Server, Error> {
    Server::start(
        Router::new()
            .route(
                "/internal/info",
                get(|State(s): State<Arc<Mutex<Source>>>| async move {
                    Json(s.lock().await.snapshot.info.clone())
                }),
            )
            .route(
                "/internal/snapshot",
                get(|State(s): State<Arc<Mutex<Source>>>| async move {
                    let s = s.lock().await;
                    Ok::<_, Error>(Json(if s.compact {
                        serde_json::to_value(s.core())?
                    } else {
                        serde_json::to_value(&s.snapshot)?
                    }))
                }),
            )
            .route(
                "/internal/task-state",
                get(|State(s): State<Arc<Mutex<Source>>>| async move {
                    let s = s.lock().await;
                    Ok::<_, Error>(Json(if s.compact {
                        serde_json::to_value(s.task_core())?
                    } else {
                        serde_json::to_value(&s.tasks)?
                    }))
                }),
            )
            .route(
                "/internal/disputes/{id}",
                get(
                    |State(s): State<Arc<Mutex<Source>>>, Path(id): Path<String>| async move {
                        let s = s.lock().await;
                        let case = s.cases.get(&parse(&id)?).cloned().ok_or(Error::NotFound)?;
                        Ok::<_, Error>(Json(if s.compact {
                            serde_json::to_value(CoreDisputeView {
                                dispute: case.dispute,
                                milestone: case.milestone.into(),
                            })?
                        } else {
                            serde_json::to_value(case)?
                        }))
                    },
                ),
            )
            .with_state(state),
    )
    .await
}

#[tokio::test]
async fn historical_case_cutover_and_rollback_preserve_full_tasks_and_actual_counter() -> TestResult
{
    let state = Arc::new(Mutex::new(Source::fixture()?));
    let server = source(state.clone()).await?;
    let app = app(&server).await?;
    crate::task_state::prepare_cutover(&app).await?;
    crate::task_state::prepare_cutover(&app).await?;
    let generation = state.lock().await.tasks.generation;
    assert!(!crate::task_views::owned(&app, generation).await?);
    state.lock().await.compact = true;
    let missing = {
        let mut state = state.lock().await;
        let project = state
            .snapshot
            .projects
            .iter_mut()
            .find(|p| p.active_dispute_id.is_some())
            .ok_or("active case")?;
        (project.project_id, project.active_dispute_id.take())
    };
    assert!(crate::task_state::activate(&app).await.is_err());
    assert!(!crate::task_views::owned(&app, generation).await?);
    state
        .lock()
        .await
        .snapshot
        .projects
        .iter_mut()
        .find(|p| p.project_id == missing.0)
        .ok_or("project")?
        .active_dispute_id = missing.1;
    crate::task_state::activate(&app).await?;
    crate::task_state::activate(&app).await?;
    assert!(crate::task_views::owned(&app, generation).await?);
    let (proposal, case, expected) = {
        let state = state.lock().await;
        let case = state.cases.values().next().ok_or("case")?.clone();
        let proposal = state
            .tasks
            .storages
            .iter()
            .find(|r| r.storage.milestone_id == case.dispute.milestone_id)
            .ok_or("context")?
            .proposal_id;
        let expected = case.milestone.task_storage.clone();
        (
            proposal,
            CoreDisputeView {
                dispute: case.dispute,
                milestone: case.milestone.into(),
            },
            expected,
        )
    };
    assert_eq!(
        crate::task_freezes::compose_case(&app, generation, proposal, case.clone()).await?,
        expected
    );
    // Later responses do not change the immutable opening fingerprint.
    let mut answered = case.clone();
    answered.dispute.response = None;
    assert_eq!(
        crate::task_freezes::compose_case(&app, generation, proposal, answered).await?,
        expected
    );
    let mut unknown = case;
    unknown.dispute.dispute_id = EntityId::from_bytes([244; 16]);
    assert!(
        crate::task_freezes::compose_case(&app, generation, proposal, unknown)
            .await
            .is_err()
    );
    // A direct core create/delete can leave a gap without changing the selected
    // graph. Export must use the actual core counter, not the API cache.
    state.lock().await.tasks.next_task_id += 5;
    let path = std::env::temp_dir().join(format!(
        "task-migration-{}.json",
        OperationId::from_bytes(crate::state::random()?)
    ));
    crate::task_rollback::export(&app, generation, path.clone()).await?;
    let saved: TaskStateV1 = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(saved.next_task_id, state.lock().await.tasks.next_task_id);
    state.lock().await.compact = false;
    state
        .lock()
        .await
        .snapshot
        .projects
        .iter_mut()
        .find(|p| p.project_id == missing.0)
        .ok_or("project")?
        .active_dispute_id = None;
    assert!(
        crate::task_rollback::complete(&app, path.clone())
            .await
            .is_err()
    );
    assert!(crate::task_views::owned(&app, generation).await?);
    state
        .lock()
        .await
        .snapshot
        .projects
        .iter_mut()
        .find(|p| p.project_id == missing.0)
        .ok_or("project")?
        .active_dispute_id = missing.1;
    crate::task_rollback::complete(&app, path.clone()).await?;
    crate::task_rollback::complete(&app, path.clone()).await?;
    assert!(!crate::task_views::owned(&app, generation).await?);
    std::fs::remove_file(path)?;
    server.finish().await?;
    Ok(())
}

#[tokio::test]
async fn historical_case_preparation_rejects_changed_opening_and_missing_provenance() -> TestResult
{
    let state = Arc::new(Mutex::new(Source::fixture()?));
    let server = source(state.clone()).await?;
    let app = app(&server).await?;
    crate::task_state::prepare_cutover(&app).await?;
    let before: Vec<String> =
        sqlx::query_scalar("SELECT payload FROM task_migration_cases ORDER BY case_id")
            .fetch_all(&app.db)
            .await?;
    {
        let mut state = state.lock().await;
        let case = state.cases.values_mut().next().ok_or("case")?;
        case.dispute.opened_by = AccountId32::from_bytes([243; 32]);
    }
    assert!(crate::task_state::prepare_cutover(&app).await.is_err());
    let after: Vec<String> =
        sqlx::query_scalar("SELECT payload FROM task_migration_cases ORDER BY case_id")
            .fetch_all(&app.db)
            .await?;
    assert_eq!(before, after);
    // Losing provenance never synthesizes historical content from current tasks.
    sqlx::query("DELETE FROM task_migration_cases")
        .execute(&app.db)
        .await?;
    state.lock().await.compact = true;
    assert!(crate::task_state::activate(&app).await.is_err());
    assert!(!crate::task_views::owned(&app, state.lock().await.tasks.generation).await?);
    server.finish().await?;
    Ok(())
}

#[path = "task_rollback_tests.rs"]
mod rollback;
