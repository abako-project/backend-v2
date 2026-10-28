//! Signed ABI routing. Domain permissions and rules run only in the pinned contracts.
use super::*;
use crate::transport::execution::{Admission, Effect, ExecutionInputs, VerifiedCall};
use parity_scale_codec::DecodeAll;
use protocol::provider_core::{CatalogEntryCore, SkillRoleCore};
use protocol::{DomainEventKind, EntityId, ProviderCommand};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type DaoCatalog = (
    Vec<CatalogEntryCore>,
    Vec<CatalogEntryCore>,
    Vec<SkillRoleCore>,
);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub(super) version: u32,
    pub(super) configuration: H256,
    pub(super) shell: Shell,
    pub(super) dao: Account,
}
#[derive(Decode)]
struct ContractEffect {
    kind: DomainEventKind,
    entity_id: Option<EntityId>,
    recipients: Vec<AccountId32>,
    allocated_task_id: Option<u32>,
}
fn account(value: AccountId32) -> Account {
    Account::new(value.into_bytes())
}
pub(super) fn input(name: &str, args: impl Encode) -> Vec<u8> {
    let mut bytes = sp_core::hashing::blake2_256(name.as_bytes())[..4].to_vec();
    bytes.extend(args.encode());
    bytes
}
pub(super) fn encode(state: &State) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(state).map_err(|_| Error::InvalidState)?;
    if bytes.len() > MAX_SHELL {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
pub(super) fn decode(stored: &Stored, mode: Mode) -> Result<State, Error> {
    decode_shell(&stored.shell, mode)
}
fn decode_shell(bytes: &[u8], mode: Mode) -> Result<State, Error> {
    let Mode::Contracts {
        root,
        configuration,
        ..
    } = mode
    else {
        return Err(Error::InvalidState);
    };
    let state: State = serde_json::from_slice(bytes).map_err(|_| Error::InvalidState)?;
    if state.version != 1 || state.configuration != configuration {
        return Err(Error::InvalidState);
    }
    state.shell.validate(root)?;
    Ok(state)
}
pub(super) fn initialize(
    bundles: &Bundles,
    settings: Configuration,
    catalog: DaoCatalog,
    generation: protocol::ProviderInstanceId,
) -> Result<(sp_io::TestExternalities, Vec<u8>), Error> {
    settings.validate()?;
    let Mode::Contracts { configuration, .. } = settings.mode() else {
        return Err(Error::InvalidState);
    };
    let mut ext = new_externalities(vec![account(settings.root)])?;
    let invocation = Invocation {
        caller: account(settings.root),
        action: Action::Instantiate {
            bundle: "dao".into(),
            salt: b"contracts".to_vec(),
        },
        input: input(
            "new",
            (account(settings.root), account(settings.root), catalog),
        ),
        gas: GAS,
        timestamp_ms: 1_700_000_000_000,
    };
    invocation.validate()?;
    let created = run(&mut ext, bundles, invocation)?;
    if created.status != Status::Success || created.data != [0, 0] {
        return Err(Error::InvalidState);
    }
    {
        let code = ext.execute_with(|| {
            Contracts::bare_upload_code(
                account(settings.root),
                bundles.code("project")?.to_vec(),
                None,
                Determinism::Enforced,
            )
            .map(|upload| upload.code_hash)
            .map_err(|_| Error::InvalidState)
        })?;
        let configured = run(
            &mut ext,
            bundles,
            Invocation {
                caller: account(settings.root),
                action: Action::Call {
                    address: created.address.clone().ok_or(Error::InvalidState)?,
                },
                input: input("configure_project_code", code),
                gas: GAS,
                timestamp_ms: 1_700_000_000_000,
            },
        )?;
        if configured.status != Status::Success || configured.data != [0, 0] {
            return Err(Error::InvalidState);
        }
    }
    {
        let mock_funding = settings.mock_funding;
        let address = created.address.clone().ok_or(Error::InvalidState)?;
        ext.execute_with(|| -> Result<(), Error> {
            crate::Assets::force_create(
                crate::RuntimeOrigin::root(),
                1,
                account(settings.root),
                true,
                1,
            )
            .map_err(|_| Error::InvalidState)?;
            // Inventory is local setup, never visible issued/free payment units. It covers
            // u64::MAX visible units and separate bounded technical minima.
            crate::Assets::mint(
                crate::RuntimeOrigin::signed(account(settings.root)),
                1,
                address.clone(),
                u128::from(u64::MAX) * 2,
            )
            .map_err(|_| Error::InvalidState)?;
            Ok(())
        })?;
        let configured = run(
            &mut ext,
            bundles,
            Invocation {
                caller: account(settings.root),
                action: Action::Call { address },
                input: input("configure_accounting", mock_funding),
                gas: GAS,
                timestamp_ms: 1_700_000_000_000,
            },
        )?;
        if configured.status != Status::Success || configured.data != [0, 0] {
            return Err(Error::InvalidState);
        }
    }
    let mut shell = Shell::new(settings.root)?;
    shell.info.provider_instance_id = generation;
    let state = State {
        version: 1,
        configuration,
        shell,
        dao: created.address.ok_or(Error::InvalidState)?,
    };
    Ok((ext, encode(&state)?))
}
pub(super) fn validate(stored: &Stored, bundles: &Bundles, mode: Mode) -> Result<(), Error> {
    let state = decode(stored, mode)?;
    let mut ext = stored.checkpoint.clone().restore()?;
    let code = ext.execute_with(|| Contracts::code_hash(&state.dao));
    if code
        != Some(H256::from(sp_core::hashing::blake2_256(
            bundles.code("dao")?,
        )))
    {
        return Err(Error::InvalidState);
    }
    let enabled: bool = read(
        &mut ext,
        bundles,
        &state.dao,
        state.shell.info.root_account,
        "accounting_enabled",
        (),
    )?;
    if !enabled {
        return Err(Error::InvalidState);
    }
    if let Mode::Contracts { root, .. } = mode {
        let addresses: Vec<(EntityId, Account)> =
            read(&mut ext, bundles, &state.dao, root, "projects", ())?;
        let expected = H256::from(sp_core::hashing::blake2_256(bundles.code("project")?));
        for (_, address) in addresses {
            if ext.execute_with(|| Contracts::code_hash(&address)) != Some(expected) {
                return Err(Error::InvalidState);
            }
        }
    }
    super::facts::read(&mut ext, &state)?;
    Ok(())
}
fn command_input(command: &ProviderCommand, inputs: &ExecutionInputs) -> Result<Vec<u8>, Error> {
    // Dao commands allocate at most one ID, before any random selection. This is
    // the same first 16 bytes of the version-1 locked native StdRng stream.
    let id = || {
        let mut bytes = [0; 16];
        StdRng::from_seed(inputs.seed).fill_bytes(&mut bytes);
        EntityId::from_bytes(bytes)
    };
    Ok(match command {
        ProviderCommand::FundAccount(request) => {
            input("fund_account", (request.account, request.amount))
        }
        ProviderCommand::CreateDeposit(request) => {
            input("create_deposit", (request.amount, inputs.seed))
        }
        ProviderCommand::ConfirmDeposit { deposit_id } => input("confirm_deposit", deposit_id),
        ProviderCommand::CreateWithdrawal(request) => {
            input("create_withdrawal", (request.amount, inputs.seed))
        }
        ProviderCommand::CancelWithdrawal { withdrawal_id } => {
            input("cancel_withdrawal", withdrawal_id)
        }

        ProviderCommand::RegisterWorkerCore(request) => input("register_worker", (request, id())),
        ProviderCommand::UpdateQualifications(request) => input("update_qualifications", request),
        ProviderCommand::SetCalendar(definition) => input("set_calendar", definition),
        ProviderCommand::SetScorePolicy(policy) => input("set_score_policy", policy),
        ProviderCommand::SetWorkerMode(request) => input("set_worker_mode", request),
        ProviderCommand::PromoteCoordinator(request) => input("promote_coordinator", request),
        ProviderCommand::UpsertCatalogEntryCore { kind, id, content } => {
            input("upsert_catalog_entry", (kind, id, content))
        }
        ProviderCommand::DeleteCatalogEntry(request) => input("delete_catalog_entry", request),
        ProviderCommand::CreateSkillRequestCore { role_ids, content } => {
            input("create_skill_request", (role_ids, content, id()))
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
                        allocated_task_id: effect.allocated_task_id,
                    }))
                }
                (Status::Reverted, Err(code)) => Ok(Err(domain_error(&code)?)),
                _ => Err(Error::InvalidState),
            }
        }
        // Translate metered failures for diagnostic decoding. Whole-provider
        // execution aborts infrastructure failures before shell completion.
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
        "project_not_found" => Ok("project_not_found"),
        "invalid_time" => Ok("invalid_time"),
        "coordinator_required" => Ok("coordinator_required"),
        "client_required" => Ok("client_required"),
        "planning_not_accepted" => Ok("planning_not_accepted"),
        "proposal_not_found" => Ok("proposal_not_found"),
        "proposal_not_draft" => Ok("proposal_not_draft"),
        "proposal_not_pending" => Ok("proposal_not_pending"),
        "revision_conflict" => Ok("revision_conflict"),
        "revision_exhausted" => Ok("revision_exhausted"),
        "execution_already_approved" => Ok("execution_already_approved"),
        "invalid_planning_state" => Ok("invalid_planning_state"),
        "separate_planning_price_removed" => Ok("separate_planning_price_removed"),
        "task_storage_not_found" => Ok("task_storage_not_found"),
        "task_not_found" => Ok("task_not_found"),
        "unknown_task_assignee" => Ok("unknown_task_assignee"),
        "task_keys_exhausted" => Ok("task_keys_exhausted"),
        "task_assignee_required" => Ok("task_assignee_required"),
        "client_read_only" => Ok("client_read_only"),
        "project_party_required" => Ok("project_party_required"),
        "project_completed" => Ok("project_completed"),
        "project_cancelled" => Ok("project_cancelled"),
        "project_disputed" => Ok("project_disputed"),
        "delivery_date_before_milestones" => Ok("delivery_date_before_milestones"),
        "invalid_delivery_date" => Ok("invalid_delivery_date"),
        "invalid_delivery_schedule" => Ok("invalid_delivery_schedule"),
        "empty_milestones" => Ok("empty_milestones"),
        "no_available_candidate" => Ok("no_available_candidate"),
        "invalid_project_state" => Err(Error::Invariant("invalid_project_state")),
        "duplicate_entity" => Err(Error::Invariant("duplicate_entity")),
        "bramp_id_collision" => Ok("duplicate_entity"),
        "insufficient_balance" => Ok("insufficient_balance"),
        "mock_funding_disabled" => Ok("mock_funding_disabled"),
        "deposit_not_found" => Ok("deposit_not_found"),
        "deposit_already_confirmed" => Ok("deposit_already_confirmed"),
        "withdrawal_not_found" => Ok("withdrawal_not_found"),
        "withdrawal_owner_or_system_required" => Ok("withdrawal_owner_or_system_required"),
        "withdrawal_already_cancelled" => Ok("withdrawal_already_cancelled"),
        "invalid_deposit_state" => Err(Error::Invariant("invalid_deposit_state")),
        "invalid_withdrawal_state" => Err(Error::Invariant("invalid_withdrawal_state")),
        "asset_transfer_failed" => Err(Error::Invariant("asset_transfer_failed")),
        "invalid_asset_state" => Err(Error::Invariant("invalid_asset_state")),
        "active_dispute_exists" => Ok("active_dispute_exists"),
        "milestone_not_changes_requested" => Ok("milestone_not_changes_requested"),
        "submission_not_current" => Ok("submission_not_current"),
        "submission_not_pending" => Ok("submission_not_pending"),
        "submission_not_rejected" => Ok("submission_not_rejected"),
        "milestone_not_found" => Ok("milestone_not_found"),
        "invalid_milestone_state" => Ok("invalid_milestone_state"),
        "worker_ratings_mismatch" => Ok("worker_ratings_mismatch"),
        "project_not_completed" => Ok("project_not_completed"),
        "project_already_evaluated" => Ok("project_already_evaluated"),
        "project_participant_required" => Ok("project_participant_required"),
        "evaluation_participants_mismatch" => Ok("evaluation_participants_mismatch"),
        "dispute_not_found" => Ok("dispute_not_found"),
        "invalid_dispute_target" => Ok("invalid_dispute_target"),
        "dispute_response_forbidden" => Ok("dispute_response_forbidden"),
        "dispute_already_answered" => Ok("dispute_already_answered"),
        "score_overflow" => Ok("score_overflow"),
        "invalid_review_state" => Err(Error::Invariant("invalid_review_state")),
        "invalid_dispute_response" => Err(Error::Invariant("invalid_dispute_response")),
        "invalid_submission_state" => Err(Error::Invariant("invalid_submission_state")),
        "invalid_dispute_state" => Err(Error::Invariant("invalid_dispute_state")),
        "invalid_dispute_id" => Err(Error::Invariant("invalid_dispute_id")),
        "invalid_assignment_state" => Err(Error::Invariant("invalid_assignment_state")),
        "provider_unavailable" => Err(Error::Invariant("provider_unavailable")),
        "invalid_storage_state" => Err(Error::Invariant("invalid_storage_state")),
        "project_instantiation_failed" => Err(Error::Invariant("project_instantiation_failed")),
        "project_code_missing" => Err(Error::Invariant("project_code_missing")),
        "registered_project_required" => Err(Error::Invariant("registered_project_required")),
        "incomplete_money_coverage" => Err(Error::IncompleteCoverage),
        "invalid_worker_state" => Err(Error::Invariant("invalid_worker_state")),
        "invalid_catalog_state" => Err(Error::Invariant("invalid_catalog_state")),
        "invalid_calendar_state" => Err(Error::Invariant("invalid_calendar_state")),
        _ => Err(Error::InvalidState),
    }
}
pub(super) struct Applied {
    pub(super) checkpoint: Checkpoint,
    pub(super) state: State,
    pub(super) receipt: OperationReceipt,
    pub(super) outcome: Outcome,
}
pub(super) enum Evaluation {
    Replay(OperationReceipt),
    Applied(Box<Applied>),
}
/// Evaluate tentatively; the caller owns the single durable commit boundary.
pub(super) fn evaluate(
    bundles: &Bundles,
    stored: &Stored,
    mode: Mode,
    verified: VerifiedCall,
    now: UnixSeconds,
) -> Result<Evaluation, Error> {
    let Mode::Contracts { gas, .. } = mode else {
        return Err(Error::InvalidState);
    };
    let mut state = decode(stored, mode)?;
    let admission = state.shell.prepare(verified, now)?;
    let pending = match admission {
        Admission::Replay(receipt) => {
            return Ok(Evaluation::Replay(receipt));
        }
        Admission::Execute(pending) => *pending,
    };
    let time = pending
        .inputs()
        .now
        .get()
        .checked_mul(1000)
        .ok_or(Error::Limit)?;
    let mut ext = stored.checkpoint.clone().restore()?;
    {
        ext.execute_with(|| {
            let origin = account(pending.call().origin);
            if crate::Balances::free_balance(&origin) == 0 {
                crate::Balances::force_set_balance(
                    crate::RuntimeOrigin::root(),
                    origin,
                    crate::TECHNICAL_BALANCE,
                )
                .map_err(|_| Error::InvalidState)?;
            }
            Ok::<(), Error>(())
        })?;
    }
    let event_cursor = state.shell.events.last().map_or(0, |event| event.cursor);
    let command = &pending.call().command;
    let outcome = match command_input(command, pending.inputs()) {
        Ok(data) => {
            let invocation = Invocation {
                caller: account(pending.call().origin),
                action: Action::Call {
                    address: state.dao.clone(),
                },
                input: data,
                gas,
                timestamp_ms: time,
            };
            invocation.validate_with_budget(crate::EXECUTION_GAS)?;
            run(&mut ext, bundles, invocation)?
        }
        Err(Error::IncompleteCoverage) => super::projects::execute(
            &mut ext,
            bundles,
            &mut state,
            &pending,
            gas,
            time,
            event_cursor,
        )?,
        Err(error) => return Err(error),
    };
    let transition = match super::projects::unsupported_planning(command, &outcome)? {
        Some(code) => Err(code),
        None => transition(&outcome)?,
    };
    // Resource/VM failures are infrastructure, not canonical business rejections.
    // Whole-provider retries must retain the nonce and original durable state.
    {
        match transition {
            Err("sandbox_out_of_gas") => return Err(Error::ExecutionLimit(outcome.gas_consumed)),
            Err("sandbox_contract_trapped" | "sandbox_language_error") => {
                return Err(Error::InvalidState);
            }
            _ => {}
        }
    }
    let success = transition.is_ok();
    let receipt = state.shell.complete(pending, transition)?;
    let checkpoint = if success {
        Checkpoint::capture(ext)?
    } else {
        stored.checkpoint.clone()
    };
    Ok(Evaluation::Applied(Box::new(Applied {
        checkpoint,
        state,
        receipt,
        outcome,
    })))
}
pub(super) async fn persist(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    mode: Mode,
    revision: i64,
    applied: &Applied,
) -> Result<(), Error> {
    let next = revision.checked_add(1).ok_or(Error::Limit)?;
    let changed = sqlx::query("UPDATE provider_state SET revision = ?, runtime = ?, shell = ?, outcome = ? WHERE singleton = 1 AND revision = ? AND mode = ?")
        .bind(next).bind(applied.checkpoint.to_bytes()?).bind(encode(&applied.state)?).bind(applied.outcome.encode())
        .bind(revision).bind(mode.key()).execute(&mut **tx).await?;
    if changed.rows_affected() != 1 {
        return Err(Error::StaleRevision);
    }
    Ok(())
}
pub(super) async fn execute(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    call: SignedContractCallV1,
    now: UnixSeconds,
) -> Result<OperationReceipt, Error> {
    let verified = VerifiedCall::verify(call)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    match evaluate(bundles, &stored, mode, verified, now)? {
        Evaluation::Replay(receipt) => {
            tx.rollback().await?;
            Ok(receipt)
        }
        Evaluation::Applied(applied) => {
            persist(&mut tx, mode, stored.revision, &applied).await?;
            tx.commit().await?;
            Ok(applied.receipt)
        }
    }
}
pub(super) async fn read_shell(pool: &SqlitePool, mode: Mode) -> Result<Shell, Error> {
    let bytes: Vec<u8> = sqlx::query_scalar("SELECT shell FROM provider_state WHERE singleton = 1 AND mode = ? AND revision >= 0 AND length(shell) <= 67108864")
        .bind(mode.key()).fetch_optional(pool).await?.ok_or(Error::InvalidState)?;
    Ok(decode_shell(&bytes, mode)?.shell)
}
pub(super) fn read<T: Decode>(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    dao: &Account,
    origin: AccountId32,
    name: &str,
    args: impl Encode,
) -> Result<T, Error> {
    let outcome = run(
        ext,
        bundles,
        Invocation {
            caller: account(origin),
            action: Action::Call {
                address: dao.clone(),
            },
            input: input(name, args),
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
// Read large labels independently so a valid signed profile remains exportable.
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
            "invalid_review_state",
            "invalid_dispute_response",
            "invalid_deposit_state",
            "invalid_withdrawal_state",
            "invalid_worker_state",
            "invalid_catalog_state",
            "invalid_calendar_state",
        ] {
            let data = Result::<Result<(), String>, u8>::Ok(Err(code.into())).encode();
            assert!(
                matches!(transition(&outcome(Status::Reverted, data)), Err(Error::Invariant(observed)) if observed == code)
            );
        }
        // Distinguish language,
        // metering and trap failures at the current signed ABI boundary.
        for (status, data, expected) in [
            (Status::LanguageError, vec![1, 1], "sandbox_language_error"),
            (
                Status::Dispatch(pallet_contracts::Error::<crate::Runtime>::OutOfGas.into()),
                vec![],
                "sandbox_out_of_gas",
            ),
            (
                Status::Dispatch(pallet_contracts::Error::<crate::Runtime>::ContractTrapped.into()),
                vec![],
                "sandbox_contract_trapped",
            ),
        ] {
            assert!(
                matches!(transition(&outcome(status, data)), Ok(Err(code)) if code == expected)
            );
        }
        let rejected =
            Result::<Result<(), String>, u8>::Ok(Err("worker_not_found".into())).encode();
        assert!(matches!(
            transition(&outcome(Status::Reverted, rejected.clone())),
            Ok(Err("worker_not_found"))
        ));
        let overflow = Result::<Result<(), String>, u8>::Ok(Err("score_overflow".into())).encode();
        assert!(matches!(
            transition(&outcome(Status::Reverted, overflow)),
            Ok(Err("score_overflow"))
        ));
        let effect = (
            DomainEventKind::WorkerRegistered,
            Some(EntityId::from_bytes([1; 16])),
            vec![AccountId32::from_bytes([2; 32])],
            None::<u32>,
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
            None::<u32>,
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
