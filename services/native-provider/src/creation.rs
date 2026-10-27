//! Bounded compatibility at the verified-call boundary, not a legacy engine.
use protocol::{
    PayloadHash, ProviderCommand, UnsignedContractCallV1,
    provider_core::{CreationMetadataRecordV1, MetadataCommitmentV1, RegisterWorkerCoreRequest},
};
use sha2::{Digest, Sha256};

/// Lower the two predecessor creation commands after replay admission. Callers
/// retain the original envelope/RecordedCall bytes and execute one core handler.
/// No metadata preimage is stored in provider business state.
#[must_use]
pub fn compact_creation(call: &UnsignedContractCallV1) -> Option<ProviderCommand> {
    let record = CreationMetadataRecordV1::from_creation_command(
        call.provider_instance_id,
        call.origin,
        call.operation_id,
        &call.command,
    )?;
    let content = MetadataCommitmentV1 {
        operation_id: call.operation_id,
        hash: PayloadHash::from_bytes(Sha256::digest(record.hash_input()).into()),
    };
    match &call.command {
        ProviderCommand::RegisterWorker(request) => Some(ProviderCommand::RegisterWorkerCore(
            RegisterWorkerCoreRequest {
                content,
                qualifications: request.qualifications.clone(),
                calendar: request.calendar.clone(),
            },
        )),
        ProviderCommand::CreateProject(_) => Some(ProviderCommand::CreateProjectCore(content)),
        _ => None,
    }
}
