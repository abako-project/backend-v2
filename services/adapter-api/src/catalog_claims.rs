//! Name admission in the existing catalog FIFO. Provider facts select identities;
//! SQL claims cover the interval from preparation through a known final outcome.
use crate::{
    catalog_metadata as metadata,
    state::{App, Error, parse},
};
use generated_contracts::{
    AccountId32, CatalogKind, EntityId, ExecutionOutcome, OperationId, OperationReceipt,
    ProviderCommand, ProviderInfo, ProviderInstanceId, ProviderOperationStatus,
    SkillRequestDecision, SkillRequestStatus, provider_core::CatalogReferenceV1,
};
use sqlx::{PgConnection, Row};

pub(crate) enum Admission {
    Ready,
    Duplicate,
    Wait,
}
fn repairing(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::DeleteCatalogEntry(_)
            | ProviderCommand::DecideSkillRequest(generated_contracts::DecideSkillRequest {
                decision: SkillRequestDecision::Reject,
                ..
            })
    )
}
async fn lease(
    connection: &mut PgConnection,
    id: OperationId,
    token: &str,
    time: i64,
) -> Result<(), Error> {
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM operations WHERE operation_id=$1 AND lease_token=$2 AND lease_until>$3 FOR UPDATE)").bind(id.to_string()).bind(token).bind(time).fetch_one(&mut *connection).await?;
    if !exists {
        return Err(Error::Conflict("lease_lost"));
    }
    Ok(())
}
/// Reconcile only a verified unique graph, retaining every uncertain reservation.
async fn reconcile(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    names: &[metadata::SkillName],
) -> Result<(), Error> {
    let mut seen = std::collections::BTreeMap::new();
    for name in names {
        if seen.insert(name.digest, &name.normalized).is_some() {
            return Err(Error::Dependency);
        }
    }
    sqlx::query(
        "DELETE FROM catalog_skill_names WHERE provider_instance_id=$1 AND reservation_op IS NULL",
    )
    .bind(generation.to_string())
    .execute(&mut *connection)
    .await?;
    sqlx::query("UPDATE catalog_skill_names SET skill_id=NULL,label_hash=NULL,origin_hash=NULL WHERE provider_instance_id=$1").bind(generation.to_string()).execute(&mut *connection).await?;
    for name in names {
        if let Some(bytes)=sqlx::query_scalar::<_,Vec<u8>>("SELECT normalized FROM catalog_skill_names WHERE provider_instance_id=$1 AND name_hash=$2 FOR UPDATE").bind(generation.to_string()).bind(name.digest.as_slice()).fetch_optional(&mut *connection).await?
            && bytes != name.normalized { return Err(Error::Dependency); }

        sqlx::query("INSERT INTO catalog_skill_names(provider_instance_id,name_hash,normalized,skill_id,label_hash,origin_hash) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(provider_instance_id,name_hash) DO UPDATE SET skill_id=EXCLUDED.skill_id,label_hash=EXCLUDED.label_hash,origin_hash=EXCLUDED.origin_hash").bind(generation.to_string()).bind(name.digest.as_slice()).bind(&name.normalized).bind(i64::from(name.id)).bind(name.label.as_bytes().as_slice()).bind(name.origin.as_bytes().as_slice()).execute(&mut *connection).await?;
    }
    Ok(())
}
pub(crate) async fn prepare(
    app: &App,
    id: OperationId,
    account: AccountId32,
    command: &ProviderCommand,
    info: &ProviderInfo,
    token: &str,
) -> Result<Admission, Error> {
    if repairing(command) {
        return Ok(Admission::Ready);
    }
    let generation = info.provider_instance_id;
    let (name, own, reserve) = match command {
        ProviderCommand::UpsertCatalogEntryCore {
            kind: CatalogKind::Skill,
            id,
            content,
        } if account == info.root_account => {
            let reference = CatalogReferenceV1::Write(content.clone());
            let record = metadata::record(app, generation, &reference).await?;
            let name = record
                .entry_name(
                    generation,
                    CatalogKind::Skill,
                    *id,
                    info.root_account,
                    &reference,
                    &[],
                )
                .map_err(|_| Error::Dependency)?
                .to_owned();
            (name, Some(*id), true)
        }
        ProviderCommand::CreateSkillRequestCore { role_ids, content } => {
            let reference = CatalogReferenceV1::Write(content.clone());
            let record = metadata::record(app, generation, &reference).await?;
            let marker = generated_contracts::provider_core::SkillRequestCore {
                request_id: EntityId::from_bytes([0; 16]),
                requester: account,
                content: reference,
                role_ids: role_ids.clone(),
                status: SkillRequestStatus::Pending,
                skill_id: None,
            };
            let name = record
                .request_name(generation, &marker)
                .map_err(|_| Error::Dependency)?
                .to_owned();
            (name, None, false)
        }
        ProviderCommand::DecideSkillRequest(request)
            if request.decision == SkillRequestDecision::Approve
                && account == info.root_account =>
        {
            let requests = metadata::requests(app, info.root_account).await?;
            let Some(marker) = requests.into_iter().find(|r| {
                r.request_id == request.request_id && r.status == SkillRequestStatus::Pending
            }) else {
                return Ok(Admission::Ready);
            };
            let record = metadata::record(app, generation, &marker.content).await?;
            let name = record
                .request_name(generation, &marker)
                .map_err(|_| Error::Dependency)?
                .to_owned();
            (name, None, true)
        }
        _ => return Ok(Admission::Ready),
    };
    let snapshot = app.snapshot().await?;
    if snapshot.info != *info {
        return Err(Error::Dependency);
    }
    let names = metadata::skill_names(app, generation, info.root_account, snapshot.catalog).await?;
    let (normalized, digest) =
        tokio::task::spawn_blocking(move || metadata::normalized(&name)).await?;
    let mut tx = app.db.begin().await?;
    lease(&mut tx, id, token, crate::state::now()?).await?;
    reconcile(&mut tx, generation, &names).await?;
    if let Some(row)=sqlx::query("SELECT normalized,skill_id,reservation_op FROM catalog_skill_names WHERE provider_instance_id=$1 AND name_hash=$2 FOR UPDATE").bind(generation.to_string()).bind(digest.as_slice()).fetch_optional(&mut *tx).await? {
        if row.try_get::<Vec<u8>,_>("normalized")?!=normalized{return Err(Error::Dependency);}
        let held:Option<String>=row.try_get("reservation_op")?;
        if held.is_some_and(|value|value!=id.to_string()) {tx.commit().await?;return Ok(Admission::Wait);}
        let selected:Option<i64>=row.try_get("skill_id")?;
        if selected.is_some_and(|value|Some(value)!=own.map(i64::from)){tx.commit().await?;return Ok(Admission::Duplicate);}
    }
    if reserve {
        sqlx::query("INSERT INTO catalog_skill_names(provider_instance_id,name_hash,normalized,reservation_op) VALUES($1,$2,$3,$4) ON CONFLICT(provider_instance_id,name_hash) DO UPDATE SET reservation_op=EXCLUDED.reservation_op").bind(generation.to_string()).bind(digest.as_slice()).bind(normalized).bind(id.to_string()).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Admission::Ready)
}

