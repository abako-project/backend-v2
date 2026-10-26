//! References must select real accepted writes; fresh catalog seeds are explicit sources.
use super::State;
use crate::{Error, Result, require};
use generated_contracts::{
    CatalogKind, DomainEventKind, ExecutionOutcome, UnsignedContractCallV1,
    provider_core::{
        CatalogReferenceV1, MetadataCommitmentV1, ReasonReferenceV1, SkillRequestCore,
        catalog_seed_v1,
    },
};
impl State {
    fn catalog_call(&self, expected: &MetadataCommitmentV1) -> Result<UnsignedContractCallV1> {
        let record = self
            .shell
            .receipts
            .get(&expected.operation_id)
            .ok_or_else(|| Error::domain("invalid_catalog_metadata_state"))?;
        require(
            record.receipt.outcome == ExecutionOutcome::Success,
            "invalid_catalog_metadata_state",
        )?;
        UnsignedContractCallV1::decode_signable(&record.bytes).map_err(Error::from)
    }
    pub(super) fn validate_catalog_history(&self) -> Result<()> {
        for request in self.engine.skill_requests.values() {
            self.validate_request_ref(request)?;
        }
        for entry in self.engine.roles.values() {
            self.validate_catalog_ref(CatalogKind::Role, entry.id, &entry.content, None)?;
        }
        for entry in self.engine.skills.values() {
            let metadata = self
                .engine
                .skill_metadata
                .get(&entry.id)
                .ok_or_else(|| Error::domain("invalid_catalog_metadata_state"))?;
            self.validate_catalog_ref(
                CatalogKind::Skill,
                entry.id,
                &metadata.origin,
                Some(&metadata.role_ids),
            )?;
            if entry.content != metadata.origin {
                self.validate_catalog_ref(CatalogKind::Skill, entry.id, &entry.content, None)?;
            }
        }
        Ok(())
    }
    fn validate_request_ref(&self, request: &SkillRequestCore) -> Result<()> {
        let CatalogReferenceV1::Write(expected) = &request.content else {
            return Ok(());
        };
        let call = self.catalog_call(expected)?;
        let recorded = &self.shell.receipts[&expected.operation_id].receipt;
        require(
            call.origin == request.requester
                && recorded.created_entity_id == Some(request.request_id),
            "invalid_catalog_metadata_state",
        )?;
        let lowered = crate::catalog_content::compact_catalog(&call);
        match lowered.as_ref().unwrap_or(&call.command) {
            generated_contracts::ProviderCommand::CreateSkillRequestCore { role_ids, content } => {
                require(
                    role_ids == &request.role_ids && content == expected,
                    "invalid_catalog_metadata_state",
                )
            }
            _ => Err(Error::domain("invalid_catalog_metadata_state")),
        }
    }
    fn validate_catalog_ref(
        &self,
        kind: CatalogKind,
        id: u32,
        reference: &CatalogReferenceV1,
        origin_roles: Option<&[u32]>,
    ) -> Result<()> {
        match reference {
            CatalogReferenceV1::Seed(expected) => {
                let seed = catalog_seed_v1()
                    .into_iter()
                    .find(|s| s.kind == kind && s.id == id)
                    .ok_or_else(|| Error::domain("invalid_catalog_metadata_state"))?;
                require(
                    crate::content_hash(
                        &seed.record(self.info().provider_instance_id).hash_input(),
                    ) == *expected
                        && origin_roles.is_none_or(|roles| roles == seed.role_ids),
                    "invalid_catalog_metadata_state",
                )
            }
            CatalogReferenceV1::Write(expected) => {
                let call = self.catalog_call(expected)?;
                let lowered = crate::catalog_content::compact_catalog(&call);
                match lowered.as_ref().unwrap_or(&call.command) {
                    generated_contracts::ProviderCommand::UpsertCatalogEntryCore {
                        kind: written,
                        id: target,
                        content,
                    } => require(
                        call.origin == self.info().root_account
                            && *written == kind
                            && *target == id
                            && content == expected
                            && origin_roles.is_none_or(<[u32]>::is_empty),
                        "invalid_catalog_metadata_state",
                    ),
                    generated_contracts::ProviderCommand::CreateSkillRequestCore {
                        content,
                        ..
                    } if kind == CatalogKind::Skill => {
                        let request_id = self.shell.receipts[&expected.operation_id]
                            .receipt
                            .created_entity_id
                            .ok_or_else(|| Error::domain("invalid_catalog_metadata_state"))?;
                        let request = self
                            .engine
                            .skill_requests
                            .get(&request_id)
                            .ok_or_else(|| Error::domain("invalid_catalog_metadata_state"))?;
                        require(
                            content == expected
                                && request.requester == call.origin
                                && request.content == *reference
                                && request.status
                                    == generated_contracts::SkillRequestStatus::Approved
                                && request.skill_id == Some(id)
                                && origin_roles.is_none_or(|roles| roles == request.role_ids),
                            "invalid_catalog_metadata_state",
                        )
                    }
                    _ => Err(Error::domain("invalid_catalog_metadata_state")),
                }
            }
        }
    }
    pub(super) fn validate_reason_history(&self) -> Result<()> {
        let mut cursor = 0;
        for reason in &self.engine.reasons {
            let ReasonReferenceV1::Write(expected) = &reason.content;
            let recorded = self
                .shell
                .receipts
                .get(&expected.operation_id)
                .ok_or_else(|| Error::domain("invalid_reason_metadata_state"))?;
            let next = recorded
                .receipt
                .last_event_cursor
                .ok_or_else(|| Error::domain("invalid_reason_metadata_state"))?;
            require(
                recorded.receipt.outcome == ExecutionOutcome::Success
                    && recorded.receipt.origin == reason.origin
                    && recorded.receipt.created_entity_id == Some(reason.project_id)
                    && reason.milestone_id.is_none()
                    && recorded.inputs.now == reason.occurred_at
                    && next > cursor,
                "invalid_reason_metadata_state",
            )?;
            cursor = next;
            let call = UnsignedContractCallV1::decode_signable(&recorded.bytes)?;
            let lowered = crate::catalog_content::compact_reason(&call);
            let (target, content, kind) = match lowered.as_ref().unwrap_or(&call.command) {
                generated_contracts::ProviderCommand::CancelProjectCore {
                    project_id,
                    content,
                } => (*project_id, content, DomainEventKind::ProjectCancelled),
                generated_contracts::ProviderCommand::DisputePlanningCore {
                    project_id,
                    content,
                } => (*project_id, content, DomainEventKind::PlanningDisputed),
                _ => return Err(Error::domain("invalid_reason_metadata_state")),
            };
            require(
                target == reason.project_id && content == expected && kind == reason.kind,
                "invalid_reason_metadata_state",
            )?;
        }
        Ok(())
    }
}
