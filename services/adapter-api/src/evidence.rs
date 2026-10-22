//! Commit existing immutable adapter records; resolve only authorized/public views.
use crate::{
    auth::Session,
    state::{App, Error},
};
use generated_contracts::{
    AccountId32, CompletionSubmission, Dispute, EntityId, EvidenceCommitmentV1, EvidencePurposeV1,
    EvidenceRecordReferenceV1, EvidenceReference, OperationId, PayloadHash, ProjectView,
    ProviderCommand, ProviderInstanceId, SubmissionReview,
};
use parity_scale_codec::Encode;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};

#[derive(Clone, Encode)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "SCALE indexes two fixed variants."
)]
enum Body {
    #[codec(index = 0)]
    Url(String),
    #[codec(index = 1)]
    Text(String),
}
#[derive(Encode)]
struct Record {
    version: u8,
    generation: ProviderInstanceId,
    project: EntityId,
    purpose: EvidencePurposeV1,
    target: EntityId,
    id: EntityId,
    author: AccountId32,
    body: Body,
}
impl Record {
    fn hash(&self) -> PayloadHash {
        let mut hasher = Sha256::new();
        hasher.update(b"abako/evidence/v1\0");
        hasher.update(self.encode());
        PayloadHash::from_bytes(hasher.finalize().into())
    }
    fn commitment(&self) -> EvidenceCommitmentV1 {
        EvidenceCommitmentV1 {
            record_id: self.id,
            hash: self.hash(),
        }
    }
}
fn tag(purpose: EvidencePurposeV1) -> i16 {
    match purpose {
        EvidencePurposeV1::Delivery => 0,
        EvidencePurposeV1::Rejection => 1,
        EvidencePurposeV1::Opening => 2,
        EvidencePurposeV1::Response => 3,
    }
}

pub(crate) fn needs_commitment(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::SubmitMilestoneDelivery {
            request: generated_contracts::SubmitMilestoneCompletionRequest {
                deliverable: Some(_)
            },
            ..
        } | ProviderCommand::RequestMilestoneCompletion { .. }
            | ProviderCommand::RejectMilestoneCompletion { .. }
            | ProviderCommand::RejectMilestoneDelivery { .. }
            | ProviderCommand::OpenDispute(_)
            | ProviderCommand::OpenDisputeWithComment(_)
            | ProviderCommand::RespondDispute { .. }
    )
}

