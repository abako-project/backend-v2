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
