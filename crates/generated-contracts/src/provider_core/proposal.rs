//! Compact proposal inputs. Human text remains in the adapter's proposal context.
use crate::{
    ContractError, MilestoneDefinition, Money, ProposalDefinition, RequirementDefinition,
    WeekWindow,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Enforceable milestone terms, without its display title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MilestoneTerms {
    /// Stable definition key; existing milestone/storage IDs follow this key.
    pub key: u32,
    /// Inclusive capacity window.
    pub window: WeekWindow,
    /// Quoted coordination share, reallocated by the execution-time policy.
    pub coordinator_fee: Money,
    /// Coordination capacity and reputation contribution.
    pub coordinator_minutes: crate::Minutes,
    /// Actual requirements needed for matching, capacity and payouts.
    pub requirements: Vec<RequirementDefinition>,
}
/// Numeric proposal definition signed alongside its human-content commitment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalTerms {
    /// Ordered milestone terms; order controls sequential activation.
    pub milestones: Vec<MilestoneTerms>,
}
impl From<&MilestoneDefinition> for MilestoneTerms {
    fn from(value: &MilestoneDefinition) -> Self {
        Self {
            key: value.key,
            window: value.window,
            coordinator_fee: value.coordinator_fee,
            coordinator_minutes: value.coordinator_minutes,
            requirements: value.requirements.clone(),
        }
    }
}
impl From<&ProposalDefinition> for ProposalTerms {
    fn from(value: &ProposalDefinition) -> Self {
        Self {
            milestones: value.milestones.iter().map(MilestoneTerms::from).collect(),
        }
    }
}
impl MilestoneTerms {
    /// Exact execution-time allocation shared with public legacy definitions.
    pub fn allocate_budget(
        &mut self,
        policy: &crate::CompensationPolicy,
    ) -> Result<(), ContractError> {
        let total = u128::from(self.total()?.units());
        let effort: u128 = self.requirements.iter().try_fold(0_u128, |sum, r| {
            sum.checked_add(u128::from(r.minutes.get()))
                .ok_or(ContractError::Invalid("budget overflow"))
        })?;
        if total > 0 && effort == 0 {
            return Err(ContractError::Invalid(
                "positive budget requires worker hours",
            ));
        }
        let percent = u128::from(policy.coordinator_percent.get());
        let denominator = effort
            .checked_mul(100)
            .ok_or(ContractError::Invalid("budget overflow"))?;
        let weights: Vec<_> = std::iter::once(percent * effort)
            .chain(
                self.requirements
                    .iter()
                    .map(|r| (100 - percent) * u128::from(r.minutes.get())),
            )
            .collect();
        let numerators: Vec<_> = weights
            .iter()
            .map(|weight| {
                total
                    .checked_mul(*weight)
                    .ok_or(ContractError::Invalid("budget overflow"))
            })
            .collect::<Result<_, _>>()?;
        let mut shares: Vec<_> = numerators
            .iter()
            .map(|amount| amount.checked_div(denominator).unwrap_or(0))
            .collect();
        let mut remainder = total - shares.iter().sum::<u128>();
        let mut order: Vec<_> = (0..shares.len()).collect();
        order.sort_by(|a, b| {
            let fraction = |index: usize| {
                if denominator == 0 {
                    0
                } else {
                    numerators[index] % denominator
                }
            };
            fraction(*b).cmp(&fraction(*a)).then_with(|| a.cmp(b))
        });
        for index in order {
            if remainder == 0 {
                break;
            }
            shares[index] += 1;
            remainder -= 1;
        }
        self.coordinator_fee = Money::new(
            u64::try_from(shares[0]).map_err(|_| ContractError::Invalid("budget overflow"))?,
        );
        for (requirement, share) in self.requirements.iter_mut().zip(shares.into_iter().skip(1)) {
            requirement.budget = Money::new(
                u64::try_from(share).map_err(|_| ContractError::Invalid("budget overflow"))?,
            );
        }
        Ok(())
    }
    /// Checked total; the core never derives an amount from descriptive content.
    pub fn total(&self) -> Result<Money, ContractError> {
        self.requirements
            .iter()
            .try_fold(self.coordinator_fee, |total, item| {
                Ok(total.checked_add(item.budget)?)
            })
    }
}
impl ProposalTerms {
    /// Validate every numeric/key constraint independently of adapter content.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.milestones.is_empty() {
            return Err(ContractError::Invalid("empty milestones"));
        }
        let mut keys = BTreeSet::new();
        for milestone in &self.milestones {
            if !keys.insert(milestone.key) {
                return Err(ContractError::Invalid("milestone keys"));
            }
            let mut requirements = BTreeSet::new();
            for requirement in &milestone.requirements {
                if !requirements.insert(requirement.key) {
                    return Err(ContractError::Invalid("requirement keys"));
                }
                if requirement.role_id == 0 || requirement.skill_ids.is_empty() {
                    return Err(ContractError::Invalid("requirement role or skills"));
                }
                super::super::catalog_ids(&requirement.skill_ids, "requirement skills")?;
            }
            milestone.total()?;
        }
        self.total()?;
        Ok(())
    }
    /// Exact full budget, checked before any balance mutation.
    pub fn total(&self) -> Result<Money, ContractError> {
        self.milestones.iter().try_fold(Money::ZERO, |total, item| {
            Ok(total.checked_add(item.total()?)?)
        })
    }
}

