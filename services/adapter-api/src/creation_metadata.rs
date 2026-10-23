//! R2a preparation. Capture exact immutable content in the enqueue transaction.
//! Commands remain predecessor-compatible until the paired core cutover; this
//! does not change which service is authoritative for current public reads.
use crate::state::{Error, hash};
use generated_contracts::{
    AccountId32, OperationId, ProviderCommand, ProviderInstanceId,
    provider_core::CreationMetadataRecordV1,
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
        .bind(&payload)
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
    Ok(command)
}
