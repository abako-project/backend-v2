use super::*;
use generated_contracts::provider_core::*;
use serde_json::Value;
#[test]
fn catalog_and_reason_vectors_preserve_context_and_unicode() -> TestResult {
    for (fixture, catalog) in [
        (
            include_str!("../../../specs/0015-adapter-core-boundary/catalog-metadata-vectors.json"),
            true,
        ),
        (
            include_str!("../../../specs/0015-adapter-core-boundary/reason-metadata-vectors.json"),
            false,
        ),
    ] {
        let fixture: Value = serde_json::from_str(fixture)?;
        for vector in fixture["vectors"].as_array().ok_or("vectors")? {
            let bytes = if catalog {
                serde_json::from_value::<CatalogMetadataRecordV1>(vector["record"].clone())?
                    .hash_input()
            } else {
                serde_json::from_value::<ReasonMetadataRecordV1>(vector["record"].clone())?
                    .hash_input()
            };
            let mut encoded = String::new();
            for byte in &bytes {
                use std::fmt::Write as _;
                write!(&mut encoded, "{byte:02x}")?;
            }
            assert_eq!(encoded, vector["hashInputHex"]);
            assert_eq!(
                PayloadHash::from_bytes(hash(&bytes)).to_string(),
                vector["sha256"]
            );
        }
    }
    assert_eq!(
        CatalogMetadataRecordV1::normalized("  DéCENTRAL\t\u{2003}Space \n"),
        "décentral space"
    );
    assert_eq!(
        CatalogMetadataRecordV1::normalized("AΣ\u{a7ce}"),
        "aσ\u{a7cf}"
    );
    assert_eq!(CatalogMetadataRecordV1::normalized(" X\0Y "), "x\0y");
    assert!(
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "é".repeat(50),
            role_ids: vec![1]
        })
        .validate()
        .is_ok()
    );
    assert!(
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "é".repeat(51),
            role_ids: vec![1]
        })
        .validate()
        .is_err()
    );
    assert!(
        ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
            kind: CatalogKind::Skill,
            id: 1,
            name: "λ\0".repeat(15_000)
        })
        .validate()
        .is_ok()
    );
    Ok(())
}

