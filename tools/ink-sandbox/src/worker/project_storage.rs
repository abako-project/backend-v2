//! Exact private codec for the pinned ordinary Project cells and typed import.
use super::{
    Account, Error, MAX_SHELL,
    archive::{Reason, Storage},
    evidence_wire as wire,
};
use generated_contracts::{provider_core::*, *};
use parity_scale_codec::{Decode, Encode};
type Result<T> = std::result::Result<T, Error>;
type MilestonePosition = (u32, u32);
type ChildPosition = (MilestonePosition, u32);
type NestedPosition = (ChildPosition, u32);
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
    fn from_view(value: &DeliveryPreference) -> Result<Self> {
        value.validate().map_err(|_| Error::InvalidState)?;
        Ok(match value {
            DeliveryPreference::WithinOneMonth {} => Self::WithinOneMonth,
            DeliveryPreference::OneToThreeMonths {} => Self::OneToThreeMonths,
            DeliveryPreference::ThreeToSixMonths {} => Self::ThreeToSixMonths,
            DeliveryPreference::SpecificDate { date } => Self::SpecificDate(
                date.as_bytes()
                    .try_into()
                    .map_err(|_| Error::InvalidState)?,
            ),
        })
    }
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
    submission_id: EntityId, version: u64, deliverable: Option<wire::Reference>,
    submitted_by: AccountId32, submitted_at: UnixSeconds, worker_ratings: u32, review: wire::Review
});
record!(EvaluationRecord {
    author: AccountId32,
    submitted_at: UnixSeconds,
    ratings: u32
});

