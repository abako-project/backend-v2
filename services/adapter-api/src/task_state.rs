//! Verified staging of actual task state. Public task routes remain provider-owned.
use crate::state::{App, Error, hash, parse};
use generated_contracts::{
    EntityId, ProviderInstanceId, TaskStorageView, TaskView,
    provider_core::{TaskStateV1, TaskStorageRecordV1},
};
use sqlx::{Row, postgres::PgRow};
use std::{collections::BTreeMap, fs::OpenOptions, path::PathBuf};

fn fingerprint(state: &TaskStateV1) -> Result<[u8; 32], Error> {
    let mut bytes = b"abako/task-state/v1\0".to_vec();
    serde_json::to_writer(&mut bytes, state)?;
    Ok(hash(&bytes))
}
struct Prepared {
    state: TaskStateV1,
    fingerprint: [u8; 32],
    tasks: Vec<(EntityId, u32, i64, String, Vec<String>)>,
}
impl Prepared {
    fn new(mut state: TaskStateV1) -> Result<Self, Error> {
        state.validate().map_err(|_| Error::Dependency)?;
        state
            .storages
            .sort_by_key(|record| record.storage.task_storage_id);
        let mut tasks = Vec::new();
        for record in &state.storages {
            for (ordinal, task) in record.storage.tasks.iter().enumerate() {
                tasks.push((
                    record.storage.task_storage_id,
                    task.task_id,
                    i64::try_from(ordinal).map_err(|_| Error::Dependency)?,
                    serde_json::to_string(task)?,
                    task.task
                        .assignees
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                ));
            }
        }
        Ok(Self {
            fingerprint: fingerprint(&state)?,
            state,
            tasks,
        })
    }
}
struct Stored {
    state: TaskStateV1,
    fingerprint: Vec<u8>,
}
async fn read(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    generation: ProviderInstanceId,
) -> Result<Option<Stored>, Error> {
    let Some(header) = sqlx::query("SELECT next_task_id,source_hash,storage_count,task_count FROM task_generations WHERE provider_instance_id=$1")
        .bind(generation.to_string()).fetch_optional(&mut **tx).await? else { return Ok(None); };
    let storages = sqlx::query("SELECT task_storage_id,project_id,proposal_id,proposal_revision,milestone_id FROM task_storages WHERE provider_instance_id=$1")
        .bind(generation.to_string()).fetch_all(&mut **tx).await?;
    let tasks = sqlx::query("SELECT task_id,task_storage_id,ordinal,payload,assignees FROM task_items WHERE provider_instance_id=$1 ORDER BY task_storage_id,ordinal")
        .bind(generation.to_string()).fetch_all(&mut **tx).await?;
    tokio::task::spawn_blocking(move || decode(generation, &header, storages, tasks))
        .await?
        .map(Some)
}
fn decode(
    generation: ProviderInstanceId,
    header: &PgRow,
    storages: Vec<PgRow>,
    tasks: Vec<PgRow>,
) -> Result<Stored, Error> {
    if header.try_get::<i64, _>("storage_count")?
        != i64::try_from(storages.len()).map_err(|_| Error::Dependency)?
        || header.try_get::<i64, _>("task_count")?
            != i64::try_from(tasks.len()).map_err(|_| Error::Dependency)?
    {
        return Err(Error::Dependency);
    }
    let mut grouped: BTreeMap<EntityId, Vec<TaskView>> = BTreeMap::new();
    for row in tasks {
        let storage: EntityId = parse(row.try_get("task_storage_id")?)?;
        let current = grouped.entry(storage).or_default();
        let task: TaskView = serde_json::from_str(row.try_get("payload")?)?;
        if i64::from(task.task_id) != row.try_get::<i64, _>("task_id")?
            || row.try_get::<i64, _>("ordinal")?
                != i64::try_from(current.len()).map_err(|_| Error::Dependency)?
            || task
                .task
                .assignees
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                != row.try_get::<Vec<String>, _>("assignees")?
        {
            return Err(Error::Dependency);
        }
        current.push(task);
    }
    let mut state = TaskStateV1 {
        schema_version: 1,
        generation,
        next_task_id: u32::try_from(header.try_get::<i64, _>("next_task_id")?)
            .map_err(|_| Error::Dependency)?,
        storages: Vec::with_capacity(storages.len()),
    };
    for row in storages {
        let id = parse(row.try_get("task_storage_id")?)?;
        state.storages.push(TaskStorageRecordV1 {
            project_id: parse(row.try_get("project_id")?)?,
            proposal_id: parse(row.try_get("proposal_id")?)?,
            proposal_revision: parse(row.try_get("proposal_revision")?)?,
            storage: TaskStorageView {
                task_storage_id: id,
                milestone_id: parse(row.try_get("milestone_id")?)?,
                tasks: grouped.remove(&id).unwrap_or_default(),
            },
        });
    }
    if !grouped.is_empty() {
        return Err(Error::Dependency);
    }
    state
        .storages
        .sort_by_key(|record| record.storage.task_storage_id);
    state.validate().map_err(|_| Error::Dependency)?;
    Ok(Stored {
        state,
        fingerprint: header.try_get("source_hash")?,
    })
}

