//! Browser-independent input and outcome rules, exercised without a DOM.
use generated_contracts::{
    ExecutionOutcome, OperationView, ProviderOperationStatus, ReputationView, Week,
};

/// Parse a native week input without inventing a timezone or week boundary.
///
/// # Errors
/// Rejects malformed input and invalid ISO weeks (including invalid week 53).
pub fn parse_week(value: &str) -> Result<Week, String> {
    let (year, week) = value.split_once("-W").ok_or("Use YYYY-Www for a week")?;
    Week::new(
        year.parse().map_err(|_| "Invalid week year")?,
        week.parse().map_err(|_| "Invalid week number")?,
    )
    .map_err(|_| "Invalid ISO week".into())
}

/// Format a week for a native input.
#[must_use]
pub fn week_input(week: Week) -> String {
    format!("{:04}-W{:02}", week.iso_year(), week.week())
}

/// Parse a human-readable comma-separated ID list without silently losing items.
///
/// # Errors
/// Rejects invalid numbers, zero IDs, and duplicates.
pub fn parse_ids(value: &str) -> Result<Vec<u32>, String> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<u32> = value
        .split(',')
        .map(|id| id.trim().parse().map_err(|_| "Invalid catalog ID".into()))
        .collect::<Result<_, String>>()?;
    if ids.contains(&0) {
        return Err("Catalog IDs must be positive".into());
    }
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    if unique.len() != ids.len() {
        return Err("Duplicate catalog IDs".into());
    }
    Ok(ids)
}

/// The UI must distinguish a successful receipt from mere finality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationResult {
    /// A provider receipt establishes successful execution.
    Success,
    /// Non-execution or failed execution has been established.
    Failed(String),
    /// Keep the operation reference; never resubmit as a new business action.
    Pending,
}

/// Interpret finality without treating an unknown result as rejection.
#[must_use]
pub fn operation_result(operation: &OperationView) -> OperationResult {
    match operation.status {
        ProviderOperationStatus::Finalized => match &operation.receipt {
            Some(receipt) => match &receipt.outcome {
                ExecutionOutcome::Success => OperationResult::Success,
                ExecutionOutcome::Failed(code) => OperationResult::Failed(code.clone()),
            },
            None => OperationResult::Pending,
        },
        ProviderOperationStatus::Rejected | ProviderOperationStatus::Expired => {
            OperationResult::Failed(
                operation
                    .error_code
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", operation.status)),
            )
        }
        _ => OperationResult::Pending,
    }
}

/// Display-only truncation to two decimals; assignment uses provider exact scores.
#[must_use]
pub fn reputation_label(score: &ReputationView) -> String {
    if score.rated_minutes == 0 {
        return "5.00 (sin valoraciones)".into();
    }
    let hundredths = score.weighted_score_sum / u128::from(score.rated_minutes);
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use generated_contracts::*;

    #[test]
    fn validates_week_and_catalog_input() -> Result<(), String> {
        assert_eq!(week_input(parse_week("2026-W53")?), "2026-W53");
        assert!(parse_week("2025-W53").is_err());
        assert!(parse_week("2026-W00").is_err());
        assert_eq!(parse_ids("1, 2, 33")?, vec![1, 2, 33]);
        assert!(parse_ids("1,1").is_err());
        assert!(parse_ids("1,not-an-id").is_err());
        assert!(parse_ids("0").is_err());
        Ok(())
    }

    #[test]
    fn unknown_finality_is_never_success_or_rejection() {
        let mut view = OperationView {
            operation_id: OperationId::from_bytes([1; 16]),
            status: ProviderOperationStatus::OutcomeUnknown,
            receipt: None,
            error_code: None,
        };
        assert_eq!(operation_result(&view), OperationResult::Pending);
        view.status = ProviderOperationStatus::Finalized;
        assert_eq!(operation_result(&view), OperationResult::Pending);
        view.receipt = Some(OperationReceipt {
            operation_id: view.operation_id,
            provider_instance_id: ProviderInstanceId::from_bytes([2; 16]),
            origin: AccountId32::from_bytes([3; 32]),
            nonce: 0,
            outcome: ExecutionOutcome::Failed("insufficient_capacity".into()),
            created_entity_id: None,
            first_event_cursor: None,
            last_event_cursor: None,
            finalized_at: UnixSeconds::new(1),
        });
        assert_eq!(
            operation_result(&view),
            OperationResult::Failed("insufficient_capacity".into())
        );
        if let Some(receipt) = &mut view.receipt {
            receipt.outcome = ExecutionOutcome::Success;
        }
        assert_eq!(operation_result(&view), OperationResult::Success);
    }

    #[test]
    fn reputation_display_keeps_money_out_of_score() {
        assert_eq!(
            reputation_label(&ReputationView {
                weighted_score_sum: 1701,
                rated_minutes: 2
            }),
            "8.50"
        );
        assert_eq!(
            reputation_label(&ReputationView {
                weighted_score_sum: 0,
                rated_minutes: 0
            }),
            "5.00 (sin valoraciones)"
        );
    }
}
