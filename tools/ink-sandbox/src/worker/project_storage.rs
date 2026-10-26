//! Exact private codec for the pinned ordinary Project cells.
use super::{
    Account, Error, MAX_SHELL,
    facts::{Reason, Storage},
};
use generated_contracts::{provider_core::*, *};
use parity_scale_codec::{Decode, Encode};
type Result<T> = std::result::Result<T, Error>;
type MilestonePosition = (u32, u32);
macro_rules! record {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Encode, Decode)]
        pub(super) struct $name { $(pub(super) $field: $ty,)* }
    };
}
#[derive(Clone, Encode, Decode)]
pub(super) enum StoredDelivery {
    WithinOneMonth,
    OneToThreeMonths,
    ThreeToSixMonths,
    SpecificDate([u8; 10]),
}
impl StoredDelivery {
    fn view(&self) -> Result<DeliveryPreference> {
        let value = match self {
            Self::WithinOneMonth => DeliveryPreference::WithinOneMonth {},
            Self::OneToThreeMonths => DeliveryPreference::OneToThreeMonths {},
            Self::ThreeToSixMonths => DeliveryPreference::ThreeToSixMonths {},
            Self::SpecificDate(date) => DeliveryPreference::SpecificDate {
                date: String::from_utf8(date.to_vec()).map_err(|_| Error::InvalidState)?,
            },
        };
        value.validate().map_err(|_| Error::InvalidState)?;
        Ok(value)
    }
}
record!(ProjectRecord {
    project_id: EntityId, created_at: Option<UnixSeconds>, client: AccountId32, coordinator: AccountId32,
    content: MetadataReferenceV1, planning: PlanningView, proposals: u32,
    execution_escrow: Money, cancelled: bool, completed: bool, evaluations: u32,
    active_dispute_id: Option<EntityId>, reasons: u32, disputes: u32
});
record!(ProposalRecord {
    proposal_id: EntityId, revision: u64, content: ProposalContentReferenceV1,
    status: ProposalStatus, milestones: u32, change_request: Option<ProposalContentReferenceV1>,
    delivery: Option<StoredDelivery>, compensation_policy: Option<CompensationPolicy>
});
record!(MilestoneRecord {
    milestone_id: EntityId, key: u32, window: WeekWindow, coordinator_fee: Money,
    coordinator_minutes: Minutes, requirements: u32, status: Option<MilestoneStatus>,
    assignments: u32, worker_ratings: u32, task_storage_id: EntityId, tasks: u32,
    frozen: bool, submissions: u32
});
record!(RequirementRecord {
    key: u32,
    role_id: u32,
    skills: u32,
    minutes: Minutes,
    budget: Money
});
record!(TaskRecord {
    task_id: u32,
    assignees: u32
});
record!(SubmissionRecord {
    submission_id: EntityId, version: u64, deliverable: Option<EvidenceRecordReferenceV1>,
    submitted_by: AccountId32, submitted_at: UnixSeconds, worker_ratings: u32, review: SubmissionReview
});
record!(EvaluationRecord {
    author: AccountId32,
    submitted_at: UnixSeconds,
    ratings: u32
});

fn items<T>(length: u32, read: impl FnMut(u32) -> Result<T>) -> Result<Vec<T>> {
    if length as usize > MAX_SHELL {
        return Err(Error::Limit);
    }
    (0..length).map(read).collect()
}
pub(super) struct StoredProject {
    pub(super) directory: Account,
    pub(super) project: ProjectCore,
    pub(super) reasons: Vec<Reason>,
    pub(super) cases: Vec<Dispute>,
}
pub(super) fn read(reader: &mut Storage<'_>, address: &Account) -> Result<StoredProject> {
    let (directory, id): (Account, EntityId) = reader.cell(address, 0u32)?;

    let record: ProjectRecord = reader.cell(address, 1u32)?;
    if record.project_id != id {
        return Err(Error::InvalidState);
    }
    let project = ProjectCore {
        project_id: record.project_id,
        created_at: record.created_at,
        client: record.client,
        coordinator: record.coordinator,
        content: record.content,
        planning: record.planning,
        execution_escrow: record.execution_escrow,
        cancelled: record.cancelled,
        completed: record.completed,
        active_dispute_id: record.active_dispute_id,
        proposals: items(record.proposals, |p| proposal(reader, address, p))?,
        evaluations: items(record.evaluations, |e| {
            let record: EvaluationRecord = reader.value(address, 20, e)?;
            Ok(ProjectEvaluation {
                author: record.author,
                submitted_at: record.submitted_at,
                ratings: reader.list(address, 21, e, record.ratings)?,
            })
        })?,
    };
    Ok(StoredProject {
        directory,
        project,
        reasons: items(record.reasons, |i| reader.value(address, 22, i))?,
        cases: items(record.disputes, |i| reader.value(address, 23, i))?,
    })
}
fn proposal(reader: &mut Storage<'_>, address: &Account, p: u32) -> Result<ProposalCore> {
    let record: ProposalRecord = reader.value(address, 10, p)?;
    Ok(ProposalCore {
        proposal_id: record.proposal_id,
        revision: record.revision,
        content: record.content,
        status: record.status,
        change_request: record.change_request,
        delivery: record
            .delivery
            .as_ref()
            .map(StoredDelivery::view)
            .transpose()?,
        compensation_policy: record.compensation_policy,
        milestones: items(record.milestones, |m| milestone(reader, address, (p, m)))?,
    })
}
fn milestone(
    reader: &mut Storage<'_>,
    address: &Account,
    pos: MilestonePosition,
) -> Result<MilestoneCore> {
    let record: MilestoneRecord = reader.value(address, 11, pos)?;
    Ok(MilestoneCore {
        milestone_id: record.milestone_id,
        definition: MilestoneTerms {
            key: record.key,
            window: record.window,
            coordinator_fee: record.coordinator_fee,
            coordinator_minutes: record.coordinator_minutes,
            requirements: items(record.requirements, |r| {
                let key = (pos, r);
                let record: RequirementRecord = reader.value(address, 12, key)?;
                Ok(RequirementDefinition {
                    key: record.key,
                    role_id: record.role_id,
                    minutes: record.minutes,
                    budget: record.budget,
                    skill_ids: reader.list(address, 13, key, record.skills)?,
                })
            })?,
        },
        status: record.status,
        frozen: record.frozen,
        assignments: reader.list(address, 14, pos, record.assignments)?,
        worker_ratings: reader.list(address, 15, pos, record.worker_ratings)?,
        task_storage: TaskStorageCore {
            task_storage_id: record.task_storage_id,
            milestone_id: record.milestone_id,
            tasks: items(record.tasks, |t| {
                let key = (pos, t);
                let record: TaskRecord = reader.value(address, 16, key)?;
                Ok(TaskCore {
                    task_id: record.task_id,
                    assignees: reader.list(address, 17, key, record.assignees)?,
                })
            })?,
        },
        submissions: items(record.submissions, |s| {
            let key = (pos, s);
            let record: SubmissionRecord = reader.value(address, 18, key)?;
            Ok(CompletionSubmission {
                submission_id: record.submission_id,
                version: record.version,
                deliverable: record.deliverable,
                submitted_by: record.submitted_by,
                submitted_at: record.submitted_at,
                review: record.review,
                worker_ratings: reader.list(address, 19, key, record.worker_ratings)?,
            })
        })?,
    })
}
