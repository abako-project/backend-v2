//! Narrow text-to-commitment lowering after verified replay admission.
use crate::migration::source_hash;
use generated_contracts::{
    ProviderCommand, UnsignedContractCallV1,
    provider_core::{
        CatalogContentV1, CatalogMetadataRecordV1, MetadataCommitmentV1, ReasonMetadataRecordV1,
    },
};

/// Legacy labels/requests use the same record as API staging, without a cache.
#[must_use]
pub fn compact_catalog(call: &UnsignedContractCallV1) -> Option<ProviderCommand> {
    let record = CatalogMetadataRecordV1::from_command(
        call.provider_instance_id,
        call.origin,
        call.operation_id,
        &call.command,
    )?;
    let content = MetadataCommitmentV1 {
        operation_id: call.operation_id,
        hash: source_hash(&record.hash_input()),
    };
    match record.content {
        CatalogContentV1::Entry { kind, id, .. } => {
            Some(ProviderCommand::UpsertCatalogEntryCore { kind, id, content })
        }
        CatalogContentV1::Request(request) => Some(ProviderCommand::CreateSkillRequestCore {
            role_ids: request.role_ids,
            content,
        }),
        CatalogContentV1::RetainedRequest { .. } => None,
    }
}
/// Legacy lifecycle text becomes a fixed commitment, preserving original bytes.
#[must_use]
pub fn compact_reason(call: &UnsignedContractCallV1) -> Option<ProviderCommand> {
    let record = ReasonMetadataRecordV1::from_command(
        call.provider_instance_id,
        call.origin,
        call.operation_id,
        &call.command,
    )?;
    let content = MetadataCommitmentV1 {
        operation_id: call.operation_id,
        hash: source_hash(&record.hash_input()),
    };
    match call.command {
        ProviderCommand::CancelProject { project_id, .. } => {
            Some(ProviderCommand::CancelProjectCore {
                project_id,
                content,
            })
        }
        ProviderCommand::DisputePlanning { project_id, .. } => {
            Some(ProviderCommand::DisputePlanningCore {
                project_id,
                content,
            })
        }
        _ => None,
    }
}
