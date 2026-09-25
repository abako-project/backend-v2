use std::collections::BTreeSet;

use generated_contracts::{
    AccountId32, CatalogEntry, CatalogKind, CreateSkillRequest, DecideSkillRequest,
    DomainEventKind, ProviderCommand, SkillRequestDecision, SkillRequestStatus, SkillRequestView,
    UpsertCatalogEntryRequest,
};

use super::{Effect, State};
use crate::{Error, Result, random_id, require, seed::SkillMetadata};

#[cfg(test)]
mod tests;

pub(super) fn normalized(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

impl State {
    pub(super) fn upsert_catalog_entry(
        &mut self,
        origin: AccountId32,
        request: &UpsertCatalogEntryRequest,
    ) -> Result<Effect> {
        self.root(origin)?;
        require(
            !(request.kind == CatalogKind::Role && request.id == 1),
            "fixed_coordinator_role",
        )?;
        if request.kind == CatalogKind::Skill {
            self.unique_skill_name(&request.name, Some(request.id))?;
        }
        let catalog = if request.kind == CatalogKind::Role {
            &mut self.roles
        } else {
            &mut self.skills
        };
        catalog.insert(
            request.id,
            CatalogEntry {
                id: request.id,
                name: request.name.clone(),
                fixed: false,
            },
        );
        if request.kind == CatalogKind::Skill {
            self.skill_metadata
                .entry(request.id)
                .or_insert_with(|| SkillMetadata::requested(Vec::new()));
        }
        Ok(Effect::new(DomainEventKind::CatalogUpdated, None, origin))
    }

    pub(super) fn validate_catalog(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        for skill in self.skills.values() {
            require(
                names.insert(normalized(&skill.name)),
                "invalid_catalog_state",
            )?;
            let metadata = self
                .skill_metadata
                .get(&skill.id)
                .ok_or_else(|| Error::domain("invalid_catalog_state"))?;
            require(
                metadata
                    .role_ids
                    .iter()
                    .all(|id| self.roles.contains_key(id)),
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
            ProviderCommand::CreateSkillRequest(CreateSkillRequest {
                name: request.name.clone(),
                role_ids: request.role_ids.clone(),
            })
            .validate()
            .map_err(|_| Error::domain("invalid_skill_request_state"))?;
            require(
                (request.status == SkillRequestStatus::Approved) == request.skill_id.is_some(),
                "invalid_skill_request_state",
            )?;
        }
        Ok(())
    }

    pub(crate) fn skill_requests(&self, origin: AccountId32) -> Vec<SkillRequestView> {
        self.skill_requests
            .values()
            .filter(|request| origin == self.info.root_account || request.requester == origin)
            .cloned()
            .collect()
    }

    pub(super) fn unique_skill_name(&self, name: &str, except_id: Option<u32>) -> Result<()> {
        let name = normalized(name);
        require(
            !self
                .skills
                .values()
                .any(|skill| Some(skill.id) != except_id && normalized(&skill.name) == name),
            "skill_name_exists",
        )
    }

    pub(super) fn create_skill_request(
        &mut self,
        origin: AccountId32,
        request: &CreateSkillRequest,
    ) -> Result<Effect> {
        require(self.workers.contains_key(&origin), "worker_not_found")?;
        self.unique_skill_name(&request.name, None)?;
        require(
            request
                .role_ids
                .iter()
                .all(|id| self.roles.contains_key(id)),
            "unknown_role",
        )?;
        let id = random_id()?;
        require(
            !self.skill_requests.contains_key(&id),
            "request_id_collision",
        )?;
        self.skill_requests.insert(
            id,
            SkillRequestView {
                request_id: id,
                requester: origin,
                name: request.name.trim().to_owned(),
                role_ids: request.role_ids.clone(),
                status: SkillRequestStatus::Pending,
                skill_id: None,
            },
        );
        let mut effect = Effect::new(DomainEventKind::SkillRequested, Some(id), origin);
        effect.recipients.insert(self.info.root_account);
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
                self.unique_skill_name(&request.name, None)?;
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
                    CatalogEntry {
                        id,
                        name: request.name.clone(),
                        fixed: false,
                    },
                );
                self.skill_metadata
                    .insert(id, SkillMetadata::requested(request.role_ids.clone()));
                (DomainEventKind::SkillRequestApproved, Some(id))
            }
            SkillRequestDecision::Reject => (DomainEventKind::SkillRequestRejected, None),
        };
        let request = self
            .skill_requests
            .get_mut(&decision.request_id)
            .ok_or_else(Error::internal)?;
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
