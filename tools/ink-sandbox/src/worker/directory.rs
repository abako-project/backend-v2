//! M3 ABI bridge. Domain permissions and rules run only in the pinned contracts.
use super::*;
use generated_contracts::{
    CatalogEntry, CatalogView, CompensationPolicy, DomainEventKind, EntityId, ProjectView,
    ProviderCommand, ProviderInstanceId, ScorePolicy, SkillRequestView, SkillRoleAssociation,
    WorkerView,
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

pub type MarketplaceProjection = (
    CatalogView,
    Vec<WorkerView>,
    Vec<SkillRequestView>,
    Vec<ProjectView>,
    Vec<(
        EntityId,
        Option<EntityId>,
        AccountId32,
        String,
        generated_contracts::UnixSeconds,
        DomainEventKind,
    )>,
);

pub type FinancialProjection = (
    MarketplaceProjection,
    MoneyProjection,
    Vec<(
        generated_contracts::Dispute,
        generated_contracts::MilestoneView,
    )>,
);
pub type MoneyProjection = (
    Vec<(AccountId32, generated_contracts::Money)>,
    generated_contracts::Money,
    Vec<generated_contracts::DepositView>,
    Vec<generated_contracts::WithdrawalView>,
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
    /// Enable the explicit M5 Project fixture; money/execution remain incomplete.
    pub projects: bool,
    /// Explicit M6 coverage; Some controls test-only funding, None disables money.
    pub money: Option<bool>,
}
impl DirectoryFixture {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.funded_accounts.len() > 127
            || self.catalog.encoded_size() > MAX_INPUT
            || (self.money.is_some() && !self.projects)
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
                self.projects,
                self.money,
                self.gas_limit.ref_time(),
                self.gas_limit.proof_size(),
            )
                .encode(),
        );
        Mode::Directory {
            root: self.root,
            gas: self.gas_limit,
            configuration: H256::from(configuration),
            projects: self.projects,
            money: self.money.is_some(),
            whole: false,
            authority: None,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct State {
    pub(super) version: u32,
    pub(super) configuration: H256,
    pub(super) shell: Shell,
    pub(super) directory: Account,
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
    if fixture.projects {
        let code = ext.execute_with(|| {
            Contracts::bare_upload_code(
                account(fixture.root),
                bundles.code("projects-v5")?.to_vec(),
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
                caller: account(fixture.root),
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
    if fixture.projects {
        let code = ext.execute_with(|| {
            Contracts::bare_upload_code(
                account(fixture.root),
                bundles.code("references-v5")?.to_vec(),
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
                caller: account(fixture.root),
                action: Action::Call {
                    address: created.address.clone().ok_or(Error::InvalidState)?,
                },
                input: input("configure_reference_code", code),
                gas: GAS,
                timestamp_ms: 1_700_000_000_000,
            },
        )?;
        if configured.status != Status::Success || configured.data != [0, 0] {
            return Err(Error::InvalidState);
        }
    }
    if let Some(mock_funding) = fixture.money {
        let address = created.address.clone().ok_or(Error::InvalidState)?;
        ext.execute_with(|| -> Result<(), Error> {
            crate::Assets::force_create(
                crate::RuntimeOrigin::root(),
                1,
                account(fixture.root),
                true,
                1,
            )
            .map_err(|_| Error::InvalidState)?;
            // Inventory is local setup, never visible issued/free KVN. It covers
            // u64::MAX visible units and separate bounded technical minima.
            crate::Assets::mint(
                crate::RuntimeOrigin::signed(account(fixture.root)),
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
                caller: account(fixture.root),
                action: Action::Call { address },
                input: input("configure_money", mock_funding),
                gas: GAS,
                timestamp_ms: 1_700_000_000_000,
            },
        )?;
        if configured.status != Status::Success || configured.data != [0, 0] {
            return Err(Error::InvalidState);
        }
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
    let enabled: bool = read(
        &mut ext,
        bundles,
        &state.directory,
        state.shell.info.root_account,
        "money_enabled",
        (),
    )?;
    if enabled != matches!(mode, Mode::Directory { money: true, .. }) {
        return Err(Error::InvalidState);
    }
    if let Mode::Directory {
        projects: true,
        root,
        ..
    } = mode
    {
        let reference: Option<Account> = read(
            &mut ext,
            bundles,
            &state.directory,
            state.shell.info.root_account,
            "reference_address",
            (),
        )?;
        let reference = reference.ok_or(Error::InvalidState)?;
        if ext.execute_with(|| Contracts::code_hash(&reference))
            != Some(H256::from(sp_core::hashing::blake2_256(
                bundles.code("references-v5")?,
            )))
        {
            return Err(Error::InvalidState);
        }

        let addresses: Vec<(EntityId, Account)> =
            read(&mut ext, bundles, &state.directory, root, "projects", ())?;
        let expected = H256::from(sp_core::hashing::blake2_256(bundles.code("projects-v5")?));
        for (_, address) in addresses {
            if ext.execute_with(|| Contracts::code_hash(&address)) != Some(expected) {
                return Err(Error::InvalidState);
            }
        }
    }
    if matches!(mode, Mode::Directory { whole: true, .. }) {
        super::migration::read_archive(&mut ext, bundles, &state, true)?;
    }
    Ok(())
}
fn money_command(command: &ProviderCommand) -> bool {
    matches!(
        command,
        ProviderCommand::FundAccount(_)
            | ProviderCommand::CreateDeposit(_)
            | ProviderCommand::ConfirmDeposit { .. }
            | ProviderCommand::CreateWithdrawal(_)
            | ProviderCommand::CancelWithdrawal { .. }
    )
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

        ProviderCommand::RegisterWorker(request) => input("register_worker", (request, id())),
        ProviderCommand::UpdateQualifications(request) => input("update_qualifications", request),
        ProviderCommand::SetCalendar(definition) => input("set_calendar", definition),
        ProviderCommand::SetScorePolicy(policy) => input("set_score_policy", policy),
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
        "reference_call_failed" => Err(Error::Invariant("reference_call_failed")),
        "reference_instantiation_failed" => Err(Error::Invariant("reference_instantiation_failed")),
        "incomplete_reference_coverage" => Err(Error::IncompleteCoverage),
        "invalid_storage_state" => Err(Error::Invariant("invalid_storage_state")),
        "project_instantiation_failed" => Err(Error::Invariant("project_instantiation_failed")),
        "project_code_missing" => Err(Error::Invariant("project_code_missing")),
        "registered_project_required" => Err(Error::Invariant("registered_project_required")),
        "incomplete_money_coverage" => Err(Error::IncompleteCoverage),
        "invalid_worker_state" => Err(Error::Invariant("invalid_worker_state")),
        "invalid_catalog_state" => Err(Error::Invariant("invalid_catalog_state")),
        "calendar_instantiation_failed" => Err(Error::Invariant("calendar_instantiation_failed")),
        _ => Err(Error::InvalidState),
    }
}
pub(super) struct Applied {
    pub(super) checkpoint: Checkpoint,
    pub(super) state: State,
    pub(super) receipt: OperationReceipt,
    pub(super) outcome: Outcome,
    pub(super) archive: Option<provider_execution::migration::ArchiveV1>,
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
    inputs: Option<ExecutionInputs>,
) -> Result<Evaluation, Error> {
    let Mode::Directory { gas, .. } = mode else {
        return Err(Error::InvalidState);
    };
    let mut state = decode(stored, mode)?;
    let admission = match inputs {
        Some(inputs) => state.shell.prepare_with_inputs(verified, inputs)?,
        None => state.shell.prepare(verified, now)?,
    };
    let pending = match admission {
        Admission::Replay(receipt) => {
            return Ok(Evaluation::Replay(receipt));
        }
        Admission::Execute(pending) => *pending,
    };
    if (money_command(&pending.call().command)
        || super::projects::requires_money(&pending.call().command))
        && !matches!(mode, Mode::Directory { money: true, .. })
    {
        return Err(Error::IncompleteCoverage);
    }
    let time = pending
        .inputs()
        .now
        .get()
        .checked_mul(1000)
        .ok_or(Error::Limit)?;
    let mut ext = stored.checkpoint.clone().restore()?;
    if matches!(mode, Mode::Directory { whole: true, .. }) {
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
    let outcome = match command_input(&pending.call().command, pending.inputs()) {
        Ok(data) => {
            let invocation = Invocation {
                caller: account(pending.call().origin),
                action: Action::Call {
                    address: state.directory.clone(),
                },
                input: data,
                gas,
                timestamp_ms: time,
            };
            invocation.validate_with_budget(
                if matches!(mode, Mode::Directory { whole: true, .. }) {
                    crate::WHOLE_GAS
                } else {
                    GAS
                },
            )?;
            run(&mut ext, bundles, invocation)?
        }
        Err(Error::IncompleteCoverage)
            if matches!(mode, Mode::Directory { projects: true, .. }) =>
        {
            super::projects::execute(
                &mut ext,
                bundles,
                &state.directory,
                &pending,
                gas,
                time,
                state.shell.events.last().map_or(0, |event| event.cursor),
            )?
        }
        Err(error) => return Err(error),
    };
    let transition = transition(&outcome)?;
    // Resource/VM failures are infrastructure, not canonical business rejections.
    // Whole-provider retries must retain the nonce and original durable state.
    if matches!(mode, Mode::Directory { whole: true, .. }) {
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
    let archive = if matches!(mode, Mode::Directory { whole: true, .. }) {
        Some(super::migration::read_archive(
            &mut checkpoint.clone().restore()?,
            bundles,
            &state,
            true,
        )?)
    } else {
        None
    };
    Ok(Evaluation::Applied(Box::new(Applied {
        checkpoint,
        state,
        receipt,
        outcome,
        archive,
    })))
}
pub(super) async fn persist(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    mode: Mode,
    revision: i64,
    applied: &Applied,
) -> Result<(), Error> {
    let next = revision.checked_add(1).ok_or(Error::Limit)?;
    let changed = sqlx::query("UPDATE candidate_state SET revision = ?, runtime = ?, shell = ?, outcome = ? WHERE singleton = 1 AND revision = ? AND mode = ?")
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
    inputs: Option<ExecutionInputs>,
) -> Result<OperationReceipt, Error> {
    let verified = VerifiedCall::verify(call)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    match evaluate(bundles, &stored, mode, verified, now, inputs)? {
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
pub(super) fn read<T: Decode>(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    directory: &Account,
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
                address: directory.clone(),
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
pub(super) fn read_workers(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    origin: AccountId32,
) -> Result<Vec<WorkerView>, Error> {
    let _ = (bundles, origin);
    super::archive::workers(ext, state)
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
    let catalog: DirectoryCatalog =
        read(&mut ext, bundles, &state.directory, origin, "catalog", ())?;
    let workers = read_workers(&mut ext, bundles, &state, origin)?;
    let requests = read(
        &mut ext,
        bundles,
        &state.directory,
        origin,
        "skill_requests",
        (),
    )?;
    tx.rollback().await?;
    Ok((catalog.0, catalog.1, catalog.2, workers, requests))
}

pub(super) async fn marketplace_projection(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    origin: AccountId32,
) -> Result<MarketplaceProjection, Error> {
    if !matches!(mode, Mode::Directory { projects: true, .. }) {
        return Err(Error::IncompleteCoverage);
    }
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let state = decode(&stored, mode)?;
    let mut ext = stored.checkpoint.restore()?;
    let projection = read_marketplace(&mut ext, bundles, &state, origin)?;
    tx.rollback().await?;
    Ok(projection)
}
fn read_marketplace(
    ext: &mut sp_io::TestExternalities,
    bundles: &Bundles,
    state: &State,
    origin: AccountId32,
) -> Result<MarketplaceProjection, Error> {
    let (roles, skills, skill_roles): DirectoryCatalog =
        read(ext, bundles, &state.directory, origin, "catalog", ())?;
    let score_policy: ScorePolicy =
        read(ext, bundles, &state.directory, origin, "score_policy", ())?;
    let compensation_policy: CompensationPolicy = read(
        ext,
        bundles,
        &state.directory,
        origin,
        "compensation_policy",
        (),
    )?;
    let workers = read_workers(ext, bundles, state, origin)?;
    let requests = read(ext, bundles, &state.directory, origin, "skill_requests", ())?;
    let addresses: Vec<(EntityId, Account)> =
        read(ext, bundles, &state.directory, origin, "projects", ())?;
    let mut projects = Vec::new();
    let mut reasons: Vec<(
        EntityId,
        Option<EntityId>,
        AccountId32,
        String,
        generated_contracts::UnixSeconds,
        DomainEventKind,
    )> = Vec::new();
    for (id, address) in addresses {
        let project: ProjectView = read(ext, bundles, &address, origin, "view", ())?;
        if project.project_id != id {
            return Err(Error::InvalidState);
        }
        let entries: Vec<(
            EntityId,
            Option<EntityId>,
            AccountId32,
            String,
            generated_contracts::UnixSeconds,
            DomainEventKind,
        )> = read(ext, bundles, &address, origin, "reasons", ())?;
        reasons.extend(entries);
        projects.push(project);
    }
    reasons.sort_by_key(Encode::encode);
    Ok((
        CatalogView {
            roles,
            skills,
            skill_roles,
            score_policy,
            compensation_policy,
        },
        workers,
        requests,
        projects,
        reasons,
    ))
}

pub(super) async fn money_projection(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    origin: AccountId32,
) -> Result<MoneyProjection, Error> {
    if !matches!(mode, Mode::Directory { money: true, .. }) {
        return Err(Error::IncompleteCoverage);
    }
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let state = decode(&stored, mode)?;
    let mut ext = stored.checkpoint.restore()?;
    let result: Result<MoneyProjection, String> = read(
        &mut ext,
        bundles,
        &state.directory,
        origin,
        "financial_projection",
        (),
    )?;
    tx.rollback().await?;
    result.map_err(|_| Error::InvalidState)
}

pub(super) async fn financial_projection(
    pool: &SqlitePool,
    bundles: &Bundles,
    mode: Mode,
    origin: AccountId32,
) -> Result<FinancialProjection, Error> {
    if !matches!(mode, Mode::Directory { money: true, .. }) {
        return Err(Error::IncompleteCoverage);
    }
    let mut tx = pool.begin().await?;
    let stored = load(&mut tx, bundles.identity(), mode).await?;
    let state = decode(&stored, mode)?;
    let mut ext = stored.checkpoint.restore()?;
    let marketplace = read_marketplace(&mut ext, bundles, &state, origin)?;
    let money: Result<MoneyProjection, String> = read(
        &mut ext,
        bundles,
        &state.directory,
        origin,
        "financial_projection",
        (),
    )?;
    let money = money.map_err(|_| Error::InvalidState)?;
    let registry: Vec<(EntityId, EntityId)> =
        read(&mut ext, bundles, &state.directory, origin, "disputes", ())?;
    let mut cases = Vec::new();
    for (id, project_id) in registry {
        let address: Result<Account, String> = read(
            &mut ext,
            bundles,
            &state.directory,
            origin,
            "resolve_project",
            project_id,
        )?;
        let address = address.map_err(|_| Error::InvalidState)?;
        let record: Result<
            (
                generated_contracts::Dispute,
                generated_contracts::MilestoneView,
            ),
            String,
        > = read(&mut ext, bundles, &address, origin, "dispute", id)?;
        let record = record.map_err(|_| Error::InvalidState)?;
        if record.0.dispute_id != id || record.0.project_id != project_id {
            return Err(Error::InvalidState);
        }
        cases.push(record);
    }
    tx.rollback().await?;
    Ok((marketplace, money, cases))
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
            "invalid_review_state",
            "invalid_dispute_response",
            "invalid_deposit_state",
            "invalid_withdrawal_state",
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
        let overflow = Result::<Result<(), String>, u8>::Ok(Err("score_overflow".into())).encode();
        assert!(matches!(
            transition(&outcome(Status::Reverted, overflow)),
            Ok(Err("score_overflow"))
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
