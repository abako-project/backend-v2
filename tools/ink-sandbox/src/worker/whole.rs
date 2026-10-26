//! One primary and an optional derivative, committed in the same transaction.
use super::*;
use provider_execution::{Admission, ExecutionInputs, VerifiedCall, migration::CoreArchiveV5};
use serde::{Deserialize, Serialize};

#[cfg(test)]
thread_local! {
    static NATIVE_EXECUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    Rust,
    Shadow,
    Sandbox,
}
impl Authority {
    fn key(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Shadow => "shadow",
            Self::Sandbox => "sandbox",
        }
    }
}
#[derive(Debug, Serialize)]
pub struct WholeDiagnostics {
    pub authority: Authority,
    /// Disabled, matched, different or incomplete. A stopped comparison needs a fresh import.
    pub comparison: String,
    pub comparisons: i64,
}
struct Retained {
    archive: CoreArchiveV5,
    diagnostics: WholeDiagnostics,
}
pub(super) fn authority(mode: Mode) -> Result<Authority, Error> {
    match mode {
        Mode::Directory {
            authority: Some(authority),
            ..
        } => Ok(authority),
        _ => Err(Error::InvalidState),
    }
}
pub(super) fn mode(configuration: &WholeConfiguration, authority: Authority) -> Mode {
    let mut mode = configuration.mode();
    if let Mode::Directory {
        configuration,
        authority: selected,
        ..
    } = &mut mode
    {
        *configuration = H256::from(sp_core::hashing::blake2_256(
            &(b"whole-authority-v1", *configuration, authority).encode(),
        ));
        *selected = Some(authority);
    }
    mode
}
pub(super) fn reference(
    mode: Mode,
    archive: CoreArchiveV5,
) -> Result<mock_provider::ReferenceState, Error> {
    let Mode::Directory { root, .. } = mode else {
        return Err(Error::InvalidState);
    };
    mock_provider::ReferenceState::from_archive(root, archive).map_err(Error::Reference)
}
fn bytes(archive: &CoreArchiveV5) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(archive).map_err(|_| Error::InvalidState)?;
    if bytes.len() > MAX_SHELL {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
fn candidate(stored: &Stored, bundles: &Bundles, mode: Mode) -> Result<CoreArchiveV5, Error> {
    migration::read_archive(
        &mut stored.checkpoint.clone().restore()?,
        bundles,
        &directory::decode(stored, mode)?,
        true,
    )
}
async fn retained(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    mode: Mode,
) -> Result<Retained, Error> {
    let selected = authority(mode)?;
    let row = sqlx::query("SELECT archive, comparison, comparisons FROM candidate_reference WHERE singleton = 1 AND authority = ? AND length(archive) <= 67108864 AND comparisons >= 0 AND comparison IN ('disabled','matched','different','incomplete')")
        .bind(selected.key()).fetch_optional(&mut **tx).await?.ok_or(Error::InvalidState)?;
    let bytes: Vec<u8> = row.try_get("archive")?;
    let archive: CoreArchiveV5 = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidState)?;
    reference(mode, archive.clone())?;
    let comparison: String = row.try_get("comparison")?;
    if (selected == Authority::Rust) != (comparison == "disabled") {
        return Err(Error::InvalidState);
    }
    Ok(Retained {
        archive,
        diagnostics: WholeDiagnostics {
            authority: selected,
            comparison,
            comparisons: row.try_get("comparisons")?,
        },
    })
}
pub(super) async fn initialize(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    stored: &Stored,
    bundles: &Bundles,
    mode: Mode,
    fresh: Option<CoreArchiveV5>,
) -> Result<(), Error> {
    sqlx::query("CREATE TABLE IF NOT EXISTS candidate_reference (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), authority TEXT NOT NULL CHECK(authority IN ('rust','shadow','sandbox')), archive BLOB NOT NULL CHECK(length(archive) <= 67108864), comparison TEXT NOT NULL CHECK(comparison IN ('disabled','matched','different','incomplete')), comparisons INTEGER NOT NULL CHECK(comparisons >= 0))").execute(&mut **tx).await?;
    if let Some(archive) = fresh {
        if bytes(&archive)? != bytes(&candidate(stored, bundles, mode)?)? {
            return Err(Error::InvalidState);
        }
        let selected = authority(mode)?;
        sqlx::query("INSERT INTO candidate_reference(singleton, authority, archive, comparison, comparisons) VALUES(1, ?, ?, ?, 0)")
            .bind(selected.key()).bind(bytes(&archive)?).bind(if selected == Authority::Rust { "disabled" } else { "matched" }).execute(&mut **tx).await?;
    }
    let retained = retained(tx, mode).await?;
    let state = directory::decode(stored, mode)?;
    if retained.archive.checkpoint.info != state.shell.info {
        return Err(Error::InvalidState);
    }
    if retained.diagnostics.comparison == "matched"
        && bytes(&retained.archive)? != bytes(&candidate(stored, bundles, mode)?)?
    {
        return Err(Error::InvalidState);
    }
    Ok(())
}
pub(super) async fn diagnostics(pool: &SqlitePool, mode: Mode) -> Result<WholeDiagnostics, Error> {
    let mut tx = pool.begin().await?;
    let result = retained(&mut tx, mode).await?.diagnostics;
    tx.rollback().await?;
    Ok(result)
}
pub(super) async fn export(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
) -> Result<CoreArchiveV5, Error> {
    let mut tx = pool.begin().await?;
    let native = retained(&mut tx, mode).await?;
    let result = if authority(mode)? == Authority::Sandbox {
        candidate(
            &load(&mut tx, bundles.identity(), mode).await?,
            bundles,
            mode,
        )?
    } else {
        native.archive
    };
    reference(mode, result.clone())?;
    tx.rollback().await?;
    Ok(result)
}
fn applied(evaluation: directory::Evaluation) -> Result<Box<directory::Applied>, Error> {
    match evaluation {
        directory::Evaluation::Applied(applied) => Ok(applied),
        directory::Evaluation::Replay(_) => Err(Error::InvalidState),
    }
}
fn validated_candidate(applied: &directory::Applied, mode: Mode) -> Result<&CoreArchiveV5, Error> {
    let archive = applied.archive.as_ref().ok_or(Error::IncompleteCoverage)?;
    reference(mode, archive.clone())?;
    Ok(archive)
}
fn evaluate_candidate(
    bundles: &Bundles,
    stored: &Stored,
    mode: Mode,
    verified: VerifiedCall,
    inputs: ExecutionInputs,
) -> Result<Box<directory::Applied>, Error> {
    let evaluated = directory::evaluate(bundles, stored, mode, verified, inputs.now, Some(inputs))?;
    let applied = applied(evaluated)?;
    validated_candidate(&applied, mode)?;
    Ok(applied)
}
pub(super) async fn execute(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    call: SignedContractCallV1,
    now: UnixSeconds,
    supplied: Option<ExecutionInputs>,
) -> Result<OperationReceipt, Error> {
    let verified = VerifiedCall::verify(call)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let mut native = retained(&mut tx, mode).await?;
    let selected = authority(mode)?;
    let candidate_shell = directory::decode(&stored, mode)?.shell;
    if native.archive.checkpoint.info != candidate_shell.info {
        return Err(Error::InvalidState);
    }
    let primary_shell = if selected == Authority::Sandbox {
        candidate_shell
    } else {
        native.archive.checkpoint.shell()
    };
    let admission = match supplied {
        Some(inputs) => primary_shell.prepare_with_inputs(verified.clone(), inputs)?,
        None => primary_shell.prepare(verified.clone(), now)?,
    };
    let inputs = match admission {
        Admission::Replay(receipt) => {
            tx.rollback().await?;
            return Ok(receipt);
        }
        Admission::Execute(pending) => pending.inputs().clone(),
    };
    let comparing = native.diagnostics.comparison == "matched";
    // A failed metered primary must not start unmetered native comparison work.
    let mut candidate_result = if selected == Authority::Sandbox {
        Some(evaluate_candidate(
            bundles,
            &stored,
            mode,
            verified.clone(),
            inputs.clone(),
        )?)
    } else {
        None
    };
    let mut native_receipt = None;
    if selected != Authority::Sandbox || comparing {
        let result = reference(mode, native.archive.clone())
            .and_then(|state| {
                #[cfg(test)]
                NATIVE_EXECUTIONS.with(|count| count.set(count.get() + 1));
                state
                    .execute(verified.clone(), inputs.clone())
                    .map_err(Error::Reference)
            })
            .and_then(|(state, receipt)| {
                let archive = state.archive().map_err(Error::Reference)?;
                bytes(&archive)?;
                Ok((archive, receipt))
            });
        match result {
            Ok((archive, receipt)) => {
                native.archive = archive;
                native_receipt = Some(receipt);
            }
            Err(error) if selected != Authority::Sandbox => return Err(error),
            Err(_) => native.diagnostics.comparison = "incomplete".into(),
        }
    }
    if selected == Authority::Shadow && comparing {
        match evaluate_candidate(bundles, &stored, mode, verified, inputs) {
            Ok(applied) => candidate_result = Some(applied),
            Err(_) => native.diagnostics.comparison = "incomplete".into(),
        }
    }
    if comparing {
        native.diagnostics.comparisons = native
            .diagnostics
            .comparisons
            .checked_add(1)
            .ok_or(Error::Limit)?;
        if native.diagnostics.comparison == "matched" {
            use comparison::{Comparison, Observation, ObservedOutcome};
            let candidate = candidate_result.as_ref().ok_or(Error::InvalidState)?;
            let outcome = native_receipt.as_ref().ok_or(Error::InvalidState)?;
            native.diagnostics.comparison = match comparison::compare(
                Observation {
                    archive: Some(&native.archive),
                    outcome: ObservedOutcome::Receipt(outcome),
                },
                Observation {
                    archive: candidate.archive.as_ref(),
                    outcome: ObservedOutcome::Receipt(&candidate.receipt),
                },
            )? {
                Comparison::Agreement { .. } => "matched",
                Comparison::Difference { .. } => "different",
                Comparison::Incomplete => "incomplete",
            }
            .into();
        }
    }
    let receipt = if selected == Authority::Sandbox {
        candidate_result
            .as_ref()
            .ok_or(Error::InvalidState)?
            .receipt
            .clone()
    } else {
        native_receipt.ok_or(Error::InvalidState)?
    };
    if let Some(applied) = candidate_result {
        directory::persist(&mut tx, mode, stored.revision, &applied).await?;
    } else {
        let next = stored.revision.checked_add(1).ok_or(Error::Limit)?;
        let changed = sqlx::query("UPDATE candidate_state SET revision = ? WHERE singleton = 1 AND revision = ? AND mode = ?").bind(next).bind(stored.revision).bind(mode.key()).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(Error::StaleRevision);
        }
    }
    let changed = sqlx::query("UPDATE candidate_reference SET archive = ?, comparison = ?, comparisons = ? WHERE singleton = 1 AND authority = ?")
        .bind(bytes(&native.archive)?).bind(&native.diagnostics.comparison).bind(native.diagnostics.comparisons).bind(selected.key()).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(Error::InvalidState);
    }
    tx.commit().await?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sandbox_exhaustion_does_not_execute_native_reference() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let fixtures: serde_json::Value = serde_json::from_str(include_str!(
                    "../../../../services/mock-provider/src/domain/fixtures/migration-signed-v5.json"
                ))
                .unwrap();
                let source = fixtures["archives"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["beforeStep"] == 70)
                    .unwrap()["archive"]
                    .as_str()
                    .unwrap();
                let archive: CoreArchiveV5 = serde_json::from_str(source).unwrap();
                let configuration = WholeConfiguration {
                    root: archive.checkpoint.info.root_account,
                    generation: archive.checkpoint.info.provider_instance_id,
                    mock_funding: true,
                    gas_limit: Weight::from_parts(1, 1),
                };
                let selected = mode(&configuration, Authority::Sandbox);
                let path = std::env::temp_dir()
                    .join(format!("ink-reference-order-{}.sqlite", std::process::id()));
                assert!(!path.exists());
                let bundles = Bundles::pinned().unwrap();
                let pool = open(
                    path.clone(),
                    InitialState::WholeImport {
                        archive: Box::new(archive),
                        configuration,
                        authority: Authority::Sandbox,
                    },
                    &bundles,
                )
                .await
                .unwrap();
                let trace: serde_json::Value = serde_json::from_str(include_str!(
                    "../../../../services/mock-provider/src/domain/fixtures/financial-signed-v5.json"
                ))
                .unwrap();
                let step = &trace["steps"][70];
                let call = serde_json::from_value(step["signed"].clone()).unwrap();
                let inputs: ExecutionInputs =
                    serde_json::from_value(step["inputs"].clone()).unwrap();
                NATIVE_EXECUTIONS.with(|count| count.set(0));
                let result =
                    execute(&pool, &bundles, selected, call, inputs.now, Some(inputs)).await;
                let native_executions = NATIVE_EXECUTIONS.with(std::cell::Cell::get);
                pool.close().await;
                std::fs::remove_file(path).unwrap();
                assert!(matches!(result, Err(Error::ExecutionLimit(_))));
                assert_eq!(
                    native_executions, 0,
                    "metered primary failure must precede native reference work"
                );
            });
    }
}
