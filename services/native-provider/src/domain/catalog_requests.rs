use super::{Effect, RustEngine};
use crate::{Error, Result, require};
use protocol::{
    AccountId32, CatalogKind, DecideSkillRequest, DomainEventKind, SkillRequestDecision,
    SkillRequestStatus,
    provider_core::{
        CatalogEntryCore, CatalogReferenceV1, MetadataCommitmentV1, SkillMetadataCore,
        SkillRequestCore,
    },
};
#[cfg(test)]
mod tests;

impl RustEngine {
    pub(super) fn upsert_catalog_entry(
        &mut self,
        origin: AccountId32,
        kind: CatalogKind,
        id: u32,
        content: &MetadataCommitmentV1,
    ) -> Result<Effect> {
        self.root(origin)?;
        require(
            !(kind == CatalogKind::Role && id == 1),
            "fixed_coordinator_role",
        )?;
        let reference = CatalogReferenceV1::Write(content.clone());
        let catalog = if kind == CatalogKind::Role {
            &mut self.roles
        } else {
            &mut self.skills
        };
        catalog.insert(
            id,
            CatalogEntryCore {
                id,
                fixed: false,
                content: reference.clone(),
            },
        );
        if kind == CatalogKind::Skill {
            self.skill_metadata.entry(id).or_insert(SkillMetadataCore {
                role_ids: Vec::new(),
                origin: reference,
            });
        }
        Ok(Effect::new(DomainEventKind::CatalogUpdated, None, origin))
    }
    pub(super) fn validate_catalog(&self) -> Result<()> {
        for (id, entry) in &self.roles {
            require(
                *id != 0 && entry.id == *id && entry.fixed == (*id == 1),
                "invalid_catalog_state",
            )?;
        }
        for (id, entry) in &self.skills {
            require(
                *id != 0 && entry.id == *id && !entry.fixed,
                "invalid_catalog_state",
            )?;
            let metadata = self
                .skill_metadata
                .get(id)
                .ok_or_else(|| Error::domain("invalid_catalog_state"))?;
            require(
                metadata
                    .role_ids
                    .iter()
                    .all(|role| self.roles.contains_key(role)),
                "invalid_catalog_state",
            )?;
        }
        require(
            self.skill_metadata
                .keys()
                .all(|id| self.skills.contains_key(id)),
            "invalid_catalog_state",
        )?;
        for (id, request) in &self.skill_requests {
            require(
                *id == request.request_id && self.workers.contains_key(&request.requester),
                "invalid_skill_request_state",
            )?;
            protocol::provider_core::validate_skill_request_roles(&request.role_ids)
                .map_err(|_| Error::domain("invalid_skill_request_state"))?;
            require(
                (request.status == SkillRequestStatus::Approved) == request.skill_id.is_some(),
                "invalid_skill_request_state",
            )?;
        }
        Ok(())
    }
    pub(crate) fn skill_requests(&self, origin: AccountId32) -> Vec<SkillRequestCore> {
        self.skill_requests
            .values()
            .filter(|request| origin == self.context.root_account || request.requester == origin)
            .cloned()
            .collect()
    }
    pub(super) fn create_skill_request(
        &mut self,
        origin: AccountId32,
        role_ids: &[u32],
        content: &MetadataCommitmentV1,
    ) -> Result<Effect> {
        require(self.workers.contains_key(&origin), "worker_not_found")?;
        require(
            role_ids.iter().all(|id| self.roles.contains_key(id)),
            "unknown_role",
        )?;
        let id = self.context.random_id();
        require(
            !self.skill_requests.contains_key(&id),
            "request_id_collision",
        )?;
        self.skill_requests.insert(
            id,
            SkillRequestCore {
                request_id: id,
                requester: origin,
                content: CatalogReferenceV1::Write(content.clone()),
                role_ids: role_ids.to_vec(),
                status: SkillRequestStatus::Pending,
                skill_id: None,
            },
        );
        let mut effect = Effect::new(DomainEventKind::SkillRequested, Some(id), origin);
        effect.recipients.insert(self.context.root_account);
        Ok(effect)
    }
    pub(super) fn decide_skill_request(
        &mut self,
        origin: AccountId32,
        decision: &DecideSkillRequest,
    ) -> Result<Effect> {
        self.root(origin)?;
        let request = self
            .skill_requests
            .get(&decision.request_id)
            .ok_or_else(|| Error::domain("skill_request_not_found"))?;
        require(
            request.status == SkillRequestStatus::Pending,
            "skill_request_decided",
        )?;
        let requester = request.requester;
        let (kind, skill_id) = match decision.decision {
            SkillRequestDecision::Approve => {
                require(
                    request
                        .role_ids
                        .iter()
                        .all(|id| self.roles.contains_key(id)),
                    "unknown_role",
                )?;
                let id = self
                    .skills
                    .last_key_value()
                    .map_or(Some(1), |(id, _)| id.checked_add(1))
                    .ok_or_else(|| Error::domain("catalog_id_exhausted"))?;
                self.skills.insert(
                    id,
                    CatalogEntryCore {
                        id,
                        fixed: false,
                        content: request.content.clone(),
                    },
                );
                self.skill_metadata.insert(
                    id,
                    SkillMetadataCore {
                        role_ids: request.role_ids.clone(),
                        origin: request.content.clone(),
                    },
                );
                (DomainEventKind::SkillRequestApproved, Some(id))
            }
            SkillRequestDecision::Reject => (DomainEventKind::SkillRequestRejected, None),
        };
        let request = self
            .skill_requests
            .get_mut(&decision.request_id)
            .ok_or_else(Error::unavailable)?;
        request.status = match decision.decision {
            SkillRequestDecision::Approve => SkillRequestStatus::Approved,
            SkillRequestDecision::Reject => SkillRequestStatus::Rejected,
        };
        request.skill_id = skill_id;
        let mut effect = Effect::new(kind, Some(decision.request_id), requester);
        effect.recipients.insert(origin);
        Ok(effect)
    }
}
