//! Narrow legacy input lowering, after signature/replay admission only.
use crate::content_hash;
use generated_contracts::{
    ProviderCommand, UnsignedContractCallV1,
    provider_core::{
        MetadataCommitmentV1, ProposalContentV1, ProposalMetadataRecordV1, ProposalTerms,
    },
};
/// Produce the same compact command as API staging, without caching preimages.
#[must_use]
pub fn compact_proposal(call: &UnsignedContractCallV1) -> Option<ProviderCommand> {
    let record = ProposalMetadataRecordV1::from_command(
        call.provider_instance_id,
        call.origin,
        call.operation_id,
        &call.command,
    )?;
    let content = MetadataCommitmentV1 {
        operation_id: call.operation_id,
        hash: content_hash(&record.hash_input()),
    };
    match (&call.command, record.content) {
        (
            ProviderCommand::CreateProposal { project_id, .. },
            ProposalContentV1::Definition(proposal),
        ) => Some(ProviderCommand::CreateProposalCore {
            project_id: *project_id,
            proposal: ProposalTerms::from(&proposal),
            content,
        }),
        (
            ProviderCommand::UpdateProposal {
                project_id,
                proposal_id,
                ..
            },
            ProposalContentV1::Definition(proposal),
        ) => Some(ProviderCommand::UpdateProposalCore {
            project_id: *project_id,
            proposal_id: *proposal_id,
            proposal: ProposalTerms::from(&proposal),
            content,
        }),
        (
            ProviderCommand::RequestProposalChanges {
                project_id,
                proposal_id,
                ..
            },
            ProposalContentV1::ChangeRequest(_),
        ) => Some(ProviderCommand::RequestProposalChangesCore {
            project_id: *project_id,
            proposal_id: *proposal_id,
            content,
        }),
        _ => None,
    }
}
