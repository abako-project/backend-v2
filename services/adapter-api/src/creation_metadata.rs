//! Immutable creation content owned by the API and bound to core references.
use crate::state::{App, Error, hash};
use generated_contracts::{
    AccountId32, EntityId, OperationId, PayloadHash, ProviderCommand, ProviderInstanceId,
    provider_core::{
        CreationContentV1, CreationMetadataRecordV1, MetadataCommitmentV1, MetadataEntityV1,
        MetadataReferenceV1, RegisterWorkerCoreRequest,
    },
};
use sqlx::{PgConnection, Row};

pub(crate) fn needs_record(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::RegisterWorker(_) | ProviderCommand::CreateProject(_)
    )
}

/// Called after authorization and queue admission, before the operation insert.
/// Content and its queued request commit together or neither becomes durable.
pub(crate) async fn capture(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    author: AccountId32,
    operation: OperationId,
    command: ProviderCommand,
) -> Result<ProviderCommand, Error> {
    if !needs_record(&command) {
        return Ok(command);
    }
    let worker = matches!(command, ProviderCommand::RegisterWorker(_));
    let (command, fingerprint, payload) = tokio::task::spawn_blocking(move || {
        let record = CreationMetadataRecordV1::from_creation_command(
            generation, author, operation, &command,
        )
        .ok_or(Error::Internal)?;
        let fingerprint = hash(&record.hash_input());
        let payload = serde_json::to_string(&record)?;
        Ok::<_, Error>((command, fingerprint, payload))
    })
    .await??;
    insert(connection, generation, worker, fingerprint, &payload).await?;
    let content = MetadataCommitmentV1 {
        operation_id: operation,
        hash: PayloadHash::from_bytes(fingerprint),
    };
    match command {
        ProviderCommand::RegisterWorker(request) => Ok(ProviderCommand::RegisterWorkerCore(
            RegisterWorkerCoreRequest {
                content,
                qualifications: request.qualifications,
                calendar: request.calendar,
            },
        )),
        ProviderCommand::CreateProject(_) => Ok(ProviderCommand::CreateProjectCore(content)),
        _ => Err(Error::Internal),
    }
}

pub(crate) async fn insert(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    worker: bool,
    fingerprint: [u8; 32],
    payload: &str,
) -> Result<(), Error> {
    let (insert, select) = if worker {
        (
            "INSERT INTO worker_directory_metadata(provider_instance_id,content_hash,payload) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
            "SELECT payload FROM worker_directory_metadata WHERE provider_instance_id=$1 AND content_hash=$2",
        )
    } else {
        (
            "INSERT INTO project_creation_metadata(provider_instance_id,content_hash,payload) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
            "SELECT payload FROM project_creation_metadata WHERE provider_instance_id=$1 AND content_hash=$2",
        )
    };
    sqlx::query(insert)
        .bind(generation.to_string())
        .bind(fingerprint.as_slice())
        .bind(payload)
        .execute(&mut *connection)
        .await?;
    let row = sqlx::query(select)
        .bind(generation.to_string())
        .bind(fingerprint.as_slice())
        .fetch_one(&mut *connection)
        .await?;
    if row.try_get::<&str, _>("payload")? != payload {
        // An occupied digest never silently substitutes a different preimage.
        return Err(Error::Dependency);
    }
    Ok(())
}

pub(crate) async fn record(
    app: &App,
    generation: ProviderInstanceId,
    entity: MetadataEntityV1,
    owner: AccountId32,
    reference: &MetadataReferenceV1,
) -> Result<CreationMetadataRecordV1, Error> {
    let query = match entity {
        MetadataEntityV1::Worker(_) => {
            "SELECT payload FROM worker_directory_metadata WHERE provider_instance_id=$1 AND content_hash=$2"
        }
        MetadataEntityV1::Project(_) => {
            "SELECT payload FROM project_creation_metadata WHERE provider_instance_id=$1 AND content_hash=$2"
        }
    };
    let row = sqlx::query(query)
        .bind(generation.to_string())
        .bind(reference.hash().as_bytes().as_slice())
        .fetch_optional(&app.db)
        .await?
        .ok_or(Error::Dependency)?;
    let payload: String = row.try_get("payload")?;
    let reference = reference.clone();
    tokio::task::spawn_blocking(move || {
        let record: CreationMetadataRecordV1 =
            serde_json::from_str(&payload).map_err(|_| Error::Dependency)?;
        if !record.matches_context(generation, &entity, owner, &reference)
            || hash(&record.hash_input()) != reference.hash().into_bytes()
        {
            return Err(Error::Dependency);
        }
        Ok(record)
    })
    .await?
}

pub(crate) async fn worker_name(
    app: &App,
    generation: ProviderInstanceId,
    account: AccountId32,
    reference: &MetadataReferenceV1,
) -> Result<String, Error> {
    match record(
        app,
        generation,
        MetadataEntityV1::Worker(account),
        account,
        reference,
    )
    .await?
    .content
    {
        CreationContentV1::WorkerName { display_name } => Ok(display_name),
        CreationContentV1::ProjectText { .. } => Err(Error::Dependency),
    }
}

pub(crate) async fn project_text(
    app: &App,
    generation: ProviderInstanceId,
    project: EntityId,
    client: AccountId32,
    reference: &MetadataReferenceV1,
) -> Result<(String, String), Error> {
    match record(
        app,
        generation,
        MetadataEntityV1::Project(project),
        client,
        reference,
    )
    .await?
    .content
    {
        CreationContentV1::ProjectText { title, description } => Ok((title, description)),
        CreationContentV1::WorkerName { .. } => Err(Error::Dependency),
    }
}
