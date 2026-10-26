//! Whole-state shadow diagnostic. Missing exports or command coverage can never
//! count as agreement, including when both available projections look identical.
use super::Error;
use generated_contracts::OperationReceipt;
use provider_execution::migration::CoreArchiveV5;

#[derive(Clone, Copy)]
pub enum ObservedOutcome<'a> {
    Receipt(&'a OperationReceipt),
    Rejected { code: &'a str, infrastructure: bool },
    Incomplete,
}
pub struct Observation<'a> {
    pub archive: Option<&'a CoreArchiveV5>,
    pub outcome: ObservedOutcome<'a>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Comparison {
    Agreement { archive_sha256: [u8; 32] },
    Difference { state: bool, outcome: bool },
    Incomplete,
}
/// Both executions must use the same captured operation inputs. This function
/// compares complete, validated native-compatible archives and exact outcomes;
/// it never substitutes a public snapshot or reruns a failed command.
pub fn compare(
    reference: Observation<'_>,
    candidate: Observation<'_>,
) -> Result<Comparison, Error> {
    let (Some(reference_state), Some(candidate_state)) = (reference.archive, candidate.archive)
    else {
        return Ok(Comparison::Incomplete);
    };
    if [reference_state, candidate_state]
        .iter()
        .any(|archive| archive.version != 5 || archive.checkpoint.engine_version != 5)
    {
        return Ok(Comparison::Incomplete);
    }
    let outcome = match (reference.outcome, candidate.outcome) {
        (ObservedOutcome::Incomplete, _) | (_, ObservedOutcome::Incomplete) => {
            return Ok(Comparison::Incomplete);
        }
        (ObservedOutcome::Receipt(left), ObservedOutcome::Receipt(right)) => left != right,
        (
            ObservedOutcome::Rejected {
                code: left,
                infrastructure: left_class,
            },
            ObservedOutcome::Rejected {
                code: right,
                infrastructure: right_class,
            },
        ) => left != right || left_class != right_class,
        _ => true,
    };
    let left = serde_json::to_vec(reference_state).map_err(|_| Error::InvalidState)?;
    let right = serde_json::to_vec(candidate_state).map_err(|_| Error::InvalidState)?;
    if left.len() > super::MAX_SHELL || right.len() > super::MAX_SHELL {
        return Err(Error::Limit);
    }
    let state = left != right;
    Ok(if state || outcome {
        Comparison::Difference { state, outcome }
    } else {
        Comparison::Agreement {
            archive_sha256: sp_core::hashing::sha2_256(&left),
        }
    })
}