/// Reference to a proposal definition or change request, selected by core facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
#[allow(clippy::cast_possible_truncation, reason = "Two fixed SCALE variants.")]
pub enum ProposalContentReferenceV1 {
    /// Metadata operation committed atomically with actual terms/revision.
    #[codec(index = 0)]
    Signed(super::MetadataCommitmentV1),
    /// Explicit migration-time value, never a fabricated approval.
    #[codec(index = 1)]
    Migration(crate::PayloadHash),
}
impl ProposalContentReferenceV1 {
    /// Digest of the exact proposal-context preimage.
    #[must_use]
    pub const fn hash(&self) -> crate::PayloadHash {
        match self {
            Self::Signed(value) => value.hash,
            Self::Migration(value) => *value,
        }
    }
}
/// Core milestone facts; task ownership remains a subsequent milestone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MilestoneCore {
    /// Provider-generated identity retained by definition key on edit.
    pub milestone_id: crate::EntityId,
    /// Enforceable terms without the display title.
    pub definition: MilestoneTerms,
    /// Contractual lifecycle state.
    pub status: Option<crate::MilestoneStatus>,
    /// Contractual assignments; distinct from task assignees.
    pub assignments: Vec<crate::AssignmentView>,
    /// Exact scores retained for settlement.
    pub worker_ratings: Vec<crate::WorkerRating>,
    /// Existing tasks remain unchanged until R3.
    pub task_storage: crate::TaskStorageView,
    /// Core freeze flag.
    pub frozen: bool,
    /// Complete submission history/evidence commitments.
    pub submissions: Vec<crate::CompletionSubmission>,
}
/// Proposal terms, selected content references and current revision/lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposalCore {
    /// Provider-assigned identity.
    pub proposal_id: crate::EntityId,
    /// Actual authoritative revision; not predicted by metadata staging.
    pub revision: u64,
    /// Current definition preimage, independent from change request.
    pub content: ProposalContentReferenceV1,
    /// Current contractual state.
    pub status: crate::ProposalStatus,
    /// Ordered enforceable milestone facts.
    pub milestones: Vec<MilestoneCore>,
    /// Latest request reference, preserved through ordinary draft updates.
    pub change_request: Option<ProposalContentReferenceV1>,
    /// Includes the exact date constraint used by submission validation.
    pub delivery: Option<crate::DeliveryPreference>,
    /// Policy captured atomically when quoted terms were allocated.
    pub compensation_policy: Option<crate::CompensationPolicy>,
}
/// Private dispute query, without milestone descriptive preimages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoreDisputeView {
    /// Core case state and evidence references.
    pub dispute: crate::Dispute,
    /// Selected frozen core milestone; presentation is composed by the API.
    pub milestone: MilestoneCore,
}
impl MilestoneTerms {
    /// Assemble only with the selected, verified title.
    #[must_use]
    pub fn into_definition(self, title: String) -> MilestoneDefinition {
        MilestoneDefinition {
            key: self.key,
            title,
            window: self.window,
            coordinator_fee: self.coordinator_fee,
            coordinator_minutes: self.coordinator_minutes,
            requirements: self.requirements,
        }
    }
}
impl MilestoneCore {
    /// Preserve current facts while supplying only the descriptive title.
    #[must_use]
    pub fn into_view(self, title: String) -> crate::MilestoneView {
        crate::MilestoneView {
            milestone_id: self.milestone_id,
            definition: self.definition.into_definition(title),
            status: self.status,
            assignments: self.assignments,
            worker_ratings: self.worker_ratings,
            task_storage: self.task_storage,
            frozen: self.frozen,
            submissions: self.submissions,
        }
    }
}

impl ProposalCore {
    /// Current quoted terms, excluding mutable tasks, assignments and submissions.
    #[must_use]
    pub fn terms(&self) -> ProposalTerms {
        ProposalTerms {
            milestones: self
                .milestones
                .iter()
                .map(|m| m.definition.clone())
                .collect(),
        }
    }
    /// Resolve only after the owner verifies context/digest. Public numeric values
    /// always come from core, never from the pre-allocation metadata input.
    pub fn definition(
        &self,
        record: &super::ProposalMetadataRecordV1,
    ) -> Result<ProposalDefinition, ContractError> {
        let super::ProposalContentV1::Definition(original) = &record.content else {
            return Err(ContractError::Invalid("proposal metadata kind"));
        };
        original.validate()?;
        let mut expected = ProposalTerms::from(original);
        if matches!(
            record.provenance,
            super::ProposalProvenanceV1::Operation { .. }
        ) {
            let policy = self
                .compensation_policy
                .as_ref()
                .ok_or(ContractError::Invalid("proposal metadata policy"))?;
            for milestone in &mut expected.milestones {
                milestone.allocate_budget(policy)?;
            }
        }
        if expected != self.terms() {
            return Err(ContractError::Invalid("proposal metadata terms"));
        }
        Ok(ProposalDefinition {
            title: original.title.clone(),
            description: original.description.clone(),
            milestones: self
                .milestones
                .iter()
                .zip(&original.milestones)
                .map(|(core, text)| core.definition.clone().into_definition(text.title.clone()))
                .collect(),
        })
    }
    /// Compose current business facts with a verified definition and explanation.
    pub fn into_view(
        self,
        definition: ProposalDefinition,
        change_request: Option<String>,
    ) -> Result<crate::ProposalView, ContractError> {
        if self.terms() != ProposalTerms::from(&definition)
            || self.change_request.is_some() != change_request.is_some()
        {
            return Err(ContractError::Invalid("proposal metadata terms"));
        }
        Ok(crate::ProposalView {
            proposal_id: self.proposal_id,
            revision: self.revision,
            title: definition.title,
            description: definition.description,
            status: self.status,
            milestones: self
                .milestones
                .into_iter()
                .zip(definition.milestones)
                .map(|(core, text)| core.into_view(text.title))
                .collect(),
            change_request,
            delivery: self.delivery,
            compensation_policy: self.compensation_policy,
        })
    }
}
