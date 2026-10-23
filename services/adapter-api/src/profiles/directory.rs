//! R1 directory display-name copy, distinct from editable profile/principal names.
use crate::state::{App, Error, hash};
use generated_contracts::{ProviderInstanceId, WorkerView};
use sqlx::Row;

pub(crate) async fn compose(
    app: &App,
    generation: ProviderInstanceId,
    worker: &mut WorkerView,
) -> Result<(), Error> {
    let fingerprint = hash(
        &[
            b"abako/worker-directory-copy/v1\0".as_slice(),
            generation.as_bytes(),
            worker.account.as_bytes(),
            worker.display_name.as_bytes(),
        ]
        .concat(),
    );
    sqlx::query("INSERT INTO worker_directory_copies(provider_instance_id,account_id,source_fingerprint,display_name_json) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING")
        .bind(generation.to_string()).bind(worker.account.to_string()).bind(fingerprint.as_slice()).bind(serde_json::to_string(&worker.display_name)?).execute(&app.db).await?;
    let row = sqlx::query("SELECT display_name_json FROM worker_directory_copies WHERE provider_instance_id=$1 AND account_id=$2 AND source_fingerprint=$3")
        .bind(generation.to_string()).bind(worker.account.to_string()).bind(fingerprint.as_slice()).fetch_one(&app.db).await?;
    let stored: String =
        serde_json::from_str(row.try_get("display_name_json")?).map_err(|_| Error::Dependency)?;
    if stored != worker.display_name {
        return Err(Error::Dependency);
    }
    worker.display_name = stored;
    Ok(())
}
