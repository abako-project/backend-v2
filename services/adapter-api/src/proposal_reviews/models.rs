use crate::state::Error;
use generated_contracts::{AccountId32, EntityId, ProposalDefinition, ProposalView};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Experience {
    Junior,
    MidLevel,
    Senior,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequestedExperience {
    pub(crate) key: u32,
    pub(crate) experience: Option<Experience>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MilestonePresentation {
    pub(crate) key: u32,
    pub(crate) description: String,
    pub(crate) requirements: Vec<RequestedExperience>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PutPresentation {
    pub(crate) expected_proposal_revision: u64,
    pub(crate) expected_presentation_revision: i64,
    pub(crate) milestones: Vec<MilestonePresentation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresentationView {
    pub(crate) proposal_id: EntityId,
    pub(crate) revision: i64,
    pub(crate) proposal_revision: u64,
    pub(crate) matches_current_definition: bool,
    pub(crate) milestones: Vec<MilestonePresentation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PostComment {
    pub(crate) expected_proposal_revision: u64,
    pub(crate) request_id: EntityId,
    pub(crate) message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewComment {
    pub(crate) comment_id: i64,
    pub(crate) proposal_revision: u64,
    pub(crate) author_account: AccountId32,
    pub(crate) created_at: i64,
    pub(crate) message: String,
    pub(crate) definition: ProposalDefinition,
    pub(crate) presentation: Option<PresentationView>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommentsPage {
    pub(crate) items: Vec<ReviewComment>,
    pub(crate) next_after: Option<i64>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    #[serde(default)]
    pub(crate) after: i64,
}
pub(super) fn definition(proposal: &ProposalView) -> ProposalDefinition {
    ProposalDefinition {
        title: proposal.title.clone(),
        description: proposal.description.clone(),
        milestones: proposal
            .milestones
            .iter()
            .map(|m| m.definition.clone())
            .collect(),
    }
}
impl PutPresentation {
    pub(super) fn validate(&self, proposal: &ProposalView) -> Result<(), Error> {
        if self.expected_proposal_revision > i64::MAX as u64
            || !(0..i64::MAX).contains(&self.expected_presentation_revision)
            || self.milestones.len() > 100
        {
            return Err(Error::Invalid);
        }
        let mut keys = BTreeSet::new();
        for item in &self.milestones {
            let milestone = proposal
                .milestones
                .iter()
                .find(|m| m.definition.key == item.key)
                .ok_or(Error::Invalid)?;
            if !keys.insert(item.key)
                || item.description.len() > 2000
                || item.description.contains('\0')
                || item.requirements.len() > 100
            {
                return Err(Error::Invalid);
            }
            let mut requirements = BTreeSet::new();
            for item in &item.requirements {
                if !requirements.insert(item.key)
                    || !milestone
                        .definition
                        .requirements
                        .iter()
                        .any(|r| r.key == item.key)
                {
                    return Err(Error::Invalid);
                }
            }
        }
        Ok(())
    }
}
impl PostComment {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.expected_proposal_revision > i64::MAX as u64
            || self.message.trim().is_empty()
            || self.message.len() > 10000
            || self.message.contains('\0')
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
