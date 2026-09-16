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
