//! M3 ABI bridge. Domain permissions and rules run only in the pinned contracts.
use super::*;
use generated_contracts::{
    CatalogEntry, DomainEventKind, EntityId, ProviderCommand, ProviderInstanceId, SkillRequestView,
    SkillRoleAssociation, WorkerView,
};
use parity_scale_codec::DecodeAll;
use provider_execution::{Admission, Effect, ExecutionInputs, VerifiedCall};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type DirectoryCatalog = (
    Vec<CatalogEntry>,
    Vec<CatalogEntry>,
    Vec<SkillRoleAssociation>,
);
pub type DirectoryProjection = (
    Vec<CatalogEntry>,
    Vec<CatalogEntry>,
    Vec<SkillRoleAssociation>,
    Vec<WorkerView>,
    Vec<SkillRequestView>,
);

/// Explicit genesis inputs, never inferred from a partial application snapshot.
pub struct DirectoryFixture {
    pub root: AccountId32,
    pub funded_accounts: Vec<AccountId32>,
    pub catalog: DirectoryCatalog,
    pub reservation_controller: AccountId32,
    pub reference_controller: AccountId32,
    /// A trusted parity fixture may reuse the reference generation.
    pub generation: Option<ProviderInstanceId>,
    pub gas_limit: Weight,
}
impl DirectoryFixture {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.funded_accounts.len() > 127
            || self.catalog.encoded_size() > MAX_INPUT
            || !self.gas_limit.all_lte(GAS)
        {
            return Err(Error::Limit);
        }
        Ok(())
    }
    pub(super) fn mode(&self) -> Mode {
        let accounts: BTreeSet<_> = self.funded_accounts.iter().copied().collect();
        let configuration = sp_core::hashing::blake2_256(
            &(
                self.root,
                accounts,
                &self.catalog,
                self.reservation_controller,
                self.reference_controller,
                self.generation,
                self.gas_limit.ref_time(),
                self.gas_limit.proof_size(),
            )
                .encode(),
        );
        Mode::Directory {
            root: self.root,
            gas: self.gas_limit,
            configuration: H256::from(configuration),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    configuration: H256,
    shell: Shell,
    directory: Account,
}
#[derive(Decode)]
struct ContractEffect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: Vec<AccountId32>,
}
fn account(value: AccountId32) -> Account {
    Account::new(value.into_bytes())
}
fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut bytes = sp_core::hashing::blake2_256(name.as_bytes())[..4].to_vec();
    bytes.extend(args.encode());
    bytes
}
fn encode(state: &State) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(state).map_err(|_| Error::InvalidState)?;
    if bytes.len() > MAX_SHELL {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
fn decode(stored: &Stored, mode: Mode) -> Result<State, Error> {
    let Mode::Directory {
        root,
        configuration,
        ..
    } = mode
    else {
        return Err(Error::InvalidState);
    };
    let state: State = serde_json::from_slice(&stored.shell).map_err(|_| Error::InvalidState)?;
    if state.version != 1 || state.configuration != configuration {
        return Err(Error::InvalidState);
    }
    state.shell.validate(root)?;
    Ok(state)
}
pub(super) fn initialize(
    bundles: &Bundles,
    fixture: DirectoryFixture,
) -> Result<(sp_io::TestExternalities, Vec<u8>), Error> {
    fixture.validate()?;
    let Mode::Directory { configuration, .. } = fixture.mode() else {
        return Err(Error::InvalidState);
    };
    let accounts: BTreeSet<_> = fixture
        .funded_accounts
        .into_iter()
        .chain([fixture.root])
        .collect();
    let mut ext = new_externalities(accounts.into_iter().map(account).collect())?;
    let calendar = ext.execute_with(|| {
        Contracts::bare_upload_code(
            account(fixture.root),
            bundles.code("calendar-v5")?.to_vec(),
            None,
            Determinism::Enforced,
        )
        .map(|code| code.code_hash)
        .map_err(|_| Error::InvalidState)
    })?;
    let invocation = Invocation {
        caller: account(fixture.root),
        action: Action::Instantiate {
            bundle: "directory-v5".into(),
            salt: b"signed-directory-v1".to_vec(),
        },
        input: input(
            "new",
            (
                calendar,
                account(fixture.reservation_controller),
                account(fixture.reference_controller),
                fixture.catalog,
            ),
        ),
        gas: GAS,
        timestamp_ms: 1_700_000_000_000,
    };
    invocation.validate()?;
    let created = run(&mut ext, bundles, invocation)?;
    if created.status != Status::Success || created.data != [0, 0] {
        return Err(Error::InvalidState);
    }
    let mut shell = Shell::new(fixture.root)?;
    if let Some(generation) = fixture.generation {
        shell.info.provider_instance_id = generation;
    }
    let state = State {
        version: 1,
        configuration,
        shell,
        directory: created.address.ok_or(Error::InvalidState)?,
    };
    Ok((ext, encode(&state)?))
}
pub(super) fn validate(stored: &Stored, bundles: &Bundles, mode: Mode) -> Result<(), Error> {
    let state = decode(stored, mode)?;
    let mut ext = stored.checkpoint.clone().restore()?;
    let code = ext.execute_with(|| Contracts::code_hash(&state.directory));
    if code
        != Some(H256::from(sp_core::hashing::blake2_256(
            bundles.code("directory-v5")?,
        )))
    {
        return Err(Error::InvalidState);
    }
    Ok(())
}
fn command_input(command: &ProviderCommand, inputs: &ExecutionInputs) -> Result<Vec<u8>, Error> {
    // M3 commands allocate at most one ID, before any random selection. This is
    // the same first 16 bytes of the version-1 locked native StdRng stream.
    let id = || {
        let mut bytes = [0; 16];
        StdRng::from_seed(inputs.seed).fill_bytes(&mut bytes);
        EntityId::from_bytes(bytes)
    };
    Ok(match command {
        ProviderCommand::RegisterWorker(request) => input("register_worker", (request, id())),
        ProviderCommand::UpdateQualifications(request) => input("update_qualifications", request),
        ProviderCommand::SetCalendar(definition) => input("set_calendar", definition),
        ProviderCommand::SetWorkerMode(request) => input("set_worker_mode", request),
        ProviderCommand::PromoteCoordinator(request) => input("promote_coordinator", request),
        ProviderCommand::UpsertCatalogEntry(request) => input("upsert_catalog_entry", request),
        ProviderCommand::DeleteCatalogEntry(request) => input("delete_catalog_entry", request),
        ProviderCommand::CreateSkillRequest(request) => {
            input("create_skill_request", (request, id()))
        }
        ProviderCommand::DecideSkillRequest(request) => input("decide_skill_request", request),
        _ => return Err(Error::IncompleteCoverage),
    })
}
fn transition(outcome: &Outcome) -> Result<Result<Effect, &'static str>, Error> {
    match outcome.status {
        Status::Success | Status::Reverted => {
            let decoded =
                Result::<Result<ContractEffect, String>, u8>::decode_all(&mut &outcome.data[..])
                    .map_err(|_| Error::InvalidState)?
                    .map_err(|_| Error::InvalidState)?;
            match (outcome.status.clone(), decoded) {
                (Status::Success, Ok(effect)) => {
                    let recipients: BTreeSet<_> = effect.recipients.iter().copied().collect();
                    if recipients.iter().copied().collect::<Vec<_>>() != effect.recipients {
                        return Err(Error::InvalidState);
                    }
                    Ok(Ok(Effect {
                        kind: effect.kind,
                        entity_id: effect.entity_id,
                        recipients,
                    }))
                }
                (Status::Reverted, Err(code)) => Ok(Err(domain_error(&code)?)),
                _ => Err(Error::InvalidState),
            }
        }
        // Known metered execution failures are failed operations, as in the M2
        // fixture. Unknown host/runtime errors abort without touching the shell.
        Status::LanguageError if outcome.data == [1, 1] => Ok(Err("sandbox_language_error")),
        Status::Dispatch(error)
            if error == pallet_contracts::Error::<crate::Runtime>::OutOfGas.into() =>
        {
            Ok(Err("sandbox_out_of_gas"))
        }
        Status::Dispatch(error)
            if error == pallet_contracts::Error::<crate::Runtime>::ContractTrapped.into() =>
        {
            Ok(Err("sandbox_contract_trapped"))
        }
        _ => Err(Error::InvalidState),
    }
}
fn domain_error(code: &str) -> Result<&'static str, Error> {
    // Only known public domain rejections can consume a nonce. In particular,
    // child-instantiation failure and invalid_worker_state are infrastructure /
    // invariant failures in the native engine, not accepted failed operations.
    match code {
        "invalid_contract" => Ok("invalid_contract"),
        "worker_exists" => Ok("worker_exists"),
        "worker_not_found" => Ok("worker_not_found"),
        "unknown_qualification" => Ok("unknown_qualification"),
        "coordinator_not_eligible" => Ok("coordinator_not_eligible"),
        "system_origin_required" => Ok("system_origin_required"),
        "fixed_coordinator_role" => Ok("fixed_coordinator_role"),
        "catalog_entry_in_use" => Ok("catalog_entry_in_use"),
        "catalog_entry_not_found" => Ok("catalog_entry_not_found"),
        "skill_name_exists" => Ok("skill_name_exists"),
        "unknown_role" => Ok("unknown_role"),
        "request_id_collision" => Ok("request_id_collision"),
        "skill_request_not_found" => Ok("skill_request_not_found"),
        "skill_request_decided" => Ok("skill_request_decided"),
        "catalog_id_exhausted" => Ok("catalog_id_exhausted"),
        "calendar_overcommitted" => Ok("calendar_overcommitted"),
        "arithmetic_or_calendar_error" => Ok("arithmetic_or_calendar_error"),
        "invalid_worker_state" => Err(Error::Invariant("invalid_worker_state")),
        "invalid_catalog_state" => Err(Error::Invariant("invalid_catalog_state")),
        "calendar_instantiation_failed" => Err(Error::Invariant("calendar_instantiation_failed")),
        _ => Err(Error::InvalidState),
    }
}
pub(super) async fn execute(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    call: SignedContractCallV1,
    now: UnixSeconds,
    inputs: Option<ExecutionInputs>,
) -> Result<OperationReceipt, Error> {
    let Mode::Directory { gas, .. } = mode else {
        return Err(Error::InvalidState);
    };
    let verified = VerifiedCall::verify(call)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let mut state = decode(&stored, mode)?;
    let admission = match inputs {
        Some(inputs) => state.shell.prepare_with_inputs(verified, inputs)?,
        None => state.shell.prepare(verified, now)?,
    };
    let pending = match admission {
        Admission::Replay(receipt) => {
            tx.rollback().await?;
            return Ok(receipt);
        }
        Admission::Execute(pending) => *pending,
    };
    let invocation = Invocation {
        caller: account(pending.call().origin),
        action: Action::Call {
            address: state.directory.clone(),
        },
        input: command_input(&pending.call().command, pending.inputs())?,
        gas,
        timestamp_ms: pending
            .inputs()
            .now
            .get()
            .checked_mul(1000)
            .ok_or(Error::Limit)?,
    };
    invocation.validate()?;
    let mut ext = stored.checkpoint.clone().restore()?;
    let outcome = run(&mut ext, bundles, invocation)?;
    let transition = transition(&outcome)?;
    let success = transition.is_ok();
    let receipt = state.shell.complete(pending, transition)?;
    let checkpoint = if success {
        Checkpoint::capture(ext)?
    } else {
        stored.checkpoint
    };
    let revision = stored.revision.checked_add(1).ok_or(Error::Limit)?;
    let changed = sqlx::query("UPDATE candidate_state SET revision = ?, runtime = ?, shell = ?, outcome = ? WHERE singleton = 1 AND revision = ? AND mode = ?")
        .bind(revision).bind(checkpoint.to_bytes()?).bind(encode(&state)?).bind(outcome.encode()).bind(stored.revision).bind(mode.key()).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(Error::StaleRevision);
    }
    tx.commit().await?;
    Ok(receipt)
}
pub(super) async fn read_shell(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
) -> Result<Shell, Error> {
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let state = decode(&stored, mode)?;
    tx.rollback().await?;
    Ok(state.shell)
}
fn read<T: Decode>(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    directory: &Account,
    origin: AccountId32,
    name: &str,
) -> Result<T, Error> {
    let outcome = run(
        ext,
        bundles,
        Invocation {
            caller: account(origin),
            action: Action::Call {
                address: directory.clone(),
            },
            input: input(name, ()),
            gas: GAS,
            timestamp_ms: 1_700_000_000_000,
        },
    )?;
    if outcome.status != Status::Success {
        return Err(Error::InvalidState);
    }
    Result::<T, u8>::decode_all(&mut &outcome.data[..])
        .map_err(|_| Error::InvalidState)?
        .map_err(|_| Error::InvalidState)
}
pub(super) async fn projection(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    origin: AccountId32,
) -> Result<DirectoryProjection, Error> {
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let state = decode(&stored, mode)?;
    let mut ext = stored.checkpoint.restore()?;
    let catalog: DirectoryCatalog = read(&mut ext, bundles, &state.directory, origin, "catalog")?;
    let workers = read(&mut ext, bundles, &state.directory, origin, "workers")?;
    let requests = read(
        &mut ext,
        bundles,
        &state.directory,
        origin,
        "skill_requests",
    )?;
    tx.rollback().await?;
    Ok((catalog.0, catalog.1, catalog.2, workers, requests))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: Status, data: Vec<u8>) -> Outcome {
        Outcome {
            status,
            data,
            address: None,
            gas_consumed: Weight::zero(),
        }
    }

    #[test]
    fn unknown_abi_errors_and_result_flags_abort_without_shell_finalization() {
        for code in [
            "invalid_worker_state",
            "invalid_catalog_state",
            "calendar_instantiation_failed",
        ] {
            let data = Result::<Result<(), String>, u8>::Ok(Err(code.into())).encode();
            assert!(
                matches!(transition(&outcome(Status::Reverted, data)), Err(Error::Invariant(observed)) if observed == code)
            );
        }
        let rejected =
            Result::<Result<(), String>, u8>::Ok(Err("worker_not_found".into())).encode();
        assert!(matches!(
            transition(&outcome(Status::Reverted, rejected.clone())),
            Ok(Err("worker_not_found"))
        ));
        let effect = (
            DomainEventKind::WorkerRegistered,
            Some(EntityId::from_bytes([1; 16])),
            vec![AccountId32::from_bytes([2; 32])],
        );
        let success = Result::<Result<_, String>, u8>::Ok(Ok(effect)).encode();
        assert!(matches!(
            transition(&outcome(Status::Success, success.clone())),
            Ok(Ok(_))
        ));
        let unknown_error =
            Result::<Result<(), String>, u8>::Ok(Err("unknown_future_rejection".into())).encode();
        let duplicate = Result::<Result<_, String>, u8>::Ok(Ok((
            DomainEventKind::WorkerRegistered,
            None::<EntityId>,
            vec![AccountId32::from_bytes([2; 32]); 2],
        )))
        .encode();
        for (status, data) in [
            (Status::Success, rejected),
            (Status::Reverted, success.clone()),
            (Status::Reverted, unknown_error),
            (Status::Success, duplicate),
            (Status::Success, [success, vec![99]].concat()),
            (Status::LanguageError, vec![1, 99]),
            (
                Status::Dispatch(DispatchError::Other("unknown host error")),
                vec![],
            ),
        ] {
            assert!(matches!(
                transition(&outcome(status, data)),
                Err(Error::InvalidState)
            ));
        }
    }
}