/// Runs inside the enqueue transaction after authorization and queue admission.
/// Existing queued/signed commands never pass through this conversion.
#[allow(
    clippy::too_many_lines,
    reason = "Keep the explicit conversion of existing signed paths together; no new dispatch abstraction."
)]
pub(crate) async fn prepare(
    connection: &mut PgConnection,
    session: &Session,
    generation: ProviderInstanceId,
    operation: OperationId,
    command: ProviderCommand,
) -> Result<ProviderCommand, Error> {
    use EvidencePurposeV1 as Purpose;
    use ProviderCommand as Command;
    let Some(project) = command.project_id() else {
        return Ok(command);
    };
    let author = session.view.account_id;
    let new_record_id = EntityId::from_bytes(operation.into_bytes());
    let (purpose, target, record_id, body) = match &command {
        Command::SubmitMilestoneDelivery {
            milestone_id,
            request,
            ..
        } => {
            let Some(reference) = &request.deliverable else {
                return Ok(command);
            };
            (
                Purpose::Delivery,
                *milestone_id,
                new_record_id,
                Body::Url(reference.url().to_owned()),
            )
        }
        Command::RequestMilestoneCompletion {
            milestone_id,
            request,
            ..
        } => (
            Purpose::Delivery,
            *milestone_id,
            new_record_id,
            Body::Url(request.deliverable.url().to_owned()),
        ),
        Command::RejectMilestoneCompletion {
            submission_id,
            request,
            ..
        } => (
            Purpose::Rejection,
            *submission_id,
            new_record_id,
            Body::Url(request.evidence.url().to_owned()),
        ),
        Command::OpenDispute(request) => (
            Purpose::Opening,
            request.rejected_submission_id,
            new_record_id,
            Body::Url(request.evidence.url().to_owned()),
        ),
        Command::RespondDispute {
            dispute_id,
            request,
            ..
        } => (
            Purpose::Response,
            *dispute_id,
            new_record_id,
            Body::Url(request.evidence.url().to_owned()),
        ),
        Command::RejectMilestoneDelivery {
            submission_id,
            comment_id,
            ..
        } => {
            let row = sqlx::query("SELECT author_account,message,kind FROM submission_comments WHERE provider_instance_id=$1 AND submission_id=$2 AND comment_id=$3")
                .bind(generation.to_string()).bind(submission_id.to_string()).bind(comment_id.to_string())
                .fetch_optional(&mut *connection).await?.ok_or(Error::NotFound)?;
            if row.try_get::<String, _>("author_account")? != author.to_string()
                || row.try_get::<String, _>("kind")? != "Rejection"
            {
                return Err(Error::Forbidden);
            }
            (
                Purpose::Rejection,
                *submission_id,
                *comment_id,
                Body::Text(row.try_get("message")?),
            )
        }
        Command::OpenDisputeWithComment(request) => {
            let row = sqlx::query("SELECT * FROM dispute_opening_arguments WHERE provider_instance_id=$1 AND comment_id=$2")
                .bind(generation.to_string()).bind(request.comment_id.to_string()).fetch_optional(&mut *connection).await?.ok_or(Error::NotFound)?;
            for (column, expected) in [
                ("project_id", project.to_string()),
                ("milestone_id", request.milestone_id.to_string()),
                ("submission_id", request.rejected_submission_id.to_string()),
                ("author_account", author.to_string()),
            ] {
                if row.try_get::<String, _>(column)? != expected {
                    return Err(Error::Forbidden);
                }
            }
            (
                Purpose::Opening,
                request.rejected_submission_id,
                request.comment_id,
                Body::Text(row.try_get("reason")?),
            )
        }
        _ => return Ok(command),
    };
    if let Body::Url(url) = &body {
        // Existing public input validation is preserved, including exact bytes.
        EvidenceReference::new(url.clone()).map_err(|_| Error::Invalid)?;
        sqlx::query("INSERT INTO evidence_url_records(provider_instance_id,project_id,purpose,target_id,record_id,author_account,url) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
            .bind(generation.to_string()).bind(project.to_string()).bind(tag(purpose)).bind(target.to_string())
            .bind(record_id.to_string()).bind(author.to_string()).bind(url).execute(&mut *connection).await?;
        let row=sqlx::query("SELECT author_account,url FROM evidence_url_records WHERE provider_instance_id=$1 AND project_id=$2 AND purpose=$3 AND target_id=$4 AND record_id=$5")
            .bind(generation.to_string()).bind(project.to_string()).bind(tag(purpose)).bind(target.to_string()).bind(record_id.to_string())
            .fetch_one(&mut *connection).await?;
        if row.try_get::<String, _>("author_account")? != author.to_string()
            || row.try_get::<String, _>("url")? != *url
        {
            return Err(Error::Conflict("evidence_record_conflict"));
        }
    }
    let evidence = Record {
        version: 1,
        generation,
        project,
        purpose,
        target,
        id: record_id,
        author,
        body,
    }
    .commitment();
    Ok(match command {
        Command::SubmitMilestoneDelivery { milestone_id, .. } => {
            Command::SubmitMilestoneCommitted {
                project_id: project,
                milestone_id,
                evidence,
                worker_ratings: None,
            }
        }
        Command::RequestMilestoneCompletion {
            milestone_id,
            request,
            ..
        } => Command::SubmitMilestoneCommitted {
            project_id: project,
            milestone_id,
            evidence,
            worker_ratings: Some(request.worker_ratings),
        },
        Command::RejectMilestoneCompletion {
            milestone_id,
            submission_id,
            ..
        }
        | Command::RejectMilestoneDelivery {
            milestone_id,
            submission_id,
            ..
        } => Command::RejectMilestoneCommitted {
            project_id: project,
            milestone_id,
            submission_id,
            evidence,
        },
        Command::OpenDispute(request) => Command::OpenDisputeCommitted {
            project_id: project,
            milestone_id: request.milestone_id,
            rejected_submission_id: request.rejected_submission_id,
            evidence,
        },
        Command::OpenDisputeWithComment(request) => Command::OpenDisputeCommitted {
            project_id: project,
            milestone_id: request.milestone_id,
            rejected_submission_id: request.rejected_submission_id,
            evidence,
        },
        Command::RespondDispute { dispute_id, .. } => Command::RespondDisputeCommitted {
            project_id: project,
            dispute_id,
            evidence,
        },
        _ => return Err(Error::Internal),
    })
}

async fn resolve(
    app: &App,
    generation: ProviderInstanceId,
    project: EntityId,
    purpose: EvidencePurposeV1,
    target: EntityId,
    author: AccountId32,
    reference: &EvidenceRecordReferenceV1,
) -> Result<Body, Error> {
    let EvidenceRecordReferenceV1::Committed(commitment) = reference else {
        return Ok(Body::Url(
            reference.url().ok_or(Error::Dependency)?.to_owned(),
        ));
    };
    let record_id = commitment.record_id;
    let row=sqlx::query("SELECT author_account,url FROM evidence_url_records WHERE provider_instance_id=$1 AND project_id=$2 AND purpose=$3 AND target_id=$4 AND record_id=$5")
        .bind(generation.to_string()).bind(project.to_string()).bind(tag(purpose)).bind(target.to_string()).bind(record_id.to_string())
        .fetch_optional(&app.db).await?;
    let mut candidates = Vec::with_capacity(2);
    if let Some(row) = row
        && row.try_get::<String, _>("author_account")? == author.to_string()
    {
        candidates.push(Body::Url(row.try_get("url")?));
    }
    match purpose {
        EvidencePurposeV1::Rejection => {
            let row=sqlx::query("SELECT message FROM submission_comments WHERE provider_instance_id=$1 AND submission_id=$2 AND comment_id=$3 AND author_account=$4 AND kind='Rejection'")
                .bind(generation.to_string()).bind(target.to_string()).bind(record_id.to_string()).bind(author.to_string()).fetch_optional(&app.db).await?;
            if let Some(row) = row {
                candidates.push(Body::Text(row.try_get("message")?));
            }
        }
        EvidencePurposeV1::Opening => {
            let row=sqlx::query("SELECT reason FROM dispute_opening_arguments WHERE provider_instance_id=$1 AND project_id=$2 AND submission_id=$3 AND comment_id=$4 AND author_account=$5")
                .bind(generation.to_string()).bind(project.to_string()).bind(target.to_string()).bind(record_id.to_string()).bind(author.to_string()).fetch_optional(&app.db).await?;
            if let Some(row) = row {
                candidates.push(Body::Text(row.try_get("reason")?));
            }
        }
        _ => {}
    }
    for body in candidates {
        let record = Record {
            version: 1,
            generation,
            project,
            purpose,
            target,
            id: record_id,
            author,
            body,
        };
        if record.hash() == commitment.hash {
            return Ok(record.body);
        }
    }
    Err(Error::Dependency)
}
fn url(body: Body) -> Result<EvidenceRecordReferenceV1, Error> {
    match body {
        Body::Url(url) => Ok(EvidenceReference::new(url)
            .map_err(|_| Error::Dependency)?
            .into()),
        Body::Text(_) => Err(Error::Dependency),
    }
}

/// Hydrate only a project already selected by existing visibility rules.
pub(crate) async fn project(
    app: &App,
    generation: ProviderInstanceId,
    project: &mut ProjectView,
) -> Result<(), Error> {
    for milestone in project.proposals.iter_mut().flat_map(|p| &mut p.milestones) {
        for item in &mut milestone.submissions {
            submission(
                app,
                generation,
                project.project_id,
                milestone.milestone_id,
                item,
            )
            .await?;
        }
    }
    Ok(())
}

pub(crate) async fn submission(
    app: &App,
    generation: ProviderInstanceId,
    project: EntityId,
    milestone: EntityId,
    item: &mut CompletionSubmission,
) -> Result<(), Error> {
    if let Some(reference) = &item.deliverable {
        item.deliverable = Some(url(resolve(
            app,
            generation,
            project,
            EvidencePurposeV1::Delivery,
            milestone,
            item.submitted_by,
            reference,
        )
        .await?)?);
    }
    if let SubmissionReview::Rejected {
        reason,
        reviewed_by,
        reviewed_at,
    } = &item.review
    {
        let body = resolve(
            app,
            generation,
            project,
            EvidencePurposeV1::Rejection,
            item.submission_id,
            *reviewed_by,
            reason,
        )
        .await?;
        item.review = match body {
            Body::Url(value) => SubmissionReview::Rejected {
                reason: url(Body::Url(value))?,
                reviewed_by: *reviewed_by,
                reviewed_at: *reviewed_at,
            },
            Body::Text(_) => {
                let EvidenceRecordReferenceV1::Committed(reference) = reason else {
                    return Err(Error::Dependency);
                };
                SubmissionReview::RejectedWithComment {
                    comment_id: reference.record_id,
                    reviewed_by: *reviewed_by,
                    reviewed_at: *reviewed_at,
                }
            }
        };
    }
    Ok(())
}

pub(crate) async fn case(
    app: &App,
    generation: ProviderInstanceId,
    view: &mut generated_contracts::DisputeView,
) -> Result<(), Error> {
    dispute(app, generation, &mut view.dispute).await?;
    for item in &mut view.milestone.submissions {
        submission(
            app,
            generation,
            view.dispute.project_id,
            view.milestone.milestone_id,
            item,
        )
        .await?;
    }
    Ok(())
}

/// Hydrate the requested public case; authorization continues using raw snapshots.
pub(crate) async fn dispute(
    app: &App,
    generation: ProviderInstanceId,
    case: &mut Dispute,
) -> Result<(), Error> {
    if let Some(reference) = &case.evidence {
        let body = resolve(
            app,
            generation,
            case.project_id,
            EvidencePurposeV1::Opening,
            case.rejected_submission_id,
            case.opened_by,
            reference,
        )
        .await?;
        match body {
            Body::Url(value) => case.evidence = Some(url(Body::Url(value))?),
            Body::Text(_) => {
                let EvidenceRecordReferenceV1::Committed(reference) = reference else {
                    return Err(Error::Dependency);
                };
                case.opening_comment_id = Some(reference.record_id);
                case.evidence = None;
            }
        }
    }
    if let Some(response) = &mut case.response {
        response.evidence = url(resolve(
            app,
            generation,
            case.project_id,
            EvidencePurposeV1::Response,
            case.dispute_id,
            response.author,
            &response.evidence,
        )
        .await?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;
    #[test]
    fn commitment_vector_binds_context_kind_author_and_exact_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../specs/0014-evidence-commitments/hash-vectors.json"
        ))?;
        let record = Record {
            version: 1,
            generation: ProviderInstanceId::from_bytes([1; 16]),
            project: EntityId::from_bytes([2; 16]),
            purpose: EvidencePurposeV1::Rejection,
            target: EntityId::from_bytes([3; 16]),
            id: EntityId::from_bytes([4; 16]),
            author: AccountId32::from_bytes([5; 32]),
            body: Body::Text("ipfs://notes — explicación".into()),
        };
        assert_eq!(record.hash().to_string(), vector["recordV1"]["hash"]);
        let mut bytes = String::new();
        for byte in record.encode() {
            write!(&mut bytes, "{byte:02x}")?;
        }
        assert_eq!(bytes, vector["recordV1"]["encodedHex"]);
        let baseline = record.hash();
        for index in 0..8 {
            let mut changed = Record {
                version: record.version,
                generation: record.generation,
                project: record.project,
                purpose: record.purpose,
                target: record.target,
                id: record.id,
                author: record.author,
                body: record.body.clone(),
            };
            match index {
                0 => changed.version = 2,
                1 => changed.generation = ProviderInstanceId::from_bytes([9; 16]),
                2 => changed.project = EntityId::from_bytes([9; 16]),
                3 => changed.purpose = EvidencePurposeV1::Opening,
                4 => changed.target = EntityId::from_bytes([9; 16]),
                5 => changed.id = EntityId::from_bytes([9; 16]),
                6 => changed.author = AccountId32::from_bytes([9; 32]),
                _ => changed.body = Body::Text("ipfs://notes — explicacio\u{301}n".into()),
            }
            assert_ne!(baseline, changed.hash());
        }
        let as_url = Record {
            body: Body::Url("ipfs://notes — explicación".into()),
            ..record
        };
        assert_ne!(baseline, as_url.hash());
        Ok(())
    }
}
