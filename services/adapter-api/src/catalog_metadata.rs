//! API-owned immutable labels/requests, with explicit seed provisioning.
use crate::state::{App, Error, hash};
use generated_contracts::{
    AccountId32, CatalogEntry, CatalogKind, CatalogView, OperationId, PayloadHash, ProviderCommand,
    ProviderInstanceId, SkillRequestView, SkillRoleAssociation,
    provider_core::{
        CatalogContentV1, CatalogCore, CatalogMetadataRecordV1, CatalogReferenceV1,
        MetadataCommitmentV1, SkillRequestCore, catalog_seed_v1,
    },
};
use sqlx::PgConnection;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn operation(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::UpsertCatalogEntry(_)
            | ProviderCommand::UpsertCatalogEntryCore { .. }
            | ProviderCommand::DeleteCatalogEntry(_)
            | ProviderCommand::CreateSkillRequest(_)
            | ProviderCommand::CreateSkillRequestCore { .. }
            | ProviderCommand::DecideSkillRequest(_)
    )
}
pub(crate) async fn insert(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    fingerprint: [u8; 32],
    payload: &str,
) -> Result<(), Error> {
    sqlx::query("INSERT INTO catalog_documents(provider_instance_id,content_hash,payload) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(generation.to_string()).bind(fingerprint.as_slice()).bind(payload).execute(&mut *connection).await?;
    let stored: String = sqlx::query_scalar(
        "SELECT payload FROM catalog_documents WHERE provider_instance_id=$1 AND content_hash=$2",
    )
    .bind(generation.to_string())
    .bind(fingerprint.as_slice())
    .fetch_one(&mut *connection)
    .await?;
    if stored != payload {
        return Err(Error::Dependency);
    }
    Ok(())
}
pub(crate) async fn capture(
    connection: &mut PgConnection,
    generation: ProviderInstanceId,
    author: AccountId32,
    operation_id: OperationId,
    command: ProviderCommand,
) -> Result<ProviderCommand, Error> {
    if !matches!(
        command,
        ProviderCommand::UpsertCatalogEntry(_) | ProviderCommand::CreateSkillRequest(_)
    ) {
        return Ok(command);
    }
    let (record, fingerprint, payload) = tokio::task::spawn_blocking(move || {
        let record =
            CatalogMetadataRecordV1::from_command(generation, author, operation_id, &command)
                .ok_or(Error::Internal)?;
        let fingerprint = hash(&record.hash_input());
        let payload = serde_json::to_string(&record)?;
        Ok::<_, Error>((record, fingerprint, payload))
    })
    .await??;
    insert(connection, generation, fingerprint, &payload).await?;
    let content = MetadataCommitmentV1 {
        operation_id,
        hash: PayloadHash::from_bytes(fingerprint),
    };
    match record.content {
        CatalogContentV1::Entry { kind, id, .. } => {
            Ok(ProviderCommand::UpsertCatalogEntryCore { kind, id, content })
        }
        CatalogContentV1::Request(request) => Ok(ProviderCommand::CreateSkillRequestCore {
            role_ids: request.role_ids,
            content,
        }),
    }
}
pub(crate) async fn record(
    app: &App,
    generation: ProviderInstanceId,
    reference: &CatalogReferenceV1,
) -> Result<CatalogMetadataRecordV1, Error> {
    let payload: String = sqlx::query_scalar(
        "SELECT payload FROM catalog_documents WHERE provider_instance_id=$1 AND content_hash=$2",
    )
    .bind(generation.to_string())
    .bind(reference.hash().as_bytes().as_slice())
    .fetch_optional(&app.db)
    .await?
    .ok_or(Error::Dependency)?;
    let expected = reference.hash();
    tokio::task::spawn_blocking(move || {
        let record: CatalogMetadataRecordV1 =
            serde_json::from_str(&payload).map_err(|_| Error::Dependency)?;
        if record.generation != generation || hash(&record.hash_input()) != expected.into_bytes() {
            return Err(Error::Dependency);
        }
        Ok(record)
    })
    .await?
}
/// Provision only exact selected Seed refs. Missing writes/imports never fall back.
pub(crate) async fn provision_seeds(
    app: &App,
    generation: ProviderInstanceId,
    catalog: &CatalogCore,
) -> Result<(), Error> {
    let mut selected = Vec::new();
    for entry in &catalog.roles {
        if let CatalogReferenceV1::Seed(hash) = entry.content {
            selected.push((CatalogKind::Role, entry.id, hash));
        }
    }
    for entry in &catalog.skills {
        if let CatalogReferenceV1::Seed(hash) = entry.content {
            selected.push((CatalogKind::Skill, entry.id, hash));
        }
    }
    for entry in &catalog.skill_roles {
        if let CatalogReferenceV1::Seed(hash) = entry.origin {
            selected.push((CatalogKind::Skill, entry.skill_id, hash));
        }
    }
    let records = tokio::task::spawn_blocking(move || {
        let mut records = BTreeMap::new();
        for (kind, id, expected) in selected {
            let seed = catalog_seed_v1()
                .into_iter()
                .find(|entry| entry.kind == kind && entry.id == id)
                .ok_or(Error::Dependency)?;
            let record = seed.record(generation);
            let fingerprint = hash(&record.hash_input());
            if fingerprint != expected.into_bytes() {
                return Err(Error::Dependency);
            }
            records.insert(fingerprint, serde_json::to_string(&record)?);
        }
        Ok::<_, Error>(records)
    })
    .await??;
    let mut tx = app.db.begin().await?;
    for (fingerprint, payload) in records {
        insert(&mut tx, generation, fingerprint, &payload).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub(crate) async fn requests(
    app: &App,
    account: AccountId32,
) -> Result<Vec<SkillRequestCore>, Error> {
    app.get(
        &app.config.provider_url,
        &format!("/internal/catalog/skill-requests?account={account}"),
    )
    .await
}
pub(crate) async fn request_view(
    app: &App,
    generation: ProviderInstanceId,
    request: SkillRequestCore,
) -> Result<SkillRequestView, Error> {
    let record = record(app, generation, &request.content).await?;
    tokio::task::spawn_blocking(move || {
        let name = record
            .request_name(generation, &request)
            .map_err(|_| Error::Dependency)?
            .to_owned();
        Ok(SkillRequestView {
            request_id: request.request_id,
            requester: request.requester,
            name,
            role_ids: request.role_ids,
            status: request.status,
            skill_id: request.skill_id,
        })
    })
    .await?
}
pub(crate) async fn label_records(
    app: &App,
    generation: ProviderInstanceId,
    catalog: &CatalogCore,
    roles: bool,
) -> Result<BTreeMap<PayloadHash, CatalogMetadataRecordV1>, Error> {
    provision_seeds(app, generation, catalog).await?;
    let mut records = BTreeMap::new();
    for entry in catalog
        .skills
        .iter()
        .chain(catalog.roles.iter().filter(|_| roles))
    {
        if let std::collections::btree_map::Entry::Vacant(slot) =
            records.entry(entry.content.hash())
        {
            slot.insert(record(app, generation, &entry.content).await?);
        }
    }
    Ok(records)
}
pub(crate) async fn request_context(
    app: &App,
    root: AccountId32,
    records: &BTreeMap<PayloadHash, CatalogMetadataRecordV1>,
) -> Result<Vec<SkillRequestCore>, Error> {
    if records
        .values()
        .any(|record| !matches!(record.content, CatalogContentV1::Entry { .. }))
    {
        requests(app, root).await
    } else {
        Ok(Vec::new())
    }
}
pub(crate) async fn compose(
    app: &App,
    generation: ProviderInstanceId,
    root: AccountId32,
    catalog: CatalogCore,
) -> Result<CatalogView, Error> {
    let records = label_records(app, generation, &catalog, true).await?;
    let context = request_context(app, root, &records).await?;
    tokio::task::spawn_blocking(move || {
        let entry = |kind,
                     core: &generated_contracts::provider_core::CatalogEntryCore|
         -> Result<CatalogEntry, Error> {
            let record = records.get(&core.content.hash()).ok_or(Error::Dependency)?;
            let name = record
                .entry_name(generation, kind, core.id, root, &core.content, &context)
                .map_err(|_| Error::Dependency)?
                .to_owned();
            Ok(CatalogEntry {
                id: core.id,
                name,
                fixed: core.fixed,
            })
        };
        let roles = catalog
            .roles
            .iter()
            .map(|core| entry(CatalogKind::Role, core))
            .collect::<Result<_, _>>()?;
        let skills = catalog
            .skills
            .iter()
            .map(|core| entry(CatalogKind::Skill, core))
            .collect::<Result<Vec<_>, _>>()?;
        let mut names = BTreeSet::new();
        for skill in &skills {
            if !names.insert(CatalogMetadataRecordV1::normalized(&skill.name)) {
                return Err(Error::Dependency);
            }
        }
        Ok(CatalogView {
            roles,
            skills,
            skill_roles: catalog
                .skill_roles
                .into_iter()
                .map(|value| SkillRoleAssociation {
                    skill_id: value.skill_id,
                    role_ids: value.role_ids,
                })
                .collect(),
            score_policy: catalog.score_policy,
            compensation_policy: catalog.compensation_policy,
        })
    })
    .await?
}
#[derive(Clone)]
pub(crate) struct SkillName {
    pub id: u32,
    pub normalized: Vec<u8>,
    pub digest: [u8; 32],
    pub label: PayloadHash,
    pub origin: PayloadHash,
}
pub(crate) fn normalized(name: &str) -> (Vec<u8>, [u8; 32]) {
    let normalized = CatalogMetadataRecordV1::normalized(name).into_bytes();
    let mut bytes = b"abako/catalog-skill-name/v1\0".to_vec();
    bytes.extend_from_slice(&normalized);
    let digest = hash(&bytes);
    (normalized, digest)
}
/// Name-sensitive mutations inspect actual selected skill labels. Existing invalid
/// graphs can still be repaired by raw Delete/Reject without this path.
pub(crate) async fn skill_names(
    app: &App,
    generation: ProviderInstanceId,
    root: AccountId32,
    catalog: CatalogCore,
) -> Result<Vec<SkillName>, Error> {
    let records = label_records(app, generation, &catalog, false).await?;
    let context = request_context(app, root, &records).await?;
    tokio::task::spawn_blocking(move || {
        let mut names = Vec::new();
        for core in &catalog.skills {
            let record = records.get(&core.content.hash()).ok_or(Error::Dependency)?;
            let name = record
                .entry_name(
                    generation,
                    CatalogKind::Skill,
                    core.id,
                    root,
                    &core.content,
                    &context,
                )
                .map_err(|_| Error::Dependency)?;
            let (normalized, digest) = normalized(name);
            let origin = catalog
                .skill_roles
                .iter()
                .find(|skill| skill.skill_id == core.id)
                .ok_or(Error::Dependency)?
                .origin
                .hash();
            names.push(SkillName {
                id: core.id,
                normalized,
                digest,
                label: core.content.hash(),
                origin,
            });
        }
        Ok(names)
    })
    .await?
}