pub(crate) struct Completion {
    generation: ProviderInstanceId,
    selected: Option<(u32, CatalogReferenceV1, CatalogReferenceV1)>,
    deleted: Option<u32>,
}
/// Read only raw core facts, so repair deletes/rejections never require the invalid
/// name graph they are repairing. No receipt-created ID prediction is used.
pub(crate) async fn completion(
    app: &App,
    command: &ProviderCommand,
    instance: Option<&str>,
    status: ProviderOperationStatus,
    submitted: bool,
    code: Option<&str>,
    receipt: Option<&str>,
) -> Result<Option<Completion>, Error> {
    if !metadata::operation(command) {
        return Ok(None);
    }
    let known = match status {
        ProviderOperationStatus::Finalized => true,
        ProviderOperationStatus::Rejected => !submitted || code == Some("provider_rejected"),
        ProviderOperationStatus::Expired => !submitted,
        _ => false,
    };
    if !known {
        return Ok(None);
    }
    let Some(instance) = instance else {
        return Ok(None);
    };
    let generation = parse(instance)?;
    let mut result = Completion {
        generation,
        selected: None,
        deleted: None,
    };
    let receipt = receipt
        .map(serde_json::from_str::<OperationReceipt>)
        .transpose()?;
    if !receipt.is_some_and(|r| r.outcome == ExecutionOutcome::Success) {
        return Ok(Some(result));
    }
    let id = match command {
        ProviderCommand::UpsertCatalogEntryCore {
            kind: CatalogKind::Skill,
            id,
            content,
        } => Some((*id, CatalogReferenceV1::Write(content.clone()))),
        ProviderCommand::DecideSkillRequest(request)
            if request.decision == SkillRequestDecision::Approve =>
        {
            let info: ProviderInfo = app.get(&app.config.provider_url, "/internal/info").await?;
            if info.provider_instance_id != generation {
                return Err(Error::Dependency);
            }
            let marker = metadata::requests(app, info.root_account)
                .await?
                .into_iter()
                .find(|r| {
                    r.request_id == request.request_id && r.status == SkillRequestStatus::Approved
                })
                .ok_or(Error::Dependency)?;
            Some((marker.skill_id.ok_or(Error::Dependency)?, marker.content))
        }
        ProviderCommand::DeleteCatalogEntry(request) if request.kind == CatalogKind::Skill => {
            result.deleted = Some(request.id);
            None
        }
        _ => None,
    };
    if let Some((id, expected)) = id {
        let snapshot = app.snapshot().await?;
        if snapshot.info.provider_instance_id != generation {
            return Err(Error::Dependency);
        }
        let label = snapshot
            .catalog
            .entry(CatalogKind::Skill, id)
            .ok_or(Error::Dependency)?
            .content
            .clone();
        if label != expected {
            return Err(Error::Dependency);
        }
        let origin = snapshot
            .catalog
            .skill_roles
            .into_iter()
            .find(|s| s.skill_id == id)
            .ok_or(Error::Dependency)?
            .origin;
        result.selected = Some((id, label, origin));
    }
    Ok(Some(result))
}
pub(crate) async fn publish(
    connection: &mut PgConnection,
    operation_id: OperationId,
    completion: Completion,
) -> Result<(), Error> {
    let generation = completion.generation.to_string();
    let operation = operation_id.to_string();
    if let Some(id) = completion.deleted {
        // Remove only this numeric identity, never another ID's same-digest row.
        sqlx::query("DELETE FROM catalog_skill_names WHERE provider_instance_id=$1 AND skill_id=$2 AND reservation_op IS NULL").bind(&generation).bind(i64::from(id)).execute(&mut *connection).await?;
        sqlx::query("UPDATE catalog_skill_names SET skill_id=NULL,label_hash=NULL,origin_hash=NULL WHERE provider_instance_id=$1 AND skill_id=$2 AND reservation_op IS NOT NULL").bind(&generation).bind(i64::from(id)).execute(&mut *connection).await?;
    }
    if let Some((id, label, origin)) = completion.selected {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog_skill_names WHERE provider_instance_id=$1 AND reservation_op=$2)").bind(&generation).bind(&operation).fetch_one(&mut *connection).await?;
        if !exists {
            return Err(Error::Dependency);
        }
        sqlx::query("DELETE FROM catalog_skill_names WHERE provider_instance_id=$1 AND skill_id=$2 AND reservation_op IS DISTINCT FROM $3").bind(&generation).bind(i64::from(id)).bind(&operation).execute(&mut *connection).await?;
        sqlx::query("UPDATE catalog_skill_names SET skill_id=$1,label_hash=$2,origin_hash=$3,reservation_op=NULL WHERE provider_instance_id=$4 AND reservation_op=$5").bind(i64::from(id)).bind(label.hash().as_bytes().as_slice()).bind(origin.hash().as_bytes().as_slice()).bind(&generation).bind(&operation).execute(&mut *connection).await?;
    } else {
        sqlx::query("DELETE FROM catalog_skill_names WHERE provider_instance_id=$1 AND reservation_op=$2 AND skill_id IS NULL").bind(&generation).bind(&operation).execute(&mut *connection).await?;
        sqlx::query("UPDATE catalog_skill_names SET reservation_op=NULL WHERE provider_instance_id=$1 AND reservation_op=$2").bind(&generation).bind(&operation).execute(&mut *connection).await?;
    }
    Ok(())
}
