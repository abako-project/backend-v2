//! Immutable lifecycle explanation staging; execution owns its timestamp/freeze.
use crate::state::{Error, hash};
use generated_contracts::{
    AccountId32, OperationId, PayloadHash, ProviderCommand, ProviderInstanceId,
    provider_core::{MetadataCommitmentV1, ReasonMetadataRecordV1},
};
use sqlx::PgConnection;
pub(crate) fn needs_record(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::CancelProject { .. } | ProviderCommand::DisputePlanning { .. }
    )
}
pub(crate) async fn insert(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    fingerprint: [u8; 32],
    payload: &str,
) -> Result<(), Error> {
    sqlx::query("INSERT INTO lifecycle_reason_documents(provider_instance_id,content_hash,payload) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(generation.to_string()).bind(fingerprint.as_slice()).bind(payload).execute(&mut *connection).await?;
    let stored:String=sqlx::query_scalar("SELECT payload FROM lifecycle_reason_documents WHERE provider_instance_id=$1 AND content_hash=$2").bind(generation.to_string()).bind(fingerprint.as_slice()).fetch_one(&mut *connection).await?;
    if stored != payload {
        return Err(Error::Dependency);
    }
    Ok(())
}
pub(crate) async fn capture(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    author: AccountId32,
    operation_id: OperationId,
    command: ProviderCommand,
) -> Result<ProviderCommand, Error> {
    if !needs_record(&command) {
        return Ok(command);
    }
    let (record, fingerprint, payload) = tokio::task::spawn_blocking(move || {
        let record =
            ReasonMetadataRecordV1::from_command(generation, author, operation_id, &command)
                .ok_or(Error::Internal)?;
        let fingerprint = hash(&record.hash_input());
        let payload = serde_json::to_string(&record)?;
        Ok::<_, Error>((record, fingerprint, payload))
    })
    .await??;
    insert(connection, generation, fingerprint, &payload).await?;
    let content = MetadataCommitmentV1 {
        operation_id,
        hash: PayloadHash::from_bytes(fingerprint),
    };
    match record.kind {
        generated_contracts::DomainEventKind::ProjectCancelled => {
            Ok(ProviderCommand::CancelProjectCore {
                project_id: record.project_id,
                content,
            })
        }
        generated_contracts::DomainEventKind::PlanningDisputed => {
            Ok(ProviderCommand::DisputePlanningCore {
                project_id: record.project_id,
                content,
            })
        }
        _ => Err(Error::Internal),
    }
}
