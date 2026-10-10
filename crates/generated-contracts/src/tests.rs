use super::*;

fn sample_call() -> Result<UnsignedContractCallV1, ContractError> {
    UnsignedContractCallV1::new(
        ProviderInstanceId::from_bytes([1; 16]),
        OperationId::from_bytes([2; 16]),
        AccountId32::from_bytes([3; 32]),
        0,
        UnixSeconds::new(1000),
        ProviderCommand::CreateProject(CreateProjectRequest {
            title: "Build it".into(),
            description: String::new(),
        }),
    )
}

#[test]
fn scale_is_exact_bounded_versioned_and_instance_bound() -> Result<(), Box<dyn std::error::Error>> {
    let call = sample_call()?;
    let bytes = call.signable_bytes()?;
    assert_eq!(UnsignedContractCallV1::decode_signable(&bytes)?, call);
    assert_eq!(&bytes[..16], MOCK_SIGNING_DOMAIN);
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(UnsignedContractCallV1::decode_signable(&trailing).is_err());
    let mut altered = call.clone();
    altered.provider_instance_id = ProviderInstanceId::from_bytes([4; 16]);
    assert_ne!(bytes, altered.signable_bytes()?);
    altered.payload_version = PAYLOAD_VERSION + 1;
    assert!(altered.signable_bytes().is_err());
    altered.payload_version = PAYLOAD_VERSION;
    altered.signing_domain = [0; 16];
    assert!(UnsignedContractCallV1::decode_signable(&altered.encode()).is_err());
    let mut oversized = call;
    oversized.command = ProviderCommand::CreateProject(CreateProjectRequest {
        title: "x".repeat(MAX_SIGNABLE_BYTES),
        description: String::new(),
    });
    assert!(oversized.signable_bytes().is_err());
    Ok(())
}