pub(crate) async fn backfill(app: &App) -> Result<(usize, usize), Error> {
    let mut tx = app.db.begin().await?;
    // Serialize capture as well as replacement: an old fetch cannot commit last.
    sqlx::query("SELECT pg_advisory_xact_lock(621009)")
        .execute(&mut *tx)
        .await?;
    let response = app
        .internal(
            reqwest::Method::GET,
            &app.config.provider_url,
            "/internal/task-state",
        )
        .send()
        .await
        .map_err(|_| Error::Dependency)?;
    let bytes = crate::state::bounded_response(response).await?;
    let mut prepared = tokio::task::spawn_blocking(move || {
        let state = serde_json::from_slice(&bytes).map_err(|_| Error::Dependency)?;
        Prepared::new(state)
    })
    .await??;
    let generation = prepared.state.generation;
    // Fence the existing admission transaction before checking API-owned history.
    sqlx::query("SELECT pg_advisory_xact_lock(621007)")
        .execute(&mut *tx)
        .await?;
    let api_owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations WHERE task_operation AND provider_instance_id=$1) OR EXISTS(SELECT 1 FROM task_generations WHERE provider_instance_id=$1 AND api_owned) OR EXISTS(SELECT 1 FROM task_freezes WHERE provider_instance_id=$1)",
    )
    .bind(generation.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if api_owned {
        return Err(Error::Conflict("task_state_api_owned"));
    }
    if let Some(previous) = read(&mut tx, generation).await? {
        let (returned, previous_counter, same) = tokio::task::spawn_blocking(move || {
            if fingerprint(&previous.state)? != previous.fingerprint.as_slice() {
                return Err(Error::Dependency);
            }
            let same = previous.state == prepared.state;
            Ok::<_, Error>((prepared, previous.state.next_task_id, same))
        })
        .await??;
        prepared = returned;
        if same {
            tx.commit().await?;
            return Ok((prepared.state.storages.len(), prepared.tasks.len()));
        }
        if prepared.state.next_task_id < previous_counter {
            return Err(Error::Conflict("task_counter_regressed"));
        }
    }
    sqlx::query("DELETE FROM task_generations WHERE provider_instance_id=$1")
        .bind(generation.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO task_generations(provider_instance_id,next_task_id,source_hash,storage_count,task_count) VALUES($1,$2,$3,$4,$5)")
        .bind(generation.to_string()).bind(i64::from(prepared.state.next_task_id)).bind(prepared.fingerprint.as_slice())
        .bind(i64::try_from(prepared.state.storages.len()).map_err(|_|Error::Dependency)?)
        .bind(i64::try_from(prepared.tasks.len()).map_err(|_|Error::Dependency)?).execute(&mut *tx).await?;
    for record in &prepared.state.storages {
        sqlx::query("INSERT INTO task_storages(provider_instance_id,task_storage_id,project_id,proposal_id,proposal_revision,milestone_id) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(generation.to_string()).bind(record.storage.task_storage_id.to_string()).bind(record.project_id.to_string())
            .bind(record.proposal_id.to_string()).bind(record.proposal_revision.to_string()).bind(record.storage.milestone_id.to_string())
            .execute(&mut *tx).await?;
    }
    for (storage, id, ordinal, payload, assignees) in &prepared.tasks {
        sqlx::query("INSERT INTO task_items(provider_instance_id,task_id,task_storage_id,ordinal,payload,assignees) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(generation.to_string()).bind(i64::from(*id)).bind(storage.to_string()).bind(*ordinal).bind(payload).bind(assignees)
            .execute(&mut *tx).await?;
    }
    let actual = read(&mut tx, generation).await?.ok_or(Error::Dependency)?;
    let verified = tokio::task::spawn_blocking(move || {
        if actual.state != prepared.state || actual.fingerprint != prepared.fingerprint {
            return Err(Error::Dependency);
        }
        Ok((prepared.state.storages.len(), prepared.tasks.len()))
    })
    .await??;
    tx.commit().await?;
    Ok(verified)
}

pub(crate) async fn export(
    app: &App,
    generation: ProviderInstanceId,
    path: PathBuf,
) -> Result<(), Error> {
    let mut tx = app.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(621009)")
        .execute(&mut *tx)
        .await?;
    let owned: Option<bool> = sqlx::query_scalar(
        "SELECT api_owned FROM task_generations WHERE provider_instance_id=$1 FOR UPDATE",
    )
    .bind(generation.to_string())
    .fetch_optional(&mut *tx)
    .await?;
    if owned == Some(true) {
        return Err(Error::Conflict("task_state_api_owned"));
    }
    let stored = read(&mut tx, generation).await?.ok_or(Error::NotFound)?;
    let state = tokio::task::spawn_blocking(move || {
        if fingerprint(&stored.state)? != stored.fingerprint.as_slice() {
            return Err(Error::Dependency);
        }
        Ok::<_, Error>(stored.state)
    })
    .await??;
    tx.commit().await?;
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let bytes = serde_json::to_vec(&state)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(Error::Invalid);
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    })
    .await?
}
