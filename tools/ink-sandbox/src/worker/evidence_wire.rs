//! Private fixed-size contract ABI. Public/native legacy encodings stay unchanged.
use super::Error;
use generated_contracts as g;
use parity_scale_codec::{Decode, Encode};
use std::collections::BTreeMap;

pub(super) type LegacyReferences = BTreeMap<g::PayloadHash, g::EvidenceReference>;
const MAX_LEGACY_REFERENCES: usize = 16_384;

pub(super) fn legacy_hash(value: &g::EvidenceReference) -> g::PayloadHash {
    let mut bytes = b"abako/legacy-reference/v1\0".to_vec();
    bytes.extend(value.encode());
    g::PayloadHash::from_bytes(sp_core::hashing::sha2_256(&bytes))
}
pub(super) fn validate(refs: &LegacyReferences) -> Result<(), Error> {
    if refs.len() > MAX_LEGACY_REFERENCES {
        return Err(Error::Limit);
    }
    if refs.iter().any(|(hash, value)| *hash != legacy_hash(value)) {
        return Err(Error::InvalidState);
    }
    Ok(())
}
#[derive(Clone, Debug, Encode, Decode)]
pub(super) enum Reference {
    #[codec(index = 0)]
    LegacyReferenceHash(g::PayloadHash),
    #[codec(index = 1)]
    Committed(g::EvidenceCommitmentV1),
}
impl Reference {
    pub(super) fn legacy(
        value: g::EvidenceReference,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        let hash = legacy_hash(&value);
        if let Some(saved) = refs.get(&hash) {
            if saved != &value {
                return Err(Error::InvalidState);
            }
        } else {
            if refs.len() >= MAX_LEGACY_REFERENCES {
                return Err(Error::Limit);
            }
            refs.insert(hash, value);
        }
        Ok(Self::LegacyReferenceHash(hash))
    }
    fn from_domain(
        value: g::EvidenceRecordReferenceV1,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        match value {
            g::EvidenceRecordReferenceV1::Legacy(value) => Self::legacy(value, refs),
            g::EvidenceRecordReferenceV1::Committed(value) => Ok(Self::Committed(value)),
        }
    }
    fn into_domain(self, refs: &LegacyReferences) -> Result<g::EvidenceRecordReferenceV1, Error> {
        match self {
            Self::LegacyReferenceHash(hash) => {
                let value = refs.get(&hash).ok_or(Error::InvalidState)?;
                if legacy_hash(value) != hash {
                    return Err(Error::InvalidState);
                }
                Ok(g::EvidenceRecordReferenceV1::Legacy(value.clone()))
            }
            Self::Committed(value) => Ok(g::EvidenceRecordReferenceV1::Committed(value)),
        }
    }
}
#[derive(Clone, Debug, Encode, Decode)]
pub(super) enum Review {
    #[codec(index = 0)]
    Pending,
    #[codec(index = 1)]
    Rejected {
        reason: Reference,
        reviewed_by: g::AccountId32,
        reviewed_at: g::UnixSeconds,
    },
    #[codec(index = 2)]
    Accepted {
        reviewed_by: g::AccountId32,
        reviewed_at: g::UnixSeconds,
    },
    #[codec(index = 3)]
    RejectedWithComment {
        comment_id: g::EntityId,
        reviewed_by: g::AccountId32,
        reviewed_at: g::UnixSeconds,
    },
}
impl Review {
    fn from_domain(value: g::SubmissionReview, refs: &mut LegacyReferences) -> Result<Self, Error> {
        Ok(match value {
            g::SubmissionReview::PendingReview => Self::Pending,
            g::SubmissionReview::Rejected {
                reason,
                reviewed_by,
                reviewed_at,
            } => Self::Rejected {
                reason: Reference::from_domain(reason, refs)?,
                reviewed_by,
                reviewed_at,
            },
            g::SubmissionReview::Accepted {
                reviewed_by,
                reviewed_at,
            } => Self::Accepted {
                reviewed_by,
                reviewed_at,
            },
            g::SubmissionReview::RejectedWithComment {
                comment_id,
                reviewed_by,
                reviewed_at,
            } => Self::RejectedWithComment {
                comment_id,
                reviewed_by,
                reviewed_at,
            },
        })
    }
    fn into_domain(self, refs: &LegacyReferences) -> Result<g::SubmissionReview, Error> {
        Ok(match self {
            Self::Pending => g::SubmissionReview::PendingReview,
            Self::Rejected {
                reason,
                reviewed_by,
                reviewed_at,
            } => g::SubmissionReview::Rejected {
                reason: reason.into_domain(refs)?,
                reviewed_by,
                reviewed_at,
            },
            Self::Accepted {
                reviewed_by,
                reviewed_at,
            } => g::SubmissionReview::Accepted {
                reviewed_by,
                reviewed_at,
            },
            Self::RejectedWithComment {
                comment_id,
                reviewed_by,
                reviewed_at,
            } => g::SubmissionReview::RejectedWithComment {
                comment_id,
                reviewed_by,
                reviewed_at,
            },
        })
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub(super) struct Submission {
    pub(super) submission_id: g::EntityId,
    pub(super) version: u64,
    pub(super) deliverable: Option<Reference>,
    pub(super) submitted_by: g::AccountId32,
    pub(super) submitted_at: g::UnixSeconds,
    pub(super) worker_ratings: Vec<g::WorkerRating>,
    pub(super) review: Review,
}
impl Submission {
    pub(super) fn from_domain(
        value: g::CompletionSubmission,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        Ok(Self {
            submission_id: value.submission_id,
            version: value.version,
            deliverable: value
                .deliverable
                .map(|item| Reference::from_domain(item, refs))
                .transpose()?,
            submitted_by: value.submitted_by,
            submitted_at: value.submitted_at,
            worker_ratings: value.worker_ratings,
            review: Review::from_domain(value.review, refs)?,
        })
    }
    pub(super) fn into_domain(
        self,
        refs: &LegacyReferences,
    ) -> Result<g::CompletionSubmission, Error> {
        Ok(g::CompletionSubmission {
            submission_id: self.submission_id,
            version: self.version,
            deliverable: self
                .deliverable
                .map(|item| item.into_domain(refs))
                .transpose()?,
            submitted_by: self.submitted_by,
            submitted_at: self.submitted_at,
            worker_ratings: self.worker_ratings,
            review: self.review.into_domain(refs)?,
        })
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub(super) struct Milestone {
    pub(super) milestone_id: g::EntityId,
    pub(super) definition: g::provider_core::MilestoneTerms,
    pub(super) status: Option<g::MilestoneStatus>,
    pub(super) assignments: Vec<g::AssignmentView>,
    pub(super) worker_ratings: Vec<g::WorkerRating>,
    pub(super) task_storage: g::provider_core::TaskStorageCore,
    pub(super) frozen: bool,
    pub(super) submissions: Vec<Submission>,
}
impl Milestone {
    pub(super) fn from_domain(
        value: g::provider_core::MilestoneCore,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        Ok(Self {
            milestone_id: value.milestone_id,
            definition: value.definition,
            status: value.status,
            assignments: value.assignments,
            worker_ratings: value.worker_ratings,
            task_storage: value.task_storage,
            frozen: value.frozen,
            submissions: value
                .submissions
                .into_iter()
                .map(|item| Submission::from_domain(item, refs))
                .collect::<Result<_, _>>()?,
        })
    }
    pub(super) fn into_domain(
        self,
        refs: &LegacyReferences,
    ) -> Result<g::provider_core::MilestoneCore, Error> {
        Ok(g::provider_core::MilestoneCore {
            milestone_id: self.milestone_id,
            definition: self.definition,
            status: self.status,
            assignments: self.assignments,
            worker_ratings: self.worker_ratings,
            task_storage: self.task_storage,
            frozen: self.frozen,
            submissions: self
                .submissions
                .into_iter()
                .map(|item| item.into_domain(refs))
                .collect::<Result<_, _>>()?,
        })
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub(super) struct Proposal {
    pub(super) proposal_id: g::EntityId,
    pub(super) revision: u64,
    pub(super) content: g::provider_core::ProposalContentReferenceV1,
    pub(super) status: g::ProposalStatus,
    pub(super) milestones: Vec<Milestone>,
    pub(super) change_request: Option<g::provider_core::ProposalContentReferenceV1>,
    pub(super) delivery: Option<g::DeliveryPreference>,
    pub(super) compensation_policy: Option<g::CompensationPolicy>,
}
impl Proposal {
    pub(super) fn from_domain(
        value: g::provider_core::ProposalCore,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        Ok(Self {
            proposal_id: value.proposal_id,
            revision: value.revision,
            content: value.content,
            status: value.status,
            milestones: value
                .milestones
                .into_iter()
                .map(|item| Milestone::from_domain(item, refs))
                .collect::<Result<_, _>>()?,
            change_request: value.change_request,
            delivery: value.delivery,
            compensation_policy: value.compensation_policy,
        })
    }
    pub(super) fn into_domain(
        self,
        refs: &LegacyReferences,
    ) -> Result<g::provider_core::ProposalCore, Error> {
        Ok(g::provider_core::ProposalCore {
            proposal_id: self.proposal_id,
            revision: self.revision,
            content: self.content,
            status: self.status,
            milestones: self
                .milestones
                .into_iter()
                .map(|item| item.into_domain(refs))
                .collect::<Result<_, _>>()?,
            change_request: self.change_request,
            delivery: self.delivery,
            compensation_policy: self.compensation_policy,
        })
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub(super) struct Project {
    pub(super) project_id: g::EntityId,
    pub(super) created_at: Option<g::UnixSeconds>,
    pub(super) client: g::AccountId32,
    pub(super) coordinator: g::AccountId32,
    pub(super) content: g::provider_core::MetadataReferenceV1,
    pub(super) planning: g::PlanningView,
    pub(super) proposals: Vec<Proposal>,
    pub(super) execution_escrow: g::Money,
    pub(super) cancelled: bool,
    pub(super) completed: bool,
    pub(super) evaluations: Vec<g::ProjectEvaluation>,
    pub(super) active_dispute_id: Option<g::EntityId>,
}
impl Project {
    pub(super) fn from_domain(
        value: g::provider_core::ProjectCore,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        Ok(Self {
            project_id: value.project_id,
            created_at: value.created_at,
            client: value.client,
            coordinator: value.coordinator,
            content: value.content,
            planning: value.planning,
            proposals: value
                .proposals
                .into_iter()
                .map(|item| Proposal::from_domain(item, refs))
                .collect::<Result<_, _>>()?,
            execution_escrow: value.execution_escrow,
            cancelled: value.cancelled,
            completed: value.completed,
            evaluations: value.evaluations,
            active_dispute_id: value.active_dispute_id,
        })
    }
    pub(super) fn into_domain(
        self,
        refs: &LegacyReferences,
    ) -> Result<g::provider_core::ProjectCore, Error> {
        Ok(g::provider_core::ProjectCore {
            project_id: self.project_id,
            created_at: self.created_at,
            client: self.client,
            coordinator: self.coordinator,
            content: self.content,
            planning: self.planning,
            proposals: self
                .proposals
                .into_iter()
                .map(|item| item.into_domain(refs))
                .collect::<Result<_, _>>()?,
            execution_escrow: self.execution_escrow,
            cancelled: self.cancelled,
            completed: self.completed,
            evaluations: self.evaluations,
            active_dispute_id: self.active_dispute_id,
        })
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub(super) struct Response {
    pub(super) evidence: Reference,
    pub(super) author: g::AccountId32,
    pub(super) occurred_at: g::UnixSeconds,
}
impl Response {
    pub(super) fn from_domain(
        value: g::DisputeResponse,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        Ok(Self {
            evidence: Reference::from_domain(value.evidence, refs)?,
            author: value.author,
            occurred_at: value.occurred_at,
        })
    }
    pub(super) fn into_domain(self, refs: &LegacyReferences) -> Result<g::DisputeResponse, Error> {
        Ok(g::DisputeResponse {
            evidence: self.evidence.into_domain(refs)?,
            author: self.author,
            occurred_at: self.occurred_at,
        })
    }
}

#[derive(Clone, Debug, Encode, Decode)]
pub(super) struct Dispute {
    pub(super) dispute_id: g::EntityId,
    pub(super) project_id: g::EntityId,
    pub(super) milestone_id: g::EntityId,
    pub(super) rejected_submission_id: g::EntityId,
    pub(super) status: g::DisputeStatus,
    pub(super) opened_by: g::AccountId32,
    pub(super) counterparty: g::AccountId32,
    pub(super) opened_at: g::UnixSeconds,
    pub(super) evidence: Option<Reference>,
    pub(super) opening_comment_id: Option<g::EntityId>,
    pub(super) response: Option<Response>,
    pub(super) proposal_revision: u64,
    pub(super) context_event_cursor: u64,
}
impl Dispute {
    pub(super) fn from_domain(
        value: g::Dispute,
        refs: &mut LegacyReferences,
    ) -> Result<Self, Error> {
        Ok(Self {
            dispute_id: value.dispute_id,
            project_id: value.project_id,
            milestone_id: value.milestone_id,
            rejected_submission_id: value.rejected_submission_id,
            status: value.status,
            opened_by: value.opened_by,
            counterparty: value.counterparty,
            opened_at: value.opened_at,
            evidence: value
                .evidence
                .map(|item| Reference::from_domain(item, refs))
                .transpose()?,
            opening_comment_id: value.opening_comment_id,
            response: value
                .response
                .map(|item| Response::from_domain(item, refs))
                .transpose()?,
            proposal_revision: value.proposal_revision,
            context_event_cursor: value.context_event_cursor,
        })
    }
    pub(super) fn into_domain(self, refs: &LegacyReferences) -> Result<g::Dispute, Error> {
        Ok(g::Dispute {
            dispute_id: self.dispute_id,
            project_id: self.project_id,
            milestone_id: self.milestone_id,
            rejected_submission_id: self.rejected_submission_id,
            status: self.status,
            opened_by: self.opened_by,
            counterparty: self.counterparty,
            opened_at: self.opened_at,
            evidence: self
                .evidence
                .map(|item| item.into_domain(refs))
                .transpose()?,
            opening_comment_id: self.opening_comment_id,
            response: self
                .response
                .map(|item| item.into_domain(refs))
                .transpose()?,
            proposal_revision: self.proposal_revision,
            context_event_cursor: self.context_event_cursor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_reference_codec_preserves_preimages_and_rejects_missing_or_changed_map_entries() {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../specs/0014-evidence-commitments/hash-vectors.json"
        ))
        .unwrap();
        let vector = &vector["legacyReferenceV1"];
        let legacy: g::EvidenceReference = serde_json::from_value(serde_json::json!({
            "url": vector["url"], "sha256": vector["legacySha256"]
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(legacy_hash(&legacy)).unwrap(),
            vector["hash"]
        );
        let mut refs = LegacyReferences::new();
        let wire = Reference::legacy(legacy.clone(), &mut refs).unwrap();
        assert_eq!(wire.encoded_size(), 33);
        assert_eq!(
            wire.clone().into_domain(&refs).unwrap(),
            legacy.clone().into()
        );
        assert!(wire.clone().into_domain(&LegacyReferences::new()).is_err());
        let hash = legacy_hash(&legacy);
        refs.insert(
            hash,
            g::EvidenceReference::new("https://example.test/changed".into()).unwrap(),
        );
        assert!(wire.into_domain(&refs).is_err());
        assert!(validate(&refs).is_err());
        let committed = Reference::Committed(g::EvidenceCommitmentV1 {
            record_id: g::EntityId::from_bytes([1; 16]),
            hash,
        });
        assert_eq!(committed.encoded_size(), 49);
        assert!(committed.into_domain(&LegacyReferences::new()).is_ok());
    }
}