#[test]
fn payload_fixture_and_json_commands_are_stable() -> Result<(), Box<dyn std::error::Error>> {
    let command = ProviderCommand::AcceptPlanningDelivery {
        project_id: EntityId::from_bytes([5; 16]),
        expected_revision: 7,
    };
    let value = serde_json::to_value(&command)?;
    assert_eq!(value["type"], "AcceptPlanningDelivery");
    assert_eq!(
        value["data"]["projectId"],
        EntityId::from_bytes([5; 16]).to_string()
    );
    assert!(value["data"].get("project_id").is_none());
    assert_eq!(value["data"]["expectedRevision"], 7);
    let call = sample_call()?;
    // SCALE: fixed domain16 + instance16 + version2 + operation16 + account32
    // + nonce8 + expiration8 + CreateProject index1 + title SCALElen1+8 + emptylen1.
    assert_eq!(call.signable_bytes()?.len(), 109);
    assert_eq!(call.signable_bytes()?[98], 9);
    assert!(
        serde_json::from_str::<RegisterRequest>(
            r#"{"username":"u","password":"p","displayName":"n","isAdmin":true}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ProviderCommand>(r#"{"type":"ArbitraryExtrinsic","data":[]}"#)
            .is_err()
    );
    Ok(())
}

#[test]
fn acceptance_requires_and_signs_the_observed_revision() -> Result<(), Box<dyn std::error::Error>> {
    let project_id = EntityId::from_bytes([5; 16]);
    let proposal_id = EntityId::from_bytes([6; 16]);
    let commands = [
        ProviderCommand::AcceptPlanningQuote {
            project_id,
            expected_revision: 7,
        },
        ProviderCommand::AcceptPlanningDelivery {
            project_id,
            expected_revision: 7,
        },
        ProviderCommand::ApproveExecution {
            project_id,
            proposal_id,
            expected_revision: 7,
        },
    ];
    for command in commands {
        let mut call = sample_call()?;
        call.command = command.clone();
        let before = call.signable_bytes()?;
        assert_eq!(UnsignedContractCallV1::decode_signable(&before)?, call);
        let mut json = serde_json::to_value(&command)?;
        json["data"]["expectedRevision"] = serde_json::json!(8);
        call.command = serde_json::from_value(json.clone())?;
        assert_ne!(before, call.signable_bytes()?);
        json["data"]
            .as_object_mut()
            .ok_or("command data missing")?
            .remove("expectedRevision");
        assert!(serde_json::from_value::<ProviderCommand>(json).is_err());
    }
    let request: RevisionRequest = serde_json::from_str(r#"{"expectedRevision":7}"#)?;
    assert_eq!(request.expected_revision, 7);
    assert!(serde_json::from_str::<RevisionRequest>("{}").is_err());
    Ok(())
}

#[test]
fn score_policy_does_not_round_or_accept_bad_weights() -> Result<(), Box<dyn std::error::Error>> {
    let policy = ScorePolicy::new(Percentage::new(50)?, Percentage::new(50)?)?;
    assert_eq!(policy.blend(Score::new(5)?, Score::new(4)?), 450);
    assert_eq!(policy.coordinator_client_vote_weight(), 1);
    assert_eq!(policy.coordinator_worker_vote_weight(), 1);
    let weighted = policy.with_coordinator_vote_weights(2, 1)?;
    assert_eq!(ScorePolicy::decode(&mut &weighted.encode()[..])?, weighted);
    assert_eq!(
        serde_json::from_value::<ScorePolicy>(serde_json::to_value(weighted)?)?,
        weighted
    );
    assert!(policy.with_coordinator_vote_weights(0, 0).is_err());
    assert!(policy.with_coordinator_vote_weights(101, 1).is_err());
    assert!(serde_json::from_str::<ScorePolicy>(r#"{"coordinatorPercent":50,"clientPercent":50,"coordinatorClientVoteWeight":0,"coordinatorWorkerVoteWeight":0}"#).is_err());

    assert!(ScorePolicy::new(Percentage::new(40)?, Percentage::new(40)?).is_err());
    assert!(ScorePolicy::decode(&mut &[40_u8, 40][..]).is_err());
    assert!(
        serde_json::from_str::<ScorePolicy>(r#"{"coordinatorPercent":80,"clientPercent":80}"#)
            .is_err()
    );
    assert!(serde_json::from_str::<WorkerRating>(r#"{"worker":"0x0303030303030303030303030303030303030303030303030303030303030303","score":11}"#).is_err());
    let view = ReputationView {
        weighted_score_sum: u128::MAX,
        rated_minutes: 7,
    };
    assert_eq!(
        serde_json::to_value(&view)?["weightedScoreSum"],
        u128::MAX.to_string()
    );
    Ok(())
}

#[test]
fn structural_validation_rejects_duplicate_slots_and_overflow()
-> Result<(), Box<dyn std::error::Error>> {
    let week = Week::new(2026, 40)?;
    let requirement = RequirementDefinition {
        key: 1,
        role_id: 2,
        skill_ids: vec![1, 2],
        minutes: Minutes::new(60),
        budget: Money::new(10),
    };
    let milestone = MilestoneDefinition {
        key: 1,
        title: "Ship".into(),
        window: WeekWindow::new(week, week)?,
        coordinator_fee: Money::new(2),
        coordinator_minutes: Minutes::new(5),
        requirements: vec![requirement.clone()],
    };
    let mut proposal = ProposalDefinition {
        title: "Plan".into(),
        description: String::new(),
        milestones: vec![milestone],
    };
    proposal.validate()?;
    assert_eq!(proposal.total()?, Money::new(12));
    proposal.milestones[0].requirements.push(requirement);
    assert!(proposal.validate().is_err());
    proposal.milestones[0].requirements.pop();
    proposal.milestones[0].coordinator_fee = Money::new(u64::MAX);
    assert!(proposal.validate().is_err());
    let calendar = CalendarDefinition {
        default_weekly_minutes: Minutes::new(60),
        overrides: vec![
            WeekOverride {
                week,
                capacity: Minutes::ZERO,
            },
            WeekOverride {
                week,
                capacity: Minutes::new(30),
            },
        ],
    };
    assert!(calendar.validate().is_err());
    Ok(())
}

#[test]
fn prepared_call_keeps_exact_bytes_until_signed() -> Result<(), Box<dyn std::error::Error>> {
    let call = sample_call()?;
    let prepared = PreparedCall::new(call.clone())?;
    assert_eq!(prepared.bytes(), call.signable_bytes()?);
    let signed = prepared.with_signature(Sr25519Signature::from_bytes([6; 64]));
    assert_eq!(signed.call, call);
    assert_eq!(signed.signature.as_bytes(), &[6; 64]);
    Ok(())
}

#[test]
fn debug_output_never_formats_credentials_or_session_tokens() {
    let login = LoginRequest {
        username: "private-login".into(),
        password: "secret-marker-password".into(),
    };
    let debug = format!("{login:?}");
    assert!(!debug.contains(&login.password));
    assert!(!debug.contains(&login.username));
    let session = SessionView {
        principal_id: PrincipalId::from_bytes([1; 16]),
        account_id: AccountId32::from_bytes([2; 32]),
        display_name: "person".into(),
        csrf_token: "secret-marker-csrf".into(),
        is_admin: false,
    };
    assert!(!format!("{session:?}").contains(&session.csrf_token));
}

#[test]
fn optional_deliveries_preserve_existing_command_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let project_id = EntityId::from_bytes([5; 16]);
    let milestone_id = EntityId::from_bytes([6; 16]);
    let request = RequestMilestoneCompletionRequest {
        worker_ratings: vec![],
        deliverable: EvidenceReference::new("https://example.test/old".into())?,
    };
    let old = ProviderCommand::RequestMilestoneCompletion {
        project_id,
        milestone_id,
        request: request.clone(),
    };
    let expected = (27_u8, project_id, milestone_id, request).encode();
    assert_eq!(old.encode(), expected);
    assert_eq!(ProviderCommand::decode(&mut expected.as_slice())?, old);
    let new = ProviderCommand::RequestMilestoneCompletionWithoutDeliverable {
        project_id,
        milestone_id,
        worker_ratings: vec![],
    };
    new.validate()?;
    assert_eq!(new.encode()[0], 35);
    assert_eq!(ProviderCommand::decode(&mut new.encode().as_slice())?, new);
    let body: SubmitMilestoneCompletionRequest = serde_json::from_str(r#"{"deliverable":null}"#)?;
    assert!(body.deliverable.is_none());
    assert!(
        serde_json::from_str::<SubmitMilestoneCompletionRequest>(r#"{"deliverable":{"url":""}}"#)
            .is_err()
    );
    let doc: serde_json::Value =
        serde_json::from_str(include_str!("../../../contracts/openapi.json"))?;
    let legacy: CompletionSubmission = serde_json::from_value(
        doc["components"]["schemas"]["CompletionSubmission"]["examples"][0].clone(),
    )?;
    assert!(legacy.deliverable.is_some());
    let mut absent = legacy;
    absent.deliverable = None;
    assert_eq!(
        serde_json::from_str::<CompletionSubmission>(&serde_json::to_string(&absent)?)?,
        absent
    );
    Ok(())
}

#[test]
fn proposal_dates_validate_real_calendar_dates() {
    for value in ["2026-02-29", "2026-2-09", "0000-01-01", ""] {
        assert!(
            DeliveryPreference::SpecificDate { date: value.into() }
                .validate()
                .is_err()
        );
    }
    assert!(
        DeliveryPreference::SpecificDate {
            date: "2028-02-29".into()
        }
        .validate()
        .is_ok()
    );
}
