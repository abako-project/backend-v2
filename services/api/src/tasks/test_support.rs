//! Shared task actors, state and queue helpers for API tests.
use super::*;
use parity_scale_codec::Encode;
use serde_json::Value;

pub(super) type CaseSlot = Arc<Mutex<Option<protocol::provider_core::CoreDisputeView>>>;

pub(super) struct Cohort {
    pub(super) app: Arc<App>,
    pub(super) internal: Server,
    pub(super) fake: Arc<Mutex<Fake>>,
    pub(super) owner: Session,
    pub(super) owner_cookie: String,
    pub(super) worker: Session,
    pub(super) worker_cookie: String,
    pub(super) client: Session,
    pub(super) client_cookie: String,
    pub(super) project: ProjectView,
    pub(super) definition: TaskDefinition,
}
impl Cohort {
    pub(super) async fn new(seed_tasks: bool) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_case(seed_tasks, None).await
    }
    pub(super) async fn with_case(
        seed_tasks: bool,
        case: Option<CaseSlot>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let fake = fake()?;
        let router = internal(fake.clone());
        let router = if let Some(case) = case {
            router.route(
                "/internal/disputes/{id}",
                get(move |Path(id): Path<String>| {
                    let case = case.clone();
                    async move {
                        let view = case.lock().await.clone().ok_or(Error::NotFound)?;
                        if view.dispute.dispute_id.to_string() != id {
                            return Err(Error::NotFound);
                        }
                        Ok::<_, Error>(Json(view))
                    }
                }),
            )
        } else {
            router
        };
        let internal = Server::start(router).await?;
        let app = app(&internal).await?;
        let (owner, owner_cookie) = register(&app, "executor_owner").await?;
        let (worker, worker_cookie) = register(&app, "executor_worker").await?;
        let (client, client_cookie) = register(&app, "executor_client").await?;
        let doc: Value = serde_json::from_str(include_str!("../../../../contracts/openapi.json"))?;
        let mut project: ProjectView = serde_json::from_value(
            doc["components"]["schemas"]["ProjectView"]["examples"][0].clone(),
        )?;
        project.coordinator = owner.view.account_id;
        project.client = client.view.account_id;
        project.proposals[0].status = ProposalStatus::Draft;
        let core =
            stage_project_without_tasks(&app, info().provider_instance_id, project.clone()).await?;
        let mut known = content::source_worker();
        known.account = worker.view.account_id;
        let known = stage_worker(&app, info().provider_instance_id, known).await?;
        fake.lock().await.snapshot.projects = vec![core];
        fake.lock().await.snapshot.workers = vec![known];
        if seed_tasks {
            stage_task_content(&app, info().provider_instance_id, project.clone()).await?;
            sqlx::query("UPDATE task_generations SET next_task_id=9")
                .execute(&app.db)
                .await?;
        }
        let mut definition: TaskDefinition = serde_json::from_value(
            doc["components"]["schemas"]["TaskDefinition"]["examples"][0].clone(),
        )?;
        definition.assignees = vec![worker.view.account_id];
        definition.description = "Task description".into();
        Ok(Self {
            app,
            internal,
            fake,
            owner,
            owner_cookie,
            worker,
            worker_cookie,
            client,
            client_cookie,
            project,
            definition,
        })
    }
    pub(super) fn create(&self) -> ProviderCommand {
        ProviderCommand::CreateTask {
            project_id: self.project.project_id,
            task_storage_id: self.storage(),
            task: self.definition.clone(),
        }
    }
    pub(super) fn storage(&self) -> EntityId {
        self.project.proposals[0].milestones[0]
            .task_storage
            .task_storage_id
    }
    pub(super) async fn queue(
        &self,
        actor: &Session,
        command: ProviderCommand,
    ) -> Result<OperationId, Error> {
        let id = OperationId::from_bytes(crate::state::random()?);
        let time = now()?;
        sqlx::query("INSERT INTO operations(operation_id,principal_id,wallet_id,account_id,command_json,command_bytes,request_command_bytes,status,next_attempt_at,expires_at,created_at,provider_instance_id,task_operation,task_project_id) VALUES($1,$2,$3,$4,$5,$6,$6,'Queued',0,$7,$8,$9,TRUE,$10)")
            .bind(id.to_string()).bind(actor.view.principal_id.to_string()).bind(actor.wallet.to_string()).bind(actor.view.account_id.to_string())
            .bind(serde_json::to_string(&command)?).bind(command.encode()).bind(time+300).bind(time).bind(info().provider_instance_id.to_string()).bind(command.project_id().map(|id| id.to_string())).execute(&self.app.db).await?;
        Ok(id)
    }
    pub(super) async fn step(&self) -> Result<(), Error> {
        let Some(operations::Work::Task(job)) = operations::claim_work(&self.app).await? else {
            return Err(Error::Internal);
        };
        crate::tasks::execution::process(&self.app, job).await
    }
    pub(super) async fn run(
        &self,
        actor: &Session,
        command: ProviderCommand,
    ) -> Result<OperationView, Error> {
        let id = self.queue(actor, command).await?;
        self.finish(actor, id).await
    }
    pub(super) async fn finish(
        &self,
        actor: &Session,
        id: OperationId,
    ) -> Result<OperationView, Error> {
        for _ in 0..16 {
            let view = operations::read(&self.app, actor, id).await?;
            if matches!(
                view.status,
                ProviderOperationStatus::Finalized
                    | ProviderOperationStatus::Rejected
                    | ProviderOperationStatus::Expired
            ) {
                return Ok(view);
            }
            step(&self.app).await?;
        }
        Err(Error::Internal)
    }
    pub(super) async fn tasks(&self) -> Result<Vec<TaskView>, Error> {
        let payloads: Vec<String> =
            sqlx::query_scalar("SELECT payload FROM task_items ORDER BY ordinal")
                .fetch_all(&self.app.db)
                .await?;
        payloads
            .into_iter()
            .map(|s| serde_json::from_str(&s).map_err(Error::from))
            .collect()
    }
    pub(super) async fn counter(&self) -> Result<i64, Error> {
        Ok(
            sqlx::query_scalar("SELECT next_task_id FROM task_generations")
                .fetch_one(&self.app.db)
                .await?,
        )
    }
}
pub(super) fn outcome(
    operation: &OperationView,
) -> Result<&ExecutionOutcome, Box<dyn std::error::Error>> {
    assert_eq!(operation.status, ProviderOperationStatus::Finalized);
    match (&operation.receipt, &operation.task_result) {
        (Some(receipt), None) => Ok(&receipt.outcome),
        (None, Some(result)) => Ok(&result.outcome),
        _ => Err("finalized task must have exactly one result source".into()),
    }
}