#[derive(Clone, Encode)]
pub(super) enum Record {
    Project(ProjectRecord),
    Proposal(u32, ProposalRecord),
    Milestone(MilestonePosition, MilestoneRecord),
    Requirement(ChildPosition, RequirementRecord),
    Skill(NestedPosition, u32),
    Assignment(ChildPosition, AssignmentView),
    MilestoneRating(ChildPosition, WorkerRating),
    Task(ChildPosition, TaskRecord),
    Assignee(NestedPosition, AccountId32),
    Submission(ChildPosition, SubmissionRecord),
    SubmissionRating(NestedPosition, WorkerRating),
    Evaluation(u32, EvaluationRecord),
    EvaluationRating((u32, u32), AccountRating),
    Reason(u32, Reason),
    Dispute(u32, wire::Dispute),
}
fn count(value: usize) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::Limit)
}
fn items<T>(length: u32, read: impl FnMut(u32) -> Result<T>) -> Result<Vec<T>> {
    if length as usize > MAX_SHELL {
        return Err(Error::Limit);
    }
    (0..length).map(read).collect()
}
pub(super) struct StoredProject {
    pub(super) directory: Account,
    pub(super) project: wire::Project,
    pub(super) reasons: Vec<Reason>,
    pub(super) cases: Vec<wire::Dispute>,
    pub(super) live: bool,
}
pub(super) fn read(reader: &mut Storage<'_>, address: &Account) -> Result<StoredProject> {
    let (directory, id, phase): (Account, EntityId, u8) = reader.cell(address, 0u32)?;
    if !(1..=2).contains(&phase) {
        return Err(Error::InvalidState);
    }
    let record: ProjectRecord = reader.cell(address, 1u32)?;
    if record.project_id != id {
        return Err(Error::InvalidState);
    }
    let project = wire::Project {
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
        live: phase == 2,
        reasons: items(record.reasons, |i| reader.value(address, 22, i))?,
        cases: items(record.disputes, |i| reader.value(address, 23, i))?,
    })
}
fn proposal(reader: &mut Storage<'_>, address: &Account, p: u32) -> Result<wire::Proposal> {
    let record: ProposalRecord = reader.value(address, 10, p)?;
    Ok(wire::Proposal {
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
) -> Result<wire::Milestone> {
    let record: MilestoneRecord = reader.value(address, 11, pos)?;
    Ok(wire::Milestone {
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
            Ok(wire::Submission {
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

pub(super) fn records(
    project: wire::Project,
    reasons: Vec<Reason>,
    cases: Vec<wire::Dispute>,
) -> Result<Vec<Record>> {
    let mut records = vec![Record::Project(ProjectRecord {
        project_id: project.project_id,
        created_at: project.created_at,
        client: project.client,
        coordinator: project.coordinator,
        content: project.content,
        planning: project.planning,
        proposals: count(project.proposals.len())?,
        execution_escrow: project.execution_escrow,
        cancelled: project.cancelled,
        completed: project.completed,
        evaluations: count(project.evaluations.len())?,
        active_dispute_id: project.active_dispute_id,
        reasons: count(reasons.len())?,
        disputes: count(cases.len())?,
    })];
    for (p, proposal) in project.proposals.into_iter().enumerate() {
        let p = count(p)?;
        records.push(Record::Proposal(
            p,
            ProposalRecord {
                proposal_id: proposal.proposal_id,
                revision: proposal.revision,
                content: proposal.content,
                status: proposal.status,
                milestones: count(proposal.milestones.len())?,
                change_request: proposal.change_request,
                delivery: proposal
                    .delivery
                    .as_ref()
                    .map(StoredDelivery::from_view)
                    .transpose()?,
                compensation_policy: proposal.compensation_policy,
            },
        ));
        for (m, milestone) in proposal.milestones.into_iter().enumerate() {
            let pos = (p, count(m)?);
            if milestone.task_storage.milestone_id != milestone.milestone_id {
                return Err(Error::InvalidState);
            }
            records.push(Record::Milestone(
                pos,
                MilestoneRecord {
                    milestone_id: milestone.milestone_id,
                    key: milestone.definition.key,
                    window: milestone.definition.window,
                    coordinator_fee: milestone.definition.coordinator_fee,
                    coordinator_minutes: milestone.definition.coordinator_minutes,
                    requirements: count(milestone.definition.requirements.len())?,
                    status: milestone.status,
                    assignments: count(milestone.assignments.len())?,
                    worker_ratings: count(milestone.worker_ratings.len())?,
                    task_storage_id: milestone.task_storage.task_storage_id,
                    tasks: count(milestone.task_storage.tasks.len())?,
                    frozen: milestone.frozen,
                    submissions: count(milestone.submissions.len())?,
                },
            ));
            for (r, requirement) in milestone.definition.requirements.into_iter().enumerate() {
                let key = (pos, count(r)?);
                records.push(Record::Requirement(
                    key,
                    RequirementRecord {
                        key: requirement.key,
                        role_id: requirement.role_id,
                        skills: count(requirement.skill_ids.len())?,
                        minutes: requirement.minutes,
                        budget: requirement.budget,
                    },
                ));
                for (i, skill) in requirement.skill_ids.into_iter().enumerate() {
                    records.push(Record::Skill((key, count(i)?), skill));
                }
            }
            for (i, assignment) in milestone.assignments.into_iter().enumerate() {
                records.push(Record::Assignment((pos, count(i)?), assignment));
            }
            for (i, rating) in milestone.worker_ratings.into_iter().enumerate() {
                records.push(Record::MilestoneRating((pos, count(i)?), rating));
            }
            for (t, task) in milestone.task_storage.tasks.into_iter().enumerate() {
                let key = (pos, count(t)?);
                records.push(Record::Task(
                    key,
                    TaskRecord {
                        task_id: task.task_id,
                        assignees: count(task.assignees.len())?,
                    },
                ));
                for (i, assignee) in task.assignees.into_iter().enumerate() {
                    records.push(Record::Assignee((key, count(i)?), assignee));
                }
            }
            for (s, submission) in milestone.submissions.into_iter().enumerate() {
                let key = (pos, count(s)?);
                records.push(Record::Submission(
                    key,
                    SubmissionRecord {
                        submission_id: submission.submission_id,
                        version: submission.version,
                        deliverable: submission.deliverable,
                        submitted_by: submission.submitted_by,
                        submitted_at: submission.submitted_at,
                        worker_ratings: count(submission.worker_ratings.len())?,
                        review: submission.review,
                    },
                ));
                for (i, rating) in submission.worker_ratings.into_iter().enumerate() {
                    records.push(Record::SubmissionRating((key, count(i)?), rating));
                }
            }
        }
    }
    for (e, evaluation) in project.evaluations.into_iter().enumerate() {
        let e = count(e)?;
        records.push(Record::Evaluation(
            e,
            EvaluationRecord {
                author: evaluation.author,
                submitted_at: evaluation.submitted_at,
                ratings: count(evaluation.ratings.len())?,
            },
        ));
        for (i, rating) in evaluation.ratings.into_iter().enumerate() {
            records.push(Record::EvaluationRating((e, count(i)?), rating));
        }
    }
    for (i, reason) in reasons.into_iter().enumerate() {
        records.push(Record::Reason(count(i)?, reason));
    }
    for (i, case) in cases.into_iter().enumerate() {
        records.push(Record::Dispute(count(i)?, case));
    }
    Ok(records)
}
