//! Read-only SPEC-0015 inventory of existing fixture SCALE records and headers.
use generated_contracts::{
    Dispute, EvidenceRecordReferenceV1, PayloadHash, ProjectView, SubmissionReview, WorkerView,
};
use parity_scale_codec::Encode;
use provider_execution::migration::ArchiveV1;
use serde_json::{Value, json};
use std::{collections::BTreeMap, error::Error, io::Write};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Sizes = BTreeMap<&'static str, usize>;
fn maximum(sizes: &mut Sizes, key: &'static str, value: usize) {
    sizes
        .entry(key)
        .and_modify(|max| *max = (*max).max(value))
        .or_insert(value);
}
fn reference_delta(reference: &EvidenceRecordReferenceV1) -> usize {
    // Native legacy strings become the canonical ABI's 33-byte enum/hash;
    // native committed references are 50 bytes, canonical references 49.
    reference.encoded_size()
        - match reference {
            EvidenceRecordReferenceV1::Legacy(_) => 33,
            EvidenceRecordReferenceV1::Committed(_) => 49,
        }
}
fn project_delta(project: &ProjectView) -> usize {
    project
        .proposals
        .iter()
        .flat_map(|p| &p.milestones)
        .flat_map(|m| &m.submissions)
        .map(|s| {
            s.deliverable.as_ref().map_or(0, reference_delta)
                + match &s.review {
                    SubmissionReview::Rejected { reason, .. } => reference_delta(reason),
                    _ => 0,
                }
        })
        .sum()
}
fn case_delta(case: &Dispute) -> usize {
    case.evidence.as_ref().map_or(0, reference_delta)
        + case
            .response
            .as_ref()
            .map_or(0, |r| reference_delta(&r.evidence))
}
#[allow(
    clippy::too_many_lines,
    reason = "Keep the read-only field inventory together; this introduces no runtime state."
)]
fn inventory(archive: &ArchiveV1) -> Sizes {
    let mut sizes = Sizes::new();
    let c = &archive.checkpoint;
    for entry in c.roles.values().chain(c.skills.values()) {
        maximum(&mut sizes, "catalog_entry", entry.encoded_size());
        maximum(&mut sizes, "catalog_label_utf8", entry.name.len());
    }
    for worker in c.workers.values() {
        worker_sizes(&mut sizes, worker);
    }
    for project in c.projects.values() {
        let reasons: Vec<_> = c
            .reasons
            .iter()
            .filter(|r| r.project_id == project.project_id)
            .map(|r| {
                (
                    r.project_id,
                    r.milestone_id,
                    r.origin,
                    &r.reason,
                    r.occurred_at,
                    r.kind,
                )
            })
            .collect();
        let cases: Vec<_> = c
            .disputes
            .values()
            .filter(|d| d.project_id == project.project_id)
            .collect();
        let delta = project_delta(project) + cases.iter().map(|d| case_delta(d)).sum::<usize>();
        // Exact ProjectData layout at canonical139a01f: Directory account,
        // ProjectView, local reasons, cases, live. Text adapters preserve bytes.
        let root =
            32 + project.encoded_size() + reasons.encoded_size() + cases.encoded_size() + 1 - delta;
        maximum(&mut sizes, "project_root_canonical", root);
        maximum(&mut sizes, "project_view_native", project.encoded_size());
        maximum(
            &mut sizes,
            "project_description_utf8",
            project.description.len(),
        );
        for proposal in &project.proposals {
            maximum(&mut sizes, "proposal_view", proposal.encoded_size());
            maximum(
                &mut sizes,
                "proposal_description_utf8",
                proposal.description.len(),
            );
            for milestone in &proposal.milestones {
                maximum(&mut sizes, "milestone_view", milestone.encoded_size());
                maximum(
                    &mut sizes,
                    "task_storage",
                    milestone.task_storage.encoded_size(),
                );
                for task in &milestone.task_storage.tasks {
                    maximum(&mut sizes, "task_view", task.encoded_size());
                }
                for requirement in &milestone.definition.requirements {
                    maximum(&mut sizes, "requirement", requirement.encoded_size());
                    maximum(
                        &mut sizes,
                        "candidate_requirement_without_skill_memberships",
                        (
                            requirement.key,
                            requirement.role_id,
                            requirement.minutes,
                            requirement.budget,
                        )
                            .encoded_size(),
                    );
                }
            }
        }
        let mut stripped = project.clone();
        stripped.title.clear();
        stripped.description.clear();
        for proposal in &mut stripped.proposals {
            proposal.title.clear();
            proposal.description.clear();
            proposal.change_request = None;
            for milestone in &mut proposal.milestones {
                milestone.definition.title.clear();
                milestone.task_storage.tasks.clear();
            }
        }
        maximum(
            &mut sizes,
            "project_view_without_text_or_tasks_lower_bound",
            stripped.encoded_size() - project_delta(&stripped),
        );
        maximum(
            &mut sizes,
            "candidate_project_header",
            (
                project.project_id,
                project.created_at,
                project.client,
                project.coordinator,
                &project.planning,
                project.execution_escrow,
                project.cancelled,
                project.completed,
                project.active_dispute_id,
                0u64,
                PayloadHash::from_bytes([0; 32]),
            )
                .encoded_size(),
        );
    }
    // These illustrate why even compact IDs cannot live in one growing vector.
    maximum(
        &mut sizes,
        "directory_worker_ids_vector",
        c.workers.keys().collect::<Vec<_>>().encoded_size(),
    );
    maximum(
        &mut sizes,
        "directory_skill_ids_vector",
        c.skills.keys().collect::<Vec<_>>().encoded_size(),
    );
    sizes
}
fn worker_sizes(sizes: &mut Sizes, worker: &WorkerView) {
    maximum(sizes, "worker_display_name_utf8", worker.display_name.len());
    maximum(sizes, "worker_view", worker.encoded_size());
    maximum(
        sizes,
        "qualifications",
        worker.qualifications.encoded_size(),
    );
    maximum(
        sizes,
        "calendar_root_canonical",
        worker.calendar.encoded_size() + 32 + 32 + 1,
    );
    maximum(
        sizes,
        "calendar_reservations_vector",
        worker.calendar.reservations.encoded_size(),
    );
    maximum(
        sizes,
        "calendar_overrides_vector",
        worker.calendar.definition.overrides.encoded_size(),
    );
    maximum(
        sizes,
        "candidate_worker_header",
        (
            worker.account,
            worker.mode,
            worker.coordinator_eligible,
            &worker.worker_score,
            &worker.coordinator_score,
            worker.calendar.calendar_id,
        )
            .encoded_size(),
    );
    maximum(
        sizes,
        "candidate_calendar_header",
        (
            worker.calendar.calendar_id,
            worker.account,
            worker.calendar.definition.default_weekly_minutes,
            0u64,
        )
            .encoded_size(),
    );
    for reservation in &worker.calendar.reservations {
        maximum(sizes, "reservation_record", reservation.encoded_size());
    }
}
fn main() -> Result<()> {
    let fixture: Value =
        serde_json::from_str(include_str!("../src/domain/fixtures/migration-signed.json"))?;
    let mut records = Vec::new();
    for (group, items) in fixture.as_object().ok_or("fixture object")? {
        if group == "digests" {
            continue;
        }
        for item in items.as_array().ok_or("fixture cases")? {
            let archive: ArchiveV1 =
                serde_json::from_str(item["archive"].as_str().ok_or("archive")?)?;
            let label = item
                .get("label")
                .cloned()
                .unwrap_or_else(|| item["beforeStep"].clone());
            records.push(json!({"group":group,"case":label,"maxBytes":inventory(&archive)}));
        }
    }
    let report = json!({
        "source": "SPEC-0015 stage0; canonical runtime139a01f; existing migration-signed fixtures",
        "units": "SCALE encoded bytes except explicit utf8 fields; each number is a per-case maximum",
        "candidateHeaders": "Illustrative tuples, not a finalized ABI or full replacement-state size. Child collections would be separate mappings. Stripped views omit future commitments and remain nested.",
        "syntheticIndexGrowth": {"1000_account_ids": vec![[0u8;32];1000].encoded_size(), "1000_public_ids": vec![[0u8;16];1000].encoded_size()},
        "cases": records,
    });
    let mut output = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut output, &report)?;
    writeln!(output)?;
    Ok(())
}