pub(super) async fn step(app: &App) -> Result<(), Error> {
    match operations::claim_work(app).await?.ok_or(Error::Internal)? {
        operations::Work::Task(job) => crate::tasks::execution::process(app, job).await,
        operations::Work::Provider(job) => operations::process(app, *job).await,
    }
}

// Transport fixtures supply successful core facts; the signed WASM tests
// independently exercise contract authorization, allocation and event behavior.
pub(in crate::tests) fn apply_fixture(
    fake: &mut Fake,
    call: &UnsignedContractCallV1,
    mut receipt: OperationReceipt,
) -> Result<OperationReceipt, Error> {
    use protocol::provider_core::{TaskAllocation, TaskCore};
    if receipt.outcome != ExecutionOutcome::Success {
        return Ok(receipt);
    }
    let (project_id, storage_id) = match &call.command {
        ProviderCommand::CreateTaskCore {
            project_id,
            task_storage_id,
            ..
        }
        | ProviderCommand::SetTaskAssigneesCore {
            project_id,
            task_storage_id,
            ..
        }
        | ProviderCommand::DeleteTask {
            project_id,
            task_storage_id,
            ..
        } => (*project_id, *task_storage_id),
        _ => return Ok(receipt),
    };
    let Some(storage) = fake
        .snapshot
        .projects
        .iter_mut()
        .find(|p| p.project_id == project_id)
        .and_then(|p| {
            p.proposals
                .iter_mut()
                .flat_map(|p| &mut p.milestones)
                .find(|m| m.task_storage.task_storage_id == storage_id)
        })
        .map(|m| &mut m.task_storage)
    else {
        return Err(Error::Dependency);
    };
    receipt.created_entity_id = Some(storage_id);
    match &call.command {
        ProviderCommand::CreateTaskCore { assignees, .. } => {
            let task_id = fake.next_task_id;
            fake.next_task_id = task_id.checked_add(1).ok_or(Error::Dependency)?;
            storage.tasks.push(TaskCore {
                task_id,
                assignees: assignees.clone(),
            });
            fake.task_allocations.insert(
                call.operation_id,
                TaskAllocation {
                    receipt: receipt.clone(),
                    project_id,
                    task_storage_id: storage_id,
                    task_id,
                },
            );
        }
        ProviderCommand::SetTaskAssigneesCore {
            task_id, assignees, ..
        } => {
            storage
                .tasks
                .iter_mut()
                .find(|t| t.task_id == *task_id)
                .ok_or(Error::Dependency)?
                .assignees
                .clone_from(assignees);
        }
        ProviderCommand::DeleteTask { task_id, .. } => {
            storage.tasks.retain(|t| t.task_id != *task_id);
        }
        _ => unreachable!(),
    }
    Ok(receipt)
}
