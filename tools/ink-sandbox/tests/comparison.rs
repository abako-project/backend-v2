use ink_v5_compatibility::worker::comparison::*;
use provider_execution::migration::ArchiveV1;
fn archive() -> ArchiveV1 {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../services/mock-provider/src/domain/fixtures/migration-signed.json"
    ))
    .unwrap();
    serde_json::from_str(fixture["archives"][5]["archive"].as_str().unwrap()).unwrap()
}
fn observed(archive: &ArchiveV1) -> Observation<'_> {
    Observation {
        archive: Some(archive),
        outcome: ObservedOutcome::Rejected {
            code: "invalid_nonce",
            infrastructure: false,
        },
    }
}
#[test]
fn complete_comparison_detects_private_metadata_counters_and_exact_scores() {
    let original = archive();
    assert!(matches!(
        compare(observed(&original), observed(&original)).unwrap(),
        Comparison::Agreement { .. }
    ));
    let mut metadata = original.clone();
    metadata
        .checkpoint
        .skill_metadata
        .values_mut()
        .next()
        .unwrap()
        .category
        .push('!');
    let mut counter = original.clone();
    counter.checkpoint.next_task_id += 1;
    let mut score = original.clone();
    score
        .checkpoint
        .workers
        .values_mut()
        .next()
        .unwrap()
        .worker_score
        .weighted_score_sum = u128::from(u64::MAX) * 500;
    for changed in [metadata, counter, score] {
        assert_eq!(
            compare(observed(&original), observed(&changed)).unwrap(),
            Comparison::Difference {
                state: true,
                outcome: false
            }
        );
    }
}
#[test]
fn missing_coverage_and_unreadable_exports_are_incomplete() {
    let original = archive();
    for incomplete in [
        Observation {
            archive: None,
            outcome: ObservedOutcome::Rejected {
                code: "invalid_nonce",
                infrastructure: false,
            },
        },
        Observation {
            archive: Some(&original),
            outcome: ObservedOutcome::Incomplete,
        },
    ] {
        assert_eq!(
            compare(observed(&original), incomplete).unwrap(),
            Comparison::Incomplete
        );
    }
    let mut incompatible = original.clone();
    incompatible.version = 2;
    assert_eq!(
        compare(observed(&original), observed(&incompatible)).unwrap(),
        Comparison::Incomplete
    );
}
#[test]
fn different_failure_classes_cannot_agree_on_unchanged_state() {
    let original = archive();
    assert_eq!(
        compare(
            observed(&original),
            Observation {
                archive: Some(&original),
                outcome: ObservedOutcome::Rejected {
                    code: "invalid_nonce",
                    infrastructure: true
                }
            }
        )
        .unwrap(),
        Comparison::Difference {
            state: false,
            outcome: true
        }
    );
}
