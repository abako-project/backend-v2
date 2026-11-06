//! Immutable directory names, distinct from editable identity/profile names.
use crate::state::{App, Error};
use protocol::{ProviderInstanceId, WorkerView, provider_core::WorkerCore};

pub(crate) async fn compose(
    app: &App,
    generation: ProviderInstanceId,
    worker: WorkerCore,
) -> Result<WorkerView, Error> {
    let name =
        crate::creation_metadata::worker_name(app, generation, worker.account, &worker.content)
            .await?;
    Ok(worker.into_view(name))
}
