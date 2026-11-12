//! Immutable proposal documents selected by actual core facts, never staged views.
use crate::state::{App, Error, hash};
use protocol::{
    AccountId32, EntityId, OperationId, ProposalDefinition, ProposalView, ProviderCommand,
    ProviderInstanceId,
    provider_core::{
        MetadataCommitmentV1, ProposalContentReferenceV1, ProposalContentV1, ProposalCore,
        ProposalMetadataRecordV1, ProposalTerms,
    },
};
use sqlx::{PgConnection, Row};

pub(crate) fn needs_record(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::CreateProposal { .. }
            | ProviderCommand::UpdateProposal { .. }
            | ProviderCommand::RequestProposalChanges { .. }
    )
}
pub(crate) async fn insert(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    fingerprint: [u8; 32],
    payload: &str,
) -> Result<(), Error> {
    sqlx::query("INSERT INTO proposal_documents(provider_instance_id,content_hash,payload) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(generation.to_string()).bind(fingerprint.as_slice()).bind(payload).execute(&mut *connection).await?;
    let stored: String = sqlx::query_scalar(
        "SELECT payload FROM proposal_documents WHERE provider_instance_id=$1 AND content_hash=$2",
    )
    .bind(generation.to_string())
    .bind(fingerprint.as_slice())
    .fetch_one(&mut *connection)
    .await?;
    if stored != payload {
        return Err(Error::Dependency);
    }
    Ok(())
}
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
    let (record, fingerprint, payload) = tokio::task::spawn_blocking(move || {
        let record =
            ProposalMetadataRecordV1::from_command(generation, author, operation, &command)
                .ok_or(Error::Internal)?;
        let fingerprint = hash(&record.hash_input());
        let payload = serde_json::to_string(&record)?;
        Ok::<_, Error>((record, fingerprint, payload))
    })
    .await??;
    insert(connection, generation, fingerprint, &payload).await?;
    let content = MetadataCommitmentV1 {
        operation_id: operation,
        hash: protocol::PayloadHash::from_bytes(fingerprint),
    };
    let protocol::provider_core::ProposalProvenanceV1::Operation { proposal_id, .. } =
        record.provenance;
    match (record.content, proposal_id) {
        (ProposalContentV1::Definition(proposal), None) => {
            Ok(ProviderCommand::CreateProposalCore {
                project_id: record.project_id,
                proposal: ProposalTerms::from(&proposal),
                content,
            })
        }
        (ProposalContentV1::Definition(proposal), Some(proposal_id)) => {
            Ok(ProviderCommand::UpdateProposalCore {
                project_id: record.project_id,
                proposal_id,
                proposal: ProposalTerms::from(&proposal),
                content,
            })
        }
        (ProposalContentV1::ChangeRequest(_), Some(proposal_id)) => {
            Ok(ProviderCommand::RequestProposalChangesCore {
                project_id: record.project_id,
                proposal_id,
                content,
            })
        }
        _ => Err(Error::Internal),
    }
}
pub(crate) async fn record(
    app: &App,
    generation: ProviderInstanceId,
    project: EntityId,
    proposal: EntityId,
    author: AccountId32,
    reference: &ProposalContentReferenceV1,
) -> Result<ProposalMetadataRecordV1, Error> {
    let row = sqlx::query(
        "SELECT payload FROM proposal_documents WHERE provider_instance_id=$1 AND content_hash=$2",
    )
    .bind(generation.to_string())
    .bind(reference.hash().as_bytes().as_slice())
    .fetch_optional(&app.db)
    .await?
    .ok_or(Error::Dependency)?;
    let payload: String = row.try_get("payload")?;
    let reference = reference.clone();
    tokio::task::spawn_blocking(move || {
        let record: ProposalMetadataRecordV1 =
            serde_json::from_str(&payload).map_err(|_| Error::Dependency)?;
        if !record.matches_context(generation, project, proposal, author, &reference)
            || hash(&record.hash_input()) != reference.hash().into_bytes()
        {
            return Err(Error::Dependency);
        }
        Ok(record)
    })
    .await?
}
pub(crate) async fn definition(
    app: &App,
    generation: ProviderInstanceId,
    project: EntityId,
    coordinator: AccountId32,
    proposal: ProposalCore,
) -> Result<ProposalDefinition, Error> {
    let record = record(
        app,
        generation,
        project,
        proposal.proposal_id,
        coordinator,
        &proposal.content,
    )
    .await?;
    tokio::task::spawn_blocking(move || proposal.definition(&record).map_err(|_| Error::Dependency))
        .await?
}
pub(crate) async fn compose(
    app: &App,
    generation: ProviderInstanceId,
    project: EntityId,
    coordinator: AccountId32,
    client: AccountId32,
    proposal: ProposalCore,
    tasks: Vec<protocol::TaskStorageView>,
) -> Result<ProposalView, Error> {
    let selected = record(
        app,
        generation,
        project,
        proposal.proposal_id,
        coordinator,
        &proposal.content,
    )
    .await?;
    let (proposal, definition) = tokio::task::spawn_blocking(move || {
        let definition = proposal
            .definition(&selected)
            .map_err(|_| Error::Dependency)?;
        Ok::<_, Error>((proposal, definition))
    })
    .await??;
    let change = if let Some(reference) = &proposal.change_request {
        let record = record(
            app,
            generation,
            project,
            proposal.proposal_id,
            client,
            reference,
        )
        .await?;
        let ProposalContentV1::ChangeRequest(text) = record.content else {
            return Err(Error::Dependency);
        };
        Some(text)
    } else {
        None
    };
    tokio::task::spawn_blocking(move || {
        proposal
            .into_view(definition, change, tasks)
            .map_err(|_| Error::Dependency)
    })
    .await?
}