// Deliberately small transport fixture: supplied core facts/IDs, not a second
// provider engine. Real permission, allocation and WASM parity tests live there.
pub(super) fn apply_fixture_facts(
    fake: &mut Fake,
    call: &UnsignedContractCallV1,
) -> Option<EntityId> {
    match &call.command {
        ProviderCommand::UpsertCatalogEntryCore { kind, id, content } => {
            let entries = if *kind == CatalogKind::Role {
                &mut fake.snapshot.catalog.roles
            } else {
                &mut fake.snapshot.catalog.skills
            };
            let entry = CatalogEntryCore {
                id: *id,
                fixed: false,
                content: CatalogReferenceV1::Write(content.clone()),
            };
            if let Some(existing) = entries.iter_mut().find(|e| e.id == *id) {
                *existing = entry;
            } else {
                entries.push(entry);
            }
            if *kind == CatalogKind::Skill
                && !fake
                    .snapshot
                    .catalog
                    .skill_roles
                    .iter()
                    .any(|s| s.skill_id == *id)
            {
                fake.snapshot.catalog.skill_roles.push(SkillRoleCore {
                    skill_id: *id,
                    role_ids: vec![],
                    origin: CatalogReferenceV1::Write(content.clone()),
                });
            }
            if fake.fail_catalog_completion {
                fake.fail_catalog_completion = false;
                fake.snapshot_failures = 1;
            }
            None
        }
        ProviderCommand::CreateSkillRequestCore { role_ids, content } => {
            let id = EntityId::from_bytes(call.operation_id.into_bytes());
            fake.catalog_requests.push(SkillRequestCore {
                request_id: id,
                requester: call.origin,
                content: CatalogReferenceV1::Write(content.clone()),
                role_ids: role_ids.clone(),
                status: SkillRequestStatus::Pending,
                skill_id: None,
            });
            Some(id)
        }
        ProviderCommand::DecideSkillRequest(request) => {
            if let Some(marker) = fake
                .catalog_requests
                .iter_mut()
                .find(|r| r.request_id == request.request_id)
            {
                marker.status = if request.decision == SkillRequestDecision::Approve {
                    SkillRequestStatus::Approved
                } else {
                    SkillRequestStatus::Rejected
                };
                if request.decision == SkillRequestDecision::Approve {
                    // Explicitly supplied authoritative ID, unlike an API prediction.
                    marker.skill_id = Some(973);
                    fake.snapshot.catalog.skills.push(CatalogEntryCore {
                        id: 973,
                        fixed: false,
                        content: marker.content.clone(),
                    });
                    fake.snapshot.catalog.skill_roles.push(SkillRoleCore {
                        skill_id: 973,
                        role_ids: marker.role_ids.clone(),
                        origin: marker.content.clone(),
                    });
                }
            }
            Some(request.request_id)
        }
        ProviderCommand::DeleteCatalogEntry(request) => {
            let entries = if request.kind == CatalogKind::Role {
                &mut fake.snapshot.catalog.roles
            } else {
                &mut fake.snapshot.catalog.skills
            };
            entries.retain(|e| e.id != request.id);
            if request.kind == CatalogKind::Skill {
                fake.snapshot
                    .catalog
                    .skill_roles
                    .retain(|e| e.skill_id != request.id);
            }
            None
        }
        _ => None,
    }
}
async fn administrator(app: &App) -> Result<Session, Box<dyn std::error::Error>> {
    let response = auth::login(
        app,
        LoginRequest {
            username: "admin".into(),
            password: PASSWORD.into(),
        },
    )
    .await?;
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or("cookie")?
        .to_str()?
        .split(';')
        .next()
        .ok_or("cookie")?;
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse()?);
    Ok(auth::authenticate(app, &headers).await?)
}
async fn seed(fake: &Arc<Mutex<Fake>>) {
    let mut fake = fake.lock().await;
    let generation = fake.snapshot.info.provider_instance_id;
    for seed in catalog_seed_v1() {
        let record = seed.record(generation);
        let reference =
            CatalogReferenceV1::Seed(PayloadHash::from_bytes(hash(&record.hash_input())));
        let entry = CatalogEntryCore {
            id: seed.id,
            fixed: seed.kind == CatalogKind::Role && seed.id == 1,
            content: reference.clone(),
        };
        if seed.kind == CatalogKind::Role {
            fake.snapshot.catalog.roles.push(entry);
        } else {
            fake.snapshot.catalog.skills.push(entry);
            fake.snapshot.catalog.skill_roles.push(SkillRoleCore {
                skill_id: seed.id,
                role_ids: seed.role_ids.to_vec(),
                origin: reference,
            });
        }
    }
}
fn upsert(id: u32, name: &str) -> ProviderCommand {
    ProviderCommand::UpsertCatalogEntry(UpsertCatalogEntryRequest {
        kind: CatalogKind::Skill,
        id,
        name: name.into(),
    })
}
async fn step(app: &App) -> Result<(), Error> {
    sqlx::query("UPDATE operations SET next_attempt_at=0")
        .execute(&app.db)
        .await?;
    let job = operations::claim(app).await?.ok_or(Error::Internal)?;
    operations::process(app, job).await
}
async fn claims(app: &App) -> Result<i64, Error> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM catalog_skill_names WHERE reservation_op IS NOT NULL",
    )
    .fetch_one(&app.db)
    .await?)
}
#[tokio::test]
async fn catalog_completion_read_failure_keeps_lane_claim_and_retries_after_restart() -> TestResult
{
    let fake = fake()?;
    seed(&fake).await;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let admin = administrator(&app).await?;
    let (worker, _) = register(&app, "catalog_worker").await?;
    let mut view = content_copies::source_worker()?;
    view.account = worker.view.account_id;
    fake.lock()
        .await
        .snapshot
        .workers
        .push(stage_worker(&app, info().provider_instance_id, view).await?);
    let name = format!("Exact\0{}", "λ".repeat(12_000));
    let first = operations::enqueue(&app, &admin, upsert(90, &name), None).await?;
    step(&app).await?;
    assert_eq!(claims(&app).await?, 1);
    let public = crate::catalog_metadata::compose(
        &app,
        info().provider_instance_id,
        info().root_account,
        fake.lock().await.snapshot.catalog.clone(),
    )
    .await?;
    assert!(!public.skills.iter().any(|s| s.id == 90));
    let other = operations::enqueue(
        &app,
        &worker,
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "Independent pending request".into(),
            role_ids: vec![3],
        }),
        None,
    )
    .await?;
    step(&app).await?;
    fake.lock().await.fail_catalog_completion = true;
    step(&app).await?;
    assert_eq!(
        operations::read(&app, &admin, first.operation_id)
            .await?
            .status,
        ProviderOperationStatus::OutcomeUnknown
    );
    assert_eq!(claims(&app).await?, 1);
    assert_eq!(fake.lock().await.submissions, 1);
    assert!(operations::claim(&app).await?.is_none());
    let url = app.config.database_url.clone();
    app.db.close().await;
    let reopened = App::new(test_config(url, &internal)?).await?;
    assert_eq!(
        drive(&reopened, &admin, first.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    assert_eq!(claims(&reopened).await?, 0);
    assert_eq!(fake.lock().await.submissions, 1);
    assert_eq!(
        drive(&reopened, &worker, other.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    let row: (i64, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT skill_id,label_hash,origin_hash FROM catalog_skill_names WHERE skill_id=90",
    )
    .fetch_one(&reopened.db)
    .await?;
    assert_eq!(row.0, 90);
    assert_eq!(row.1, row.2);
    internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn duplicate_requests_stay_pending_and_approval_publishes_actual_id_once() -> TestResult {
    let fake = fake()?;
    seed(&fake).await;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let admin = administrator(&app).await?;
    let (worker, _) = register(&app, "catalog_pending").await?;
    let mut view = content_copies::source_worker()?;
    view.account = worker.view.account_id;
    fake.lock()
        .await
        .snapshot
        .workers
        .push(stage_worker(&app, info().provider_instance_id, view).await?);
    let mut ids = vec![];
    for name in ["  AΣ\u{a7ce}  ", "aσ\u{a7cf}"] {
        let op = operations::enqueue(
            &app,
            &worker,
            ProviderCommand::CreateSkillRequest(CreateSkillRequest {
                name: name.into(),
                role_ids: vec![3],
            }),
            None,
        )
        .await?;
        let receipt = drive(&app, &worker, op.operation_id)
            .await?
            .receipt
            .ok_or("receipt")?;
        ids.push(receipt.created_entity_id.ok_or("request")?);
    }
    assert_eq!(fake.lock().await.catalog_requests.len(), 2);
    assert_eq!(claims(&app).await?, 0);
    let first = operations::enqueue(
        &app,
        &admin,
        ProviderCommand::DecideSkillRequest(DecideSkillRequest {
            request_id: ids[0],
            decision: SkillRequestDecision::Approve,
        }),
        None,
    )
    .await?;
    let second = operations::enqueue(
        &app,
        &admin,
        ProviderCommand::DecideSkillRequest(DecideSkillRequest {
            request_id: ids[1],
            decision: SkillRequestDecision::Approve,
        }),
        None,
    )
    .await?;
    assert_eq!(
        drive(&app, &admin, first.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    let submissions = fake.lock().await.submissions;
    let result = drive(&app, &admin, second.operation_id).await?;
    assert_eq!(result.status, ProviderOperationStatus::Rejected);
    assert_eq!(result.error_code.as_deref(), Some("skill_name_exists"));
    assert!(result.receipt.is_none());
    assert_eq!(fake.lock().await.submissions, submissions);
    assert_eq!(
        fake.lock().await.catalog_requests[1].status,
        SkillRequestStatus::Pending
    );
    let id: i64 = sqlx::query_scalar("SELECT skill_id FROM catalog_skill_names WHERE skill_id=973")
        .fetch_one(&app.db)
        .await?;
    assert_eq!(id, 973);
    let duplicate = operations::enqueue(&app, &admin, upsert(974, "AΣ\u{a7ce}"), None).await?;
    assert_eq!(
        drive(&app, &admin, duplicate.operation_id)
            .await?
            .error_code
            .as_deref(),
        Some("skill_name_exists")
    );
    internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn catalog_staging_and_queue_fail_together_in_both_directions() -> TestResult {
    let fake = fake()?;
    seed(&fake).await;
    let internal = Server::start(internal(fake)).await?;
    let app = app(&internal).await?;
    let admin = administrator(&app).await?;
    sqlx::raw_sql("ALTER TABLE catalog_documents ADD CONSTRAINT blocked CHECK(false)")
        .execute(&app.db)
        .await?;
    assert!(
        operations::enqueue(&app, &admin, upsert(90, "atomic"), None)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations")
            .fetch_one(&app.db)
            .await?,
        0
    );
    sqlx::raw_sql("ALTER TABLE catalog_documents DROP CONSTRAINT blocked; ALTER TABLE operations ADD CONSTRAINT blocked CHECK(false)").execute(&app.db).await?;
    assert!(
        operations::enqueue(&app, &admin, upsert(90, "atomic"), None)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM catalog_documents")
            .fetch_one(&app.db)
            .await?,
        0
    );
    internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn publication_failure_rolls_back_terminal_status_and_retains_the_claim() -> TestResult {
    let fake = fake()?;
    seed(&fake).await;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let admin = administrator(&app).await?;
    let operation =
        operations::enqueue(&app, &admin, upsert(90, "atomic publication"), None).await?;
    step(&app).await?;
    step(&app).await?;
    sqlx::raw_sql("ALTER TABLE catalog_skill_names ADD CONSTRAINT blocked CHECK(skill_id IS DISTINCT FROM 90)").execute(&app.db).await?;
    assert!(step(&app).await.is_err());
    assert_eq!(claims(&app).await?, 1);
    assert_eq!(
        operations::read(&app, &admin, operation.operation_id)
            .await?
            .status,
        ProviderOperationStatus::Submitted
    );
    sqlx::raw_sql("ALTER TABLE catalog_skill_names DROP CONSTRAINT blocked; UPDATE operations SET lease_until=0").execute(&app.db).await?;
    assert_eq!(
        drive(&app, &admin, operation.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    assert_eq!(claims(&app).await?, 0);
    assert_eq!(fake.lock().await.submissions, 1);
    internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn raw_delete_repairs_duplicate_graph_without_releasing_another_identity() -> TestResult {
    let fake = fake()?;
    seed(&fake).await;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let admin = administrator(&app).await?;
    let first = operations::enqueue(&app, &admin, upsert(90, "dup label"), None).await?;
    drive(&app, &admin, first.operation_id).await?;
    // Supply a second direct-core incarnation with the same label; only API
    // writes guarantee naming. The raw repair must not hydrate this broken graph.
    let record = CatalogMetadataRecordV1::from_command(
        info().provider_instance_id,
        info().root_account,
        OperationId::from_bytes([88; 16]),
        &upsert(91, "DUP  LABEL"),
    )
    .ok_or("record")?;
    let fingerprint = hash(&record.hash_input());
    let mut tx = app.db.begin().await?;
    crate::catalog_metadata::insert(
        &mut tx,
        info().provider_instance_id,
        fingerprint,
        &serde_json::to_string(&record)?,
    )
    .await?;
    tx.commit().await?;
    let reference = CatalogReferenceV1::Write(MetadataCommitmentV1 {
        operation_id: OperationId::from_bytes([88; 16]),
        hash: PayloadHash::from_bytes(fingerprint),
    });
    {
        let mut fake = fake.lock().await;
        fake.snapshot.catalog.skills.push(CatalogEntryCore {
            id: 91,
            fixed: false,
            content: reference.clone(),
        });
        fake.snapshot.catalog.skill_roles.push(SkillRoleCore {
            skill_id: 91,
            role_ids: vec![],
            origin: reference,
        });
    }
    assert!(
        crate::catalog_metadata::compose(
            &app,
            info().provider_instance_id,
            info().root_account,
            fake.lock().await.snapshot.catalog.clone()
        )
        .await
        .is_err()
    );
    // Removing the unclaimed duplicate must not delete ID90's normalized claim.
    let delete = operations::enqueue(
        &app,
        &admin,
        ProviderCommand::DeleteCatalogEntry(DeleteCatalogEntryRequest {
            kind: CatalogKind::Skill,
            id: 91,
        }),
        None,
    )
    .await?;
    assert_eq!(
        drive(&app, &admin, delete.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM catalog_skill_names WHERE skill_id=90")
            .fetch_one(&app.db)
            .await?,
        1
    );
    assert!(
        crate::catalog_metadata::compose(
            &app,
            info().provider_instance_id,
            info().root_account,
            fake.lock().await.snapshot.catalog.clone()
        )
        .await
        .is_ok()
    );
    internal.finish().await?;
    Ok(())
}
#[tokio::test]
async fn catalog_exchange_verifies_shared_refs_and_rolls_back_bad_cohorts() -> TestResult {
    let fake = fake()?;
    seed(&fake).await;
    let internal = Server::start(internal(fake.clone())).await?;
    let source = app(&internal).await?;
    let target = app(&internal).await?;
    let admin = administrator(&source).await?;
    let created = operations::enqueue(&source, &admin, upsert(90, "Owned\0catalog"), None).await?;
    drive(&source, &admin, created.operation_id).await?;
    let path = metadata_path()?;
    let count = crate::catalog_metadata_io::export(&source, path.clone()).await?;
    assert_eq!(
        crate::catalog_metadata_io::import(&target, path.clone()).await?,
        count
    );
    assert_eq!(
        crate::catalog_metadata_io::import(&target, path.clone()).await?,
        count
    );
    assert!(
        crate::catalog_metadata_io::export(&source, path.clone())
            .await
            .is_err()
    );
    let original = tokio::fs::read_to_string(&path).await?;
    let first = original.lines().nth(1).ok_or("first")?;
    let bad = metadata_path()?;
    tokio::fs::write(&bad, format!("{original}{first}\n")).await?;
    let empty = app(&internal).await?;
    assert!(
        crate::catalog_metadata_io::import(&empty, bad.clone())
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM catalog_documents")
            .fetch_one(&empty.db)
            .await?,
        0
    );
    // Content unavailable affects catalog presentation; ordinary financial work
    // still executes without entering the catalog lane or hydrating these refs.
    sqlx::query("DELETE FROM catalog_documents WHERE provider_instance_id=$1")
        .bind(info().provider_instance_id.to_string())
        .execute(&target.db)
        .await?;
    let (money, _) = register(&target, "money_owner").await?;
    let deposit = operations::enqueue(
        &target,
        &money,
        ProviderCommand::CreateDeposit(CreateDepositRequest {
            amount: KvnAmount::new(Money::new(5))?,
        }),
        None,
    )
    .await?;
    assert_eq!(
        drive(&target, &money, deposit.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    tokio::fs::remove_file(path).await?;
    tokio::fs::remove_file(bad).await?;
    internal.finish().await?;
    Ok(())
}

fn metadata_path() -> Result<std::path::PathBuf, Error> {
    Ok(std::env::temp_dir().join(format!(
        "catalog-metadata-{}.ndjson",
        OperationId::from_bytes(crate::state::random()?)
    )))
}
#[tokio::test]
async fn uncertain_expired_submissions_hold_names_until_authoritative_reconciliation() -> TestResult
{
    let fake = fake()?;
    seed(&fake).await;
    let internal = Server::start(internal(fake.clone())).await?;
    let app = app(&internal).await?;
    let admin = administrator(&app).await?;
    let (worker, _) = register(&app, "uncertain_worker").await?;
    let mut view = content_copies::source_worker()?;
    view.account = worker.view.account_id;
    fake.lock()
        .await
        .snapshot
        .workers
        .push(stage_worker(&app, info().provider_instance_id, view).await?);
    let first = operations::enqueue(&app, &admin, upsert(90, "Delayed catalog"), None).await?;
    fake.lock().await.fail_before_execution = true;
    assert_eq!(
        drive(&app, &admin, first.operation_id).await?.status,
        ProviderOperationStatus::OutcomeUnknown
    );
    assert_eq!(claims(&app).await?, 1);
    let next = operations::enqueue(
        &app,
        &worker,
        ProviderCommand::CreateSkillRequest(CreateSkillRequest {
            name: "Other request".into(),
            role_ids: vec![3],
        }),
        None,
    )
    .await?;
    sqlx::query("UPDATE operations SET status='Expired',possibly_submitted=true,lease_token=NULL,lease_until=NULL WHERE operation_id=$1").bind(first.operation_id.to_string()).execute(&app.db).await?;
    assert!(operations::claim(&app).await?.is_none());
    assert_eq!(claims(&app).await?, 1);
    // The provider later observes that exact immutable submission. Recovery reads
    // its real fixture receipt; the queue does not invent an execution result.
    let signed: String =
        sqlx::query_scalar("SELECT signed_json FROM operations WHERE operation_id=$1")
            .bind(first.operation_id.to_string())
            .fetch_one(&app.db)
            .await?;
    fake.lock().await.fail_before_execution = false;
    reqwest::Client::new()
        .post(format!("{}/internal/contracts/call", internal.url))
        .json(&serde_json::from_str::<SignedContractCallV1>(&signed)?)
        .send()
        .await?
        .error_for_status()?;
    sqlx::query(
        "UPDATE operations SET status='OutcomeUnknown',next_attempt_at=0 WHERE operation_id=$1",
    )
    .bind(first.operation_id.to_string())
    .execute(&app.db)
    .await?;
    assert_eq!(
        drive(&app, &admin, first.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    assert_eq!(claims(&app).await?, 0);
    assert_eq!(
        drive(&app, &worker, next.operation_id).await?.status,
        ProviderOperationStatus::Finalized
    );
    let expired = operations::enqueue(&app, &admin, upsert(91, "Never submitted"), None).await?;
    step(&app).await?;
    assert_eq!(claims(&app).await?, 1);
    sqlx::query("UPDATE operations SET expires_at=0 WHERE operation_id=$1")
        .bind(expired.operation_id.to_string())
        .execute(&app.db)
        .await?;
    assert_eq!(
        drive(&app, &admin, expired.operation_id).await?.status,
        ProviderOperationStatus::Expired
    );
    assert_eq!(claims(&app).await?, 0);
    let reset = operations::enqueue(&app, &admin, upsert(92, "Old generation"), None).await?;
    fake.lock().await.snapshot.info.provider_instance_id = ProviderInstanceId::from_bytes([77; 16]);
    assert_eq!(
        drive(&app, &admin, reset.operation_id)
            .await?
            .error_code
            .as_deref(),
        Some("provider_instance_changed")
    );
    assert_eq!(claims(&app).await?, 0);
    internal.finish().await?;
    Ok(())
}
